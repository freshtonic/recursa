mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn cargo_clean_rebuilds_only_the_exact_stable_fixture_package() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = workspace.join("target/generated-fixtures/package-clean/package");
    let target = fixture.join("target");
    scaffold_workspace(&fixture);

    for package in ["foo-bar", "foo-bar-helper", "fixture-dependency"] {
        support::clean_downstream_package(&fixture, package, &target);
    }
    let initial = cargo(&fixture, &target, ["build", "--workspace"]);
    assert_success(&initial, "initial three-package fixture build");

    let deps = target.join("debug/deps");
    let own_metadata = one_prefixed(&deps, "libfoo_bar-", ".rmeta");
    let helper_metadata = one_prefixed(&deps, "libfoo_bar_helper-", ".rmeta");
    let dependency_metadata = one_prefixed(&deps, "libfixture_dependency-", ".rmeta");
    let own_incremental = prefixed(&target.join("debug/incremental"), "foo_bar-");
    let own_build = prefixed(&target.join("debug/build"), "foo-bar-");
    assert!(
        !own_incremental.is_empty(),
        "foo-bar produced no incremental state"
    );
    assert!(
        own_build.iter().any(|path| generation_exists(path)),
        "foo-bar build script published no generated state",
    );

    let mut permissions = fs::metadata(&own_metadata)
        .expect("read foo-bar metadata permissions")
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&own_metadata, permissions).expect("make foo-bar metadata read-only");

    support::clean_downstream_package(&fixture, "foo-bar", &target);

    assert!(
        !own_metadata.exists(),
        "foo-bar metadata survived Cargo clean"
    );
    assert!(
        own_incremental.iter().all(|path| !path.exists()),
        "foo-bar incremental state survived Cargo clean",
    );
    assert!(
        own_build.iter().all(|path| !path.exists()),
        "foo-bar build publication survived Cargo clean",
    );
    assert!(
        helper_metadata.exists(),
        "foo-bar-helper artifacts were removed"
    );
    assert!(
        dependency_metadata.exists(),
        "foo-bar dependency artifacts were removed",
    );

    fs::write(
        fixture.join("foo-bar/src/lib.rs"),
        "pub fn value() -> u8 { 2 }\n#[test]\nfn rebuilt_value_is_current() { assert_eq!(value(), 2); }\n",
    )
    .expect("change foo-bar authored source");
    let rebuilt = cargo(&fixture, &target, ["test", "-p", "foo-bar"]);
    assert_success(&rebuilt, "changed foo-bar rebuild");
    assert!(
        !prefixed(&deps, "libfoo_bar-").is_empty(),
        "foo-bar metadata was not rebuilt",
    );
    assert!(
        !prefixed(&target.join("debug/incremental"), "foo_bar-").is_empty(),
        "foo-bar incremental state was not rebuilt",
    );
    assert!(
        prefixed(&target.join("debug/build"), "foo-bar-")
            .iter()
            .any(|path| generation_exists(path)),
        "foo-bar generated publication was not rebuilt",
    );
    assert!(
        helper_metadata.exists(),
        "helper artifacts did not survive rebuild"
    );
    assert!(
        dependency_metadata.exists(),
        "dependency artifacts did not survive rebuild",
    );
}

fn scaffold_workspace(root: &Path) {
    for package in ["foo-bar", "foo-bar-helper", "fixture-dependency"] {
        fs::create_dir_all(root.join(package).join("src"))
            .expect("create package source directory");
    }
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"foo-bar\", \"foo-bar-helper\", \"fixture-dependency\"]\nresolver = \"3\"\n[profile.dev]\nincremental = true\n[profile.test]\nincremental = true\n",
    )
    .expect("write lifecycle workspace manifest");
    fs::write(
        root.join("foo-bar/Cargo.toml"),
        "[package]\nname = \"foo-bar\"\nversion = \"0.0.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n[dependencies]\nfixture-dependency = { path = \"../fixture-dependency\" }\n",
    )
    .expect("write foo-bar manifest");
    fs::write(
        root.join("foo-bar/build.rs"),
        "fn main() { let out = std::path::PathBuf::from(std::env::var_os(\"OUT_DIR\").unwrap()).join(\"recursa-generations\"); std::fs::create_dir_all(&out).unwrap(); std::fs::write(out.join(\"owned\"), \"generated\").unwrap(); }\n",
    )
    .expect("write foo-bar build script");
    fs::write(
        root.join("foo-bar/src/lib.rs"),
        "pub fn value() -> u8 { fixture_dependency::value() }\n",
    )
    .expect("write foo-bar source");
    fs::write(
        root.join("foo-bar-helper/Cargo.toml"),
        "[package]\nname = \"foo-bar-helper\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .expect("write foo-bar-helper manifest");
    fs::write(
        root.join("foo-bar-helper/src/lib.rs"),
        "pub fn helper() -> u8 { 7 }\n",
    )
    .expect("write foo-bar-helper source");
    fs::write(
        root.join("fixture-dependency/Cargo.toml"),
        "[package]\nname = \"fixture-dependency\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .expect("write fixture-dependency manifest");
    fs::write(
        root.join("fixture-dependency/src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .expect("write fixture-dependency source");
}

fn cargo<const N: usize>(root: &Path, target: &Path, arguments: [&str; N]) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args(arguments)
        .arg("--offline")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target)
        .env("CARGO_INCREMENTAL", "1")
        .env("CARGO_PROFILE_DEV_INCREMENTAL", "true")
        .env("CARGO_PROFILE_TEST_INCREMENTAL", "true")
        .env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", "");
    command.output().expect("run lifecycle fixture Cargo")
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn one_prefixed(root: &Path, prefix: &str, suffix: &str) -> PathBuf {
    prefixed(root, prefix)
        .into_iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(suffix))
        })
        .unwrap_or_else(|| {
            panic!(
                "missing artifact {prefix}*{suffix} below {}",
                root.display()
            )
        })
}

fn prefixed(root: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .map(|entry| entry.path())
        .collect()
}

fn generation_exists(build: &Path) -> bool {
    build.join("out/recursa-generations/owned").is_file()
}
