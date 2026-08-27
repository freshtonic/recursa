#![allow(
    dead_code,
    reason = "the private source-analysis seam is consumed by the generation pipeline"
)]
#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Orchestration and diagnostic adaptation for the source-analysis pipeline.
//!
//! Each stage consumes only the preceding stage's successful value while
//! preserving its build inputs on failure. Stage-specific findings are
//! adapted once into stable build diagnostics and Cargo invalidation output is
//! emitted independently of analysis success.

use std::path::Path;

use crate::build_diagnostic::diagnostic_code_spec;
use crate::discovery::{
    DiscoveryDiagnostic, DiscoveryDiagnosticKind, DiscoveryInput, discover_module_tree,
};
use crate::lexical::{
    FixedTokenClass, LexicalDefinitionMetadata, LexicalDiagnostic, LexicalDiagnosticCode,
    LexicalDiagnosticEvidence, LexicalPatternEvidence, LexicalRemedy,
};
use crate::lowering::{DiscoveryInputs, RustEdition};
use crate::materialization::{
    LoweredGrammar, MaterializationDiagnostic, MaterializationDiagnosticKind, materialize,
};
use crate::resolution::{
    ResolutionDiagnostic, ResolutionDiagnosticKind, ResolutionInput, resolve_local_semantics,
};
use crate::source_model::{
    SourceModelDiagnostic, SourceModelDiagnosticKind, SourceModelInput, SourceSite,
    lower_source_model,
};
use crate::source_syntax::CargoCfgSnapshot;
use crate::{
    AnalysisReport, BuildReport, ConflictRemedy, DiagnosticEvidence, DiagnosticFact,
    DiagnosticLocation, DiagnosticRemedy, DiagnosticStage, GenerateError, LexicalEvidenceClass,
    RecursaDiagnostic, RecursaDiagnosticCode, RecursaDiagnosticLabel, Remedy, Severity, analyze,
};

#[derive(Clone, Debug)]
/// Borrowed invocation inputs for one source-analysis run.
pub(crate) struct SourceAnalysisInput<'input> {
    /// Package root against which selected and discovered paths are confined.
    manifest_dir: &'input Path,
    /// Authored Rust file selected as the grammar root.
    selected_root: &'input Path,
    /// Rust edition used to parse the selected module tree.
    edition: RustEdition,
    /// Cargo configuration snapshot controlling module inclusion.
    cfg: CargoCfgSnapshot,
}

impl<'input> SourceAnalysisInput<'input> {
    /// Creates default Rust 2024 inputs for tests and fixed fixtures.
    pub(crate) fn rust_2024(manifest_dir: &'input Path, selected_root: &'input str) -> Self {
        Self {
            manifest_dir,
            selected_root: Path::new(selected_root),
            edition: RustEdition::Rust2024,
            cfg: CargoCfgSnapshot::default(),
        }
    }

    /// Creates a value with the supplied analysis inputs.
    pub(crate) fn new(
        manifest_dir: &'input Path,
        selected_root: &'input Path,
        edition: RustEdition,
        cfg: CargoCfgSnapshot,
    ) -> Self {
        Self {
            manifest_dir,
            selected_root,
            edition,
            cfg,
        }
    }
}

/// Private sink for line-oriented Cargo build-script instructions.
///
/// Source analysis emits invalidation before it can fail and warnings exactly
/// once at the boundary where their stage is known.
pub(crate) trait CargoOutput {
    /// Emits one complete Cargo instruction without an ending newline.
    fn emit(&mut self, instruction: String);
}

impl CargoOutput for Vec<String> {
    fn emit(&mut self, instruction: String) {
        self.push(instruction);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A validated lowered grammar paired with successful semantic facts.
pub(crate) struct AnalyzedGrammar {
    /// Authored and normalized structures with total correlations.
    pub(crate) lowered: LoweredGrammar,
    /// Successful source-independent semantic result.
    pub(crate) analysis: AnalysisReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Successful pipeline value returned to generation.
pub(crate) struct SourceAnalysisSuccess {
    /// Grammar and semantic facts generation may join into an emission plan.
    pub(crate) analyzed: AnalyzedGrammar,
    /// Non-fatal warnings retained for programmatic consumers.
    pub(crate) build_report: BuildReport,
}

/// Analyzes one selected Rust grammar tree through all dependent stages.
///
/// The first failing stage returns every independent error from that stage and
/// suppresses dependent stages. Cargo invalidation instructions survive all
/// failure paths, and successful warnings are emitted once before return.
pub(crate) fn analyze_source_tree(
    input: SourceAnalysisInput<'_>,
    cargo: &mut impl CargoOutput,
) -> Result<SourceAnalysisSuccess, GenerateError> {
    // Run discovery, resolution, modeling, materialization, lexical planning, lowering, and semantic analysis in dependency order.
    let mut collector = DiagnosticCollector::default();
    let discovery = discover_module_tree(DiscoveryInput {
        manifest_dir: input.manifest_dir,
        selected_root: input.selected_root,
        edition: input.edition,
        cfg: &input.cfg,
    });
    emit_invalidation(&discovery.discovery_inputs, cargo);
    collector.extend(
        discovery
            .diagnostics
            .iter()
            .map(|diagnostic| adapt_discovery(diagnostic, &discovery.discovery_inputs)),
    );
    collector.stop_if_errors(cargo)?;
    let Some(tree) = discovery.tree else {
        return Err(collector.internal_failure(
            DiagnosticStage::Discovery,
            "discovery produced neither diagnostics nor a selected module tree",
            &discovery.discovery_inputs.selected_root,
            cargo,
        ));
    };

    let resolution = resolve_local_semantics(ResolutionInput {
        discovery_inputs: discovery.discovery_inputs,
        tree,
        cfg: &input.cfg,
    });
    collector.extend(
        resolution
            .diagnostics
            .iter()
            .map(|diagnostic| adapt_resolution(diagnostic, &resolution.discovery_inputs)),
    );
    collector.stop_if_errors(cargo)?;
    let Some(resolved) = resolution.resolved else {
        return Err(collector.internal_failure(
            DiagnosticStage::Resolution,
            "resolution produced neither diagnostics nor a resolved source grammar",
            &resolution.discovery_inputs.selected_root,
            cargo,
        ));
    };

    let source_model = lower_source_model(SourceModelInput {
        resolved,
        cfg: &input.cfg,
    });
    collector.extend(
        source_model
            .diagnostics
            .iter()
            .map(|diagnostic| adapt_source_model(diagnostic, input.selected_root)),
    );
    collector.stop_if_errors(cargo)?;
    let Some(source_modeled) = source_model.source_modeled else {
        return Err(collector.internal_failure(
            DiagnosticStage::Lowering,
            "source lowering produced neither diagnostics nor a Source model",
            input.selected_root,
            cargo,
        ));
    };

    let materialization = materialize(source_modeled);
    collector.extend(
        materialization
            .lexical_diagnostics
            .iter()
            .map(|diagnostic| adapt_lexical(diagnostic, input.selected_root)),
    );
    collector.extend(
        materialization
            .diagnostics
            .iter()
            .map(|diagnostic| adapt_materialization(diagnostic, input.selected_root)),
    );
    collector.stop_if_errors(cargo)?;
    let Some(lowered) = materialization.lowered else {
        return Err(collector.internal_failure(
            DiagnosticStage::Lowering,
            "materialization produced neither diagnostics nor a Lowered grammar",
            input.selected_root,
            cargo,
        ));
    };

    let analysis = analyze(&lowered.grammar);
    collector.extend(
        analysis
            .diagnostics
            .iter()
            .map(|diagnostic| adapt_analysis(diagnostic, &analysis, &lowered, input.selected_root)),
    );
    collector.stop_if_errors(cargo)?;
    let Some(output) = analysis.output.as_ref() else {
        return Err(collector.internal_failure(
            DiagnosticStage::Analysis,
            "semantic analysis produced neither errors nor semantic analysis output",
            input.selected_root,
            cargo,
        ));
    };
    if let Err(invariant) = output
        .decisions()
        .validate(&lowered.grammar, output.first_by_type())
    {
        return Err(collector.internal_failure(
            DiagnosticStage::Analysis,
            &format!("invalid frozen decision product: {invariant}"),
            input.selected_root,
            cargo,
        ));
    }
    let build_report = collector.finish();
    emit_warnings(&build_report, cargo);
    Ok(SourceAnalysisSuccess {
        analyzed: AnalyzedGrammar { lowered, analysis },
        build_report,
    })
}

#[derive(Default)]
/// Stage-aware accumulator enforcing earliest-failure diagnostic policy.
struct DiagnosticCollector {
    /// The diagnostics.
    diagnostics: Vec<RecursaDiagnostic>,
}

impl DiagnosticCollector {
    /// Adds independently produced diagnostics without rendering them.
    fn extend(&mut self, diagnostics: impl IntoIterator<Item = RecursaDiagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    /// Stops at the earliest failing stage and emits its retained warnings.
    fn stop_if_errors(&mut self, cargo: &mut impl CargoOutput) -> Result<(), GenerateError> {
        let Some(earliest) = self
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity() == Severity::Error)
            .map(RecursaDiagnostic::stage)
            .min()
        else {
            return Ok(());
        };
        self.diagnostics.retain(|diagnostic| {
            (diagnostic.severity() == Severity::Warning && diagnostic.stage() <= earliest)
                || (diagnostic.severity() == Severity::Error && diagnostic.stage() == earliest)
        });
        let report = self.take_report();
        emit_warnings(&report, cargo);
        Err(GenerateError::new(report))
    }

    /// Converts a violated pipeline invariant into the stage's internal code.
    fn internal_failure(
        &mut self,
        stage: DiagnosticStage,
        summary: &str,
        path: &Path,
        cargo: &mut impl CargoOutput,
    ) -> GenerateError {
        self.diagnostics
            .push(internal_diagnostic(stage, summary, path));
        let report = self.take_report();
        emit_warnings(&report, cargo);
        GenerateError::new(report)
    }

    /// Moves accumulated diagnostics into a canonically ordered report.
    fn take_report(&mut self) -> BuildReport {
        BuildReport::new(std::mem::take(&mut self.diagnostics))
    }

    /// Finishes a successful report without discarding warnings.
    fn finish(self) -> BuildReport {
        BuildReport::new(self.diagnostics)
    }
}

/// Returns the reserved internal-invariant code for a pipeline stage.
fn internal_code(stage: DiagnosticStage) -> &'static str {
    match stage {
        DiagnosticStage::Discovery => "RCA9101",
        DiagnosticStage::Resolution => "RCA9102",
        DiagnosticStage::Lowering => "RCA9103",
        DiagnosticStage::Analysis => "RCA9104",
        DiagnosticStage::Emission => "RCA9105",
        DiagnosticStage::Loading => "RCA9104",
    }
}

#[allow(
    clippy::manual_ok_err,
    reason = "this is the explicit last-resort global fallback after selected-root validation fails"
)]
/// Computes the internal diagnostic.
fn internal_diagnostic(
    stage: DiagnosticStage,
    detail: &str,
    selected_root: &Path,
) -> RecursaDiagnostic {
    let location = match DiagnosticLocation::new(selected_root.to_owned(), None) {
        Ok(location) => Some(location),
        Err(_) => None,
    };
    make_diagnostic(
        internal_code(stage),
        "Recursa detected an internal generation invariant failure",
        location,
        DiagnosticEvidence::InternalInvariant {
            producer: format!("{stage:?}"),
            detail: detail.to_owned(),
        },
        DiagnosticRemedy::ReportInternalFailure,
        None,
    )
}

/// Computes the internal generate error.
pub(crate) fn internal_generate_error(
    stage: DiagnosticStage,
    detail: &str,
    selected_root: &Path,
) -> GenerateError {
    GenerateError::new(BuildReport::new(vec![internal_diagnostic(
        stage,
        detail,
        selected_root,
    )]))
}

/// Computes the checked location.
pub(crate) fn checked_location(
    stage: DiagnosticStage,
    path: &Path,
    range: Option<std::ops::Range<u32>>,
    selected_root: &Path,
) -> Result<DiagnosticLocation, Box<RecursaDiagnostic>> {
    DiagnosticLocation::new(path.to_owned(), range).map_err(|invariant| {
        Box::new(internal_diagnostic(
            stage,
            &format!("diagnostic producer supplied an invalid source location: {invariant:?}"),
            selected_root,
        ))
    })
}

/// Emits invalidation.
fn emit_invalidation(inputs: &DiscoveryInputs, cargo: &mut impl CargoOutput) {
    for path in inputs.watched_paths() {
        cargo.emit(format!("cargo::rerun-if-changed={}", path.display()));
    }
    let mut environment = inputs.environment_dependencies.clone();
    environment.sort();
    environment.dedup();
    for variable in environment {
        cargo.emit(format!("cargo::rerun-if-env-changed={variable}"));
    }
}

/// Emits warnings.
pub(crate) fn emit_warnings(report: &BuildReport, cargo: &mut impl CargoOutput) {
    for warning in report.warnings() {
        let location = cargo_one_line(&warning.primary().location().map_or_else(
            || "<global>".to_owned(),
            |location| location.path().display().to_string(),
        ));
        let evidence = cargo_one_line(&warning.evidence().canonical_text());
        let remedy = cargo_one_line(&warning.remedies().first().map_or_else(
            || "review this diagnostic".to_owned(),
            |remedy| remedy.canonical_text(),
        ));
        cargo.emit(format!(
            "cargo::warning=[{}] {location}: {}; evidence: {}; remedy: {}",
            warning.code().as_str(),
            cargo_one_line(warning.summary()),
            evidence,
            remedy,
        ));
    }
}

/// Computes the cargo one line.
fn cargo_one_line(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\r' | '\n' => ' ',
            other => other,
        })
        .collect()
}

/// Constructs diagnostic.
fn make_diagnostic(
    code: &'static str,
    summary: &str,
    location: Option<DiagnosticLocation>,
    evidence: DiagnosticEvidence,
    remedy: DiagnosticRemedy,
    order: Option<(u64, String)>,
) -> RecursaDiagnostic {
    let code = RecursaDiagnosticCode::new(code);
    let spec = diagnostic_code_spec(code);
    let primary = location.map_or_else(
        || RecursaDiagnosticLabel::unlocated(summary),
        |location| RecursaDiagnosticLabel::located(location, summary),
    );
    let mut diagnostic = RecursaDiagnostic::with_primary(
        spec.stage,
        code,
        spec.severity,
        summary,
        primary,
        evidence,
        vec![remedy],
    );
    if let Some((source_order, origin_key)) = order {
        diagnostic = diagnostic.with_order(source_order, origin_key);
    }
    diagnostic
}

/// Adapts discovery.
fn adapt_discovery(
    diagnostic: &DiscoveryDiagnostic,
    inputs: &DiscoveryInputs,
) -> RecursaDiagnostic {
    let code = match diagnostic.kind {
        DiscoveryDiagnosticKind::InvalidManifest => "RCA1001",
        DiscoveryDiagnosticKind::AbsolutePath => "RCA1002",
        DiscoveryDiagnosticKind::PackageEscape => "RCA1003",
        DiscoveryDiagnosticKind::SymlinkPath => "RCA1004",
        DiscoveryDiagnosticKind::MissingRoot => "RCA1005",
        DiscoveryDiagnosticKind::UnreadableSource => "RCA1006",
        DiscoveryDiagnosticKind::RustSyntax => "RCA2001",
        DiscoveryDiagnosticKind::MissingModule => "RCA1007",
        DiscoveryDiagnosticKind::AmbiguousModule => "RCA1008",
        DiscoveryDiagnosticKind::InvalidModulePath => "RCA1009",
        DiscoveryDiagnosticKind::MultipleModulePaths => "RCA1010",
        DiscoveryDiagnosticKind::AncestorCycle => "RCA1011",
        DiscoveryDiagnosticKind::UnknownCfg => "RCA1012",
        DiscoveryDiagnosticKind::InvocationOnlyCfg => "RCA1013",
        DiscoveryDiagnosticKind::MalformedCfg => "RCA1014",
    };
    if matches!(
        diagnostic.kind,
        DiscoveryDiagnosticKind::InvalidManifest
            | DiscoveryDiagnosticKind::AbsolutePath
            | DiscoveryDiagnosticKind::PackageEscape
    ) {
        let mut facts = vec![fact("path", diagnostic.path.display().to_string())];
        if let Some(range) = &diagnostic.range {
            facts.push(fact("range", format!("{}..{}", range.start, range.end)));
        }
        return make_diagnostic(
            code,
            &diagnostic.message,
            None,
            DiagnosticEvidence::SourceFacts(facts),
            DiagnosticRemedy::SelectSupportedConfiguration,
            None,
        );
    }
    located_diagnostic(
        code,
        &diagnostic.message,
        &diagnostic.path,
        diagnostic.range.clone(),
        inputs,
        LogicalDiagnosticOrigin::default(),
        DiagnosticRemedy::SelectSupportedConfiguration,
    )
}

/// Adapts resolution.
fn adapt_resolution(
    diagnostic: &ResolutionDiagnostic,
    inputs: &DiscoveryInputs,
) -> RecursaDiagnostic {
    let code = match diagnostic.kind {
        ResolutionDiagnosticKind::MissingGrammarConfig => "RCA1015",
        ResolutionDiagnosticKind::MultipleGrammarConfigs => "RCA1016",
        ResolutionDiagnosticKind::InvalidGrammarConfig => "RCA1017",
        ResolutionDiagnosticKind::InvalidConfiguration => "RCA2002",
        ResolutionDiagnosticKind::DuplicateDeclaration => "RCA2003",
        ResolutionDiagnosticKind::DuplicateExplicitImport => "RCA2004",
        ResolutionDiagnosticKind::ImportCollision => "RCA2005",
        ResolutionDiagnosticKind::AmbiguousImport => "RCA2006",
        ResolutionDiagnosticKind::InaccessibleReexport => "RCA2007",
        ResolutionDiagnosticKind::TokenBindingCollision => "RCA2008",
        ResolutionDiagnosticKind::InaccessibleTokenBinding => "RCA2009",
        ResolutionDiagnosticKind::InvalidVisibility => "RCA2010",
        ResolutionDiagnosticKind::UnsupportedDeclaration => "RCA2011",
        ResolutionDiagnosticKind::Unresolved => "RCA2012",
        ResolutionDiagnosticKind::Inaccessible => "RCA2013",
        ResolutionDiagnosticKind::AboveAnchor => "RCA2014",
        ResolutionDiagnosticKind::OutsideAnchor => "RCA2015",
        ResolutionDiagnosticKind::ExternalPath => "RCA2016",
        ResolutionDiagnosticKind::LeadingExternalPath => "RCA2017",
    };
    located_diagnostic(
        code,
        &diagnostic.message,
        &diagnostic.path,
        diagnostic.range.clone(),
        inputs,
        LogicalDiagnosticOrigin {
            module: diagnostic.module,
            path: diagnostic.logical_module.as_deref(),
        },
        DiagnosticRemedy::CorrectReference,
    )
}

#[derive(Clone, Copy, Default)]
/// The logical diagnostic origin representation used by source analysis.
struct LogicalDiagnosticOrigin<'a> {
    /// The module.
    module: Option<crate::lowering::ModuleId>,
    /// The path.
    path: Option<&'a [String]>,
}

/// Computes the located diagnostic.
fn located_diagnostic(
    code: &'static str,
    summary: &str,
    path: &Path,
    range: Option<std::ops::Range<u32>>,
    inputs: &DiscoveryInputs,
    origin: LogicalDiagnosticOrigin<'_>,
    remedy: DiagnosticRemedy,
) -> RecursaDiagnostic {
    let order = source_path_order(inputs, path, range.as_ref()).map(|(source_order, _)| {
        let start = range.as_ref().map_or(0, |range| range.start);
        (
            source_order,
            format!(
                "logical={};path={};offset={start}",
                origin
                    .path
                    .map_or_else(|| "-".to_owned(), |path| path.join("::")),
                path.display(),
            ),
        )
    });
    let mut facts = vec![fact("path", path.display().to_string())];
    if let Some(logical_module) = origin.path {
        facts.push(fact("logical-path", logical_module.join("::")));
    }
    if let Some(range) = &range {
        facts.push(fact("range", format!("{}..{}", range.start, range.end)));
    }
    let location = match checked_location(
        diagnostic_code_spec(RecursaDiagnosticCode::new(code)).stage,
        path,
        range.clone(),
        &inputs.selected_root,
    ) {
        Ok(location) => location,
        Err(internal) => return *internal,
    };
    make_diagnostic(
        code,
        summary,
        Some(location),
        DiagnosticEvidence::SourceFacts(facts),
        remedy,
        order,
    )
}

/// Computes the source path order.
fn source_path_order(
    inputs: &DiscoveryInputs,
    path: &Path,
    range: Option<&std::ops::Range<u32>>,
) -> Option<(u64, String)> {
    let source = inputs
        .source_files
        .iter()
        .position(|source| source == path)? as u64;
    let start = u64::from(range.map_or(0, |range| range.start));
    Some(((source << 32) | start, path.display().to_string()))
}

/// Adapts source model.
fn adapt_source_model(
    diagnostic: &SourceModelDiagnostic,
    selected_root: &Path,
) -> RecursaDiagnostic {
    let code = source_model_code(diagnostic.kind);
    let primary = diagnostic.sites.first();
    let mut adapted = site_diagnostic(
        code,
        &diagnostic.message,
        primary,
        DiagnosticEvidence::SourceFacts(vec![
            fact("condition", format!("{:?}", diagnostic.kind)),
            fact("related-sites", diagnostic.sites.len().to_string()),
        ]),
        DiagnosticRemedy::CorrectSource,
        selected_root,
    );
    let secondary = diagnostic
        .sites
        .iter()
        .skip(1)
        .map(|site| {
            checked_location(
                DiagnosticStage::Lowering,
                &site.path,
                Some(site.range.clone()),
                selected_root,
            )
            .map(|location| RecursaDiagnosticLabel::located(location, "related authored site"))
        })
        .collect::<Result<Vec<_>, _>>();
    let secondary = match secondary {
        Ok(secondary) => secondary,
        Err(internal) => return *internal,
    };
    adapted = adapted.with_secondary(secondary);
    adapted
}

/// Computes the source model code.
pub(crate) const fn source_model_code(kind: SourceModelDiagnosticKind) -> &'static str {
    match kind {
        SourceModelDiagnosticKind::InvalidConfiguration => "RCA3101",
        SourceModelDiagnosticKind::MultipleTokenSequences => "RCA3102",
        SourceModelDiagnosticKind::EmptyTokenSequence => "RCA3103",
        SourceModelDiagnosticKind::MultipleThisAnchors => "RCA3104",
        SourceModelDiagnosticKind::EmptyThisAnchor => "RCA3105",
        SourceModelDiagnosticKind::UnresolvedFixedToken => "RCA3106",
        SourceModelDiagnosticKind::UnsupportedConfiguredTrait => "RCA3107",
        SourceModelDiagnosticKind::InvalidTraitDelta => "RCA3108",
        SourceModelDiagnosticKind::ConflictingTraitDelta => "RCA3109",
        SourceModelDiagnosticKind::InvalidContentToken => "RCA3110",
        SourceModelDiagnosticKind::ContentDisplayOverride => "RCA3111",
        SourceModelDiagnosticKind::InvalidSeparator => "RCA3112",
        SourceModelDiagnosticKind::UnsupportedFixedSyntax => "RCA3113",
        SourceModelDiagnosticKind::InvalidTokenPlacement => "RCA3114",
        SourceModelDiagnosticKind::InvalidPrattTokenPlacement => "RCA3115",
        SourceModelDiagnosticKind::ConfiguredOrdinaryDeriveConflict => "RCA3116",
        SourceModelDiagnosticKind::MultiplePostLexHooks => "RCA3117",
        SourceModelDiagnosticKind::InvalidPrettyHint => "RCA3118",
        SourceModelDiagnosticKind::InapplicablePrettyHint => "RCA3210",
        SourceModelDiagnosticKind::UnsafePrettyHint => "RCA3211",
    }
}

/// Adapts lexical.
fn adapt_lexical(diagnostic: &LexicalDiagnostic, selected_root: &Path) -> RecursaDiagnostic {
    let code = lexical_code(diagnostic.code);
    let summary = lexical_summary(diagnostic.code);
    let evidence = lexical_evidence(&diagnostic.evidence);
    let remedies = diagnostic
        .remedies
        .iter()
        .map(adapt_lexical_remedy)
        .collect::<Vec<_>>();
    let mut adapted = site_diagnostic(
        code,
        summary,
        diagnostic.sites.first(),
        evidence,
        remedies
            .first()
            .cloned()
            .unwrap_or(DiagnosticRemedy::CorrectSource),
        selected_root,
    );
    adapted = adapted.with_remedies(remedies);
    let secondary = diagnostic
        .sites
        .iter()
        .skip(1)
        .map(|site| {
            checked_location(
                DiagnosticStage::Lowering,
                &site.path,
                Some(site.range.clone()),
                selected_root,
            )
            .map(|location| RecursaDiagnosticLabel::located(location, "related lexical definition"))
        })
        .collect::<Result<Vec<_>, _>>();
    let secondary = match secondary {
        Ok(secondary) => secondary,
        Err(internal) => return *internal,
    };
    adapted = adapted.with_secondary(secondary);
    adapted
}

/// Computes the lexical code.
pub(crate) const fn lexical_code(code: LexicalDiagnosticCode) -> &'static str {
    code.as_str()
}

/// Computes the lexical summary.
const fn lexical_summary(code: LexicalDiagnosticCode) -> &'static str {
    match code {
        LexicalDiagnosticCode::Rca3001 => "unsupported keyword matching policy",
        LexicalDiagnosticCode::Rca3002 => "fixed-token class contradicts its spelling",
        LexicalDiagnosticCode::Rca3003 => "token definitions have conflicting metadata",
        LexicalDiagnosticCode::Rca3004 => "unsupported lexical pattern",
        LexicalDiagnosticCode::Rca3005 => "lexical patterns overlap without a priority decision",
        LexicalDiagnosticCode::Rca3006 => "lexical patterns tie at the same authored priority",
    }
}

/// Computes the lexical evidence.
fn lexical_evidence(evidence: &LexicalDiagnosticEvidence) -> DiagnosticEvidence {
    let (condition, facts, witness) = match evidence {
        LexicalDiagnosticEvidence::InvalidKeywordMatchingPolicy { authored } => (
            LexicalEvidenceClass::KeywordMatchingPolicy,
            vec![fact("authored", authored)],
            None,
        ),
        LexicalDiagnosticEvidence::FixedTokenClassificationMismatch {
            spelling,
            authored,
            mechanical,
        } => (
            LexicalEvidenceClass::FixedTokenClassification,
            vec![
                fact("spelling", spelling),
                fact("authored", fixed_class(*authored)),
                fact("mechanical", fixed_class(*mechanical)),
            ],
            None,
        ),
        LexicalDiagnosticEvidence::ConflictingTokenDefinition {
            identity,
            normalized_metadata,
        } => (
            LexicalEvidenceClass::ConflictingDefinitionMetadata,
            std::iter::once(fact("identity", identity))
                .chain(
                    normalized_metadata
                        .iter()
                        .enumerate()
                        .map(|(index, metadata)| {
                            fact(
                                format!("definition-{index}"),
                                render_lexical_metadata(metadata),
                            )
                        }),
                )
                .collect(),
            None,
        ),
        LexicalDiagnosticEvidence::InvalidPattern { pattern, reason } => (
            LexicalEvidenceClass::InvalidPattern,
            vec![
                fact("pattern", pattern),
                fact("reason", format!("{reason:?}")),
            ],
            None,
        ),
        LexicalDiagnosticEvidence::PatternOverlap {
            contenders,
            witness,
        } => (
            LexicalEvidenceClass::PatternOverlap,
            pattern_facts(contenders),
            Some(witness.clone()),
        ),
        LexicalDiagnosticEvidence::PriorityTie {
            contenders,
            priority,
            witness,
        } => (
            LexicalEvidenceClass::PriorityTie,
            std::iter::once(fact("priority", priority.to_string()))
                .chain(pattern_facts(contenders))
                .collect(),
            Some(witness.clone()),
        ),
    };
    DiagnosticEvidence::Lexical {
        condition,
        facts,
        witness,
    }
}

/// Computes the pattern facts.
fn pattern_facts(contenders: &[LexicalPatternEvidence]) -> Vec<DiagnosticFact> {
    contenders
        .iter()
        .enumerate()
        .map(|(index, contender)| {
            fact(
                format!("contender-{index}"),
                format!(
                    "pattern={:?},targets={},priority={:?},outcome={:?}",
                    contender.pattern,
                    contender.target_names.join("|"),
                    contender.authored_priority,
                    contender.outcome,
                ),
            )
        })
        .collect()
}

/// Renders lexical metadata.
fn render_lexical_metadata(metadata: &LexicalDefinitionMetadata) -> String {
    match metadata {
        LexicalDefinitionMetadata::Fixed {
            class,
            canonical_spelling,
        } => format!("fixed:{canonical_spelling}:{}", fixed_class(*class)),
        LexicalDefinitionMetadata::Content {
            pattern,
            callback,
            authored_priority,
            admitted_fixed_tokens,
        } => format!(
            "content:{pattern:?}:callback={}:priority={authored_priority:?}:admits={}",
            callback
                .as_ref()
                .map_or_else(|| "-".to_owned(), crate::source_syntax::path_text),
            admitted_fixed_tokens.join("|"),
        ),
    }
}

/// Adapts lexical remedy.
fn adapt_lexical_remedy(remedy: &LexicalRemedy) -> DiagnosticRemedy {
    match remedy {
        LexicalRemedy::ChooseSupportedKeywordMatchingPolicy => {
            DiagnosticRemedy::SelectSupportedConfiguration
        }
        LexicalRemedy::UseMechanicalFixedTokenClass { class } => {
            DiagnosticRemedy::UseMechanicalFixedTokenClass {
                class: fixed_class(*class).to_owned(),
            }
        }
        LexicalRemedy::MakeDefinitionMetadataEquivalent { identity } => {
            DiagnosticRemedy::MakeDefinitionMetadataEquivalent {
                identity: identity.clone(),
            }
        }
        LexicalRemedy::UseSupportedNonEmptyPattern => DiagnosticRemedy::UseSupportedNonEmptyPattern,
        LexicalRemedy::AssignDistinctPriority { .. } => DiagnosticRemedy::AssignDistinctPriority,
        LexicalRemedy::MakePatternsDisjoint { .. } => DiagnosticRemedy::MakePatternsDisjoint,
    }
}

/// Computes the fact.
fn fact(name: impl Into<String>, value: impl Into<String>) -> DiagnosticFact {
    DiagnosticFact {
        name: name.into(),
        value: value.into(),
    }
}

/// Computes the fixed class.
const fn fixed_class(class: FixedTokenClass) -> &'static str {
    match class {
        FixedTokenClass::Keyword => "keyword",
        FixedTokenClass::Punctuation => "punctuation",
    }
}

/// Computes the site diagnostic.
fn site_diagnostic(
    code: &'static str,
    summary: &str,
    site: Option<&SourceSite>,
    mut evidence: DiagnosticEvidence,
    remedy: DiagnosticRemedy,
    selected_root: &Path,
) -> RecursaDiagnostic {
    let location = match site {
        Some(site) => match checked_location(
            DiagnosticStage::Lowering,
            &site.path,
            Some(site.range.clone()),
            selected_root,
        ) {
            Ok(location) => Some(location),
            Err(internal) => return *internal,
        },
        None => None,
    };
    let order = site.map(|site| {
        (
            u64::from(site.range.start),
            format!(
                "logical={};path={};offset={}",
                site.logical_module.join("::"),
                site.path.display(),
                site.range.start,
            ),
        )
    });
    if let Some(site) = site {
        let logical = fact("logical-path", site.logical_module.join("::"));
        match &mut evidence {
            DiagnosticEvidence::SourceFacts(facts) | DiagnosticEvidence::Lexical { facts, .. } => {
                facts.push(logical)
            }
            _ => {}
        }
    }
    make_diagnostic(code, summary, location, evidence, remedy, order)
}

/// Adapts materialization.
fn adapt_materialization(
    diagnostic: &MaterializationDiagnostic,
    selected_root: &Path,
) -> RecursaDiagnostic {
    let code = materialization_code(diagnostic.kind);
    let mut facts = vec![
        fact("condition", format!("{:?}", diagnostic.kind)),
        fact("path", diagnostic.path.display().to_string()),
        fact(
            "range",
            diagnostic.range.as_ref().map_or_else(
                || "-".to_owned(),
                |range| format!("{}..{}", range.start, range.end),
            ),
        ),
    ];
    if let Some(logical_module) = &diagnostic.logical_module {
        facts.push(fact("logical-path", logical_module.join("::")));
    }
    let start = u64::from(diagnostic.range.as_ref().map_or(0, |range| range.start));
    let order = diagnostic.range.as_ref().map(|_| {
        (
            start,
            format!(
                "logical={};path={};offset={start}",
                diagnostic
                    .logical_module
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |path| path.join("::")),
                diagnostic.path.display(),
            ),
        )
    });
    let location = match checked_location(
        DiagnosticStage::Lowering,
        &diagnostic.path,
        diagnostic.range.clone(),
        selected_root,
    ) {
        Ok(location) => location,
        Err(internal) => return *internal,
    };
    make_diagnostic(
        code,
        &diagnostic.message,
        Some(location),
        DiagnosticEvidence::SourceFacts(facts),
        DiagnosticRemedy::CorrectSource,
        order,
    )
}

/// Computes the materialization code.
pub(crate) const fn materialization_code(kind: MaterializationDiagnosticKind) -> &'static str {
    match kind {
        MaterializationDiagnosticKind::OpenApplication => "RCA3202",
        MaterializationDiagnosticKind::ConstGeneric => "RCA3203",
        MaterializationDiagnosticKind::UnsupportedType => "RCA3204",
        MaterializationDiagnosticKind::InvalidArguments => "RCA3205",
        MaterializationDiagnosticKind::InvalidContainer => "RCA3206",
        MaterializationDiagnosticKind::InvalidPrattShape => "RCA3209",
        MaterializationDiagnosticKind::InapplicablePrettyHint => "RCA3210",
        MaterializationDiagnosticKind::UnsafePrettyBoundary => "RCA3211",
        MaterializationDiagnosticKind::AliasCycle => "RCA3207",
        MaterializationDiagnosticKind::NonRegularRecursion => "RCA3208",
    }
}

/// Adapts analysis.
fn adapt_analysis(
    diagnostic: &crate::Diagnostic,
    report: &AnalysisReport,
    lowered: &LoweredGrammar,
    selected_root: &Path,
) -> RecursaDiagnostic {
    let primary = match adapt_analysis_label(&diagnostic.primary, report, lowered) {
        Ok(primary) => primary,
        Err(invariant) => {
            return internal_diagnostic(
                DiagnosticStage::Analysis,
                &format!("semantic analysis supplied an invalid source location: {invariant:?}"),
                selected_root,
            );
        }
    };
    let evidence = analysis_evidence(diagnostic, lowered);
    let remedy = match (&diagnostic.remedy, &diagnostic.conflict_remedy) {
        (Some(Remedy::CorrectReference), _) => DiagnosticRemedy::CorrectReference,
        (Some(Remedy::BreakLeftRecursion { cycle }), _) => DiagnosticRemedy::BreakRecursion {
            cycle: cycle
                .iter()
                .map(|type_id| lowered.grammar.types[type_id.index()].name.clone())
                .collect(),
        },
        (Some(Remedy::ReportInternalFailure), _) => DiagnosticRemedy::ReportInternalFailure,
        (_, Some(remedy)) => adapt_conflict_remedy(remedy, &lowered.grammar),
        _ => DiagnosticRemedy::CorrectSource,
    };
    let code = RecursaDiagnosticCode::new(diagnostic.code.as_str());
    let spec = diagnostic_code_spec(code);
    let mut adapted = RecursaDiagnostic::with_primary(
        spec.stage,
        code,
        diagnostic.severity,
        &diagnostic.message,
        primary,
        evidence,
        vec![remedy],
    );
    let secondary = diagnostic
        .secondary
        .iter()
        .map(|label| adapt_analysis_label(label, report, lowered))
        .collect::<Result<Vec<_>, _>>();
    let secondary = match secondary {
        Ok(secondary) => secondary,
        Err(invariant) => {
            return internal_diagnostic(
                DiagnosticStage::Analysis,
                &format!(
                    "semantic analysis supplied an invalid related source location: {invariant:?}"
                ),
                selected_root,
            );
        }
    };
    adapted = adapted.with_secondary(secondary);
    if let Some(location) = diagnostic.primary.location {
        let location = &report.locations()[location.index()];
        let source = &lowered.source_modeled.resolved().tree.files[location.source.index()];
        adapted = adapted.with_order(
            u64::from(location.range.start),
            format!(
                "path={};offset={}",
                source.path.display(),
                location.range.start,
            ),
        );
    }
    adapted
}

/// Computes the diagnostic site name.
fn diagnostic_site_name(site: crate::SiteId, grammar: &crate::GrammarDescription) -> String {
    let site = &grammar.sites[site.index()];
    grammar.declarations[site.declaration.index()].name.clone()
}

/// Adapts conflict remedy.
pub(crate) fn adapt_conflict_remedy(
    remedy: &ConflictRemedy,
    grammar: &crate::GrammarDescription,
) -> DiagnosticRemedy {
    let declaration = |id: crate::DeclId| grammar.declarations[id.index()].name.clone();
    let pratt = |id: crate::TypeId| grammar.types[id.index()].name.clone();
    match remedy {
        ConflictRemedy::DisambiguateAlternatives { left, right } => {
            DiagnosticRemedy::DisambiguateAlternatives {
                left: declaration(*left),
                right: declaration(*right),
            }
        }
        ConflictRemedy::RaiseMaxLookahead {
            configured,
            sufficient,
        } => DiagnosticRemedy::RaiseMaxLookahead {
            configured: *configured,
            sufficient: *sufficient,
        },
        ConflictRemedy::RestructureSharedPrefix { left, right } => {
            DiagnosticRemedy::RestructureSharedPrefix {
                left: declaration(*left),
                right: declaration(*right),
            }
        }
        ConflictRemedy::MakeSiteRequired { site } => DiagnosticRemedy::MakeSiteRequired {
            site: diagnostic_site_name(*site, grammar),
        },
        ConflictRemedy::DisambiguatePrattVariants {
            pratt: ty,
            left,
            right,
        } => DiagnosticRemedy::DisambiguatePrattVariants {
            pratt: pratt(*ty),
            left: declaration(*left),
            right: declaration(*right),
        },
        ConflictRemedy::RestructurePrattSharedPrefix {
            pratt: ty,
            left,
            right,
        } => DiagnosticRemedy::RestructurePrattSharedPrefix {
            pratt: pratt(*ty),
            left: declaration(*left),
            right: declaration(*right),
        },
        ConflictRemedy::ReviewNullableFollow { site } => DiagnosticRemedy::ReviewNullableFollow {
            site: diagnostic_site_name(*site, grammar),
        },
        ConflictRemedy::ReviewPrattFollow { site, min_bp } => DiagnosticRemedy::ReviewPrattFollow {
            site: diagnostic_site_name(*site, grammar),
            min_bp: *min_bp,
        },
    }
}

/// Computes the analysis evidence.
fn analysis_evidence(
    diagnostic: &crate::Diagnostic,
    lowered: &LoweredGrammar,
) -> DiagnosticEvidence {
    let overlap = diagnostic
        .conflict
        .as_ref()
        .map_or_else(Vec::new, |conflict| {
            conflict
                .overlap
                .iter()
                .map(|kind| lowered.grammar.kinds[kind.index()].name.clone())
                .collect()
        });
    let witness = diagnostic.lookahead.as_ref().map_or_else(
        || {
            diagnostic
                .conflict
                .as_ref()
                .map_or_else(Vec::new, |conflict| {
                    vec![
                        lowered.grammar.kinds[conflict.witness.kind.index()]
                            .name
                            .clone(),
                    ]
                })
        },
        |lookahead| {
            lookahead
                .witness
                .iter()
                .map(|position| lowered.grammar.kinds[position.kind.index()].name.clone())
                .collect()
        },
    );
    let cycle = match &diagnostic.remedy {
        Some(Remedy::BreakLeftRecursion { cycle }) => cycle
            .iter()
            .map(|type_id| lowered.grammar.types[type_id.index()].name.clone())
            .collect(),
        Some(Remedy::CorrectGrammar | Remedy::CorrectReference | Remedy::ReportInternalFailure)
        | None => Vec::new(),
    };
    let (configured_lookahead, sufficient_lookahead, end_of_alternative) = diagnostic
        .lookahead
        .as_ref()
        .map_or((None, None, false), |lookahead| {
            (
                Some(lookahead.configured_depth),
                lookahead.sufficient_depth,
                lookahead.end_of_alternative,
            )
        });
    if diagnostic.code == crate::DiagnosticCode::InternalAnalysisInvariant {
        DiagnosticEvidence::InternalInvariant {
            producer: "semantic analysis decision publication".to_owned(),
            detail: diagnostic.message.clone(),
        }
    } else {
        DiagnosticEvidence::Analysis {
            overlap,
            witness,
            cycle,
            configured_lookahead,
            sufficient_lookahead,
            end_of_alternative,
        }
    }
}

/// Adapts analysis label.
fn adapt_analysis_label(
    label: &crate::DiagnosticLabel,
    report: &AnalysisReport,
    lowered: &LoweredGrammar,
) -> Result<RecursaDiagnosticLabel, crate::build_diagnostic::DiagnosticLocationInvariant> {
    let Some(location) = label.location else {
        return Ok(RecursaDiagnosticLabel::unlocated(&label.message));
    };
    let location = &report.locations()[location.index()];
    let source = &lowered.source_modeled.resolved().tree.files[location.source.index()];
    DiagnosticLocation::new(source.path.clone(), Some(location.range.clone()))
        .and_then(|location| location.with_source_text(&source.text))
        .map(|location| RecursaDiagnosticLabel::located(location, &label.message))
}
