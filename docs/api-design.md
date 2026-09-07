# Recursa API design

Recursa is an AST-first Rust parser generator. Author Rust types express the
semantic tree and grammar attributes express fixed syntax. Generation produces
grammar-local lexing and parsing entry points; a successful strict parse yields
the authored semantic type.

The parser is one implicit LALR(1) machine per grammar. Its state and semantic
value stacks, action/goto tables, and start states are implementation details.
Every generated Node retains `Parse` as its entry point, but authors do not
select parser implementations or configure parser lookahead.

Grammar ambiguity is resolved by ordinary LALR precedence or an exact scoped
conflict resolution. Unresolved conflicts fail generation. Pratt declarations
lower to LR precedence, and fixed-token, container, and lexical declarations
retain the contracts documented in the [facade guide](../recursa/README.md).

The supported grammar surface is the compiled facade API and its Rustdoc. For
the parser architecture, migration of removed declarations, and the acceptance
gates, see [the LALR parser design](table-driven-parsing.md) and
[ADR 0005](adr/0005-sole-lr-parser.md).
