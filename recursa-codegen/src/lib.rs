//! Grammar analysis and Rust-source generation.
//!
//! Semantic analysis interprets normalized [`GrammarDescription`] values. The private source-analysis
//! pipeline discovers and analyzes Rust grammar sources, and [`generate`]
//! joins that analyzed output into a generation emission plan before atomically
//! publishing one complete generated [`Generation`].

mod analysis;
mod build_diagnostic;
mod decision_plan;
mod diagnostic;
mod discovery;
mod generation_compound;
mod ids;
mod ir;
mod kind_set;
mod lexical;
mod lowering;
mod materialization;
mod provenance;
mod resolution;
mod semantic_projection;
mod source_analysis;
mod source_model;
mod source_syntax;

#[cfg(test)]
mod semantic_projection_tests;
#[cfg(test)]
mod source_analysis_contract_tests;
#[cfg(test)]
mod source_analysis_tests;

pub use analysis::{
    AnalysisOutput, AnalysisReport, ExtenderFacts, FirstFacts, FirstPosition, FirstSequence,
    FollowFacts, analyze, disambiguation_depth,
};
pub use build_diagnostic::{
    BuildReport, DiagnosticEvidence, DiagnosticFact, DiagnosticLocation, DiagnosticRemedy,
    DiagnosticStage, GenerateError, LexicalEvidenceClass, LineColumn, RecursaDiagnostic,
    RecursaDiagnosticCode, RecursaDiagnosticLabel,
};
pub use decision_plan::{
    ContainerAction, ContainerMachine, ContainerOperation, ContainerState, ContainerTransition,
    DecisionProducts, DispatchBranch, DispatchKind, DispatchPlan, DispatchSequence,
    DispatchTerminal, DispatchTrieEdge, DispatchTrieNode, ExpectedRecipe, FailureEdge,
    FailureEdgeKind, FollowRecipe,
};
pub use diagnostic::{
    ConflictEvidence, ConflictRemedy, ConflictWitness, Diagnostic, DiagnosticCode, DiagnosticLabel,
    LookaheadWitnessPosition, Remedy, Severity, SharedPrefixEvidence,
};
pub use generation_compound::{Generation, generate};
pub use ids::{
    AtomId, DeclId, KindId, LocationId, ProofId, SiteId, SourceId, TextPredicateId, TypeId,
};
pub use ir::{
    Alternative, AtomDescription, AtomExpression, ContainerShape, Declaration, GrammarDescription,
    KindDescription, Location, ParseSite, PrattPosition, PrattVariant, RepeatSeparator,
    RepetitionCardinality, TextEquivalence, TextPredicate, TypeDescription, TypeShape,
};
pub use kind_set::{KindSet, KindSetError, KindSetIter};
pub use provenance::{
    ProofArena, ProofChain, ProofNode, ProofRank, ProvenanceStep, ProvenancedKindSet,
};
