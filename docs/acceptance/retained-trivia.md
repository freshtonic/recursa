# Retained-trivia acceptance contract

This is the normative acceptance contract for classified source trivia from
[issue #93](https://github.com/freshtonic/recursa/issues/93). Retention is an
opt-in lexical capability: named `trivia` rules preserve exact source records
outside the significant-token stream, while existing `ignore` rules remain
non-retaining.

| Stable ID | Observable contract | Executable evidence | Expected result | Status |
| --- | --- | --- | --- | --- |
| R4-GAPS | Beginning, inter-record, and end gaps | `cargo test -p recursa-codegen --test generated_lexing a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text --locked -- --exact` | A generated grammar exposes exact ordered whitespace, line-comment, and nested block-comment records; empty and trivia-only inputs still expose one gap; alternating gaps and significant tokens reproduces the source exactly. | Green |
| R4-PARSE | Significant parsing and parsed provenance | `cargo test -p recursa-codegen --test generated_lexing a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text --locked -- --exact` | Retained trivia never enters the parser cursor or changes dispatch, and a complete parsed root exposes the same immutable document gaps as its lexing result. | Green |
| R4-ERRORS | Rejected lexical ranges | `cargo test -p recursa-codegen --test generated_lexing a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text --locked -- --exact` | Lexical failures remain error records with their exact range and stable diagnostic code rather than becoming trivia. | Green |
| R4-ALTERNATIVE | Alternative-lexer validation | `cargo test -p recursa-codegen --test generated_lexing --locked` | Typed alternative trivia receives the same source-boundary, UTF-8, global-order, and non-overlap validation and exposes the same gap behavior without rescanning unrecorded bytes. | Green |
| R4-OPT-OUT | Non-retaining allocation contract | `cargo test -p recursa-core lex::tests::non_retaining_grammar_keeps_no_trivia_sidecar_or_gap_projection --all-features --locked -- --exact` | A grammar containing only non-retaining rules creates no trivia sidecar or gap projection. | Green |

Trivia classification is grammar-local and deterministic. The generated lexer
and `LexBuilder` share one record model, and `Input` owns a separate
significant-record cursor, so trivia cannot participate in FIRST, dispatch, or
token consumption. Parsed values retain the document sidecar only when the
grammar opted in; consuming `Parsed` with `into_ast` remains the explicit lossy
operation.
