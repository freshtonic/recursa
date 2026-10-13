use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/rejections";

/// The AST storage the scaffolded grammar declares.
///
/// One grammar template serves all three, because all three run one rule table through one walk
/// and must therefore accept and reject exactly the same sources.
#[derive(Clone, Copy)]
enum Ast {
    /// Ordinary nested storage: a parse returns `Parsed<Unique>`.
    Value,
    /// `arena_ast`: a parse returns `ArenaParsed`.
    Arena,
    /// `flat`: a parse returns `FlatAst`, whose Nodes live in the pools.
    Flat,
}

/// Every line the fixture prints, whichever storage it holds and whatever it samples.
///
/// A declared rejection reports Recursa's own `RCA4105`, then the rule's opaque code, message and
/// citation, then the extent of the item that completed the forbidden pair.
const REJECTED_BY_PG18: &[&str] = &[
    "build=pg18",
    // An empty list, a conflict-free list, and a list that repeats one attribute are all
    // accepted: PostgreSQL admits `DEFERRABLE DEFERRABLE`.
    "empty=ok",
    "clean=ok",
    "repeated=ok",
    // The pair is unordered, and the position is the item that completes it. `not deferrable`
    // first puts the rejection on `initially deferred` at 26..44; the reverse order puts it on
    // `not deferrable` at 30..44.
    "specific=RCA4105/42601/constraint declared INITIALLY DEFERRED must be DEFERRABLE/gram.y \
     REL_17_11 6114, ConstraintAttributeSpec/26..44",
    "specific_reversed=RCA4105/42601/constraint declared INITIALLY DEFERRED must be \
     DEFERRABLE/gram.y REL_17_11 6114, ConstraintAttributeSpec/30..44",
    // Each pair of the second rule carries that rule's own message.
    "general=RCA4105/42601/conflicting constraint properties/gram.y REL_17_11 6121, \
     ConstraintAttributeSpec/22..36",
    "general_second_pair=RCA4105/42601/conflicting constraint properties/gram.y REL_17_11 6121, \
     ConstraintAttributeSpec/31..49",
    // The third item completes a pair of each rule. The rule written first reports.
    "attribute_order=RCA4105/42601/constraint declared INITIALLY DEFERRED must be \
     DEFERRABLE/gram.y REL_17_11 6114, ConstraintAttributeSpec/41..55",
    // The same rules run over the one-or-more repetition of the same element type.
    "one_or_more=RCA4105/42601/conflicting constraint properties/gram.y REL_17_11 6121, \
     ConstraintAttributeSpec/21..31",
    "one_or_more_clean=ok",
    // The `config_attr`-gated rule exists in this build, because its variants do.
    "gated=RCA4105/42601/conflicting enforcement properties/gram.y REL_18_6, \
     ConstraintAttributeSpec/20..32",
];

/// The sampling line a storage with its own sampler prints.
const SAMPLED: &str = "sampled=64 samples parse back";

/// The sampling line a storage with no sampler of its own prints.
const NOT_SAMPLED: &str = "sampled=not sampled";

#[test]
fn a_rejection_rule_refuses_the_list_a_gram_y_action_refuses() {
    let package = scaffold_fixture(Ast::Value);
    let output = run_fixture(&package, Ast::Value, &["since-pg18"]);

    let mut expected = REJECTED_BY_PG18.to_vec();
    expected.push(SAMPLED);
    let reported = reported(&output);
    assert_eq!(reported, expected);
    // Every rejection reports Recursa's own runtime code for the failure class, beside the
    // grammar's opaque code.
    assert!(
        reported
            .iter()
            .filter(|line| line.contains("RCA4105"))
            .count()
            == 7,
        "every declared rejection of the fixture reports RCA4105: {reported:?}"
    );
}

#[test]
fn a_build_that_lacks_the_gated_variants_lacks_the_gated_rule() {
    let package = scaffold_fixture(Ast::Value);
    let output = run_fixture(&package, Ast::Value, &[]);

    // Every ungated rule reports exactly as it does in the newer build. The gated rule is gone
    // with the variants it names, so `enforced` is no keyword and the source is a syntax error
    // rather than a declared rejection.
    let reported = reported(&output);
    assert_eq!(reported.first(), Some(&"build=pg17"));
    let gated = reported
        .iter()
        .find(|line| line.starts_with("gated="))
        .expect("the fixture always reports the gated line");
    assert!(
        gated.starts_with("gated=RCA4100/"),
        "a build without the gated variants rejects the source as unexpected input: {gated}"
    );
    for line in REJECTED_BY_PG18
        .iter()
        .filter(|line| !line.starts_with("build=") && !line.starts_with("gated="))
    {
        assert!(
            reported.contains(line),
            "the older build reports every ungated rule unchanged, missing: {line}"
        );
    }
    assert!(reported.contains(&SAMPLED));
}

#[test]
fn an_arena_parse_rejects_what_a_value_parse_rejects_with_the_same_note_and_span() {
    let package = scaffold_fixture(Ast::Arena);
    let output = run_fixture(&package, Ast::Arena, &["since-pg18"]);

    // The value form and the arena form share one generated list action, so there is nothing
    // left that could differ.
    let mut expected = REJECTED_BY_PG18.to_vec();
    expected.push(NOT_SAMPLED);
    assert_eq!(reported(&output), expected);
}

#[test]
fn a_flat_parse_rejects_what_an_arena_parse_rejects_with_the_same_note_and_span() {
    let package = scaffold_fixture(Ast::Flat);
    let output = run_fixture(&package, Ast::Flat, &["since-pg18"]);

    // The flat action reads each item's variant out of its pool and reports the extent of the
    // element symbol the step consumed, which is the extent the nested action reports. Every
    // line therefore matches, span included.
    let mut expected = REJECTED_BY_PG18.to_vec();
    expected.push(NOT_SAMPLED);
    assert_eq!(reported(&output), expected);
}

/// Returns the lines the fixture printed, or fails with everything it wrote.
fn reported(output: &Output) -> Vec<&str> {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    std::str::from_utf8(&output.stdout)
        .expect("fixture output is text")
        .lines()
        .collect()
}

fn scaffold_fixture(ast: Ast) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated rejections package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in ["build.rs", "Cargo.lock", "src/lib.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative);
    }

    let grammar = fs::read_to_string(fixture.join("src/grammar.rs.in"))
        .expect("read generated rejections fixture grammar")
        .replace(
            "{{AST}}",
            match ast {
                Ast::Value => "",
                Ast::Arena => "arena_ast,",
                Ast::Flat => "flat,",
            },
        );
    fs::write(package.path().join("src/grammar.rs"), grammar)
        .expect("write generated rejections fixture grammar");

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generated rejections fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generated rejections fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generated rejections fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

/// Runs the scaffolded fixture, with the feature that states which grammar it holds.
fn run_fixture(package: &TempDir, ast: Ast, features: &[&str]) -> Output {
    let mut selected = features.to_vec();
    match ast {
        Ast::Value => {}
        Ast::Arena => selected.push("arena-ast"),
        Ast::Flat => selected.push("flat-ast"),
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet");
    if !selected.is_empty() {
        command.arg("--features").arg(selected.join(","));
    }
    command
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run generated rejections fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}
