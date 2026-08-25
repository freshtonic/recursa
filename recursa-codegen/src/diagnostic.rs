#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticCode {
    InvalidGrammar,
    InvalidReference,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidGrammar => "RCA0001",
            Self::InvalidReference => "RCA0002",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub(crate) fn invalid_grammar(message: impl Into<String>) -> Self {
        Self {
            code: DiagnosticCode::InvalidGrammar,
            severity: Severity::Error,
            message: message.into(),
        }
    }

    pub(crate) fn invalid_reference(message: impl Into<String>) -> Self {
        Self {
            code: DiagnosticCode::InvalidReference,
            severity: Severity::Error,
            message: message.into(),
        }
    }
}
