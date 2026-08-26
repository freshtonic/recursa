//! Facade-owned structured rendering contracts.

use std::fmt;

/// Rendering state shared by generated and source-owned [`Pretty`] impls.
#[derive(Default)]
pub struct PrettyContext {
    output: String,
    needs_separator: bool,
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
        for value in self {
            value.pretty(context)?;
        }
        Ok(())
    }
}

impl<T: Pretty> Pretty for crate::Vec1<T> {
    fn pretty(&self, context: &mut PrettyContext) -> fmt::Result {
        for value in self.iter() {
            value.pretty(context)?;
        }
        Ok(())
    }
}
