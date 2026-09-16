use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[test]
fn checked_fixture_locks_use_workspace_preloaded_registry_packages() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let workspace_lock =
        fs::read_to_string(workspace.join("Cargo.lock")).expect("read workspace lock");
    let workspace_packages = registry_package_records(&workspace_lock);

    for relative in [
        "compound-nodes/Cargo.core.lock",
        "compound-nodes/Cargo.lock",
        "expressions/Cargo.core.lock",
        "follow-pollution/Cargo.core.lock",
        "lexing/Cargo.lock",
        "lookahead/Cargo.core.lock",
        "lookahead-precedence/Cargo.core.lock",
        "one-node/Cargo.core.lock",
        "one-node/Cargo.lock",
        "repetitions/Cargo.core.lock",
        "repetitions/Cargo.lock",
        "table-driven/Cargo.core.lock",
    ] {
        let lock = fs::read_to_string(
            manifest_dir
                .join("tests/fixtures/generation")
                .join(relative),
        )
        .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        for package in registry_package_records(&lock) {
            assert!(
                workspace_packages.contains(&package),
                "{relative} registry resolution {package:?} is not preloaded by the workspace lock",
            );
        }
    }
}

#[test]
fn diagram_fixture_locks_cover_the_mutated_optional_dependency_graph() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "compound-nodes/Cargo.lock",
        "one-node/Cargo.lock",
        "repetitions/Cargo.lock",
    ] {
        let lock = fs::read_to_string(
            manifest_dir
                .join("tests/fixtures/generation")
                .join(relative),
        )
        .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let packages = registry_package_records(&lock);
        for dependency in ["railroad", "unicode-width"] {
            assert!(
                packages.iter().any(|(name, _, _)| name == dependency),
                "{relative} omits diagram dependency {dependency}",
            );
        }
    }
}

fn registry_package_records(lock: &str) -> BTreeSet<(String, String, String)> {
    lock.split("[[package]]")
        .filter(|package| package.contains("source = \"registry+"))
        .map(|package| {
            let value = |field: &str| {
                package
                    .lines()
                    .find_map(|line| line.strip_prefix(field))
                    .and_then(|value| value.strip_prefix('"'))
                    .and_then(|value| value.strip_suffix('"'))
                    .unwrap_or_else(|| panic!("registry package omits {field:?}:\n{package}"))
                    .to_owned()
            };
            (value("name = "), value("version = "), value("checksum = "))
        })
        .collect()
}
