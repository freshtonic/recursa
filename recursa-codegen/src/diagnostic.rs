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
    /// Generation cannot continue for the affected grammar.
    Error,
    /// Generation may continue, but authored intent is ambiguous or fragile.
    Warning,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Stable semantic-analysis diagnostic family.
pub enum DiagnosticCode {
    /// Grammar structure violates an authored-language invariant.
    InvalidGrammar,
    /// An authored reference cannot be resolved consistently.
    InvalidReference,
    /// A non-Pratt parse path recurses before consuming input.
    LeftRecursion,
    /// Enum alternatives have overlapping FIRST evidence.
    EnumFirstOverlap,
    /// A static use site cannot choose between alternatives.
    StaticUseSiteOverlap,
    /// Pratt prefix alternatives overlap.
    PrattAtomOverlap,
    /// Pratt extender alternatives overlap.
    PrattLedOverlap,
    /// A nullable choice overlaps its FOLLOW context.
    NullableFollowOverlap,
    /// Pratt continuation overlaps the caller FOLLOW context.
    PrattFollowOverlap,
    /// Frozen analysis products contradict an internal invariant.
    InternalAnalysisInvariant,
}

impl DiagnosticCode {
    /// Returns the stable machine-readable diagnostic code.
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
    /// Normalized grammar location when semantic analysis can identify one.
    pub location: Option<LocationId>,
    /// Human-readable evidence attached to the location.
    pub message: String,
}

impl DiagnosticLabel {
    /// Creates evidence tied to an authored source range.
    pub fn located(location: LocationId, message: impl Into<String>) -> Self {
        Self {
            location: Some(location),
            message: message.into(),
        }
    }

    /// Creates global evidence when no authored range is available.
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
    /// Directs the author to repair invalid grammar structure.
    CorrectGrammar,
    /// Directs the author to repair a missing or incompatible reference.
    CorrectReference,
    /// Directs the author to ensure input is consumed before recursion.
    BreakLeftRecursion {
        /// Types in the detected recursive cycle, in canonical traversal order.
        cycle: Vec<TypeId>,
    },
    /// Identifies an internal invariant failure for bug reporting.
    ReportInternalFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Corrective action for an ambiguity or FOLLOW conflict.
pub enum ConflictRemedy {
    /// Requests distinct prefixes for the named alternatives.
    DisambiguateAlternatives {
        /// First alternative declaration requiring disambiguation.
        left: DeclId,
        /// Conflicting alternative declaration requiring disambiguation.
        right: DeclId,
    },
    /// Suggests a larger lookahead bound when it can distinguish the alternatives.
    RaiseMaxLookahead {
        /// Grammar's current maximum lookahead depth.
        configured: usize,
        /// Smallest proven depth that separates the alternatives.
        sufficient: usize,
    },
    /// Requests factoring or restructuring of a shared prefix.
    RestructureSharedPrefix {
        /// First declaration sharing the prefix.
        left: DeclId,
        /// Second declaration sharing the prefix.
        right: DeclId,
    },
    /// Requests removing nullability at the affected use site.
    MakeSiteRequired {
        /// Nullable use site whose following syntax must be separated.
        site: SiteId,
    },
    /// Requests distinct prefixes for the named Pratt variants.
    DisambiguatePrattVariants {
        /// Pratt Node owning the conflicting variants.
        pratt: TypeId,
        /// First conflicting Pratt declaration.
        left: DeclId,
        /// Second conflicting Pratt declaration.
        right: DeclId,
    },
    /// Requests restructuring a shared Pratt prefix.
    RestructurePrattSharedPrefix {
        /// Pratt Node owning the shared prefix.
        pratt: TypeId,
        /// First Pratt declaration sharing the prefix.
        left: DeclId,
        /// Second Pratt declaration sharing the prefix.
        right: DeclId,
    },
    /// Requests separating a nullable branch from its FOLLOW context.
    ReviewNullableFollow {
        /// Nullable use site overlapping caller FOLLOW.
        site: SiteId,
    },
    /// Requests separating Pratt continuation from caller FOLLOW.
    ReviewPrattFollow {
        /// Pratt use site whose active extenders overlap caller FOLLOW.
        site: SiteId,
        /// Minimum binding power currently admitted at the use site.
        min_bp: u8,
    },
}

impl ConflictRemedy {
    /// Renders stable, user-facing remedy text.
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
    /// Concrete token kind present in both symbolic sets.
    pub kind: KindId,
    /// Proof reaching the kind from the first alternative.
    pub left: ProofId,
    /// Proof reaching the kind from the conflicting alternative.
    pub right: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete symbolic overlap and one reproducible proof witness.
pub struct ConflictEvidence {
    /// Complete intersection of the conflicting kind sets.
    pub overlap: KindSet,
    /// Reproducible proofs for one representative overlapping kind.
    pub witness: ConflictWitness,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One token and text-predicate position in a shared-prefix witness.
pub struct LookaheadWitnessPosition {
    /// Token kind consumed at this witness position.
    pub kind: KindId,
    /// Exact-text constraint from the first alternative, when present.
    pub left_text_predicate: Option<TextPredicateId>,
    /// Exact-text constraint from the second alternative, when present.
    pub right_text_predicate: Option<TextPredicateId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Finite or pumped evidence that configured lookahead cannot separate alternatives.
pub struct SharedPrefixEvidence {
    /// Maximum lookahead depth used by the conflicting analysis.
    pub configured_depth: usize,
    /// Smallest separating depth, or `None` when pumping proves no finite separation.
    pub sufficient_depth: Option<usize>,
    /// Shared token/text prefix in consumption order.
    pub witness: Vec<LookaheadWitnessPosition>,
    /// Whether one alternative ends while the shared prefix is still active.
    pub end_of_alternative: bool,
}

impl Remedy {
    /// Renders stable diagnostic text from structured evidence and remedies.
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
    /// Stable semantic diagnostic family.
    pub code: DiagnosticCode,
    /// Whether the finding blocks a valid analyzed grammar.
    pub severity: Severity,
    /// Source-independent summary of the semantic failure.
    pub message: String,
    /// Principal normalized location and explanatory label.
    pub primary: DiagnosticLabel,
    /// Corroborating locations in deterministic evidence order.
    pub secondary: Vec<DiagnosticLabel>,
    /// Corrective action for a non-conflict finding.
    pub remedy: Option<Remedy>,
    /// Symbolic kind overlap and proof witness, when applicable.
    pub conflict: Option<ConflictEvidence>,
    /// Shared-prefix evidence for bounded-lookahead findings.
    pub lookahead: Option<SharedPrefixEvidence>,
    /// Corrective action retaining conflict identities and site facts.
    pub conflict_remedy: Option<ConflictRemedy>,
}

impl Diagnostic {
    /// Creates a fatal diagnostic for invalid authored grammar structure.
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

    /// Creates a fatal diagnostic for an unresolved authored reference.
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

    /// Creates a fatal diagnostic for contradictory analysis products.
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

    /// Creates a fatal diagnostic with the witnessed left-recursive cycle.
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

    /// Creates a fatal ambiguity diagnostic from competing alternatives.
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

    /// Creates a fatal ambiguity diagnostic with shared-prefix evidence.
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

    /// Creates a non-fatal ambiguity warning from competing alternatives.
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
