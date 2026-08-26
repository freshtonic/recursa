//! Facade-owned structured rendering contracts.

use std::fmt;

use crate::parsed::ViewCursor;

#[derive(Clone)]
pub(crate) struct PrettyProvenance {
    text: Option<String>,
    children: Box<[PrettyProvenance]>,
}

impl PrettyProvenance {
    pub(crate) fn new(text: Option<String>, children: Box<[Self]>) -> Self {
        Self { text, children }
    }
}

/// Width and indentation used by the structured Pretty renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrettyConfig {
    max_width: usize,
    indent: usize,
}

impl PrettyConfig {
    pub const fn new(max_width: usize, indent: usize) -> Self {
        Self { max_width, indent }
    }

    pub const fn max_width(self) -> usize {
        self.max_width
    }

    pub const fn indent(self) -> usize {
        self.indent
    }
}

impl Default for PrettyConfig {
    fn default() -> Self {
        Self::new(88, 2)
    }
}

#[derive(Clone, Debug)]
enum PrettyEvent {
    Text(String),
    SoftBreak(String),
    HardBreak,
    BeginGroup,
    EndGroup,
    Indent,
    Dedent,
}

/// Facade-owned structured document sink used by [`PrettyContext`].
#[derive(Clone, Debug)]
pub struct PrettySink {
    config: PrettyConfig,
    events: Vec<PrettyEvent>,
}

impl PrettySink {
    pub fn new(config: PrettyConfig) -> Self {
        Self {
            config,
            events: Vec::new(),
        }
    }

    fn finish(self) -> String {
        let mut output = String::new();
        let mut column = 0_usize;
        let mut indent = 0_usize;
        let mut groups = Vec::new();
        for (index, event) in self.events.iter().enumerate() {
            match event {
                PrettyEvent::Text(text) => {
                    output.push_str(text);
                    column = column_after(column, text);
                }
                PrettyEvent::SoftBreak(flat) => {
                    if groups.last().copied().unwrap_or(false) {
                        newline(&mut output, &mut column, indent, self.config.indent);
                    } else {
                        output.push_str(flat);
                        column = column_after(column, flat);
                    }
                }
                PrettyEvent::HardBreak => {
                    newline(&mut output, &mut column, indent, self.config.indent);
                }
                PrettyEvent::BeginGroup => {
                    let flat = flat_width(&self.events, index + 1);
                    groups.push(
                        flat.is_none_or(|width| {
                            column.saturating_add(width) > self.config.max_width
                        }),
                    );
                }
                PrettyEvent::EndGroup => {
                    groups.pop();
                }
                PrettyEvent::Indent => indent = indent.saturating_add(1),
                PrettyEvent::Dedent => indent = indent.saturating_sub(1),
            }
        }
        output
    }
}

fn column_after(column: usize, text: &str) -> usize {
    text.rsplit_once('\n').map_or_else(
        || column.saturating_add(text.chars().count()),
        |(_, tail)| tail.chars().count(),
    )
}

fn newline(output: &mut String, column: &mut usize, indent: usize, width: usize) {
    output.push('\n');
    let spaces = indent.saturating_mul(width);
    output.extend(std::iter::repeat_n(' ', spaces));
    *column = spaces;
}

fn flat_width(events: &[PrettyEvent], start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut width = 0_usize;
    for event in &events[start..] {
        match event {
            PrettyEvent::Text(text) | PrettyEvent::SoftBreak(text) => {
                if text.contains('\n') {
                    return None;
                }
                width = width.saturating_add(text.chars().count());
            }
            PrettyEvent::HardBreak => return None,
            PrettyEvent::BeginGroup => depth += 1,
            PrettyEvent::EndGroup if depth == 0 => return Some(width),
            PrettyEvent::EndGroup => depth -= 1,
            PrettyEvent::Indent | PrettyEvent::Dedent => {}
        }
    }
    Some(width)
}

/// Rendering state shared by generated and source-owned [`Pretty`] impls.
pub struct PrettyContext {
    sink: PrettySink,
    needs_separator: bool,
    separator_supplied: bool,
    provenance: Option<PrettyProvenance>,
}

impl Default for PrettyContext {
    fn default() -> Self {
        Self::with_config(PrettyConfig::default())
    }
}

impl PrettyContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: PrettyConfig) -> Self {
        Self {
            sink: PrettySink::new(config),
            needs_separator: false,
            separator_supplied: false,
            provenance: None,
        }
    }

    /// Emits one semantic token using the default separating-space policy.
    pub fn token(&mut self, text: &str) -> fmt::Result {
        if self.needs_separator && !self.separator_supplied && !text.is_empty() {
            self.sink
                .events
                .push(PrettyEvent::SoftBreak(" ".to_owned()));
        }
        self.tight_token(text)
    }

    /// Emits a token without adding stylistic whitespace.
    pub fn tight_token(&mut self, text: &str) -> fmt::Result {
        self.sink.events.push(PrettyEvent::Text(text.to_owned()));
        self.needs_separator = !text.is_empty();
        self.separator_supplied = false;
        Ok(())
    }

    /// Emits an exact captured source fragment without normalization.
    pub fn exact(&mut self, text: &str) -> fmt::Result {
        self.tight_token(text)
    }

    /// Adds a legal width-dependent break with `flat` text when the group fits.
    pub fn soft_break(&mut self, flat: &str) -> fmt::Result {
        self.sink
            .events
            .push(PrettyEvent::SoftBreak(flat.to_owned()));
        self.separator_supplied = true;
        Ok(())
    }

    /// Adds an unconditional line break.
    pub fn hard_break(&mut self) -> fmt::Result {
        self.sink.events.push(PrettyEvent::HardBreak);
        self.separator_supplied = true;
        Ok(())
    }

    /// Emits one consistent fit-or-break group.
    pub fn group<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.sink.events.push(PrettyEvent::BeginGroup);
        let result = render(self);
        self.sink.events.push(PrettyEvent::EndGroup);
        result
    }

    /// Emits a document region one configured indentation level deeper.
    pub fn indented<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.sink.events.push(PrettyEvent::Indent);
        let result = render(self);
        self.sink.events.push(PrettyEvent::Dedent);
        result
    }

    /// Runs structured rendering with one captured occurrence as provenance.
    #[doc(hidden)]
    pub fn with_cursor<R>(
        &mut self,
        cursor: ViewCursor<'_>,
        render: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let provenance = cursor.pretty_provenance();
        self.with_provenance(provenance, render)
    }

    /// Runs structured rendering at one source-ordered child occurrence.
    #[doc(hidden)]
    pub fn with_child<R>(&mut self, index: usize, render: impl FnOnce(&mut Self) -> R) -> R {
        let provenance = self
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.children.get(index))
            .cloned();
        self.with_provenance(provenance, render)
    }

    /// Returns the exact matched text at the active occurrence, when captured.
    #[doc(hidden)]
    pub fn matched_text(&self) -> Option<String> {
        self.provenance
            .as_ref()
            .and_then(|provenance| provenance.text.as_deref())
            .map(str::to_owned)
    }

    fn with_provenance<R>(
        &mut self,
        provenance: Option<PrettyProvenance>,
        render: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.provenance, provenance);
        let result = render(self);
        self.provenance = previous;
        result
    }

    pub fn finish(self) -> String {
        self.sink.finish()
    }
}

/// Structured rendering shared by semantic ASTs, parsed roots, and views.
pub trait Pretty {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result;

    fn render(&self) -> String {
        self.render_with(PrettyConfig::default())
    }

    fn render_with(&self, config: PrettyConfig) -> String {
        let mut context = PrettyContext::with_config(config);
        self.pretty(&mut context)
            .expect("rendering into a structured sink cannot fail");
        context.finish()
    }
}

impl<T: Pretty + ?Sized> Pretty for Box<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        (**self).pretty(context)
    }
}

impl<T: Pretty> Pretty for Option<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        if let Some(value) = self {
            value.pretty(context)?;
        }
        Ok(())
    }
}

impl<T: Pretty> Pretty for Vec<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        for (index, value) in self.iter().enumerate() {
            context.with_child(index, |context| value.pretty(context))?;
        }
        Ok(())
    }
}

impl<T: Pretty> Pretty for crate::Vec1<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        for (index, value) in self.iter().enumerate() {
            context.with_child(index, |context| value.pretty(context))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Pretty, PrettyConfig, PrettyContext};
    use std::fmt;

    struct Delimited;

    impl Pretty for Delimited {
        fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
            context.group(|context| {
                context.tight_token("[")?;
                context.indented(|context| {
                    context.soft_break("")?;
                    context.token("alpha")?;
                    context.tight_token(",")?;
                    context.soft_break(" ")?;
                    context.tight_token("beta")?;
                    Ok(())
                })?;
                context.soft_break("")?;
                context.tight_token("]")
            })
        }
    }

    #[test]
    fn pretty_defaults_are_88_columns_and_two_spaces() {
        assert_eq!(PrettyConfig::default().max_width(), 88);
        assert_eq!(PrettyConfig::default().indent(), 2);
        assert_eq!(Delimited.render(), "[alpha, beta]");
    }

    #[test]
    fn caller_width_and_indent_overrides_drive_structural_breaks() {
        let rendered = Delimited.render_with(PrettyConfig::new(8, 3));
        assert_eq!(rendered, "[\n   alpha,\n   beta\n]");
    }

    #[test]
    fn indivisible_lexemes_are_the_only_default_width_exception() {
        struct LongLexeme;
        impl Pretty for LongLexeme {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.token("indivisible")
            }
        }
        assert_eq!(
            LongLexeme.render_with(PrettyConfig::new(3, 2)),
            "indivisible"
        );
    }
}
