//! The storage-selection contracts a generated Flat AST owner implements, once per Node
//! application.
//!
//! [`crate::flat`] supplies the stores. This module supplies the traits that let a generated owner
//! answer `get`, `span` and `pool` for any Node type. The call site therefore names no pool of its
//! own. A Node's declaration decides whether its handle resolves into a [`crate::flat::TypedPool`]
//! or the [`crate::flat::SharedPool`], so the choice belongs to generated code rather than to a
//! runtime branch.
//!
//! `docs/flat-ast-design.md` settles the owner API and
//! `docs/adr/0012-tiered-flat-storage.md` records the decision.

use crate::Span;
use crate::flat::{FlatNode, Handle, TypedPool};

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

/// Borrows the typed pool of one pooled Node type.
///
/// Recursa's generator implements this trait on the grammar's own `FlatAst` owner once per pooled
/// Node application, because only that application's own pool holds its Nodes. The owner's public
/// `pool` method dispatches through it, so `ast.pool::<FlatExpr>()` reaches one pool of the
/// grammar without naming the field the generator chose.
///
/// The pool is read-only: it iterates its handles in post-order and reports its length, which is
/// what a consumer needs to size a side table.
pub trait FlatPool<T: FlatNode> {
    /// Borrows the pool that holds every Node of type `T`.
    fn pool(&self) -> &TypedPool<T>;
}
