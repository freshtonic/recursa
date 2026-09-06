# Use LALR(1) tables for the table-driven parser style

Status: accepted

Design: [`docs/table-driven-parsing.md`](../table-driven-parsing.md).

## Context

Recursa generates a recursive-descent parser whose decisions are already
frozen tables, but whose node-to-node structure is a Rust call per
nonterminal. The pg-sql benchmark of 2026-09-05 measured that structure
against PostgreSQL's bison parser: both recursive-descent parsers in the
comparison pay about 7.5 times PostgreSQL's cost per nesting level, and
pg-sql runs at 3.4 times PostgreSQL's total time on the frozen corpus.
The measured advantages of the bison machine are that 44.7% of its states
act without reading a lookahead token, and that a nesting level costs one
`i16` push instead of a stack frame, a `Result` match, and drop glue. Both
advantages belong to bottom-up parsing, not to table interpretation as
such.

## Decision

A new parser style, `table_driven`, generates an LALR(1) parser in the
shape bison produces: one grammar-wide automaton, comb-vector tables with
default reductions, one value stack, and a start state per grammar type
so that every type keeps its `Parse` capability as an entry point. The
recursive-descent style is unchanged and remains the default. The
lowering from Recursa's grammar to LR rules is a new analysis over the
same lowered grammar; unresolved conflicts are hard build errors, Pratt
binding powers lower to precedence, and multi-token lookahead is bought once
with a declared lookahead filter, as PostgreSQL does. An author may resolve a
specific LR conflict through a scoped declaration identified by stable
grammar-source identities and lookaheads, never generated state numbers. The
declaration must match exactly the conflict cells it claims, name the intended
action or reducing source rule, and be recorded in the automaton dump; stale,
missing, ambiguous, and
over-broad declarations are build errors.

## Considered options

An explicit-stack pushdown machine driven by the existing FIRST, dispatch
and viability decisions was the cheaper build. It reuses every analysis
Recursa has and cannot surface new conflicts. It was rejected because it
removes only the Rust call frames; it keeps one lookahead read per
decision and one decision per nonterminal, so it cannot reach the two
properties the measurement attributes the gap to.

A generic LR(1) or IELR(1) construction was deferred. gram.y is LALR(1)
with `%expect 0`, and pg-sql mirrors its nonterminals, so LALR merge
artefacts should be rare. The conflict report names a merge artefact when
it sees one, so the need for IELR will be visible rather than guessed.

Multiple `grammar!` invocations, one per parser, were rejected in favour
of several named parsers inside one generation. The macro interprets
nothing today, one generation emits one lexer and one `TokenKind`, and
two parsers over one AST need a marker type to satisfy trait coherence
either way; named parsers get the same result with no macro change.

## Consequences

The grammar an author writes accepts exactly the same language under both
styles only if the author keeps it so. Recursa does not enforce that;
pg-sql enforces it against its differential baseline with both parsers in
one binary. A grammar that is LL(k)-clean today may report LR conflicts
under the new style, and the remedy is a declaration or a restructuring
that still mirrors the PostgreSQL nonterminal, never a silent resolution.
The hidden `GeneratedParse` trait gains a defaulted parser-marker
parameter, which every existing generation compiles against unchanged.
