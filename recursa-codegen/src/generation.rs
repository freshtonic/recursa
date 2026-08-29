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
use crate::lexical::{KeywordRecognition, LexicalMatcher, LexicalPriority, TokenKindIdentity};
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
use crate::source_analysis::{
    CargoOutput, SourceAnalysisInput, analyze_source_tree, internal_generate_error,
};
use crate::source_model::{
    ClosedMatcher, ConfiguredTraitRecipe, GrammarElement, Node as SourceNode,
    NodeBody as SourceNodeBody, PrettyBreak, PrettyGroup, PrettyHints, PrettySeparatorPosition,
    PrettySpacing, PrettyTrailingSeparator, SourcePrattPosition, TokenDefinitionKind,
};
use crate::source_syntax::{CaptureMode, CargoCfgSnapshot, span_range};
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
use rust_names::*;
use visit::*;

/// One complete published logical output.
pub struct Generation {
    /// Warnings retained after the publication barrier succeeds.
    report: BuildReport,
}

impl Generation {
    /// Warnings retained on this successful Generation.
    pub const fn report(&self) -> &BuildReport {
        &self.report
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
pub fn generate(root: impl AsRef<Path>) -> Result<Generation, GenerateError> {
    let mut cargo = StdoutCargo;
    emit_debug_directory_invalidation(&mut cargo);
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "OUT_DIR is unavailable"))?;
    invalidate_selection(&out_dir).map_err(|detail| failure(root.as_ref(), &detail))?;
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "CARGO_MANIFEST_DIR is unavailable"))?;
    let debug_directory = env::var_os(DEBUG_DIRECTORY_ENVIRONMENT).map(PathBuf::from);
    let cfg = CargoCfgSnapshot::from_cargo_environment(env::vars_os());
    let success = analyze_source_tree(
        SourceAnalysisInput::new(
            &manifest_dir,
            root.as_ref(),
            crate::lowering::RustEdition::Rust2024,
            cfg.clone(),
        ),
        &mut cargo,
    )?;
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
            root.as_ref(),
            "diagram generation was requested but recursa-codegen was built without its `diagrams` capability",
        ));
    }
    let plan = EmissionPlan::join(&success.analyzed, &cfg)
        .map_err(|detail| failure(root.as_ref(), &detail))?;
    let rendered = render(&plan).map_err(|detail| failure(root.as_ref(), &detail))?;
    let debug_bundle = debug_directory
        .as_deref()
        .map(|directory| stage_debug_bundle(directory, &plan, &rendered))
        .transpose()
        .map_err(|detail| failure(root.as_ref(), &detail))?;
    publish_generation_and_debug(&out_dir, &plan.identity, &rendered, debug_bundle)
        .map_err(|detail| failure(root.as_ref(), &detail))?;
    Ok(Generation {
        report: success.build_report,
    })
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
