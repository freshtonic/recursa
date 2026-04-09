# Code Review - 2026-04-08

## Status: APPROVED WITH NON-BLOCKING SUGGESTIONS


## Test Results
- Status: PASS
- Details: 131 tests across workspace all passed, including 9 new seq_parse integration tests and 3 new unit tests in recursa-core.


## Check Results
- Status: PASS
- Details: cargo clippy --all-targets -- -D warnings: clean, no warnings.


## Next Steps
- Address non-blocking suggestions below
- Proceed with Task 7 (NonEmpty variants) and Task 8 (re-exports)


## BLOCKING (Must Fix Before Merge)

None


## NON-BLOCKING (May Be Deferred)

**Significant code duplication across three Parse impls:**
- Description: The three Parse impls (NoTrailing, OptionalTrailing, RequiredTrailing) share ~80% identical code: the same `type Rules`, `IS_TERMINAL`, `first_pattern`, `peek`, and identical element/separator parsing boilerplate. Only the loop body differs slightly.
- Location: `recursa-core/src/seq.rs:105-277`
- Action: Extract a shared helper function for the common parse preamble (consume_ignored, peek first element, parse element, consume_ignored, parse separator). Each impl would call the helper with a closure or strategy for the loop termination logic. Alternatively, a private `parse_seq_inner` function parameterized by trailing behavior. This is deferrable since Task 7 (NonEmpty) will add three more impls -- addressing duplication before that avoids six-way duplication.

**`empty()` only available for `NoTrailing` variant:**
- Description: `Seq::empty()` is only implemented for `Seq<T, S, R, NoTrailing, AllowEmpty>`. But `OptionalTrailing` and `RequiredTrailing` with `AllowEmpty` can also be semantically empty (as proven by the tests). Users constructing Seq values manually cannot create empty `OptionalTrailing` or `RequiredTrailing` sequences.
- Location: `recursa-core/src/seq.rs:75-84`
- Action: Either move `empty()` to the general `AllowEmpty` impl (for all Trailing variants), or add `empty()` to each `AllowEmpty` trailing variant. The parse impls already construct empty Seqs via `Self::from_pairs(vec![])`, so this works.

**`consume_ignored` at top of `parse` is redundant with derive:**
- Description: Each Parse impl calls `input.consume_ignored()` before the first peek. However, the derive macro already calls `fork.consume_ignored()` before rebinding and parsing each field. This means whitespace is consumed twice at the boundary. While harmless (consuming no-op on already-consumed whitespace), it's misleading and could mask issues where Seq is used without the derive.
- Location: `recursa-core/src/seq.rs:127-128`, `:184-185`, `:247-248`
- Action: Document this intentional double-consume in a comment, or remove it and rely on the derive. Keeping it is safer for standalone usage of Seq::parse.

**Plan divergence: added `R` type parameter not in original design:**
- Description: The implementation adds a rules type parameter `R` to `Seq` that was not in the design doc or implementation plan. While this is a correct solution to the whitespace handling problem (the plan's approach of using `T::Rules` doesn't work when `T` is a Scan type), this design change should be documented.
- Location: `recursa-core/src/seq.rs:42`
- Action: Update the design doc `docs/plans/2026-04-09-container-types-design.md` to reflect the `R` parameter, or add a note explaining why the plan was deviated from. The doc comment on the struct is good but the design doc is now stale.


## Highlights

**Clean type-level configuration pattern:**
- What: The marker types (NoTrailing, OptionalTrailing, RequiredTrailing, AllowEmpty, NonEmpty) with default type parameters provide excellent ergonomics while maintaining type safety. The common case `Seq<Ident, Comma, WsRules>` reads cleanly.
- Location: `recursa-core/src/seq.rs:14-26`, `:42`

**Well-structured integration tests:**
- What: Each trailing variant has its own wrapper struct (ArgList, ArrayLit, StmtBlock) that tests Seq within a realistic grammar context, not in isolation. Tests cover happy path, empty, and error cases. The RequiredTrailing error test (`seq_required_trailing_error_on_missing_sep`) is a particularly good behavioral test.
- Location: `recursa-derive/tests/seq_parse.rs`

**Correct rebind pattern for element vs separator parsing:**
- What: The careful rebinding to `T::Rules` for elements and `NoRules` for separators correctly handles the type-system constraints where Scan types parse in NoRules context while composite types parse in their own Rules context.
- Location: `recursa-core/src/seq.rs:134-151`

**Good TDD discipline:**
- What: Each task followed test-first workflow with separate commits. Four atomic commits with clear scope.
