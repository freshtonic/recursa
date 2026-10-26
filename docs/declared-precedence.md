# Declared precedence as the sole precedence model — Specification

Status: accepted design, 2026-09-15. Decision record: [ADR 0011](adr/0011-declared-precedence-replaces-pratt.md), which supersedes the "Pratt binding powers lower to precedence" clause of [ADR 0004](adr/0004-lalr-tables-for-the-table-driven-parser-style.md).

Recursa resolves every LR shift/reduce conflict with one precedence model: the
model bison uses and PostgreSQL's `gram.y` is written against. The Pratt
authoring surface (`#[pratt]`, `#[parse(prefix | infix | postfix, ...)]`,
`#[parse(pratt(exclude(...)))]`), its binding-power scale, its automatic
restriction clones, and every consumer of Pratt facts are removed. Expression
grammars are ordinary left-recursive enums whose operator conflicts are decided
by a `precedence { ... }` declaration in the token grammar and by per-rule
overrides.

## Evidence

The decision rests on three verified facts.

1. **PostgreSQL 17.9 decides expression shape entirely in the bison tables.**
   The precedence block is `gram.y:829-908`, `%expect 0` is at line 223, and
   `%prec` is used freely in `a_expr` and `b_expr` (`UMINUS`, `AT`, `Op`,
   `NOT`, `LIKE`, `IS`, `BETWEEN`, `NOT_LA`). Multi-token lookahead is bought
   once in `parser.c`'s `base_yylex`, which rewrites `NOT`, `NULLS`, `WITH`,
   `WITHOUT` and `FORMAT` into `_LA` tokens. There is no operator-precedence
   loop in C. `b_expr` exists because precedence cannot separate `BETWEEN`'s
   `AND` from boolean `AND`; `gram.y` removes the boolean-keyword productions
   by hand (`gram.y:14762-14766`, `15284-15288`). `c_expr` is shared factoring
   only (`gram.y:14771-14772`).
2. **Pratt is pg-sql's largest remaining structural cost.** pg-sql's research
   note records that the 22 exclusion and minimum-binding-power languages copy
   2,301 rules and 117 nonterminals, 11.7% of all automaton states.
   PostgreSQL's hand-factored `a_expr`/`b_expr`/`c_expr` is 104 rules.
3. **A plain enum with declared precedence already works, with one gap.** A
   throwaway crate rewrote `examples/pratt-expressions` as a plain enum with a
   `precedence` block. It generated conflict-free and passed the example's
   behaviour tests unchanged when every operator token carried its own level.
   It failed with four `RCA0400` conflicts when the prefix rule relied on
   `#[parse(prec = UMINUS)]`, because the override is applied to the variant's
   rule while the conflict is inside the attachment unit that
   `#[tok(MINUS, this)]` materializes. The failure was also reported as an
   internal invariant failure (`RCA9105`) rather than as an authoring error.

## The precedence model

The model is bison's, stated in Recursa's terms.

- **Levels.** The token grammar declares ordered levels. Each level is `left`,
  `right` or `nonassoc` and lists token names and precedence-only names. A
  lower level binds looser. The numeric `bp` on a level orders the levels; it
  has no other meaning.
- **Precedence-only name.** A level member that names no token, such as
  `UMINUS`. It exists only so that a rule override can take that level. A
  precedence-only name that no override uses is a build error.
- **Rule level.** A rule takes the level of its last terminal that has a
  declared level. A Node or variant may name a level with
  `#[parse(prec = NAME)]`; a variant override wins over a Node override, and an
  override wins over the last terminal. A rule with no declared terminal and
  no override has no level.
- **Resolution.** For a shift/reduce conflict, compare the reducing rule's
  level with the lookahead token's level. The higher rule reduces; the higher
  token shifts. On a tie, `left` reduces, `right` shifts, and `nonassoc` makes
  the cell an error action. If either side has no level, the conflict stays
  unresolved and is reported as `RCA0400`, the ordinary build error, with the
  remedy of declaring a level or an override.
- **No second source.** The resolver has no Pratt path, no per-nonterminal
  token levels, and no "levels straddle the rule" case. Reduce/reduce
  conflicts are never resolved by precedence.

## Authoring surface

An expression Node is an ordinary enum. Recursive operands are `boxed!(Self)`.
Operator tokens attach to the operand field they precede or follow.

```rust
recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation { PLUS => "+", MINUS => "-", STAR => "*", CARET => "^",
                  BANG => "!", LPAREN => "(", RPAREN => ")" }
    precedence {
        left(bp = 10) { PLUS },
        left(bp = 20) { STAR },
        right(bp = 30) { CARET },
        right(bp = 35) { UMINUS },
        left(bp = 40) { BANG },
    }
}

recursa::ast_node! {
    pub enum Expr {
        Integer(Integer),
        Name(Name),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),
        #[parse(prec = UMINUS)]
        Negate(#[tok(MINUS, this)] boxed!(Self)),
        Factorial(#[tok(this, BANG)] boxed!(Self)),
        Power(boxed!(Self), #[tok(CARET, this)] boxed!(Self)),
        Multiply(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
```

This is the language of the current `examples/pratt-expressions` with the same
AST type. `#[parse(prec = NAME)]` is accepted on any Node or variant.

A name in a level can have `#[cfg(...)]` attributes, for example
`nonassoc(bp = 110) { RANGE, #[cfg(feature = "v2")] PATH }`. Generation
removes an inactive name from its level, and removes a level that has no active
name. The checks on the block shape (ascending `bp`, one or more names in each
level, each name one time only) apply to the block as written, in all
configurations. An active name that refers to an inactive token is an error.
A `#[config(since = NAME)]` gate is accepted in the same place and removes a
name in the same way; see
[declared build configurations](configuration-requirements.md).

### Restricted expressions

Declared precedence cannot express `b_expr`, and `gram.y` does not try. A
grammar that needs an interior operand to stop at a token that also closes the
enclosing syntax writes a restricted expression Node by hand:

```rust
recursa::ast_node! {
    /// gram.y `b_expr`: `a_expr` without the boolean-keyword productions.
    #[restricts(Expr)]
    pub enum BExpr {
        Integer, Name, Parenthesized, Negate, Power, Multiply, Add,
    }
}
```

A restricted expression names its base Node and lists, by name only, the base
variants it admits. It has no fields of its own and is not an AST type: every
rule it lowers constructs the base type. Its rules are the admitted base
variants with one substitution: a recursive operand that is the first or the
last symbol of the rule is retyped to the restricted Node, and every other
operand keeps its base type. That is exactly `gram.y`'s `b_expr`: `b_expr '+'
b_expr` but `'(' a_expr ')'`. A restricted Node may name another restricted
Node as its base. Listing a variant the base lacks, or a base that is not an
enum Node, is a build error. Interior operands need a restricted Node only
where `gram.y` uses one; the author follows `gram.y` case by case and never
derives the restriction from an exclusion list.

`#[restricts(Base, leading)]` restricts only the operand that leads a rule, and
an operand that ends a rule keeps its base type. It is for a position that
excludes a leading form and nothing else. `gram.y`'s single rule action is
one: a `(` after `DO ALSO` opens the action list, so the action's
`select_clause` cannot lead with a parenthesized query, but `SELECT 1 UNION
(SELECT 2)` is an action. With the option the rule is `Action UNION Expr`;
without it, `Action UNION Action`, which rejects that statement. The trailing
restriction is what `b_expr` needs, so it stays the default.

### Attachment units inline

A semantic field with attached fixed tokens and cardinality one
(`#[tok(MINUS, this)] boxed!(Self)`, `#[tok(LPAREN, this, RPAREN)] boxed!(Self)`)
lowers inline into its owner's rule instead of into a private attachment
nonterminal. `Negate` lowers to `Expr -> MINUS Expr`, and `Power` to
`Expr -> Expr CARET Expr`. The reduce action takes the field's value from its
position in the right-hand side, as Pratt LED reduces already do. Repetition
attachment units, which wrap each occurrence of a repeated element, are
unchanged.

This is what makes the bison model apply literally. The rule's last terminal
is the operator the author wrote, an override on the variant lands on the rule
that reduces, and a restricted expression is a substitution over symbols with
no attachment clones. It also removes one nonterminal per operator from
pg-sql's automaton. The prototype's `UMINUS` failure is fixed by this change
and by nothing else.

## Consumers

- **Pretty.** Parenthesization is derived from the level and associativity of
  each variant's rule. A child variant with level `L'` under a parent with
  level `L` is parenthesized when it is the parent's first operand and
  `L' < L`, or `L' == L` and the parent is `right` or `nonassoc`; or when it is
  the parent's last operand and `L' < L`, or `L' == L` and the parent is
  `left` or `nonassoc`. A child with no level is never parenthesized. Interior
  operands are delimited by fixed tokens and are not parenthesized by
  precedence, with one exception below. The requirement that the grammar have
  a grouping atom (an atom that encloses `Self` between fixed tokens) is
  unchanged; see "The enclosure proof" below for what counts as fixed syntax
  and how one atom is selected.

  The rule above compares the levels of two rules. Bison compares the level of
  a rule with the level of the lookahead token, and the two are different when
  a variant has a `#[parse(prec = ...)]` override or when the token after its
  first operand is not its last levelled terminal (pg-sql's quantified
  comparisons: `#[parse(prec = Op)]`, and the token after the operand is `*`,
  `<` or `LIKE`). So Pretty also uses the token's level `T`, and the union of
  the two tests can only add parentheses:

  - A first operand is also parenthesized when its level is below `T(p)`, the
    level of the token after it in the parent (or equal, and `T(p)` is not
    `left`). `T(p)` is the level of a fixed token, or, for a child Node, the
    level of the token that the child's value starts with, which a generated
    function reads at run time.
  - A last operand whose own first symbol is an operand is also parenthesized
    when `T(c)`, the level of the token after that operand, is below the
    parent's level (or equal, and the parent is `left` or `nonassoc`): the
    parser then reduces the parent before it sees that token.
  - An interior operand that ends a rule of the same Node in some derivation is
    a last operand there: the elements after it can all be absent
    (`a LIKE b [ESCAPE c]`), or another variant is the same sequence up to and
    including the operand (`LIKE` beside `LIKE ... ESCAPE`). It takes the bound
    of a last operand of that rule.

  A parsed tree never meets the new terms, because the parser built that shape
  when the reduce or the shift won. So the rendering of a parsed tree does not
  change.

  At a restricted site (see "Restricted expressions"), Pretty also
  parenthesizes a value whose variant the restriction refuses, and it passes
  the restriction on to the operands the lowering retypes. A field that names a
  restriction renders through a generated entry of the base that carries the
  restriction. This works only when the restriction admits the grouping atom,
  because the atom's interior is the base again. A restriction that refuses the
  atom (a `leading` restriction that refuses `(`, or a restriction of constants)
  keeps the rendering it had, and `Arbitrary` never samples a value it refuses:
  it samples each restricted site through the restriction's own variants.

  Synthesized parentheses render through the enclosure frame of the selected
  grouping atom: the same element sequence, layout, groups and hints that the
  atom of a parsed value renders, so a repaired value and the parse of its
  rendering lay out the same way.

  A remaining limit: the shadow rule above covers a shorter rule of the same
  Node only. An interior operand of a rule whose prefix is a rule of another
  Node is not covered.
- **Diagrams.** A variant renders as the rule the author wrote. The
  `atom`/`prefix`/`infix`/`postfix` role labels and binding powers are
  removed. The precedence table is not rendered on rules.
- **Explorer.** The Pratt body plan and the `PrattLhs` element source are
  removed; an expression enum is an ordinary enum.
- **Arbitrary seeds, Visit, node views, configured traits.** Already treat a
  Pratt body as an enum; the Pratt branch is deleted.
- **Runtime.** The automaton's `tail` flag in `recursa-core` is currently
  cleared by "a Pratt rule". It is restated in terms of rule shape: a rule
  whose last symbol is its own left-hand side, or whose reduce is a
  left-recursive continuation, clears it. Existing fixtures that reject `1 2`
  and accept a parsed prefix are the proof.

## The enclosure proof

Pretty repairs precedence by parenthesizing an operand, so a Node with an
operator variant and a declared level must have a grouping atom: a variant with
no self-recursive operand in an operator or edge role that encloses the Node
between fixed syntax on both sides. The atom may reach the Node directly
(`Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self))`) or through helper
Nodes, as pg-sql's `ParenthesizedExpr` reaches `Expr` through
`ParenthesizedOpen`, `ParenContent` and `ParenthesizedClose`. A Node with an
operator variant and no proven atom is the build error
"Pretty cannot repair precedence for Node `N` because its grammar has no atom
that encloses Self between fixed tokens" (`RCA9105`).

**What counts as fixed syntax.** A helper Node on the path is fixed syntax when
every one of its variants expands to the *same rendered fixed syntax*: the same
number of fixed entries, each with the same spelling, the same mechanical
spacing class and the same placement. The comparison is over what the entries
render, never over how many arms the Node has and never over the token kinds the
arms name. A Node that names a `lookahead { ... }` merged kind beside the token
it merges is therefore still fixed syntax — pg-sql's
`ParenthesizedClose: RPAREN | RPAREN_SELECT_LA`, gram.y's
`any_with: WITH | WITH_LA` — because a merged-kind admission keeps the merged
token's own definition and so both arms carry that token's spelling and spacing.
Two arms that render different syntax (`)` beside `]`) are not fixed syntax, and
the Node contributes no enclosure.

**How one atom is selected.** Every atom variant that proves an enclosure is a
candidate. A candidate that reaches the Node directly outranks one that reaches
it through a helper, because a direct witness is the enum's own authored
grouping variant. Among the outranking candidates the enclosure must be
unanimous, compared as rendered fixed syntax again: several atoms that render
the same enclosure are one answer, which is what admits one atom per close kind.
Candidates that disagree leave the Node without grouping syntax and so report
the error above. Pretty never picks one of several disagreeing enclosures
silently.

## Diagnostics

- Retire `RCA0402`, `RCA3115`, `RCA3130`, `RCA3209`, `RCA3212` and the runtime
  classification `RCA4102` through `retired_diagnostic_spec!`. Numbers are
  never reused.
- `RCA0400` stays and becomes the only precedence-related build error. An
  unresolved conflict in an authored grammar surfaces as `RCA0400` with an
  authoring remedy, never as `RCA9105`.
- New diagnostics, with new codes: a Pratt attribute is a migration error that
  names this document; a precedence-only name without an override; a
  `#[restricts]` that names an unknown variant, a non-enum base, or a base
  that is itself the restricting Node; a `#[parse(prec = NAME)]` naming an
  undeclared level.

## Removed surface

- `#[pratt]`, `#[parse(prefix, bp)]`, `#[parse(infix, lbp, rbp)]`,
  `#[parse(postfix, bp)]`, `#[parse(pratt(exclude(...)))]`.
- `TypeShape::Pratt`, `PrattPosition`, `PrattVariant`, `PrattRestriction`,
  `PrattExtensionFilter`, `MaterializedNodeBody::Pratt`, the exclusion and
  minimum-binding-power clone lattice, `operand_attachments`,
  `attachment_clones`, the Pratt-first path of the conflict resolver, and the
  `pratt=typed-binding-powers` projection stamp.
- Fixtures `generation/pratt`, `generation/pratt-shapes`,
  `generation/table-driven-pratt`, test files `generated_pratt.rs`,
  `generated_pratt_shapes.rs`, `generated_pratt_table_driven.rs`, their
  lock-ledger rows, the Pratt unit tests in `lr/precedence.rs`,
  `lr/fixture_tests.rs`, `lr.rs`, and the `joined_pratt_*` test support.
- Acceptance gates `M4-PRATT-DIAGRAM` and `M4-LEGACY-RUNTIME-ARITH` are retired
  through the legacy-case ledger with the new expressions example's tests as
  successors.

## Retained and added surface

- `examples/pratt-expressions` becomes `examples/expressions`
  (`recursa-example-expressions`), written as above, with the same behaviour
  tests and the same README claims. Every README reference follows.
- A generation fixture `expressions` proves, through the generated parser: a
  prefix override through a precedence-only name; left, right and nonassoc
  infix levels; a postfix operator; an interior operand through a restricted
  expression (`a BETWEEN b AND c` against boolean `AND`); an `ESCAPE`-style
  keyword one level above its operator; and that `RCA0400` is reported for an
  undeclared operator with an authoring remedy.
- The `precedence` fixture keeps its six `gram.y` conflict families, rewritten
  without `#[pratt]`.
- `docs/table-driven-parsing.md`, `docs/api-design.md`,
  `docs/flat-ast-design.md`, the crate READMEs, and `CONTEXT.md` describe the
  one model. The glossary terms are **Declared precedence**, **Precedence-only
  name** and **Restricted expression**; **Binding-power precedence** is gone.

## pg-sql migration

pg-sql is the only consumer and is in scope. Its `Expr` is a `#[pratt]` enum
whose binding powers are `gram.y`'s levels times ten, layered over a
`precedence` block in `src/tokens.rs` that today is "consulted only where
Pratt says nothing", with three `#[parse(prec = ...)]` overrides and seven
`pratt(exclude(...))` sites.

- `Expr` drops `#[pratt]` and every role. Each operator variant keeps its
  name and fields; the operator token moves to the field it precedes or
  follows. The AST type is unchanged.
- The `precedence` block becomes complete: every level of `gram.y:829-908`
  in `gram.y` order, including the arithmetic operators, `AT`, `COLLATE`,
  `UMINUS`, `TYPECAST`, `.`, `[` and `(` that binding powers carried before.
  Each level cites its `gram.y` line. The documented `AT` divergence is
  removed; `AT` takes `gram.y`'s position.
- Every `%prec` in `gram.y`'s `a_expr`, `b_expr` and `c_expr` becomes a
  `#[parse(prec = ...)]` on the mirroring variant. The `NOT_LA` family, the
  `IS` family, the `BETWEEN` family, `AT`, `UMINUS` and `Op` are the cases.
- Each of the seven exclusion sites is mapped to the `gram.y` nonterminal it
  stands for, and that nonterminal is written as a restricted expression.
  `b_expr` is the first. The six `QuantifiedComparison*` variants, which
  exist only to give one token several binding powers, collapse to the
  `gram.y` shape `a_expr subquery_Op sub_type ... %prec Op`.
- The comments in `src/tokens.rs` and `src/ast/shared/expr.rs` that describe
  the layered model are rewritten.
- Gates: zero LALR conflicts; the PostgreSQL differential at 234/234 over the
  same corpus; the frozen differential baseline unchanged; forced generation
  at most ten seconds; and automaton statistics recorded before and after in
  `docs/research/postgres-parser-performance.md` §9, replacing the "Pratt
  restrictions" ledger rows.

## Landing order

Each step is gated before the next starts.

1. **Recursa, Pratt still lowers.** Inline cardinality-one attachment units;
   apply overrides to the inlined rule; add `#[restricts(Base)]`; report
   unresolved conflicts as `RCA0400`; derive Pretty parenthesization from
   rule level and associativity for every variant; add the `expressions`
   fixture; rewrite `examples/pratt-expressions` as `examples/expressions`.
   Gate: the full Recursa suite, and pg-sql builds unchanged against this
   revision with zero conflicts and 234/234.
2. **pg-sql.** The migration above, on a pg-sql branch pinned to the step-1
   revision. Gate: the pg-sql gates above.
3. **Recursa, removal.** Delete the removed surface, retire the diagnostics
   and gates, add the migration diagnostic, restate the runtime `tail`
   predicate, update the docs. Gate: the full Recursa suite, and pg-sql at
   the step-2 revision builds with zero conflicts and 234/234 against it.

## Working arrangement

pg-sql depends on `../recursa/*` by path and pins a revision in
`.recursa-revision`. Recursa work for this design happens on branch
`precedence/declared-only` in the worktree
`~/projects/recursa-wt-precedence`, never in the shared checkout. pg-sql work
happens in a pg-sql worktree whose `Cargo.toml` repoints all three
`../recursa/` paths to that worktree with `Cargo.lock` deleted, and whose
`vendor/postgres` submodule is initialised with
`git submodule update --init --reference ~/projects/pg-sql/vendor/postgres`
so the oracle suites can run.
