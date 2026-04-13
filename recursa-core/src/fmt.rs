//! Pretty-printing IR tokens and Wadler-style printer engine.

use std::marker::PhantomData;

/// A token in the pretty-printing intermediate representation.
#[derive(Debug, Clone)]
pub enum Token {
    /// Literal text (keyword, identifier, operator, punctuation).
    String(String),
    /// A potential line break. If the group fits on one line, `flat` is used.
    /// Otherwise, `broken` is used (typically a newline).
    Break { flat: String, broken: String },
    /// Begin a group of tokens.
    Begin(GroupKind),
    /// End the current group.
    End,
    /// Increase indentation level.
    Indent,
    /// Decrease indentation level.
    Dedent,
}

/// How breaks within a group behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// All breaks in this group either all break or all stay flat.
    Consistent,
    /// Each break independently decides whether to break.
    Inconsistent,
}

// -- TokenText trait --

/// Trait for token types with a fixed textual representation.
/// Implemented by keyword and punctuation unit structs via their macros.
pub trait TokenText {
    const TEXT: &'static str;
}

/// Strip trailing `\b` (word boundary) from a keyword pattern at compile time.
/// `r"SELECT\b"` → `"SELECT"`.
pub const fn strip_word_boundary(s: &str) -> &str {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len >= 2 && bytes[len - 2] == b'\\' && bytes[len - 1] == b'b' {
        // SAFETY: removing valid ASCII suffix from valid UTF-8 string.
        // We use from_raw_parts to avoid the const-indexing limitation.
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(bytes.as_ptr(), len - 2)) }
    } else {
        s
    }
}

// -- FormatTokens trait --

/// Emit pretty-printer tokens for an AST node.
pub trait FormatTokens {
    fn format_tokens(&self, tokens: &mut Vec<Token>);
}

// -- Blanket impls --

impl<T: TokenText> FormatTokens for PhantomData<T> {
    fn format_tokens(&self, tokens: &mut Vec<Token>) {
        tokens.push(Token::String(T::TEXT.to_string()));
    }
}

impl FormatTokens for String {
    fn format_tokens(&self, tokens: &mut Vec<Token>) {
        tokens.push(Token::String(self.clone()));
    }
}

impl<T: FormatTokens> FormatTokens for Option<T> {
    fn format_tokens(&self, tokens: &mut Vec<Token>) {
        if let Some(inner) = self {
            inner.format_tokens(tokens);
        }
    }
}

impl<T: FormatTokens> FormatTokens for Box<T> {
    fn format_tokens(&self, tokens: &mut Vec<Token>) {
        (**self).format_tokens(tokens);
    }
}

impl<T: FormatTokens> FormatTokens for Vec<T> {
    fn format_tokens(&self, tokens: &mut Vec<Token>) {
        for item in self {
            item.format_tokens(tokens);
        }
    }
}

impl FormatTokens for () {
    fn format_tokens(&self, _tokens: &mut Vec<Token>) {}
}

/// Runtime formatting style configuration.
#[derive(Debug, Clone)]
pub struct FormatStyle {
    /// Maximum line width before breaking.
    pub max_width: usize,
    /// Number of spaces per indentation level.
    pub indent_width: usize,
    /// Whether to use uppercase keywords (SELECT vs select).
    pub uppercase_keywords: bool,
    /// Whether commas lead the next line (true) or trail the current line (false).
    pub leading_commas: bool,
}

impl Default for FormatStyle {
    fn default() -> Self {
        Self {
            max_width: 80,
            indent_width: 4,
            uppercase_keywords: true,
            leading_commas: false,
        }
    }
}

/// Wadler-style pretty-printing engine.
///
/// Consumes a stream of IR tokens and produces formatted text,
/// deciding where to break lines based on group sizes and max width.
pub struct PrintEngine {
    style: FormatStyle,
    output: String,
    /// Current column position.
    column: usize,
    /// Current indentation level (number of indent steps).
    indent_level: usize,
    /// Stack of active groups with their starting column and kind.
    groups: Vec<GroupState>,
}

struct GroupState {
    _kind: GroupKind,
    /// Whether this group has decided to break.
    broken: bool,
}

impl PrintEngine {
    pub fn new(style: FormatStyle) -> Self {
        Self {
            style,
            output: String::new(),
            column: 0,
            indent_level: 0,
            groups: Vec::new(),
        }
    }

    /// Process a stream of tokens and return the formatted output.
    pub fn print(mut self, tokens: &[Token]) -> String {
        // First pass: compute group sizes to decide breaking.
        let break_decisions = compute_breaks(tokens, self.style.max_width);

        let mut group_idx = 0;
        let mut needs_space = false;
        for token in tokens {
            match token {
                Token::String(s) => {
                    if needs_space && !is_attached_punct(s) {
                        self.output.push(' ');
                        self.column += 1;
                    }
                    self.output.push_str(s);
                    self.column += s.len();
                    needs_space = !is_opening_punct(s);
                }
                Token::Break { flat, broken } => {
                    let should_break = self
                        .groups
                        .last()
                        .map(|g| g.broken)
                        .unwrap_or(false);

                    if should_break {
                        self.output.push_str(broken);
                        if broken.contains('\n') {
                            self.column = 0;
                            self.emit_indent();
                        } else {
                            self.column += broken.len();
                        }
                    } else {
                        self.output.push_str(flat);
                        self.column += flat.len();
                    }
                    needs_space = false;
                }
                Token::Begin(kind) => {
                    let broken = if group_idx < break_decisions.len() {
                        break_decisions[group_idx]
                    } else {
                        false
                    };
                    group_idx += 1;
                    self.groups.push(GroupState {
                        _kind: *kind,
                        broken,
                    });
                }
                Token::End => {
                    self.groups.pop();
                }
                Token::Indent => {
                    self.indent_level += 1;
                }
                Token::Dedent => {
                    if self.indent_level > 0 {
                        self.indent_level -= 1;
                    }
                }
            }
        }

        self.output
    }

    fn emit_indent(&mut self) {
        let spaces = self.indent_level * self.style.indent_width;
        for _ in 0..spaces {
            self.output.push(' ');
        }
        self.column = spaces;
    }
}

/// First pass: compute which groups need to break.
///
/// A group breaks if its total flat size exceeds the remaining
/// space on the current line. For Consistent groups, all breaks
/// break together. For Inconsistent groups, each break is independent.
fn compute_breaks(tokens: &[Token], max_width: usize) -> Vec<bool> {
    let mut decisions = Vec::new();
    let mut group_sizes: Vec<usize> = Vec::new();

    // Measure flat size of each group
    let mut current_sizes: Vec<usize> = Vec::new(); // stack of running sizes

    for token in tokens {
        match token {
            Token::String(s) => {
                for size in &mut current_sizes {
                    *size += s.len();
                }
            }
            Token::Break { flat, .. } => {
                for size in &mut current_sizes {
                    *size += flat.len();
                }
            }
            Token::Begin(_) => {
                current_sizes.push(0);
            }
            Token::End => {
                if let Some(size) = current_sizes.pop() {
                    group_sizes.push(size);
                    // Add this group's size to parent
                    if let Some(parent) = current_sizes.last_mut() {
                        *parent += size;
                    }
                }
            }
            Token::Indent | Token::Dedent => {}
        }
    }

    // Now decide: a group breaks if its flat size > max_width
    // (simplified — a full implementation would track column position)
    for size in &group_sizes {
        decisions.push(*size > max_width);
    }

    decisions
}

/// Punctuation that attaches to the preceding token (no space before).
fn is_attached_punct(s: &str) -> bool {
    matches!(s, ";" | "," | ")" | "]" | "." | "::")
}

/// Punctuation that attaches to the following token (no space after).
fn is_opening_punct(s: &str) -> bool {
    matches!(s, "(" | "[" | "." | "::")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_string() {
        let tokens = vec![Token::String("hello".into())];
        let engine = PrintEngine::new(FormatStyle::default());
        assert_eq!(engine.print(&tokens), "hello");
    }

    #[test]
    fn group_fits_on_line() {
        let tokens = vec![
            Token::Begin(GroupKind::Consistent),
            Token::String("SELECT".into()),
            Token::Break {
                flat: " ".into(),
                broken: "\n".into(),
            },
            Token::String("a, b, c".into()),
            Token::End,
        ];
        let engine = PrintEngine::new(FormatStyle::default());
        assert_eq!(engine.print(&tokens), "SELECT a, b, c");
    }

    #[test]
    fn group_breaks_when_too_wide() {
        let tokens = vec![
            Token::Begin(GroupKind::Consistent),
            Token::String("SELECT".into()),
            Token::Break {
                flat: " ".into(),
                broken: "\n".into(),
            },
            Token::String("a_very_long_column_name, another_long_column_name, yet_another_really_long_column_name_that_pushes_over".into()),
            Token::End,
        ];
        let engine = PrintEngine::new(FormatStyle { max_width: 40, ..Default::default() });
        let result = engine.print(&tokens);
        assert!(result.contains('\n'), "expected line break but got: {result}");
    }

    #[test]
    fn indent_after_break() {
        let tokens = vec![
            Token::Begin(GroupKind::Consistent),
            Token::String("SELECT".into()),
            Token::Indent,
            Token::Break {
                flat: " ".into(),
                broken: "\n".into(),
            },
            Token::String("a".into()),
            Token::Dedent,
            Token::End,
        ];
        let engine = PrintEngine::new(FormatStyle {
            max_width: 5, // force break
            ..Default::default()
        });
        let result = engine.print(&tokens);
        assert_eq!(result, "SELECT\n    a");
    }
}
