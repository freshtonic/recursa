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

## End-marker reduce/reduce

Every grammar type is a parsing entry point, so the construction has one start
rule per type and the end marker lands in the FOLLOW of every nonterminal that
can end some root. No rule shifts the end marker, so that never makes a
shift/reduce conflict. It does make reduce/reduce conflicts on the end marker,
between rules that can each end a root and that one LALR(1) state holds because
they share an LR(0) core.

Such a conflict is settled by declaration order — the earliest-declared rule
wins, as in bison — rather than reported, when both of these hold:

- the end marker is the one lookahead of the state those rules still contest; and
- every competing reduction **wraps one thing**: a *token-wrapper rule*, whose
  reduce yields the token just popped (`ColIdText -> IDENT` beside
  `TypeFunctionNameText -> IDENT`, an admission-set member or a content token),
  or a *unit rule* `A -> B` of a grammar type — one right-hand-side symbol,
  that symbol a value-carrying nonterminal, so the reduce wraps the one value
  `B` built (`Expr -> SelectWithParens` beside
  `SelectClause -> SelectWithParens`, gram.y's `select_with_parens` factoring).

The choice is recorded for the automaton dump, which prints it beside the state
and counts it in a line of its own, and it is not counted as a conflict.

**Why it is safe.** Every complete item of such a state has its dot at position
one, so the symbol that reached the state is the whole right-hand side of every
competitor: the competitors differ in their left-hand side alone, never in what
they pop. The end marker is the lookahead only when the parse is ending — at
the end of input, or at a token no continuation of any competitor accepts,
where every choice fails at that same token. So the choice decides nothing but
which wrapper a prefix parse yields when the input stops after the wrapped
symbol. The end-marker cell is the only cell the rule writes: every
continuation terminal keeps its explicit action, so a terminal one root accepts
and another does not still reduces its own rule, and a root that continues past
the wrapper still reduces the right one.

**Still contested, not merely shared.** What the argument asks of every other
lookahead is an explicit action, so the first condition reads the conflicts that
remain, not the raw LALR(1) lookahead sets. A cell that declared precedence or a
scoped `lr_conflict` declaration settled carries an explicit action already; a
cell those two leave contested is a hard error, and the whole reduce/reduce is
then reported on every cell it occupies, the end marker included. That division
is what the rule exists for. pg-sql's `Expr -> SelectWithParens` beside
`SelectClause -> SelectWithParens` compete on `RPAREN`, `RPAREN_SELECT_LA` and
`RETURNING` as well, and a declaration names those three. The end marker is the
one cell it cannot name, because a scoped lookahead may only be a token or a
merged kind.

**Its limit.** The choice is exact for each root context in which the
earliest-declared rule can end the parse. A root context that can end only in a
later-declared rule takes the earlier one at the end of input and then finds no
action, which reports an unexpected end of input. That is an LALR(1) merge
artefact of the per-type start rules, and its remedy is declaration order.

**What stays a conflict.** A reduction that wraps more than one symbol is never
settled this way. Nor is a unit rule whose left-hand side is not a grammar type:
a helper nonterminal's reduce/reduce stays reported so that the per-site helper
copy pass still sees it, and a helper copy and a declared restriction are
excluded for the same reason.

A **mixed** competition between a token wrapper and a unit rule cannot arise,
because a complete item at dot one fixes the symbol that reached the state and a
terminal is not a nonterminal; the rule admits each competitor on its own merits
all the same. A **chain** of unit rules (`A -> B` with `B -> C`) never competes
with itself for the same reason, but it does produce successive resolutions: the
goto taken after the chosen reduce can land in another state whose one contested
cell is the end marker, and that state settles in its own right.

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
