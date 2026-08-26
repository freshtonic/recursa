//! Source-independent grammar analysis.
//!
//! M2 accepts a normalized [`GrammarDescription`]. Rust source discovery,
//! attribute parsing, and Rust emission belong to later stages.

mod analysis;
mod diagnostic;
mod discovery;
mod ids;
mod ir;
mod kind_set;
mod lowering;
mod provenance;
mod resolution;
mod source_model;
mod source_syntax;

pub use analysis::{
    AnalysisOutput, AnalysisReport, ExtenderFacts, FirstFacts, FirstPosition, FirstSequence,
    FollowFacts, analyze, disambiguation_depth,
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
    KindDescription, Location, ParseSite, PrattPosition, PrattVariant, RepetitionCardinality,
    TextEquivalence, TextPredicate, TypeDescription, TypeShape,
};
pub use kind_set::{KindSet, KindSetError, KindSetIter};
pub use provenance::{
    ProofArena, ProofChain, ProofNode, ProofRank, ProvenanceStep, ProvenancedKindSet,
};
