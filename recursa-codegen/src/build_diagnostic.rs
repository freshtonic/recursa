#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Stable structured diagnostics produced during generation and structural loading.
//!
//! Diagnostic codes determine their owning stage, default severity, location rule, evidence
//! schema, and remedy schema. [`BuildReport`] validates those contracts, deduplicates findings
//! by semantic identity, and publishes them in deterministic source order. An error-bearing
//! report is the publication barrier represented by [`GenerateError`].

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use crate::Severity;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
/// Pipeline phase that owns a Recursa diagnostic.
pub enum DiagnosticStage {
    /// Selected-root and module discovery.
    Discovery,
    /// Grammar-semantic name resolution.
    Resolution,
    /// Source-model lowering and materialization.
    Lowering,
    /// Source-independent semantic analysis.
    Analysis,
    /// Emission-plan validation or generated-code rendering.
    Emission,
    /// Post-publication structural loader verification.
    Loading,
}

impl DiagnosticStage {
    /// Returns the stable pipeline-stage spelling used in diagnostic output.
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Resolution => "resolution",
            Self::Lowering => "lowering",
            Self::Analysis => "analysis",
            Self::Emission => "emission",
            Self::Loading => "loading",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Registry contract for one stable build-diagnostic code.
pub(crate) struct DiagnosticCodeSpec {
    /// Stable code governed by this registry entry.
    pub(crate) code: RecursaDiagnosticCode,
    /// Pipeline stage permitted to emit the code.
    pub(crate) stage: DiagnosticStage,
    /// Default blocking or advisory severity.
    pub(crate) severity: Severity,
    /// Rule selecting the principal source label.
    pub(crate) primary_location: PrimaryLocationRule,
    /// Required structured evidence shape.
    pub(crate) evidence: DiagnosticEvidenceSchema,
    /// Allowed corrective-action family.
    pub(crate) remedy: DiagnosticRemedySchema,
    /// Whether the assignment may still be emitted or is retained only to prevent reuse.
    pub(crate) status: DiagnosticCodeStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Records whether one stable diagnostic assignment remains active.
pub(crate) enum DiagnosticCodeStatus {
    /// The failure class may still produce this diagnostic.
    Active,
    /// The failure class was removed and its code must never be reassigned.
    Retired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Selects the authored range—or root fallback—that receives a diagnostic's primary label.
pub(crate) enum PrimaryLocationRule {
    /// Uses the first related range, falling back to a global diagnostic.
    FirstRelatedOrGlobal,
    /// Uses the selected root when more specific evidence has no range.
    SelectedRootFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Constrains the structured proof an emitter must attach for a registered diagnostic code.
pub(crate) enum DiagnosticEvidenceSchema {
    /// Requires evidence derived directly from authored source.
    SourceFacts,
    /// Requires structured lexical-policy evidence.
    LexicalFacts(LexicalEvidenceClass),
    /// Requires a semantic-analysis conflict witness.
    AnalysisWitness,
    /// Records contradictory internal state without blaming authored input.
    InternalInvariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Constrains the corrective-action family permitted for a registered diagnostic code.
pub(crate) enum DiagnosticRemedySchema {
    /// Remedy must identify an authored source correction.
    CorrectAuthoredInput,
    /// Remedy must resolve token classification or matching policy.
    ResolveLexicalPolicy(LexicalEvidenceClass),
    /// Remedy must remove a witnessed semantic ambiguity.
    ResolveAnalysisConflict,
    /// Remedy directs the user to report an internal invariant failure.
    ReportInternalFailure,
}

/// Returns the registry-selected primary-location policy for a code.
const fn primary_location_rule(code: &str) -> PrimaryLocationRule {
    if code.as_bytes()[3] == b'8' || code.as_bytes()[3] == b'9' {
        PrimaryLocationRule::SelectedRootFallback
    } else {
        PrimaryLocationRule::FirstRelatedOrGlobal
    }
}

/// Returns the structured evidence family required by a code.
const fn evidence_schema(code: &str, _stage: DiagnosticStage) -> DiagnosticEvidenceSchema {
    match diagnostic_number(code) {
        3001 => DiagnosticEvidenceSchema::LexicalFacts(LexicalEvidenceClass::KeywordMatchingPolicy),
        3002 => {
            DiagnosticEvidenceSchema::LexicalFacts(LexicalEvidenceClass::FixedTokenClassification)
        }
        3003 => DiagnosticEvidenceSchema::LexicalFacts(
            LexicalEvidenceClass::ConflictingDefinitionMetadata,
        ),
        3004 => DiagnosticEvidenceSchema::LexicalFacts(LexicalEvidenceClass::InvalidPattern),
        3005 => DiagnosticEvidenceSchema::LexicalFacts(LexicalEvidenceClass::PatternOverlap),
        3006 => DiagnosticEvidenceSchema::LexicalFacts(LexicalEvidenceClass::PriorityTie),
        9101..=9105 => DiagnosticEvidenceSchema::InternalInvariant,
        1..=301 => DiagnosticEvidenceSchema::AnalysisWitness,
        1001..=3211 => DiagnosticEvidenceSchema::SourceFacts,
        _ => DiagnosticEvidenceSchema::InternalInvariant,
    }
}

/// Returns the remedy family permitted for a code.
const fn remedy_schema(code: &str, _stage: DiagnosticStage) -> DiagnosticRemedySchema {
    match diagnostic_number(code) {
        3001 => DiagnosticRemedySchema::ResolveLexicalPolicy(
            LexicalEvidenceClass::KeywordMatchingPolicy,
        ),
        3002 => DiagnosticRemedySchema::ResolveLexicalPolicy(
            LexicalEvidenceClass::FixedTokenClassification,
        ),
        3003 => DiagnosticRemedySchema::ResolveLexicalPolicy(
            LexicalEvidenceClass::ConflictingDefinitionMetadata,
        ),
        3004 => DiagnosticRemedySchema::ResolveLexicalPolicy(LexicalEvidenceClass::InvalidPattern),
        3005 => DiagnosticRemedySchema::ResolveLexicalPolicy(LexicalEvidenceClass::PatternOverlap),
        3006 => DiagnosticRemedySchema::ResolveLexicalPolicy(LexicalEvidenceClass::PriorityTie),
        9101..=9105 => DiagnosticRemedySchema::ReportInternalFailure,
        1..=301 => DiagnosticRemedySchema::ResolveAnalysisConflict,
        1001..=3211 => DiagnosticRemedySchema::CorrectAuthoredInput,
        _ => DiagnosticRemedySchema::ReportInternalFailure,
    }
}

/// Returns the stable numeric suffix used in the published code.
const fn diagnostic_number(code: &str) -> u16 {
    let bytes = code.as_bytes();
    ((bytes[3] - b'0') as u16) * 1000
        + ((bytes[4] - b'0') as u16) * 100
        + ((bytes[5] - b'0') as u16) * 10
        + (bytes[6] - b'0') as u16
}

/// Defines diagnostic spec entries in canonical order.
macro_rules! diagnostic_spec {
    ($code:literal, $stage:ident, $severity:ident) => {
        DiagnosticCodeSpec {
            code: RecursaDiagnosticCode::new($code),
            stage: DiagnosticStage::$stage,
            severity: Severity::$severity,
            primary_location: primary_location_rule($code),
            evidence: evidence_schema($code, DiagnosticStage::$stage),
            remedy: remedy_schema($code, DiagnosticStage::$stage),
            status: DiagnosticCodeStatus::Active,
        }
    };
}

/// Retains an inactive diagnostic assignment so its stable code cannot be reused.
macro_rules! retired_diagnostic_spec {
    ($code:literal, $stage:ident, $severity:ident) => {
        DiagnosticCodeSpec {
            code: RecursaDiagnosticCode::new($code),
            stage: DiagnosticStage::$stage,
            severity: Severity::$severity,
            primary_location: primary_location_rule($code),
            evidence: evidence_schema($code, DiagnosticStage::$stage),
            remedy: remedy_schema($code, DiagnosticStage::$stage),
            status: DiagnosticCodeStatus::Retired,
        }
    };
}

/// Canonical DIAGNOSTIC CODE REGISTRY value for this boundary.
static DIAGNOSTIC_CODE_REGISTRY: &[DiagnosticCodeSpec] = &[
    diagnostic_spec!("RCA0001", Analysis, Error),
    diagnostic_spec!("RCA0002", Analysis, Error),
    diagnostic_spec!("RCA0100", Analysis, Error),
    diagnostic_spec!("RCA0200", Analysis, Error),
    diagnostic_spec!("RCA0201", Analysis, Error),
    diagnostic_spec!("RCA0202", Analysis, Error),
    diagnostic_spec!("RCA0203", Analysis, Error),
    diagnostic_spec!("RCA0300", Analysis, Warning),
    diagnostic_spec!("RCA0301", Analysis, Warning),
    diagnostic_spec!("RCA1001", Discovery, Error),
    diagnostic_spec!("RCA1002", Discovery, Error),
    diagnostic_spec!("RCA1003", Discovery, Error),
    diagnostic_spec!("RCA1004", Discovery, Error),
    diagnostic_spec!("RCA1005", Discovery, Error),
    diagnostic_spec!("RCA1006", Discovery, Error),
    diagnostic_spec!("RCA1007", Discovery, Error),
    diagnostic_spec!("RCA1008", Discovery, Error),
    diagnostic_spec!("RCA1009", Discovery, Error),
    diagnostic_spec!("RCA1010", Discovery, Error),
    diagnostic_spec!("RCA1011", Discovery, Error),
    diagnostic_spec!("RCA1012", Discovery, Error),
    diagnostic_spec!("RCA1013", Discovery, Error),
    diagnostic_spec!("RCA1014", Discovery, Error),
    diagnostic_spec!("RCA1015", Discovery, Error),
    diagnostic_spec!("RCA1016", Discovery, Error),
    diagnostic_spec!("RCA1017", Discovery, Error),
    diagnostic_spec!("RCA2001", Resolution, Error),
    diagnostic_spec!("RCA2002", Resolution, Error),
    diagnostic_spec!("RCA2003", Resolution, Error),
    diagnostic_spec!("RCA2004", Resolution, Error),
    diagnostic_spec!("RCA2005", Resolution, Error),
    diagnostic_spec!("RCA2006", Resolution, Error),
    diagnostic_spec!("RCA2007", Resolution, Error),
    diagnostic_spec!("RCA2008", Resolution, Error),
    diagnostic_spec!("RCA2009", Resolution, Error),
    diagnostic_spec!("RCA2010", Resolution, Error),
    diagnostic_spec!("RCA2011", Resolution, Error),
    diagnostic_spec!("RCA2012", Resolution, Error),
    diagnostic_spec!("RCA2013", Resolution, Error),
    diagnostic_spec!("RCA2014", Resolution, Error),
    diagnostic_spec!("RCA2015", Resolution, Error),
    diagnostic_spec!("RCA2016", Resolution, Error),
    diagnostic_spec!("RCA2017", Resolution, Error),
    diagnostic_spec!("RCA3001", Lowering, Error),
    diagnostic_spec!("RCA3002", Lowering, Error),
    diagnostic_spec!("RCA3003", Lowering, Error),
    diagnostic_spec!("RCA3004", Lowering, Error),
    diagnostic_spec!("RCA3005", Lowering, Error),
    diagnostic_spec!("RCA3006", Lowering, Error),
    diagnostic_spec!("RCA3101", Lowering, Error),
    diagnostic_spec!("RCA3102", Lowering, Error),
    diagnostic_spec!("RCA3103", Lowering, Error),
    diagnostic_spec!("RCA3104", Lowering, Error),
    diagnostic_spec!("RCA3105", Lowering, Error),
    diagnostic_spec!("RCA3106", Lowering, Error),
    diagnostic_spec!("RCA3107", Lowering, Error),
    diagnostic_spec!("RCA3108", Lowering, Error),
    diagnostic_spec!("RCA3109", Lowering, Error),
    diagnostic_spec!("RCA3110", Lowering, Error),
    diagnostic_spec!("RCA3111", Lowering, Error),
    diagnostic_spec!("RCA3112", Lowering, Error),
    diagnostic_spec!("RCA3113", Lowering, Error),
    diagnostic_spec!("RCA3114", Lowering, Error),
    diagnostic_spec!("RCA3115", Lowering, Error),
    diagnostic_spec!("RCA3116", Lowering, Error),
    retired_diagnostic_spec!("RCA3117", Lowering, Error),
    diagnostic_spec!("RCA3118", Lowering, Error),
    diagnostic_spec!("RCA3119", Lowering, Warning),
    diagnostic_spec!("RCA3120", Lowering, Error),
    diagnostic_spec!("RCA3121", Lowering, Error),
    diagnostic_spec!("RCA3122", Lowering, Error),
    diagnostic_spec!("RCA3123", Lowering, Error),
    diagnostic_spec!("RCA3124", Lowering, Error),
    diagnostic_spec!("RCA3125", Lowering, Error),
    diagnostic_spec!("RCA3126", Lowering, Error),
    diagnostic_spec!("RCA3127", Lowering, Error),
    diagnostic_spec!("RCA3128", Lowering, Error),
    diagnostic_spec!("RCA3202", Lowering, Error),
    diagnostic_spec!("RCA3203", Lowering, Error),
    diagnostic_spec!("RCA3204", Lowering, Error),
    diagnostic_spec!("RCA3205", Lowering, Error),
    diagnostic_spec!("RCA3206", Lowering, Error),
    diagnostic_spec!("RCA3209", Lowering, Error),
    diagnostic_spec!("RCA3210", Lowering, Error),
    diagnostic_spec!("RCA3211", Lowering, Error),
    diagnostic_spec!("RCA3207", Lowering, Error),
    diagnostic_spec!("RCA3208", Lowering, Error),
    // M3-owned internal failures. RCA9001-RCA9005 retain their distinct M4
    // loader/protocol meanings from #40 and have no M3 producers.
    diagnostic_spec!("RCA9101", Discovery, Error),
    diagnostic_spec!("RCA9102", Resolution, Error),
    diagnostic_spec!("RCA9103", Lowering, Error),
    diagnostic_spec!("RCA9104", Analysis, Error),
    diagnostic_spec!("RCA9105", Emission, Error),
];

/// Returns the closed contract for diagnostics produced while building or loading a grammar.
pub(crate) fn diagnostic_code_registry() -> &'static [DiagnosticCodeSpec] {
    DIAGNOSTIC_CODE_REGISTRY
}

/// Performs the diagnostic code spec phase of this boundary.
pub(crate) fn diagnostic_code_spec(code: RecursaDiagnosticCode) -> &'static DiagnosticCodeSpec {
    diagnostic_code_registry()
        .iter()
        .find(|spec| spec.code == code && spec.status == DiagnosticCodeStatus::Active)
        .expect("every emitted Recursa diagnostic code is actively registered")
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Stable externally rendered identifier for one diagnostic contract.
pub struct RecursaDiagnosticCode(&'static str);

impl RecursaDiagnosticCode {
    /// Constructs a value after enforcing this boundary's invariants.
    pub(crate) const fn new(code: &'static str) -> Self {
        Self(code)
    }

    /// Returns the stable `RCA` code spelling.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Manifest-relative diagnostic location with an optional half-open UTF-8 byte range.
pub struct DiagnosticLocation {
    /// Normalized manifest-relative source path.
    path: PathBuf,
    /// Optional half-open UTF-8 byte range within the source.
    range: Option<Range<u32>>,
    /// Optional rendered coordinates derived from attached source text.
    line_columns: Option<(LineColumn, LineColumn)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One-based source line and Unicode-scalar column used only for canonical rendering.
pub struct LineColumn {
    /// One-based line number.
    pub line: u32,
    /// One-based Unicode-scalar column number.
    pub column: u32,
}

impl DiagnosticLocation {
    /// Creates a validated source-range resolver rooted at the manifest directory.
    pub(crate) fn new(
        path: PathBuf,
        range: Option<Range<u32>>,
    ) -> Result<Self, DiagnosticLocationInvariant> {
        let path = manifest_relative_path(&path)
            .ok_or(DiagnosticLocationInvariant::NonManifestRelativePath)?;
        if range.as_ref().is_some_and(|range| range.start > range.end) {
            return Err(DiagnosticLocationInvariant::ReversedRange);
        }
        Ok(Self {
            path,
            range,
            line_columns: None,
        })
    }

    /// Returns this value with source text attached.
    pub(crate) fn with_source_text(
        mut self,
        source: &str,
    ) -> Result<Self, DiagnosticLocationInvariant> {
        let line_columns = self.range.as_ref().and_then(|range| {
            let start = line_column(source, range.start)?;
            let end = line_column(source, range.end)?;
            Some((start, end))
        });
        if self.range.is_some() && line_columns.is_none() {
            return Err(DiagnosticLocationInvariant::InvalidUtf8Boundary);
        }
        self.line_columns = line_columns;
        Ok(self)
    }

    /// Returns the normalized manifest-relative source path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Returns the optional half-open UTF-8 byte range.
    pub const fn range(&self) -> Option<&Range<u32>> {
        self.range.as_ref()
    }

    /// Returns derived one-based endpoints when source text was available.
    pub const fn line_columns(&self) -> Option<(LineColumn, LineColumn)> {
        self.line_columns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Identifies which source-location invariant prevented construction of a trustworthy span.
pub(crate) enum DiagnosticLocationInvariant {
    /// Path cannot be represented relative to the selected manifest.
    NonManifestRelativePath,
    /// Range end precedes its start.
    ReversedRange,
    /// Range offset splits a UTF-8 scalar value.
    InvalidUtf8Boundary,
}

/// Performs the manifest relative path phase of this boundary.
fn manifest_relative_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

/// Performs the line column phase of this boundary.
fn line_column(source: &str, byte: u32) -> Option<LineColumn> {
    let byte = usize::try_from(byte).ok()?;
    if byte > source.len() || !source.is_char_boundary(byte) {
        return None;
    }
    let prefix = &source[..byte];
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    Some(LineColumn {
        line: u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1).ok()?,
        column: u32::try_from(source[line_start..byte].chars().count() + 1).ok()?,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A primary or secondary diagnostic message with an optional source location.
pub struct RecursaDiagnosticLabel {
    /// Source location when the evidence can be correlated to authored text.
    location: Option<DiagnosticLocation>,
    /// Explanation displayed beside the location.
    message: String,
}

impl RecursaDiagnosticLabel {
    /// Creates evidence tied to a validated authored range.
    pub fn located(location: DiagnosticLocation, message: impl Into<String>) -> Self {
        Self {
            location: Some(location),
            message: message.into(),
        }
    }

    /// Creates global evidence when no authored range is meaningful.
    pub fn unlocated(message: impl Into<String>) -> Self {
        Self {
            location: None,
            message: message.into(),
        }
    }

    /// Returns the optional authored range carried by this evidence.
    pub const fn location(&self) -> Option<&DiagnosticLocation> {
        self.location.as_ref()
    }

    /// Returns the stable user-facing evidence description.
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
/// Structured facts that explain why a Recursa diagnostic was produced.
pub enum DiagnosticEvidence {
    /// Describes the primary authored subject.
    Subject(String),
    /// Records source paths participating in the failure.
    SourcePaths(Vec<PathBuf>),
    /// Records structured facts derived from authored source.
    SourceFacts(Vec<DiagnosticFact>),
    /// Records an internal contradiction and its observed details.
    InternalInvariant {
        /// Internal stage or component that violated its invariant.
        producer: String,
        /// Maintainer-facing invariant failure detail.
        detail: String,
    },
    /// Records the semantic-analysis witness for a conflict.
    SemanticWitness {
        /// Semantic entity explained by the witness.
        subject: String,
        /// Ordered proof steps rendered for the user.
        witness: Vec<String>,
    },
    /// Records a lexical-policy failure and its relevant definitions.
    Lexical {
        /// Lexical rule family that produced the finding.
        condition: LexicalEvidenceClass,
        /// Stable named lexical facts supporting the finding.
        facts: Vec<DiagnosticFact>,
        /// Optional concrete spelling or pattern witness.
        witness: Option<String>,
    },
    /// Records an analysis conflict and its competing alternatives.
    Analysis {
        /// Complete set of overlapping token descriptions.
        overlap: Vec<String>,
        /// Ordered semantic proof path for one overlap.
        witness: Vec<String>,
        /// Recursive type path when recursion contributes evidence.
        cycle: Vec<String>,
        /// Maximum lookahead depth used by analysis.
        configured_lookahead: Option<usize>,
        /// Proven separating depth when one exists.
        sufficient_lookahead: Option<usize>,
        /// Whether an alternative terminates inside the shared prefix.
        end_of_alternative: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies lexical evidence so diagnostics and remedies describe the same policy failure.
pub enum LexicalEvidenceClass {
    /// Keyword and word-token matching policies conflict.
    KeywordMatchingPolicy,
    /// Fixed syntax has an unsupported lexical classification.
    FixedTokenClassification,
    /// Equivalent token definitions disagree on metadata.
    ConflictingDefinitionMetadata,
    /// Token pattern is empty or unsupported.
    InvalidPattern,
    /// Token patterns can match the same input.
    PatternOverlap,
    /// Overlapping token definitions have equal priority.
    PriorityTie,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Structured, deterministically rendered evidence attached to a build diagnostic.
pub struct DiagnosticFact {
    /// Stable fact key defined by the diagnostic schema.
    pub name: String,
    /// Canonical textual representation of the fact.
    pub value: String,
}

impl DiagnosticEvidence {
    /// Renders stable evidence text without discarding its structured identity.
    pub(crate) fn canonical_text(&self) -> String {
        match self {
            Self::Subject(subject) => subject.clone(),
            Self::SourcePaths(paths) => paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            Self::SourceFacts(facts) => facts
                .iter()
                .map(|fact| format!("{}={}", fact.name, fact.value))
                .collect::<Vec<_>>()
                .join(" "),
            Self::InternalInvariant { producer, detail } => {
                format!("internal producer={producer} detail={detail}")
            }
            Self::SemanticWitness { subject, witness } => {
                format!("{subject}: {}", witness.join(" -> "))
            }
            Self::Lexical {
                condition,
                facts,
                witness,
            } => {
                let mut rendered = format!("lexical {condition:?}");
                for fact in facts {
                    write!(rendered, " {}={}", fact.name, fact.value)
                        .expect("writing to a String cannot fail");
                }
                if let Some(witness) = witness {
                    write!(rendered, " witness={witness}")
                        .expect("writing to a String cannot fail");
                }
                rendered
            }
            Self::Analysis {
                overlap,
                witness,
                cycle,
                configured_lookahead,
                sufficient_lookahead,
                end_of_alternative,
            } => format!(
                "analysis overlap=[{}] witness=[{}] cycle=[{}] lookahead={:?}->{:?} end={end_of_alternative}",
                overlap.join(","),
                witness.join(" -> "),
                cycle.join(" -> "),
                configured_lookahead,
                sufficient_lookahead,
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
/// Structured corrective action associated with a diagnostic contract.
pub enum DiagnosticRemedy {
    /// Directs the author to correct invalid source structure.
    CorrectSource,
    /// Directs the author to repair a missing or incompatible reference.
    CorrectReference,
    /// Directs the author to consume input before recursive parsing.
    BreakRecursion {
        /// Recursive semantic path that must be interrupted.
        cycle: Vec<String>,
    },
    /// Requests distinct prefixes for competing alternatives.
    DisambiguateAlternatives {
        /// First alternative identity to make distinct.
        left: String,
        /// Conflicting alternative identity to make distinct.
        right: String,
    },
    /// Requests factoring or restructuring a shared prefix.
    RestructureSharedPrefix {
        /// First declaration sharing the prefix.
        left: String,
        /// Second declaration sharing the prefix.
        right: String,
    },
    /// Requests distinct prefixes for competing Pratt variants.
    DisambiguatePrattVariants {
        /// Pratt Node owning the conflicting variants.
        pratt: String,
        /// First conflicting Pratt variant.
        left: String,
        /// Second conflicting Pratt variant.
        right: String,
    },
    /// Requests restructuring a shared Pratt prefix.
    RestructurePrattSharedPrefix {
        /// Pratt Node whose variants share the prefix.
        pratt: String,
        /// First Pratt variant sharing the prefix.
        left: String,
        /// Second Pratt variant sharing the prefix.
        right: String,
    },
    /// Requests a configuration supported by the named capability.
    SelectSupportedConfiguration,
    /// Requests distinct precedence for overlapping lexical rules.
    AssignDistinctPriority,
    /// Directs the user to report an internal invariant failure.
    ReportInternalFailure,
    /// Suggests a larger lookahead bound when it resolves the witness.
    RaiseMaxLookahead {
        /// Current grammar maximum lookahead.
        configured: usize,
        /// Smallest proven separating lookahead.
        sufficient: usize,
    },
    /// Requests removing nullability at the affected use site.
    MakeSiteRequired {
        /// Nullable grammar site to make required.
        site: String,
    },
    /// Requests separating a nullable branch from its FOLLOW context.
    ReviewNullableFollow {
        /// Nullable site whose caller FOLLOW must be reviewed.
        site: String,
    },
    /// Requests separating Pratt continuation from caller FOLLOW.
    ReviewPrattFollow {
        /// Pratt site whose caller FOLLOW overlaps active extenders.
        site: String,
        /// Minimum binding power currently admitted at the Pratt site.
        min_bp: u8,
    },
    /// Requests the mechanically derived class for fixed syntax.
    UseMechanicalFixedTokenClass {
        /// Mechanically derived fixed-token class to use.
        class: String,
    },
    /// Requests identical metadata for equivalent token definitions.
    MakeDefinitionMetadataEquivalent {
        /// Token-definition identity whose declarations must agree.
        identity: String,
    },
    /// Requests a supported token pattern that cannot match empty input.
    UseSupportedNonEmptyPattern,
    /// Requests lexical patterns that cannot match the same input.
    MakePatternsDisjoint,
}

impl DiagnosticRemedy {
    /// Renders stable, actionable remedy text.
    pub(crate) fn canonical_text(&self) -> String {
        match self {
            Self::CorrectSource => "correct the authored grammar source".to_owned(),
            Self::CorrectReference => {
                "resolve the grammar-semantic reference to a supported target".to_owned()
            }
            Self::BreakRecursion { cycle } => {
                format!(
                    "break the unguarded recursive grammar path {}",
                    cycle.join(" -> ")
                )
            }
            Self::DisambiguateAlternatives { left, right } => {
                format!("make alternatives {left} and {right} distinguishable")
            }
            Self::RestructureSharedPrefix { left, right } => {
                format!("restructure alternatives {left} and {right} into a shared-prefix type")
            }
            Self::DisambiguatePrattVariants { pratt, left, right } => {
                format!("make Pratt variants {left} and {right} of {pratt} distinguishable")
            }
            Self::RestructurePrattSharedPrefix { pratt, left, right } => format!(
                "restructure Pratt variants {left} and {right} of {pratt} into a shared-prefix type"
            ),
            Self::SelectSupportedConfiguration => {
                "select one supported build configuration".to_owned()
            }
            Self::AssignDistinctPriority => {
                "assign distinct authored lexical priorities".to_owned()
            }
            Self::ReportInternalFailure => "report this Recursa invariant failure".to_owned(),
            Self::RaiseMaxLookahead {
                configured,
                sufficient,
            } => {
                format!("raise max_lookahead from {configured} to {sufficient}")
            }
            Self::MakeSiteRequired { site } => {
                format!("make nullable grammar site {site} required")
            }
            Self::ReviewNullableFollow { site } => {
                format!("make site {site} required or separate it from caller FOLLOW")
            }
            Self::ReviewPrattFollow { site, min_bp } => format!(
                "separate caller FOLLOW at {site} from Pratt extenders active at minimum binding power {min_bp}"
            ),
            Self::UseMechanicalFixedTokenClass { class } => {
                format!("use the mechanically derived fixed-token class {class}")
            }
            Self::MakeDefinitionMetadataEquivalent { identity } => {
                format!("make every definition of {identity} semantically equivalent")
            }
            Self::UseSupportedNonEmptyPattern => {
                "use a supported non-empty lexical pattern".to_owned()
            }
            Self::MakePatternsDisjoint => "make the lexical patterns disjoint".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One stable-coded structured finding produced by generation or a structural loader.
pub struct RecursaDiagnostic {
    /// Pipeline owner fixed by the code registry.
    stage: DiagnosticStage,
    /// Stable externally rendered diagnostic identifier.
    code: RecursaDiagnosticCode,
    /// Blocking or advisory classification validated against the registry.
    severity: Severity,
    /// Concise user-facing description of the failure.
    summary: String,
    /// Principal source label selected by the code's location rule.
    primary: RecursaDiagnosticLabel,
    /// Corroborating labels in deterministic source order.
    secondary: Vec<RecursaDiagnosticLabel>,
    /// Structured facts required by the code's evidence schema.
    evidence: DiagnosticEvidence,
    /// Valid corrective actions for the registered remedy schema.
    remedies: Vec<DiagnosticRemedy>,
    /// Additional non-remedial user guidance.
    notes: Vec<String>,
    /// Optional authored-order key used before location tie-breaks.
    source_order: Option<u64>,
    /// Semantic identity used to deduplicate equivalent findings.
    origin_key: String,
}

impl RecursaDiagnostic {
    #[cfg(test)]
    pub(crate) fn new(
        stage: DiagnosticStage,
        code: RecursaDiagnosticCode,
        severity: Severity,
        summary: impl Into<String>,
        location: DiagnosticLocation,
        evidence: DiagnosticEvidence,
        remedies: Vec<DiagnosticRemedy>,
    ) -> Self {
        let summary = summary.into();
        Self {
            stage,
            code,
            severity,
            primary: RecursaDiagnosticLabel::located(location, summary.clone()),
            summary,
            secondary: Vec::new(),
            evidence,
            remedies,
            notes: Vec::new(),
            source_order: None,
            origin_key: String::new(),
        }
    }

    /// Attaches primary evidence while preserving the diagnostic code contract.
    pub(crate) fn with_primary(
        stage: DiagnosticStage,
        code: RecursaDiagnosticCode,
        severity: Severity,
        summary: impl Into<String>,
        primary: RecursaDiagnosticLabel,
        evidence: DiagnosticEvidence,
        remedies: Vec<DiagnosticRemedy>,
    ) -> Self {
        Self {
            stage,
            code,
            severity,
            summary: summary.into(),
            primary,
            secondary: Vec::new(),
            evidence,
            remedies,
            notes: Vec::new(),
            source_order: None,
            origin_key: String::new(),
        }
    }

    /// Returns this value with secondary attached.
    pub(crate) fn with_secondary(mut self, secondary: Vec<RecursaDiagnosticLabel>) -> Self {
        self.secondary = secondary;
        self
    }

    /// Returns this value with remedies attached.
    pub(crate) fn with_remedies(mut self, remedies: Vec<DiagnosticRemedy>) -> Self {
        self.remedies = remedies;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }

    /// Returns this value with order attached.
    pub(crate) fn with_order(mut self, source_order: u64, origin_key: impl Into<String>) -> Self {
        self.source_order = Some(source_order);
        self.origin_key = origin_key.into();
        self
    }

    /// Returns the pipeline stage that detected the failure.
    pub const fn stage(&self) -> DiagnosticStage {
        self.stage
    }

    /// Returns the stable machine-readable diagnostic code.
    pub const fn code(&self) -> RecursaDiagnosticCode {
        self.code
    }

    /// Returns whether the diagnostic blocks generation.
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Returns the concise user-facing failure summary.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Returns the primary evidence selected for display.
    pub const fn primary(&self) -> &RecursaDiagnosticLabel {
        &self.primary
    }

    /// Returns additional ordered evidence ranges.
    pub fn secondary(&self) -> &[RecursaDiagnosticLabel] {
        &self.secondary
    }

    /// Returns all structured evidence in deterministic order.
    pub const fn evidence(&self) -> &DiagnosticEvidence {
        &self.evidence
    }

    /// Returns the actionable remedies permitted by the code contract.
    pub fn remedies(&self) -> &[DiagnosticRemedy] {
        &self.remedies
    }

    /// Returns supplemental explanatory notes.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Returns the canonical key used to order diagnostics.
    fn sort_key(&self) -> DiagnosticSortKey<'_> {
        let (location_class, path, range) = match self.primary.location() {
            Some(location) if location.range.is_some() => (
                0_u8,
                Some(location.path.as_path()),
                location
                    .range
                    .as_ref()
                    .map(|range| (range.start, range.end)),
            ),
            Some(location) => (1, Some(location.path.as_path()), None),
            None => (2, None, None),
        };
        DiagnosticSortKey {
            stage: self.stage,
            source_order: self.source_order.unwrap_or(u64::MAX),
            location_class,
            path,
            range,
            severity: self.severity,
            code: self.code,
            origin_key: &self.origin_key,
            evidence: &self.evidence,
        }
    }

    /// Returns the stable deduplication identity.
    fn identity(&self) -> DiagnosticIdentity {
        let (path, range, line_columns) =
            self.primary
                .location()
                .map_or((None, None, None), |location| {
                    (
                        Some(location.path.clone()),
                        location
                            .range
                            .as_ref()
                            .map(|range| (range.start, range.end)),
                        location.line_columns.map(|(start, end)| {
                            ((start.line, start.column), (end.line, end.column))
                        }),
                    )
                });
        DiagnosticIdentity {
            stage: self.stage,
            code: self.code,
            origin_key: self.origin_key.clone(),
            path,
            range,
            line_columns,
            evidence: self.evidence.clone(),
        }
    }

    /// Returns the key governing deterministic presentation order.
    fn display_key(&self) -> DiagnosticDisplayKey {
        DiagnosticDisplayKey {
            summary: self.summary.clone(),
            primary_message: self.primary.message.clone(),
            secondary: self.secondary.iter().map(label_display_key).collect(),
            remedies: self.remedies.clone(),
            notes: self.notes.clone(),
        }
    }

    /// Reports any mismatch between this diagnostic and its registry contract.
    fn contract_violation(&self) -> Option<String> {
        let spec = diagnostic_code_spec(self.code);
        if self.stage != spec.stage || self.severity != spec.severity {
            return Some("stage or severity disagrees with the diagnostic registry".to_owned());
        }
        if spec.primary_location == PrimaryLocationRule::SelectedRootFallback
            && self.primary.location().is_none()
        {
            return Some(
                "diagnostic requires the validated selected-root fallback location".to_owned(),
            );
        }
        let evidence_matches = match (spec.evidence, &self.evidence) {
            (
                DiagnosticEvidenceSchema::SourceFacts,
                DiagnosticEvidence::SourceFacts(_) | DiagnosticEvidence::SourcePaths(_),
            )
            | (
                DiagnosticEvidenceSchema::AnalysisWitness,
                DiagnosticEvidence::Analysis { .. } | DiagnosticEvidence::SemanticWitness { .. },
            )
            | (
                DiagnosticEvidenceSchema::InternalInvariant,
                DiagnosticEvidence::InternalInvariant { .. },
            ) => true,
            (
                DiagnosticEvidenceSchema::LexicalFacts(expected),
                DiagnosticEvidence::Lexical { condition, .. },
            ) => expected == *condition,
            _ => false,
        };
        if !evidence_matches {
            return Some("evidence disagrees with the per-code registry schema".to_owned());
        }
        if self.remedies.is_empty()
            || !self
                .remedies
                .iter()
                .all(|remedy| remedy_matches_schema(spec.remedy, remedy))
        {
            return Some("remedy disagrees with the per-code registry schema".to_owned());
        }
        None
    }

    /// Converts a registry-contract violation into an internal diagnostic.
    fn internal_contract_failure(self, detail: String) -> Self {
        let (stage, code) = match self.stage {
            DiagnosticStage::Discovery => (self.stage, RecursaDiagnosticCode::new("RCA9101")),
            DiagnosticStage::Resolution => (self.stage, RecursaDiagnosticCode::new("RCA9102")),
            DiagnosticStage::Lowering => (self.stage, RecursaDiagnosticCode::new("RCA9103")),
            DiagnosticStage::Analysis => (self.stage, RecursaDiagnosticCode::new("RCA9104")),
            DiagnosticStage::Emission => (self.stage, RecursaDiagnosticCode::new("RCA9105")),
            DiagnosticStage::Loading => (
                DiagnosticStage::Analysis,
                RecursaDiagnosticCode::new("RCA9104"),
            ),
        };
        Self {
            stage,
            code,
            severity: Severity::Error,
            summary: "diagnostic producer violated its typed registry contract".to_owned(),
            primary: self.primary,
            secondary: self.secondary,
            evidence: DiagnosticEvidence::InternalInvariant {
                producer: self.code.as_str().to_owned(),
                detail,
            },
            remedies: vec![DiagnosticRemedy::ReportInternalFailure],
            notes: Vec::new(),
            source_order: self.source_order,
            origin_key: self.origin_key,
        }
    }
}

/// Performs the remedy matches schema phase of this boundary.
fn remedy_matches_schema(schema: DiagnosticRemedySchema, remedy: &DiagnosticRemedy) -> bool {
    match schema {
        DiagnosticRemedySchema::ReportInternalFailure => {
            matches!(remedy, DiagnosticRemedy::ReportInternalFailure)
        }
        DiagnosticRemedySchema::ResolveLexicalPolicy(_) => matches!(
            remedy,
            DiagnosticRemedy::SelectSupportedConfiguration
                | DiagnosticRemedy::AssignDistinctPriority
                | DiagnosticRemedy::UseMechanicalFixedTokenClass { .. }
                | DiagnosticRemedy::MakeDefinitionMetadataEquivalent { .. }
                | DiagnosticRemedy::UseSupportedNonEmptyPattern
                | DiagnosticRemedy::MakePatternsDisjoint
        ),
        DiagnosticRemedySchema::ResolveAnalysisConflict => matches!(
            remedy,
            DiagnosticRemedy::CorrectSource
                | DiagnosticRemedy::CorrectReference
                | DiagnosticRemedy::BreakRecursion { .. }
                | DiagnosticRemedy::DisambiguateAlternatives { .. }
                | DiagnosticRemedy::RestructureSharedPrefix { .. }
                | DiagnosticRemedy::DisambiguatePrattVariants { .. }
                | DiagnosticRemedy::RestructurePrattSharedPrefix { .. }
                | DiagnosticRemedy::RaiseMaxLookahead { .. }
                | DiagnosticRemedy::MakeSiteRequired { .. }
                | DiagnosticRemedy::ReviewNullableFollow { .. }
                | DiagnosticRemedy::ReviewPrattFollow { .. }
        ),
        DiagnosticRemedySchema::CorrectAuthoredInput => matches!(
            remedy,
            DiagnosticRemedy::CorrectSource
                | DiagnosticRemedy::CorrectReference
                | DiagnosticRemedy::SelectSupportedConfiguration
        ),
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
/// Canonical ordering key derived from code, location, and evidence identity.
struct DiagnosticSortKey<'a> {
    /// Pipeline-stage rank forming the first canonical sort component.
    stage: DiagnosticStage,
    /// Authored-order rank, with unlocated findings sorted last.
    source_order: u64,
    /// Rank distinguishing located and global findings.
    location_class: u8,
    /// Manifest-relative path sort component.
    path: Option<&'a std::path::Path>,
    /// Half-open byte-range sort component.
    range: Option<(u32, u32)>,
    /// Severity tie-break preserving deterministic display.
    severity: Severity,
    /// Stable-code tie-break preserving deterministic display.
    code: RecursaDiagnosticCode,
    /// Semantic-origin tie-break.
    origin_key: &'a str,
    /// Final structured-evidence tie-break.
    evidence: &'a DiagnosticEvidence,
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
/// Stable equality key used to deduplicate equivalent diagnostics.
struct DiagnosticIdentity {
    /// Pipeline owner participating in diagnostic deduplication.
    stage: DiagnosticStage,
    /// Stable code participating in diagnostic deduplication.
    code: RecursaDiagnosticCode,
    /// Semantic origin forming the primary deduplication identity.
    origin_key: String,
    /// Optional normalized source path distinguishing origins.
    path: Option<PathBuf>,
    /// Optional half-open byte range distinguishing origins.
    range: Option<(u32, u32)>,
    /// Optional rendered coordinates retained for exact identity.
    line_columns: Option<((u32, u32), (u32, u32))>,
    /// Structured proof facts distinguishing otherwise equal origins.
    evidence: DiagnosticEvidence,
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
/// Presentation key that keeps user-visible ordering deterministic.
struct DiagnosticDisplayKey {
    /// Summary used to choose deterministically among duplicate identities.
    summary: String,
    /// Principal label text used as a display tie-break.
    primary_message: String,
    /// Canonicalized secondary labels used as a display tie-break.
    secondary: Vec<DiagnosticLabelDisplayKey>,
    /// Corrective actions used as a display tie-break.
    remedies: Vec<DiagnosticRemedy>,
    /// Additional notes used as a display tie-break.
    notes: Vec<String>,
}

/// Names the DiagnosticLabelDisplayKey representation used by this boundary.
type DiagnosticLabelDisplayKey = (
    Option<PathBuf>,
    Option<(u32, u32)>,
    Option<((u32, u32), (u32, u32))>,
    String,
);

/// Performs the label display key phase of this boundary.
fn label_display_key(label: &RecursaDiagnosticLabel) -> DiagnosticLabelDisplayKey {
    let (path, range, line_columns) = label.location().map_or((None, None, None), |location| {
        (
            Some(location.path.clone()),
            location
                .range
                .as_ref()
                .map(|range| (range.start, range.end)),
            location
                .line_columns
                .map(|(start, end)| ((start.line, start.column), (end.line, end.column))),
        )
    });
    (path, range, line_columns, label.message.clone())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Deterministic, contract-validated collection of generation diagnostics.
///
/// Construction replaces malformed diagnostics with internal-invariant findings, deduplicates
/// equivalent origins, and sorts the survivors canonically. Generation may publish output only
/// when this report contains no errors; warnings remain attached to the successful generation.
pub struct BuildReport {
    /// Contract-valid, deduplicated findings in canonical display order.
    diagnostics: Vec<RecursaDiagnostic>,
}

impl BuildReport {
    /// Creates an accumulator that validates, deduplicates, and orders diagnostics.
    pub(crate) fn new(mut diagnostics: Vec<RecursaDiagnostic>) -> Self {
        diagnostics = diagnostics
            .into_iter()
            .map(|diagnostic| {
                if let Some(violation) = diagnostic.contract_violation() {
                    diagnostic.internal_contract_failure(violation)
                } else {
                    diagnostic
                }
            })
            .collect();
        let mut by_identity = BTreeMap::<DiagnosticIdentity, RecursaDiagnostic>::new();
        for diagnostic in diagnostics {
            by_identity
                .entry(diagnostic.identity())
                .and_modify(|representative| {
                    if diagnostic.display_key() < representative.display_key() {
                        representative.clone_from(&diagnostic);
                    }
                })
                .or_insert(diagnostic);
        }
        let mut diagnostics = by_identity.into_values().collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        Self { diagnostics }
    }

    /// Returns all findings in canonical presentation order.
    pub fn diagnostics(&self) -> &[RecursaDiagnostic] {
        &self.diagnostics
    }

    /// Iterates findings that prevent generation publication.
    pub fn errors(&self) -> impl Iterator<Item = &RecursaDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
    }

    /// Iterates non-blocking findings retained on a successful generation.
    pub fn warnings(&self) -> impl Iterator<Item = &RecursaDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Warning)
    }

    /// Renders the stable multiline plain-text diagnostic representation.
    pub fn canonical_rendering(&self) -> String {
        let mut rendered = String::new();
        for diagnostic in &self.diagnostics {
            let severity = match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            write!(
                rendered,
                "{severity}[{}] {}",
                diagnostic.code.as_str(),
                diagnostic.stage.canonical_name(),
            )
            .expect("writing to String cannot fail");
            if let Some(location) = diagnostic.primary.location() {
                write!(rendered, " at {}", location.path.display())
                    .expect("writing to String cannot fail");
                if let Some(range) = &location.range {
                    if let Some((start, end)) = location.line_columns {
                        write!(
                            rendered,
                            ":{}:{}..{}:{}",
                            start.line, start.column, end.line, end.column,
                        )
                        .expect("writing to String cannot fail");
                    } else {
                        write!(rendered, ":{}..{}", range.start, range.end)
                            .expect("writing to String cannot fail");
                    }
                }
            }
            writeln!(rendered, ": {}", diagnostic.summary).expect("writing to String cannot fail");
            for label in &diagnostic.secondary {
                write!(rendered, "  secondary").expect("writing to String cannot fail");
                if let Some(location) = label.location() {
                    write!(rendered, " at {}", location.path.display())
                        .expect("writing to String cannot fail");
                    if let Some(range) = &location.range {
                        if let Some((start, end)) = location.line_columns {
                            write!(
                                rendered,
                                ":{}:{}..{}:{}",
                                start.line, start.column, end.line, end.column,
                            )
                            .expect("writing to String cannot fail");
                        } else {
                            write!(rendered, ":{}..{}", range.start, range.end)
                                .expect("writing to String cannot fail");
                        }
                    }
                }
                writeln!(rendered, ": {}", label.message()).expect("writing to String cannot fail");
            }
            writeln!(
                rendered,
                "  evidence: {}",
                diagnostic.evidence.canonical_text()
            )
            .expect("writing to String cannot fail");
            for remedy in &diagnostic.remedies {
                writeln!(rendered, "  remedy: {}", remedy.canonical_text())
                    .expect("writing to String cannot fail");
            }
            for note in &diagnostic.notes {
                writeln!(rendered, "  note: {note}").expect("writing to String cannot fail");
            }
        }
        rendered
    }
}

/// Failed generation represented by its complete deterministic [`BuildReport`].
pub struct GenerateError {
    /// Complete error-bearing report that prevented publication.
    report: BuildReport,
}

impl GenerateError {
    /// Constructs a value after enforcing this boundary's invariants.
    pub(crate) const fn new(report: BuildReport) -> Self {
        Self { report }
    }

    /// Returns the report that prevented publication.
    pub const fn report(&self) -> &BuildReport {
        &self.report
    }

    /// Returns the report's diagnostics in canonical order.
    pub fn diagnostics(&self) -> &[RecursaDiagnostic] {
        self.report.diagnostics()
    }
}

impl std::fmt::Display for GenerateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.report.canonical_rendering())
    }
}

impl std::fmt::Debug for GenerateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for GenerateError {}
