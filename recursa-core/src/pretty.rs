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

/// Rendering state shared by generated and source-owned [`Pretty`] impls.
#[derive(Default)]
pub struct PrettyContext {
    output: String,
    needs_separator: bool,
    provenance: Option<PrettyProvenance>,
}

impl PrettyContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits one semantic token using the default separating-space policy.
    pub fn token(&mut self, text: &str) -> fmt::Result {
        if self.needs_separator && !text.is_empty() {
            self.output.push(' ');
        }
        self.output.push_str(text);
        self.needs_separator = !text.is_empty();
        Ok(())
    }

    /// Emits an exact captured source fragment without normalization.
    pub fn exact(&mut self, text: &str) -> fmt::Result {
        self.output.push_str(text);
        self.needs_separator = !text.is_empty();
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
        self.output
    }
}

/// Structured rendering shared by semantic ASTs, parsed roots, and views.
pub trait Pretty {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result;

    fn render(&self) -> String {
        let mut context = PrettyContext::new();
        self.pretty(&mut context)
            .expect("rendering into a String cannot fail");
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
