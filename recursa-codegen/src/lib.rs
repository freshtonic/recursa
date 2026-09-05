//! Grammar analysis and Rust-source generation.
//!
//! Semantic analysis interprets normalized [`GrammarDescription`] values. The private source-analysis
//! pipeline discovers and analyzes Rust grammar sources, and [`generate`]
//! joins that analyzed output into a generation emission plan before atomically
//! publishing one complete generated [`Generation`].

/// Source-independent semantic fact computation and validation.
mod analysis;
/// Stable generated-build and loader diagnostic contracts.
mod build_diagnostic;
/// Frozen parser decisions derived from semantic facts.
mod decision_plan;
/// Typed source-independent semantic-analysis diagnostics.
mod diagnostic;
/// Deterministic authored-module discovery and invalidation inputs.
mod discovery;
/// Fast deterministic hashing for analysis-internal interning tables.
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
/// LR lowering, LALR(1) construction and tables for the table-driven parser style.
pub mod lr;
/// Closed generic application and container materialization.
mod materialization;
/// Canonical proof provenance for semantic facts and diagnostics.
mod provenance;
/// Anchored resolution of grammar-semantic references.
mod resolution;
/// Stable, location-free semantic review projections.
mod semantic_projection;
/// Source-analysis orchestration and diagnostic adaptation.
mod source_analysis;
/// Grammar-relevant authored structure and locations.
mod source_model;
/// Parsing of generator-owned source configuration syntax.
mod source_syntax;
/// Canonical text-cell algebra shared by symbolic analysis and dispatch.
mod text_partition;

#[cfg(test)]
mod semantic_projection_tests;
#[cfg(test)]
mod source_analysis_contract_tests;
#[cfg(test)]
mod source_analysis_tests;

pub use analysis::{
    AcceptedOverlap, AnalysisOutput, AnalysisReport, ExtenderFacts, FirstFacts, FirstPosition,
    FirstSequence, FollowFacts, analyze, disambiguation_depth,
};
pub use build_diagnostic::{
    BuildReport, DiagnosticEvidence, DiagnosticFact, DiagnosticLocation, DiagnosticRemedy,
    DiagnosticStage, GenerateError, LexicalEvidenceClass, LineColumn, RecursaDiagnostic,
    RecursaDiagnosticCode, RecursaDiagnosticLabel,
};
pub use decision_plan::{
    BalancedDispatch, ContainerAction, ContainerMachine, ContainerOperation, ContainerState,
    ContainerTransition, DecisionProducts, DispatchBranch, DispatchKind, DispatchPlan,
    DispatchPosition, DispatchSequence, DispatchTerminal, DispatchTerminalMode, DispatchTrieEdge,
    DispatchTrieNode, ExpectedRecipe, FailureEdge, FailureEdgeKind, FollowRecipe, OptionalAction,
    OptionalViabilityDecision, OptionalViabilityDecisionEdge, OptionalViabilityDecisionNode,
    OptionalViabilityDecisionTerminal,
};
pub use diagnostic::{
    ConflictEvidence, ConflictRemedy, ConflictWitness, Diagnostic, DiagnosticCode, DiagnosticLabel,
    LookaheadWitnessPosition, Remedy, Severity, SharedPrefixEvidence,
};
pub use generation::{Generation, generate};
pub use ids::{
    AtomId, DeclId, KindId, LocationId, ProofId, SiteId, SourceId, TextPredicateId, TypeId,
};
pub use ir::{
    Alternative, AtomDescription, AtomExpression, AuthoredRole, AuthoredSite, BalancedDelimiter,
    ContainerShape, Declaration, GrammarDescription, GreedyAcceptance, GreedyKinds,
    KindDescription, Location, ParseSite, PrattPosition, PrattVariant, RepeatSeparator,
    RepetitionCardinality, TextEquivalence, TextPredicate, TypeDescription, TypeShape,
};
pub use kind_set::{KindSet, KindSetError, KindSetIter};
pub use provenance::{
    ProofArena, ProofChain, ProofNode, ProofRank, ProvenanceStep, ProvenancedKindSet,
};
