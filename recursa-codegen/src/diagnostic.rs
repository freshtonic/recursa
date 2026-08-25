use crate::{DeclId, KindId, KindSet, LocationId, ProofId, SiteId, TypeId};

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
    EnumFirstOverlap,
    StaticUseSiteOverlap,
    PrattAtomOverlap,
    PrattLedOverlap,
    NullableFollowOverlap,
    PrattFollowOverlap,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidGrammar => "RCA0001",
            Self::InvalidReference => "RCA0002",
            Self::LeftRecursion => "RCA0100",
            Self::EnumFirstOverlap => "RCA0200",
            Self::StaticUseSiteOverlap => "RCA0201",
            Self::PrattAtomOverlap => "RCA0202",
            Self::PrattLedOverlap => "RCA0203",
            Self::NullableFollowOverlap => "RCA0300",
            Self::PrattFollowOverlap => "RCA0301",
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConflictRemedy {
    DisambiguateAlternatives {
        left: DeclId,
        right: DeclId,
    },
    MakeSiteRequired {
        site: SiteId,
    },
    DisambiguatePrattVariants {
        pratt: TypeId,
        left: DeclId,
        right: DeclId,
    },
    ReviewNullableFollow {
        site: SiteId,
    },
    ReviewPrattFollow {
        site: SiteId,
        min_bp: u8,
    },
}

impl ConflictRemedy {
    pub(crate) const fn canonical_text(&self) -> &'static str {
        match self {
            Self::DisambiguateAlternatives { .. } => {
                "make the alternatives start with disjoint kinds or text predicates"
            }
            Self::MakeSiteRequired { .. } => {
                "make the nullable use site required or disambiguate the following site"
            }
            Self::DisambiguatePrattVariants { .. } => {
                "make the Pratt variants start with disjoint kinds or text predicates"
            }
            Self::ReviewNullableFollow { .. } => {
                "make the use site required or separate it from its possible caller FOLLOW"
            }
            Self::ReviewPrattFollow { .. } => {
                "raise the site's minimum binding power or separate its caller FOLLOW from active extenders"
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictWitness {
    pub kind: KindId,
    pub left: ProofId,
    pub right: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictEvidence {
    pub overlap: KindSet,
    pub witness: ConflictWitness,
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
    pub conflict: Option<ConflictEvidence>,
    pub conflict_remedy: Option<ConflictRemedy>,
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
            conflict: None,
            conflict_remedy: None,
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
            conflict: None,
            conflict_remedy: None,
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
            conflict: None,
            conflict_remedy: None,
        }
    }

    pub(crate) fn conflict(
        code: DiagnosticCode,
        message: impl Into<String>,
        primary: DiagnosticLabel,
        secondary: DiagnosticLabel,
        evidence: ConflictEvidence,
        remedy: ConflictRemedy,
    ) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            primary,
            secondary: vec![secondary],
            remedy: None,
            conflict: Some(evidence),
            conflict_remedy: Some(remedy),
        }
    }

    pub(crate) fn conflict_warning(
        code: DiagnosticCode,
        message: impl Into<String>,
        primary: DiagnosticLabel,
        secondary: DiagnosticLabel,
        evidence: ConflictEvidence,
        remedy: ConflictRemedy,
    ) -> Self {
        let mut diagnostic = Self::conflict(code, message, primary, secondary, evidence, remedy);
        diagnostic.severity = Severity::Warning;
        diagnostic
    }
}
