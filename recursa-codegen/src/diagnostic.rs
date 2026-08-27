#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Typed source-independent semantic-analysis diagnostics.
//!
//! These values retain normalized identities and proof witnesses. The generation-facing
//! diagnostic boundary later converts them into stable-coded, source-located findings without
//! discarding conflict provenance or recomputing semantic analysis.

use crate::{DeclId, KindId, KindSet, LocationId, ProofId, SiteId, TextPredicateId, TypeId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Whether a semantic finding blocks the analyzed grammar.
pub enum Severity {
    /// The `Error` case of this diagnostic contract.
    Error,
    /// The `Warning` case of this diagnostic contract.
    Warning,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Stable semantic-analysis diagnostic family.
pub enum DiagnosticCode {
    /// The `InvalidGrammar` case of this diagnostic contract.
    InvalidGrammar,
    /// The `InvalidReference` case of this diagnostic contract.
    InvalidReference,
    /// The `LeftRecursion` case of this diagnostic contract.
    LeftRecursion,
    /// The `EnumFirstOverlap` case of this diagnostic contract.
    EnumFirstOverlap,
    /// The `StaticUseSiteOverlap` case of this diagnostic contract.
    StaticUseSiteOverlap,
    /// The `PrattAtomOverlap` case of this diagnostic contract.
    PrattAtomOverlap,
    /// The `PrattLedOverlap` case of this diagnostic contract.
    PrattLedOverlap,
    /// The `NullableFollowOverlap` case of this diagnostic contract.
    NullableFollowOverlap,
    /// The `PrattFollowOverlap` case of this diagnostic contract.
    PrattFollowOverlap,
    /// The `InternalAnalysisInvariant` case of this diagnostic contract.
    InternalAnalysisInvariant,
}

impl DiagnosticCode {
    /// Returns the as str value.
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
            Self::InternalAnalysisInvariant => "RCA9104",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Semantic diagnostic message optionally attached to a normalized location identity.
pub struct DiagnosticLabel {
    /// Location associated with this value.
    pub location: Option<LocationId>,
    /// Message associated with this value.
    pub message: String,
}

impl DiagnosticLabel {
    /// Constructs the located result.
    pub fn located(location: LocationId, message: impl Into<String>) -> Self {
        Self {
            location: Some(location),
            message: message.into(),
        }
    }

    /// Constructs the unlocated result.
    pub fn unlocated(message: impl Into<String>) -> Self {
        Self {
            location: None,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Corrective action for a non-conflict semantic diagnostic.
pub enum Remedy {
    /// The `CorrectGrammar` case of this diagnostic contract.
    CorrectGrammar,
    /// The `CorrectReference` case of this diagnostic contract.
    CorrectReference,
    /// The `BreakLeftRecursion` case of this diagnostic contract.
    BreakLeftRecursion {
        /// Types in the detected recursive cycle, in canonical traversal order.
        cycle: Vec<TypeId>,
    },
    /// The `ReportInternalFailure` case of this diagnostic contract.
    ReportInternalFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Corrective action for an ambiguity or FOLLOW conflict.
pub enum ConflictRemedy {
    /// The `DisambiguateAlternatives` case of this diagnostic contract.
    DisambiguateAlternatives {
        /// Left associated with this value.
        left: DeclId,
        /// Right associated with this value.
        right: DeclId,
    },
    /// The `RaiseMaxLookahead` case of this diagnostic contract.
    RaiseMaxLookahead {
        /// Configured associated with this value.
        configured: usize,
        /// Sufficient associated with this value.
        sufficient: usize,
    },
    /// The `RestructureSharedPrefix` case of this diagnostic contract.
    RestructureSharedPrefix {
        /// Left associated with this value.
        left: DeclId,
        /// Right associated with this value.
        right: DeclId,
    },
    /// The `MakeSiteRequired` case of this diagnostic contract.
    MakeSiteRequired {
        /// Site associated with this value.
        site: SiteId,
    },
    /// The `DisambiguatePrattVariants` case of this diagnostic contract.
    DisambiguatePrattVariants {
        /// Pratt associated with this value.
        pratt: TypeId,
        /// Left associated with this value.
        left: DeclId,
        /// Right associated with this value.
        right: DeclId,
    },
    /// The `RestructurePrattSharedPrefix` case of this diagnostic contract.
    RestructurePrattSharedPrefix {
        /// Pratt associated with this value.
        pratt: TypeId,
        /// Left associated with this value.
        left: DeclId,
        /// Right associated with this value.
        right: DeclId,
    },
    /// The `ReviewNullableFollow` case of this diagnostic contract.
    ReviewNullableFollow {
        /// Site associated with this value.
        site: SiteId,
    },
    /// The `ReviewPrattFollow` case of this diagnostic contract.
    ReviewPrattFollow {
        /// Site associated with this value.
        site: SiteId,
        /// Min bp associated with this value.
        min_bp: u8,
    },
}

impl ConflictRemedy {
    /// Produces the deterministic text.
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
/// Pair of proof-arena witnesses demonstrating one overlapping token kind.
pub struct ConflictWitness {
    /// Kind associated with this value.
    pub kind: KindId,
    /// Left associated with this value.
    pub left: ProofId,
    /// Right associated with this value.
    pub right: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete symbolic overlap and one reproducible proof witness.
pub struct ConflictEvidence {
    /// Overlap associated with this value.
    pub overlap: KindSet,
    /// Witness associated with this value.
    pub witness: ConflictWitness,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One token and text-predicate position in a shared-prefix witness.
pub struct LookaheadWitnessPosition {
    /// Kind associated with this value.
    pub kind: KindId,
    /// Left text predicate associated with this value.
    pub left_text_predicate: Option<TextPredicateId>,
    /// Right text predicate associated with this value.
    pub right_text_predicate: Option<TextPredicateId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Finite or pumped evidence that configured lookahead cannot separate alternatives.
pub struct SharedPrefixEvidence {
    /// Configured depth associated with this value.
    pub configured_depth: usize,
    /// Sufficient depth associated with this value.
    pub sufficient_depth: Option<usize>,
    /// Witness associated with this value.
    pub witness: Vec<LookaheadWitnessPosition>,
    /// End of alternative associated with this value.
    pub end_of_alternative: bool,
}

impl Remedy {
    /// Produces the deterministic text.
    pub(crate) const fn canonical_text(&self) -> &'static str {
        match self {
            Self::CorrectGrammar => "correct the normalized grammar before semantic analysis",
            Self::CorrectReference => "correct the normalized reference before semantic analysis",
            Self::BreakLeftRecursion { .. } => {
                "express recursive branches as Pratt led variants or move recursion after a required token"
            }
            Self::ReportInternalFailure => {
                "report this internal invariant failure with the authored grammar"
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Typed semantic-analysis finding before source correlation and stable rendering.
pub struct Diagnostic {
    /// Code associated with this value.
    pub code: DiagnosticCode,
    /// Severity associated with this value.
    pub severity: Severity,
    /// Message associated with this value.
    pub message: String,
    /// Primary associated with this value.
    pub primary: DiagnosticLabel,
    /// Secondary associated with this value.
    pub secondary: Vec<DiagnosticLabel>,
    /// Remedy associated with this value.
    pub remedy: Option<Remedy>,
    /// Conflict associated with this value.
    pub conflict: Option<ConflictEvidence>,
    /// Lookahead associated with this value.
    pub lookahead: Option<SharedPrefixEvidence>,
    /// Conflict remedy associated with this value.
    pub conflict_remedy: Option<ConflictRemedy>,
}

impl Diagnostic {
    /// Constructs the invalid grammar result.
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

    /// Constructs the invalid reference result.
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

    /// Constructs the internal analysis invariant result.
    pub(crate) fn internal_analysis_invariant(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code: DiagnosticCode::InternalAnalysisInvariant,
            severity: Severity::Error,
            primary: DiagnosticLabel::unlocated(message.clone()),
            message,
            secondary: Vec::new(),
            remedy: Some(Remedy::ReportInternalFailure),
            conflict: None,
            lookahead: None,
            conflict_remedy: None,
        }
    }

    /// Constructs the left recursion result.
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

    /// Constructs the conflict result.
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

    /// Constructs the shared prefix conflict result.
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

    /// Constructs the conflict warning result.
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
