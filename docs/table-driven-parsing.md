# Recursa's LALR parser — Design

Status: accepted design, 2026-09-07. Decision record: [ADR 0005](adr/0005-sole-lr-parser.md), which supersedes the dual-parser portions of [ADR 0004](adr/0004-lalr-tables-for-the-table-driven-parser-style.md).

Recursa generates one implicit parser: an LALR(1) machine with a grammar-wide
automaton, comb-vector action and goto tables, default reductions, a state
stack, a semantic value stack, and a start state per grammar type. Publicly it
is simply the Recursa parser; “LR” and “table-driven” describe its internal
architecture, tables, conflicts, and performance characteristics.

## Scope and grammar contract

The lexer supplies the significant-token stream. The parser reduces LR rules
lowered from the authored semantic grammar; each reduce action constructs the
semantic value and, when requested, folds exact occurrence provenance in
parallel. Fixed tokens carry no semantic value. Document framing and Explorer
invoke this sole parser for each strict island.

LALR conflicts are hard generation errors. Pratt binding powers lower to LR
precedence. Scoped conflict resolutions name source-level rules and
lookaheads, never generated state numbers, and must match exactly the cells
they claim. Text predicates remain an unsupported grammar construct until they
have a useful LR lowering. A future IELR or canonical-LR mode would be a
single grammar-level construction choice, never a second runtime parser.

The grammar language has no parser selection. `parser_style`, `parsers(...)`,
`max_lookahead`, and balanced predictive-dispatch configuration are removed;
old declarations receive a targeted migration diagnostic. There are no parser
markers, named parser modules, parser-generic generated interfaces, or
recursive-descent fallback.

## Acceptance gates

Completion requires all retained Recursa fixtures and examples to use the one
parser, no production predictive-dispatch implementation, and no active
documentation promising recursive descent or multiple parsers. Recursa's full
suite must pass. pg-sql must report zero LALR conflicts and retain its pinned
234/234 PostgreSQL differential. The manual performance gates are forced
generation at most 10 seconds, generation reuse at most 1 second, and a
release rebuild below 60 seconds. Canonical merge-conflict classification is
lazy: it runs only when a final unresolved conflict needs that diagnostic.

The lexer remains a DFA and document framing remains a strict exact-source
partitioning layer; neither is folded into the LR automaton.
