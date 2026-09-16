//! The storage-selection contract a generated Flat AST owner implements once per Node
//! application.
//!
//! [`crate::flat`] supplies the stores; this module supplies the one trait that lets a generated
//! owner answer `get` and `span` for any Node type without naming every pool at the call site. A
//! Node's declaration decides whether its handle resolves into a [`crate::flat::TypedPool`] or the
//! [`crate::flat::SharedPool`], so the choice belongs to generated code, not to a runtime branch.
//! `docs/flat-ast-design.md` settles the owner API and
//! `docs/adr/0012-tiered-flat-storage.md` records the decision.

use crate::Span;
use crate::flat::{FlatNode, Handle};

/// Reads one Node type out of a Flat AST store.
///
/// Recursa's generator implements this trait on the grammar's own `FlatAst` owner once per Node
/// application: a pooled application reads its own typed pool, and every other application reads
/// the shared pool. The owner's public `get` and `span` methods then dispatch through it, so one
/// generic method covers every Node type of the grammar.
///
/// An implementation never panics: an out-of-range or foreign handle returns `None`.
pub trait FlatStorage<T: FlatNode> {
    /// Borrows the Node `handle` names, or returns `None` when this store does not hold it.
    fn node(&self, handle: Handle<T>) -> Option<&T>;

    /// Returns the span of the Node `handle` names.
    ///
    /// The result is `None` when the parse did not capture spans, and also when this store does
    /// not hold the Node.
    fn node_span(&self, handle: Handle<T>) -> Option<Span>;
}
