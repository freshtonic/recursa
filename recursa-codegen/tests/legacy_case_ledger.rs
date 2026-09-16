use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const FROZEN_REVISION: &str = "b410d2f123cd7510a8a40e9e5b971936a2fd8463";
const DIRECT_EXECUTION_TARGETS: [&str; 4] = [
    "--lib",
    "normalized_analysis",
    "generated_expressions",
    "recursa-example-expressions/behavior",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity<'a> {
    category: &'a str,
    grammar: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Evidence<'a> {
    kind: &'a str,
    reference: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Gate<'a> {
    executable: &'a str,
    status: &'a str,
}

#[test]
fn frozen_inventory_and_ledger_have_exactly_the_same_167_identities() {
    let inventory_source = read_acceptance("legacy-identities.tsv");
    let ledger_source = read_acceptance("legacy-case-ledger.tsv");
    let inventory = parse_inventory(&inventory_source);
    let ledger = parse_ledger(&ledger_source);

    assert_eq!(inventory.len(), 167);
    assert_eq!(ledger.len(), 167);
    assert_eq!(
        inventory.keys().collect::<BTreeSet<_>>(),
        ledger.keys().collect::<BTreeSet<_>>(),
        "the evidence ledger must neither omit nor invent a frozen legacy identity",
    );

    let category_counts = counts(inventory.values().map(|identity| identity.category));
    assert_eq!(
        category_counts,
        BTreeMap::from([
            ("analysis", 27),
            ("runtime", 100),
            ("combinator", 22),
            ("diagram", 15),
            ("generated", 3),
        ]),
    );

    let runtime_grammar_counts = counts(
        inventory
            .values()
            .filter(|identity| identity.category == "runtime")
            .map(|identity| identity.grammar),
    );
    assert_eq!(
        runtime_grammar_counts,
        BTreeMap::from([
            ("toy", 8),
            ("arith", 22),
            ("json", 29),
            ("sql", 19),
            ("optq", 10),
            ("cskw", 12),
        ]),
    );
}

#[test]
fn every_successor_reference_is_an_executable_test_or_a_named_typed_gate() {
    let ledger_source = read_acceptance("legacy-case-ledger.tsv");
    let matrix_source = read_acceptance("generation.md");
    let ledger = parse_ledger(&ledger_source);
    let gates = parse_gates(&matrix_source);
    let workspace = workspace_root();
    let mut parsed_test_files = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    let mut pending_identities = BTreeSet::new();
    let mut fixture_test_rows = 0;
    let mut direct_test_rows = 0;
    let mut direct_execution_targets = BTreeSet::<String>::new();

    for (legacy_id, evidence) in ledger {
        match evidence.kind {
            "test" => {
                let (relative, function) = evidence
                    .reference
                    .split_once('#')
                    .unwrap_or_else(|| panic!("invalid test evidence for {legacy_id}"));
                assert!(!relative.is_empty() && !function.is_empty());
                let path = workspace.join(relative);
                if relative
                    .starts_with("recursa-codegen/tests/fixtures/generation/legacy-grammars/")
                {
                    fixture_test_rows += 1;
                } else {
                    direct_test_rows += 1;
                    let target = direct_execution_target(relative).unwrap_or_else(|| {
                        panic!(
                            "{legacy_id} is direct test evidence outside the selected execution targets: {relative}"
                        )
                    });
                    assert!(
                        DIRECT_EXECUTION_TARGETS.contains(&target.as_str()),
                        "{legacy_id} requires unselected direct test target {target}",
                    );
                    direct_execution_targets.insert(target);
                }
                let functions = parsed_test_files
                    .entry(path.clone())
                    .or_insert_with(|| test_functions(&path));
                assert!(
                    functions.contains(function),
                    "{legacy_id} dangles: {relative} has no #[test] fn {function}",
                );
            }
            "gate" => {
                let gate = gates.get(evidence.reference).unwrap_or_else(|| {
                    panic!("{legacy_id} dangles: unknown gate {}", evidence.reference)
                });
                match gate.status {
                    "Green" => assert!(
                        gate.executable.starts_with("`cargo test ")
                            && gate.executable.ends_with('`'),
                        "green gate {} must name executable evidence",
                        evidence.reference,
                    ),
                    "Pending" => {
                        assert!(
                            gate.executable.starts_with("Pending on "),
                            "pending gate {} must expose its blocker",
                            evidence.reference,
                        );
                        pending_identities.insert(legacy_id);
                    }
                    status => panic!("unknown gate status {status:?}"),
                }
            }
            kind => panic!("unsupported evidence kind {kind:?} for {legacy_id}"),
        }
    }

    assert!(
        pending_identities.is_empty(),
        "every legacy identity must resolve to Green executable evidence",
    );
    assert_eq!(fixture_test_rows, 81);
    assert_eq!(direct_test_rows, 44);
    assert_eq!(
        direct_execution_targets,
        DIRECT_EXECUTION_TARGETS
            .iter()
            .map(|target| (*target).to_owned())
            .collect::<BTreeSet<_>>(),
        "every target containing direct successor evidence must be selected",
    );
}

#[test]
fn acceptance_sources_pin_the_frozen_spike_revision() {
    let matrix = read_acceptance("generation.md");
    assert!(matrix.contains(FROZEN_REVISION));
    assert!(matrix.contains("legacy-identities.tsv"));
    assert!(matrix.contains("legacy-case-ledger.tsv"));
}

fn parse_inventory(source: &str) -> BTreeMap<&str, Identity<'_>> {
    let mut lines = source.lines();
    assert_eq!(lines.next(), Some("legacy_id\tcategory\tgrammar"));
    let mut inventory = BTreeMap::new();
    for (index, line) in lines.enumerate() {
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 3, "inventory line {}", index + 2);
        assert!(
            matches!(
                columns[1],
                "analysis" | "runtime" | "combinator" | "diagram" | "generated"
            ),
            "unknown category on inventory line {}",
            index + 2,
        );
        let previous = inventory.insert(
            columns[0],
            Identity {
                category: columns[1],
                grammar: columns[2],
            },
        );
        assert!(
            previous.is_none(),
            "duplicate frozen identity {}",
            columns[0]
        );
    }
    inventory
}

fn parse_ledger(source: &str) -> BTreeMap<&str, Evidence<'_>> {
    let mut lines = source.lines();
    assert_eq!(lines.next(), Some("legacy_id\tevidence_kind\tevidence"));
    let mut ledger = BTreeMap::new();
    for (index, line) in lines.enumerate() {
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 3, "ledger line {}", index + 2);
        let previous = ledger.insert(
            columns[0],
            Evidence {
                kind: columns[1],
                reference: columns[2],
            },
        );
        assert!(
            previous.is_none(),
            "duplicate ledger identity {}",
            columns[0]
        );
    }
    ledger
}

fn parse_gates(source: &str) -> BTreeMap<&str, Gate<'_>> {
    let mut gates = BTreeMap::new();
    for line in source.lines().filter(|line| line.starts_with("| M4-")) {
        let columns = line
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        assert_eq!(columns.len(), 5, "malformed generation matrix row: {line}");
        let previous = gates.insert(
            columns[0],
            Gate {
                executable: columns[2],
                status: columns[4],
            },
        );
        assert!(previous.is_none(), "duplicate M4 gate {}", columns[0]);
    }
    gates
}

fn test_functions(path: &Path) -> BTreeSet<String> {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read successor source {}: {error}", path.display()));
    let file = syn::parse_file(&source)
        .unwrap_or_else(|error| panic!("parse successor source {}: {error}", path.display()));
    let mut functions = BTreeSet::new();
    collect_test_functions(&file.items, &mut functions);
    functions
}

fn direct_execution_target(relative: &str) -> Option<String> {
    if relative
        .strip_prefix("recursa-codegen/src/")
        .and_then(|path| path.strip_suffix(".rs"))
        .is_some_and(|path| !path.contains('/'))
    {
        return Some("--lib".to_owned());
    }
    if let Some((package, test)) = relative
        .strip_prefix("examples/")
        .and_then(|path| path.split_once("/tests/"))
        && let Some(test) = test.strip_suffix(".rs")
        && !package.contains('/')
        && !test.contains('/')
    {
        return Some(format!("recursa-example-{package}/{test}"));
    }
    relative
        .strip_prefix("recursa-codegen/tests/")
        .and_then(|path| path.strip_suffix(".rs"))
        .filter(|path| !path.contains('/'))
        .map(ToOwned::to_owned)
}

fn collect_test_functions(items: &[syn::Item], functions: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Fn(function)
                if function
                    .attrs
                    .iter()
                    .any(|attribute| attribute.path().is_ident("test")) =>
            {
                functions.insert(function.sig.ident.to_string());
            }
            syn::Item::Mod(module) => {
                if let Some((_, items)) = &module.content {
                    collect_test_functions(items, functions);
                }
            }
            _ => {}
        }
    }
}

fn counts<'a>(values: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
}

fn read_acceptance(name: &str) -> String {
    fs::read_to_string(workspace_root().join("docs/acceptance").join(name))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("recursa-codegen belongs to the workspace")
        .to_owned()
}
