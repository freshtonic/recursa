//! Facade-owned structured rendering contracts.

use std::fmt;
use std::ops::Range;

use crate::Span;
use crate::parsed::ViewCursor;

#[derive(Clone)]
/// Captured text and child structure available to one Pretty occurrence.
pub(crate) struct PrettyProvenance {
    /// Exact matched text for the active occurrence.
    text: Option<String>,
    /// Matched source span for source-map output.
    span: Option<Span>,
    /// Child provenance in generated element/occurrence order.
    children: Box<[PrettyProvenance]>,
    /// Separator text and spans kept outside semantic child ordinals.
    repetition_separators: Box<[RepetitionSeparatorProvenance]>,
}

impl PrettyProvenance {
    /// Constructs validated provenance copied from an immutable parsed sidecar.
    pub(crate) fn new(
        text: Option<String>,
        span: Option<Span>,
        children: Box<[Self]>,
        repetition_separators: Box<[RepetitionSeparatorProvenance]>,
    ) -> Self {
        Self {
            text,
            span,
            children,
            repetition_separators,
        }
    }
}

#[derive(Clone)]
/// Exact parsed spelling and span of one repetition separator.
pub(crate) struct RepetitionSeparatorProvenance {
    /// Matched separator text.
    pub(crate) text: String,
    /// Matched separator source range.
    pub(crate) span: Span,
}

/// Width and indentation used by the structured Pretty renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrettyConfig {
    /// Preferred maximum rendered line width.
    max_width: usize,
    /// Spaces introduced by one indentation level.
    indent: usize,
}

impl PrettyConfig {
    /// Creates caller-selected width and indentation settings.
    pub const fn new(max_width: usize, indent: usize) -> Self {
        Self { max_width, indent }
    }

    /// Returns the preferred maximum rendered line width.
    pub const fn max_width(self) -> usize {
        self.max_width
    }

    /// Returns the number of spaces per indentation level.
    pub const fn indent(self) -> usize {
        self.indent
    }
}

impl Default for PrettyConfig {
    fn default() -> Self {
        Self::new(88, 2)
    }
}

/// One rendered token's byte range and its originating input span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMapEntry {
    /// Half-open rendered byte range.
    output: Range<usize>,
    /// Originating parsed source range.
    source: Span,
}

impl SourceMapEntry {
    /// Returns the rendered byte range for this token mapping.
    pub fn output(&self) -> Range<usize> {
        self.output.clone()
    }

    /// Returns the originating parsed source span.
    pub const fn source(&self) -> Span {
        self.source
    }
}

/// Token-granular provenance for one Pretty rendering.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceMap {
    /// Token mappings in rendered output order.
    entries: Box<[SourceMapEntry]>,
}

impl SourceMap {
    /// Returns all token mappings in rendered output order.
    pub fn entries(&self) -> &[SourceMapEntry] {
        &self.entries
    }

    /// Returns the source span for the rendered token containing `offset`.
    pub fn source_at(&self, offset: usize) -> Option<Span> {
        self.entries
            .iter()
            .find(|entry| entry.output.contains(&offset))
            .map(SourceMapEntry::source)
    }
}

/// Structured rendering instruction queued before layout is resolved.
#[derive(Clone, Debug)]
enum PrettyEvent {
    /// Token text with its parsed or synthetic source location.
    Text {
        /// Text to append exactly.
        text: String,
        /// Originating span used for source mapping.
        source: Span,
    },
    /// Whitespace rendered flat or as a newline according to group mode.
    SoftBreak(String),
    /// One space between two tokens that layout never breaks.
    Space,
    /// Separator-aware break between repeated semantic values.
    RepetitionBreak {
        /// Parsed or canonical separator spelling.
        separator: Option<MappedText>,
        /// Whether a broken separator begins the next line.
        leading: bool,
        /// Whether this boundary always breaks.
        hard: bool,
        /// Whether flat layout inserts space before the separator.
        space_before_separator: bool,
        /// Whether flat layout inserts space after the separator.
        space_after_separator: bool,
    },
    /// Unconditional newline.
    HardBreak,
    /// Begin a group whose soft breaks are consistently flat or broken.
    BeginGroup,
    /// Begin a group whose breaks are decided independently.
    BeginInconsistentGroup,
    /// End the innermost group.
    EndGroup,
    /// Increase indentation after subsequent line breaks.
    Indent,
    /// Decrease indentation after subsequent line breaks.
    Dedent,
}

#[derive(Clone, Debug)]
/// Rendered text paired with one source-map span.
struct MappedText {
    /// Text emitted into the output.
    text: String,
    /// Source range correlated with the emitted text.
    source: Span,
}

/// Facade-owned structured document sink used by [`PrettyContext`].
#[derive(Clone, Debug)]
pub struct PrettySink {
    /// Layout constraints fixed for this rendering.
    config: PrettyConfig,
    /// Structured instructions awaiting layout resolution.
    events: Vec<PrettyEvent>,
    /// The grammar's test for two adjacent texts that its lexer would join or split in another
    /// way; layout writes one space between such texts.
    lexical_boundary: Option<fn(&str, &str) -> bool>,
}

impl PrettySink {
    /// Creates an empty sink using caller-selected layout settings.
    pub fn new(config: PrettyConfig) -> Self {
        Self {
            config,
            events: Vec::new(),
            lexical_boundary: None,
        }
    }

    /// Resolves events into rendered text without retaining its source map.
    fn finish(self) -> String {
        self.finish_mapped().0
    }

    /// Resolves groups and breaks into text plus monotonic token mappings.
    fn finish_mapped(self) -> (String, SourceMap) {
        // Scan the queued layout program once. Group decisions depend only on
        // the remaining flat width, while indentation is updated by explicit
        // balanced events emitted by generated Pretty implementations.
        let mut output = String::new();
        let mut source_map = Vec::new();
        let mut column = 0_usize;
        let mut indent = 0_usize;
        let mut groups = Vec::<GroupMode>::new();
        // The last nonempty text written and whether nothing separates it from the next text,
        // so that two texts the lexer would join get one space between them.
        let mut boundary = LexicalBoundary::new(self.lexical_boundary);
        for (index, event) in self.events.iter().enumerate() {
            match event {
                PrettyEvent::Text { text, source } => {
                    if boundary.guard(text) {
                        output.push(' ');
                        column = column.saturating_add(1);
                    }
                    let start = output.len();
                    output.push_str(text);
                    if !text.is_empty() {
                        source_map.push(SourceMapEntry {
                            output: start..output.len(),
                            source: *source,
                        });
                    }
                    column = column_after(column, text);
                    boundary.wrote(text);
                }
                PrettyEvent::SoftBreak(flat) => {
                    let broken = match groups.last().copied() {
                        Some(GroupMode::Consistent(broken)) => broken,
                        Some(GroupMode::Inconsistent) => {
                            column
                                .saturating_add(flat.chars().count())
                                .saturating_add(flat_width_to_next_break(&self.events, index + 1))
                                > self.config.max_width
                        }
                        None => false,
                    };
                    if broken {
                        newline(&mut output, &mut column, indent, self.config.indent);
                        boundary.separate();
                    } else {
                        output.push_str(flat);
                        column = column_after(column, flat);
                        if !flat.is_empty() {
                            boundary.separate();
                        }
                    }
                }
                PrettyEvent::HardBreak => {
                    newline(&mut output, &mut column, indent, self.config.indent);
                    boundary.separate();
                }
                PrettyEvent::Space => {
                    output.push(' ');
                    column = column.saturating_add(1);
                    boundary.separate();
                }
                PrettyEvent::RepetitionBreak {
                    separator,
                    leading,
                    hard,
                    space_before_separator,
                    space_after_separator,
                } => {
                    let boundary_can_break =
                        separator.is_none() || *space_before_separator || *space_after_separator;
                    let broken = boundary_can_break
                        && (*hard
                            || match groups.last().copied() {
                                Some(GroupMode::Consistent(broken)) => broken,
                                Some(GroupMode::Inconsistent) => {
                                    column
                                        .saturating_add(usize::from(*space_before_separator))
                                        .saturating_add(usize::from(*space_after_separator))
                                        .saturating_add(
                                            separator
                                                .as_ref()
                                                .map_or(0, |mapped| mapped.text.chars().count()),
                                        )
                                        .saturating_add(flat_width_to_next_break(
                                            &self.events,
                                            index + 1,
                                        ))
                                        > self.config.max_width
                                }
                                None => false,
                            });
                    if broken {
                        if !leading && let Some(separator) = separator {
                            // A requested space, or a space the lexer needs, comes first.
                            if *space_before_separator || boundary.guard(&separator.text) {
                                output.push(' ');
                                column = column.saturating_add(1);
                            }
                            let start = output.len();
                            output.push_str(&separator.text);
                            source_map.push(SourceMapEntry {
                                output: start..output.len(),
                                source: separator.source,
                            });
                            column = column_after(column, &separator.text);
                        }
                        newline(&mut output, &mut column, indent, self.config.indent);
                        boundary.separate();
                        if *leading && let Some(separator) = separator {
                            let start = output.len();
                            output.push_str(&separator.text);
                            source_map.push(SourceMapEntry {
                                output: start..output.len(),
                                source: separator.source,
                            });
                            column = column_after(column, &separator.text);
                            boundary.wrote(&separator.text);
                            if *space_after_separator {
                                output.push(' ');
                                column = column.saturating_add(1);
                                boundary.separate();
                            }
                        }
                    } else {
                        if *space_before_separator {
                            output.push(' ');
                            column = column.saturating_add(1);
                            boundary.separate();
                        }
                        if let Some(separator) = separator {
                            if boundary.guard(&separator.text) {
                                output.push(' ');
                                column = column.saturating_add(1);
                            }
                            let start = output.len();
                            output.push_str(&separator.text);
                            source_map.push(SourceMapEntry {
                                output: start..output.len(),
                                source: separator.source,
                            });
                            column = column_after(column, &separator.text);
                            boundary.wrote(&separator.text);
                        }
                        if *space_after_separator {
                            output.push(' ');
                            column = column.saturating_add(1);
                            boundary.separate();
                        }
                    }
                }
                PrettyEvent::BeginGroup => {
                    let flat = flat_width(&self.events, index + 1);
                    groups.push(GroupMode::Consistent(flat.is_none_or(|width| {
                        column.saturating_add(width) > self.config.max_width
                    })));
                }
                PrettyEvent::BeginInconsistentGroup => groups.push(GroupMode::Inconsistent),
                PrettyEvent::EndGroup => {
                    groups.pop();
                }
                PrettyEvent::Indent => indent = indent.saturating_add(1),
                PrettyEvent::Dedent => indent = indent.saturating_sub(1),
            }
        }
        (
            output,
            SourceMap {
                entries: source_map.into_boxed_slice(),
            },
        )
    }
}

/// The text before the next boundary that layout writes, for the lexical guard.
struct LexicalBoundary<'events> {
    /// The grammar's test for two adjacent texts that its lexer would not keep apart.
    fuses: Option<fn(&str, &str) -> bool>,
    /// The last nonempty text written, while nothing has been written after it.
    last: Option<&'events str>,
    /// Answers already computed for one pair of texts in this rendering.
    cache: std::collections::HashMap<(&'events str, &'events str), bool>,
}

impl<'events> LexicalBoundary<'events> {
    /// Creates the guard of one rendering.
    fn new(fuses: Option<fn(&str, &str) -> bool>) -> Self {
        Self {
            fuses,
            last: None,
            cache: std::collections::HashMap::new(),
        }
    }

    /// Reports whether `next`, written now, needs one space before it so that the lexer keeps
    /// it apart from the last text.
    fn guard(&mut self, next: &'events str) -> bool {
        let (Some(fuses), Some(last)) = (self.fuses, self.last) else {
            return false;
        };
        if next.is_empty() || next.starts_with(char::is_whitespace) {
            return false;
        }
        *self
            .cache
            .entry((last, next))
            .or_insert_with(|| fuses(last, next))
    }

    /// Records that `text` was written.
    fn wrote(&mut self, text: &'events str) {
        if text.ends_with(char::is_whitespace) {
            self.last = None;
        } else if !text.is_empty() {
            self.last = Some(text);
        }
    }

    /// Records that whitespace was written, so the next text starts a new boundary.
    fn separate(&mut self) {
        self.last = None;
    }
}

/// Combines two declared-precedence bounds of one operand edge into the stricter one.
///
/// A bound is a level and whether an operand at exactly that level is parenthesized. The higher
/// level is stricter; on equal levels a tie parenthesizes when either bound says so. Generated
/// `Pretty` code uses this to join the bound of the enclosing rule with the bound of the token
/// that the parser sees after the operand (`docs/declared-precedence.md`, "Consumers").
#[doc(hidden)]
pub fn stricter_bound(
    left: Option<(u32, bool)>,
    right: Option<(u32, bool)>,
) -> Option<(u32, bool)> {
    match (left, right) {
        (Some(left), Some(right)) if left.0 > right.0 => Some(left),
        (Some(left), Some(right)) if right.0 > left.0 => Some(right),
        (Some(left), Some(right)) => Some((left.0, left.1 || right.1)),
        (bound, None) | (None, bound) => bound,
    }
}

/// Computes the display column after appending possibly multiline text.
fn column_after(column: usize, text: &str) -> usize {
    text.rsplit_once('\n').map_or_else(
        || column.saturating_add(text.chars().count()),
        |(_, tail)| tail.chars().count(),
    )
}

/// Emits one newline and the active indentation, updating the display column.
fn newline(output: &mut String, column: &mut usize, indent: usize, width: usize) {
    output.push('\n');
    let spaces = indent.saturating_mul(width);
    output.extend(std::iter::repeat_n(' ', spaces));
    *column = spaces;
}

/// Measures a consistent group's flat width, rejecting hard or embedded breaks.
fn flat_width(events: &[PrettyEvent], start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut width = 0_usize;
    for event in &events[start..] {
        match event {
            PrettyEvent::Text { text, .. } | PrettyEvent::SoftBreak(text) => {
                if text.contains('\n') {
                    return None;
                }
                width = width.saturating_add(text.chars().count());
            }
            PrettyEvent::Space => width = width.saturating_add(1),
            PrettyEvent::HardBreak => return None,
            PrettyEvent::RepetitionBreak {
                separator,
                hard,
                space_before_separator,
                space_after_separator,
                ..
            } => {
                if *hard {
                    return None;
                }
                width = width.saturating_add(
                    separator
                        .as_ref()
                        .map_or(0, |mapped| mapped.text.chars().count())
                        + usize::from(*space_before_separator)
                        + usize::from(*space_after_separator),
                );
            }
            PrettyEvent::BeginGroup | PrettyEvent::BeginInconsistentGroup => depth += 1,
            PrettyEvent::EndGroup if depth == 0 => return Some(width),
            PrettyEvent::EndGroup => depth -= 1,
            PrettyEvent::Indent | PrettyEvent::Dedent => {}
        }
    }
    Some(width)
}

/// Layout behavior selected for the innermost active group.
#[derive(Clone, Copy)]
enum GroupMode {
    /// Every soft break shares this precomputed broken decision.
    Consistent(bool),
    /// Each soft break tests the next flat segment independently.
    Inconsistent,
}

/// Measures the flat segment up to the next peer-level break or group end.
fn flat_width_to_next_break(events: &[PrettyEvent], start: usize) -> usize {
    let mut depth = 0_usize;
    let mut width = 0_usize;
    for event in &events[start..] {
        match event {
            PrettyEvent::Text { text, .. } => {
                width = width.saturating_add(text.chars().count());
            }
            PrettyEvent::SoftBreak(_) | PrettyEvent::RepetitionBreak { .. } if depth == 0 => break,
            PrettyEvent::SoftBreak(text) => {
                width = width.saturating_add(text.chars().count());
            }
            PrettyEvent::RepetitionBreak {
                separator,
                hard,
                space_before_separator,
                space_after_separator,
                ..
            } => {
                if *hard {
                    break;
                }
                width = width.saturating_add(
                    separator
                        .as_ref()
                        .map_or(0, |mapped| mapped.text.chars().count())
                        + usize::from(*space_before_separator)
                        + usize::from(*space_after_separator),
                );
            }
            PrettyEvent::Space => width = width.saturating_add(1),
            PrettyEvent::HardBreak => break,
            PrettyEvent::BeginGroup | PrettyEvent::BeginInconsistentGroup => depth += 1,
            PrettyEvent::EndGroup if depth == 0 => break,
            PrettyEvent::EndGroup => depth -= 1,
            PrettyEvent::Indent | PrettyEvent::Dedent => {}
        }
    }
    width
}

/// Rendering state shared by generated and source-owned [`Pretty`] impls.
pub struct PrettyContext {
    /// Structured output sink and layout configuration.
    sink: PrettySink,
    /// Whether the previous semantic token normally requires separation.
    needs_separator: bool,
    /// Whether an explicit break already supplies that separation.
    separator_supplied: bool,
    /// Pending repetition boundary that may absorb the next item's spacing request.
    pending_repetition_break: Option<usize>,
    /// Whether the previous token was automatic punctuation.
    previous_auto_punctuation: bool,
    /// Whether the previous token opened a delimited group or joins tightly to the next token, so
    /// that a token with space before it stays tight after it.
    previous_tight: bool,
    /// Whether generated policy requires spacing after the previous token.
    requested_space_after: bool,
    /// Whether any nonempty token has been emitted.
    has_emitted_token: bool,
    /// Parsed occurrence currently supplying matched text and spans.
    provenance: Option<PrettyProvenance>,
    /// Whether the caller overrode grammar-default layout settings.
    caller_configured: bool,
    /// Active generated repetition policy.
    repetition: Option<RepetitionLayout>,
    /// Nested speculative layout state for optional/present grammar shapes.
    layout_checkpoints: Vec<PrettyLayoutCheckpoint>,
    /// The event index just after each explicit group that is open, innermost last.
    explicit_groups: Vec<usize>,
    /// Whether only hinted breaks can break: the space between two tokens is then never a break.
    hinted_breaks: bool,
}

/// A generated Pretty scope that restores its enclosing context when dropped.
///
/// This is public only so generated code can hold the scope across calls into
/// user-authored [`Pretty`] implementations. Its representation is private.
#[doc(hidden)]
pub struct PrettyScope<'a> {
    context: &'a mut PrettyContext,
    restore: PrettyScopeRestore,
}

/// Enclosing Pretty state saved across one generated scope.
enum PrettyScopeRestore {
    /// Occurrence provenance replaced while rendering one generated child.
    Provenance(Option<PrettyProvenance>),
    /// Repetition layout replaced while rendering one generated sequence.
    Repetition(Option<RepetitionLayout>),
}

impl std::ops::Deref for PrettyScope<'_> {
    type Target = PrettyContext;

    fn deref(&self) -> &Self::Target {
        self.context
    }
}

impl std::ops::DerefMut for PrettyScope<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.context
    }
}

impl Drop for PrettyScope<'_> {
    fn drop(&mut self) {
        match std::mem::replace(&mut self.restore, PrettyScopeRestore::Provenance(None)) {
            PrettyScopeRestore::Provenance(previous) => self.context.provenance = previous,
            PrettyScopeRestore::Repetition(previous) => self.context.repetition = previous,
        }
    }
}

#[derive(Clone)]
/// Restorable rendering state around one optional grammar layout.
struct PrettyLayoutCheckpoint {
    /// Whether the optional shape emitted any token.
    present: bool,
    /// Event length to which an absent shape rolls back.
    event_len: usize,
    /// Saved automatic separator state.
    needs_separator: bool,
    /// Saved explicit separator state.
    separator_supplied: bool,
    /// Saved pending repetition boundary.
    pending_repetition_break: Option<usize>,
    /// Saved automatic punctuation state.
    previous_auto_punctuation: bool,
    /// Saved trailing-space request.
    requested_space_after: bool,
    /// Saved nonempty-output state.
    has_emitted_token: bool,
    /// Saved repetition policy.
    repetition: Option<RepetitionLayout>,
    /// Saved number of open explicit groups.
    explicit_groups: usize,
}

#[derive(Clone)]
/// Generated separator and line-breaking policy for one repetition.
struct RepetitionLayout {
    /// Canonical separator spelling, if any.
    separator: Option<String>,
    /// Whether the grammar permits/emits a trailing separator.
    trailing: bool,
    /// Whether every item boundary must break.
    hard_break: bool,
    /// Whether broken separators lead the following line.
    leading_separator: bool,
    /// Whether flat layout inserts space before each separator.
    space_before_separator: bool,
    /// Whether flat layout inserts space after each separator.
    space_after_separator: bool,
}

impl Default for PrettyContext {
    fn default() -> Self {
        Self {
            sink: PrettySink::new(PrettyConfig::default()),
            needs_separator: false,
            separator_supplied: false,
            pending_repetition_break: None,
            previous_auto_punctuation: false,
            previous_tight: false,
            requested_space_after: false,
            has_emitted_token: false,
            provenance: None,
            caller_configured: false,
            repetition: None,
            layout_checkpoints: Vec::new(),
            explicit_groups: Vec::new(),
            hinted_breaks: false,
        }
    }
}

impl PrettyContext {
    /// Creates a rendering context that accepts grammar-default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a rendering context whose caller settings override defaults.
    pub fn with_config(config: PrettyConfig) -> Self {
        Self {
            sink: PrettySink::new(config),
            needs_separator: false,
            separator_supplied: false,
            pending_repetition_break: None,
            previous_auto_punctuation: false,
            previous_tight: false,
            requested_space_after: false,
            has_emitted_token: false,
            provenance: None,
            caller_configured: true,
            repetition: None,
            layout_checkpoints: Vec::new(),
            explicit_groups: Vec::new(),
            hinted_breaks: false,
        }
    }

    /// Applies one grammar's defaults unless the caller supplied overrides.
    #[doc(hidden)]
    pub fn configure_defaults(&mut self, max_width: usize, indent: usize) {
        if !self.caller_configured {
            self.sink.config = PrettyConfig::new(max_width, indent);
        }
    }

    /// Selects the grammar's test for two adjacent texts that its lexer would join or split in
    /// another way. The first grammar entry that renders sets it; later entries keep it.
    ///
    /// Layout then writes one space between two such texts, so the rendering lexes as the
    /// tokens it was built from.
    #[doc(hidden)]
    pub fn configure_lexical_boundary(&mut self, fuses: fn(&str, &str) -> bool) {
        if self.sink.lexical_boundary.is_none() {
            self.sink.lexical_boundary = Some(fuses);
        }
    }

    /// Selects the grammar's `breaks = hinted` setting: only the breaks that the grammar hints,
    /// repetition boundaries and delimited sequences can break, and the space between two tokens
    /// is never a break. The first grammar entry that renders sets it; later entries keep it.
    #[doc(hidden)]
    pub fn configure_hinted_breaks(&mut self) {
        if !self.has_emitted_token && self.sink.events.is_empty() {
            self.hinted_breaks = true;
        }
    }

    /// Queues the space that separates two tokens: a soft break, or a space that never breaks
    /// when the grammar hints its breaks.
    fn push_token_space(&mut self) {
        let event = if self.hinted_breaks {
            PrettyEvent::Space
        } else {
            PrettyEvent::SoftBreak(" ".to_owned())
        };
        self.sink.events.push(event);
    }

    /// Emits one semantic token using the default separating-space policy.
    pub fn token(&mut self, text: &str) -> fmt::Result {
        if (self.needs_separator || self.requested_space_after)
            && !self.separator_supplied
            && !text.is_empty()
        {
            self.push_token_space();
        }
        self.tight_token(text)
    }

    /// Emits a token without adding stylistic whitespace.
    pub fn tight_token(&mut self, text: &str) -> fmt::Result {
        self.push_text(text, self.active_source_span());
        self.needs_separator = !text.is_empty();
        self.separator_supplied = false;
        self.previous_auto_punctuation = false;
        self.requested_space_after = false;
        self.has_emitted_token |= !text.is_empty();
        Ok(())
    }

    /// Emits punctuation tightly unless adjacent automatic punctuation needs
    /// a conservative lexer boundary.
    #[doc(hidden)]
    pub fn punctuation_token(&mut self, text: &str) -> fmt::Result {
        if (self.previous_auto_punctuation || self.requested_space_after)
            && !self.separator_supplied
            && !text.is_empty()
        {
            self.push_token_space();
        }
        self.push_text(text, self.active_source_span());
        self.needs_separator = !text.is_empty();
        self.separator_supplied = false;
        self.previous_auto_punctuation = !text.is_empty();
        self.requested_space_after = false;
        self.has_emitted_token |= !text.is_empty();
        Ok(())
    }

    /// Returns the active parsed span or the source-free synthetic sentinel.
    fn active_source_span(&self) -> Span {
        self.provenance
            .as_ref()
            .and_then(|provenance| provenance.span)
            .unwrap_or(Span::SYNTHETIC)
    }

    /// Queues exact token text with its source-map origin.
    fn push_text(&mut self, text: &str, source: Span) {
        self.sink.events.push(PrettyEvent::Text {
            text: text.to_owned(),
            source,
        });
        if !text.is_empty() {
            self.pending_repetition_break = None;
            self.previous_tight = false;
        }
    }

    /// Emits an opening delimiter tightly and suppresses spacing after it.
    #[doc(hidden)]
    pub fn opening_token(&mut self, text: &str) -> fmt::Result {
        if (self.previous_auto_punctuation || self.requested_space_after)
            && !self.separator_supplied
            && !text.is_empty()
        {
            self.push_token_space();
        }
        self.tight_token(text)?;
        self.needs_separator = false;
        self.previous_tight = !text.is_empty();
        Ok(())
    }

    /// Emits a grammar-known closing delimiter tightly and suppresses
    /// conservative punctuation spacing after it.
    #[doc(hidden)]
    pub fn closing_token(&mut self, text: &str) -> fmt::Result {
        // A flat break just inside a closing delimiter writes no space.
        self.tighten_adjacent_separator();
        self.tight_token(text)?;
        self.previous_auto_punctuation = false;
        Ok(())
    }

    /// Marks the preceding punctuation token as a grammar-known closing delimiter.
    #[doc(hidden)]
    pub fn close_delimiter(&mut self) {
        self.previous_auto_punctuation = false;
    }

    /// Makes the space or flat break just before the next token write nothing, because that
    /// token is tight on its left.
    fn tighten_adjacent_separator(&mut self) {
        if !self.separator_supplied {
            return;
        }
        match self.adjacent_separator() {
            Some(index) if matches!(self.sink.events[index], PrettyEvent::Space) => {
                // Replaced, not removed, so that the indices other state holds stay valid.
                self.sink.events[index] = PrettyEvent::Text {
                    text: String::new(),
                    source: Span::SYNTHETIC,
                };
                self.separator_supplied = false;
            }
            Some(index) => {
                if let PrettyEvent::SoftBreak(flat) = &mut self.sink.events[index] {
                    flat.clear();
                }
            }
            None => {}
        }
    }

    /// Emits a grammar-validated token tightly on both sides.
    #[doc(hidden)]
    pub fn tight_sequence_token(&mut self, text: &str) -> fmt::Result {
        self.tighten_adjacent_separator();
        if self.requested_space_after && !self.separator_supplied && !text.is_empty() {
            self.push_token_space();
        }
        self.tight_token(text)?;
        self.needs_separator = false;
        self.previous_tight = !text.is_empty();
        Ok(())
    }

    /// Emits a grammar-validated token with space before and tight after.
    #[doc(hidden)]
    pub fn before_token(&mut self, text: &str) -> fmt::Result {
        if self.has_emitted_token && !self.previous_tight && !text.is_empty() {
            if let Some(index) = self.pending_repetition_break
                && let Some(PrettyEvent::RepetitionBreak {
                    space_after_separator,
                    ..
                }) = self.sink.events.get_mut(index)
            {
                *space_after_separator = true;
            } else if self.separator_supplied {
                if let Some(PrettyEvent::SoftBreak(flat)) = self.sink.events.last_mut()
                    && flat.is_empty()
                {
                    flat.push(' ');
                }
            } else {
                self.push_token_space();
            }
        }
        self.tight_token(text)?;
        self.needs_separator = false;
        Ok(())
    }

    /// Emits a grammar-validated token tight before and with space after.
    #[doc(hidden)]
    pub fn after_token(&mut self, text: &str) -> fmt::Result {
        if self.requested_space_after && !self.separator_supplied && !text.is_empty() {
            self.push_token_space();
        }
        self.tight_token(text)?;
        self.requested_space_after = !text.is_empty();
        Ok(())
    }

    /// Emits a grammar-validated token with requested space on both sides.
    #[doc(hidden)]
    pub fn around_token(&mut self, text: &str) -> fmt::Result {
        self.before_token(text)?;
        self.needs_separator = !text.is_empty();
        self.requested_space_after = !text.is_empty();
        Ok(())
    }

    /// Emits an exact captured source fragment without normalization.
    pub fn exact(&mut self, text: &str) -> fmt::Result {
        self.tight_token(text)
    }

    /// Adds one space between two tokens that layout never breaks.
    pub fn space(&mut self) -> fmt::Result {
        if !self.has_emitted_token {
            return Ok(());
        }
        if self.separator_supplied {
            // A break or a space is already there; a break that is flat without a space gets one.
            if let Some(index) = self.adjacent_separator()
                && let PrettyEvent::SoftBreak(existing) = &mut self.sink.events[index]
                && existing.is_empty()
            {
                existing.push(' ');
            }
            return Ok(());
        }
        self.sink.events.push(PrettyEvent::Space);
        self.separator_supplied = true;
        self.pending_repetition_break = None;
        self.previous_auto_punctuation = false;
        self.requested_space_after = false;
        Ok(())
    }

    /// Writes the space that a generated rule puts between two of its parts: a soft break, or a
    /// space that never breaks when the grammar hints its breaks.
    #[doc(hidden)]
    pub fn separating_space(&mut self) -> fmt::Result {
        if self.previous_tight {
            return Ok(());
        }
        if self.hinted_breaks {
            self.space()
        } else {
            self.soft_break(" ")
        }
    }

    /// Finds the break or space that separates the last token from what comes next, looking past
    /// the group and indentation boundaries queued after it.
    fn adjacent_separator(&self) -> Option<usize> {
        for (index, event) in self.sink.events.iter().enumerate().rev() {
            match event {
                PrettyEvent::SoftBreak(_) | PrettyEvent::HardBreak | PrettyEvent::Space => {
                    return Some(index);
                }
                PrettyEvent::BeginGroup
                | PrettyEvent::BeginInconsistentGroup
                | PrettyEvent::EndGroup
                | PrettyEvent::Indent
                | PrettyEvent::Dedent => {}
                PrettyEvent::Text { .. } | PrettyEvent::RepetitionBreak { .. } => return None,
            }
        }
        None
    }

    /// Adds a legal width-dependent break with `flat` text when the group fits.
    ///
    /// A break before the first token does nothing, so a rendering never starts with a line
    /// break. Two breaks with no token between them are one break: the first one, which a
    /// space becomes.
    pub fn soft_break(&mut self, flat: &str) -> fmt::Result {
        if !self.has_emitted_token {
            return Ok(());
        }
        let flat = if self.requested_space_after && flat.is_empty() {
            " "
        } else if self.previous_tight {
            // A flat break just inside an opening delimiter writes no space.
            ""
        } else {
            flat
        };
        let adjacent = if self.separator_supplied {
            self.adjacent_separator()
        } else {
            None
        };
        match adjacent.map(|index| (index, &mut self.sink.events[index])) {
            Some((_, PrettyEvent::SoftBreak(existing))) => {
                if existing.is_empty() && !flat.is_empty() {
                    *existing = flat.to_owned();
                }
            }
            Some((_, PrettyEvent::HardBreak)) => {}
            Some((_, event @ PrettyEvent::Space)) => {
                *event = PrettyEvent::SoftBreak(" ".to_owned());
            }
            Some(_) | None => self
                .sink
                .events
                .push(PrettyEvent::SoftBreak(flat.to_owned())),
        }
        self.separator_supplied = true;
        self.pending_repetition_break = None;
        self.previous_auto_punctuation = false;
        self.requested_space_after = false;
        Ok(())
    }

    /// Adds an unconditional line break.
    pub fn hard_break(&mut self) -> fmt::Result {
        let adjacent = if self.separator_supplied {
            self.adjacent_separator()
        } else {
            None
        };
        match adjacent.map(|index| &mut self.sink.events[index]) {
            Some(event @ (PrettyEvent::SoftBreak(_) | PrettyEvent::Space)) => {
                *event = PrettyEvent::HardBreak;
            }
            Some(_) | None => self.sink.events.push(PrettyEvent::HardBreak),
        }
        self.separator_supplied = true;
        self.pending_repetition_break = None;
        self.previous_auto_punctuation = false;
        self.requested_space_after = false;
        Ok(())
    }

    /// Emits one consistent fit-or-break group.
    pub fn group<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.begin_consistent_group();
        let result = render(self);
        self.end_group();
        result
    }

    /// Applies the default Node group without hiding a surrounding local group.
    ///
    /// When the grammar hints its breaks, the Node opens no group of its own only when a local
    /// group has just opened around it, so that the local group decides the Node's breaks. Only
    /// indentation may come between the two. A Node that starts later inside a local group, after
    /// other output, gets its own group. Otherwise no Node inside a local group opens a group, so
    /// that the local group decides every space between two tokens in it.
    #[doc(hidden)]
    pub fn baseline_group<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        let wrapped = if self.hinted_breaks {
            self.explicit_groups.last().is_some_and(|&start| {
                self.sink.events[start..]
                    .iter()
                    .all(|event| matches!(event, PrettyEvent::Indent))
            })
        } else {
            !self.explicit_groups.is_empty()
        };
        if wrapped {
            return render(self);
        }
        self.sink.events.push(PrettyEvent::BeginGroup);
        let result = render(self);
        self.sink.events.push(PrettyEvent::EndGroup);
        result
    }

    /// Emits a fill group whose soft-break opportunities decide independently.
    #[doc(hidden)]
    pub fn inconsistent_group<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.begin_inconsistent_group();
        let result = render(self);
        self.end_group();
        result
    }

    #[doc(hidden)]
    /// Begins a consistent group for generated multi-part layouts.
    pub fn begin_consistent_group(&mut self) {
        self.sink.events.push(PrettyEvent::BeginGroup);
        self.explicit_groups.push(self.sink.events.len());
    }

    #[doc(hidden)]
    /// Begins an inconsistent group for independently breaking layouts.
    pub fn begin_inconsistent_group(&mut self) {
        self.sink.events.push(PrettyEvent::BeginInconsistentGroup);
        self.explicit_groups.push(self.sink.events.len());
    }

    #[doc(hidden)]
    /// Ends the innermost explicit generated group.
    pub fn end_group(&mut self) {
        self.explicit_groups.pop();
        self.sink.events.push(PrettyEvent::EndGroup);
    }

    #[doc(hidden)]
    /// Begins one generated indentation scope.
    pub fn begin_indent(&mut self) {
        self.sink.events.push(PrettyEvent::Indent);
    }

    #[doc(hidden)]
    /// Ends one generated indentation scope.
    pub fn end_indent(&mut self) {
        self.sink.events.push(PrettyEvent::Dedent);
    }

    /// Starts one attachment layout that may be discarded as a unit.
    #[doc(hidden)]
    pub fn begin_present_layout(&mut self, present: bool) {
        self.layout_checkpoints.push(PrettyLayoutCheckpoint {
            present,
            event_len: self.sink.events.len(),
            needs_separator: self.needs_separator,
            separator_supplied: self.separator_supplied,
            pending_repetition_break: self.pending_repetition_break,
            previous_auto_punctuation: self.previous_auto_punctuation,
            requested_space_after: self.requested_space_after,
            has_emitted_token: self.has_emitted_token,
            repetition: self.repetition.clone(),
            explicit_groups: self.explicit_groups.len(),
        });
    }

    /// Ends one attachment layout, restoring the prior sink state when absent.
    #[doc(hidden)]
    pub fn end_present_layout(&mut self) {
        let checkpoint = self
            .layout_checkpoints
            .pop()
            .expect("generated Pretty layout checkpoints are balanced");
        if checkpoint.present {
            return;
        }
        self.sink.events.truncate(checkpoint.event_len);
        self.needs_separator = checkpoint.needs_separator;
        self.separator_supplied = checkpoint.separator_supplied;
        self.pending_repetition_break = checkpoint.pending_repetition_break;
        self.previous_auto_punctuation = checkpoint.previous_auto_punctuation;
        self.requested_space_after = checkpoint.requested_space_after;
        self.has_emitted_token = checkpoint.has_emitted_token;
        self.repetition = checkpoint.repetition;
        self.explicit_groups.truncate(checkpoint.explicit_groups);
    }

    /// Emits a document region one configured indentation level deeper.
    pub fn indented<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.sink.events.push(PrettyEvent::Indent);
        let result = render(self);
        self.sink.events.push(PrettyEvent::Dedent);
        result
    }

    /// Enters one generated repetition policy without monomorphizing a
    /// callback at every generated call site.
    #[doc(hidden)]
    pub fn repetition_scope_style(
        &mut self,
        separator: Option<&str>,
        trailing: bool,
        hard_break: bool,
        leading_separator: bool,
        separator_spacing: (bool, bool),
    ) -> PrettyScope<'_> {
        let (space_before_separator, space_after_separator) = separator_spacing;
        let previous = self.repetition.replace(RepetitionLayout {
            separator: separator.map(str::to_owned),
            trailing,
            hard_break,
            leading_separator,
            space_before_separator,
            space_after_separator,
        });
        PrettyScope {
            context: self,
            restore: PrettyScopeRestore::Repetition(previous),
        }
    }

    /// Applies one generated repetition's separator policy to its container.
    #[doc(hidden)]
    pub fn with_repetition<R>(
        &mut self,
        separator: Option<&str>,
        trailing: bool,
        hard_break: bool,
        render: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.repetition.replace(RepetitionLayout {
            separator: separator.map(str::to_owned),
            trailing,
            hard_break,
            leading_separator: false,
            space_before_separator: false,
            space_after_separator: true,
        });
        let result = render(self);
        self.repetition = previous;
        result
    }

    #[doc(hidden)]
    /// Applies the complete generated separator and line-breaking policy.
    pub fn with_repetition_style<R>(
        &mut self,
        separator: Option<&str>,
        trailing: bool,
        hard_break: bool,
        leading_separator: bool,
        separator_spacing: (bool, bool),
        render: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let (space_before_separator, space_after_separator) = separator_spacing;
        let previous = self.repetition.replace(RepetitionLayout {
            separator: separator.map(str::to_owned),
            trailing,
            hard_break,
            leading_separator,
            space_before_separator,
            space_after_separator,
        });
        let result = render(self);
        self.repetition = previous;
        result
    }

    /// Renders ordinary repeated values under the active repetition policy.
    fn render_repeated<'a, T: Pretty + 'a>(
        &mut self,
        values: impl ExactSizeIterator<Item = &'a T>,
    ) -> fmt::Result {
        self.render_repeated_by(values, |context, value| value.pretty(context))
    }

    /// Renders a repetition whose individual values have grammar-attached syntax.
    #[doc(hidden)]
    pub fn render_repeated_by<'a, T: 'a>(
        &mut self,
        values: impl ExactSizeIterator<Item = &'a T>,
        mut render: impl FnMut(&mut Self, &'a T) -> fmt::Result,
    ) -> fmt::Result {
        let layout = self.repetition.take();
        let length = values.len();
        for (index, value) in values.enumerate() {
            self.with_child(index, |context| render(context, value))?;
            if let Some(layout) = &layout
                && (index + 1 < length || layout.trailing)
            {
                let separator = self.repetition_separator(index, layout);
                if index + 1 < length {
                    let space_before_separator =
                        layout.space_before_separator || self.requested_space_after;
                    self.requested_space_after = false;
                    self.previous_auto_punctuation = false;
                    let event_index = self.sink.events.len();
                    self.sink.events.push(PrettyEvent::RepetitionBreak {
                        separator,
                        leading: layout.leading_separator,
                        hard: layout.hard_break,
                        space_before_separator,
                        space_after_separator: layout.space_after_separator,
                    });
                    self.separator_supplied = true;
                    self.pending_repetition_break = Some(event_index);
                } else if let Some(separator) = separator {
                    self.trailing_repetition_separator(&separator, layout);
                }
            }
        }
        self.repetition = layout;
        Ok(())
    }

    /// Emits one final repetition separator while preserving its outer boundary policy.
    fn trailing_repetition_separator(&mut self, separator: &MappedText, layout: &RepetitionLayout) {
        if (layout.space_before_separator || self.requested_space_after)
            && !self.separator_supplied
            && self.has_emitted_token
        {
            self.push_token_space();
        }
        self.push_text(&separator.text, separator.source);
        self.needs_separator = false;
        self.separator_supplied = false;
        self.previous_auto_punctuation = false;
        self.requested_space_after = layout.space_after_separator;
        self.has_emitted_token |= !separator.text.is_empty();
    }

    /// Selects parsed separator text when available, otherwise canonical text.
    fn repetition_separator(&self, index: usize, layout: &RepetitionLayout) -> Option<MappedText> {
        let fallback = layout.separator.as_ref()?;
        Some(
            self.provenance
                .as_ref()
                .and_then(|provenance| provenance.repetition_separators.get(index))
                .map_or_else(
                    || MappedText {
                        text: fallback.clone(),
                        source: Span::SYNTHETIC,
                    },
                    |separator| MappedText {
                        text: separator.text.clone(),
                        source: separator.span,
                    },
                ),
        )
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

    /// Enters one source-ordered child occurrence without a generated
    /// callback. Dropping the returned scope restores the parent occurrence.
    #[doc(hidden)]
    pub fn child_scope(&mut self, index: usize) -> PrettyScope<'_> {
        let provenance = self
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.children.get(index))
            .cloned();
        let previous = std::mem::replace(&mut self.provenance, provenance);
        PrettyScope {
            context: self,
            restore: PrettyScopeRestore::Provenance(previous),
        }
    }

    /// Runs rendering at a source-ordered child whose concrete closed shape was
    /// opaque at the generic declaration boundary.
    #[doc(hidden)]
    pub fn with_opaque_child<R>(&mut self, index: usize, render: impl FnOnce(&mut Self) -> R) -> R {
        self.with_child(index, render)
    }

    /// Enters a source-ordered child whose concrete closed shape was opaque at
    /// the generic declaration boundary.
    #[doc(hidden)]
    pub fn opaque_child_scope(&mut self, index: usize) -> PrettyScope<'_> {
        self.child_scope(index)
    }

    /// Runs structured rendering without assuming a source occurrence shape.
    #[doc(hidden)]
    pub fn without_provenance<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.with_provenance(None, render)
    }

    /// Enters generated rendering without a source occurrence.
    #[doc(hidden)]
    pub fn without_provenance_scope(&mut self) -> PrettyScope<'_> {
        let previous = self.provenance.take();
        PrettyScope {
            context: self,
            restore: PrettyScopeRestore::Provenance(previous),
        }
    }

    /// Returns the exact matched text at the active occurrence, when captured.
    #[doc(hidden)]
    pub fn matched_text(&self) -> Option<String> {
        self.provenance
            .as_ref()
            .and_then(|provenance| provenance.text.as_deref())
            .map(str::to_owned)
    }

    /// Reports whether the active generated occurrence has a concrete source span.
    #[doc(hidden)]
    pub fn has_source_span(&self) -> bool {
        self.provenance
            .as_ref()
            .and_then(|provenance| provenance.span)
            .is_some()
    }

    /// Temporarily replaces active provenance for one nested render operation.
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

    /// Consumes the context and resolves its structured events into text.
    pub fn finish(self) -> String {
        self.sink.finish()
    }

    /// Consumes the context and returns rendered text plus token provenance.
    pub fn finish_mapped(self) -> (String, SourceMap) {
        self.sink.finish_mapped()
    }
}

/// Structured rendering shared by semantic ASTs, parsed roots, and views.
pub trait Pretty {
    /// Emits this value into a structured rendering context.
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result;

    /// Renders with grammar-selected default layout.
    fn render(&self) -> String {
        let mut context = PrettyContext::new();
        self.pretty(&mut context)
            .expect("rendering into a structured sink cannot fail");
        context.finish()
    }

    /// Renders with caller-selected width and indentation.
    fn render_with(&self, config: PrettyConfig) -> String {
        let mut context = PrettyContext::with_config(config);
        self.pretty(&mut context)
            .expect("rendering into a structured sink cannot fail");
        context.finish()
    }

    /// Renders with default layout and returns token-level source provenance.
    fn render_mapped(&self) -> (String, SourceMap) {
        let mut context = PrettyContext::new();
        self.pretty(&mut context)
            .expect("rendering into a structured sink cannot fail");
        context.finish_mapped()
    }

    /// Renders with caller layout and returns token-level source provenance.
    fn render_mapped_with(&self, config: PrettyConfig) -> (String, SourceMap) {
        let mut context = PrettyContext::with_config(config);
        self.pretty(&mut context)
            .expect("rendering into a structured sink cannot fail");
        context.finish_mapped()
    }
}

impl<T: Pretty + ?Sized> Pretty for Box<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        (**self).pretty(context)
    }
}

impl<T: Pretty + ?Sized> Pretty for bumpalo::boxed::Box<'_, T> {
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
        context.render_repeated(self.iter())
    }
}

impl<T: Pretty> Pretty for bumpalo::collections::Vec<'_, T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        context.render_repeated(self.iter())
    }
}

impl<T: Pretty> Pretty for crate::Vec1<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        context.render_repeated(self.iter())
    }
}

impl<T: Pretty> Pretty for crate::ArenaVec1<'_, T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        context.render_repeated(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::{Pretty, PrettyConfig, PrettyContext, PrettyProvenance};
    use std::fmt;

    #[test]
    fn source_free_tokens_are_mapped_as_synthetic() {
        struct Synthetic;

        impl Pretty for Synthetic {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.token("alpha")
            }
        }

        let (rendered, source_map) = Synthetic.render_mapped();
        assert_eq!(rendered, "alpha");
        let [entry] = source_map.entries() else {
            panic!("one token produces one mapping")
        };
        assert_eq!(entry.output(), 0..5);
        assert!(entry.source().is_synthetic());
        assert_eq!(source_map.source_at(2), Some(crate::Span::SYNTHETIC));
        assert_eq!(source_map.source_at(5), None);
    }

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
    fn inconsistent_groups_break_only_the_overflowing_opportunity() {
        struct Words(bool);
        impl Pretty for Words {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                let render = |context: &mut PrettyContext| {
                    context.token("alpha")?;
                    context.soft_break(" ")?;
                    context.token("beta")?;
                    context.soft_break(" ")?;
                    context.token("c")
                };
                if self.0 {
                    context.group(render)
                } else {
                    context.inconsistent_group(render)
                }
            }
        }

        let config = PrettyConfig::new(10, 2);
        assert_eq!(Words(true).render_with(config), "alpha\nbeta\nc");
        assert_eq!(Words(false).render_with(config), "alpha beta\nc");
    }

    #[test]
    fn indivisible_lexemes_and_mandatory_tight_sequences_may_exceed_width() {
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

        struct TightSequence;
        impl Pretty for TightSequence {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.group(|context| {
                    context.tight_sequence_token("a")?;
                    context.tight_sequence_token("::")?;
                    context.tight_sequence_token("b")
                })
            }
        }
        assert_eq!(TightSequence.render_with(PrettyConfig::new(1, 2)), "a::b");
    }

    #[test]
    fn repeated_values_emit_trailing_separators_and_structural_item_breaks() {
        struct Word(&'static str);
        impl Pretty for Word {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.tight_token(self.0)
            }
        }
        struct Words(Vec<Word>);
        impl Pretty for Words {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.group(|context| {
                    context
                        .with_repetition(Some(","), false, false, |context| self.0.pretty(context))
                })
            }
        }

        let words = Words(vec![Word("alpha"), Word("beta"), Word("gamma")]);
        assert_eq!(words.render(), "alpha, beta, gamma");
        assert_eq!(
            words.render_with(PrettyConfig::new(12, 2)),
            "alpha,\nbeta,\ngamma"
        );
    }

    #[test]
    fn semantic_repetition_breaks_do_not_depend_on_separator_spelling() {
        struct Word(&'static str);
        impl Pretty for Word {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.tight_token(self.0)
            }
        }
        struct Statements(Vec<Word>);
        impl Pretty for Statements {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.with_repetition(Some("GO"), false, true, |context| self.0.pretty(context))
            }
        }
        struct Dispatch(Vec<Word>);
        impl Pretty for Dispatch {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.with_repetition(None, false, true, |context| self.0.pretty(context))
            }
        }

        assert_eq!(
            Statements(vec![Word("one"), Word("two")]).render(),
            "oneGO\ntwo"
        );
        assert_eq!(
            Dispatch(vec![Word("one"), Word("two")]).render(),
            "one\ntwo"
        );
    }

    #[test]
    fn generated_scopes_match_callback_layout_and_restore_on_error() {
        fn render_with_callback(config: PrettyConfig) -> String {
            let mut context = PrettyContext::with_config(config);
            context
                .with_repetition_style(Some(","), false, false, false, (false, true), |context| {
                    context.render_repeated_by(["alpha", "beta"].iter(), |context, word| {
                        context.tight_token(word)
                    })
                })
                .unwrap();
            context.finish()
        }

        fn render_with_scope(config: PrettyConfig) -> String {
            let mut context = PrettyContext::with_config(config);
            {
                let mut scope =
                    context.repetition_scope_style(Some(","), false, false, false, (false, true));
                scope
                    .render_repeated_by(["alpha", "beta"].iter(), |context, word| {
                        context.tight_token(word)
                    })
                    .unwrap();
            }
            context.finish()
        }

        for config in [PrettyConfig::new(88, 2), PrettyConfig::new(6, 2)] {
            assert_eq!(render_with_scope(config), render_with_callback(config));
        }

        let child =
            PrettyProvenance::new(Some("child".to_owned()), None, Box::new([]), Box::new([]));
        let mut context = PrettyContext::new();
        context.provenance = Some(PrettyProvenance::new(
            Some("parent".to_owned()),
            None,
            Box::new([child]),
            Box::new([]),
        ));
        let failed = (|| -> fmt::Result {
            let scope = context.child_scope(0);
            assert_eq!(scope.matched_text().as_deref(), Some("child"));
            Err(fmt::Error)?;
            Ok(())
        })();
        assert!(failed.is_err());
        assert_eq!(context.matched_text().as_deref(), Some("parent"));
    }

    #[test]
    fn automatic_punctuation_preserves_a_conservative_lexer_boundary() {
        struct AdjacentPunctuation;
        impl Pretty for AdjacentPunctuation {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.punctuation_token(":")?;
                context.punctuation_token(":")
            }
        }
        assert_eq!(AdjacentPunctuation.render(), ": :");
    }

    #[test]
    fn a_lexical_boundary_separates_only_adjacent_texts_that_the_lexer_joins() {
        fn joins(left: &str, right: &str) -> bool {
            left.ends_with('@') && right.starts_with('@')
        }
        struct Adjacent;
        impl Pretty for Adjacent {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.configure_lexical_boundary(joins);
                context.tight_token("@")?;
                context.tight_token("@")?;
                context.tight_token("x")?;
                context.tight_token("@")?;
                context.soft_break("")?;
                context.tight_token("@")?;
                context.soft_break(" ")?;
                context.tight_token("@")
            }
        }
        // The first two texts and the last two of the flat break meet and get a space. A text
        // after `x`, and a text after a written space, do not.
        assert_eq!(Adjacent.render(), "@ @x@ @ @");

        struct Broken;
        impl Pretty for Broken {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.configure_lexical_boundary(joins);
                context.group(|context| {
                    context.tight_token("@aaaaaaaaaa")?;
                    context.soft_break("")?;
                    context.tight_token("@")
                })
            }
        }
        // A broken soft break writes a newline, so the texts no longer meet.
        assert_eq!(
            Broken.render_with(PrettyConfig::new(4, 2)),
            "@aaaaaaaaaa\n@"
        );

        struct Unset;
        impl Pretty for Unset {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.tight_token("@")?;
                context.tight_token("@")
            }
        }
        // Without a grammar test, layout writes the texts as before.
        assert_eq!(Unset.render(), "@@");
    }

    #[test]
    fn the_stricter_bound_takes_the_higher_level_and_joins_ties() {
        assert_eq!(
            super::stricter_bound(Some((3, false)), Some((5, false))),
            Some((5, false))
        );
        assert_eq!(
            super::stricter_bound(Some((5, true)), Some((3, false))),
            Some((5, true))
        );
        assert_eq!(
            super::stricter_bound(Some((4, false)), Some((4, true))),
            Some((4, true))
        );
        assert_eq!(
            super::stricter_bound(None, Some((4, true))),
            Some((4, true))
        );
        assert_eq!(
            super::stricter_bound(Some((4, false)), None),
            Some((4, false))
        );
        assert_eq!(super::stricter_bound(None, None), None);
    }

    #[test]
    fn requested_spacing_is_an_absolute_floor_before_punctuation() {
        struct RequestedFloor;
        impl Pretty for RequestedFloor {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.around_token("IS")?;
                context.punctuation_token("!")?;
                context.after_token(":")?;
                context.punctuation_token("?")
            }
        }
        assert_eq!(RequestedFloor.render(), "IS !: ?");
    }

    #[test]
    fn requested_delimiter_spacing_survives_empty_layout_breaks() {
        struct Delimited;
        impl Pretty for Delimited {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.group(|context| {
                    context.after_token("[")?;
                    context.indented(|context| {
                        context.soft_break("")?;
                        context.token("content")
                    })?;
                    context.soft_break("")?;
                    context.before_token("]")
                })
            }
        }

        assert_eq!(Delimited.render(), "[ content ]");
        assert_eq!(
            Delimited.render_with(PrettyConfig::new(4, 2)),
            "[\n  content\n]"
        );
    }

    struct SeparatedWords {
        separator: &'static str,
        spacing: (bool, bool),
        leading: bool,
        trailing: bool,
        item_boundaries: bool,
        tail: Option<&'static str>,
    }

    impl Pretty for SeparatedWords {
        fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
            context.group(|context| {
                context.with_repetition_style(
                    Some(self.separator),
                    self.trailing,
                    false,
                    self.leading,
                    self.spacing,
                    |context| {
                        context.render_repeated_by(["alpha", "beta"].iter(), |context, word| {
                            if self.item_boundaries {
                                context.around_token(word)
                            } else {
                                context.tight_token(word)
                            }
                        })
                    },
                )?;
                if let Some(tail) = self.tail {
                    context.token(tail)?;
                }
                Ok(())
            })
        }
    }

    #[test]
    fn repetition_breaks_preserve_each_separator_spacing_side() {
        let config = PrettyConfig::new(8, 2);
        for (separator, spacing, flat, broken) in [
            ("@", (true, false), "alpha @beta", "alpha @\nbeta"),
            ("%", (false, true), "alpha% beta", "alpha%\nbeta"),
            ("&", (true, true), "alpha & beta", "alpha &\nbeta"),
            ("OR", (true, true), "alpha OR beta", "alpha OR\nbeta"),
        ] {
            let words = SeparatedWords {
                separator,
                spacing,
                leading: false,
                trailing: false,
                item_boundaries: false,
                tail: None,
            };
            assert_eq!(words.render(), flat);
            assert_eq!(words.render_with(config), broken);
        }

        let leading = SeparatedWords {
            separator: "&",
            spacing: (true, true),
            leading: true,
            trailing: false,
            item_boundaries: false,
            tail: None,
        };
        assert_eq!(leading.render_with(config), "alpha\n& beta");
    }

    #[test]
    fn repetition_spacing_combines_item_boundaries_and_survives_a_trailing_separator() {
        let bilateral = SeparatedWords {
            separator: ".",
            spacing: (false, false),
            leading: false,
            trailing: false,
            item_boundaries: true,
            tail: None,
        };
        assert_eq!(bilateral.render(), "alpha . beta");

        let trailing = SeparatedWords {
            separator: "&",
            spacing: (true, true),
            leading: false,
            trailing: true,
            item_boundaries: false,
            tail: Some("tail"),
        };
        assert_eq!(trailing.render(), "alpha & beta & tail");
    }

    #[test]
    fn tight_repetition_boundaries_are_indivisible_even_when_the_group_overflows() {
        for separator in [".", "("] {
            let words = SeparatedWords {
                separator,
                spacing: (false, false),
                leading: false,
                trailing: false,
                item_boundaries: false,
                tail: None,
            };
            assert_eq!(
                words.render_with(PrettyConfig::new(3, 2)),
                format!("alpha{separator}beta")
            );
        }
    }

    /// `SELECT` with an indented list whose break is inside its group, then a clause break that
    /// belongs to the enclosing group, as a SQL grammar renders a query.
    struct Query {
        hinted: bool,
        items: &'static [&'static str],
    }

    impl Pretty for Query {
        fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
            if self.hinted {
                context.configure_hinted_breaks();
            }
            context.baseline_group(|context| {
                context.token("SELECT")?;
                context.group(|context| {
                    context.indented(|context| {
                        context.soft_break(" ")?;
                        context.with_repetition(Some(","), false, false, |context| {
                            context.render_repeated_by(self.items.iter(), |context, item| {
                                context.baseline_group(|context| {
                                    for word in item.split(' ') {
                                        context.token(word)?;
                                    }
                                    Ok(())
                                })
                            })
                        })
                    })
                })?;
                context.soft_break(" ")?;
                context.token("FROM")?;
                context.token("t")
            })
        }
    }

    #[test]
    fn hinted_breaks_never_break_the_space_between_two_tokens() {
        let items: &[&str] = &["a AS alpha", "b AS beta"];
        let config = PrettyConfig::new(16, 2);
        assert_eq!(
            Query {
                hinted: true,
                items
            }
            .render_with(config),
            "SELECT\n  a AS alpha,\n  b AS beta\nFROM t"
        );
        // By default the space between two tokens is a soft break of the group around it.
        assert_eq!(
            Query {
                hinted: false,
                items
            }
            .render_with(PrettyConfig::new(8, 2)),
            "SELECT\n  a\n  AS\n  alpha,\n  b\n  AS\n  beta\nFROM\nt"
        );
    }

    #[test]
    fn by_default_a_local_group_decides_every_space_between_two_tokens_in_it() {
        let items: &[&str] = &["a AS alpha", "b AS beta"];
        assert_eq!(
            Query {
                hinted: false,
                items
            }
            .render_with(PrettyConfig::new(16, 2)),
            "SELECT\n  a\n  AS\n  alpha,\n  b\n  AS\n  beta\nFROM\nt"
        );
    }

    #[test]
    fn with_hinted_breaks_a_node_inside_a_local_group_gets_its_own_group_after_other_output() {
        struct List;
        impl Pretty for List {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.configure_hinted_breaks();
                context.token("SELECT")?;
                context.group(|context| {
                    context.indented(|context| {
                        context.soft_break(" ")?;
                        context.with_repetition(Some(","), false, false, |context| {
                            context.render_repeated_by(["alpha", "beta"].iter(), |context, word| {
                                // An item with a break of its own, which its own group decides.
                                context.baseline_group(|context| {
                                    context.token("f")?;
                                    context.soft_break(" ")?;
                                    context.token(word)
                                })
                            })
                        })
                    })
                })
            }
        }
        assert_eq!(
            List.render_with(PrettyConfig::new(12, 2)),
            "SELECT\n  f alpha,\n  f beta"
        );
    }

    #[test]
    fn a_node_that_a_local_group_just_opened_around_has_no_group_of_its_own() {
        struct Words;
        impl Pretty for Words {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.baseline_group(|context| {
                    context.token("alpha")?;
                    context.soft_break(" ")?;
                    context.token("beta")?;
                    context.soft_break(" ")?;
                    context.token("c")
                })
            }
        }
        struct Fill;
        impl Pretty for Fill {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context
                    .inconsistent_group(|context| context.indented(|context| Words.pretty(context)))
            }
        }
        assert_eq!(
            Fill.render_with(PrettyConfig::new(10, 2)),
            "alpha beta\n  c"
        );
    }

    #[test]
    fn adjacent_breaks_are_one_break_and_a_first_break_is_none() {
        struct Nested;
        impl Pretty for Nested {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.configure_hinted_breaks();
                context.soft_break(" ")?;
                context.group(|context| {
                    context.opening_token("(")?;
                    context.indented(|context| {
                        context.soft_break("")?;
                        context.group(|context| {
                            context.soft_break(" ")?;
                            context.token("SELECT")?;
                            context.token("alpha")
                        })
                    })?;
                    context.soft_break(" ")?;
                    context.closing_token(")")
                })
            }
        }
        assert_eq!(Nested.render(), "(SELECT alpha)");
        assert_eq!(
            Nested.render_with(PrettyConfig::new(8, 2)),
            "(\n  SELECT alpha\n)"
        );
    }

    #[test]
    fn a_space_never_breaks_and_a_break_or_a_tight_token_replaces_it() {
        struct Spaced(bool);
        impl Pretty for Spaced {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.group(|context| {
                    context.token("INSERT")?;
                    context.token("t")?;
                    context.space()?;
                    if self.0 {
                        context.soft_break(" ")?;
                        context.token("VALUES")
                    } else {
                        context.tight_sequence_token("::")?;
                        context.token("int")
                    }
                })
            }
        }
        assert_eq!(Spaced(true).render(), "INSERT t VALUES");
        assert_eq!(
            Spaced(true).render_with(PrettyConfig::new(4, 2)),
            "INSERT\nt\nVALUES"
        );
        assert_eq!(Spaced(false).render(), "INSERT t::int");
    }

    #[test]
    fn a_word_with_space_before_it_is_tight_after_an_opening_delimiter() {
        struct Exists;
        impl Pretty for Exists {
            fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
                context.token("WHERE")?;
                context.opening_token("(")?;
                context.around_token("EXISTS")?;
                context.opening_token("(")?;
                context.token("x")?;
                context.closing_token(")")?;
                context.closing_token(")")
            }
        }
        assert_eq!(Exists.render(), "WHERE(EXISTS (x))");
    }
}
