//! The rendering contract one generated flat Node type implements per Flat AST store.
//!
//! A nested Node owns its children, so [`crate::Pretty`] needs nothing but the value itself. A
//! flat Node holds four-byte handles, so rendering one needs the store that resolves them. This
//! module supplies the one trait that carries the store alongside the value; [`crate::flat_owner`]
//! supplies the storage selection the trait's implementations read through.
//!
//! Recursa's generator implements this trait once per flat Node type, against the grammar's own
//! `FlatAst` owner. The owner's `pretty`, `render`, `render_with` and `pretty_node` methods
//! dispatch through it, so one generic method renders any Node of the grammar. A flat render
//! carries no occurrence provenance, so it emits the canonical spelling of every fixed token and
//! omits optional fixed syntax it cannot have captured, exactly as a detached nested AST does.
//!
//! `docs/flat-ast-design.md` settles the owner API and
//! `docs/adr/0008-keep-flat-asts-semantic-only.md` records the rendering contract.

use core::fmt;

use crate::pretty::PrettyContext;

/// Renders one flat Node out of the store that holds its children.
///
/// `S` is the Flat AST store the Node belongs to, which is the grammar's generated `FlatAst`
/// owner. An implementation resolves each of its handles through that store and emits the same
/// document a detached nested value of the same Node would emit.
pub trait FlatPretty<S: ?Sized> {
    /// Emits this Node into `context`, resolving its children through `store`.
    fn pretty_in(&self, store: &S, context: &mut PrettyContext) -> fmt::Result;
}
