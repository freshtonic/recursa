use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::{
    BuildReport, DiagnosticEvidence, DiagnosticLocation, DiagnosticRemedy, DiagnosticStage,
    GenerateError, RecursaDiagnostic, RecursaDiagnosticCode, Severity,
};

fn write_fixture(source: &str) -> tempfile::TempDir {
    let package = tempfile::tempdir().expect("temporary source-analysis fixture");
    std::fs::write(package.path().join("grammar.rs"), source)
        .expect("write source-analysis fixture");
    package
}

fn diagnostic(
    stage: DiagnosticStage,
    code: &'static str,
    severity: Severity,
    path: &str,
    range: Range<u32>,
    summary: &str,
) -> RecursaDiagnostic {
    let evidence = if stage == DiagnosticStage::Analysis {
        DiagnosticEvidence::Analysis {
            overlap: Vec::new(),
            witness: Vec::new(),
            cycle: Vec::new(),
            configured_lookahead: None,
            sufficient_lookahead: None,
            end_of_alternative: false,
        }
    } else {
        DiagnosticEvidence::SourceFacts(vec![crate::DiagnosticFact {
            name: "subject".to_owned(),
            value: summary.to_owned(),
        }])
    };
    RecursaDiagnostic::new(
        stage,
        RecursaDiagnosticCode::new(code),
        severity,
        summary,
        DiagnosticLocation::new(PathBuf::from(path), Some(range))
            .expect("test location is manifest-relative and ordered"),
        evidence,
        vec![DiagnosticRemedy::CorrectSource],
    )
}

struct ProducerEvidence {
    codes: &'static [&'static str],
    producer_source: &'static str,
    test_source: &'static str,
    test_name: &'static str,
    command: &'static str,
}

/// Executable evidence for stable failures in the matching-version runtime protocol.
///
/// These codes are not build diagnostics, so this ledger is deliberately separate from the build
/// diagnostic registry and its producer ledger.
struct RuntimeDiagnosticEvidence {
    /// Identifies the runtime protocol failures exercised by this evidence row.
    codes: &'static [&'static str],
    /// Points to the runtime branch that creates each classified failure.
    producer_source: &'static str,
    /// Points to the function that executes and checks the producer outcome.
    assertion_source: &'static str,
    /// Selects that asserting function within its source file.
    assertion_name: &'static str,
    /// Points to the Cargo-visible test that runs the assertion source.
    command_test_source: &'static str,
    /// Selects the Cargo-visible test used by the executable command.
    command_test_name: &'static str,
    /// Records the exact focused command maintainers can rerun for this evidence.
    command: &'static str,
}

/// Complete stable-code contract for failures produced while lexing user input.
const RUNTIME_LEXICAL_CODES: &[&str] = &["RCA4000", "RCA4001", "RCA4002", "RCA4004"];

/// Maps every runtime lexical failure code to executable downstream evidence.
const RUNTIME_LEXICAL_EVIDENCE: &[RuntimeDiagnosticEvidence] = &[RuntimeDiagnosticEvidence {
    codes: &["RCA4000", "RCA4001", "RCA4002", "RCA4004"],
    producer_source: "../recursa-core/src/lex.rs",
    assertion_source: "tests/fixtures/generation/lexing/src/main.rs",
    assertion_name: "main",
    command_test_source: "tests/generated_lexing.rs",
    command_test_name: "a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text",
    command: "cargo test -p recursa-codegen --test generated_lexing a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text --locked",
}];

/// Complete append-only code contract for failures produced while parsing user input.
const RUNTIME_PARSE_CODES: &[&str] = &["RCA4100", "RCA4101", "RCA4102", "RCA4103", "RCA4104"];

/// Maps every runtime parse failure code to executable public or core-boundary evidence.
const RUNTIME_PARSE_EVIDENCE: &[RuntimeDiagnosticEvidence] = &[
    RuntimeDiagnosticEvidence {
        codes: &["RCA4100", "RCA4101"],
        producer_source: "../recursa-core/src/parse.rs",
        assertion_source: "tests/fixtures/generation/one-node/src/main.rs",
        assertion_name: "main",
        command_test_source: "tests/generated_one_node.rs",
        command_test_name: "a_downstream_crate_loads_and_parses_one_generated_node_through_the_facade",
        command: "cargo test -p recursa-codegen --test generated_one_node a_downstream_crate_loads_and_parses_one_generated_node_through_the_facade --locked",
    },
    RuntimeDiagnosticEvidence {
        codes: &["RCA4102"],
        producer_source: "../recursa-core/src/parse.rs",
        assertion_source: "../recursa-core/src/parsed.rs",
        assertion_name: "pratt_ambiguities_expose_the_runtime_code_at_the_extender_anchor",
        command_test_source: "../recursa-core/src/parsed.rs",
        command_test_name: "pratt_ambiguities_expose_the_runtime_code_at_the_extender_anchor",
        command: "cargo test -p recursa-core parsed::tests::pratt_ambiguities_expose_the_runtime_code_at_the_extender_anchor --locked",
    },
    RuntimeDiagnosticEvidence {
        codes: &["RCA4103"],
        producer_source: "../recursa-core/src/parse.rs",
        assertion_source: "../recursa-core/src/parsed.rs",
        assertion_name: "invalid_generated_record_spans_are_structured_errors",
        command_test_source: "../recursa-core/src/parsed.rs",
        command_test_name: "invalid_generated_record_spans_are_structured_errors",
        command: "cargo test -p recursa-core parsed::tests::invalid_generated_record_spans_are_structured_errors --locked",
    },
    RuntimeDiagnosticEvidence {
        codes: &["RCA4104"],
        producer_source: "../recursa-core/src/parse.rs",
        assertion_source: "../recursa-core/src/parsed.rs",
        assertion_name: "grammar_mismatches_expose_the_runtime_code_at_the_failure_anchor",
        command_test_source: "../recursa-core/src/parsed.rs",
        command_test_name: "grammar_mismatches_expose_the_runtime_code_at_the_failure_anchor",
        command: "cargo test -p recursa-core parsed::tests::grammar_mismatches_expose_the_runtime_code_at_the_failure_anchor --locked",
    },
];

macro_rules! phase_producer_evidence {
    ($code:literal, $module:literal, $test:literal) => {
        ProducerEvidence {
            codes: &[$code],
            producer_source: "src/source_analysis.rs",
            test_source: concat!("src/", $module, ".rs"),
            test_name: $test,
            command: concat!(
                "cargo test -p recursa-codegen ",
                $module,
                "::tests::",
                $test,
                " --locked"
            ),
        }
    };
}

const PRODUCER_EVIDENCE: &[ProducerEvidence] = &[
    ProducerEvidence {
        codes: &["RCA0001"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/normalized_analysis.rs",
        test_name: "unsupported_container_cardinality_is_rejected_before_analysis",
        command: "cargo test -p recursa-codegen --test normalized_analysis unsupported_container_cardinality_is_rejected_before_analysis --locked",
    },
    ProducerEvidence {
        codes: &["RCA0002"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_diagnostics.rs",
        test_name: "invalid_references_are_reported_before_dependent_semantics",
        command: "cargo test -p recursa-codegen --test analysis_diagnostics invalid_references_are_reported_before_dependent_semantics --locked",
    },
    ProducerEvidence {
        codes: &["RCA0100"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_diagnostics.rs",
        test_name: "mutual_left_recursion_has_one_deterministic_cycle_snapshot",
        command: "cargo test -p recursa-codegen --test analysis_diagnostics mutual_left_recursion_has_one_deterministic_cycle_snapshot --locked",
    },
    ProducerEvidence {
        codes: &["RCA0200"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_conflicts.rs",
        test_name: "singleton_enum_overlap_matches_the_spike_scalar_baseline",
        command: "cargo test -p recursa-codegen --test analysis_conflicts singleton_enum_overlap_matches_the_spike_scalar_baseline --locked",
    },
    ProducerEvidence {
        codes: &["RCA0201"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_conflicts.rs",
        test_name: "static_nullable_overlap_is_an_error_but_a_fully_nullable_tail_is_not",
        command: "cargo test -p recursa-codegen --test analysis_conflicts static_nullable_overlap_is_an_error_but_a_fully_nullable_tail_is_not --locked",
    },
    ProducerEvidence {
        codes: &["RCA0202"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_pratt_conflicts.rs",
        test_name: "pratt_atom_atom_conflict_has_a_canonical_snapshot",
        command: "cargo test -p recursa-codegen --test analysis_pratt_conflicts pratt_atom_atom_conflict_has_a_canonical_snapshot --locked",
    },
    ProducerEvidence {
        codes: &["RCA0203"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_pratt_conflicts.rs",
        test_name: "pratt_led_led_conflict_has_a_canonical_snapshot",
        command: "cargo test -p recursa-codegen --test analysis_pratt_conflicts pratt_led_led_conflict_has_a_canonical_snapshot --locked",
    },
    ProducerEvidence {
        codes: &["RCA0300"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_follow.rs",
        test_name: "nested_nullable_tail_warning_is_predicate_aware_and_keeps_output",
        command: "cargo test -p recursa-codegen --test analysis_follow nested_nullable_tail_warning_is_predicate_aware_and_keeps_output --locked",
    },
    ProducerEvidence {
        codes: &["RCA0301"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_follow.rs",
        test_name: "pratt_follow_warnings_filter_extenders_by_site_min_bp",
        command: "cargo test -p recursa-codegen --test analysis_follow pratt_follow_warnings_filter_extenders_by_site_min_bp --locked",
    },
    ProducerEvidence {
        codes: &["RCA0302"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_greedy.rs",
        test_name: "stale_greedy_acceptance_is_a_blocking_rca0302_error",
        command: "cargo test -p recursa-codegen --test analysis_greedy stale_greedy_acceptance_is_a_blocking_rca0302_error --locked",
    },
    ProducerEvidence {
        codes: &["RCA0303"],
        producer_source: "src/diagnostic.rs",
        test_source: "tests/analysis_greedy.rs",
        test_name: "stale_kinds_and_undeclared_kinds_are_reported_separately",
        command: "cargo test -p recursa-codegen --test analysis_greedy stale_kinds_and_undeclared_kinds_are_reported_separately --locked",
    },
    phase_producer_evidence!(
        "RCA1001",
        "discovery",
        "rca1001_reports_an_invalid_manifest_directory"
    ),
    phase_producer_evidence!(
        "RCA1002",
        "discovery",
        "rca1002_rejects_an_absolute_selected_root"
    ),
    phase_producer_evidence!(
        "RCA1003",
        "discovery",
        "rca1003_rejects_a_selected_root_that_escapes_the_package"
    ),
    phase_producer_evidence!(
        "RCA1004",
        "discovery",
        "rca1004_rejects_a_selected_root_symlink"
    ),
    phase_producer_evidence!(
        "RCA1005",
        "discovery",
        "rca1005_reports_a_missing_selected_root"
    ),
    phase_producer_evidence!(
        "RCA1006",
        "discovery",
        "rca1006_reports_an_unreadable_selected_root"
    ),
    phase_producer_evidence!(
        "RCA1007",
        "discovery",
        "rca1007_reports_a_missing_module_candidate"
    ),
    phase_producer_evidence!(
        "RCA1008",
        "discovery",
        "rca1008_reports_ambiguous_module_candidates"
    ),
    phase_producer_evidence!(
        "RCA1009",
        "discovery",
        "rca1009_rejects_a_malformed_module_path_attribute"
    ),
    phase_producer_evidence!(
        "RCA1010",
        "discovery",
        "rca1010_rejects_multiple_active_module_paths"
    ),
    phase_producer_evidence!(
        "RCA1011",
        "discovery",
        "rca1011_reports_a_module_ancestor_cycle"
    ),
    phase_producer_evidence!(
        "RCA1012",
        "discovery",
        "rca1012_rejects_an_unknown_cfg_predicate"
    ),
    phase_producer_evidence!(
        "RCA1013",
        "discovery",
        "rca1013_rejects_an_invocation_only_cfg_predicate"
    ),
    phase_producer_evidence!(
        "RCA1014",
        "discovery",
        "rca1014_rejects_a_malformed_cfg_predicate"
    ),
    phase_producer_evidence!(
        "RCA1015",
        "resolution",
        "rca1015_reports_a_missing_grammar_configuration"
    ),
    phase_producer_evidence!(
        "RCA1016",
        "resolution",
        "rca1016_reports_multiple_active_grammar_configurations"
    ),
    phase_producer_evidence!(
        "RCA1017",
        "resolution",
        "rca1017_reports_an_invalid_grammar_configuration"
    ),
    phase_producer_evidence!(
        "RCA2001",
        "discovery",
        "rca2001_reports_invalid_rust_syntax"
    ),
    phase_producer_evidence!(
        "RCA2002",
        "resolution",
        "rca2002_reports_invalid_item_configuration"
    ),
    phase_producer_evidence!(
        "RCA2003",
        "resolution",
        "rca2003_reports_duplicate_type_namespace_declarations"
    ),
    phase_producer_evidence!(
        "RCA2004",
        "resolution",
        "rca2004_reports_a_duplicate_explicit_import"
    ),
    phase_producer_evidence!(
        "RCA2005",
        "resolution",
        "rca2005_reports_an_import_collision"
    ),
    phase_producer_evidence!(
        "RCA2006",
        "resolution",
        "rca2006_reports_an_ambiguous_import_used_by_a_node"
    ),
    phase_producer_evidence!(
        "RCA2007",
        "resolution",
        "rca2007_reports_an_inaccessible_reexport"
    ),
    phase_producer_evidence!(
        "RCA2008",
        "resolution",
        "rca2008_reports_a_content_token_binding_collision"
    ),
    phase_producer_evidence!(
        "RCA2009",
        "resolution",
        "rca2009_reports_an_inaccessible_content_token_binding"
    ),
    phase_producer_evidence!(
        "RCA2010",
        "resolution",
        "rca2010_reports_a_visibility_outside_the_selected_tree"
    ),
    phase_producer_evidence!(
        "RCA2011",
        "resolution",
        "rca2011_reports_an_unsupported_local_declaration"
    ),
    phase_producer_evidence!(
        "RCA2012",
        "resolution",
        "rca2012_reports_an_unresolved_grammar_semantic_reference"
    ),
    phase_producer_evidence!(
        "RCA2013",
        "resolution",
        "rca2013_reports_an_inaccessible_local_declaration"
    ),
    phase_producer_evidence!(
        "RCA2014",
        "resolution",
        "rca2014_reports_a_reference_above_the_selected_root_anchor"
    ),
    phase_producer_evidence!(
        "RCA2015",
        "resolution",
        "rca2015_reports_a_crate_path_outside_the_selected_root_anchor"
    ),
    phase_producer_evidence!("RCA2016", "resolution", "rca2016_reports_an_external_path"),
    phase_producer_evidence!(
        "RCA2017",
        "resolution",
        "rca2017_reports_a_leading_external_path"
    ),
    ProducerEvidence {
        codes: &["RCA3001"],
        producer_source: "src/lexical.rs",
        test_source: "src/lexical.rs",
        test_name: "invalid_root_keyword_policy_emits_typed_rca3001",
        command: "cargo test -p recursa-codegen lexical::tests::invalid_root_keyword_policy_emits_typed_rca3001 --locked",
    },
    ProducerEvidence {
        codes: &["RCA3002"],
        producer_source: "src/lexical.rs",
        test_source: "src/lexical.rs",
        test_name: "central_fixed_section_mismatch_emits_typed_rca3002",
        command: "cargo test -p recursa-codegen lexical::tests::central_fixed_section_mismatch_emits_typed_rca3002 --locked",
    },
    ProducerEvidence {
        codes: &["RCA3003"],
        producer_source: "src/lexical.rs",
        test_source: "src/lexical.rs",
        test_name: "conflicting_content_metadata_emits_typed_rca3003_at_all_sites",
        command: "cargo test -p recursa-codegen lexical::tests::conflicting_content_metadata_emits_typed_rca3003_at_all_sites --locked",
    },
    ProducerEvidence {
        codes: &["RCA3004", "RCA3005", "RCA3006"],
        producer_source: "src/lexical.rs",
        test_source: "src/lexical.rs",
        test_name: "reports_unsupported_overlap_and_tie_diagnostics_with_sites_and_remedies",
        command: "cargo test -p recursa-codegen lexical::tests::reports_unsupported_overlap_and_tie_diagnostics_with_sites_and_remedies --locked",
    },
    ProducerEvidence {
        codes: &[
            "RCA3101", "RCA3102", "RCA3103", "RCA3104", "RCA3105", "RCA3106", "RCA3112", "RCA3113",
            "RCA3114", "RCA3115",
        ],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_model.rs",
        test_name: "rejects_every_invalid_positional_attachment_before_semantic_analysis",
        command: "cargo test -p recursa-codegen source_model::tests::rejects_every_invalid_positional_attachment_before_semantic_analysis --locked",
    },
    ProducerEvidence {
        codes: &["RCA3107", "RCA3108", "RCA3109", "RCA3116"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_model.rs",
        test_name: "rejects_unsupported_trait_recipes_delta_overlap_and_ordinary_derive_ownership",
        command: "cargo test -p recursa-codegen source_model::tests::rejects_unsupported_trait_recipes_delta_overlap_and_ordinary_derive_ownership --locked",
    },
    ProducerEvidence {
        codes: &["RCA3110", "RCA3111"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_model.rs",
        test_name: "rejects_content_display_and_misplacement_before_lexical_planning",
        command: "cargo test -p recursa-codegen source_model::tests::rejects_content_display_and_misplacement_before_lexical_planning --locked",
    },
    ProducerEvidence {
        codes: &["RCA3125", "RCA3126", "RCA3127", "RCA3128"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_analysis_contract_tests.rs",
        test_name: "obsolete_authored_hooks_fail_directly_at_the_source_analysis_seam",
        command: "cargo test -p recursa-codegen source_analysis_contract_tests::obsolete_authored_hooks_fail_directly_at_the_source_analysis_seam --locked",
    },
    ProducerEvidence {
        codes: &["RCA3129"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_analysis_tests.rs",
        test_name: "invalid_presence_and_optional_fixed_syntax_reports_source_analysis_diagnostics",
        command: "cargo test -p recursa-codegen source_analysis_tests::tests::invalid_presence_and_optional_fixed_syntax_reports_source_analysis_diagnostics --locked",
    },
    ProducerEvidence {
        codes: &["RCA3130"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_analysis_tests.rs",
        test_name: "invalid_pratt_extension_filter_reports_source_analysis_diagnostic",
        command: "cargo test -p recursa-codegen source_analysis_tests::tests::invalid_pratt_extension_filter_reports_source_analysis_diagnostic --locked",
    },
    ProducerEvidence {
        codes: &["RCA3131"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_analysis_tests.rs",
        test_name: "malformed_greedy_acceptances_report_source_analysis_diagnostics",
        command: "cargo test -p recursa-codegen source_analysis_tests::tests::malformed_greedy_acceptances_report_source_analysis_diagnostics --locked",
    },
    ProducerEvidence {
        codes: &["RCA3118", "RCA3210"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_model.rs",
        test_name: "pretty_hints_reject_unknown_duplicate_and_inapplicable_settings",
        command: "cargo test -p recursa-codegen source_model::tests::pretty_hints_reject_unknown_duplicate_and_inapplicable_settings --locked",
    },
    ProducerEvidence {
        codes: &["RCA3119"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_analysis_tests.rs",
        test_name: "duplicate_admission_warning_crosses_the_full_analysis_boundary",
        command: "cargo test -p recursa-codegen source_analysis_tests::tests::duplicate_admission_warning_crosses_the_full_analysis_boundary --locked",
    },
    ProducerEvidence {
        codes: &["RCA3120", "RCA3121", "RCA3122", "RCA3123", "RCA3124"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_model.rs",
        test_name: "invalid_admission_graphs_and_keyword_metadata_have_structured_diagnostics",
        command: "cargo test -p recursa-codegen source_model::tests::invalid_admission_graphs_and_keyword_metadata_have_structured_diagnostics --locked",
    },
    ProducerEvidence {
        codes: &["RCA3132"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_model.rs",
        test_name: "invalid_lookahead_filters_have_structured_diagnostics",
        command: "cargo test -p recursa-codegen source_model::tests::invalid_lookahead_filters_have_structured_diagnostics --locked",
    },
    ProducerEvidence {
        codes: &["RCA3211"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/source_model.rs",
        test_name: "pretty_hints_reject_missing_pretty_trait_empty_groups_and_tight_breaks",
        command: "cargo test -p recursa-codegen source_model::tests::pretty_hints_reject_missing_pretty_trait_empty_groups_and_tight_breaks --locked",
    },
    ProducerEvidence {
        codes: &[
            "RCA3202", "RCA3203", "RCA3204", "RCA3205", "RCA3206", "RCA3207", "RCA3208",
        ],
        producer_source: "src/source_analysis.rs",
        test_source: "src/materialization.rs",
        test_name: "rejects_open_unsupported_cyclic_and_invalid_closed_types_before_semantic_analysis",
        command: "cargo test -p recursa-codegen materialization::tests::rejects_open_unsupported_cyclic_and_invalid_closed_types_before_semantic_analysis --locked",
    },
    ProducerEvidence {
        codes: &["RCA3209"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/materialization.rs",
        test_name: "rejects_missing_unboxed_and_foreign_pratt_lhs_fields",
        command: "cargo test -p recursa-codegen materialization::tests::rejects_missing_unboxed_and_foreign_pratt_lhs_fields --locked",
    },
    ProducerEvidence {
        codes: &["RCA0400"],
        producer_source: "src/lr/report.rs",
        test_source: "src/lr.rs",
        test_name: "shift_reduce_conflicts_report_rca0400_with_items_lookahead_and_actions",
        command: "cargo test -p recursa-codegen lr::tests::shift_reduce_conflicts_report_rca0400_with_items_lookahead_and_actions --locked",
    },
    ProducerEvidence {
        codes: &["RCA0401", "RCA0405"],
        producer_source: "src/lr/report.rs",
        test_source: "src/lr.rs",
        test_name: "lalr_merge_artefacts_report_rca0401_with_an_rca0405_note",
        command: "cargo test -p recursa-codegen lr::tests::lalr_merge_artefacts_report_rca0401_with_an_rca0405_note --locked",
    },
    ProducerEvidence {
        codes: &["RCA0403"],
        producer_source: "src/lr/lowering.rs",
        test_source: "src/lr.rs",
        test_name: "text_predicates_are_rejected_with_rca0403",
        command: "cargo test -p recursa-codegen lr::tests::text_predicates_are_rejected_with_rca0403 --locked",
    },
    ProducerEvidence {
        codes: &["RCA9101", "RCA9102", "RCA9103", "RCA9104"],
        producer_source: "src/build_diagnostic.rs",
        test_source: "src/source_analysis_contract_tests.rs",
        test_name: "typed_registry_contracts_fail_closed_and_cargo_warnings_are_single_directives",
        command: "cargo test -p recursa-codegen source_analysis_contract_tests::typed_registry_contracts_fail_closed_and_cargo_warnings_are_single_directives --locked",
    },
    ProducerEvidence {
        codes: &["RCA9105"],
        producer_source: "src/source_analysis.rs",
        test_source: "src/generation/tests.rs",
        test_name: "emission_failures_use_the_registered_generation_code_and_preserve_detail",
        command: "cargo test -p recursa-codegen generation::tests::emission_failures_use_the_registered_generation_code_and_preserve_detail --locked",
    },
];

#[test]
fn every_registered_code_has_real_producer_evidence() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected = crate::build_diagnostic::diagnostic_code_registry()
        .iter()
        .filter(|spec| spec.status == crate::build_diagnostic::DiagnosticCodeStatus::Active)
        .map(|spec| spec.code.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut actual = std::collections::BTreeSet::new();

    for evidence in PRODUCER_EVIDENCE {
        let producer = std::fs::read_to_string(manifest.join(evidence.producer_source)).unwrap();
        let test_source = std::fs::read_to_string(manifest.join(evidence.test_source)).unwrap();
        let asserted_output = asserted_test_output(&test_source, evidence.test_name);
        assert_eq!(
            evidence.command,
            producer_test_command(evidence.test_source, evidence.test_name),
            "producer evidence must provide an exact executable command",
        );
        for code in evidence.codes {
            assert!(
                producer.contains(code),
                "{code} is absent from its cited producer"
            );
            assert!(
                asserted_output.contains(code),
                "{} does not explicitly assert its claimed {code} output",
                evidence.test_name,
            );
            assert!(
                actual.insert(*code),
                "duplicate producer evidence for {code}"
            );
        }
    }

    assert_eq!(
        actual, expected,
        "producer ledger must cover the registry exactly"
    );
}

#[test]
fn runtime_lexical_codes_have_public_producer_evidence() {
    assert_runtime_diagnostic_evidence(
        RUNTIME_LEXICAL_CODES,
        RUNTIME_LEXICAL_EVIDENCE,
        "runtime lexical",
    );
}

#[test]
fn runtime_parse_codes_have_executable_producer_evidence() {
    assert_runtime_diagnostic_evidence(
        RUNTIME_PARSE_CODES,
        RUNTIME_PARSE_EVIDENCE,
        "runtime parse",
    );
}

fn assert_runtime_diagnostic_evidence(
    expected_codes: &[&str],
    evidence_rows: &[RuntimeDiagnosticEvidence],
    contract_name: &str,
) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected = expected_codes
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut actual = std::collections::BTreeSet::new();

    for evidence in evidence_rows {
        let producer = std::fs::read_to_string(manifest.join(evidence.producer_source)).unwrap();
        let assertion_source =
            std::fs::read_to_string(manifest.join(evidence.assertion_source)).unwrap();
        let assertion_output =
            asserted_function_output(&assertion_source, evidence.assertion_name, false);
        let command_test_source =
            std::fs::read_to_string(manifest.join(evidence.command_test_source)).unwrap();
        let command_output = asserted_test_output(&command_test_source, evidence.command_test_name);
        assert_eq!(
            evidence.command,
            producer_test_command(evidence.command_test_source, evidence.command_test_name),
            "{contract_name} evidence must provide an exact executable command",
        );
        for code in evidence.codes {
            assert!(
                producer.contains(code),
                "{code} is absent from its cited runtime producer"
            );
            assert!(
                assertion_output.contains(code),
                "{} does not explicitly assert its claimed {code} output",
                evidence.assertion_name,
            );
            assert!(
                command_output.contains(code),
                "{} does not assert the commanded producer's {code} output",
                evidence.command_test_name,
            );
            assert!(
                actual.insert(*code),
                "duplicate runtime evidence for {code}"
            );
        }
    }

    assert_eq!(
        actual, expected,
        "{contract_name} evidence must cover its separate code contract exactly"
    );
}

fn asserted_test_output(source: &str, name: &str) -> String {
    asserted_function_output(source, name, true)
}

fn asserted_function_output(source: &str, name: &str, require_test: bool) -> String {
    struct AssertionVisitor {
        tokens: String,
    }

    impl<'ast> syn::visit::Visit<'ast> for AssertionVisitor {
        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            let is_assertion = node
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident.to_string().starts_with("assert"));
            if is_assertion {
                self.tokens.push_str(&format!("{:?}", node.tokens));
            }
            syn::visit::visit_macro(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
            let is_assertion_helper = match node.func.as_ref() {
                syn::Expr::Path(path) => path
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident.to_string().starts_with("assert_")),
                _ => false,
            };
            if is_assertion_helper {
                self.tokens.push_str(&format!("{:?}", node.args));
            }
            syn::visit::visit_expr_call(self, node);
        }
    }

    fn find(items: &[syn::Item], name: &str, require_test: bool) -> Option<String> {
        for item in items {
            match item {
                syn::Item::Fn(function) if function.sig.ident == name => {
                    if require_test {
                        assert!(
                            function
                                .attrs
                                .iter()
                                .any(|attribute| attribute.path().is_ident("test")),
                            "cited producer evidence {name} is not a test",
                        );
                    }
                    let mut visitor = AssertionVisitor {
                        tokens: String::new(),
                    };
                    syn::visit::Visit::visit_block(&mut visitor, &function.block);
                    return Some(visitor.tokens);
                }
                syn::Item::Mod(module) => {
                    if let Some((_, items)) = &module.content
                        && let Some(found) = find(items, name, require_test)
                    {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let file = syn::parse_file(source).expect("producer test source is valid Rust");
    find(&file.items, name, require_test)
        .unwrap_or_else(|| panic!("missing cited producer function {name}"))
}

fn producer_test_command(source: &str, test: &str) -> String {
    let stem = Path::new(source)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .expect("Rust test source has a stem");
    if source.starts_with("../recursa-core/src/") {
        format!("cargo test -p recursa-core {stem}::tests::{test} --locked")
    } else if source.starts_with("tests/") {
        format!("cargo test -p recursa-codegen --test {stem} {test} --locked")
    } else {
        let path = if source == "src/generation/tests.rs" {
            format!("generation::tests::{test}")
        } else if stem == "source_analysis_contract_tests" {
            format!("{stem}::{test}")
        } else {
            format!("{stem}::tests::{test}")
        };
        format!("cargo test -p recursa-codegen {path} --locked")
    }
}

#[test]
fn build_report_deduplicates_orders_and_renders_structured_diagnostics() {
    let late_warning = diagnostic(
        DiagnosticStage::Analysis,
        "RCA0300",
        Severity::Warning,
        "grammar/root.rs",
        24..31,
        "nullable use overlaps caller FOLLOW",
    );
    let early_error = diagnostic(
        DiagnosticStage::Resolution,
        "RCA2004",
        Severity::Error,
        "grammar/root.rs",
        8..12,
        "unresolved grammar-semantic reference",
    );
    let report = BuildReport::new(vec![late_warning.clone(), early_error.clone(), early_error]);

    assert_eq!(report.diagnostics().len(), 2);
    assert_eq!(report.diagnostics()[0].code().as_str(), "RCA2004");
    assert_eq!(report.errors().count(), 1);
    assert_eq!(report.warnings().count(), 1);
    assert_eq!(
        report.canonical_rendering(),
        concat!(
            "error[RCA2004] resolution at grammar/root.rs:8..12: unresolved grammar-semantic reference\n",
            "  evidence: subject=unresolved grammar-semantic reference\n",
            "  remedy: correct the authored grammar source\n",
            "warning[RCA0300] analysis at grammar/root.rs:24..31: nullable use overlaps caller FOLLOW\n",
            "  evidence: analysis overlap=[] witness=[] cycle=[] lookahead=None->None end=false\n",
            "  remedy: correct the authored grammar source\n",
        ),
    );

    let error = GenerateError::new(report);
    assert_eq!(error.to_string(), error.report().canonical_rendering());
    assert_eq!(format!("{error:?}"), error.to_string());
    let as_error: &dyn std::error::Error = &error;
    assert!(as_error.source().is_none());
}

#[test]
fn diagnostic_code_registry_is_unique_and_stage_partitioned() {
    let registry = crate::build_diagnostic::diagnostic_code_registry();
    let mut codes = registry
        .iter()
        .map(|spec| spec.code.as_str())
        .collect::<Vec<_>>();
    let original_len = codes.len();
    codes.sort_unstable();
    codes.dedup();

    assert_eq!(codes.len(), original_len);
    assert_eq!(
        registry
            .iter()
            .filter(|spec| {
                spec.status == crate::build_diagnostic::DiagnosticCodeStatus::Retired
            })
            .map(|spec| spec.code.as_str())
            .collect::<Vec<_>>(),
        ["RCA3117"],
        "retired diagnostic assignments remain reserved and cannot be reused",
    );
    assert!(
        registry.len() >= 70,
        "every current failure class is allocated"
    );
    for spec in registry {
        assert_eq!(spec.code.as_str().len(), 7);
        assert!(spec.code.as_str().starts_with("RCA"));
        let family = spec.code.as_str().as_bytes()[3];
        if spec.code.as_str().starts_with("RCA91") {
            assert_eq!(spec.severity, Severity::Error);
        } else {
            let expected = match spec.stage {
                DiagnosticStage::Analysis => b'0',
                DiagnosticStage::Discovery => b'1',
                DiagnosticStage::Resolution => b'2',
                DiagnosticStage::Lowering => b'3',
                DiagnosticStage::Emission => b'4',
                DiagnosticStage::Loading => b'9',
            };
            assert_eq!(
                family,
                expected,
                "{} has the wrong family",
                spec.code.as_str()
            );
        }
        assert!(matches!(
            spec.primary_location,
            crate::build_diagnostic::PrimaryLocationRule::FirstRelatedOrGlobal
                | crate::build_diagnostic::PrimaryLocationRule::SelectedRootFallback
        ));
        assert!(matches!(
            spec.evidence,
            crate::build_diagnostic::DiagnosticEvidenceSchema::SourceFacts
                | crate::build_diagnostic::DiagnosticEvidenceSchema::LexicalFacts(_)
                | crate::build_diagnostic::DiagnosticEvidenceSchema::AnalysisWitness
                | crate::build_diagnostic::DiagnosticEvidenceSchema::InternalInvariant
        ));
        assert!(matches!(
            spec.remedy,
            crate::build_diagnostic::DiagnosticRemedySchema::CorrectAuthoredInput
                | crate::build_diagnostic::DiagnosticRemedySchema::ResolveLexicalPolicy(_)
                | crate::build_diagnostic::DiagnosticRemedySchema::ResolveAnalysisConflict
                | crate::build_diagnostic::DiagnosticRemedySchema::ReportInternalFailure
        ));
        if spec.status == crate::build_diagnostic::DiagnosticCodeStatus::Retired {
            continue;
        }

        let evidence = match spec.evidence {
            crate::build_diagnostic::DiagnosticEvidenceSchema::SourceFacts => {
                DiagnosticEvidence::SourceFacts(vec![crate::DiagnosticFact {
                    name: "condition".to_owned(),
                    value: "registry-contract".to_owned(),
                }])
            }
            crate::build_diagnostic::DiagnosticEvidenceSchema::LexicalFacts(condition) => {
                DiagnosticEvidence::Lexical {
                    condition,
                    facts: vec![crate::DiagnosticFact {
                        name: "condition".to_owned(),
                        value: "registry-contract".to_owned(),
                    }],
                    witness: None,
                }
            }
            crate::build_diagnostic::DiagnosticEvidenceSchema::AnalysisWitness => {
                DiagnosticEvidence::Analysis {
                    overlap: Vec::new(),
                    witness: Vec::new(),
                    cycle: Vec::new(),
                    configured_lookahead: None,
                    sufficient_lookahead: None,
                    end_of_alternative: false,
                }
            }
            crate::build_diagnostic::DiagnosticEvidenceSchema::InternalInvariant => {
                DiagnosticEvidence::InternalInvariant {
                    producer: "registry-contract".to_owned(),
                    detail: "typed".to_owned(),
                }
            }
        };
        let remedy = match spec.remedy {
            crate::build_diagnostic::DiagnosticRemedySchema::CorrectAuthoredInput => {
                DiagnosticRemedy::CorrectSource
            }
            crate::build_diagnostic::DiagnosticRemedySchema::ResolveLexicalPolicy(_) => {
                DiagnosticRemedy::SelectSupportedConfiguration
            }
            crate::build_diagnostic::DiagnosticRemedySchema::ResolveAnalysisConflict => {
                DiagnosticRemedy::CorrectSource
            }
            crate::build_diagnostic::DiagnosticRemedySchema::ReportInternalFailure => {
                DiagnosticRemedy::ReportInternalFailure
            }
        };
        let diagnostic = RecursaDiagnostic::with_primary(
            spec.stage,
            spec.code,
            spec.severity,
            "registry contract",
            crate::RecursaDiagnosticLabel::located(
                DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(0..1)).unwrap(),
                "registry contract",
            ),
            evidence,
            vec![remedy],
        );
        assert_eq!(
            BuildReport::new(vec![diagnostic]).diagnostics()[0].code(),
            spec.code,
            "{} producer must satisfy its exact typed registry contract",
            spec.code.as_str(),
        );
    }
    assert!(codes.iter().all(|code| !code.starts_with("RCA900")));
}

#[test]
fn obsolete_authored_hooks_fail_directly_at_the_source_analysis_seam() {
    let package = write_fixture(
        r#"
recursa::grammar! { module = crate }
recursa::tokens! {
    post_lex = crate::repair,
    lexer_tokens { Opaque => r"~[a-z]+" with crate::scan }
}

#[derive(recursa::Node)]
#[parse(postcondition = crate::condition)]
struct Root<'input> {
    #[lex(pattern = r"[a-z]+", callback = crate::scan)]
    word: Word<'input>,
}

#[derive(recursa::Node)]
#[recursa::parser(postcondition = crate::condition)]
struct Namespaced;
"#,
    );
    let error = crate::source_analysis::analyze_source_tree(
        crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect_err("obsolete authored hooks must stop at source-model lowering");
    let mut actual = error
        .diagnostics()
        .iter()
        .map(|diagnostic| (diagnostic.code().as_str(), diagnostic.summary()))
        .collect::<Vec<_>>();
    actual.sort_unstable();

    assert_eq!(
        actual,
        [
            (
                "RCA3125",
                "authored `#[lex(..., callback = path)]` is unsupported; use a closed matcher",
            ),
            (
                "RCA3126",
                "authored central `with path` lexer callbacks are unsupported; use a closed matcher",
            ),
            (
                "RCA3127",
                "authored `post_lex = path` hooks are unsupported; express tokenization declaratively",
            ),
            (
                "RCA3128",
                "authored parser postconditions are unsupported; use a named admission",
            ),
            (
                "RCA3128",
                "authored parser postconditions are unsupported; use a named admission",
            ),
        ],
    );
    assert!(
        error
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.stage() == DiagnosticStage::Lowering),
    );
}

#[test]
fn full_source_analysis_pipeline_emits_invalidation_and_stops_at_the_earliest_failing_stage() {
    let package = write_fixture(
        r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    left: MissingLeft,
    right: MissingRight,
}
"#,
    );
    let mut cargo = Vec::new();
    let error = crate::source_analysis::analyze_source_tree(
        crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut cargo,
    )
    .expect_err("unresolved source cannot produce an analyzed grammar");

    assert_eq!(cargo, ["cargo::rerun-if-changed=grammar.rs"]);
    assert_eq!(error.diagnostics().len(), 2);
    assert!(
        error
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.stage() == DiagnosticStage::Resolution)
    );
    assert!(
        error
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.code().as_str() == "RCA2012")
    );
}

#[test]
fn closed_matcher_priority_ties_fail_at_source_analysis_and_distinct_priorities_succeed() {
    let source = |second_priority| {
        format!(
            r#"
recursa::grammar! {{ module = crate }}

recursa::tokens! {{
    matchers {{
        LineRegion => physical_line(prefix = r"@x", priority = 1),
        DelimitedRegion => same_delimiter(opener = r"@x", priority = {second_priority}),
    }}
}}

#[derive(recursa::Node)]
struct Root;
"#
        )
    };
    let tied = write_fixture(&source(1));
    let error = crate::source_analysis::analyze_source_tree(
        crate::source_analysis::SourceAnalysisInput::rust_2024(tied.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect_err("equal closed-matcher priorities must not fall back to declaration order");
    assert_eq!(error.diagnostics().len(), 1);
    assert_eq!(
        error.diagnostics()[0].code().as_str(),
        "RCA3006",
        "{:?}",
        error.diagnostics()
    );
    assert!(
        error.diagnostics()[0]
            .evidence()
            .canonical_text()
            .contains("priority=Some(1)")
    );

    let distinct = write_fixture(&source(2));
    let analyzed = crate::source_analysis::analyze_source_tree(
        crate::source_analysis::SourceAnalysisInput::rust_2024(distinct.path(), "grammar.rs"),
        &mut Vec::new(),
    );
    assert!(
        analyzed.is_ok(),
        "explicit closed-matcher priority must resolve the tie"
    );

    let converging_extent = write_fixture(
        r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    keywords { EXACT_A => "a" }
    matchers {
        ShortStarter => physical_line(prefix = r"a"),
        LongStarter => physical_line(prefix = r"ab"),
    }
}

#[derive(recursa::Node)]
struct Root;
"#,
    );
    let error = crate::source_analysis::analyze_source_tree(
        crate::source_analysis::SourceAnalysisInput::rust_2024(
            converging_extent.path(),
            "grammar.rs",
        ),
        &mut Vec::new(),
    )
    .expect_err("closed starters that converge on one final line extent need a priority");
    assert_eq!(error.diagnostics().len(), 1, "{:?}", error.diagnostics());
    assert_eq!(error.diagnostics()[0].code().as_str(), "RCA3005");
    assert!(
        error.diagnostics()[0]
            .evidence()
            .canonical_text()
            .contains("witness=ab"),
        "a shorter fixed spelling must not suppress the closed-matcher tie: {:?}",
        error.diagnostics()
    );
}

#[test]
fn diagnostics_use_unicode_scalar_coordinates_semantic_dedup_and_lossless_cargo_warnings() {
    let location = DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(2..4))
        .and_then(|location| location.with_source_text("αβ\nx"))
        .expect("test range uses UTF-8 boundaries");
    assert_eq!(
        location.line_columns(),
        Some((
            crate::LineColumn { line: 1, column: 2 },
            crate::LineColumn { line: 1, column: 3 },
        )),
    );
    let warning = RecursaDiagnostic::new(
        DiagnosticStage::Analysis,
        RecursaDiagnosticCode::new("RCA0300"),
        Severity::Warning,
        "nullable use overlaps caller FOLLOW",
        location,
        DiagnosticEvidence::SemanticWitness {
            subject: "overlapping grammar paths".to_owned(),
            witness: vec!["IDENT".to_owned()],
        },
        vec![DiagnosticRemedy::MakeSiteRequired {
            site: "Root::optional".to_owned(),
        }],
    )
    .with_order(1, "logical=root;site=optional-name");
    let unrelated = diagnostic(
        DiagnosticStage::Analysis,
        "RCA0301",
        Severity::Warning,
        "grammar.rs",
        8..9,
        "Pratt extender overlaps caller FOLLOW",
    );
    let report = BuildReport::new(vec![warning.clone(), unrelated, warning]);
    assert_eq!(report.diagnostics().len(), 2, "A-B-A duplicates collapse");
    assert!(report.canonical_rendering().contains("grammar.rs:1:2..1:3"));

    let mut cargo = Vec::new();
    crate::source_analysis::emit_warnings(&report, &mut cargo);
    assert_eq!(cargo.len(), 2);
    assert!(cargo[0].contains("evidence: overlapping grammar paths: IDENT"));
    assert!(cargo[0].contains("remedy: make nullable grammar site Root::optional required"));
    assert!(cargo.iter().all(|line| !line.contains('\n')));
}

#[test]
fn cargo_warning_transport_collapses_indistinguishable_semantic_findings() {
    let warning = |range: std::ops::Range<u32>, origin: &str| {
        RecursaDiagnostic::new(
            DiagnosticStage::Analysis,
            RecursaDiagnosticCode::new("RCA0301"),
            Severity::Warning,
            "Pratt use site may consume an active extender from caller FOLLOW",
            DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(range)).unwrap(),
            DiagnosticEvidence::SemanticWitness {
                subject: "overlapping grammar paths".to_owned(),
                witness: vec!["NOTNULL".to_owned()],
            },
            vec![DiagnosticRemedy::ReviewPrattFollow {
                site: "Expr::__recursa_attachment_element1".to_owned(),
                min_bp: 6,
            }],
        )
        .with_order(1, origin)
    };
    let report = BuildReport::new(vec![
        warning(4..8, "logical=expr;attachment=one"),
        warning(12..16, "logical=expr;attachment=two"),
    ]);
    assert_eq!(
        report.warnings().count(),
        2,
        "the structured build report retains distinct semantic origins",
    );

    let mut cargo = Vec::new();
    crate::source_analysis::emit_warnings(&report, &mut cargo);
    assert_eq!(
        cargo.len(),
        1,
        "Cargo must not print indistinguishable warning directives repeatedly",
    );
}

#[test]
fn semantic_dedup_is_not_adjacent_or_summary_dependent() {
    let same_failure = |summary: &str| {
        RecursaDiagnostic::new(
            DiagnosticStage::Resolution,
            RecursaDiagnosticCode::new("RCA2004"),
            Severity::Error,
            summary,
            DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(4..8)).unwrap(),
            DiagnosticEvidence::SourceFacts(vec![crate::DiagnosticFact {
                name: "import".to_owned(),
                value: "crate::same".to_owned(),
            }]),
            vec![DiagnosticRemedy::CorrectReference],
        )
        .with_order(4, "same-origin")
    };
    let first = same_failure("z summary for the same failure");
    let unrelated = diagnostic(
        DiagnosticStage::Resolution,
        "RCA2005",
        Severity::Error,
        "grammar.rs",
        5..9,
        "middle summary for another failure",
    )
    .with_order(5, "different-origin");
    let duplicate = same_failure("a different summary for the same failure");

    let report = BuildReport::new(vec![first.clone(), unrelated.clone(), duplicate.clone()]);
    let permuted = BuildReport::new(vec![duplicate, unrelated, first]);

    assert_eq!(report.diagnostics().len(), 2);
    assert_eq!(report.canonical_rendering(), permuted.canonical_rendering());
    assert!(
        report
            .canonical_rendering()
            .contains("a different summary for the same failure")
    );
    assert_eq!(
        report
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code().as_str() == "RCA2004")
            .count(),
        1,
    );

    let distinct = |summary: &str, value: &str| {
        RecursaDiagnostic::new(
            DiagnosticStage::Resolution,
            RecursaDiagnosticCode::new("RCA2004"),
            Severity::Error,
            summary,
            DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(4..8)).unwrap(),
            DiagnosticEvidence::SourceFacts(vec![crate::DiagnosticFact {
                name: "semantic-key".to_owned(),
                value: value.to_owned(),
            }]),
            vec![DiagnosticRemedy::CorrectReference],
        )
        .with_order(4, "same-origin")
    };
    let first = BuildReport::new(vec![distinct("z prose", "a"), distinct("a prose", "b")]);
    let second = BuildReport::new(vec![distinct("a prose", "a"), distinct("z prose", "b")]);
    let evidence = |report: &BuildReport| {
        report
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.evidence().canonical_text())
            .collect::<Vec<_>>()
    };
    assert_eq!(evidence(&first), evidence(&second));

    let assert_representative_is_permutation_stable =
        |left: RecursaDiagnostic, right: RecursaDiagnostic| {
            let forward = BuildReport::new(vec![left.clone(), right.clone()]);
            let reverse = BuildReport::new(vec![right, left]);
            assert_eq!(forward.canonical_rendering(), reverse.canonical_rendering());
        };
    let same_summary = same_failure("same summary");
    assert_representative_is_permutation_stable(
        same_summary
            .clone()
            .with_secondary(vec![crate::RecursaDiagnosticLabel::unlocated(
                "z secondary",
            )]),
        same_summary
            .clone()
            .with_secondary(vec![crate::RecursaDiagnosticLabel::unlocated(
                "a secondary",
            )]),
    );
    assert_representative_is_permutation_stable(
        same_summary
            .clone()
            .with_remedies(vec![DiagnosticRemedy::CorrectReference]),
        same_summary
            .clone()
            .with_remedies(vec![DiagnosticRemedy::CorrectSource]),
    );
    assert_representative_is_permutation_stable(
        same_summary.clone().with_notes(vec!["z note".to_owned()]),
        same_summary.with_notes(vec!["a note".to_owned()]),
    );
}

#[test]
fn diagnostic_locations_cannot_represent_package_escape_reversed_or_non_utf8_ranges() {
    assert_eq!(
        DiagnosticLocation::new(PathBuf::from("../outside.rs"), Some(2..9)),
        Err(crate::build_diagnostic::DiagnosticLocationInvariant::NonManifestRelativePath)
    );
    let reversed_start = 9;
    let reversed_end = 2;
    assert_eq!(
        DiagnosticLocation::new(
            PathBuf::from("grammar.rs"),
            Some(reversed_start..reversed_end),
        ),
        Err(crate::build_diagnostic::DiagnosticLocationInvariant::ReversedRange)
    );

    let split_scalar = DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(1..2))
        .and_then(|location| location.with_source_text("α"));
    assert_eq!(
        split_scalar,
        Err(crate::build_diagnostic::DiagnosticLocationInvariant::InvalidUtf8Boundary)
    );

    let internal = crate::source_analysis::checked_location(
        DiagnosticStage::Resolution,
        std::path::Path::new("../outside.rs"),
        Some(0..1),
        std::path::Path::new("grammar.rs"),
    )
    .expect_err("invalid producer location must fail closed");
    let report = BuildReport::new(vec![*internal]);
    assert_eq!(report.diagnostics()[0].code().as_str(), "RCA9102");
    assert_eq!(
        report.diagnostics()[0].primary().location().unwrap().path(),
        std::path::Path::new("grammar.rs"),
    );
}

#[test]
fn typed_registry_contracts_fail_closed_and_cargo_warnings_are_single_directives() {
    let malformed = RecursaDiagnostic::new(
        DiagnosticStage::Analysis,
        RecursaDiagnosticCode::new("RCA0300"),
        Severity::Warning,
        "bad producer",
        DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(0..1)).unwrap(),
        DiagnosticEvidence::SourceFacts(vec![crate::DiagnosticFact {
            name: "wrong".to_owned(),
            value: "schema".to_owned(),
        }]),
        vec![DiagnosticRemedy::CorrectSource],
    );
    let report = BuildReport::new(vec![malformed]);
    assert_eq!(report.diagnostics()[0].code().as_str(), "RCA9104");
    assert!(matches!(
        report.diagnostics()[0].evidence(),
        DiagnosticEvidence::InternalInvariant { .. }
    ));
    let mut internal_codes = Vec::new();
    for (stage, authored, internal) in [
        (DiagnosticStage::Discovery, "RCA1001", "RCA9101"),
        (DiagnosticStage::Resolution, "RCA2001", "RCA9102"),
        (DiagnosticStage::Lowering, "RCA3101", "RCA9103"),
        (DiagnosticStage::Analysis, "RCA0300", "RCA9104"),
        (DiagnosticStage::Emission, "RCA9105", "RCA9105"),
    ] {
        let malformed = RecursaDiagnostic::new(
            stage,
            RecursaDiagnosticCode::new(authored),
            if stage == DiagnosticStage::Analysis {
                Severity::Warning
            } else {
                Severity::Error
            },
            "bad producer",
            DiagnosticLocation::new(PathBuf::from("grammar.rs"), Some(0..1)).unwrap(),
            DiagnosticEvidence::Subject("untyped prose".to_owned()),
            vec![DiagnosticRemedy::CorrectSource],
        );
        let emitted = BuildReport::new(vec![malformed]).diagnostics()[0]
            .code()
            .as_str();
        assert_eq!(emitted, internal);
        internal_codes.push(emitted);
    }
    assert_eq!(
        internal_codes,
        ["RCA9101", "RCA9102", "RCA9103", "RCA9104", "RCA9105"]
    );

    let warning = diagnostic(
        DiagnosticStage::Analysis,
        "RCA0300",
        Severity::Warning,
        "gram\r\nmar.rs",
        0..1,
        "summary\r\ncontinued",
    );
    let mut cargo = Vec::new();
    crate::source_analysis::emit_warnings(&BuildReport::new(vec![warning]), &mut cargo);
    assert_eq!(cargo.len(), 1);
    assert!(cargo[0].starts_with("cargo::warning="));
    assert!(!cargo[0].contains('\r'));
    assert!(!cargo[0].contains('\n'));
}

#[test]
fn repeated_physical_inclusions_keep_distinct_permutation_stable_logical_origins() {
    fn diagnostics(order: &str) -> Vec<String> {
        let package = tempfile::tempdir().expect("temporary repeated-module fixture");
        std::fs::write(
            package.path().join("shared.rs"),
            "#[derive(recursa::Node)] pub struct Shared { missing: Missing }",
        )
        .expect("write shared module");
        std::fs::write(
            package.path().join("grammar.rs"),
            format!(
                "recursa::grammar! {{ module = crate }}\n{order}\n#[derive(recursa::Node)] struct Root {{ left: left::Shared, right: right::Shared }}"
            ),
        )
        .expect("write repeated-module root");
        let error = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect_err("each logical inclusion contains its own unresolved reference");
        assert_eq!(error.diagnostics().len(), 2);
        assert!(
            error
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.code().as_str() == "RCA2012")
        );
        error
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.evidence().canonical_text())
            .collect()
    }

    let left_first =
        diagnostics("#[path = \"shared.rs\"] mod left;\n#[path = \"shared.rs\"] mod right;");
    let right_first =
        diagnostics("#[path = \"shared.rs\"] mod right;\n#[path = \"shared.rs\"] mod left;");
    assert!(left_first.iter().any(|evidence| evidence.contains("left")));
    assert!(left_first.iter().any(|evidence| evidence.contains("right")));
    let normalize = |evidence: &[String]| {
        let mut logical = evidence
            .iter()
            .map(|item| {
                item.split_whitespace()
                    .find(|fact| fact.starts_with("logical-path="))
                    .expect("resolution evidence includes logical path")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        logical.sort();
        logical
    };
    assert_eq!(normalize(&left_first), normalize(&right_first));
}
