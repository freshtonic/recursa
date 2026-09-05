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
use syn::{GenericArgument, GenericParam, Generics, Ident, PathArguments, Type, Visibility};

use crate::PrattPosition;
use crate::TypeShape;
use crate::build_diagnostic::{BuildReport, DiagnosticStage, GenerateError};
use crate::decision_plan::{
    ContainerOperation, DispatchKind, ExpectedRecipe, FailureEdgeKind, FollowRecipe,
};
use crate::lexical::{
    KeywordRecognition, LexicalMatcher, LexicalPriority, NonTokenLexicalOutcome, TokenKindIdentity,
};
use crate::lowering::{ModuleId, NodeId, TokenDefinitionId, TokenUseId};
use crate::materialization::{
    ClosedType, MaterializedApplicationId, MaterializedElement, MaterializedFixedAttachment,
    MaterializedNodeApplication, MaterializedNodeBody, MaterializedPrattLed,
    MaterializedPrattLedPosition, MaterializedPrattNud, MaterializedPrattNudPosition,
    MaterializedPrattVariant, MaterializedSemanticFieldProjection, MaterializedVariant,
};
use crate::resolution::{
    CatalogType, ContentTokenId, GenericParameterId, ResolvedFieldId, ResolvedFieldStyle,
    ResolvedGenericArgument, ResolvedNode, ResolvedNodeBody, ResolvedSourceGrammar, ResolvedType,
    ResolvedTypeKind, ResolvedVariantId, SemanticOwner, SemanticTarget,
};
#[cfg(test)]
use crate::source_analysis::analyze_source_tree;
use crate::source_analysis::{
    CargoOutput, DiscoveredSourceTree, SourceAnalysisInput, analyze_discovered_tree,
    discover_source_tree, internal_generate_error, warning_directives,
};
use crate::source_model::{
    ClosedMatcher, ConfiguredTraitRecipe, GrammarElement, Node as SourceNode,
    NodeBody as SourceNodeBody, PrettyBreak, PrettyGroup, PrettyHints, PrettySeparatorPosition,
    PrettySpacing, PrettyTrailingSeparator, SourcePrattPosition, TokenDefinitionKind,
};
use crate::source_syntax::{CaptureMode, CargoCfgSnapshot, ParserStyle, span_range};
use crate::{AtomExpression, FirstPosition, KindSet, SiteId, TextEquivalence, TextPredicate};

mod configured_traits;
mod debug_bundle;
#[cfg(feature = "diagrams")]
mod diagrams;
mod emission_plan;
mod explorer;
mod generated_output;
mod lexing;
mod node_views;
mod parsing;
mod pretty;
mod protocol;
mod publication;
mod reuse;
mod rust_names;
mod visit;

use configured_traits::*;
use debug_bundle::*;
#[cfg(feature = "diagrams")]
use diagrams::*;
use emission_plan::*;
use explorer::*;
use generated_output::*;
use lexing::*;
use node_views::*;
use parsing::*;
use pretty::*;
use protocol::*;
use publication::*;
use reuse::*;
use rust_names::*;
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
    let discovered = discover_source_tree(&input, cargo)?;
    let reuse_key = reuse_key(&discovered, &request.cfg);
    if request.debug_directory.is_none()
        && let Some(reused) =
            reuse_published(out_dir, &reuse_key).map_err(|detail| failure(root, &detail))?
    {
        for directive in reused.warnings {
            cargo.emit(directive);
        }
        return Ok(Generation {
            report: BuildReport::new(Vec::new()),
            reused: true,
        });
    }
    let success = analyze_discovered_tree(discovered, &input, cargo)?;
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
    let plan = EmissionPlan::join(&success.analyzed, &request.cfg)
        .map_err(|detail| failure(root, &detail))?;
    let rendered = render_by_parser_style(&plan).map_err(|detail| failure(root, &detail))?;
    let debug_bundle = request
        .debug_directory
        .as_deref()
        .map(|directory| stage_debug_bundle(directory, &plan, &rendered))
        .transpose()
        .map_err(|detail| failure(root, &detail))?;
    publish_generation_and_debug(out_dir, &plan.identity, &rendered, debug_bundle)
        .map_err(|detail| failure(root, &detail))?;
    record_reuse(
        out_dir,
        &reuse_key,
        &plan.identity,
        &rendered.selector,
        &warning_directives(&success.build_report),
    )
    .map_err(|detail| failure(root, &detail))?;
    Ok(Generation {
        report: success.build_report,
        reused: false,
    })
}

/// Selects the renderer for the plan's declared parsers.
///
/// This is the one point where a parser style chooses its rendering. Every
/// parser of the implicit or explicit recursive-descent style renders through
/// [`render`]. A `table_driven` parser has no renderer yet; selecting one is
/// reported as an internal emission failure until the table renderer lands.
fn render_by_parser_style(plan: &impl RenderingPlan) -> Result<RenderedGeneration, String> {
    let parsers = &plan.emission_plan().parsers;
    if let Some(table) = parsers
        .iter()
        .find(|parser| parser.style == ParserStyle::TableDriven)
    {
        let selector = match &table.name {
            Some(name) => format!("parser `{name}`"),
            None => "the default parser".to_string(),
        };
        return Err(format!(
            "table-driven rendering not implemented yet: {selector} selects `table_driven`"
        ));
    }
    if parsers.iter().count() > 1 {
        return Err(
            "named parser rendering not implemented yet: only the default parser renders"
                .to_string(),
        );
    }
    render(plan)
}

/// Registers the optional debug destination as a Cargo build input.
fn emit_debug_directory_invalidation(cargo: &mut impl CargoOutput) {
    cargo.emit(format!(
        "cargo::rerun-if-env-changed={DEBUG_DIRECTORY_ENVIRONMENT}"
    ));
}

/// Converts an emission invariant into a stable-coded diagnostic at the selected root.
fn failure(selected_root: &Path, detail: &str) -> GenerateError {
    internal_generate_error(DiagnosticStage::Emission, detail, selected_root)
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
