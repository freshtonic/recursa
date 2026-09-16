# Recursa API design

Recursa is an AST-first Rust parser generator. `ast_node!` declarations express
the semantic tree and grammar attributes express fixed syntax. Generation supplies
concrete Node types, their required lifetimes, and grammar-local lexing and parsing
entry points; a successful strict parse yields the generated semantic type.

Node declarations are independent of concrete storage. Explicit `boxed!`,
`one_or_many!`, and `zero_or_many!` markers select indirection and cardinality,
while `arena_ast` selects the nested containers. `Option` remains concrete.
The [Node declaration contract](ast-node-declarations.md) defines the supported
syntax, generated API, trait selection, and bulk migration requirements.

The parser is one implicit LALR(1) machine per grammar. Its state and semantic
value stacks, action/goto tables, and start states are implementation details.
Every generated Node retains `Parse` as its entry point, but authors do not
select parser implementations or configure parser lookahead.

Grammar ambiguity is resolved by the token grammar's `precedence { ... }`
declaration or by an exact scoped conflict resolution. Unresolved conflicts
fail generation. An expression Node is an ordinary left-recursive enum whose
operators take declared levels; see
[declared precedence](declared-precedence.md). Fixed-token, container, and
lexical declarations retain the contracts documented in the
[facade guide](../recursa/README.md).

The supported grammar surface is the compiled facade API and its Rustdoc. For
the parser architecture, migration of removed declarations, and the acceptance
gates, see [the LALR parser design](table-driven-parsing.md) and
[ADR 0005](adr/0005-sole-lr-parser.md).
