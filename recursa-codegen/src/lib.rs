//! Grammar lowering and Rust-source generation.
//!
//! The private source pipeline discovers and lowers Rust grammar sources, and [`generate`] joins
//! that output into an emission plan before atomically publishing one complete [`Generation`].

/// Stable generated-build and loader diagnostic contracts.
mod build_diagnostic;
/// Representation-independent AST Node declaration parsing and lowering.
mod declaration;
/// Recursive concrete-layout validation for materialized Node declarations.
mod declaration_layout;
/// Deterministic authored-module discovery and invalidation inputs.
mod discovery;
/// Deterministic hashing used by generation identity.
mod fast_hash;
mod generation;
/// Compact typed identities used throughout normalized grammar products.
mod ids;
/// Normalized, source-independent grammar representation.
mod ir;
/// Width-checked symbolic token-kind sets.
mod kind_set;
/// Grammar-wide lexical planning and carrier proofs.
mod lexical;
/// Correlation-preserving lowering into the normalized grammar.
mod lowering;
/// LR lowering, LALR(1) construction and parser tables.
pub mod lr;
/// Closed generic application and container materialization.
mod materialization;
/// Opt-in hierarchical timing for generation and LR stages.
mod profile;
/// Anchored resolution of grammar-semantic references.
mod resolution;
/// Stable, location-free projections of LR grammar meaning.
mod semantic_projection;
#[cfg(test)]
mod semantic_projection_tests;
/// Source-analysis orchestration and diagnostic adaptation.
mod source_analysis;
#[cfg(test)]
mod source_analysis_contract_tests;
#[cfg(test)]
mod source_analysis_tests;
/// Grammar-relevant authored structure and locations.
mod source_model;
/// Parsing of generator-owned source configuration syntax.
mod source_syntax;

pub use build_diagnostic::{
    BuildReport, DiagnosticEvidence, DiagnosticFact, DiagnosticLocation, DiagnosticRemedy,
    DiagnosticStage, GenerateError, LexicalEvidenceClass, LineColumn, RecursaDiagnostic,
    RecursaDiagnosticCode, RecursaDiagnosticLabel, Severity,
};
pub use generation::{Generation, generate};
pub use ids::{
    AtomId, DeclId, KindId, LocationId, PrecedenceLevelId, RestrictionId, SiteId, SourceId,
    TextPredicateId, TypeId,
};
pub use ir::{
    Alternative, AtomDescription, AtomExpression, AuthoredRole, AuthoredSite, ContainerShape,
    Declaration, GrammarDescription, KindDescription, Location, LookaheadFilter, LrConflictAction,
    LrConflictResolution, LrRuleOwner, ParseSite, PrattPosition, PrattVariant,
    PrecedenceAssociativity, PrecedenceLevel, RepeatSeparator, RepetitionCardinality,
    RestrictionDescription, TextEquivalence, TextPredicate, TypeDescription, TypeShape,
};
pub use kind_set::{KindSet, KindSetError, KindSetIter};
