use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/pratt";

#[test]
fn a_downstream_arithmetic_grammar_parses_the_frozen_pratt_plan() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);

    assert_success(&output);
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "pratt-ok");
}

#[test]
fn repeated_generation_of_the_pratt_grammar_is_deterministic() {
    let package = scaffold_fixture();
    assert_success(&run_fixture(&package));
    let first = generation_directories(&target_dir(&package));

    assert_success(&run_fixture(&package));
    let second = generation_directories(&target_dir(&package));

    assert_eq!(first, second);
    assert_eq!(first.len(), 1, "one semantic Generation is published");
}

#[test]
fn pratt_explorer_schema_exposes_inactive_variants_and_active_lhs_occurrences() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read Pratt manifest");
    fs::write(
        &manifest,
        source
            .replace(
                "features = [\"arbitrary\", \"serde\"]",
                "features = [\"arbitrary\", \"serde\", \"explorer\"]",
            )
            .replace(
                "recursa-codegen = { path = \"",
                "recursa-codegen = { features = [\"explorer\"], path = \"",
            ),
    )
    .expect("enable Pratt explorer facade payload");
    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read Pratt consumer");
    let assertions = r#"
    let explorer = grammar::Expr::explorer();
    let variants = explorer
        .schema()
        .root()
        .variants()
        .map(|variant| (variant.name(), variant.elements().len()))
        .collect::<Vec<_>>();
    assert_eq!(
        variants,
        [
            ("Integer", 1),
            ("Name", 1),
            ("MultiGroup", 1),
            ("Paren", 1),
            ("Neg", 2),
            ("NotExists", 3),
            ("Factorial", 2),
            ("Power", 3),
            ("Multiply", 3),
            ("Add", 3),
            ("IsNot", 4),
            ("Cast", 3),
            ("IsNull", 3),
        ],
    );
    let parsed = parse("a + 2 * 3");
    let parsed = recursa::explorer::ExplorerDocument::from_parsed(parsed);
    assert_eq!(parsed.root().active_variant().unwrap().name(), "Add");
    assert_eq!(parsed.root().elements().len(), 3);
    assert_eq!(
        parsed.root().elements().next().unwrap().children().len(),
        1,
    );
    let grouped = recursa::explorer::ExplorerDocument::from_parsed(parse("BEGIN GROUP a END GROUP"));
    assert_eq!(grouped.root().active_variant().unwrap().name(), "MultiGroup");
    let grouped_child = grouped.root().elements().next().unwrap().children().next().unwrap();
    assert_eq!(grouped_child.active_variant().unwrap().name(), "Name");
    let grouped_token = grouped_child.elements().next().unwrap()
        .children().next().unwrap()
        .elements().next().unwrap()
        .children().next().unwrap();
    assert_eq!(grouped_token.token_text(), Some("a"));
    assert!(explorer.schema().tokens().any(|token| {
        token.canonical_text() == Some("BEGIN")
            && token.highlight_class() == recursa::explorer::HighlightClass::Keyword
    }));
"#;
    fs::write(
        main,
        source.replace(
            "    println!(\"pratt-ok\");",
            &format!("{assertions}\n    println!(\"pratt-ok\");"),
        ),
    )
    .expect("add Pratt explorer assertions");

    assert_success(&run_fixture(&package));
}

#[test]
fn pratt_diagrams_render_frozen_roles_binding_powers_and_operands() {
    let package = scaffold_fixture();
    configure_diagrams(&package);

    let output = run_fixture_doc(&package);
    assert_success(&output);

    let docs = rendered_rustdoc(&package);
    let expression = docs
        .iter()
        .find(|(path, source)| {
            path.ends_with("grammar/enum.Expr.html") && source.contains("RECURSA_NODE_DIAGRAM")
        })
        .expect("Pratt expression documentation page");
    assert!(
        expression
            .1
            .contains("atom Paren(\"[\" grammar::Expr \"]\")"),
        "attached grouping tokens must come from the frozen materialized body:\n{}",
        expression.1,
    );
    assert!(
        !expression.1.contains("__recursa_attachment"),
        "private attachment units must not surface in Pratt diagrams",
    );
    assert!(
        expression
            .1
            .contains("prefix(bp=35) Neg(\"-\" grammar::Expr)"),
        "prefix role and binding power must be rendered:\n{}",
        expression.1,
    );
    assert!(
        expression
            .1
            .contains("postfix(bp=40) Factorial(grammar::Expr \"!\")"),
        "postfix lhs placement and binding power must be rendered:\n{}",
        expression.1,
    );
    assert!(
        expression
            .1
            .contains("infix(lbp=30, rbp=30) Power(grammar::Expr \"^\" grammar::Expr)"),
        "infix lhs, tail, and binding powers must be rendered:\n{}",
        expression.1,
    );
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary Pratt grammar package");
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
        .expect("read Pratt grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write Pratt grammar fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy Pratt grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run Pratt grammar fixture")
}

fn run_fixture_doc(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("doc")
        .arg("--offline")
        .arg("--quiet")
        .arg("--locked")
        .arg("--no-deps")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("RUSTDOCFLAGS", "-D rustdoc::broken_intra_doc_links")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("render Pratt grammar fixture documentation")
}

fn configure_diagrams(package: &TempDir) {
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read Pratt grammar");
    fs::write(
        grammar,
        source.replace(
            "    module = crate::grammar,",
            "    module = crate::grammar,\n    diagrams,",
        ),
    )
    .expect("request Pratt diagrams");

    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read Pratt manifest");
    let source = source
        .replace(
            "features = [\"arbitrary\", \"serde\"]",
            "features = [\"arbitrary\", \"serde\", \"diagrams\"]",
        )
        .replace(
            "recursa-codegen = { path = \"",
            "recursa-codegen = { features = [\"diagrams\"], path = \"",
        );
    fs::write(manifest, source).expect("enable Pratt diagram capabilities");
}

fn rendered_rustdoc(package: &TempDir) -> Vec<(PathBuf, String)> {
    let mut rendered = Vec::new();
    collect_rendered_rustdoc(&target_dir(package).join("doc"), &mut rendered);
    rendered
}

fn collect_rendered_rustdoc(root: &Path, rendered: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rendered_rustdoc(&path, rendered);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "html")
        {
            let source = fs::read_to_string(&path).expect("read rendered rustdoc page");
            rendered.push((path, source));
        }
    }
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn generation_directories(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|name| name == "recursa-generations")
        {
            if let Ok(generations) = fs::read_dir(&path) {
                found.extend(
                    generations
                        .flatten()
                        .map(|generation| generation.path())
                        .filter(|generation| generation.is_dir()),
                );
            }
        } else {
            found.extend(generation_directories(&path));
        }
    }
    found
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
