//! End-to-end proof of step two of the tiered flat AST milestone through a generated grammar: the
//! `flat` grammar-root flag, the `#[flat(...)]` declaration attribute, one flat type per
//! declaration, the `FlatAst` owner, and the layout assertions the design requires.
//!
//! Flat reduce actions and `parse_flat` are step three, so the fixture binary only compile-checks
//! the generated surface and prints its sizes. The negative cases mutate the grammar string and
//! assert the stable diagnostic the build must produce.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// The generation fixture this test scaffolds, runs, and mutates.
const FIXTURE: &str = "tests/fixtures/generation/flat-containers";

/// The grammar-root line the negative cases replace to drop or keep the `flat` flag.
const FLAT_FLAG: &str = "    flat,\n";

#[test]
fn the_flat_containers_fixture_emits_a_flat_type_for_every_declaration_and_one_owner() {
    let package = scaffold(FIXTURE, None);
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("flat-containers-ok\n"), "{stdout}");
    let lines = parser_lines(&stdout);

    // A unit fixed-token Node carries no field at all, so its flat form is a zero-size type.
    assert!(
        lines.contains("size grammar::FlatBang = 0\n"),
        "a unit Node has no flat field\n{lines}"
    );

    // One handle per child: a struct with one Node child is four bytes.
    for expected in [
        "size grammar::FlatAlias = 4\n",
        "size grammar::FlatGuard = 4\n",
        "size grammar::FlatBlock = 8\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // A content token becomes a four-byte text handle, not inline text.
    assert!(
        lines.contains("size grammar::FlatName = 4\n"),
        "a content token becomes one text handle\n{lines}"
    );

    // `Value` declares `max_size = 16`; its widest variant holds two handles plus a tag.
    assert!(
        lines.contains("size grammar::FlatValue = 12\n"),
        "the pooled enum stays inside its declared bound\n{lines}"
    );

    // The design's handle promises, checked in the generated code and printed here.
    assert!(
        lines.contains("optional handle=4 optional list=8\n"),
        "{lines}"
    );

    // Every flat type carries a distinct tag derived from its Node key.
    let tags = lines
        .lines()
        .find(|line| line.starts_with("tags "))
        .expect("the fixture prints three tags");
    let values = tags
        .split_whitespace()
        .skip(1)
        .map(|entry| entry.split_once('=').expect("a tag entry").1)
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 3, "{tags}");
    assert!(
        values[0] != values[1] && values[1] != values[2] && values[0] != values[2],
        "{tags}"
    );
}

#[test]
fn a_flat_declaration_attribute_without_the_root_flag_is_rejected() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(FIXTURE, Some(&grammar.replacen(FLAT_FLAG, "", 1)));
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("error[RCA3142] lowering at src/grammar.rs:")
            && stderr.contains("`#[flat(...)]` requires the `flat` flag in the grammar root"),
        "stderr:\n{stderr}",
    );
}

#[test]
fn an_unknown_flat_setting_is_rejected_at_the_declaration() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(
        FIXTURE,
        Some(&grammar.replacen(
            "#[flat(pool, max_size = 16)]",
            "#[flat(pool, packed, max_size = 16)]",
            1,
        )),
    );
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("error[RCA3141] lowering at src/grammar.rs:")
            && stderr.contains("unsupported flat setting `packed`"),
        "stderr:\n{stderr}",
    );
}

#[test]
fn a_flat_attribute_on_an_enum_variant_is_rejected() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(
        FIXTURE,
        Some(&grammar.replacen(
            "        Number(Number),",
            "        #[flat(pool)]\n        Number(Number),",
            1,
        )),
    );
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("error[RCA3141] lowering at src/grammar.rs:")
            && stderr.contains(
                "`#[flat(...)]` belongs on a whole struct or enum declaration, not on an enum \
                 variant",
            ),
        "stderr:\n{stderr}",
    );
}

#[test]
fn an_exceeded_max_size_bound_fails_the_generated_build() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(
        FIXTURE,
        Some(&grammar.replacen(
            "#[flat(pool, max_size = 16)]",
            "#[flat(pool, max_size = 4)]",
            1,
        )),
    );
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "flat Node `FlatValue` exceeds its declared `#[flat(pool, max_size = 4)]` bound",
        ),
        "stderr:\n{stderr}",
    );
}

/// Returns the fixture's authored grammar source path.
fn fixture_grammar() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE)
        .join("src/grammar.rs")
}

/// Copies the fixture package into a temporary directory; `grammar` replaces its grammar file.
fn scaffold(fixture: &str, grammar: Option<&str>) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(fixture);
    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative, relative);
    }
    copy(&fixture, package.path(), "Cargo.core.lock", "Cargo.lock");
    if let Some(grammar) = grammar {
        fs::write(package.path().join("src/grammar.rs"), grammar).expect("write the grammar");
    }
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

/// Selects the fixture's tagged lines and terminates each with a newline for whole-line matching.
fn parser_lines(stdout: &str) -> String {
    let mut parser = String::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("lr ") {
            parser.push_str(rest);
            parser.push('\n');
        }
    }
    parser
}

/// Copies one fixture file into the scaffolded package.
fn copy(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy generated grammar fixture file");
}

/// Renders a path for a TOML string literal.
fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

/// Builds and runs the scaffolded fixture binary.
fn run_fixture(package: &TempDir) -> Output {
    cargo(package, "run")
}

/// Builds the scaffolded fixture without running it, for a case that must fail to compile.
fn check_fixture(package: &TempDir) -> Output {
    cargo(package, "check")
}

/// Runs one Cargo subcommand inside the scaffolded package.
fn cargo(package: &TempDir, subcommand: &str) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg(subcommand)
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", package.path().join("target"))
        .output()
        .expect("run generated grammar fixture")
}

/// Fails with both streams when the fixture did not run.
fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
