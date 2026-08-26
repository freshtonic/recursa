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
    RepetitionBreak {
        separator: Option<String>,
        leading: bool,
    },
    HardBreak,
    BeginGroup,
    BeginInconsistentGroup,
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
        let mut groups = Vec::<GroupMode>::new();
        for (index, event) in self.events.iter().enumerate() {
            match event {
                PrettyEvent::Text(text) => {
                    output.push_str(text);
                    column = column_after(column, text);
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
                    } else {
                        output.push_str(flat);
                        column = column_after(column, flat);
                    }
                }
                PrettyEvent::HardBreak => {
                    newline(&mut output, &mut column, indent, self.config.indent);
                }
                PrettyEvent::RepetitionBreak { separator, leading } => {
                    let broken = match groups.last().copied() {
                        Some(GroupMode::Consistent(broken)) => broken,
                        Some(GroupMode::Inconsistent) => {
                            column
                                .saturating_add(1)
                                .saturating_add(
                                    separator.as_ref().map_or(0, |text| text.chars().count()),
                                )
                                .saturating_add(flat_width_to_next_break(&self.events, index + 1))
                                > self.config.max_width
                        }
                        None => false,
                    };
                    if broken {
                        if !leading && let Some(separator) = separator {
                            output.push_str(separator);
                            column = column_after(column, separator);
                        }
                        newline(&mut output, &mut column, indent, self.config.indent);
                        if *leading && let Some(separator) = separator {
                            output.push_str(separator);
                            output.push(' ');
                            column = column_after(column, separator).saturating_add(1);
                        }
                    } else {
                        if let Some(separator) = separator {
                            output.push_str(separator);
                            column = column_after(column, separator);
                        }
                        output.push(' ');
                        column = column.saturating_add(1);
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
            PrettyEvent::RepetitionBreak { separator, .. } => {
                width = width
                    .saturating_add(separator.as_ref().map_or(0, |text| text.chars().count()) + 1);
            }
            PrettyEvent::BeginGroup | PrettyEvent::BeginInconsistentGroup => depth += 1,
            PrettyEvent::EndGroup if depth == 0 => return Some(width),
            PrettyEvent::EndGroup => depth -= 1,
            PrettyEvent::Indent | PrettyEvent::Dedent => {}
        }
    }
    Some(width)
}

#[derive(Clone, Copy)]
enum GroupMode {
    Consistent(bool),
    Inconsistent,
}

fn flat_width_to_next_break(events: &[PrettyEvent], start: usize) -> usize {
    let mut depth = 0_usize;
    let mut width = 0_usize;
    for event in &events[start..] {
        match event {
            PrettyEvent::Text(text) => width = width.saturating_add(text.chars().count()),
            PrettyEvent::SoftBreak(_) | PrettyEvent::RepetitionBreak { .. } if depth == 0 => break,
            PrettyEvent::SoftBreak(text) => {
                width = width.saturating_add(text.chars().count());
            }
            PrettyEvent::RepetitionBreak { separator, .. } => {
                width = width
                    .saturating_add(separator.as_ref().map_or(0, |text| text.chars().count()) + 1);
            }
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
    sink: PrettySink,
    needs_separator: bool,
    separator_supplied: bool,
    previous_auto_punctuation: bool,
    requested_space_after: bool,
    has_emitted_token: bool,
    provenance: Option<PrettyProvenance>,
    caller_configured: bool,
    repetition: Option<RepetitionLayout>,
}

#[derive(Clone)]
struct RepetitionLayout {
    separator: Option<String>,
    trailing: bool,
    hard_break: bool,
    leading_separator: bool,
}

impl Default for PrettyContext {
    fn default() -> Self {
        Self {
            sink: PrettySink::new(PrettyConfig::default()),
            needs_separator: false,
            separator_supplied: false,
            previous_auto_punctuation: false,
            requested_space_after: false,
            has_emitted_token: false,
            provenance: None,
            caller_configured: false,
            repetition: None,
        }
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
            previous_auto_punctuation: false,
            requested_space_after: false,
            has_emitted_token: false,
            provenance: None,
            caller_configured: true,
            repetition: None,
        }
    }

    /// Applies one grammar's defaults unless the caller supplied overrides.
    #[doc(hidden)]
    pub fn configure_defaults(&mut self, max_width: usize, indent: usize) {
        if !self.caller_configured {
            self.sink.config = PrettyConfig::new(max_width, indent);
        }
    }

    /// Emits one semantic token using the default separating-space policy.
    pub fn token(&mut self, text: &str) -> fmt::Result {
        if (self.needs_separator || self.requested_space_after)
            && !self.separator_supplied
            && !text.is_empty()
        {
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
            self.sink
                .events
                .push(PrettyEvent::SoftBreak(" ".to_owned()));
        }
        self.sink.events.push(PrettyEvent::Text(text.to_owned()));
        self.needs_separator = !text.is_empty();
        self.separator_supplied = false;
        self.previous_auto_punctuation = !text.is_empty();
        self.requested_space_after = false;
        self.has_emitted_token |= !text.is_empty();
        Ok(())
    }

    /// Emits an opening delimiter tightly and suppresses spacing after it.
    #[doc(hidden)]
    pub fn opening_token(&mut self, text: &str) -> fmt::Result {
        if (self.previous_auto_punctuation || self.requested_space_after)
            && !self.separator_supplied
            && !text.is_empty()
        {
            self.sink
                .events
                .push(PrettyEvent::SoftBreak(" ".to_owned()));
        }
        self.tight_token(text)?;
        self.needs_separator = false;
        Ok(())
    }

    /// Marks the preceding punctuation token as a grammar-known closing delimiter.
    #[doc(hidden)]
    pub fn close_delimiter(&mut self) {
        self.previous_auto_punctuation = false;
    }

    /// Emits a grammar-validated token tightly on both sides.
    #[doc(hidden)]
    pub fn tight_sequence_token(&mut self, text: &str) -> fmt::Result {
        if self.requested_space_after && !self.separator_supplied && !text.is_empty() {
            self.sink
                .events
                .push(PrettyEvent::SoftBreak(" ".to_owned()));
        }
        self.tight_token(text)?;
        self.needs_separator = false;
        Ok(())
    }

    /// Emits a grammar-validated token with space before and tight after.
    #[doc(hidden)]
    pub fn before_token(&mut self, text: &str) -> fmt::Result {
        if self.has_emitted_token && !text.is_empty() {
            if self.separator_supplied {
                if let Some(PrettyEvent::SoftBreak(flat)) = self.sink.events.last_mut()
                    && flat.is_empty()
                {
                    flat.push(' ');
                }
            } else {
                self.sink
                    .events
                    .push(PrettyEvent::SoftBreak(" ".to_owned()));
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
            self.sink
                .events
                .push(PrettyEvent::SoftBreak(" ".to_owned()));
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

    /// Adds a legal width-dependent break with `flat` text when the group fits.
    pub fn soft_break(&mut self, flat: &str) -> fmt::Result {
        let flat = if self.requested_space_after && flat.is_empty() {
            " "
        } else {
            flat
        };
        if self.separator_supplied
            && let Some(PrettyEvent::SoftBreak(existing)) = self.sink.events.last_mut()
        {
            if existing.is_empty() && !flat.is_empty() {
                *existing = flat.to_owned();
            }
        } else {
            self.sink
                .events
                .push(PrettyEvent::SoftBreak(flat.to_owned()));
        }
        self.separator_supplied = true;
        self.previous_auto_punctuation = false;
        self.requested_space_after = false;
        Ok(())
    }

    /// Adds an unconditional line break.
    pub fn hard_break(&mut self) -> fmt::Result {
        if self.separator_supplied
            && self
                .sink
                .events
                .last()
                .is_some_and(|event| matches!(event, PrettyEvent::SoftBreak(_)))
        {
            *self.sink.events.last_mut().expect("checked soft break") = PrettyEvent::HardBreak;
        } else {
            self.sink.events.push(PrettyEvent::HardBreak);
        }
        self.separator_supplied = true;
        self.previous_auto_punctuation = false;
        self.requested_space_after = false;
        Ok(())
    }

    /// Emits one consistent fit-or-break group.
    pub fn group<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.sink.events.push(PrettyEvent::BeginGroup);
        let result = render(self);
        self.sink.events.push(PrettyEvent::EndGroup);
        result
    }

    /// Emits a fill group whose soft-break opportunities decide independently.
    #[doc(hidden)]
    pub fn inconsistent_group<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.sink.events.push(PrettyEvent::BeginInconsistentGroup);
        let result = render(self);
        self.sink.events.push(PrettyEvent::EndGroup);
        result
    }

    #[doc(hidden)]
    pub fn begin_consistent_group(&mut self) {
        self.sink.events.push(PrettyEvent::BeginGroup);
    }

    #[doc(hidden)]
    pub fn begin_inconsistent_group(&mut self) {
        self.sink.events.push(PrettyEvent::BeginInconsistentGroup);
    }

    #[doc(hidden)]
    pub fn end_group(&mut self) {
        self.sink.events.push(PrettyEvent::EndGroup);
    }

    #[doc(hidden)]
    pub fn begin_indent(&mut self) {
        self.sink.events.push(PrettyEvent::Indent);
    }

    #[doc(hidden)]
    pub fn end_indent(&mut self) {
        self.sink.events.push(PrettyEvent::Dedent);
    }

    /// Emits a document region one configured indentation level deeper.
    pub fn indented<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.sink.events.push(PrettyEvent::Indent);
        let result = render(self);
        self.sink.events.push(PrettyEvent::Dedent);
        result
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
        });
        let result = render(self);
        self.repetition = previous;
        result
    }

    #[doc(hidden)]
    pub fn with_repetition_style<R>(
        &mut self,
        separator: Option<&str>,
        trailing: bool,
        hard_break: bool,
        leading_separator: bool,
        render: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.repetition.replace(RepetitionLayout {
            separator: separator.map(str::to_owned),
            trailing,
            hard_break,
            leading_separator,
        });
        let result = render(self);
        self.repetition = previous;
        result
    }

    fn render_repeated<'a, T: Pretty + 'a>(
        &mut self,
        values: impl ExactSizeIterator<Item = &'a T>,
    ) -> fmt::Result {
        let layout = self.repetition.take();
        let length = values.len();
        for (index, value) in values.enumerate() {
            self.with_child(index, |context| value.pretty(context))?;
            if let Some(layout) = &layout
                && (index + 1 < length || layout.trailing)
            {
                if index + 1 < length {
                    if layout.hard_break {
                        if let Some(separator) = &layout.separator {
                            self.tight_token(separator)?;
                        }
                        self.hard_break()?;
                    } else {
                        self.sink.events.push(PrettyEvent::RepetitionBreak {
                            separator: layout.separator.clone(),
                            leading: layout.leading_separator,
                        });
                        self.separator_supplied = true;
                    }
                } else if let Some(separator) = &layout.separator {
                    self.tight_token(separator)?;
                }
            }
        }
        self.repetition = layout;
        Ok(())
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

    /// Runs structured rendering without assuming a source occurrence shape.
    #[doc(hidden)]
    pub fn without_provenance<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> R {
        self.with_provenance(None, render)
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
        let mut context = PrettyContext::new();
        self.pretty(&mut context)
            .expect("rendering into a structured sink cannot fail");
        context.finish()
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
        context.render_repeated(self.iter())
    }
}

impl<T: Pretty> Pretty for crate::Vec1<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        context.render_repeated(self.iter())
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
}
