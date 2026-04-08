use miette::{Diagnostic, LabeledSpan, SourceCode};
use std::fmt;
use std::ops::Range;

/// A parse error with source location, expected/found info, and optional context chain.
///
/// Internally boxed to keep `Result<T, ParseError>` small on the stack.
#[derive(Debug, Clone)]
pub struct ParseError {
    inner: Box<ParseErrorInner>,
}

#[derive(Debug, Clone)]
struct ParseErrorInner {
    src: String,
    span: Range<usize>,
    expected: String,
    found: Option<String>,
    help: Option<String>,
    context: Vec<ContextError>,
}

/// A "while parsing X" breadcrumb attached to a ParseError.
#[derive(Debug, Clone)]
struct ContextError {
    label: String,
    span: Range<usize>,
}

impl ParseError {
    /// Create a new parse error.
    ///
    /// - `src`: the full source text
    /// - `span`: byte range of the problematic input
    /// - `expected`: description of what was expected
    pub fn new(src: impl Into<String>, span: Range<usize>, expected: impl Into<String>) -> Self {
        Self {
            inner: Box::new(ParseErrorInner {
                src: src.into(),
                span,
                expected: expected.into(),
                found: None,
                help: None,
                context: Vec::new(),
            }),
        }
    }

    /// What was expected at this position.
    pub fn expected(&self) -> &str {
        &self.inner.expected
    }

    /// Set what was actually found.
    pub fn with_found(mut self, found: impl Into<String>) -> Self {
        self.inner.found = Some(found.into());
        self
    }

    /// Add a help message.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.inner.help = Some(help.into());
        self
    }

    /// Wrap this error with "while parsing <label>" context.
    pub fn with_context(mut self, label: impl Into<String>, span: Range<usize>) -> Self {
        self.inner.context.push(ContextError {
            label: label.into(),
            span,
        });
        self
    }

    /// Merge multiple expected values into one error (for enum dispatch failures).
    pub fn merge(errors: Vec<ParseError>) -> Self {
        assert!(!errors.is_empty(), "cannot merge empty error list");
        let first = &errors[0];
        let src = first.inner.src.clone();
        let span = first.inner.span.clone();

        let expected_items: Vec<&str> = errors.iter().map(|e| e.inner.expected.as_str()).collect();
        let expected = format!("one of: {}", expected_items.join(", "));

        Self {
            inner: Box::new(ParseErrorInner {
                src,
                span,
                expected,
                found: first.inner.found.clone(),
                help: None,
                context: Vec::new(),
            }),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner.found {
            Some(found) => write!(f, "expected {} but found {}", self.inner.expected, found),
            None => write!(f, "expected {}", self.inner.expected),
        }
    }
}

impl std::error::Error for ParseError {}

impl Diagnostic for ParseError {
    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.inner.src)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let label = match &self.inner.found {
            Some(found) => format!("found {}", found),
            None => format!("expected {}", self.inner.expected),
        };
        Some(Box::new(std::iter::once(LabeledSpan::new(
            Some(label),
            self.inner.span.start,
            self.inner.span.len(),
        ))))
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        if self.inner.context.is_empty() {
            return None;
        }
        Some(Box::new(
            self.inner.context.iter().map(|c| c as &dyn Diagnostic),
        ))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.inner
            .help
            .as_ref()
            .map(|h| Box::new(h.as_str()) as Box<dyn fmt::Display>)
    }
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "while parsing {}", self.label)
    }
}

impl std::error::Error for ContextError {}

impl Diagnostic for ContextError {
    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(std::iter::once(LabeledSpan::new(
            Some(format!("while parsing {}", self.label)),
            self.span.start,
            self.span.len(),
        ))))
    }
}
