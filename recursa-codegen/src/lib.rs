//! Source-independent grammar analysis.
//!
//! M2 accepts a normalized [`GrammarDescription`]. Rust source discovery,
//! attribute parsing, and Rust emission belong to later stages.

mod analysis;
mod diagnostic;
mod ids;
mod ir;
mod kind_set;

pub use analysis::{
    AnalysisOutput, AnalysisReport, ExtenderFacts, FirstFacts, FirstPosition, analyze,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, Severity};
pub use ids::{
    AtomId, DeclId, KindId, LocationId, ProofId, SiteId, SourceId, TextPredicateId, TypeId,
};
pub use ir::{
    Alternative, AtomDescription, AtomExpression, Declaration, GrammarDescription, KindDescription,
    Location, ParseSite, PrattPosition, PrattVariant, SiteContainer, TextPredicate,
    TypeDescription, TypeShape,
};
pub use kind_set::{KindSet, KindSetError, KindSetIter};
