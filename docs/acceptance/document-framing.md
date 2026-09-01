# Document-framing acceptance contract

This is the normative acceptance contract for construction-free document
framing from [issue #98](https://github.com/freshtonic/recursa/issues/98).
Framing composes the finalized token and classified-trivia records, closed
line and delimited lexical regions, and the generated grammar-aware recovery
plans; it never re-derives them and never changes strict parsing. No
PostgreSQL-specific directive or COPY policy is part of Recursa.

| Stable ID | Observable contract | Executable evidence | Expected result | Status |
| --- | --- | --- | --- | --- |
| R8F-FIXTURE | Mixed-document fixture breadth | `cargo test -p recursa-codegen --test generated_framing generated_framing_partitions_mixed_source_and_erases_every_island_after_failure --locked -- --exact` | One generated fixture mixes complete typed entries, an exact physical-line directive, a COPY-like payload through a standalone delimiter, blank rows, a raw protocol region, a recoverable failed island, and a later valid island. | Green |
| R8F-OWNERSHIP | Every byte owned exactly once | same evidence as R8F-FIXTURE | Beginning, inter-item, end, empty, trivia-only, and comment-only source is owned through ordered checked spans; `assert_owned_once` proves each part starts where the previous part ended and ends at EOF. | Green |
| R8F-EXACT-BYTES | Byte-for-byte reproduction | same evidence as R8F-FIXTURE | Concatenating owned regions reproduces the input exactly, including CRLF line endings and a missing final newline; no newline or text is synthesized, and `render_exact` equals the input for complete and recovered partitions. | Green |
| R8F-STRICT-ONLY | Authored values only in successful islands | `cargo test -p recursa-codegen --test generated_framing recovered_and_raw_framing_surfaces_have_no_authored_conversion_or_pretty --locked -- --exact` | Failed, raw, and payload regions have no conversion to authored values and no Pretty projection; the boundaries are compile-fail probes, not runtime refusals. | Green |
| R8F-SYNC | Synchronization reuses frozen recovery and atomic regions | same evidence as R8F-FIXTURE, plus `cargo test -p recursa-codegen --test generated_framing struct_framing_uses_generated_restart_evidence_for_an_unassignable_suffix --locked -- --exact` | Boundary spellings inside strings, comments, nested syntax, trivia, and payloads never split an island; a delimiter is a framing boundary only when generated recovery leaves it unconsumed as root FOLLOW, and struct, enum, and Pratt roots contribute their generated restart FIRST evidence. | Green |
| R8F-PAYLOAD | Missing payload termination | same evidence as R8F-FIXTURE | Payload mode opens only after strict parsing commits the configured variant; a missing standalone terminator retains the complete suffix as payload data and reports the recoverable `RCA5001` framing diagnostic naming the missing control marker. | Green |
| R8F-FAIL-CLOSED | Ambiguous ownership and fatal invariants | same evidence as R8F-FIXTURE | Unassignable ownership fails closed with the `RCA5002` diagnostic, retains the exact unresolved suffix, and keeps hard bounded progress on every replayed island; invariant violations select `FrameError` rather than recoverable syntax. | Green |
| R8F-RESOLUTION | Declaration identity resolution | `cargo test -p recursa-codegen --test generated_framing framing_references_follow_normal_import_alias_resolution --locked -- --exact && cargo test -p recursa-codegen --test generated_framing framing_generation_rejects_an_unreachable_payload_variant --locked -- --exact` | Island and payload references resolve through the ordinary typed import/alias pipeline, and an unreachable payload variant is rejected during generation with its exact source identity. | Green |

The framing declaration, transactional result, exact-ownership rules, and
payload contract are documented in
[`docs/document-framing.md`](../document-framing.md). Mixed recovery
documents never gain Pretty; only strict authored values render structurally.
