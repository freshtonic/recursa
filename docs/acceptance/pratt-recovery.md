# Pratt-recovery acceptance contract

This is the normative acceptance contract for construction-free Pratt
recovery from [issue #97](https://github.com/freshtonic/recursa/issues/97).
Recovery consumes the exact frozen null-denotation and left-denotation
selection, binding powers, associativity, and caller FOLLOW; it never
re-derives them and never changes strict Pratt parsing.

| Stable ID | Observable contract | Executable evidence | Expected result | Status |
| --- | --- | --- | --- | --- |
| R7P-FIXTURE | Expression fixture breadth | `cargo test -p recursa-codegen --test generated_pratt a_downstream_arithmetic_grammar_parses_the_frozen_pratt_plan --locked -- --exact` | One generated fixture covers atoms, prefix, postfix, infix, associativity, and qualifying extenders at multiple minimum binding powers, and its strict behavior is unchanged. | Green |
| R7P-MISSING | Missing prefix and infix right operands | `cargo test -p recursa-codegen --test generated_pratt pratt_explorer_recovers_committed_infix_without_constructing_an_operand --locked -- --exact` | `a +`, `-`, `NEVER`, and `a HAS` retain committed recovery structure with zero-width missing markers and construct no authored value. | Green |
| R7P-RESTART | Skip to a valid null-denotation restart | same evidence as R7P-MISSING | `! 2` and `é 2` retain exactly one skipped run with its exact UTF-8 span and a skipped recovery action before the restarted atom. | Green |
| R7P-COMMIT | Permanent commitment only after unique frozen dispatch | same evidence as R7P-MISSING, plus `cargo test -p recursa-codegen --test generated_pratt pratt_dispatch_regressions_share_one_downstream_fixture --locked -- --exact` | A variant exists in the recovery document only after the frozen dispatch trie leaves exactly one declaration reachable; an exact deeper edge outranks a shorter recovery commitment. | Green |
| R7P-AMBIGUITY | Extender and caller-FOLLOW overlap | same evidence as R7P-MISSING | A token shared by a qualifying extender and caller FOLLOW keeps the strict Pratt ambiguity diagnostic, stays unconsumed for the caller, and is never resolved by convention. | Green |
| R7P-R7-INVARIANTS | UTF-8 spans, nesting, ordering, end of input | same evidence as R7P-MISSING | Spans are checked UTF-8 ranges, nested expression failures recover inside grouping, diagnostic order is deterministic, and end-of-input matches strict evidence. | Green |
| R7P-PROGRESS | Bounded work | same evidence as R7P-MISSING | Every recovery document's progress trace proves no repeated zero-consumption Pratt state and total work within the finite bound. | Green |

Root-wide availability is unchanged for the two frozen products the recovery
slice still cannot interpret: an enum selected through a balanced-delimiter
dispatch cut, and an optional container refined by a sparse consume-vs-skip
viability decision. A Pratt selector that contains a balanced cut keeps the
same exclusion. Delimiter-scanning fallbacks for Pratt-owned failures are a
separate contraction step and are not removed by this contract.
