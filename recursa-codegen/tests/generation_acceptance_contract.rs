use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Row<'a> {
    evidence: &'a str,
    status: &'a str,
}

#[test]
fn generation_matrix_distinguishes_executable_green_evidence_from_pending_dependencies() {
    let document = fs::read_to_string(matrix_path()).expect("read generation acceptance contract");
    let rows = matrix_rows(&document);

    assert_eq!(rows.len(), 16);
    assert_eq!(
        rows.values().filter(|row| row.status == "Green").count(),
        16,
    );
    assert_eq!(
        rows.values().filter(|row| row.status == "Pending").count(),
        0,
    );

    for (id, row) in &rows {
        match row.status {
            "Green" => assert!(
                row.evidence.starts_with("`cargo test ") && row.evidence.ends_with('`'),
                "{id} is Green without an executable cargo test",
            ),
            "Pending" => assert!(
                row.evidence.starts_with("Pending on "),
                "{id} must name its unresolved dependency instead of resembling executable evidence",
            ),
            status => panic!("{id} has unsupported status {status:?}"),
        }
    }

    let green = rows
        .iter()
        .filter(|(_, row)| row.status == "Green")
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        green,
        BTreeSet::from([
            "M4-LEDGER",
            "M4-SIX-PROJECTIONS",
            "M4-FROZEN-PARSE-PLANS",
            "M4-GENERATED-CONTAINERS",
            "M4-DIAGRAM-DECL-LOCAL",
            "M4-PRATT-DIAGRAM",
            "M4-LEGACY-RUNTIME-TOY",
            "M4-LEGACY-RUNTIME-ARITH",
            "M4-LEGACY-RUNTIME-JSON",
            "M4-LEGACY-RUNTIME-SQL",
            "M4-LEGACY-RUNTIME-OPTQ",
            "M4-LEGACY-RUNTIME-CSKW",
            "M4-PRETTY-ROUNDTRIP",
            "M4-EXPLORER",
            "M4-DEBUG-BUNDLE",
            "M4-FULL-FUNCTIONAL-MATRIX",
        ]),
    );

    let pending = rows
        .iter()
        .filter(|(_, row)| row.status == "Pending")
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    assert!(pending.is_empty());
    assert_eq!(
        rows["M4-LEDGER"].evidence,
        "`cargo test -p recursa-codegen --lib --test legacy_case_ledger --test analysis_parity --test analysis_diagnostics --test analysis_conflicts --test analysis_pratt_conflicts --locked`",
    );
    assert_eq!(
        rows["M4-EXPLORER"].evidence,
        "`cargo test -p recursa-codegen --test generated_compound_nodes explorer_adapters_expose_erased_schema_occurrences_lexing_pretty_and_samples --locked && cargo test -p recursa-codegen --test generated_generic_applications explorer_adapters_are_generated_for_each_closed_generic_application --locked`",
    );
    assert_eq!(
        rows["M4-DEBUG-BUNDLE"].evidence,
        "`cargo test -p recursa-codegen --test generated_one_node debug_emission_preserves_compiled_semantics_for_every_capability_combination --locked`",
    );
    assert_eq!(
        rows["M4-FULL-FUNCTIONAL-MATRIX"].evidence,
        "`cargo test -p recursa-codegen --test generation_functional_matrix --locked`",
    );
}

#[test]
fn generation_matrix_links_the_machine_readable_inventory_and_ledger() {
    let document = fs::read_to_string(matrix_path()).expect("read generation acceptance contract");
    for relative in ["legacy-identities.tsv", "legacy-case-ledger.tsv"] {
        assert!(document.contains(relative));
        assert!(
            matrix_path().parent().unwrap().join(relative).is_file(),
            "missing linked acceptance artifact {relative}",
        );
    }
}

fn matrix_rows(document: &str) -> BTreeMap<&str, Row<'_>> {
    let mut rows = BTreeMap::new();
    for line in document.lines().filter(|line| line.starts_with("| M4-")) {
        let columns = line
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        assert_eq!(columns.len(), 5, "malformed generation matrix row: {line}");
        let previous = rows.insert(
            columns[0],
            Row {
                evidence: columns[2],
                status: columns[4],
            },
        );
        assert!(previous.is_none(), "duplicate M4 row {}", columns[0]);
    }
    rows
}

fn matrix_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("recursa-codegen belongs to the workspace")
        .join("docs/acceptance/generation.md")
}
