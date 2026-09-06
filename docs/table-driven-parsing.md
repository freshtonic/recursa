# Table-driven parsing — Design

Status: accepted design, 2026-09-05. Not yet implemented.
Decision record: [ADR 0004](adr/0004-lalr-tables-for-the-table-driven-parser-style.md).
Motivation and measurements: pg-sql
`docs/research/postgres-parser-performance.md`, sections 4, 7 and 8.
Vocabulary: `CONTEXT.md` (parser style, table-driven parser, named parser,
lookahead filter, reduce action, value stack, prefix acceptance,
binding-power precedence).

This document records the design tree as it was settled, decision by
decision, with the facts each decision rested on. It is the contract for
the implementation. Anything not listed here was not decided.

## 1. Goal and gate

pg-sql must match or exceed PostgreSQL 17.9's `raw_parser`. Done means,
with the table-driven style selected and the same recursa pin:

| Measure | Requirement |
| --- | --- |
| Frozen statement corpus | geomean pg-sql / PostgreSQL median at or below 1.0 |
| `stress/nested_subquery` | marginal cost per nesting level within 1.5x of PostgreSQL |
| Style equivalence gate | green, see section 4 |

The nesting slope is included because it is the one measurement that
isolates parser architecture from constant factors.

## 2. Scope

The parser only. The lexer is already a DFA (logos). The lexer-side items
of the research document stay as follow-ups: classify inside the lexer
(7.3) and the no-backup gate (7.5). The token vector remains the parser's
input; its copy measured at 0.4% of the statement path.

Document framing is unchanged. It calls the selected parser once per
island. The post-payload re-lex is a lexer-mode problem, and a grammar
rule cannot solve it, so framing is not folded into the automaton.

Text predicates (dispatch predicate modes 1 to 6) are not lowered. A
grammar that reaches table mode with one gets a hard error. Fact: pg-sql's
generation has 12,857 dispatch positions and every one is mode 0; no
production code path in recursa-codegen constructs a text predicate.

## 3. The machine

**Algorithm.** LALR(1), lookaheads by DeRemer–Pennello, as bison. The
conflict report states whether a conflict is a merge artefact, present in
LALR(1) and absent in canonical LR(1) for that state, so a later move to
IELR(1) is a measured decision rather than a guess.

**Tables.** Comb-vector packing identical in shape to bison: `pact`,
`table`, `check`, `defact`, `pgoto`, `defgoto`, `r1`, `r2`, plus two
Recursa additions, `value_pops` per rule and `start_state` per grammar
type. Default reductions are mandatory; they are what lets a state act
without reading a token. Tables are rendered as base64 string literals, as
`DispatchTable` already does, so each table is one compiler token.
Estimate for pg-sql from gram.y's figures: about 550 KB of parser tables
for 595 token kinds and 1,452 node types.

**One automaton, one start state per node.** Every grammar type gets a
synthetic start rule in the one shared automaton. A node's `Parse` runs
the machine from its start state and unwraps its value. Nodes never call
each other. `Parse` is a public entry point and nothing more, which is
what the glossary's "Parse capability" already says.

**The end marker in every FOLLOW (#125).** One start rule per type puts
`$end` in the FOLLOW of every nonterminal that can end some root. That
never makes a shift/reduce conflict: no rule shifts `$end`, so a reduce on
it competes only with other reduces; on pg-sql every remaining
shift/reduce conflict is on a real terminal. It does make reduce/reduce
conflicts on `$end` alone between rules that can each end a root and
share an LR(0) state: two token wrappers over one terminal,
`ColIdText -> IDENT .` beside `TypeFunctionNameText -> IDENT .`, reached
from a statement context where the column ends the statement and from a
`FuncName` context where the function name ends the root. When every
competitor is a token-wrapper rule (an admission-set member or a content
token, whose reduce yields the token) and the competitors share no
lookahead but `$end`, the construction takes the earliest-declared rule,
as bison does, records `Conflict between rules N and M resolved on $end
by declaration order` in the dump, and counts no conflict. The rule is
safe because `$end` is the lookahead only when the parse is ending: at
the end of input, or at a token no continuation of either wrapper
accepts, where every choice fails at that token. Every continuation
terminal keeps its explicit action, so a root that continues past the
wrapper reduces the right one, and a root that is genuinely ambiguous at
the end of input yields the first-declared wrapper, which is the
alternative recursive descent takes. The limit: a root context that can
end only in a later-declared wrapper (`FuncName -> ColId . ColId |
TypeFunctionName` on a bare name) reports an unexpected end where
recursive descent accepts; that is an LALR(1) merge artefact of the
per-type start rules, and the remedy is declaration order. The rule
never applies to a pair that also competes on a real terminal, to a
non-wrapper rule, or to a shift/reduce conflict. Measured on pg-sql main
`f45441f`: 355 of 522 conflicts.

**Prefix acceptance.** A root parse is a prefix parse today, and framing
depends on it. In a state whose item set holds `S' -> T .`, a lookahead
with no shift action accepts. A lookahead that can shift always shifts.
This is the greedy boundary recursive descent produces, so both styles
agree on where a root ends. Recursive descent also rejects the token that
follows a root when the last decision it made consulted the caller FOLLOW
(an enum alternative exactly one token long, or a repetition that ends the
rule, reached along the chain of last symbols with no Pratt rule between),
so the table-driven parser carries that decision per rule in a `tail` table,
tracks it along the reductions that follow the final shift, and on
acceptance rejects a following token outside the root FOLLOW with the same
failing token, span and error kind.

**Loop placement.** The automaton loop, `lr::run`, lives in recursa-core
and is generic over the tables. Generated code supplies data and reduce
actions only.

## 4. Invariants

**Type identity across styles.** AST types, `Parse`, `Parsed`, the
grammar-local `Input`, lexing results, framing and every derived trait are
byte-identical between styles. Only the bodies behind `GeneratedParse`
differ. A consumer flips the option and recompiles with no source change.

**Recursa is permissive; the consumer enforces.** Recursa does not require
that two styles accept the same language or build the same values. pg-sql
does, through the style equivalence gate: every named parser must accept
the same corpus statements with identical parsed values, provenance and
failing tokens, checked against the differential baseline with both
parsers in one binary. Recursa's own fixture grammars (`examples/`,
`recursa-test`) are generated with both styles and every fixture input is
parsed by both and compared; that is how the lowering is tested without
imposing a policy on consumers.

**Additive only, behavioural reading.** For an unchanged grammar under
`recursive_descent`, the published generation is identical, the semantic
projection is unchanged, and no existing test moves. New analysis lives in
new modules. Edits to existing files are plumbing: the config key, one
field on `EmissionPlan`, the renderer selection, and the defaulted trait
parameter of section 7.

## 5. Grammar lowering

**Which grammar changes are allowed.** A grammar may gain declarations
that are inert under recursive descent. Nodes may be restructured only
where the result still mirrors the gram.y nonterminal it stands for
(pg-sql principle 9). Recursa never resolves a conflict silently; every
conflict is a hard build error.

**Pratt.** Each `#[pratt]` enum lowers to ordinary rules and its binding
powers to rule precedence and associativity: the level is `lbp`,
`rbp = lbp + 1` is left-associative, `rbp = lbp` or `lbp - 1` is
right-associative, prefix and postfix use `bp` as the rule's precedence,
and interleaved shapes (`BETWEEN a AND b`) become one rule with an
explicit precedence. Precedence decides where a bare last operand
stops. Every other operand (the middle of `BETWEEN a AND b`, the inside
of `a[b]`, anything behind a fixed-token attachment such as pg-sql's
`#[tok(BETWEEN, this)] Box<Self>`) is parsed by the Pratt loop with a
minimum binding power, and lowers to the clone of that minimum,
`Expr[min 8]`: the type minus every LED rule below the minimum, its own
operands unrestricted. A fixed-token attachment around an operand also
inherits its variant's precedence, so its own reduce against the next
operator is decided as the variant's would be. A combination with no
precedence equivalent is a hard error naming the variant (`RCA0402`): an
infix `rbp` other than `lbp + 1`, `lbp` or `lbp - 1`, or an `rbp = lbp -
1` beside an operator at that level. A token's precedence is read off the
rules rather than declared: the level of the infix or postfix operator it
introduces in the reducing rule's nonterminal. A token that introduces
several operators at different levels (pg-sql's `/` starts both `a / b`
and `a / ANY (...)`, which recursive descent tells apart with a second
token) settles a conflict only when every level gives the same verdict;
otherwise the conflict stands, because no one-token decision reproduces
the Pratt parse. `#[parse(pratt(exclude(...)))]` on a field
names a distinct language, so the lowering clones the expression
nonterminal once per distinct exclusion set (`Expr[excl In]`, or
`Expr[excl In, min 8]` with a minimum), the way gram.y has `a_expr`
and `b_expr`; inside the clone the prefix, infix and postfix operands
carry the exclusion (through a cloned fixed-token attachment when one
wraps them) and an atom's operands keep the full language, as
`b_expr: '(' a_expr ')'` does. A resolved conflict is recorded in the
automaton dump (`Conflict between rule N and token T resolved by
precedence (shift)`), as in bison's `.output`, and is not a conflict.
The Pratt loop, `select_pratt` and `RCA4102` do not exist
in table mode; what they guarded is a shift/reduce conflict that
precedence resolves. Facts: pg-sql has one `#[pratt]` enum, `Expr`, with
116 variants; every infix variant has `rbp = lbp + 1`; exactly one
exclusion site exists, the `b_expr` restriction inside `POSITION`.

**Lookahead filter.** LALR(1) reads one token. PostgreSQL closes the gap
with a filter between scanner and parser (`NOT` before `BETWEEN`, `IN`,
`LIKE`, `ILIKE`, `SIMILAR` becomes `NOT_LA`, and five more). Recursa does
the same with a declaration in the `tokens!` macro, beside `admissions`:

```text
lookahead {
    NOT_LA     = NOT     before { BETWEEN, IN, LIKE, ILIKE, SIMILAR },
    NULLS_LA   = NULLS   before { FIRST, LAST },
    WITH_LA    = WITH    before { TIME, ORDINALITY },
    WITHOUT_LA = WITHOUT before { TIME },
    FORMAT_LA  = FORMAT  before { JSON },
}
```

The merged kind exists in `TokenKind` but only a table-driven parser's
token feed produces it. Grammar nodes keep writing `#[tok(NOT, BETWEEN)]`.
Lowering substitutes the merged kind at every rule position where the
next symbol can begin with a trigger. A position where the next symbol can
begin with both a trigger and a non-trigger is a hard error naming the
node and field. A token that ends its rule is decided by the FOLLOW of the
rule's nonterminal instead; when that FOLLOW holds both triggers and
non-triggers the rule is emitted twice, once with each kind, which is
conflict-free because the automaton chooses between the twins by the
terminal it shifts. The declaration is inert under recursive descent. Fact:
of pg-sql's 528 decision tables, 377 walk deeper than one token and 137
exhaust `max_lookahead = 5`; that is an LL fact, mostly shared prefixes
that LR resolves by deferring the reduction, and the true filter set is
known only after the first construction run. PostgreSQL's six are the
expected set because pg-sql mirrors gram.y.

**Containers and admission sets.** `Vec<T>` and `Vec1<T>` with separators
lower to left-recursive list rules, keeping the stack shallow and building
the vector in order. `Option<T>` lowers to a nullable nonterminal. Erased
optional fixed-token groups lower to a nullable rule with no value.
Fixed-token presence lowers to a `bool`. A token admission set lowers to
one nonterminal with one rule per member kind, gram.y's
`unreserved_keyword:`, whose reduce yields the token value. None of these
has a real alternative; each is listed because it is a place where the
two styles could silently disagree.

**Where a nullable nonterminal lives (#114).** A nullable helper carries an
epsilon reduce that LALR(1) must take before the token that decides the
choice. When that token is a keyword an admission set also accepts as
content, the epsilon reduce competes with the shift of the same keyword;
the first pg-sql construction showed 2,970 such shift/reduce conflicts on
`opt(...)` helpers and 421 reduce/reduce conflicts between the shared
erased groups of `ALTER [COLUMN] ... SET DEFAULT` and `DROP DEFAULT`.
gram.y avoids both by writing the alternatives out by hand, and so does the
lowering: a fixed-only choice (an erased group, a presence, a trailing
separator) is always inlined into the parent rule as its terminals; a
valued choice (`Option<T>`, a zero-or-more list) is inlined when its
content can begin with an admitted keyword and delegated to a shared helper
(`opt(...)`, `presence(...)`, `list(...)`, `list1t(...)`) otherwise. A rule
inlines at most sixteen alternatives; past that, choices are demoted to
helpers from the right, valued choices first. Repetitions always share the
core `list1 -> e | list1 [sep] e`. Measured on pg-sql, the same day: total
conflicts 17,220 to 15,353, the `opt(...)` shift/reduce family 2,970 to
301, its reduce/reduce family 425 to 16, the erased-group and
`WITH [RECURSIVE]` reduce/reduce pairs to zero; the cost is rules 11,586 to
12,006, states 17,549 to 18,515 and encoded tables 814 KB to 1.33 MB, the
last because inlined alternatives give more states explicit actions that
comb packing cannot share.

**Per-site copies of a shared helper (#125).** A shared helper gives
every LR(0) state whose kernel holds one of its partially or fully parsed
rules (`opt(X) -> X .`, `list1 -> list1 . sep e`) one core across all of
its use sites, and LALR(1) unions the sites' lookaheads into it. That
union can only manufacture reduce/reduce conflicts: a state's shift set
is a property of its LR(0) core, so a shift/reduce conflict LALR(1)
reports also exists in some canonical LR(1) state, and no renaming of
nonterminals removes it. So the construction looks only at reduce/reduce
conflicts in states whose kernel holds a helper item with the dot past
the start, and gives such a helper one copy per co-occurrence class of
its sites: two sites share a copy when some LR(0) state holds both of
their items with the dot before the helper, because separate copies
there would force a choice between the sites before the parser has seen
what tells them apart, a conflict the shared form did not have. Under
that rule every state of the copied grammar maps, with copy names
erased, onto a state of the shared one with the same items, so
lookaheads only shrink and no new conflict appears. The first class
keeps the helper; each other class gets `opt(X)@site`, named after its
first site (`@lhs`, or `@lhs#k` when the left-hand side has several
sites), with the original's rules (self-references redirected), reduce
actions, folds and tails. The automaton is rebuilt after each round
until a round copies nothing, at most `MAX_ROUNDS` (8) rounds; a copy
owns one class, so a helper splits again only when a copy of an
enclosing helper gives it new sites. A copy made for a genuine conflict
stays; its cost is its rules and states. The up-front alternative, one
copy per distinct static FOLLOW set, was not taken: it cannot see
co-occurrence, so it can create conflicts, and it copies helpers that
conflict nowhere. Measured on pg-sql main `f45441f`: one copy
(`list1(FuncArg, sep=COMMA)@CallArguments`), rules 16,307 to 16,309,
states 22,600 to 22,603, encoded tables 2,318,758 to 2,353,218 bytes
(1.5%), one extra construction round (40.7 s to 44.5 s); the copy's
conflict is genuine and stays, with the other 26 reduce/reduce and 140
shift/reduce conflicts that are pg-sql's.

## 6. Runtime

**Value stack.** A type-erased byte stack with a parallel `(offset,
drop_fn)` stack, behind a small sealed API with `SAFETY` comments in the
style recursa-core already uses. Reduce pops raw bytes of a statically
known size and pushes the constructed value: no per-node allocation and
one memcpy per reduce. Fixed tokens are never pushed. Only the state
stack and, when capturing, the occurrence stack see them, so each rule
carries `value_pops` separately from bison's `r2`. The stack is one
generic runtime type so that a flat AST backend (api-design A7) can later
substitute `u32` refs without touching the automaton. Lints that reject
`unsafe` are adjusted as needed.

**Provenance.** A parallel occurrence stack, pushed on shift and folded on
reduce, active only when the capture request or the node's policy asks
for it, or when the grammar has a mandatory site (an erased optional
fixed-token group), which recursive descent captures whatever the request.
When capture is off and the grammar has no mandatory site the stack is
never touched. The fold of each rule is a
packed table (`folds`) rendered from the same site forms as the reduce
actions: one node per construction rule with one child per site in
grammar-element order, the helper rules folding to what their inlined form
would have produced, every span computed as `consumed_span_from` computes
it and every occurrence compacted by the same collector. An inherited
capture policy resolves to the entry type's, so an authored
`#[spanned(...)]` override below the root is rejected (`RCA0406`): a child
rule is reduced before the site that would pass the override down is known.
Output must equal recursive descent's provenance for the same input; the
pg-sql corpus checks it.

**Errors.** LR with default reductions detects an error at the same token
as recursive descent, but after some reductions, so the natural expected
set is smaller. The failing token is what the differential suite
compares, so it must match. The expected set is computed by LAC-style
exploration (bison's lookahead correction) on the error path only, and it
may differ from recursive descent's set. Root parses stay transactional:
on failure the input cursor is restored and the value stack is drained
through its drop stack.

## 7. Configuration and public surface

**Single parser.** `parser_style = recursive_descent | table_driven` in
the root `grammar!` configuration, default `recursive_descent`. The key
is parsed beside the existing ones in `source_syntax.rs`, carried on
`GrammarConfigSyntax`, threaded to `EmissionPlan`, and it changes the
generation identity hash.

**Named parsers.** `parsers(rd = recursive_descent, lr = table_driven)`,
mutually exclusive with `parser_style`. Names are author-chosen
identifiers. The first entry is the default parser. One generation emits
one lexer, one `TokenKind`, one AST, and one `GeneratedParse` impl per
node per parser. Each style may be declared once: a repeated style is
`RCA1017`, because the parsers of one style share their marker and their
per-node helpers, so a second one would be the same parser twice. Multiple
`grammar!` invocations were rejected: the macro interprets nothing,
discovery enforces exactly one config (`RCA1016`), and two invocations
would emit two token enums for one AST.

**Selecting a parser.** The default parser owns today's surface
unchanged: the root `Input`, `lexed.input()`, framing, the explorer
adapter, and the inherent `Foo::parse(&mut Input)` and `parse_spanned`.
Each extra parser gets a generated module `crate::parsers::<name>` with
its own `Input<'input, A = ()>` newtype, an `input(&lexed)` constructor
and `input_with_builder(&lexed, builder)`. The sealed `Parse<I>` impl is
selected by the input type, and so are the inherent `Foo::parse` and
`parse_spanned`: with an extra parser declared they accept every input
type of the generation (an inherent associated function shadows a trait
one in a path call, so the trait alone would not do), and pg-sql's
differential test writes `Statement::parse(&mut lr::input(&lexed))` and
`Statement::parse_spanned(&mut lr::input(&lexed))` and nothing else
changes. A single-parser generation keeps the inherent entry points over
its root `Input` alone, spelled exactly as before. Framing and the
explorer adapter drive the default parser through facade seams bound to
the recursive-descent impl, so a table-driven default parser beside
either capability is refused at generation with a remedy: declare the
recursive-descent parser first.

**The one trait change.** `GeneratedParse<'input, A>` gains a third
parameter `P = RecursiveDescent`, and `GeneratedInput` gains an
associated `type Parser`. Coherence forbids two impls of one trait for
one type, and the marker is the only way to have two parsers over one
AST. Every existing generation compiles unchanged because the default
fills in. The markers are the two style markers, `RecursiveDescent` and
`TableDriven`; no marker type is generated per named parser. The
recursive-descent bodies recurse through facade helpers bound to the
trait-default marker, so that style cannot carry another marker, and one
parser per style leaves nothing for a generated marker to distinguish.

**Diagnostics.** New codes in the analysis stage, appended to the
registry:

| Code | Meaning |
| --- | --- |
| `RCA0400` | shift/reduce conflict |
| `RCA0401` | reduce/reduce conflict |
| `RCA0402` | Pratt binding power with no precedence equivalent |
| `RCA0403` | text predicate not supported in table mode |
| `RCA0404` | ambiguous lookahead-filter substitution |
| `RCA0405` | note: conflict is an LALR merge artefact |
| `RCA0406` | capture policy override not supported in table mode |

A conflict report carries the state's items, the conflicting lookahead,
the competing actions, and the shortest path from the start state to the
conflict state. Bison-style counterexample derivations are not in v1.

## 8. The generated shape

Table mode with a parser named `lr` beside the default. In `root.rs`
(the implemented shape reuses the style marker and nests the module
under `parsers`):

```rust
pub mod parsers { pub mod lr {
    pub struct Input<'input, A = ()> { inner: recursa::__private::Input<'input, A> }
    impl<'input, A> GeneratedInput for Input<'input, A> { type Parser = TableDriven; /* ... */ }
    pub fn input<'i>(lexed: &LexResult<'i>) -> Input<'i> { /* ... */ }
    pub fn input_with_builder<'i, A>(lexed: &LexResult<'i>, builder: A) -> Input<'i, A> { /* ... */ }

    static __RECURSA_LR: recursa::__private::LrTables = recursa::__private::lr_tables(
        b"...pact...", b"...table...", b"...check...", b"...defact...",
        b"...pgoto...", b"...defgoto...", b"...r1...", b"...r2...",
        b"...value_pops...", b"...start_states...",
        &__RECURSA_EXPECTED_SETS,
    );

    pub(crate) fn __recursa_reduce<'i, A>(
        rule: u16,
        stack: &mut ValueStack<'i>,
        spans: &mut OccurrenceStack,
        builder: &mut A,
    ) -> Result<(), ParseError> {
        match rule {
            0..=3  => crate::ast::Statement::__recursa_reduce_lr(rule, stack, spans, builder),
            4..=17 => crate::ast::SelectStmt::__recursa_reduce_lr(rule - 4, stack, spans, builder),
            // ...
        }
    }
}
```

In each `node-N.rs`, beside the untouched recursive-descent impl:

```rust
impl<'i, A> GeneratedParse<'i, A, lr::Parser> for SelectStmt<'i> {
    const START_STATE: u16 = 1234;
    const VALUE_SLOT: u16 = 17;
    fn parse(cx: &mut ParseContext<'_, 'i, A>) -> Result<ParsedNode<Self>, ParseError> {
        recursa::__private::lr::run::<Self, A>(cx, &lr::__RECURSA_LR, lr::__recursa_reduce)
    }
}

impl<'i> SelectStmt<'i> {
    fn __recursa_reduce_lr<A>(
        rule: u16,
        stack: &mut ValueStack<'i>,
        spans: &mut OccurrenceStack,
        builder: &mut A,
    ) -> Result<(), ParseError> {
        match rule {
            0 => {                       // SelectStmt -> SELECT targets from_clause
                let from = stack.pop::<Option<FromClause<'i>>>();
                let targets = stack.pop::<Vec1<Target<'i>>>();
                stack.push(SelectStmt { targets, from });
            }
            // ...
        }
        Ok(())
    }
}
```

Three points inside the sketch are decisions. Fixed tokens never reach
the value stack, so value-stack traffic is proportional to semantic
content, where PostgreSQL's pointer stack already is. The reduce
dispatcher is one generated generic function with a range match, not a
function-pointer table, because a reduce generic over the builder cannot
sit in a static; it monomorphises once per builder type. A node's LR
impl carries only its start state and value slot; the loop is in the
facade.

## 9. Facts the design rests on

Measured on 2026-09-05 from pg-sql's newest generation and source.

| Fact | Value |
| --- | --- |
| Token kinds | 595 |
| Node types | 1,452 |
| Fixed-token uses | 1,796 |
| Decision tables | 528 (450 predictive, 76 optional viability, 2 Pratt) |
| Tables deeper than one token | 377 |
| Tables exhausting `max_lookahead = 5` | 137, including the 1,765-state `Statement` dispatch |
| Text predicates in generated code | 0 |
| `#[pratt]` enums | 1 (`Expr`, 116 variants, all infix `rbp = lbp + 1`) |
| `pratt(exclude)` sites | 1 (`POSITION` needle, 22 variants excluded) |
| `#[parse(backtrack)]` | documented, not implemented, zero uses |
| `unsafe` in recursa-core | present (`grammar.rs`, `explorer.rs`); nothing forbids it |
| gram.y for comparison | 539 terminals, 728 nonterminals, 3,408 rules, 6,458 states, about 553 KB of tables |

## 10. Follow-ups outside this design

- Lexer items 7.3 and 7.5 of the research document.
- Text-predicate lowering for the recursive-descent style (7.1), now
  known to have no caller in pg-sql.
- IELR(1), only if `RCA0405` fires on a real pg-sql conflict; it would
  also lift the limit of the `$end` declaration-order rule (section 3).
- Undo a helper copy that removed no conflict, or classify the conflict
  per state (lane tracing) before copying; one such copy on pg-sql.
- Counterexample derivations in conflict reports.
- A flat AST backend over the same value stack (api-design A7).
- Housekeeping noticed on the way: pg-sql's `[package.metadata.recursa.ast]`
  is read by nothing, and pg-sql's CLAUDE.md principle 1 still names
  `FormatTokens`, which is now `Pretty`.
