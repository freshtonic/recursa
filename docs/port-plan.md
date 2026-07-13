# Recursa Port — Production Plan

## Goal

Take Recursa from its current state to **production-ready**, implementing
the developer-facing API in [`api-design.md`](./api-design.md) (A1–A13) on
top of the FIRST/FOLLOW model validated by this spike. The ambition: the
easiest-to-use parser generator in Rust.

**Production readiness is validated by a full port of pg-sql.** The
mandatory acceptance gates, in James's words:

1. **The pg-sql differential regression suite is green** — per-statement
   differential testing against PostgreSQL 17.9's `raw_parser` (via
   `pg-oracle`) over the vendored Postgres regression corpus
   (`pg-sql/tests/differential.rs`), with pass counts at least equal to
   the pre-port baseline.
2. **The benchmark suite is ported and run** — `pg-sql/benches/parse.rs`
   (self-contained harness over the regression corpus + stress fixtures,
   reporting into `docs/benchmarks/`), **including the comparison
   benchmarks against `sqlparser`**. No throughput regression against
   pre-port recursa; the sqlparser comparison is published in the report.
3. The fmt golden tests (`pg-sql/tests/fmt`) stay green — extended with
   comment-preservation cases (A10 trivia).
4. **A sustained fuzz run is clean** — the generated fuzz target (A9:
   lex + parse over arbitrary bytes) completes a CI-length run with zero
   panics, zero stack overflows, no pathological blowup.

Nothing ships as 1.0 until all four gates pass.

## Working principles

Carried over from the spike plan, plus lessons from this design phase:

- **Stop and check in after each milestone.** Wrong on M1 means redesign
  before M2, not after M7.
- **pg-sql is the proof, not the guinea pig.** Every subsystem is
  validated on the six spike grammars (toy, arith, json, sql, optq, cskw)
  before pg-sql touches it.
- **Snapshot everything**, especially diagnostics — A8 makes error text a
  tested artifact (ui tests with pinned messages).
- **Mechanize the migration.** pg-sql has ~1300 AST types; the C4 rewrite
  is performed by a written tool, not by hand.
- **No hand-written `Parse` impls, ever** (C3). If pg-sql needs one, the
  framework has a gap — fix the framework.

## Milestones

### M0 — Repository bootstrap & de-risk (≈ 1 week)

- **M0a — new repository.** The port is built in a **new repo**, not in
  place: `freshtonic/recursa` on GitHub, **private**. (The previous
  implementation was renamed to `freshtonic/recursa-old` — locally
  `~/projects/recursa-old` — to free the name.) The new repo is a Cargo
  workspace holding **all `recursa-*` crates**: `recursa` (the facade
  users depend on), `recursa-core` (runtime), the grammar macro /
  codegen crates, `recursa-test` (A11), and `recursa-explorer` (A13) —
  exact proc-macro crate split is an implementation detail. Grammar
  crates join the same workspace as they land (the six spike grammars at
  M3–M4; pg-sql with its vendored Postgres corpus and oracle crates at
  M7, mirroring the recursa-old layout).
- **M0b — macro-scale spike.** Generate a synthetic ~1300-type grammar;
  measure attribute-macro expansion time, `cargo check` time, and
  rust-analyzer latency. Answers api-design OQ2 with numbers. Go/no-go on
  the A4 clean path; if no-go, adopt the documented fallback
  (`RECURSA_EMIT_DIR`-persistent emission) *now*, not at M7.
- **M0c — MSRV.** Verify `proc_macro::Span::local_file()` on stable 1.88
  and fix the workspace MSRV (api-design OQ1).

**Exit:** the private `freshtonic/recursa` workspace exists with CI
scaffolding; OQ1/OQ2 answered in `api-design.md`; A4 mechanism committed.

### M1 — Runtime core (≈ 1–2 weeks)

Port the spike's trait surface into `recursa-core`, with the design's
generalisations — and its one major *reduction*: no rules type. Kinds are
erased to `u16` at the runtime boundary (A1, "No rules type"); `KindBitset`
and width selection move to codegen-internal.

- `Parse` (R-free, with `type Out` per A7 and the A6 method pair),
  `Input<'i, A = ()>` (span-capture flag, defaulted builder slot),
  `ParseError` + `GrammarInfo` kind tables, payload-free `TokenRecord`,
  width-erased runtime `FollowSet`.
- Kind-**sets** on token types (A3), as codegen-emitted constants.
- `Vec1`, `Span` (+ `SYNTHETIC`), `SpanBlock`, `HasSpan`.

**Exit:** `recursa-core` compiles standalone with unit tests; no macro
yet.

### M2 — Analysis port (≈ 1 week)

`analyze()` moves into `recursa-codegen`, generalised: kind-set overlap
(subsuming `TextRule`), FIRSTᵏ + `disambiguation_depth` (kept — it powers
prescriptive errors), left-recursion, all D9 conflict classes, **plus**
the D7 worst-case-FOLLOW pass (promoted from deferred to required — it
drives A8's build-time warnings) and provenance tracking through the atom
lattice.

**Exit:** the spike's analysis test suite — positive cross-checks and all
nine negative snapshot classes — ported and green under the kind-set
model.

### M3 — Grammar macro, front half (≈ 2 weeks)

The source-scanning front end (the "one large missing piece"):

- Module-tree loader: `Span::local_file()`, `mod`/`#[path]` resolution,
  `include_bytes!` dependency tracking.
- `tokens!` interpretation (one parser — C2) + inline
  definition/reference unification ("strings define, idents refer"),
  conflict detection, derived lexer priority.
- Lowering to `GrammarDescription`: `#[tok]`/`#[kwd]`/`#[sep]`/
  `#[surrounded]`/`#[trailing]` desugar, Pratt attributes, targets,
  lookahead.

**Exit:** the six spike grammars, re-expressed as annotated modules,
lower to descriptions equal to the spike's hand-built ones (differential
test against `src/descriptions/`).

### M4 — Emission, nested mode (≈ 2 weeks)

The back half, adapted from the spike's `codegen.rs`:

- Token machinery (`TokenKind`, `lex()`, `post_lex`, literal/target
  structs — real structs, C4), `GrammarInfo` tables, bitset-width
  selection (internal), and the automatic keyword/identifier lexing merge
  (word-shaped keywords lexed via the identifier rule + generated keyword
  table, no per-keyword DFA entries — A2).
- `Parse` bodies: inlined trivial combinators, shared `Seq` machinery,
  LL(k)/kind-set dispatchers, span-block population.
- Derive-stack application — including derived-default `Pretty` impls
  (A12 layers 0–2: lex-safe spacing table, parse-fact spacing, structural
  layout groups) — `RECURSA_EMIT_DIR` debug dump, railroad diagrams.

**Exit:** all six spike grammars parse via macro-generated code; the
spike's full test suite (167 tests) green under the new API.

### M5 — Diagnostics to the A8 bar (≈ 1–1.5 weeks)

Wire the A8 catalogue to real diagnostics: witnesses, provenance chains,
prescriptive fixes (computed `max_lookahead`, etc.), multi-error
emission, native spans for inline modules / `file:line` for out-of-line.

**Exit:** every A8 error class has a ui test with its message pinned;
**James reviews the actual rendered messages** (the quality bar is
subjective — this is an explicit check-in, not a formality).

### M6 — Runtime robustness & round-trip fidelity (≈ 2–3 weeks)

Everything the pg-sql port must land *on top of* — none of this is
retrofittable after 1300 types bake (A9–A11):

- **First task: resolve OQ8** (error-node representation) — it shapes
  every AST type.
- Error recovery: FOLLOW-derived sync sets, recovering entry points,
  multi-error collection, error nodes per the OQ8 decision.
- Runtime error rendering: `LineIndex`, excerpt + caret, symbolic
  expected-sets, quality `Display`, optional miette feature.
- Hostile-input hardening: depth guard on `Input`, auto `Error` token
  kind (lexing never panics), generated per-grammar fuzz target.
- Generated entry points: `parse` / `parse_prefix` / `parse_recovering`
  one-liners with the all-input-consumed default.
- Trivia: `LexResult` retains comment spans; attachment convention;
  trivia-aware `Pretty`.
- Generated round-trip property tests (`parse(format(ast)) == ast`) for
  `Arbitrary` + `Pretty` grammars (A11) — used immediately by the
  spike grammars.

**Exit:** spike grammars pass a broken-input corpus (recovery produces
error nodes + full error lists at the right sync points); a CI-length
fuzz run over all six grammars is clean; **James reviews rendered
runtime parse errors** (same subjective-bar check-in as M5).

### M7 — pg-sql port (≈ 2–4 weeks)

- Write the **mechanical rewriter** (syn-based) for the C4 edit: keyword/
  punctuation fields → attributes, `Surrounded`/`Seq*` → attributes +
  `Vec`/`Vec1`, `rules = SqlRules` removal, attribute-stack deletion.
- Restructure `ast/**` + `tokens.rs` under `#[recursa::grammar]`;
  `soft_keywords` → categories/flags/targets; the five hand-written
  `Parse` impls → `targets` entries; `pg_lex` → `post_lex` hook.
- Delete `src/generated/`, the `__firstset` prelude, xtask codegen
  targets, and the CI drift gate.
- The port lands **on** M6: recovering entry points wired, depth guard
  configured, trivia-aware formatter (add comment-preservation golden
  cases), generated round-trip property test enabled. The
  `#[parse(backtrack)]` valve (A8) is available for genuinely non-LL(k)
  corners — target: zero uses; any use is reviewed.
- **Gate 1: differential suite green** (per-statement pass counts ≥
  baseline — capture the baseline numbers *before* starting).
- **Gate 2: fmt goldens green**, including the new comment-preservation
  cases.
- **Gate 4 (first full run): pg-sql fuzz target clean** over a CI-length
  run.

**Exit:** gates green. Expect this milestone to surface framework gaps;
they flow back into M4–M6 as fixes, never as manual impls in pg-sql. The
2–4 week estimate assumes gaps stay small — the M6 groundwork and the
backtrack valve are the insurance on that assumption.

### M8 — Benchmarks (≈ 0.5–1 week) — **mandatory gate**

- Port `benches/parse.rs` to the new entry points (`lex()` + generated
  parse), regenerate stress fixtures if the AST shapes moved.
- Run the full harness: pg-sql (new) vs pg-sql (pre-port baseline) vs
  `sqlparser` on the regression corpus + stress fixtures.
- **Acceptance: no throughput regression vs pre-port recursa**, and the
  sqlparser comparison published in `docs/benchmarks/<run>/report.md`.
  If regressed: profile (prime suspects: span capture defaults, kind-set
  peek, Seq machinery, trivia retention, depth-guard checks) and fix
  before proceeding — performance is part of production-ready, not a
  follow-up.

### M9 — Production hardening & developer experience (≈ 2–3 weeks)

- README rewritten against the real API (killing the documented-but-dead
  `derive(Parse)`/`Scan`/`ParseRules` surface — C6).
- Rustdoc pass, examples (a small non-SQL grammar as the showcase — the
  "easiest parser generator" claim needs a 50-line proof).
- **Generated grammar docs**: EBNF emission alongside the railroad
  diagrams — a self-documenting grammar page per crate from rustdoc.
- **`recursa-explorer`** (A13): the interactive AST explorer crate —
  live parse panes, generated syntax highlighting, bidirectional
  span hover, Pretty-powered reformat with source-map hover,
  Arbitrary-powered random-sample generation. Consider
  pulling the first cut earlier if M7 debugging would benefit (it renders
  recovery output and diagnostics, so it also serves the M5/M6
  error-review check-ins).
- **Grammar coverage reports** (A11): instrumented dispatch counters +
  report tooling; run against the pg-sql corpus and publish the numbers.
- **`recursa-test` extraction** (A11): the corpus/differential harness
  generalized out of pg-sql into a reusable crate.
- Macro-crate compile hygiene: feature-gate diagrams and the debug
  formatter so the mandatory dependency path stays lean; cold-build time
  in the CI budget.
- MSRV + semver policy (including the logos re-export contract, A5),
  CI matrix (tests, ui-test snapshots, macro build-time budget from M0b,
  bench smoke run, scheduled fuzz job), release wiring.

**Exit / Definition of done:** all four mandatory gates green, docs
truthful, CI enforcing all of it.

## Explicitly after 1.0

Additive work, deliberately not gating production readiness:

- **A7 flat-AST mode** (the design ensures it slots in as a second
  emission backend; with `VisitMut` in place of `Transform`, flat
  rewriting comes with it — only arena **compaction** is its own
  project).
- Dialect layering / cross-crate grammar composition (api-design OQ3 —
  needed for ms-sql / my-sql, which also have oracle corpora waiting).
- The remaining `Atom` lattice extras beyond what A3 uses.

## Effort summary

| Phase | Estimate |
|---|---|
| M0 de-risk | 1 wk |
| M1–M2 runtime + analysis | 2–3 wk |
| M3–M5 grammar macro + diagnostics | 5–5.5 wk |
| M6 runtime robustness + round-trip | 2–3 wk |
| M7–M8 pg-sql port + benchmarks | 2.5–5 wk |
| M9 hardening + DX | 2–3 wk |
| **Total** | **≈ 14–20 weeks** |

Estimates assume the M0b result is a go; the wide M7 range reflects how
much the mechanical rewriter can absorb (it should absorb most of it).
