use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use crate::Severity;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum DiagnosticStage {
    Discovery,
    Resolution,
    Lowering,
    Analysis,
    Emission,
    Loading,
}

impl DiagnosticStage {
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
pub(crate) struct DiagnosticCodeSpec {
    pub(crate) code: RecursaDiagnosticCode,
    pub(crate) stage: DiagnosticStage,
    pub(crate) severity: Severity,
    pub(crate) primary_location: PrimaryLocationRule,
    pub(crate) evidence: DiagnosticEvidenceSchema,
    pub(crate) remedy: DiagnosticRemedySchema,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrimaryLocationRule {
    FirstRelatedOrGlobal,
    SelectedRootFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticEvidenceSchema {
    SourceFacts,
    LexicalFacts(LexicalEvidenceClass),
    AnalysisWitness,
    InternalInvariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticRemedySchema {
    CorrectAuthoredInput,
    ResolveLexicalPolicy(LexicalEvidenceClass),
    ResolveAnalysisConflict,
    ReportInternalFailure,
}

const fn primary_location_rule(code: &str) -> PrimaryLocationRule {
    if code.as_bytes()[3] == b'8' || code.as_bytes()[3] == b'9' {
        PrimaryLocationRule::SelectedRootFallback
    } else {
        PrimaryLocationRule::FirstRelatedOrGlobal
    }
}

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

const fn diagnostic_number(code: &str) -> u16 {
    let bytes = code.as_bytes();
    ((bytes[3] - b'0') as u16) * 1000
        + ((bytes[4] - b'0') as u16) * 100
        + ((bytes[5] - b'0') as u16) * 10
        + (bytes[6] - b'0') as u16
}

macro_rules! diagnostic_spec {
    ($code:literal, $stage:ident, $severity:ident) => {
        DiagnosticCodeSpec {
            code: RecursaDiagnosticCode::new($code),
            stage: DiagnosticStage::$stage,
            severity: Severity::$severity,
            primary_location: primary_location_rule($code),
            evidence: evidence_schema($code, DiagnosticStage::$stage),
            remedy: remedy_schema($code, DiagnosticStage::$stage),
        }
    };
}

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
    diagnostic_spec!("RCA3117", Lowering, Error),
    diagnostic_spec!("RCA3118", Lowering, Error),
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

pub(crate) fn diagnostic_code_registry() -> &'static [DiagnosticCodeSpec] {
    DIAGNOSTIC_CODE_REGISTRY
}

pub(crate) fn diagnostic_code_spec(code: RecursaDiagnosticCode) -> &'static DiagnosticCodeSpec {
    diagnostic_code_registry()
        .iter()
        .find(|spec| spec.code == code)
        .expect("every emitted Recursa diagnostic code is registered")
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecursaDiagnosticCode(&'static str);

impl RecursaDiagnosticCode {
    pub(crate) const fn new(code: &'static str) -> Self {
        Self(code)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticLocation {
    path: PathBuf,
    range: Option<Range<u32>>,
    line_columns: Option<(LineColumn, LineColumn)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineColumn {
    pub line: u32,
    pub column: u32,
}

impl DiagnosticLocation {
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

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub const fn range(&self) -> Option<&Range<u32>> {
        self.range.as_ref()
    }

    pub const fn line_columns(&self) -> Option<(LineColumn, LineColumn)> {
        self.line_columns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticLocationInvariant {
    NonManifestRelativePath,
    ReversedRange,
    InvalidUtf8Boundary,
}

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
pub struct RecursaDiagnosticLabel {
    location: Option<DiagnosticLocation>,
    message: String,
}

impl RecursaDiagnosticLabel {
    pub fn located(location: DiagnosticLocation, message: impl Into<String>) -> Self {
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

    pub const fn location(&self) -> Option<&DiagnosticLocation> {
        self.location.as_ref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum DiagnosticEvidence {
    Subject(String),
    SourcePaths(Vec<PathBuf>),
    SourceFacts(Vec<DiagnosticFact>),
    InternalInvariant {
        producer: String,
        detail: String,
    },
    SemanticWitness {
        subject: String,
        witness: Vec<String>,
    },
    Lexical {
        condition: LexicalEvidenceClass,
        facts: Vec<DiagnosticFact>,
        witness: Option<String>,
    },
    Analysis {
        overlap: Vec<String>,
        witness: Vec<String>,
        cycle: Vec<String>,
        configured_lookahead: Option<usize>,
        sufficient_lookahead: Option<usize>,
        end_of_alternative: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LexicalEvidenceClass {
    KeywordMatchingPolicy,
    FixedTokenClassification,
    ConflictingDefinitionMetadata,
    InvalidPattern,
    PatternOverlap,
    PriorityTie,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticFact {
    pub name: String,
    pub value: String,
}

impl DiagnosticEvidence {
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
pub enum DiagnosticRemedy {
    CorrectSource,
    CorrectReference,
    BreakRecursion {
        cycle: Vec<String>,
    },
    DisambiguateAlternatives {
        left: String,
        right: String,
    },
    RestructureSharedPrefix {
        left: String,
        right: String,
    },
    DisambiguatePrattVariants {
        pratt: String,
        left: String,
        right: String,
    },
    RestructurePrattSharedPrefix {
        pratt: String,
        left: String,
        right: String,
    },
    SelectSupportedConfiguration,
    AssignDistinctPriority,
    ReportInternalFailure,
    RaiseMaxLookahead {
        configured: usize,
        sufficient: usize,
    },
    MakeSiteRequired {
        site: String,
    },
    ReviewNullableFollow {
        site: String,
    },
    ReviewPrattFollow {
        site: String,
        min_bp: u8,
    },
    UseMechanicalFixedTokenClass {
        class: String,
    },
    MakeDefinitionMetadataEquivalent {
        identity: String,
    },
    UseSupportedNonEmptyPattern,
    MakePatternsDisjoint,
}

impl DiagnosticRemedy {
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
pub struct RecursaDiagnostic {
    stage: DiagnosticStage,
    code: RecursaDiagnosticCode,
    severity: Severity,
    summary: String,
    primary: RecursaDiagnosticLabel,
    secondary: Vec<RecursaDiagnosticLabel>,
    evidence: DiagnosticEvidence,
    remedies: Vec<DiagnosticRemedy>,
    notes: Vec<String>,
    source_order: Option<u64>,
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

    pub(crate) fn with_secondary(mut self, secondary: Vec<RecursaDiagnosticLabel>) -> Self {
        self.secondary = secondary;
        self
    }

    pub(crate) fn with_remedies(mut self, remedies: Vec<DiagnosticRemedy>) -> Self {
        self.remedies = remedies;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }

    pub(crate) fn with_order(mut self, source_order: u64, origin_key: impl Into<String>) -> Self {
        self.source_order = Some(source_order);
        self.origin_key = origin_key.into();
        self
    }

    pub const fn stage(&self) -> DiagnosticStage {
        self.stage
    }

    pub const fn code(&self) -> RecursaDiagnosticCode {
        self.code
    }

    pub const fn severity(&self) -> Severity {
        self.severity
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub const fn primary(&self) -> &RecursaDiagnosticLabel {
        &self.primary
    }

    pub fn secondary(&self) -> &[RecursaDiagnosticLabel] {
        &self.secondary
    }

    pub const fn evidence(&self) -> &DiagnosticEvidence {
        &self.evidence
    }

    pub fn remedies(&self) -> &[DiagnosticRemedy] {
        &self.remedies
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

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

    fn display_key(&self) -> DiagnosticDisplayKey {
        DiagnosticDisplayKey {
            summary: self.summary.clone(),
            primary_message: self.primary.message.clone(),
            secondary: self.secondary.iter().map(label_display_key).collect(),
            remedies: self.remedies.clone(),
            notes: self.notes.clone(),
        }
    }

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
struct DiagnosticSortKey<'a> {
    stage: DiagnosticStage,
    source_order: u64,
    location_class: u8,
    path: Option<&'a std::path::Path>,
    range: Option<(u32, u32)>,
    severity: Severity,
    code: RecursaDiagnosticCode,
    origin_key: &'a str,
    evidence: &'a DiagnosticEvidence,
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct DiagnosticIdentity {
    stage: DiagnosticStage,
    code: RecursaDiagnosticCode,
    origin_key: String,
    path: Option<PathBuf>,
    range: Option<(u32, u32)>,
    line_columns: Option<((u32, u32), (u32, u32))>,
    evidence: DiagnosticEvidence,
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct DiagnosticDisplayKey {
    summary: String,
    primary_message: String,
    secondary: Vec<DiagnosticLabelDisplayKey>,
    remedies: Vec<DiagnosticRemedy>,
    notes: Vec<String>,
}

type DiagnosticLabelDisplayKey = (
    Option<PathBuf>,
    Option<(u32, u32)>,
    Option<((u32, u32), (u32, u32))>,
    String,
);

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
pub struct BuildReport {
    diagnostics: Vec<RecursaDiagnostic>,
}

impl BuildReport {
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

    pub fn diagnostics(&self) -> &[RecursaDiagnostic] {
        &self.diagnostics
    }

    pub fn errors(&self) -> impl Iterator<Item = &RecursaDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &RecursaDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Warning)
    }

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

pub struct GenerateError {
    report: BuildReport,
}

impl GenerateError {
    pub(crate) const fn new(report: BuildReport) -> Self {
        Self { report }
    }

    pub const fn report(&self) -> &BuildReport {
        &self.report
    }

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
