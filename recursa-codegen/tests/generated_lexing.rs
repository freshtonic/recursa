use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/lexing";

#[test]
fn a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);

    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "lexing-ok RCA4000 RCA4001 RCA4002 RCA4004"
    );
}

#[test]
fn a_fixed_match_wins_when_trivia_loses_local_fence_arbitration() {
    let package = scaffold_fixture();
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"
recursa::grammar! {
    module = crate::grammar,
}

recursa::tokens! {
    ignore = r"--",
    punctuation {
        BANG_DASH_DASH => "!--",
        DASH_DASH => "--",
    }
    matchers {
        OperatorText => operator_run(
            characters = "!-",
            fences = ["--"],
            trailing = "",
            qualifying = "",
            priority = 3
        ),
    }
}

#[derive(recursa::Node)]
pub struct OperatorDefinition<'input> {
    #[lex(matcher)]
    pub operator: OperatorText<'input>,
}
"#,
    )
    .expect("write local fence-arbitration grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"
use recursa_generated_lexing::grammar;

fn main() {
    let lexed = grammar::lex("!--");
    assert_eq!(lexed.errors().count(), 0);
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind(), grammar::TokenKind::BANG_DASH_DASH);
    assert_eq!(tokens[0].text(), "!--");
}
"#,
    )
    .expect("write local fence-arbitration assertion");

    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn alternative_lexer_cannot_bypass_generated_lexical_validation() {
    let package = scaffold_fixture();
    let grammar_path = package.path().join("src/grammar.rs");
    let mut grammar = fs::read_to_string(&grammar_path).expect("read copied grammar fixture");
    grammar.push_str(
        r#"
pub fn attempt_raw_mutation(
    builder: &mut LexBuilder<'_>,
) {
    builder.records.clear();
}
"#,
    );
    fs::write(grammar_path, grammar).expect("write privacy-negative grammar fixture");

    let output = check_fixture(&package);
    assert!(
        !output.status.success(),
        "raw lexical mutation unexpectedly compiled"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("field `records`") && stderr.contains("private"),
        "missing privacy error for `records`\nstderr:\n{stderr}",
    );
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated lexing package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in [
        "build.rs",
        "Cargo.lock",
        "src/lib.rs",
        "src/grammar.rs",
        "src/main.rs",
    ] {
        copy(&fixture, package.path(), relative);
    }

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generated lexing fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generated lexing fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generated lexing fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run generated lexing fixture")
}

fn check_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("check")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("check generated lexing fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}
