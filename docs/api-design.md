# Recursa Developer-Facing API — Design

## Status

Historical pre-M4 design rationale. This document is retained to explain the
questions that shaped the port; it is not the current public API contract.
Later accepted M3/M4 decisions, consolidated in issues #36, #38, and #49,
supersede its raw runtime examples. In particular:

- `TokenRecord`, numeric token kinds, erased `LexResult` construction, raw
  `Input`, and caller-supplied FOLLOW are private generated/runtime ABI;
- lexing, metadata, and input types are generated and grammar-bound;
- `recursa::Parse` is sealed and has no public recursive FOLLOW operation; and
- source provenance lives beside semantic values in `Parsed` occurrence views,
  not in AST `SpanBlock` fields.

The checked facade and generated grammar fixtures are authoritative for the
currently implemented surface. The remaining sections describe the earlier
design and should be read in that historical context.

The execution of this design is planned in [`port-plan.md`](./port-plan.md).
The bar is **production readiness**, validated by a full pg-sql port with
mandatory gates: the differential regression suite against PostgreSQL's
`raw_parser`, the benchmark suite including the `sqlparser` comparison,
comment-preserving fmt goldens, and a clean sustained fuzz run.

Inputs:

- The spike's `GrammarDescription` → analysis → codegen pipeline
  (`recursa-spike/src/analysis.rs`, `recursa-spike/src/codegen.rs`) — the back half that this API feeds.
- A survey of the **current** recursa API (`../recursa-old`): the `tokens!` DSL,
  `#[recursa::parser]` marker + external scanner, the categories/targets
  work-in-progress, and the recorded pain points in `../recursa-old/CLAUDE.md`
  and `../recursa-old/docs/plans/*`.

It answers thirteen questions:

- **A1** — How is an AST module annotated?
- **A2** — How is the lexer defined?
- **A3** — How are tokens categorised (including keywords-as-identifiers)?
- **A4** — How is codegen invoked?
- **A5** — Is the lexer (logos) publicly visible?
- **A6** — How are spans captured, stored, and requested at parse time?
- **A7** — Can the same grammar definition emit a flat (arena/index) AST?
- **A8** — Which grammar ambiguities are detected at build time, and how
  are they reported?
- **A9** — What happens when input is broken or hostile: recovery, error
  reporting, hardening?
- **A10** — What are the entry points, and how are comments/trivia
  preserved?
- **A11** — What testing power does the framework generate from the
  grammar itself?
- **A12** — How good can generated pretty-printing (`Pretty`) be with
  zero manual annotation?
- **A13** — What does the interactive AST explorer look like?

## Constraints

- **C1 — One whole-tree semantic authority.** FIRST/FOLLOW/EXTENDERS are
  cross-type analyses, so only the build analyzer interprets the complete AST,
  token syntax, helper attributes, root configuration, diagnostics, and trait
  plans. A source-local `Node` derive is allowed only as an explicit membership
  anchor and generated-behavior loader; it never analyzes the item independently.
- **C2 — One parser for the DSL.** The build analyzer is the only component
  that reads grammar syntax. `grammar!`, `tokens!`, and `Node` may preserve
  syntax or load generated artifacts, but they do not reinterpret it. This
  replaces the previous keep-in-sync-by-hand scanner/macro split.
- **C3 — No hand-written `Parse` impls, structurally.** pg-sql still carries
  five (`UnquotedIdent`, `CustomOp`, `BareAliasName`, …) because the current
  framework cannot express "accept this kind plus a set of other kinds."
  The new token model (A3) must make these expressible declaratively so the
  "manual impl = framework gap" rule has no standing exceptions.
- **C4 — Fixed tokens are not types.** A token that contributes no
  information to the AST — every keyword, every piece of punctuation — does
  not get a Rust type, is not stored in a field, and never instantiates
  `Parse`. It is spelled as an inert attribute and consumed as syntax. This
  supersedes both the spike's `Tok<Marker>` / `TextTok<Marker>` pattern
  (rejected: marker + blanket-impl + alias indirection) and current
  recursa's one-struct-per-keyword model (rejected: ~500 types whose impls
  and combinator instantiations dominate the generated-code and
  monomorphization budget while carrying no data). Content-bearing tokens —
  literals, identifiers, targets — remain real named structs with directly
  emitted impls.
- **C5 — No checked-in generated megafile.** pg-sql's 5.5 MB
  `src/generated/first_set.rs`, its `include!` into a hand-maintained
  glob-import prelude module, and the `cargo xtask codegen --check` CI drift
  gate are all accidental complexity. Generated code should never be
  committed, never drift, and never need a prelude.
- **C6 — Vestigial surface dies.** `rules = SqlRules` (names a type that no
  longer exists), the `soft_keywords` block, the stranded `accepts` clause,
  and the stale README API all disappear rather than being ported.

## The API at a glance

Everything below is elaborated in A1–A5; this is the shape of a complete
grammar under the proposed API.

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    recursa_codegen::generate("src/sql/mod.rs")?;
    Ok(())
}
```

```rust
// src/sql/mod.rs (and any nested `mod`s — the tree can span many files)

recursa::grammar! {
    derives(Debug, Clone, Pretty, Visit, VisitMut, Arbitrary),
    diagrams,
    max_lookahead = 2,
}

recursa::tokens! {
    ignore = r"\s+|--[^\n]*",

    // Nothing is forward-declared by requirement (see A1). The central
    // block exists for what benefits from centrality — bulk keyword
    // tables, flags and classes, and named closed matchers — not
    // because declaration must precede use.
    flags { bare_label }

    keywords {
        FORMAT => r"FORMAT" in UNRESERVED + bare_label,
        JSON   => r"JSON"   in COL_NAME   + bare_label,
    }

    literals {
        StringLit => r"'[^']*(?:''[^']*)*'",
        Number    => r"[0-9]+(\.[0-9]+)?",
        Ident     => r"[A-Za-z_][A-Za-z0-9_]*",
    }

    classes { bare_label_keywords = keywords where bare_label }

    targets {
        ColLabel     : Ident admits UNRESERVED, COL_NAME, TYPE_FUNC_NAME, RESERVED,
        BareColLabel : Ident admits bare_label_keywords,
    }
}

#[derive(recursa::Node)]
pub struct SelectStmt<'i> {
    #[kwd("SELECT" in "RESERVED")] #[sep(",")]
    pub items: Vec1<SelectItem<'i>>,
    #[kwd("FROM" in "RESERVED")]
    pub table: ColId<'i>,          // ColId defined at its use in Expr below
}

#[derive(recursa::Node)]
pub struct SelectItem<'i> {
    pub expr: Expr<'i>,
    pub alias: Option<Alias<'i>>,
}

#[derive(recursa::Node)]
pub enum Alias<'i> {
    #[kwd("AS" in "RESERVED")]
    Explicit(ColLabel<'i>),
    Bare(BareColLabel<'i>),   // reclaim-set differs by position — see A3
}

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'i> {
    Number(Number<'i>),
    #[tok(r"[A-Za-z_][A-Za-z0-9_]*" admits "UNRESERVED", "COL_NAME")]
    Column(ColId<'i>),
    #[surrounded("(", this, ")")]
    Paren(Box<Expr<'i>>),
    #[parse(prefix, bp = 15)] #[kwd("NOT" in "RESERVED")]
    Not(Box<Expr<'i>>),
    #[parse(infix, lbp = 10, rbp = 11)] #[tok("+")]
    Add(Box<Expr<'i>>, Box<Expr<'i>>),
    #[parse(infix, lbp = 6, rbp = 7)]
    #[kwd("IS" in "RESERVED", "DISTINCT" in "RESERVED", FROM)]
    IsDistinctFrom(Box<Expr<'i>>, Box<Expr<'i>>),
}
```

Consumption:

```rust
let lexed = sql::lex(src)?;                       // generated, logos inside
let mut input = recursa::Input::new(src, &lexed);
let stmt = sql::SelectStmt::parse(&mut input)?;   // impl loaded at its declaration
```

Note what is **absent**: no per-keyword types (`SELECT` and `","` are
defined and consumed at their use sites, not declared as structs — most of
the old `keywords`/`punctuation` blocks simply disappear), no keyword fields
in the AST (`SelectStmt` is two fields, both semantic), no
`Surrounded<LParen, …, RParen>` spelling, no `#[recursa::parser(rules = …)]`
on every item, no derive stack repeated 1267 times, no `src/generated/`
directory, no xtask or drift gate, no `__firstset` prelude module, no
`*Marker` types, and no hand-written `Parse` impls for identifier-like
tokens.

## A1 — AST module annotation

**One selected root and explicit Node membership.** Each grammar crate invokes
`recursa_codegen::generate(root)` for one manifest-relative root. Exactly one
`recursa::grammar!` invocation appears before Node-bearing declarations in that
root. It establishes the generated loading scope; it does not interpret its
configuration.

Every grammar type carries `#[derive(recursa::Node)]`. The derive legalizes
helper attributes and loads the analyzer's keyed generated behavior at the
declaration site, which keeps private nested types usable. It does not compute
grammar meaning or the effective trait stack. Non-Node helper types receive no
generated behavior.

### Fixed tokens are attributes, not fields (C4)

The organising principle: **a type or field exists in the AST iff it carries
information**. Keywords and punctuation shape the syntax but carry no data —
so they are spelled as attributes on the item or field they precede, and the
generated parse body consumes them without storing anything:

| Attribute | On | Meaning |
|---|---|---|
| `#[tok(…)]` / `#[kwd(…)]` | field (named or tuple) | Consume these tokens immediately **before** parsing the field. `#[kwd]` is the keyword-introducing form (may carry `in "CATEGORY"`); `#[tok]` covers punctuation and references. Sequences allowed: `#[kwd(IS, DISTINCT, FROM)]`. |
| `#[tok(…)]` / `#[kwd(…)]` | unit enum variant | The variant *is* this token sequence: `enum Direction { #[kwd(ASC)] Asc, #[kwd(DESC)] Desc }`. |
| `#[tok(…)]` / `#[kwd(…)]` | struct / non-unit variant | Tokens before the first field (leading keywords). |
| `#[tok(r"…" admits …)]` | field / single-field variant | **Defines the field's own token type** at the use site: the pattern plus admitted categories become the type named by the field (`ColId` in the glance example). See A3 for the kind-set semantics. |
| `#[trailing(…)]` | struct / variant | Tokens after the last field (`CASE … END` shapes). |
| `#[sep(…)]` | `Vec<T>` / `Vec1<T>` field | Separator token between repetitions. |
| `#[surrounded(open, this, close)]` | field | Delimiters around the field — replaces `Surrounded` entirely. The `this` placeholder marks where the field's content sits, so open/close roles are explicit and either side may be a token sequence (`#[surrounded(LBRACKET, this, RBRACKET)]`, `#[surrounded("(", this, ")")]`). |

Grammar order stays fully explicit — attributes annotate the position they
occupy — but the struct stores only semantics: `SelectStmt { items, table }`
constructs and pattern-matches without `select: SELECT`-style dead weight,
and `Visit`/`VisitMut` traverse only meaningful nodes.

### Nothing is forward-declared

Tokens, categories, and target types do **not** have to be declared before
(or centrally at all) — the build analyzer sees the whole module tree, so
"declaration" is wherever the definition happens to sit, including inline at
a use site. One syntactic rule separates the two roles: **string literals
define, bare identifiers refer.**

```rust
#[tok("{")]                        // punctuation: defined by use, never declared
#[kwd("SELECT" in "RESERVED")]     // defines the keyword AND names its category;
                                   // the category springs into existence when
                                   // first named — no categories{} block needed
#[kwd(SELECT)]                     // reference to an existing definition
#[kwd(SELECT in "RESERVED")]       // ERROR: bare ident is a reference, and a
                                   // category is fixed at definition time — a
                                   // reference cannot carry definition clauses
```

Rules that make this coherent across a large, many-file grammar:

- **Repeated identical definitions are fine.** `#[kwd("SELECT" in
  "RESERVED")]` may appear at ten sites; the macro unifies them into one
  kind. Conflicting definitions (same keyword, different category; same
  target name, different pattern or admit-set) are a compile error naming
  both sites.
- **Definitions and references mix freely in one sequence**:
  `#[kwd("IS" in "RESERVED", "DISTINCT" in "RESERVED", FROM)]` defines two
  keywords and references a third.
- **Inline-defined types are ordinary types afterwards.** `ColId` defined by
  `#[tok(r"…" admits …)]` at one field is referenced elsewhere by just using
  the type name — definition site and use sites are symmetric, order-free.
- **The central `tokens!` block is optional**, kept for what genuinely
  benefits from centrality: `ignore` rules, named closed matchers,
  `lexer_tokens`, flags/classes, and bulk keyword tables (pg-sql's ~450
  keywords read better as a table than scattered across 1300 types). Central
  and inline definitions unify into one token model; the same
  conflict-detection applies across both.

For Pratt variants, the operator's position is implied by the parse
position: `#[parse(prefix)]` + `#[kwd(NOT)]` consumes the token before the
operand; `infix` after the implicit first `Box<Self>` lhs and before its one
recursive `Box<Self>` rhs; `postfix` after. An infix suffix may retain
additional non-recursive semantic fields, such as a content-bearing operator,
but exactly one suffix field must be the recursive rhs. Interleaved shapes
(`a BETWEEN b AND c`) put `#[tok(…)]` on the tuple field each token sequence
precedes:

```rust
#[parse(postfix, bp = 6)]
Between(Box<Expr<'i>>, #[kwd(BETWEEN)] Box<Expr<'i>>, #[kwd(AND)] Box<Expr<'i>>),
```

Every recursive `Box<Self>` field in a postfix tail inherits the postfix
binding power as its minimum. It therefore consumes tighter operators while
stopping before lower-binding delimiters such as the `AND` above.

When one grammar position admits only a named subset of a Pratt language,
`#[parse(pratt(exclude(In, And)))]` on that field disables those LED variants
for the whole recursive operand parse. A disabled extender yields its token to
the field's ordinary FOLLOW boundary; other uses of the Pratt Node are
unchanged.

Optionality of pure syntax is expressed by enum variants (presence = which
variant parsed), as in `Alias::Explicit` vs `Alias::Bare` above — no
`Option<AS>`-style keyword storage exists or is needed.

**Combinator consequences.** With separators and delimiters moved to
attributes, the user-facing combinator set shrinks to ordinary containers —
no grammar-flavoured sequence types appear in field position:

- `Surrounded<O, I, C>` — deleted; `#[surrounded(open, this, close)]` on
  the field.
- `Seq0<T, S>` — deleted; `Vec<T>` (+ optional `#[sep(…)]`) is zero-or-more.
- `Seq1<T, S>` — deleted; `Vec1<T>` (+ optional `#[sep(…)]`) is one-or-more.
  `Vec1` is a plain nonempty-vector data type (deref to `[T]`, `first()`
  total), with nothing parsing-specific about it.
- `Option<T>`, `Box<T>` — unchanged.

The AST consumer sees `Vec`/`Vec1` and iterates; nothing about separators or
delimiters survives into the data. Internally the emitter is free to route
separated-sequence parsing through `Seq`-style machinery rather than
inlining a separator loop into every parent's generated body — that is an
emission detail (see the budget note in A4), invisible in the API.

### Item-level grammar attributes

| Attribute | On | Meaning |
|---|---|---|
| `#[pratt]` | enum | Operator-precedence enum; drives the generated `parse_bp` driver. |
| `#[parse(prefix, bp = N)]` | Pratt variant | Prefix operator; operand field `Box<Self>`. |
| `#[parse(infix, lbp = N, rbp = M)]` | Pratt variant | Infix; `rbp = lbp + 1` for left-assoc. The first field is the implicit `Box<Self>` lhs; the suffix admits ordinary semantic fields but requires exactly one recursive `Box<Self>` rhs. |
| `#[parse(postfix, bp = N)]` | Pratt variant | Postfix; recursive tail operands inherit `N` as their minimum binding power. |
| `#[parse(pratt(exclude(A, B)))]` | field targeting a Pratt Node | Excludes the named LED variants from this field and its recursive operands; each name must identify an infix or postfix variant. |
| `#[parse(postcondition = path)]` | type | Unsupported legacy syntax; express token eligibility with a named admission set. |
| `#[parse(skip)]` | type | Item is not a grammar type (helper struct, hand-owned parsing). Diagrams optional via `#[parse(skip, diagram)]`. |
| `#[parse(lookahead = k)]` | enum | Raises dispatch lookahead for this type (grammar-wide max in the root configuration). |

Unmarked Pratt variants are atoms (matching the spike's `PrattPosition`
default and current recursa's behaviour).

### Module-level configuration

**The effective derive stack is computed once by the build analyzer from the
root configuration and any local exceptions:**

```rust
recursa::grammar! {
    derives(Debug, Clone, Pretty, Visit, VisitMut, Arbitrary),
    diagrams,
    max_lookahead = 2,
}
```

The loader treats this token stream as opaque. Exact local-exception spelling
remains an M3 decision; neither `grammar!` nor `Node` may interpret it
independently.

**Traversal: `Visit` + `VisitMut`; `Transform` is removed.** Because the
AST is typed, rewrites are always `T → T`, so an in-place `VisitMut`
(`&mut` nodes, syn-style: default methods call `walk_*`, code before the
walk call is pre-order, after is post-order) subsumes the fold-style
`Transform` derive completely:

- Destructive rewrites mutate in place — no tree rebuild, and **untouched
  subtrees keep their parsed spans and trivia attachments verbatim**
  (exactly what A10 round-tripping wants; a fold churns every node).
- Non-destructive rewrites are `clone()` + `VisitMut` — the same total
  cost as a fold, which also consumed its input.
- Contract: traversal is **structural depth-first**, each reachable child
  visited exactly once. In flat mode (A7) this matters: mutation may
  overwrite a pool slot in place or append-and-repoint — neither
  invalidates `Ref<T>` keys (only explicit compaction ever does) — but
  the `VisitMut` walker must be the structural walk, never a linear pool
  sweep, or it would visit nodes the visitor just appended or orphaned.
  Read-only `Visit` keeps the linear pool sweep as its flat-mode fast
  path; the asymmetry is deliberate.
- Ships with `recursa::replace_with(cx, handle, |owned| …)` for the one
  genuinely awkward idiom — replacing a node with one of its own
  descendants (unwrap-parens) — since there is no cheap placeholder value
  to `mem::replace` with and `Default` is not derived.

**One impl works for both nested and flat ASTs** (source-portable under
the A7 mode flip; not generic-over-both-in-one-binary — nobody needs
that, and it would tax every visitor with a mode parameter). The seam
differs per trait:

- `Visit`: the **walker resolves keys** — in flat mode it does `&ast[key]`
  before every callback, so methods uniformly receive
  `(&mut self, cx: &Cx, node: &T)` where `Cx` is `()` (nested,
  zero-sized) or `Ast` (flat). Matching and reading are identical in both
  modes; only *following a child yourself* goes through the handle
  accessor (`child.node(cx)` → `&Expr` either way — `Box` ignores the cx,
  `Ref` indexes it).
- `VisitMut`: the walker **cannot** pre-resolve (`&mut` node +
  `&mut Ast` is the aliasing conflict), so methods receive
  `(&mut self, cx: &mut CxMut, e: Handle<T>)` — `&mut T` nested,
  `Ref<T>` flat — and resolve at the use site. Borrowck caveat: nested
  resolution doesn't actually borrow `cx`, so the portable discipline is
  "treat every resolved node as borrowing `cx`" — code written to flat's
  stricter rules compiles in both modes.
- Portability is available, not enforced: a nested-only grammar may
  destructure `Box` fields natively; that visitor is simply nested-only.

`Arbitrary` is an ordinary entry in `derives(…)`, not a special-cased
config key (current recursa's `arbitrary_feature = "arbitrary"` metadata is
dropped — C6). Listing `Arbitrary` when recursa's `arbitrary` feature is
not enabled is a **compile error** at the grammar root ("`Arbitrary`
requires the `recursa/arbitrary` feature"), not a silently skipped derive.
The emitted derive is unconditional; a grammar crate that wants
fuzzing-only support forwards the feature
(`arbitrary = ["recursa/arbitrary"]`) rather than conditionally editing
the attribute. `serde::Serialize`/`Deserialize` follow the identical rule
behind `recursa/serde` — one more `derives(…)` entry, and particularly
useful with flat ASTs (A7), where a serialized pool set is a compact,
position-independent AST interchange format.

This finishes what `#[recursa::ast]` started (it exists, is configured in
`Cargo.toml` metadata, and is used 3 times against 1267 long-form stacks —
adoption failed because it was opt-in per item). Here it is the default for
every item in the module; an item that needs a different stack opts *out*
locally (`#[parse(no_derive(Clone))]`-style escape hatch, exact spelling TBD
during implementation). Config lives in the attribute args, not
`Cargo.toml` metadata — it is grammar-scoped, not crate-scoped, and should be
readable at the annotation site.

**Structural rules** (largely as current recursa, now diagnosed with proper
errors by the macro instead of by convention):

- Non-Pratt enum variants are unit variants with `#[tok(…)]`, or
  single-field tuples wrapping a `Parse` type (optionally with leading
  `#[tok(…)]`).
- Struct fields are `Parse` types or ordinary containers over them:
  `Option<T>`, `Box<T>`, `Vec<T>`, `Vec1<T>`.
- Every `Parse` type referenced by a field/variant must be **defined inside
  the selected grammar module tree** (or be a recursa combinator / generated
  token type). This is C1 made structural: whole-AST visibility is guaranteed
  because the AST is, by rule, entirely under the selected root. A `use`
  of an outside type in parse position is a compile error with a "grammar
  types must live under the selected grammar root" message.

Span support is designed in A6; current recursa's per-field `Spanned<T>`
wrapper and the `span` attribute (with its ordering footgun) are both
retired in its favour.

### No rules type

The rules type (`SqlRules`) is **eliminated, not generated-and-hidden**.
Its historical job — threading grammar globals like whitespace rules
through `Parse` impls — ended with the logos migration (`ignore` lives in
the lex pass; current recursa's `Parse<'input>` already takes no rules
argument, and `SqlRules` is literally undefined in today's workspace).
The spike's remaining use of `R: Grammar` — associating
`TokenKind`/`KindBitset` with *generic* runtime library code — dissolves
under whole-grammar codegen:

- Combinators are inlined at emission (A4), so no generic runtime `Parse`
  impls exist that need the association.
- Everything bitset-shaped lives in **generated code**, where the width is
  statically known: FIRST/dispatch tests emit as concrete constants.
  `KindBitset` and width selection (64/128/256 by counted kinds) remain
  codegen-internal concepts, absent from the public API.
- At the runtime boundary, kinds are **erased**: `TokenRecord { kind: u16 }`
  and an R-free `Input` (as current recursa already has). The runtime
  `FollowSet` (D2 threads it by reference) is width-erased
  (`Cow<[u64]>`-backed), so `recursa-core` needs no width parameter.
- Grammar globals that genuinely reach runtime — kind-name/atom tables for
  error rendering — travel as a **value** (`&'static GrammarInfo` handed
  to `Input` by the generated `lex()`/entry points), not as a type
  parameter.

Users never write, name, or see a rules type or a `Grammar` trait; neither
does generated code. (`rules = SqlRules` was already dead surface — C6;
this closes the question of whether anything replaces it: nothing does.)

## A2 — Lexer definition

**Keep the `tokens!` DSL, interpret it only in the build analyzer, and make
the block optional.** The DSL is proven at pg-sql scale (~960 lines
defining a full PostgreSQL surface) and survives almost verbatim: `ignore`,
`categories`, `flags`, `keywords`, `punctuation`, `literals`,
`lexer_tokens`, `classes`, `targets`. Dropped: `soft_keywords` (subsumed —
A3) and the never-honored `accepts` clause (C6). New: every declarative
block is optional, because tokens, categories, and targets can equally be
defined inline at use sites (A1, "Nothing is forward-declared") — the analyzer
unifies central and inline definitions into one token model and synthesizes
the lexer from the union.

The key change is architectural, not syntactic. `recursa::tokens! { … }`
inside the selected grammar root is a structural placeholder for the generated
token machinery. The build analyzer alone parses it and feeds the normalized
model into analysis and emission. The macro does not independently expand or
validate the DSL, so expansion and analysis cannot drift (C2).

What the M4 emitter generates per entry — note that under C4 the `keywords` and
`punctuation` blocks no longer generate any types:

- keywords / punctuation (defined centrally or inline): a `TokenKind`
  variant and a lexing rule, **nothing else**. The name (`SELECT`) / display
  string (`","`) is how `#[tok]` / `#[kwd]` / `#[sep]` / `#[surrounded]`
  attributes and error messages refer to it. `Pretty` output for a
  containing node prints the display string at the token's grammar position.
  Word-shaped keywords are subject to the automatic keyword/identifier
  merge below — they may have no DFA rule of their own at all.
- `literals`: a **real named struct** (`pub struct Number<'i> { … }`) with a
  directly emitted `Parse` impl and a text accessor resolving through the
  span (tokens stay payload-free; text is a slice of the source), plus the
  token's railroad diagram derived from its lexer pattern.
- `targets`: a real named struct per target (see A3).
- `matchers` / matcher-form `ignore`: generated closed scanners for delimited,
  nested, boundary-sensitive, operator-run, and physical-line input.
- The flat `#[derive(Logos)] enum TokenKind`, `pub fn lex(&str) ->
  LexResult`, and the static `GrammarInfo` tables (kind names, display
  atoms) that error rendering consumes — all inside the module. No rules
  type or `Grammar` impl exists (A1, "No rules type").

The lexical vocabulary is closed. Dollar strings, nested block comments,
word-boundary exclusion, PostgreSQL operator runs, and physical psql lines
are declarative matcher families. Authored callbacks and post-lex record
mutation are rejected during source analysis.

### Automatic keyword/identifier lexing merge

**Requirement (carried over from current recursa, applied automatically):
keywords must not multiply lexing cost.** Pre-logos recursa paid a
case-insensitive regex scan *per keyword attempt* — the dominant parse
cost documented in `../recursa-old/docs/notes/perf.md` — and the fix was to
recognise all word-shaped input by one path and resolve keyword-vs-
identifier afterwards. The new emitter guarantees the same property, with
no user opt-in:

- For every keyword, the macro checks (mechanically, via `regex-syntax`)
  whether its text is matched in full by a declared identifier-shaped
  regex token (e.g. `Ident`), in all case variants.
- **Subsumed keywords get no lexer rule of their own.** The word is lexed
  by the identifier token's rule; the generated `lex()` loop then resolves
  the matched text against a generated keyword table
  (case-insensitive; perfect-hash or length-bucketed compare — an
  emission detail to benchmark) and rewrites the record's kind to the
  keyword's `TokenKind`. One recognition pass, one table probe per word,
  and the DFA stays small regardless of keyword count (pg-sql: ~450
  keywords that would otherwise all be case-insensitive DFA paths).
- Keywords **not** subsumed by any identifier-shaped token (unusual
  spellings, or grammars with no identifier token at all) fall back to
  their own DFA rules — "where possible" is decided per keyword,
  automatically.

The parse layer is unaffected: every keyword still has its own
`TokenKind`, so kind-sets, targets, and dispatch (A3) see no difference.
This is purely a lexer-emission strategy, and the benchmark gate in
[`port-plan.md`](./port-plan.md) is what holds it to the "no regression"
bar.

**Declaration order stops being load-bearing.** Today logos priorities come
from declaration order (longest match wins, order breaks ties), and pg-sql
carries comment-enforced ordering blocks for prefix-sharing punctuation.
With definitions scattered across use sites there *is* no meaningful
declaration order, so the macro derives priority deterministically instead:
exact-literal tokens outrank pattern tokens, longer literals outrank
shorter, and a genuine pattern-vs-pattern conflict is a build error asking
for an explicit `priority = n` on one of them (central block or inline)
rather than a silent order-dependent resolution. The old prefix-footgun
disappears rather than being warned about.

## A3 — Token categorisation and keywords-as-identifiers

This is where the spike's model and recursa's in-flight categories/targets
design meet — and they meet exactly at **D1's atom lattice**, which the
spike designed in but never used (`Atom::Category/Class/Target` variants,
deferred).

### The model: kind-set token types

Current recursa is mid-migration between two keyword mechanisms (the
`soft_keywords` predicate — which still forces hand-written `Parse` impls —
and the newer categories/targets lattice, which generates two-variant
`Base`/`Keyword` enums plus per-kind fork-and-retry dispatchers). The new
design completes the migration and simplifies the runtime shape:

**A content-bearing token type accepts a *set of kinds*, not one kind.** The
spike's `SingleTokenKind::KIND: TokenKind` (one kind, one bit) generalises
to `KINDS: KindBitset` (a bitset, usually one bit). Everything else in the
validated model already operates on bitsets, so this generalisation is
nearly free:

- `peek` is the same bitset test it always was.
- FIRST(target type) = its kind-set. FIRST composition, FOLLOW composition,
  conflict detection — all unchanged, since they were bitset ∪/∩ already.
- The display sidecar uses the deferred `Atom::Category` / `Atom::Class` /
  `Atom::Target` variants, so an error message says `expected ColId`, not a
  200-kind enumeration. D1 finally earns its keep.

A `targets` entry generates a **single real struct** — not a
`Base`/`Keyword` two-variant enum, no companion `ColIdKeyword` type:

```rust
/// Generated from `ColId : Ident admits UNRESERVED, COL_NAME`.
pub struct ColId<'i> {
    pub token: TokenRecord,
    // text accessor via span; kind preserved on the token if the consumer
    // cares whether this identifier arrived as a keyword
}
```

whose accepted kind-set is `{Ident} ∪ kinds(UNRESERVED) ∪ kinds(COL_NAME)`,
baked at codegen into one bitset constant. Parsing a `ColId` is a bitset
membership test and a token consume — no forking, no retry, no text
comparison at runtime. (Targets are types, unlike keywords, because they
carry content: *which* identifier. C4's principle cuts exactly here.)

### Why this is the elegant answer to keywords-as-identifiers

The reservation question — "can `PARTITION` be a column name here?" — is
answered **positionally by the type system**: the AST field's type names a
target, the target names admitted categories, the categories partition the
keyword space. `SELECT a FROM t` cannot steal `FROM` as a bare alias because
the bare-alias field is typed `BareColLabel`, `BareColLabel` admits only
`bare_label`-flagged keywords, and `FROM` isn't flagged. There is no
predicate function, no soft-keyword special case, no context flag threaded
through the parser — set membership, decided at build time, tested as one
`AND` at runtime.

This also discharges C3. The five hand-written impls in pg-sql exist because
"accept this kind plus a set of other kinds" was inexpressible;
`UnquotedIdent`-reclaiming-soft-keywords is precisely a kind-set. `CustomOp`
and `BareAliasName` fall out the same way. The `soft_keywords` block and the
`token_kind_is_soft` runtime predicate are deleted, not ported.

### Conflict analysis over kind-sets

The spike's within-type and use-site overlap checks generalise from
"kinds equal" to "kind-sets intersect" (the spike's `text_rules_overlap`
already models exactly this shape, for text rules). So the analysis will
correctly reject, e.g., a non-Pratt enum with variants `Bare(ColId)` and
`#[kwd(FORMAT)] Fmt(…)` — `FORMAT`'s kind is inside `ColId`'s set — and the
error can say *which keyword* and *which category* collide. Where a grammar
genuinely needs that overlap resolved by order (PostgreSQL has such
corners), the LL(k) machinery from the spike (`max_lookahead`, FIRSTᵏ
dispatch) and declaration-order tiebreak with an explicit
`#[parse(prefer)]`-style opt-in are the pressure valves; the default is a
hard error, per D8/D9 discipline.

### What happens to the spike's `TextRule`

Subsumed as a public concept. `TextRule` existed for lexers that make no
keyword/identifier distinction (`cskw`'s single `Word` kind); under this
design the `tokens!` DSL always *can* declare keywords as kinds, so
positional reservation is always expressible as kind-sets. The runtime keeps
the text-predicate machinery internally (it is also how `Exact` keyword
matching against a case-insensitive lexer works), but the public API has no
`TextRule` and no `TextTok` (C4).

## A4 — Codegen invocation

**The stable entry is one explicit build-time generation call per grammar
crate.** A crate's `build.rs` calls
`recursa_codegen::generate("manifest/relative/root.rs")`. The selected file
is the root of one ordinary Rust module tree and contains exactly one
`recursa::grammar!` invocation before all Node-bearing declarations.

The build analyzer is the sole semantic authority. It loads the whole tree,
interprets root configuration, token syntax and helper attributes, runs
analysis, computes each Node's effective trait plan, and prepares emitted
behavior. The `grammar!` and `Node` macros are structural loaders only.
This explicitly supersedes the earlier out-of-line attribute-macro and
macro-only design.

### Build entry and source ownership

- `generate(root)` resolves one manifest-relative root and returns a typed,
  aggregated result for `build.rs` to propagate. Library code does not panic
  or terminate the process.
- The loader follows ordinary, inline, private, and `#[path]` modules. It
  emits Cargo invalidation for discovered files and declared external-module
  candidates, so edit, deletion, restoration, and creation all rerun analysis.
- Source discovery, exact module-resolution rules, Cargo tracking, syntax
  parsing, lowering, and physical diagnostic attachment are M3 work. Exotic
  nested `#[path]` cases must be specified and tested there.
- The one-root-per-crate and selected-tree ownership rules are contractual.
  Multiple roots and cross-crate grammar extension are separate designs.

### Logical generated artifact

Each grammar crate has one logical generated root. It owns the deterministic
Node registry, token and configuration data, configured trait plans, explicit
public exports, and hidden loading capability. Its physical `OUT_DIR` layout
is private: one file and internal shards are equivalent, and no hidden
filename, namespace, macro name, or shard scheme is public API.

Generation publishes only complete successful output, using atomic replacement
and an internal protocol version. Warnings may coexist with output; a hard
analysis error produces no emittable output and stops the build before stale
output can compile. `OUT_DIR` is authoritative and uncommitted. Optional
formatted debug dumps are non-authoritative; there is no checked-in generated
megafile, persistent-emission correctness path, or drift gate.

Set the Cargo-tracked `RECURSA_DEBUG_DIR` environment variable to an explicit
directory to inspect one successful Generation. Recursa writes a deterministic
index and rustfmt-style, parseable logical views of the generated root, every
Node, and the enabled capability bodies. The destination must be a plain
directory rather than a symbolic link. Recursa writes only inside its private
bundle namespace and does not remove unrelated destination contents.

This output is a generated-code debug bundle, not a generated source tree. Its
filenames, directories, index syntax, and retention are private implementation
details. Neither Recursa nor a consuming build loads the bundle, checks it in,
compares it for drift, or uses it as a fallback. Enabling or moving the bundle
may rerun Cargo generation, but it does not participate in Generation identity,
capability selection, or the authoritative compiled payload. A generation
failure publishes no partial bundle.

### Node loading, identity, and privacy

`#[derive(recursa::Node)]` declares grammar membership, legalizes helper
attributes, and loads keyed generated behavior at the declaration site. This
placement preserves private nested types and same-named declarations in sibling
modules. The analyzer—not the derive—computes effective configured traits and
field-aware behavior.

A Node key is its declaration's fully qualified module path plus item name.
Aliases and re-exports do not change it; moving or renaming the declaration
does. Keys and deterministic numeric indices are build-local identities, not
serialized IDs or semver promises. Missing keys, malformed output, and protocol
mismatches are deterministic compile failures, never silent omission or
runtime semantic fallback. Hidden generated machinery is supported only
between matching Recursa versions.

The synthetic M0 proof establishes keyed loading and configured shared trait
bodies; final field-aware body selection and physical sharding remain M4
implementation decisions.

### Diagnostics

Generation preserves M2's structured, deterministically ordered
`AnalysisReport`. Source loading maps M2 locations to deterministic physical
`path:line:column` coordinates, and the build boundary renders the collected
diagnostics once. Native compiler spans are optional. Root and Node loaders do
not reconstruct semantic diagnostics; they report only loader invariant
failures.

### Performance contract and evidence

M3/M4 must retain these representative-scale gates:

| Measurement | Absolute budget | Relative budget |
| --- | ---: | ---: |
| Generation | < 5 s | — |
| Clean `cargo check` | < 60 s | ≤ 1.25× control |
| Warm no-op `cargo check` | < 2 s | ≤ 1.25× control |
| Grammar-file edit check | < 15 s | ≤ 1.5× control |
| rust-analyzer readiness | < 60 s | ≤ 1.25× control |
| Edit diagnostics | < 2 s median, < 5 s worst | ≤ 1.5× control |

A relative failure is waived when added latency is at most two seconds. Keep
candidate-path invalidation's no-op cost visible.

The retained
[1,300-type prototype](https://github.com/freshtonic/recursa/tree/a24c3c8/recursa-codegen/prototypes/build-output-node-seam)
and its
[measurements](https://github.com/freshtonic/recursa/blob/a24c3c8/recursa-codegen/prototypes/build-output-node-seam/RESULTS.md)
are reproducible M0 evidence. All absolute budgets passed: 46.2 ms median
generation, 2.576 s clean check, 1.819 s warm no-op, 1.799 s child-edit check,
2.993 s rust-analyzer readiness, and 2.4 ms median/35.7 ms worst edit
diagnostics. The artifact is an explicit, manually run evidence fixture—not a
per-pull-request test.

## A5 — Lexer visibility

**The lexer backend is an implementation detail with no authored-code leak.**

Encapsulated surface (the 99% path): a grammar author writes the `tokens!`
DSL and consumes `sql::lex(src) -> LexResult`, `TokenKind`, and the
generated literal/target structs. No logos type appears; the grammar crate
does **not** list logos in its `Cargo.toml` (today pg-sql does).
`recursa-core` stays logos-free (`TokenRecord { kind: u16, start: u32, end:
u32 }`, `Input::new(src, &lexed)`), so the runtime/parse layer is
lexer-agnostic — a hand-rolled or alternative lexer can produce a
`LexResult` directly.

Grammar authors use the closed matcher vocabulary. Alternative lexer
implementations enter through `LexBuilder`, supplying only grammar-local
kinds and checked source spans; they cannot invoke grammar-authored scanning
or record-rewrite code.

## A6 — Spans

AST nodes carry only semantics (C4); spans are the one piece of
non-semantic information worth carrying, and they get their own model
rather than riding on fields.

### Requirements

All four capture scenarios are supported, forming an override lattice
(most specific wins — same discipline as the derive stack):

- Global: `spans = mandatory | optional | off` in the root `grammar!`
  configuration (default `off` — pay for what you use).
- Per-node: `#[spanned(mandatory)]` / `#[spanned(optional)]` /
  `#[spanned(off)]` on a struct or enum.
- Per-element: the same attribute on an individual field or variant.

### Storage: one span block per node, indexed by grammar element

A spanned struct gains **one generated field**, not a field per span:

```rust
spans: Option<SpanBlock<{ NUM_ELEMENTS }>>,   // optional mode
spans: SpanBlock<{ NUM_ELEMENTS }>,           // mandatory mode
```

where `SpanBlock<N>` is a heap block (`Box<[Span; N]>`-shaped; the exact
allocation strategy is an implementation detail behind the type) and the
index space is the node's **desugared grammar element list** from A4 step 2
— fields *and* fixed tokens. That indexing is what makes the block pay for
itself twice:

- Unstored keywords/punctuation get spans without getting fields: the
  `FROM` in `SelectStmt` has an element index, so its span is captured even
  though nothing else about it is stored. (This closes what was open
  question 5.)
- A repetition field occupies one entry recording the whole list's extent,
  separators included — the one span not reconstructible from the elements,
  which carry their own blocks recursively.

Enums cannot carry a field outside their variants, so:

- **Tuple variants** store nothing; the enum's span delegates to the inner
  node (guaranteed to exist by the single-field-tuple rule).
- **Unit variants** (`#[kwd(ASC)] Asc`) are rewritten to carry a payload
  under span capture — `Asc(Option<Span>)` / `Asc(Span)` by mode — the one
  place span mode changes a user-visible shape, and honestly so: a unit
  variant's span is genuinely new information.

### Parsing: two entry points, one trait, one emitted body

Capture is a **per-parse-session choice**, made cheap by the lazy block:
not capturing writes one `None` (8 bytes, null-pointer niche); capturing
allocates one block per node.

```rust
pub trait Parse<'i>: Sized {
    fn parse(input: &mut Input<'i>) -> Result<Self, ParseError>;
    fn parse_spanned(input: &mut Input<'i>) -> Result<Self, ParseError> {
        input.capture_spans(true);   // session-scoped flag on Input
        Self::parse(input)
    }
}
```

There is **no `ParseSpanned` trait** and no second generated body: the flag
lives on `Input`, and each generated body branches on it per element — so
the A4 code-size budget is unaffected. Per-mode semantics:

| Node mode | Storage | `parse` | `parse_spanned` |
|---|---|---|---|
| `off` | none | — | ≡ `parse` (flag ignored) |
| `optional` | `Option<SpanBlock<N>>` | sets `None` | populates |
| `mandatory` | `SpanBlock<N>` | populates | ≡ `parse` |

The two methods differ **only** for `optional`-mode nodes — which is
exactly the mode whose purpose is deferring the capture decision to the
call site. Under global `mandatory` (or global `off`), `parse_spanned` and
`parse` coincide, which is why a separate trait would have no independent
existence.

### Construction and synthesis

For every struct the M4 emitter generates:

- `fn new(<semantic fields only>) -> Self` — spans defaulted (`None` in
  optional mode; a synthetic block in mandatory mode, using a defined
  `Span::SYNTHETIC` sentinel), so `VisitMut` rewrites and tests construct
  nodes without inventing positions.
- `fn with_span(self, …) -> Self` builder for callers that do have
  positions.

`Span::is_synthetic()` distinguishes parsed from synthesized spans in
mandatory mode. (A possible refinement — defaulting `new()`'s node extent
to `Span::cover(children)` so wrapper nodes synthesized around parsed
children get real spans — is deliberately not committed here; it trades a
parsed *fact* for a derived *claim*.)

Generic code uses one universal trait across all modes:

```rust
pub trait HasSpan { fn span(&self) -> Option<Span>; }
// off → None; optional → the Option; mandatory → Some (None if synthetic)
```

### Deferred: the retrieval API

How consumers read per-element spans out of the block is a separate design
(open question 5). Placeholder proposal: generated named accessors on the
node (`fn table_span(&self) -> Option<Span>`, `fn from_kw_span(&self) ->
Option<Span>`) so no companion span structs or index constants enter the
public API.

## A7 — Flat ASTs

Reference: [Flattening ASTs and other compiler data structures
(Sampson)](https://www.cs.cornell.edu/~asampson/blog/flattening.html).
Nodes live in per-type arenas; references are `u32` indices instead of
`Box` pointers. Wins: locality, smaller nodes, bulk free, cheap
serialization, traversal in allocation order.

**Grammar definitions are written in the nested spelling regardless;
flat is an emission mode.** Two prior decisions make this mechanically
tractable:

- The FIRST/FOLLOW model is **representation-independent**: analysis,
  dispatch, lookahead, and kind-sets operate on the `GrammarDescription`,
  which says nothing about node storage. Flat is a second codegen backend
  over the same description — a different type-lowering and body-template
  set, sharing all dispatch logic with the nested backend.
- C4 already removed every non-semantic field, so the lowering table is
  short and total.

### Type lowering

| Nested spelling (as written) | Flat lowering |
|---|---|
| `Box<Expr>` | `Ref<Expr>` — `u32` + phantom, `Copy` |
| bare AST-type field `Expr` | `Ref<Expr>` (uniform rule: every AST node type is pooled) |
| `Option<T>` / `Option<Box<T>>` | `OptRef<T>` (`u32`, `MAX` = none, 4 bytes) |
| `Vec<T>` / `Vec1<T>` | `RefSlice<T>` — `(start, len)` into a ref-run side pool |
| targets / literals (`ColId`, `Number`) | unchanged — token-record-sized leaves, inline |

The `Vec` row hides the one non-obvious mechanical detail: in
recursive-descent parse order, **list elements are not adjacent in their
type's pool** (each element's own same-type descendants land between
them). So `RefSlice<T>` indexes a separate `extra: Vec<u32>` ref-run pool
— the Zig `nodes` + `extra_data` layout: parse elements, collect refs in
scratch, append the run to `extra` contiguously.

A generated per-grammar `Ast` struct owns one pool per AST type plus
`extra` (pg-sql scale: ~1300 mostly-empty `Vec`s ≈ 30 KB baseline — fine).

### Parse and derive changes

- **Builder placement**: `Input<'i, A = ()>` gains a defaulted builder
  parameter (`()` nested; the generated pool set flat) — parse signatures
  gain no parameter, nested-mode users never see it, and no rules/`Grammar`
  trait is needed to carry it (A1, "No rules type").
- **Output**: `Parse` grows `type Out` = `Self` (nested) or `Ref<Self>`
  (flat); generated bodies construct-and-return vs push-and-return-ref.
- **Pratt is the easy case**: the led loop's wrap-lhs step becomes
  push-node-referencing-lhs-ref; the pool fills in postorder (Sampson's
  RPN observation) with the driver logic unchanged.
- **The derive stack adapts too**: `Visit`/`VisitMut`/`Pretty`
  thread `&Ast` / `&mut Ast` through traversal. With `VisitMut` replacing
  the old fold-style `Transform` (A1), flat-mode rewriting is largely
  *natural*: same-type overwrite of a pool slot and append-and-repoint
  are native arena operations, and neither invalidates `Ref<T>` keys.
  Orphaned nodes are garbage until an explicit **compaction** pass —
  which is the one genuinely second-phase piece (it remaps keys and so
  must be a deliberate, standalone operation).
- **Spans compose well**: A6's per-node block becomes a *parallel pool*
  indexed by the same `Ref<T>` — strictly better than per-node boxes
  (capture-on stops allocating per node).

### Configuration

`ast = nested | flat` in the root `grammar!` configuration, default `nested`. A
"codegen-time override" must remain deterministic from source — public
type shapes may not depend on an env var (rust-analyzer and readers would
desync) — so the override form is a cfg feature
(`ast = cfg(feature = "flat", flat, nested)`). (`ast = both` — emitting a
`flat` submodule alongside nested — was considered and **cut from 1.0**:
it doubles emission for a benchmarking convenience a cfg flip already
provides.)

Honest compatibility statement: mode-switching is source-compatible for
**grammar definitions**, and for **consumers who use the portable APIs**
— `Ref<Expr>` is not `Box<Expr>`, so raw field destructuring is
mode-specific, but two mechanisms designed in from the start close the
gap: generated field/child accessors with a uniform signature
(`node.expr(cx)` in both modes; the cx is `()` and ignored in nested),
and the `Visit`/`VisitMut` portability contract in A1 (walker-resolved
nodes for `Visit`; handle + context for `VisitMut`). Consumer code
written against accessors and visitors survives a mode flip unchanged.

## A8 — Ambiguity detection and diagnostics

An AST-first grammar is written as types, so its ambiguities are *implied*
rather than declared — which makes build-time detection with excellent
messages a headline feature, not a nicety. The stance is D8/D9's: every
statically decidable conflict is a **hard build error**; nothing ambiguous
parses "best-effort".

### The catalogue of detected classes

All of these exist in the spike's `analyze()` today (nine error classes,
snapshot-tested in `tests/analysis_negative.rs`), generalised per A3 from
single kinds to kind-sets:

1. **Left recursion** — direct, through containers (`Box`, `Vec`, nested
   structs), and mutual across types. Pratt led edges are exempt (that's
   what Pratt is for); everything else is an error, never a hang.
2. **Within-type FIRST overlap** — two enum variants whose FIRST sets
   intersect and cannot be split by lookahead (see 5).
3. **Pratt overlap classes** — atom vs atom, atom vs prefix, and
   infix-vs-postfix (EXTENDERS) collisions, each reported in Pratt
   vocabulary (binding powers, positions), not raw set language.
4. **Nullable-run / use-site conflicts** — the "runs of optional fields"
   case. For `a: Option<A>, b: Option<B>, c: C`: if FIRST(A) ∩ FIRST(B)
   ≠ ∅, a token could open either `a` or `b` — ambiguous, rejected. Same
   for FIRST(A) ∩ FIRST(C) through the nullable run, and for a nullable
   *tail* colliding with what may follow the node. The statically
   decidable cases error at build time; the genuinely context-dependent
   tail cases get the worst-case-FOLLOW treatment below.
5. **LL(k) dispatch exhaustion** — variants (or optional-run decisions)
   not separable within `max_lookahead`. The analysis already computes
   the depth *needed* (`disambiguation_depth`), so the error is
   prescriptive: "FIRST² of both variants is `CREATE TABLE`; raising
   `max_lookahead` to 3 disambiguates" — or, when no finite k works
   (shared prefix of unbounded length), it says that and points at the
   structural refactor instead.
6. **Kind-set (target) collisions** — a keyword kind inside a target's
   admit-set colliding at a decision point, reported with **provenance**:
   not "kinds overlap" but "`FORMAT` can start both variants: it is
   admitted by `ColId` because `ColId admits UNRESERVED` and `FORMAT` is
   declared `UNRESERVED` (tokens.rs:412)".
7. **Definition conflicts** (new with A1's inline declarations) — same
   keyword defined with different categories, same target with different
   patterns/admit-sets, undefined bare-ident references, pattern-priority
   ties between regex tokens.

One class is runtime **by design**: D3's Pratt `EXTENDERS ∩ caller_follow`
check, which is genuinely context-dependent. The port should include the
**worst-case FOLLOW pass** (D7, deferred in the spike as validation-only)
to surface *potential* D3 sites and dynamic nullable-tail ambiguities as
build-time **warnings** — "this can only be diagnosed at runtime, and
here is the token context that would trigger it" — so runtime errors are
never a surprise.

### The diagnostic quality bar

Requirements, not aspirations — error text is a tested artifact (the
spike's insta-snapshot discipline carries over: every class has its
message pinned):

- **Every error names the grammar location** — type, field/variant, and
  source position. For inline grammar modules the macro has real token
  spans, so diagnostics attach natively (`quote_spanned!` →
  `compile_error!` at the offending field). For out-of-line module files
  the macro parsed the text itself, so rustc has no native span there —
  the message embeds `file:line` instead. (This asymmetry is a real,
  documented limitation of the A4 approach.)
- **Every ambiguity error shows a witness**: a concrete minimal token
  sequence that can be parsed two ways, printed as source-like text
  (`SELECT FORMAT …`), with both derivations named. FIRSTᵏ sequences make
  witnesses cheap to construct.
- **Set membership always carries provenance** through the atom lattice:
  which category/class/target/flag chain put the kind in the set, with
  the declaration site of each link.
- **Every error prescribes at least one fix** from its actual options:
  raise `max_lookahead` to a computed sufficient k, reorder variants with
  an explicit `#[parse(prefer)]`, split a target's admit-set, un-nullable
  a field, restructure into a shared-prefix type. If the analysis can
  compute the fix (it usually can), the message states it concretely.
- **All conflicts in one build** — errors are collected and emitted
  together (multiple `compile_error!`s), not surfaced one-per-rebuild.
- **Symbolic vocabulary** — messages say `expected ColId` / `FORMAT is
  UNRESERVED`, never bitset indices or 200-kind enumerations (A3's
  display sidecar).

### Escape hatches, in escalation order

The default is a hard error with a prescriptive fix. When the grammar
genuinely cannot be restructured, two explicitly-opt-in valves exist,
each loud and local:

1. `#[parse(prefer)]` — declaration-order tiebreak for a specific
   overlap the author has judged safe.
2. `#[parse(backtrack)]` — **bounded committed-try** over one dispatch
   site: attempt the marked variants in order with input rollback. The
   analysis still verifies termination (no left recursion through the
   site) and bounds the re-scanned token window; the site is flagged in
   generated docs. This exists to de-risk the pg-sql port against
   genuinely non-LL(k) corners (PostgreSQL's grammar is LALR; today's
   target dispatchers historically resorted to fork-and-retry) — the
   goal is **zero uses**, but the valve must exist *before* the port,
   not be designed under M7 pressure.

## A9 — Error recovery, error reporting, and hostile input

Parse failure is a user experience with three layers, all currently
missing from the model. All three must exist **before** the pg-sql port —
recovery in particular changes AST shape, which must be settled before
1300 types bake.

### Recovery and partial parse

Generated parsers support a recovering mode: instead of aborting at the
first failure, produce an AST containing error nodes plus a collected
list of all syntax errors. The mechanism is principled panic-mode
recovery, powered by the analysis we already run: on failure, skip to a
token in the failed production's **FOLLOW/sync set** (for SQL, `;`
statement boundaries are the natural coarse sync points; nested sync sets
come from FOLLOW composition). This is what makes IDE integration,
linting broken code, formatting-while-typing, and graceful proxy
degradation possible — the capability that defines tree-sitter and
rust-analyzer.

Recovery is a **per-call choice** (a recovering entry point, A10), not a
grammar property — but the *representation* of error nodes is a type-level
question with real options (generated `Error` variant per enum vs a
`Recovered<T>` wrapper vs a side-table of error spans with `Option`-ed
fields). That representation decision is open question 8 and is resolved
at the start of the runtime-robustness milestone, before any pg-sql work.

### Runtime error rendering

A8 set the bar for build-time diagnostics; runtime parse errors get the
same treatment, out of the box with zero configuration:

- Source excerpt with caret/underline span, line:col via a `LineIndex`
  utility over the source (spans stay byte offsets internally).
- Expected-sets derived from the FIRST/FOLLOW machinery, rendered
  **symbolically** through the atom lattice (`expected ColId, '(' or
  SELECT` — never a 200-kind dump), using the `GrammarInfo` tables.
- A `Display` impl that is genuinely good unformatted, plus an optional
  `miette` integration feature for consumers already in that ecosystem.

(This restores the spike follow-up item "human-readable error rendering"
that had silently fallen out of the plan.)

### Hostile input

Recursa parsers must be safe to point at untrusted input — the proxy use
case makes this a hard requirement, not hygiene:

- **Recursion depth guard**: a configurable depth counter on `Input`
  (grammar-attribute default), checked in generated recursive entries.
  Deeply nested input (`((((…`) returns a depth error; it never
  overflows the stack. (sqlparser carries the same guard for the same
  reason.)
- **Lexing never panics**: unrecognised input becomes an `Error` token
  kind (added automatically to every grammar), surfaced as a parse
  error — the spike's `tokenize` panic does not survive the port.
- **A generated fuzz target** per grammar: lex + parse over arbitrary
  bytes with the invariant *no panic, no overflow, no non-linear blowup*.
  A sustained clean fuzz run is a production gate
  ([`port-plan.md`](./port-plan.md)).

## A10 — Entry points and trivia

### Entry points

The three-step consumption dance (`lex`, `Input::new`, `T::parse`) is the
low-level API. Each grammar module also gets generated one-liners:

```rust
sql::parse::<SelectStmt>(src)            // lex + parse + all-input-consumed
    -> Result<SelectStmt, ParseReport>   //   check + render-ready errors
sql::parse_prefix::<Expr>(src)           // explicit prefix variant
sql::parse_recovering::<Script>(src)     // A9: (ast with error nodes, Vec<Error>)
```

"Must consume all input" is the *default* and explicit in the API —
trailing tokens are an error unless the `_prefix` variant was chosen. The
A6 span modes compose (these entries respect the grammar's span config;
`parse_spanned` variants exist where the mode makes them meaningful).

### Trivia (comments)

`ignore` currently discards comments, which is untenable for a framework
that ships a formatter and serves SQL *rewriting*: silently deleting
comments is not production-grade round-tripping.

The fix preserves C4's discipline (AST stays pure semantics):

- The lexer **retains trivia spans in `LexResult`** — spans only,
  payload-free, zero AST impact. `ignore` entries are still never tokens
  the parser sees.
- A standard attachment convention (Roslyn-proven): leading trivia binds
  to the next token; same-line trailing trivia binds to the previous.
- `Pretty` becomes trivia-aware: emission looks up a node's token
  range in `LexResult` and re-emits attached comments, so format and
  rewrite round-trips preserve them. Untouched nodes keep their comments
  verbatim; synthesized nodes have none.

## A11 — Grammar-powered testing

The framework knows the entire grammar; it should generate the test
infrastructure no hand-rolled parser gets:

- **Round-trip property tests, generated.** For grammars deriving
  `Arbitrary` + `Pretty`, the emitter generates (feature-gated, test-only)
  the property `parse(format(ast)) == ast`. One property catches drift in
  three artifacts at once — grammar, formatter, parser — and it costs the
  user nothing to have.
- **Grammar coverage reports.** Test-instrumented dispatch counters
  (behind a feature) record which productions, variants, and dispatch
  arms a corpus actually exercises; a report names the untested parts of
  the grammar. "Our corpus covers 91% of variants; here are the 47 it
  never touches" is a sentence no other Rust parser framework can emit.
- **A reusable corpus/differential harness.** pg-sql's per-statement
  differential runner and corpus-file test macro are bespoke today;
  extract them into a `recursa-test` crate: a corpus format, a
  statement-splitter hook, and an oracle trait — so *any* grammar can be
  differentially validated against a reference implementation the way
  pg-sql is validated against PostgreSQL.

## A12 — Pretty-printing: derived defaults

(`Pretty` is the renamed `FormatTokens` — the old name described the
mechanism, the new one describes the job.)

### Trait shape: token stream below, `Display` on top

The old `FormatTokens` contract — fill a `Vec` of tokens, let something
else stringify — survives as `Pretty`'s **lower layer**, because a token
stream is the right interface for tooling (token-level diffing,
highlighting, custom renderers). String rendering is a **provided** layer
on top, ending in a `Display`-shaped API:

```rust
pub trait Pretty {
    /// The ported FormatTokens contract: emit the node's token/doc events
    /// (tokens, group open/close, break hints, trivia) in grammar order.
    fn tokens(&self, cx: &Cx, out: &mut TokenSink);

    // Provided — token stream → layout engine → text, streaming straight
    // into the Formatter (no intermediate String or Vec round-trip):
    fn display(&self, cx: &Cx) -> impl fmt::Display + '_;          // default config
    fn pretty(&self, cx: &Cx, config: &PrettyConfig) -> impl fmt::Display + '_;
}
```

- `TokenSink` carries structured events, not strings: token references
  (display string or source slice), the layer-2 layout markers (group,
  indent, soft/hard break), and attached trivia — so the sink *is* the
  document algebra input, and alternative consumers can ignore the
  markers and read it as the flat token vector the old trait produced.
- **Every token event carries a source span** pointing back into the
  original `Input` (`Span::SYNTHETIC` when there is none). Content tokens
  (literals, identifiers, targets) always have one — their `TokenRecord`
  is the span; fixed tokens have one when the node captured a span block
  (A6), synthetic otherwise; synthesized nodes are synthetic throughout.
  A map-collecting render variant exploits this:

  ```rust
  fn pretty_mapped(&self, cx: &Cx, config: &PrettyConfig) -> (String, SourceMap)
  // SourceMap: output byte ranges → original-input spans
  ```

  This is what makes reformat-then-trace-back work end to end: a
  downstream error at position X in the *rewritten* text maps through the
  `SourceMap` to the user's *original* input span — the proxy use case —
  and `Span::is_synthetic()` distinguishes "this output came from your
  query" from "this output was injected by the rewrite".
- In **nested** mode (`Cx = ()`) the macro additionally emits a real
  `impl fmt::Display for T` delegating to `display()` — `format!("{stmt}")`
  just works. In **flat** mode `Display` needs the pools, so the adapter
  form `node.display(cx)` is the portable spelling (same cx discipline as
  `Visit`, A1).

The question: how much "sensible" formatting can the generated `Pretty`
impls produce with **zero** manual annotation? Answer: almost all of it —
because the annotations that drive *parsing* double as style facts, and
the macro holds the complete grammar-element sequence for every node.
Three derivation layers, each mechanical:

### Layer 0 — correctness (guaranteed)

Emit tokens in grammar-element order (the macro knows the exact sequence
per node, fixed tokens included) and insert whitespace **only where
required for re-lexability**. Which adjacent token-kind pairs need a
separator is computed *at generation time* from the lexer itself (would
the two rendered texts re-lex as the same two tokens?) — a static
adjacency table, no runtime checking. Output at this layer is dense but
**provably round-trips**, and the A11 property test
(`parse(pretty(ast)) == ast`) enforces that it stays true.

### Layer 1 — readability from parse facts (automatic)

Every spacing decision a human would annotate is already latent in the
grammar attributes:

| Parse fact | Derived style |
|---|---|
| word-shaped tokens (keywords, idents) | space-separated (required anyway) |
| `#[sep(",")]` | no space before separator, space after |
| `#[surrounded("(", this, ")")]` | tight inside delimiters |
| `#[parse(infix)]` operator | spaces around |
| `#[parse(prefix)]` / `#[parse(postfix)]` operator | tight against operand |
| literals / identifiers / targets | verbatim source text (via span); canonical text when synthesized |
| keyword display strings | rendered in the grammar's configured `keyword_case` (meaningful because matching is case-insensitive) |

With zero annotations this already yields canonical `SELECT a, b FROM t`
/ `f(x, y)` / `NOT a + b` style output.

### Layer 2 — layout from structure (automatic)

Multi-line formatting derives a Wadler/Prettier-style document algebra
from node shape — the same information, one level up:

- every node is a **group** (fits-on-one-line-or-breaks);
- each `#[sep]` repetition element is a soft-break opportunity
  (break-all-or-none per group);
- `#[surrounded]` content is an indented block with soft breaks inside
  the delimiters;
- top-level statement dispatch (and explicit `;` separators) are hard
  breaks.

Under `max_width` pressure, groups break outermost-first — Prettier
behaviour, derived rather than hand-written. Grammar-level knobs:
`max_width`, `indent`, `keyword_case`. One derived gem worth calling out:
**parenthesization of synthesized trees is exact**, because Pratt binding
powers are known — printing `Mul(Add(a, b), c)` inserts the parentheses
around the addition and *only* there, without any annotation and without
defensively parenthesizing everything.

### What defaults cannot know — the escape hatch

Idiom, not structure: `a::int` is conventionally tight while `a + b` is
spaced (both infix); comma-leading vs comma-trailing style; SQL's aligned
clause conventions. These get small, additive overrides — a spacing spec
on the token definition (`#[tok("::", spacing = tight)]` / in the
`punctuation` entry) and `#[pretty(…)]` on nodes/fields where taste
diverges from the derived default. The design goal is that pg-sql needs a
*handful* of these, not per-node annotation — the current fmt golden
tests are the taste benchmark, and trivia re-attachment (A10) composes:
an attached comment forces its group to break, since a comment cannot
live mid-single-line.

Layered this way, the guarantee structure is clean: layers 0–2 are always
round-trip-correct by the A11 property; goldens judge taste; overrides
adjust taste without ever being able to break correctness (a spacing
override that would merge tokens is rejected at build time by the same
adjacency table).

## A13 — The AST explorer

Ships with recursa as **`recursa-explorer`**, a separate workspace crate
that grammar crates pull in as a dev-dependency. Since parsers are
compiled Rust, the explorer runs *with* the grammar, not against it —
one line in the grammar crate:

```rust
// examples/explore.rs
fn main() { recursa_explorer::run::<sql::Script>() }
```

Two panes: **source text** on one side, the **parsed AST** on the other,
re-parsed live on edit (via `parse_recovering` + `parse_spanned`, so
broken input still shows a partial tree with error nodes and rendered A9
diagnostics inline — the explorer doubles as the review surface for the
M5/M6 error-message check-ins).

- **Generated syntax highlighting, zero config.** Token kinds classify
  into highlight classes straight from the grammar: keywords,
  punctuation, literals, identifier-targets, trivia/comments. The default
  highlighter is a byproduct of `GrammarInfo` + `lex()` — every grammar
  gets one for free.
- **Bidirectional hover.** Hovering an AST node highlights its source
  range; moving the cursor in the source selects the deepest node whose
  span contains it. Powered by A6 span blocks (the explorer always parses
  with capture on).
- **Reformat.** When `Pretty` is derived, a reformat action renders the
  A12 output — and because `pretty_mapped()` produces a `SourceMap`,
  hover keeps working on the *reformatted* text, mapped back through to
  the original AST and source.
- **Random samples via `Arbitrary`.** When `Arbitrary` is derived, a
  generate action creates a random AST and renders it through `Pretty`
  into the source pane (a seed control makes samples reproducible). This
  is the interactive twin of the A11 round-trip property: generate →
  pretty → re-parse live, with any mismatch displayed as a diff — grammar
  corners that only a fuzzer would find, surfaced while you watch. It's
  also the fastest way to *see* what a grammar under construction
  actually accepts. Requires `Pretty` (to render the sample as text);
  with `Arbitrary` alone the sample displays in the tree pane only.
- **Flat-mode introspection.** Under `ast = flat`, the tree pane can
  additionally show `Ref` keys and pool statistics — a teaching tool for
  the A7 representation.

What the M4 emitter must generate to power this (gated behind an
`explorer` feature so production builds pay nothing): an extended
`GrammarInfo` carrying type/variant/field names and token highlight
classes, plus a generated **dynamic tree adapter** — an object-safe walk
built on `Visit` yielding `(node name, span, children)` — so the explorer
UI is written once, generically, in `recursa-explorer` rather than
per grammar.

UI technology is an implementation detail, not API: recommendation is a
local single-page web UI served by the crate (richest hover interactions,
no native toolkit dependency, and it leaves the door open to a
WASM-compiled shareable playground later — the tree-sitter-playground
precedent); a TUI or egui frontend could swap in behind the same dynamic
adapter.

## Migration notes (pg-sql)

Not a migration plan — just the impact assessment for the biggest user:

- The mechanical restructure: move `ast/**` + `tokens.rs` under one selected
  grammar root, add `Node` membership to grammar types, and delete the 1267
  per-item attribute stacks, `rules = SqlRules` args, `src/generated/`, the
  `__firstset` prelude, the xtask targets, and `pg_lex` (replaced by closed
  matchers and admissions).
- The C4 rewrite is the large, grammar-wide edit: every keyword/punctuation
  field (`pub r#as: AS`, `pub lparen: punct::LParen`, `Surrounded<…>`
  spellings) becomes a `#[tok]`/`#[kwd]`/`#[sep]`/`#[surrounded]` attribute
  and the field disappears. Mostly mechanizable (the old field type names
  the token), but it touches essentially every AST struct and every
  downstream constructor and pattern match. This is the loudest break, and
  also where the payoff concentrates: ~500 deleted types plus visibly
  smaller AST nodes.
- pg-sql would likely keep its ~450-keyword table in the central `tokens!`
  block (a table is the right form at that scale) and use reference-form
  `#[kwd(SELECT)]` at use sites; inline definition mainly serves smaller
  grammars and the long tail (one-off punctuation, dialect additions).
- `soft_keywords` entries redistribute into `keywords`/categories + flags;
  the five hand-written `Parse` impls are replaced by `targets` entries
  (`UnquotedIdent`'s reclaim set, `CustomOp`, `BareAliasName`).
- Target types change shape: consumers of the old `Base`/`Keyword`
  two-variant enums now read one struct with a kind-preserving token.
  `Visit`/`Pretty` output shapes change accordingly, and existing
  `Transform`-based rewrites are re-expressed as `VisitMut` impls (A1) —
  mostly mechanical, since fold bodies become in-place assignments.

## Resolved M0 questions

1. **MSRV.** Rust 1.88 is the workspace MSRV. The accepted build analyzer no
   longer depends on `proc_macro::Span::local_file()` for source discovery.
2. **Compile-time budget.** The A4 table is the accepted performance contract.
   The retained 1,300-type prototype passed every absolute gate, so the
   build-output/Node seam is the stable direction; persistent source-tree
   emission is not a correctness fallback.

## Open questions

3. **Grammar-locality rule.** Any known case where a `Parse` type *must*
   live outside the grammar module (cross-crate grammar composition,
   dialect layering for ms-sql/my-sql)? If dialects need to extend a base
   grammar, that's a real design extension this document does not cover.
4. **Attribute spellings and disambiguation** — settle by M3, before any
   grammar bakes spellings in. Standing recommendation: **unify `#[tok]`
   and `#[kwd]` into a single `#[tok]`**, classifying each defined token
   as keyword vs punctuation by word-shape — the same mechanical test the
   keyword/identifier lexing merge (A2) already performs — so there is
   exactly one fixed-token attribute. Also to settle: (a) a `this`
   placeholder could unify `#[tok]`/`#[trailing]` into one shape
   (`#[tok(CASE, this, END)]`); (b) `#[tok]` on a field currently means
   *both* "consume before the field" (punctuation/reference args) and
   "define the field's own type" (`r"pattern" admits …` args) — fine to
   overload on argument shape, or split the definitional form?
5. **Span retrieval API and block allocation.** The span block (A6) stores
   per-element spans; how consumers read them is undecided — generated
   named accessors (proposed), an index-based generic API, or both. Also
   undecided: `SpanBlock`'s allocation strategy (per-node `Box` vs an
   arena owned by `Input`) and whether `new()` should default the node
   extent to `Span::cover(children)` rather than synthetic.
6. **Case-insensitivity.** Keyword matching is case-insensitive in both
   codebases today (SQL-appropriate). Should the DSL make this configurable
   per grammar (`case = insensitive | sensitive`) for non-SQL languages?
7. **Flat-mode scope (A7).** With `VisitMut` replacing `Transform`,
   in-place flat rewriting works as soon as flat mode does — the open
   scope question narrows to **compaction** (the explicit,
   key-remapping GC pass): first cut or later? And should the uniform
   pool-everything rule be relaxed later (inline non-recursive nodes for
   locality), or is uniformity worth keeping for model simplicity?
8. **Error-node representation (A9).** Recovered ASTs need error nodes in
   the types: a generated `Error` variant per enum, a `Recovered<T>`
   wrapper at recovery boundaries, or a side-table of error spans with
   `Option`-ed fields? Resolve at the start of the runtime-robustness
   milestone — it shapes every AST type and must precede the pg-sql port.

## Relationship to the spike codebase

| Design piece | Spike artifact it builds on |
|---|---|
| `GrammarDescription` lowering (A4 step 2) | `src/analysis.rs` description types — the scanner's output format already exists; `#[tok]` attrs desugar to single-token field positions |
| Analysis + diagnostics (A4 step 3) | `analyze()` — ported as-is; errors gain source spans |
| Emission (A4 step 4) | `src/codegen.rs` `quote!` pipeline — already token-stream-shaped; combinator inlining extends D5 from analysis to emission |
| Kind-set token types (A3) | `SingleTokenKind` generalised `KIND` → `KINDS` bitset; `TextRule` machinery internalised |
| Atom lattice display (A3) | `Atom` Category/Class/Target variants — designed in D1, deferred in the spike, used here |
| LL(k) dispatch for overlap corners (A3) | `max_lookahead` + FIRSTᵏ from post-spike M9 |
| Diagrams (A1 `diagrams` flag) | M11 railroad subsystem; fragments emitted by the same macro; fixed-token attrs render as terminal boxes |
| Fixed-tokens-as-attributes (C4) | Replaces both the spike's `Tok<Marker>`/`TextTok<Marker>` blanket pattern and per-keyword wrapper types, both explicitly rejected |
| Span blocks (A6) | New — no spike artifact; builds on `Token<R>`/`Span` and `Input` position tracking. The element index space is A4 step 2's desugared element list |
| Flat ASTs (A7) | New — enabled by the analysis being representation-independent (`GrammarDescription` says nothing about storage); the builder rides a defaulted `Input<'i, A = ()>` parameter |
| Ambiguity catalogue (A8) | The spike's `AnalysisError` variants + nine snapshot-tested negative classes (`tests/analysis_negative.rs`), `disambiguation_depth` for prescriptive LL(k) errors, `text_rules_overlap` as the overlap-with-provenance prototype; D7's worst-case FOLLOW pass gets promoted from deferred to required-for-warnings |
| No rules type (A1) | Deliberately supersedes `conclusions.md` recommendation #1 ("take the trait surface verbatim"): the analysis/const model ports intact, but the runtime boundary adopts current recursa's `u16` kind erasure instead of `Parse<R>` / `Grammar` — combinator inlining (A4) removed the only runtime consumer of the generic association |
| Recovery & error rendering (A9) | Sync sets fall out of the FOLLOW composition the spike already computes; expected-set rendering reuses the D1 atom-lattice display sidecar; the spike's `tokenize` panic is explicitly *not* ported |
| Grammar-powered testing (A11) | Generalizes `../recursa-old`'s bespoke pg-sql infrastructure: `tests/differential.rs` (per-statement oracle runner) and `benches/parse.rs` (corpus harness) become the `recursa-test` crate |
| Derived `Pretty` (A12) | Renames current recursa's `FormatTokens`; the element sequences from A4 step 2 supply token order, the C4 attributes supply spacing facts, Pratt binding powers supply exact parenthesization; `pg-sql/tests/fmt` goldens are the taste benchmark |
