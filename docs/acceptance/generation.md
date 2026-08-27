# M4 acceptance contract

This matrix is the normative functional-evidence catalog for M4. A `Green`
row names executable evidence that exists on the checked-in source. A
`Pending` row is an explicit dependency boundary: it is valid as a legacy-case
ledger destination, but it is not complete evidence and cannot contribute to
the green total.

The legacy-case inventory was extracted from `recursa-spike` revision
`b410d2f123cd7510a8a40e9e5b971936a2fd8463`. The immutable identities are in
[`legacy-identities.tsv`](legacy-identities.tsv); their successor mappings are
in [`legacy-case-ledger.tsv`](legacy-case-ledger.tsv).

## Matrix

| Stable ID | Scope | Executable evidence | Expected result | Status |
| --- | --- | --- | --- | --- |
| M4-LEDGER | All 167 frozen legacy identities | `cargo test -p recursa-codegen --lib --test legacy_case_ledger --test analysis_parity --test analysis_diagnostics --test analysis_conflicts --test analysis_pratt_conflicts --locked` | The frozen inventory and evidence ledger have identical unique identity sets, exact category and grammar totals, no dangling reference, and every direct successor test executes. | Green |
| M4-SIX-PROJECTIONS | Six representative grammars | `cargo test -p recursa-codegen source_analysis_tests::tests::canonical_projection_matches --locked` | The six reviewed source-independent semantic projections remain unchanged. | Green |
| M4-FROZEN-PARSE-PLANS | Retired public profile, peek, and caller-FOLLOW assertions | `cargo test -p recursa-codegen --test analysis_decisions --test analysis_follow --locked` | Source-independent decisions freeze dispatch and FOLLOW behavior without restoring retired public parser internals. | Green |
| M4-GENERATED-CONTAINERS | Removed combinator API | `cargo test -p recursa-codegen --test generated_repetitions --locked` | Generated Option, Box, Vec, Vec1, separator, delimiter, and cardinality behavior supersedes manual combinators. | Green |
| M4-DIAGRAM-DECL-LOCAL | Removed diagram depth, SVG, lookup, and path APIs | `cargo test -p recursa-codegen --test generated_compound_nodes diagrams_are_published_in_aggregate_and_declaration_local_rustdoc --locked` | Frozen grammar plans produce aggregate and declaration-local documentation without restoring public filesystem or SVG contracts. | Green |
| M4-PRATT-DIAGRAM | Pratt and auxiliary diagram structure | `cargo test -p recursa-codegen --test generated_pratt pratt_diagrams_render_frozen_roles_binding_powers_and_operands --locked` | Pratt roles, binding powers, recursive operands, and grouping syntax render from the frozen plan. | Green |
| M4-LEGACY-RUNTIME-TOY | Four toy runtime cases | `cargo test -p recursa-codegen --test generated_legacy_grammars all_six_frozen_legacy_grammar_suites_pass --locked` | Explicit EOF, rejection, AST/presence, and lexical behavior have named executable successor cases. | Green |
| M4-LEGACY-RUNTIME-ARITH | Sixteen arithmetic runtime cases | `cargo test -p recursa-codegen --test generated_legacy_grammars all_six_frozen_legacy_grammar_suites_pass --locked` | Pratt AST shape, precedence, associativity, errors, exact token text, lexing, and explicit EOF have named executable successor cases. | Green |
| M4-LEGACY-RUNTIME-JSON | Twenty-four JSON runtime cases | `cargo test -p recursa-codegen --test generated_legacy_grammars all_six_frozen_legacy_grammar_suites_pass --locked` | JSON AST shape, accepted and rejected lexical forms, exact token text, and explicit EOF have named executable successor cases. | Green |
| M4-LEGACY-RUNTIME-SQL | Eighteen SQL runtime cases | `cargo test -p recursa-codegen --test generated_legacy_grammars all_six_frozen_legacy_grammar_suites_pass --locked` | All twelve statement variants, errors, exact content, lexing, and explicit EOF have named executable successor cases. | Green |
| M4-LEGACY-RUNTIME-OPTQ | Nine optional-query runtime cases | `cargo test -p recursa-codegen --test generated_legacy_grammars all_six_frozen_legacy_grammar_suites_pass --locked` | Qualified and unqualified LL(2) paths, failures, content, and explicit EOF have named executable successor cases. | Green |
| M4-LEGACY-RUNTIME-CSKW | Ten contextual-keyword runtime cases | `cargo test -p recursa-codegen --test generated_legacy_grammars all_six_frozen_legacy_grammar_suites_pass --locked` | Contextual keyword admission and rejection, case policy, content, and explicit EOF have named executable successor cases. | Green |
| M4-PRETTY-ROUNDTRIP | Six representative Pretty goldens | `cargo test -p recursa-codegen --test generated_pretty_goldens --locked` | Explicit-EOF parse/format/parse preserves semantic meaning and repeated formatting is idempotent for all six grammars. | Green |
| M4-EXPLORER | Six representative opaque adapters | `cargo test -p recursa-codegen --test generated_compound_nodes explorer_adapters_expose_erased_schema_occurrences_lexing_pretty_and_samples --locked && cargo test -p recursa-codegen --test generated_generic_applications explorer_adapters_are_generated_for_each_closed_generic_application --locked` | Visit-backed explorer schema, occurrence, lexical, Pretty, and sample behavior is exercised without concrete AST reflection. | Green |
| M4-DEBUG-BUNDLE | Three removed generated-source snapshots | `cargo test -p recursa-codegen --test generated_one_node debug_emission_preserves_compiled_semantics_for_every_capability_combination --locked` | Deterministic formatted debug bundles replace checked-in generated-source paths while remaining non-authoritative. | Green |
| M4-FULL-FUNCTIONAL-MATRIX | M3 and M4 combined evidence | `cargo test -p recursa-codegen --test generation_functional_matrix --locked every_green_generation_evidence_command_executes_without_pending_rows -- --ignored --exact` | This explicit manual aggregate reruns every Green successor command; it is ignored by default because those functional tests already run independently. | Green |
