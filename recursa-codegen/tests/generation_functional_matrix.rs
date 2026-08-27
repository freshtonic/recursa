use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const AGGREGATE_GATE: &str = "M4-FULL-FUNCTIONAL-MATRIX";

#[derive(Clone, Copy)]
struct Row<'a> {
    evidence: &'a str,
    status: &'a str,
}

#[test]
fn every_green_generation_evidence_command_executes_without_pending_rows() {
    let workspace = workspace_root();
    let document = fs::read_to_string(workspace.join("docs/acceptance/generation.md"))
        .expect("read generation acceptance contract");
    let rows = matrix_rows(&document);

    assert_eq!(rows.len(), 16);
    assert!(rows.values().all(|row| row.status == "Green"));
    assert_eq!(
        rows[AGGREGATE_GATE].evidence,
        "`cargo test -p recursa-codegen --test generation_functional_matrix --locked`",
    );

    let shared_target = tempfile::tempdir().expect("temporary generation matrix target");
    let mut covered_rows = BTreeSet::new();
    let mut executed_commands = BTreeSet::new();
    for (id, row) in &rows {
        if *id == AGGREGATE_GATE {
            continue;
        }
        let evidence = row
            .evidence
            .strip_prefix('`')
            .and_then(|evidence| evidence.strip_suffix('`'))
            .unwrap_or_else(|| panic!("{id} does not contain an executable command"));
        for command in evidence.split(" && ") {
            if executed_commands.insert(command.to_owned()) {
                run_evidence(command, &workspace, shared_target.path(), id);
            }
        }
        covered_rows.insert(*id);
    }

    assert_eq!(covered_rows.len(), 15);
    assert_eq!(
        executed_commands.len(),
        11,
        "shared evidence is run once while every non-aggregate Green row remains covered",
    );
}

fn run_evidence(command: &str, workspace: &Path, target: &Path, gate: &str) {
    let mut words = command.split_ascii_whitespace();
    assert_eq!(
        words.next(),
        Some("cargo"),
        "unsupported evidence for {gate}"
    );
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(words)
        .current_dir(workspace)
        .env("CARGO_TARGET_DIR", target)
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", "")
        .output()
        .unwrap_or_else(|error| panic!("execute {gate} evidence {command:?}: {error}"));
    assert!(
        output.status.success(),
        "{gate} evidence failed: {command}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
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
        assert!(
            rows.insert(
                columns[0],
                Row {
                    evidence: columns[2],
                    status: columns[4],
                },
            )
            .is_none(),
            "duplicate M4 row {}",
            columns[0],
        );
    }
    rows
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("recursa-codegen belongs to the workspace")
        .to_owned()
}
