use crate::{DeclId, KindId, KindSet, LocationId, ProofId, SiteId, TextPredicateId, TypeId};

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
    RaiseMaxLookahead {
        configured: usize,
        sufficient: usize,
    },
    RestructureSharedPrefix {
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
    RestructurePrattSharedPrefix {
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
    pub(crate) fn canonical_text(&self) -> String {
        match self {
            Self::DisambiguateAlternatives { .. } => {
                "make the alternatives start with disjoint kinds or text predicates".to_owned()
            }
            Self::RaiseMaxLookahead {
                configured,
                sufficient,
            } => format!("raise max_lookahead from {configured} to {sufficient}"),
            Self::RestructureSharedPrefix { .. } => {
                "restructure the alternatives into a shared-prefix type".to_owned()
            }
            Self::MakeSiteRequired { .. } => {
                "make the nullable use site required or disambiguate the following site".to_owned()
            }
            Self::DisambiguatePrattVariants { .. } => {
                "make the Pratt variants start with disjoint kinds or text predicates".to_owned()
            }
            Self::RestructurePrattSharedPrefix { .. } => {
                "restructure the Pratt variants into a shared-prefix type".to_owned()
            }
            Self::ReviewNullableFollow { .. } => {
                "make the use site required or separate it from its possible caller FOLLOW"
                    .to_owned()
            }
            Self::ReviewPrattFollow { .. } => {
                "raise the site's minimum binding power or separate its caller FOLLOW from active extenders"
                    .to_owned()
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LookaheadWitnessPosition {
    pub kind: KindId,
    pub left_text_predicate: Option<TextPredicateId>,
    pub right_text_predicate: Option<TextPredicateId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedPrefixEvidence {
    pub configured_depth: usize,
    pub sufficient_depth: Option<usize>,
    pub witness: Vec<LookaheadWitnessPosition>,
    pub end_of_alternative: bool,
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
    pub lookahead: Option<SharedPrefixEvidence>,
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
            lookahead: None,
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
            lookahead: None,
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
            lookahead: None,
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
        Self::shared_prefix_conflict(code, message, primary, secondary, Some(evidence), remedy)
    }

    pub(crate) fn shared_prefix_conflict(
        code: DiagnosticCode,
        message: impl Into<String>,
        primary: DiagnosticLabel,
        secondary: DiagnosticLabel,
        evidence: Option<ConflictEvidence>,
        remedy: ConflictRemedy,
    ) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            primary,
            secondary: vec![secondary],
            remedy: None,
            conflict: evidence,
            lookahead: None,
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
