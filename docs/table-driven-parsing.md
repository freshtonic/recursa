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

A grammar that selects `flat` gets a second set of reduce actions over the same
automaton, tables and value stack, which builds a Flat AST instead of the nested
one and never folds provenance. Each flat action pops and pushes the same counts
as the nested action of its rule, so both share every packed table; see the
[flat AST design](flat-ast-design.md).

LALR conflicts are hard generation errors. The `precedence { ... }`
declaration of the token grammar is the sole source of rule and token
precedence, in bison's exact terms; see
[declared precedence](declared-precedence.md). Scoped conflict resolutions name source-level rules and
lookaheads, never generated state numbers, and must match exactly the cells
they claim. To gate a resolution on a build configuration, write it as
`#[cfg_attr(PREDICATE, parse(lr_conflict(...)))]`, or, with a declared
configuration list, `#[config_attr(since = NAME, parse(lr_conflict(...)))]`
(see [declared build configurations](configuration-requirements.md)).
Generation ignores an inactive resolution. An active resolution that matches no cell is still an
error. Text predicates remain an unsupported grammar construct until they
have a useful LR lowering. A future IELR or canonical-LR mode would be a
single grammar-level construction choice, never a second runtime parser.

The grammar language has no parser selection. `parser_style`, `parsers(...)`,
`max_lookahead`, and balanced predictive-dispatch configuration are removed;
old declarations receive a targeted migration diagnostic. There are no parser
markers, named parser modules, parser-generic generated interfaces, or
recursive-descent fallback.

## Lookahead filters

Where one token of lookahead cannot decide the parse, the token grammar may
declare a lookahead filter: `lookahead { MERGED = TOKEN before { TRIGGER, ... }
}`. The parser's token feed then presents a `TOKEN` record as `MERGED` whenever
the record after it is one of the triggers. This is PostgreSQL's `base_yylex`
filter (`src/backend/parser/parser.c`), which keeps `gram.y` LALR(1) by
replacing `NOT` with `NOT_LA`, `WITH` with `WITH_LA` and so on.

**The substitution is absolute.** Once the next record is a trigger, the merged
kind is the only terminal looked up, and a state with no action for it reports a
syntax error on that record. The source terminal is never retried, exactly as
bison never retries one: a lookahead token is the token, and a row without it is
an error. A grammar position that admits the source token where a trigger may
follow it must therefore admit the merged kind as well, which is why `gram.y`
writes `opt_with: WITH | WITH_LA`, `any_with: WITH | WITH_LA` and
`utility_option_name: ... | FORMAT_LA` by hand. The corollary is the reverse too:
PostgreSQL rejects `COPY t TO STDOUT (FORMAT json)` through release 18 because
`copy_generic_opt_elem: ColLabel copy_generic_opt_arg` has no `FORMAT_LA` arm,
and a faithful grammar must reject it as well.

Generation carries the author's burden where it can. Lowering substitutes merged
kinds into the lowered rules over FIRST and FOLLOW of the unsubstituted grammar:
a position whose continuations are all triggers becomes the merged kind, a
position whose continuations mix triggers and others is emitted twice, once per
kind, and a position no trigger can follow keeps the source token. Because FIRST
and FOLLOW over-approximate, every position a trigger can reach has its merged
form, so absoluteness rejects only what the grammar never admitted. Nodes keep
writing the source token; see
[the lowering pass](../recursa-codegen/src/lr/lookahead.rs).

The merged kind is a token kind of the grammar that is never lexed and never a
trigger, it displays as the token it merges, and a shifted record keeps its lexed
kind and span, so reduce actions, provenance and the error path never see the
merged kind. An expected set that names it names the spelling the author wrote,
because the merged kind shares the expected atom of its source token. A
`precedence` level may name a merged kind, which is how `gram.y` gives
`NOT_LA LIKE` the precedence of `LIKE`.

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
