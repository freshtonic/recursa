use crate::{LocationId, TypeId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticCode {
    InvalidGrammar,
    InvalidReference,
    LeftRecursion,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidGrammar => "RCA0001",
            Self::InvalidReference => "RCA0002",
            Self::LeftRecursion => "RCA0100",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticLabel {
    pub location: Option<LocationId>,
    pub message: String,
}

impl DiagnosticLabel {
    pub fn located(location: LocationId, message: impl Into<String>) -> Self {
        Self {
            location: Some(location),
            message: message.into(),
        }
    }

    pub fn unlocated(message: impl Into<String>) -> Self {
        Self {
            location: None,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Remedy {
    CorrectGrammar,
    CorrectReference,
    BreakLeftRecursion { cycle: Vec<TypeId> },
}

impl Remedy {
    pub(crate) const fn canonical_text(&self) -> &'static str {
        match self {
            Self::CorrectGrammar => "correct the normalized grammar before semantic analysis",
            Self::CorrectReference => "correct the normalized reference before semantic analysis",
            Self::BreakLeftRecursion { .. } => {
                "express recursive branches as Pratt led variants or move recursion after a required token"
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub primary: DiagnosticLabel,
    pub secondary: Vec<DiagnosticLabel>,
    pub remedy: Option<Remedy>,
}

impl Diagnostic {
    pub(crate) fn invalid_grammar(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code: DiagnosticCode::InvalidGrammar,
            severity: Severity::Error,
            primary: DiagnosticLabel::unlocated(message.clone()),
            message,
            secondary: Vec::new(),
            remedy: Some(Remedy::CorrectGrammar),
        }
    }

    pub(crate) fn invalid_reference(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code: DiagnosticCode::InvalidReference,
            severity: Severity::Error,
            primary: DiagnosticLabel::unlocated(message.clone()),
            message,
            secondary: Vec::new(),
            remedy: Some(Remedy::CorrectReference),
        }
    }

    pub(crate) fn left_recursion(
        message: impl Into<String>,
        cycle: Vec<TypeId>,
        labels: Vec<DiagnosticLabel>,
    ) -> Self {
        let mut labels = labels.into_iter();
        let primary = labels
            .next()
            .expect("a recursive component has at least one internal edge");
        Self {
            code: DiagnosticCode::LeftRecursion,
            severity: Severity::Error,
            message: message.into(),
            primary,
            secondary: labels.collect(),
            remedy: Some(Remedy::BreakLeftRecursion { cycle }),
        }
    }
}
