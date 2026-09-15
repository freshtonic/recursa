# Declare Nodes independently of AST representation

Status: accepted; declaration API design settled

Make representation-independent Node declarations the canonical grammar source, with concrete
nested and flat AST types derived from those declarations. Choosing authored nested Rust types
as the source privileges one storage representation and makes its containers part of the schema;
declarations instead express child relationships and cardinality independently of concrete storage.
Use `ast_node!` as the authoring surface. The declaration API and its nested output are defined
in [the Node declaration design](../ast-node-declarations.md); flat AST variant design remains
out of scope for this milestone.

Authors retain control over the placement of indirection through `boxed!(T)`; the AST style
chooses its concrete form. Input and storage lifetimes are rendered in the final generated AST
types without requiring that boilerplate at the declaration site.

Use at most one inferred input/storage lifetime per generated Node type, and keep types that do
not require it lifetime-free. This intentionally retires independently authored lifetime parameters
on Nodes; parsing already supplies the same input lifetime to every such parameter.

Replace the old authoring surface in a breaking change rather than carrying a compatibility
period. pg-sql is the only current consumer and is being co-developed with Recursa, so permanent
dual authoring or a staged migration would add obligations without serving an independent user.
Convert pg-sql with a script that handles the whole grammar in one invocation. Its size makes
node-by-node LLM rewriting an unacceptable migration strategy; spend that effort on the converter
and its validation instead.

This supersedes [ADR 0007](0007-derive-flat-asts-from-the-primary-ast.md)'s choice of the authored
nested AST as the canonical source. It preserves the single-schema intent and does not revisit
the independent semantic-storage and ownership decisions in ADRs 0008 and 0009.
