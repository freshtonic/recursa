//! `#[greedy(...)]` in a generated downstream crate: the acceptance silences the advisory
//! FOLLOW-overlap warnings and changes no parser behaviour.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/greedy";

#[test]
fn greedy_acceptances_silence_follow_warnings_without_changing_the_parser() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let accepted_source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    assert_eq!(accepted_source.matches("#[greedy(").count(), 3);

    let accepted_build = build_fixture(&package);
    assert_success(&accepted_build);
    let accepted_stderr = String::from_utf8_lossy(&accepted_build.stderr).into_owned();
    assert!(
        !accepted_stderr.contains("[RCA0300]") && !accepted_stderr.contains("[RCA0301]"),
        "accepted overlaps emit no advisory warning:\n{accepted_stderr}"
    );
    let accepted_run = run_fixture(&package);
    assert_success(&accepted_run);
    let accepted_output = String::from_utf8_lossy(&accepted_run.stdout).into_owned();
    assert!(
        accepted_output
            .contains("query \"SELECT\": ok Query { items: [Item { maybe: Some(Select) }] }"),
        "the optional element takes the SELECT that a separator could have taken:\n{accepted_output}"
    );
    assert!(
        accepted_output.contains("stmt \"1 !\": err"),
        "the postfix extender takes the `!` that the terminator needed:\n{accepted_output}"
    );

    let plain_source = accepted_source
        .lines()
        .filter(|line| !line.trim_start().starts_with("#[greedy("))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&grammar, plain_source).expect("remove the acceptances");
    let plain_build = build_fixture(&package);
    assert_success(&plain_build);
    let plain_stderr = String::from_utf8_lossy(&plain_build.stderr).into_owned();
    assert!(
        plain_stderr.contains("[RCA0300]") && plain_stderr.contains("[RCA0301]"),
        "the same grammar warns without the acceptances:\n{plain_stderr}"
    );
    let plain_run = run_fixture(&package);
    assert_success(&plain_run);
    assert_eq!(
        String::from_utf8_lossy(&plain_run.stdout),
        accepted_output,
        "the acceptance does not change what the generated parser accepts"
    );

    fs::write(
        &grammar,
        accepted_source.replace("#[greedy(BANG)]", "#[greedy(BANG, SELECT)]"),
    )
    .expect("declare a kind the overlap does not contain");
    let stale_build = build_fixture(&package);
    assert!(
        !stale_build.status.success(),
        "a stale acceptance fails the build"
    );
    let stale_stderr = String::from_utf8_lossy(&stale_build.stderr);
    assert!(stale_stderr.contains("RCA0302"), "{stale_stderr}");
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);
    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        let destination = package.path().join(relative);
        fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directories");
        fs::copy(fixture.join(relative), destination).expect("copy fixture file");
    }
    fs::copy(
        fixture.join("Cargo.core.lock"),
        package.path().join("Cargo.lock"),
    )
    .expect("copy fixture lock file");
    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generated grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generated grammar fixture manifest");
    package
}

fn toml_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn cargo(package: &TempDir, subcommand: &str, quiet: bool) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.arg(subcommand).arg("--offline").arg("--locked");
    if quiet {
        command.arg("--quiet");
    }
    command
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command
        .output()
        .expect("run cargo on the generated grammar fixture")
}

fn build_fixture(package: &TempDir) -> Output {
    cargo(package, "build", false)
}

fn run_fixture(package: &TempDir) -> Output {
    cargo(package, "run", true)
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
