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

**A merged kind is admitted only where the grammar names it.** Lowering
substitutes merged kinds into the lowered rules over FIRST of the unsubstituted
grammar, which is what the *same rule* can put after the position:

- a position whose rest can only begin with a trigger becomes the merged kind,
  as `gram.y` writes `WITH_LA TIME ZONE`, `WITHOUT_LA TIME ZONE` and
  `NULLS_LA FIRST_P`;
- a position whose rest can begin with a trigger or with something else, which
  includes every rest that can derive nothing, is emitted twice, once per kind,
  as `gram.y` writes `a_expr: NOT a_expr | NOT_LA a_expr` by hand;
- a position no trigger can follow inside its own rule keeps the source token.

FOLLOW never drives a substitution. A rule-final token has nothing after it in
its own rule, so a pass that consulted FOLLOW decided over the terminals that
follow the whole nonterminal *anywhere in the grammar*. For a keyword-admission
rule such as `ColLabelText -> NOT` that reaches nearly every trigger, so such a
pass derived `ColLabelText -> NOT_LA`, `ColIdText -> WITHOUT_LA` and
`ColLabelText -> FORMAT_LA` — arms `gram.y` never writes, and the reason a
pre-19 build accepted `COPY t TO STDOUT (FORMAT json)`. `gram.y` has no `_LA`
token in `unreserved_keyword`, `col_name_keyword`, `type_func_name_keyword`,
`reserved_keyword`, `bare_label_keyword`, `ColId`, `ColLabel` or
`NonReservedWord`.

A rule-final token therefore keeps the source token unless the grammar names the
merged kind at that site. **A token position names a merged kind by its declared
name**, `#[tok(WITH_LA)]` or `#[tok(NOT_LA, LIKE, this)]`, the way `gram.y`
writes `opt_with: WITH | WITH_LA`, `any_with: WITH | WITH_LA`,
`utility_option_name: ... | FORMAT_LA` and `opt_nulls_order: NULLS_LA FIRST_P`.
Such a position carries the merged terminal and nothing else; the substitution
pass never revisits it. The position keeps the merged token's own type, so the
value, the spelling, the span and the Pretty rendering are that token's — a
merged kind is the same record read differently, not a second token. A name that
is neither a token nor a declared merged kind is `RCA3106`. A `#[sep(...)]`
separator cannot admit a merged kind, because a separator normalizes to a type
the repetition refers to rather than to a position of its own; naming one there
is `RCA3153`.

A declaration whose merged kind no site admits and no FIRST substitutes is not
an error. The filter still fires, and the merged kind no state has an action for
is a syntax error there, which is the verdict PostgreSQL reaches for the same
input.

Nodes otherwise keep writing the source token; see
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
