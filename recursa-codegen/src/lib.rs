//! Source-independent grammar analysis.
//!
//! M2 accepts a normalized [`GrammarDescription`]. Rust source discovery,
//! attribute parsing, and Rust emission belong to later stages.

mod analysis;
mod build_diagnostic;
mod decision_plan;
mod diagnostic;
mod discovery;
mod ids;
mod ir;
mod kind_set;
mod lexical;
mod lowering;
mod m3;
mod m3_projection;
mod materialization;
mod provenance;
mod resolution;
mod source_model;
mod source_syntax;

#[cfg(test)]
mod m3_contract_tests;
#[cfg(test)]
mod m3_pipeline;
#[cfg(test)]
mod m3_projection_tests;

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
    DecisionProducts, DispatchBranch, DispatchKind, DispatchPlan, DispatchSequence, ExpectedRecipe,
    FailureEdge, FailureEdgeKind, FollowRecipe,
};
pub use diagnostic::{
    ConflictEvidence, ConflictRemedy, ConflictWitness, Diagnostic, DiagnosticCode, DiagnosticLabel,
    LookaheadWitnessPosition, Remedy, Severity, SharedPrefixEvidence,
};
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
