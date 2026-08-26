use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/generic-applications";

#[test]
fn a_generic_node_can_be_parsed_through_multiple_closed_applications() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);

    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "generic-applications-ok"
    );
    assert_application_qualified_diagrams(&package);

    let permuted = scaffold_fixture();
    let grammar = permuted.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read generic application permutation");
    let source = source.replace(
        "    Integer(Atom<Integer<'input>>),\n    Name(Atom<Name<'input>>),",
        "    Name(Atom<Name<'input>>),\n    Integer(Atom<Integer<'input>>),",
    );
    let source = source.replace(
        "    Direct(#[tok(LPAREN, this, RPAREN)] Wrap<Name<'input>>),\n    Repeated(#[tok(LBRACKET, this, RBRACKET)] Wrap<Vec<Name<'input>>>),",
        "    Repeated(#[tok(LBRACKET, this, RBRACKET)] Wrap<Vec<Name<'input>>>),\n    Direct(#[tok(LPAREN, this, RPAREN)] Wrap<Name<'input>>),",
    );
    assert_ne!(
        source,
        fs::read_to_string(&grammar).expect("reread unpermuted grammar")
    );
    fs::write(grammar, source).expect("write generic application permutation");
    let output = run_fixture(&permuted);
    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "generic-applications-ok"
    );
    assert_application_qualified_diagrams(&permuted);
}

#[test]
fn explorer_adapters_are_generated_for_each_closed_generic_application() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read generic explorer manifest");
    fs::write(
        &manifest,
        source
            .replace(
                "features = [\"serde\", \"diagrams\"]",
                "features = [\"serde\", \"diagrams\", \"explorer\"]",
            )
            .replace(
                "features = [\"diagrams\"]",
                "features = [\"diagrams\", \"explorer\"]",
            ),
    )
    .expect("enable explorer facade payload");

    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read generic explorer grammar");
    fs::write(
        &grammar,
        source
            .replace(
                "PartialEq, Pretty, serde::Serialize",
                "PartialEq, Pretty, Visit, serde::Serialize",
            )
            .replace(
                "pub struct Pair<'left, 'right>",
                "#[node(omit(Visit))]\npub struct Pair<'left, 'right>",
            ),
    )
    .expect("configure Visit for the generic explorer root family");

    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read generic explorer consumer");
    let assertions = r#"
    let integer_adapter = grammar::Atom::<grammar::Integer<'static>>::explorer();
    assert_eq!(integer_adapter.schema().root().name(), "Atom");
    let integer_lexed = grammar::lex("42");
    let mut integer_input = integer_lexed.input();
    let integer = grammar::Atom::<grammar::Integer<'_>>::parse_spanned(&mut integer_input)
        .expect("integer application explorer parse");
    let integer = recursa::explorer::ExplorerDocument::from_parsed(integer);
    let integer_pretty = integer.pretty(&recursa::PrettyConfig::default()).unwrap();
    assert_eq!(integer_pretty.rendered, "42");
    assert_eq!(
        integer_pretty.source_map.source_at(0),
        Some(recursa::Span::new(0, 2).unwrap()),
    );

    let name_adapter = grammar::Atom::<grammar::Name<'static>>::explorer();
    assert_eq!(name_adapter.schema().root().name(), "Atom");
    let name_lexed = grammar::lex("alpha");
    let mut name_input = name_lexed.input();
    let name = grammar::Atom::<grammar::Name<'_>>::parse_spanned(&mut name_input)
        .expect("name application explorer parse");
    let name = recursa::explorer::ExplorerDocument::from_parsed(name);
    assert_eq!(
        name.pretty(&recursa::PrettyConfig::default()).unwrap().rendered,
        "alpha",
    );
    assert_eq!(name.root().elements().next().unwrap().children().len(), 1);
"#;
    fs::write(
        &main,
        source.replace(
            "    println!(\"generic-applications-ok\");",
            &format!("{assertions}\n    println!(\"generic-applications-ok\");"),
        ),
    )
    .expect("add generic explorer assertions");

    let output = run_fixture(&package);
    assert_success(&output);
}

#[test]
fn a_generic_explorer_entry_method_collision_is_diagnosed_before_emission() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read generic explorer manifest");
    fs::write(
        &manifest,
        source
            .replace(
                "features = [\"serde\", \"diagrams\"]",
                "features = [\"serde\", \"diagrams\", \"explorer\"]",
            )
            .replace(
                "features = [\"diagrams\"]",
                "features = [\"diagrams\", \"explorer\"]",
            ),
    )
    .expect("enable explorer facade payload");

    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read generic explorer grammar");
    fs::write(
        &grammar,
        format!(
            "{}\nimpl<T> Atom<T> {{ pub fn explorer() {{}} }}\n",
            source
                .replace(
                    "PartialEq, Pretty, serde::Serialize",
                    "PartialEq, Pretty, Visit, serde::Serialize",
                )
                .replace(
                    "pub struct Pair<'left, 'right>",
                    "#[node(omit(Visit))]\npub struct Pair<'left, 'right>",
                ),
        ),
    )
    .expect("insert generic explorer method collision");

    let output = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "explorer collision must fail");
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("generated explorer entry method `Atom::explorer` collides"),
        "stderr:\n{stderr}",
    );
}

#[test]
fn a_secondary_generic_application_schema_collision_is_diagnosed_before_emission() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read generic explorer manifest");
    fs::write(
        &manifest,
        source
            .replace(
                "features = [\"serde\", \"diagrams\"]",
                "features = [\"serde\", \"diagrams\", \"explorer\"]",
            )
            .replace(
                "features = [\"diagrams\"]",
                "features = [\"diagrams\", \"explorer\"]",
            ),
    )
    .expect("enable explorer facade payload");

    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read generic explorer grammar");
    fs::write(
        &grammar,
        source
            .replace(
                "PartialEq, Pretty, serde::Serialize",
                "PartialEq, Pretty, Visit, serde::Serialize",
            )
            .replace(
                "pub struct Pair<'left, 'right>",
                "#[node(omit(Visit))]\npub struct Pair<'left, 'right>",
            )
            .replace(
                "#[derive(recursa::Node)]\npub struct Integer<'input>",
                "static __RECURSA_EXPLORER_SCHEMA_3: () = ();\n\n#[derive(recursa::Node)]\npub struct Integer<'input>",
            ),
    )
    .expect("insert secondary generic application schema collision");

    let output = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "explorer collision must fail");
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "generated explorer item `__RECURSA_EXPLORER_SCHEMA_3` collides with an authored value"
        ),
        "stderr:\n{stderr}",
    );
}

fn assert_application_qualified_diagrams(package: &TempDir) {
    let output = run_fixture_doc(package);
    assert_success(&output);
    let mut docs = Vec::new();
    collect_rendered_rustdoc(&target_dir(package).join("doc"), &mut docs);
    let aggregate = docs
        .iter()
        .find(|(path, source)| {
            path.ends_with("grammar_docs/index.html") && source.contains("RECURSA_GRAMMAR_DIAGRAM")
        })
        .expect("aggregate generic application diagram");
    for production in [
        "grammar::Wrap&lt;grammar::Name&gt; =&gt; Wrap ::= grammar::Name",
        "grammar::Wrap&lt;Vec&lt;grammar::Name&gt;&gt; =&gt; Wrap ::= Vec&lt;grammar::Name&gt;",
        "grammar::Atom&lt;grammar::Integer&gt; =&gt; Atom ::= grammar::Integer",
        "grammar::Atom&lt;grammar::Name&gt; =&gt; Atom ::= grammar::Name",
        "grammar::Chain&lt;grammar::Integer&gt; =&gt; Chain ::= \"{\" grammar::Integer Option&lt;grammar::Chain&lt;grammar::Integer&gt;&gt; \"}\"",
        "grammar::Chain&lt;grammar::Name&gt; =&gt; Chain ::= \"{\" grammar::Name Option&lt;grammar::Chain&lt;grammar::Name&gt;&gt; \"}\"",
    ] {
        assert!(
            aggregate.1.contains(production),
            "missing application-qualified production `{production}`:\n{}",
            aggregate.1,
        );
    }
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generic-applications package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative);
    }

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generic-applications fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generic-applications fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generic-applications fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run generic-applications fixture")
}

fn run_fixture_doc(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("doc")
        .arg("--offline")
        .arg("--quiet")
        .arg("--no-deps")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("RUSTDOCFLAGS", "-D rustdoc::broken_intra_doc_links")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("render generic application grammar documentation")
}

fn collect_rendered_rustdoc(root: &Path, rendered: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("read generic application rustdoc entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rendered_rustdoc(&path, rendered);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "html")
        {
            rendered.push((
                path.clone(),
                fs::read_to_string(path).expect("read rendered generic application rustdoc"),
            ));
        }
    }
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
