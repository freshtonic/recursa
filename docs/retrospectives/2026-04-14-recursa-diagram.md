# recursa-diagram: Implementation Retrospective

**Date:** 2026-04-14
**Scope:** 35 commits, `c6e3b62..HEAD`
**Phases:** 6 (crate scaffolding, composite geometry, SVG rendering, proc macro, pg-sql integration, polish and review)
**Outcome:** `#[railroad]` attribute macro shipping as a 3-crate facade; 274 annotated types in `pg-sql/src/ast/`; 650 SVG diagrams visible in generated rustdoc. 65 tests, all green.

---

## What We Built

A proc-macro attribute `#[railroad]` that walks a `Parse`-derived AST type's structure one level deep, builds a railroad diagram, serializes it to inline SVG, and emits the result as a `#[doc = "<svg>...</svg>"]` attribute. No CLI, no external SVG crates, no runtime dependency — diagrams live entirely in rustdoc.

The implementation ships as three crates:

- `recursa-diagram-core` — layout algorithm and SVG serializer (pure library)
- `recursa-diagram-macros` — the `#[railroad]` proc-macro crate
- `recursa-diagram` — thin facade re-exporting both; what downstream users depend on

---

## Architectural Decisions

### The Two Restructures

The plan specified a single crate. We needed two restructures before the design was stable, both caused by the same underlying constraint: **proc-macro crates cannot export non-proc-macro items**.

**Restructure 1 (Batch 1 review → commit `047fcc9`):** The first code review caught that `recursa-diagram` could not be both a regular lib (layout + SVG code) and a proc-macro crate. We split into `recursa-diagram` (lib) and `recursa-diagram-macros` (proc-macro shell), matching the well-known serde / serde_derive pattern.

**Restructure 2 (Phase 4 → commit `400c889`):** When we started implementing the proc macro in Phase 4, we discovered the macro needed to call into the layout/render code at expansion time. With `recursa-diagram` depending on `recursa-diagram-macros`, making `recursa-diagram-macros` depend back on `recursa-diagram` would have been a cycle. We split again into a three-crate facade: `recursa-diagram-core` holds all the library code; `recursa-diagram-macros` depends on `recursa-diagram-core`; `recursa-diagram` is a thin re-exporting facade.

**Lesson:** The proc-macro constraint — "proc-macro crates are opaque, you can't reach back through them for library code" — is well-documented but easy to overlook at design time. Any future project that wants a "proc macro with library utilities" should design the 3-crate facade from day one. The design doc should have asked: "will the macro need to call into library code at expansion time?" If yes, start with the core/macros/facade split.

### The 3-Crate Facade Is the Right Shape

In hindsight, the 3-crate layout is minimal and has no redundancy. A 2-crate shape (core + facade-that-is-also-proc-macro) does not work. A 4-crate shape adds complexity without benefit. The serde analogy maps cleanly: `serde` = facade, `serde_derive` = proc-macro, `serde`'s actual types = what would be `serde-core` if serde had it. Our `-core` sits where serde's lib does, and the facade is the user-facing entry point.

### No Hrefs from the Macro

The design doc specified intra-doc hrefs on non-terminal boxes linking to referenced type documentation. We attempted to implement this (commit `d354684`) and then dropped it entirely in the Phase 5 polish commit `fbf6591`.

The root cause: a proc macro at expansion time only sees the syntax of the annotated type. It does not know the module path of the *calling* crate, nor does it know where referenced types live relative to the rustdoc output. Any href formula we constructed was a guess, and Phase 5's representative-subset pass revealed that most guesses produced 404s. The `type_href` function and its seven tests were deleted.

**Lesson:** Intra-doc link resolution is a rustdoc feature; proc macros don't have the context to replicate it. If we want clickable type references in the future, the right path is the new rustdoc intra-doc link syntax (`[TypeName]` in doc comments) or a build-time post-processing step — neither of which is available at proc-macro expansion time. The `NonTerminal::href` field and the `<a href>` branch in `render_non_terminal` are kept in the library for downstream consumers building trees by hand, but the macro path never sets a `Some` href.

### PhantomData and keyword:: — Discovered at the Use Site, Not the Design Site

The design doc did not mention `PhantomData<keyword::X>` at all. The pg-sql codebase uses `PhantomData<keyword::Drop>` as a "parse-and-discard" marker for SQL keywords. The first smoke test (commit `18113a6`) revealed that `DropTableStmt` rendered as six identical "PhantomData" boxes.

The fix in commit `9bf3cd6` extended `recognize()` to detect `PhantomData<T>` and emit a `Terminal` with the inner type's last-segment ident uppercased. A follow-up in commit `0fec8b0` handled the companion case: enum variants holding `keyword::Left` directly (without the `PhantomData` wrapper) were also rendering as variant names rather than SQL tokens.

The plan's Task 20 had a different proposed fix: extend the `recursa::keywords!` macro to inject `#[railroad(label)]` on every keyword type. That would have required `recursa-core` to gain a dependency on `recursa-diagram`, a cross-crate coupling we correctly avoided. The `recognize()`-site heuristic achieves the same outcome with zero coupling — the macro infers from naming convention (`keyword::X` → upper-cased terminal) rather than requiring the keyword types to opt in.

**Lesson:** Heuristics based on naming convention can be cleaner than attribute-based opt-in when the convention is already load-bearing (pg-sql's `keyword::` path segment is part of the established crate architecture). Check whether a pattern already exists before designing a new annotation protocol.

---

## Lessons from the Process

### Review-Then-Polish Between Batches Compounded Quality

Each batch was reviewed by the code-reviewer subagent, and a polish commit addressed non-blocking findings before the next batch started. The commit history shows six alternating `refactor: batch N review polish` commits. This loop caught real bugs (the Optional geometry invariant below) and drove improvements (named layout constants, pinned geometry values in tests, baseline invariant tests) that compounded through the project.

**The key insight:** polish commits before starting the next batch prevent findings from becoming technical debt. A finding that sits unremediated across multiple batches is harder to fix because more code has been built on top of it.

### Subset-Before-Sweep in Phase 5

The plan originally had a single "apply `#[railroad]` across all AST types" task. We split it into Task 21a (10 representative types) and Task 21b (the full sweep). The 10-type probe in 21a surfaced three findings:

1. Variant-level keyword rendering was wrong (the `keyword::` heuristic, fixed before the sweep)
2. Mega-enums (`Statement`, `Expr`) overflow any reasonable diagram width — became a skip policy
3. Pratt enums don't render usefully as flat Choice — became a skip policy with a doc note

If we had swept all 274 types first, findings 2 and 3 would have become 274 bad diagrams that required a second full sweep to fix or manually annotate. Finding 1 would have produced 274 diagrams with wrong keyword labels.

**Lesson:** When applying a new annotation across a large codebase, sample the diversity first. Ten representative types plus a review is far cheaper than rolling back a 274-type sweep.

### Task 19 as an Isolated Probe

The highest-risk unknown in the entire project was whether rustdoc's HTML sanitizer (ammonia) would strip SVG elements from `#[doc = "<svg>..."]` attributes. If SVG was stripped, the entire embedding strategy was invalid. Task 19 was structured as a single annotated type to answer this binary question before committing to Phase 4's macro implementation.

The outcome: ammonia's default allowlist already includes SVG and all relevant child elements. The probe was a single commit (`18113a6`) that validated the full pipeline before any of the Phase 4 plumbing was built.

**Lesson:** When there is a single load-bearing unknown, structure a task specifically to answer it before building on the assumption. A one-type probe is much cheaper than discovering the strategy fails mid-sweep.

---

## Dead Ends and Bug Fixes

### Optional Geometry Bug (Finding B1)

The `Optional::new` constructor in the plan's code snippet used `CHOICE_RAIL_WIDTH` as the height increment for the skip branch. This violated the invariant `up + down == height` that all geometry composites must satisfy. The renderer uses `up` and `down` to place children relative to the entry/exit baseline; a violation here would misalign nested nodes.

The bug was caught by code review after Batch 2, not by the existing tests. The existing tests checked `opt.height() > 22` — a trivially-true bound that passed whether the geometry was correct or not.

The fix (commit `ca67be3`) corrected the height formula, introduced the `RETURN_RAIL_HEIGHT` constant to distinguish the back-edge height from the row-spacing gap, added an `assert_baseline_invariant` helper, and applied it across all composite geometry tests.

**Lesson:** Invariant tests ("does this property hold?") are weaker than pinned-value tests ("does this equal 47?"). An invariant like `up + down == height` is satisfied by construction if the implementation derives `down` as `height - up` — the test never actually fails. The stronger form is to pin concrete integer values and let the test break if geometry constants change. We applied this throughout the rest of the project.

### The href Formula That Never Worked

Three iterations:

1. Design doc: emit `TypeName.html` for same-module types, `../other_mod/TypeName.html` for cross-module types.
2. Phase 3 implementation: `NonTerminal::href = Some("TypeName.html")` by default, computed from the last path segment.
3. Phase 5 polish: drop href generation entirely; `recognize()` always emits `NonTerminal::new(name, None)`.

Each iteration was motivated by the proc-macro context limitation — the macro cannot see the calling module's path. The final state (no hrefs) is the correct state. We should have reached it sooner.

**Lesson:** When a design assumption requires runtime information that the implementation mechanism cannot access (module path at expansion time), the right answer is usually to drop the feature rather than approximate it. An absent feature is better than a present feature that silently produces wrong output.

---

## Things Worth Knowing Next Time

**For any Rust proc-macro project:**

- Design the crate structure around the proc-macro constraint before writing any code. Ask: "does the macro need to call library code at expansion time?" If yes, start with a 3-crate facade: `core` (lib), `macros` (proc-macro = true), `facade` (re-exports both).
- Proc-macro crates are effectively black boxes to normal Rust code. You cannot re-export non-proc-macro items from them. The facade pattern exists specifically to solve this.
- Use `unwrap_or_else(|e| e.to_compile_error())` at the proc-macro entry point. This converts internal errors into proper compile errors rather than ICEs (internal compiler errors from panics).

**For rustdoc-embedded visualizations:**

- Inline `#[doc = "<svg>...</svg>"]` works. Ammonia's default allowlist includes SVG elements.
- Use `currentColor` in SVG stylesheets rather than hardcoded hex values so diagrams adapt to rustdoc's light/dark themes.
- Intra-doc hrefs from inside an SVG cannot be resolved by the proc macro. The macro lacks the context to construct correct relative paths. If you need clickable cross-references, use rustdoc's `[TypeName]` intra-doc link syntax in the prose doc comment instead.
- `cargo doc -p pg-sql --no-deps` is the fast iteration loop for visual verification. The full `cargo doc` for the whole workspace is much slower.

**For recursa-derived crates applying `#[railroad]`:**

- `PhantomData<keyword::X>` fields and `keyword::X` enum variants are automatically recognized as terminals (uppercased last-segment ident). No annotation required.
- Mega-enums with many variants produce diagrams wider than any reasonable viewport. Skip them with `// #[railroad] not applied — too many variants` comments and note the skip policy in crate docs.
- Pratt enums (with `#[pratt]` or similar) render as flat `Choice` nodes with no precedence structure. This is correct behavior — add a doc comment explaining that precedence is not depicted.
- Apply `#[railroad]` to a 10-type sample first and review the output before doing a full sweep.

**For test quality in geometry-heavy code:**

- Pin exact integer geometry values, not invariant checks. `assert_eq!(seq.width(), 138)` is a better test than `assert!(seq.width() > 100)`.
- The `assert_baseline_invariant(node)` helper pattern — a function that checks `node.up() + node.down() == node.height()` — catches the whole class of geometry misalignment bugs that invariant tests miss.
- Snapshot tests (like `svg_snapshot.rs`) are valuable for catching SVG regression across refactors, but name them after what they actually test, not after a real-world type they only superficially resemble.

---

## What the Plan Got Right

- **Phased structure with clear batch boundaries.** Each phase had a clear deliverable and a natural review point. The alternating feat/polish pattern in the commit history reflects this working as designed.
- **Task 19 as an isolated ammonia probe.** The single highest-risk unknown got its own task before the work that depended on it. This is worth repeating in every project with a load-bearing unknown.
- **TDD discipline in every task.** Every task specified write-failing-test first, then implement, then verify. The geometry bug would have been worse without the test infrastructure.
- **Explicit YAGNI cuts.** The design doc documented what we were not building (CLI, feature flags, fully-inlined recursive expansion, precedence visualization). This prevented scope creep during implementation.

## What the Plan Got Wrong

- **Single-crate architecture.** The proc-macro constraint should have been worked through in the design doc. The two restructures consumed batch-review budget that could have gone to functional work.
- **Optional geometry code snippet had the B1 bug.** The plan's Task 6 code snippet used the wrong constant. A design doc that includes implementation code should have that code reviewed as carefully as the prose.
- **Task 17 was redundant.** Skip support was already implemented in Task 14. The plan didn't track the actual state of the implementation between tasks.
- **Task 18's `type_href` formula was conceptually flawed.** The plan specified relative href construction from the proc-macro context — information the proc macro cannot access. The feature should not have been included in the plan.
- **Task 20's literal step required cross-crate coupling.** "Extend `recursa::keywords!` to inject `#[railroad(label)]`" would have required `recursa-core` to depend on `recursa-diagram`, violating the intended one-way dependency. The implementing engineer improvised the naming-convention heuristic instead, which was the better solution.

---

## Deferred Items

These were confirmed in the Task 22 project-wide review and remain on the backlog:

| Item | File | Notes |
|---|---|---|
| NB-1: No end-to-end expand → doc-attr test | `recursa-diagram-macros/tests/` | Add a test that expands `#[railroad]` and asserts the result contains `#[doc = "\n\n<svg` |
| NB-2: Snapshot fixture decoupled from pg-sql | `recursa-diagram-core/tests/svg_snapshot.rs` | Rename fixture to `composite_layout_matches_snapshot`; remove `identifier.html` hrefs that the macro path can no longer produce |
| NB-3: No trybuild fixtures for error UX | `recursa-diagram-macros/tests/trybuild/` | Add `unknown_key.rs`, `wrong_lit_type.rs`, `union_unsupported.rs`, `empty_enum.rs` as compile_fail fixtures |
| NB-4: Dead `<a href>` branch in svg.rs | `recursa-diagram-core/src/svg.rs:87` | Add comment explaining the branch is reserved for downstream consumers; macro never produces `Some(href)` |
| NB-5: Soft-cap overflow for oversized children | `recursa-diagram-macros/src/macro_impl.rs:18` | Clarify the wrap-cap comment to mention single-oversized-child can exceed 1200px |
| Mega-enum rendering (`Statement`, `Expr`) | `pg-sql/src/ast/` | Needs a different visualization strategy — horizontal scrolling, collapsed view, or level-of-detail |
| Pratt-enum rendering | `pg-sql/src/ast/` | Flat `Choice` is not meaningful for Pratt; consider a dedicated Pratt node type in a future version |
| `fill:none` on rect visual | `recursa-diagram-core/src/svg.rs:29` | Cosmetic; verify browser rendering before changing |
| Wrap-layout left-margin asymmetry | `recursa-diagram-core/src/svg.rs` | Return-rail rows align to left edge; cosmetic, intentional for rail-geometry simplicity |
| Pre-existing pg-sql clippy/fmt red | `pg-sql/` | Separate cleanup pass, unrelated to diagram work |
| NB-6: Constructor doc coverage | `recursa-diagram-core/src/layout.rs` | Public `Terminal::new`, `NonTerminal::new`, etc. lack rustdoc comments |

---

## Summary

The project delivered exactly what the design specified — inline SVG railroad diagrams in rustdoc, applied to 274 pg-sql AST types — but required two architectural restructures and one feature drop (hrefs) to get there. The proc-macro crate constraint drove both restructures; future projects should design around it from the start. The review-then-polish loop between batches caught a real geometry bug early and produced a codebase with zero `#[allow]` attributes, zero panics on user paths, and 65 passing tests pinning concrete geometry values. The subset-before-sweep discipline in Phase 5 prevented three classes of bad diagrams from being applied at scale. The ammonia probe in Task 19 validated the entire embedding strategy cheaply before Phase 4 was built.
