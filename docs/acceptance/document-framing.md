# Document-framing acceptance contract

This is the normative acceptance contract for strict document framing from
[issue #98](https://github.com/freshtonic/recursa/issues/98), as revised by
[ADR 0003](../adr/0003-strict-only-explorer-and-framing.md). Framing composes
the finalized token and classified-trivia records, closed line and delimited
lexical regions, and strict parsing; it never re-derives them and never changes
strict parsing. No PostgreSQL-specific directive or COPY policy is part of
Recursa.

| Stable ID | Observable contract | Executable evidence | Expected result | Status |
| --- | --- | --- | --- | --- |
| R8F-FIXTURE | Mixed-document fixture breadth | `cargo test -p recursa-codegen --test generated_framing generated_framing_partitions_mixed_source_and_rejects_the_first_failing_island --locked -- --exact` | One generated fixture mixes complete typed entries, an exact physical-line directive, a COPY-like payload through a standalone delimiter, blank rows, and a raw protocol region; a second document with a failed island is rejected at that island. | Green |
| R8F-OWNERSHIP | Every byte owned exactly once | same evidence as R8F-FIXTURE | Beginning, inter-item, end, empty, trivia-only, and comment-only source is owned through ordered checked spans; `assert_owned_once` proves each part starts where the previous part ended and ends at EOF. | Green |
| R8F-EXACT-BYTES | Byte-for-byte reproduction | same evidence as R8F-FIXTURE | Concatenating owned regions reproduces the input exactly, including CRLF line endings and a missing final newline; no newline or text is synthesized, and `render_exact` equals the input. | Green |
| R8F-STRICT-ONLY | Authored values only in complete frames | `cargo test -p recursa-codegen --test generated_framing rejected_and_raw_framing_surfaces_have_no_authored_conversion_or_pretty --locked -- --exact` | Rejections, raw regions, and payload regions have no conversion to authored values and no Pretty projection; the boundaries are compile-fail probes, not runtime refusals. | Green |
| R8F-SYNC | Boundaries are strict parse results | same evidence as R8F-FIXTURE, plus `cargo test -p recursa-codegen --test generated_framing struct_framing_rejects_an_unassignable_suffix_with_its_strict_diagnostic --locked -- --exact` | Boundary spellings inside strings, comments, nested syntax, trivia, and payloads never split an island; a delimiter is a framing boundary only when the island parses strictly up to that exact token, and struct and enum roots share the same rule. | Green |
| R8F-PAYLOAD | Missing payload termination | same evidence as R8F-FIXTURE | Payload mode opens only after strict parsing commits the configured variant; a missing standalone terminator rejects the document with the `RCA5001` framing cause at end of input and the unterminated suffix as the rejected extent. | Green |
| R8F-FAIL-CLOSED | Missing boundaries and fatal invariants | same evidence as R8F-FIXTURE | A complete island followed by significant source without its boundary is rejected with the `RCA5002` framing cause and the same strict diagnostic that Explorer parsing reports for that source; invariant violations select `FrameError` rather than a rejection. | Green |
| R8F-FINAL-ISLAND | Strict final island without its boundary | same evidence as R8F-FIXTURE | A final island whose strict parse consumes every significant token to end of input frames complete and keeps its authored value, with only an end-of-file trivia gap after it; a strict failure, an earlier island failure, or unconsumed significant source rejects the document, and exact byte ownership holds for every complete frame. | Green |
| R8F-REJECTION-EXTENT | Rejected extent is statement sized | same evidence as R8F-FIXTURE | A syntax failure reports the extent from the island start to the first candidate boundary at or after the failure, or to end of input when none follows; the strict diagnostic keeps its exact location. | Green |
| R8F-RESOLUTION | Declaration identity resolution | `cargo test -p recursa-codegen --test generated_framing framing_references_follow_normal_import_alias_resolution --locked -- --exact && cargo test -p recursa-codegen --test generated_framing framing_generation_rejects_an_unreachable_payload_variant --locked -- --exact` | Island and payload references resolve through the ordinary typed import/alias pipeline, and an unreachable payload variant is rejected during generation with its exact source identity. | Green |
| R8F-PARSER | Framing and Explorer use the Recursa parser | `cargo test -p recursa-codegen --test generated_framing --features explorer framing_and_explorer_run_through_the_recursa_parser --locked -- --exact` | The sole LR parser frames mixed documents and parses them through Explorer. | Green |

The framing declaration, transactional result, exact-ownership rules, and
payload contract are documented in
[`docs/document-framing.md`](../document-framing.md). Only strict authored
values render structurally.
