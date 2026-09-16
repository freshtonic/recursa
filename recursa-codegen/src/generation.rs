//! Source-aware generation from an analyzed grammar to one published logical output.
//!
//! [`generate`] is the only public coordinator. It joins immutable semantic decisions with
//! authored source structure, validates the resulting emission plan, renders every logical
//! file, stages optional debug output, and atomically selects the completed generation. No
//! loader or partially written payload becomes observable before publication succeeds.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use quote::{ToTokens as _, format_ident, quote};
use syn::ext::IdentExt as _;
use syn::spanned::Spanned as _;
use syn::visit::{self as syn_visit, Visit as _};
use syn::{GenericArgument, GenericParam, Generics, Ident, Item, PathArguments, Type, Visibility};

use crate::TypeShape;
use crate::build_diagnostic::{
    BuildReport, DiagnosticEvidence, DiagnosticLocation, DiagnosticRemedy, DiagnosticStage,
    GenerateError,
};
use crate::lexical::{
    KeywordRecognition, LexicalMatcher, LexicalPriority, NonTokenLexicalOutcome, TokenKindIdentity,
};
use crate::lowering::{ModuleId, NodeId, TokenDefinitionId, TokenUseId};
use crate::lr::LrDiagnostic;
#[cfg(feature = "diagrams")]
use crate::materialization::MaterializedFixedAttachment;
use crate::materialization::{
    ClosedType, MaterializedApplicationId, MaterializedElement, MaterializedNodeApplication,
    MaterializedNodeBody, MaterializedSemanticFieldProjection,
};
use crate::profile::{ENVIRONMENT as PROFILE_ENVIRONMENT, Span};
use crate::resolution::{
    CatalogType, ContentTokenId, GenericParameterId, ResolvedFieldId, ResolvedFieldStyle,
    ResolvedGenericArgument, ResolvedNode, ResolvedNodeBody, ResolvedNodeOrigin,
    ResolvedSourceGrammar, ResolvedType, ResolvedTypeKind, ResolvedVariantId, SemanticOwner,
    SemanticTarget,
};
#[cfg(test)]
use crate::source_analysis::analyze_source_tree;
use crate::source_analysis::{
    CargoOutput, DiscoveredSourceTree, SourceAnalysisInput, analyze_discovered_tree,
    discover_source_tree, internal_generate_error, make_diagnostic, warning_directives,
};
use crate::source_model::{
    AttachedFixedSyntax, ClosedMatcher, ConfiguredTraitRecipe, GrammarElement, Node as SourceNode,
    NodeBody as SourceNodeBody, PrecedenceKindReference, PrecedenceOwner, PrettyBreak, PrettyGroup,
    PrettyHints, PrettySeparatorPosition, PrettySpacing, PrettyTrailingSeparator,
    SemanticFieldProjection, SourceModel, TokenDefinitionKind,
};
use crate::{PrecedenceAssociativity, PrecedenceLevelId};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Cardinality operation implemented by one generated container projection.
enum ContainerOperation {
    /// A required value.
    Direct,
    /// An optional value.
    Optional,
    /// Zero or more values.
    ZeroOrMore,
    /// One or more values.
    OneOrMore,
    /// An optional wrapper around one or more values.
    OptionalOneOrMore,
}
use crate::source_syntax::{CaptureMode, CargoCfgSnapshot, span_range};
use crate::{AtomExpression, TextEquivalence};

mod arbitrary_seed;
mod configured_traits;
mod debug_bundle;
#[cfg(feature = "diagrams")]
mod diagrams;
mod emission_plan;
mod explorer;
mod flat;
mod flat_pretty;
mod flat_traversal;
mod generated_output;
mod lexing;
mod node_views;
mod parsers;
mod parsing;
mod pretty;
mod protocol;
mod publication;
mod reuse;
mod rust_names;
mod table_driven;
mod table_driven_flat;
mod visit;

use arbitrary_seed::*;
use configured_traits::*;
use debug_bundle::*;
#[cfg(feature = "diagrams")]
use diagrams::*;
use emission_plan::*;
use explorer::*;
use flat::*;
use flat_pretty::*;
use flat_traversal::*;
use generated_output::*;
use lexing::*;
use node_views::*;
use parsers::*;
use parsing::*;
use pretty::*;
use protocol::*;
use publication::*;
use reuse::*;
use rust_names::*;
use table_driven::*;
use visit::*;

/// One complete published logical output.
pub struct Generation {
    /// Non-fatal diagnostics retained from the analysis that produced this Generation.
    report: BuildReport,
    /// Whether an already published Generation was re-selected without analyzing again.
    reused: bool,
}

impl Generation {
    /// Returns the retained non-fatal diagnostics.
    ///
    /// A reused Generation carries an empty report: its warnings were replayed to Cargo from
    /// the reuse index, and the structured diagnostics were not recomputed.
    pub const fn report(&self) -> &BuildReport {
        &self.report
    }

    /// Returns `true` when the published Generation was re-selected because the discovered
    /// grammar source, configuration, and generator were unchanged.
    pub const fn reused(&self) -> bool {
        self.reused
    }
}

/// Production Cargo-instruction sink used during source analysis and invalidation tracking.
struct StdoutCargo;

impl CargoOutput for StdoutCargo {
    fn emit(&mut self, instruction: String) {
        println!("{instruction}");
    }
}

/// Analyzes, renders, stages, and atomically selects one complete Generation.
///
/// When the discovered grammar source is unchanged since a previous successful run into the
/// same `OUT_DIR`, the published Generation is re-selected and its warnings are replayed, and
/// no analysis or rendering runs. Setting the debug-directory environment variable always runs
/// the complete pipeline so the debug bundle is fresh.
pub fn generate(root: impl AsRef<Path>) -> Result<Generation, GenerateError> {
    let mut cargo = StdoutCargo;
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "OUT_DIR is unavailable"))?;
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "CARGO_MANIFEST_DIR is unavailable"))?;
    let request = GenerateRequest {
        out_dir,
        manifest_dir,
        selected_root: root.as_ref().to_path_buf(),
        debug_directory: env::var_os(DEBUG_DIRECTORY_ENVIRONMENT).map(PathBuf::from),
        cfg: CargoCfgSnapshot::from_cargo_environment(env::vars_os()),
    };
    generate_with(&request, &mut cargo)
}

/// Explicit inputs of one generation run, decoupled from the process environment.
pub(crate) struct GenerateRequest {
    /// Build-script output directory holding Generations, the selector, and the reuse index.
    pub(crate) out_dir: PathBuf,
    /// Package root against which grammar sources are discovered.
    pub(crate) manifest_dir: PathBuf,
    /// Authored Rust file selected as the grammar root, relative to `manifest_dir`.
    pub(crate) selected_root: PathBuf,
    /// Optional debug-bundle destination; when present, reuse is bypassed.
    pub(crate) debug_directory: Option<PathBuf>,
    /// Active Cargo configuration controlling module inclusion.
    pub(crate) cfg: CargoCfgSnapshot,
}

/// Runs one generation with explicit inputs and a caller-owned Cargo instruction sink.
pub(crate) fn generate_with(
    request: &GenerateRequest,
    cargo: &mut impl CargoOutput,
) -> Result<Generation, GenerateError> {
    let _profile = Span::enter("generation");
    let root = request.selected_root.as_path();
    let out_dir = request.out_dir.as_path();
    emit_debug_directory_invalidation(cargo);
    invalidate_selection(out_dir).map_err(|detail| failure(root, &detail))?;
    let input = SourceAnalysisInput::new(
        &request.manifest_dir,
        root,
        crate::lowering::RustEdition::Rust2024,
        request.cfg.clone(),
    );
    let discovered = {
        let _stage = Span::enter("discovery");
        discover_source_tree(&input, cargo)?
    };
    let reuse_key = reuse_key(&discovered, &request.cfg);
    let reused = {
        let _stage = Span::enter("reuse-lookup");
        if request.debug_directory.is_none() {
            reuse_published(out_dir, &reuse_key).map_err(|detail| failure(root, &detail))?
        } else {
            None
        }
    };
    if let Some(reused) = reused {
        for directive in reused.warnings {
            cargo.emit(directive);
        }
        return Ok(Generation {
            report: BuildReport::new(Vec::new()),
            reused: true,
        });
    }
    let success = {
        let _stage = Span::enter("source-analysis");
        analyze_discovered_tree(discovered, &input, cargo)?
    };
    if success
        .analyzed
        .lowered
        .source_modeled
        .resolved()
        .build_inputs
        .diagrams
        && !cfg!(feature = "diagrams")
    {
        return Err(failure(
            root,
            "diagram generation was requested but recursa-codegen was built without its `diagrams` capability",
        ));
    }
    let plan = {
        let _stage = Span::enter("emission-plan");
        EmissionPlan::join(&success.analyzed, &request.cfg)
            .map_err(|detail| failure(root, &detail))?
    };
    let rendered = {
        let _stage = Span::enter("render");
        render_lr_parser(&plan, &success.analyzed.lowered).map_err(|error| match error {
            LrBuildError::Diagnostics(diagnostics) => lr_conflict_error(root, &diagnostics),
            LrBuildError::Internal(detail) => failure(root, &detail),
        })?
    };
    let debug_bundle = {
        let _stage = Span::enter("debug-bundle");
        request
            .debug_directory
            .as_deref()
            .map(|directory| stage_debug_bundle(directory, &plan, &rendered))
            .transpose()
            .map_err(|detail| failure(root, &detail))?
    };
    {
        let _stage = Span::enter("publication");
        publish_generation_and_debug(out_dir, &plan.identity, &rendered, debug_bundle)
            .map_err(|detail| failure(root, &detail))?;
    }
    {
        let _stage = Span::enter("reuse-record");
        record_reuse(
            out_dir,
            &reuse_key,
            &plan.identity,
            &rendered.selector,
            &warning_directives(&success.build_report),
        )
        .map_err(|detail| failure(root, &detail))?;
    }
    Ok(Generation {
        report: success.build_report,
        reused: false,
    })
}

/// Renders the grammar through Recursa's sole LR parser.
fn render_lr_parser(
    plan: &impl RenderingPlan,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<RenderedGeneration, LrBuildError> {
    render_declared(plan, lowered)
}

/// Registers optional debug and profiling controls as Cargo build inputs.
fn emit_debug_directory_invalidation(cargo: &mut impl CargoOutput) {
    cargo.emit(format!(
        "cargo::rerun-if-env-changed={DEBUG_DIRECTORY_ENVIRONMENT}"
    ));
    cargo.emit(format!("cargo::rerun-if-env-changed={PROFILE_ENVIRONMENT}"));
}

/// Converts an emission invariant into a stable-coded diagnostic at the selected root.
fn failure(selected_root: &Path, detail: &str) -> GenerateError {
    internal_generate_error(DiagnosticStage::Emission, detail, selected_root)
}

/// Converts every unresolved LR conflict or lowering diagnostic into a stable-coded build
/// report: an ordinary authoring error (`RCA0400`/`RCA0401` for an unresolved shift/reduce or
/// reduce/reduce conflict, `RCA0403` for a text predicate the
/// table-driven style cannot lower) whose evidence carries the automaton dump's content for
/// that state — its shortest path, items and competing actions — so an unresolved conflict in
/// an authored grammar is never wrapped as the internal-invariant `RCA9105`. A shift/reduce
/// conflict's summary also carries the authoring remedy: declare a precedence level for the
/// lookahead token, or add `#[parse(prec = NAME)]` to the rule that reduces.
fn lr_conflict_error(selected_root: &Path, diagnostics: &[LrDiagnostic]) -> GenerateError {
    let location = DiagnosticLocation::new(selected_root.to_owned(), None).ok();
    let reports = diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let mut witness = Vec::new();
            if !diagnostic.path.is_empty() {
                witness.push(format!("path: {}", diagnostic.path));
            }
            witness.extend(diagnostic.items.iter().map(|item| format!("item: {item}")));
            witness.extend(
                diagnostic
                    .actions
                    .iter()
                    .map(|action| format!("action: {action}")),
            );
            witness.extend(diagnostic.notes.iter().cloned());
            let summary = if diagnostic.code == crate::lr::report::SHIFT_REDUCE {
                format!(
                    "{}; declare a precedence level for the lookahead token, or add \
                     `#[parse(prec = NAME)]` to the rule that reduces",
                    diagnostic.message
                )
            } else {
                diagnostic.message.clone()
            };
            make_diagnostic(
                diagnostic.code,
                &summary,
                location.clone(),
                DiagnosticEvidence::SemanticWitness {
                    subject: diagnostic.message.clone(),
                    witness,
                },
                DiagnosticRemedy::CorrectSource,
                Some((
                    index as u64,
                    format!("{}#{}", diagnostic.code, diagnostic.path),
                )),
            )
        })
        .collect();
    GenerateError::new(BuildReport::new(reports))
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
