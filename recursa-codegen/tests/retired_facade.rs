use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/one-node";

struct CompileFailProbe {
    contract: &'static str,
    source: &'static str,
    diagnostic_subject: &'static str,
}

#[test]
fn retired_manual_parsing_surfaces_are_inaccessible_to_a_generated_grammar_consumer() {
    let package = scaffold_fixture();
    let baseline = check_fixture(&package);
    assert_success(&baseline, "the generated grammar baseline");

    let probes = [
        CompileFailProbe {
            contract: "handwritten public Parse implementations",
            diagnostic_subject: "Parse",
            source: r#"
struct ManualParser;
struct ManualInput;

impl recursa::Parse<ManualInput> for ManualParser {
    type Output = ();

    fn parse(_input: &mut ManualInput) -> Result<Self::Output, recursa::ParseError> {
        todo!()
    }
}

fn main() {}
"#,
        },
        CompileFailProbe {
            contract: "caller-named and caller-composed FOLLOW sets",
            diagnostic_subject: "FollowSet",
            source: r#"
fn accepts_caller_follow(_follow: &recursa::FollowSet) {}

fn main() {
    let follow = recursa::__private::follow_set(1, &[1], &[]);
    let composed = follow.union(&follow).unwrap();
    accepts_caller_follow(&composed);
}
"#,
        },
        CompileFailProbe {
            contract: "numeric token-kind identities",
            diagnostic_subject: "TokenKind",
            source: r#"
use recursa_generated_one_node::r#type as grammar;

fn main() {
    let _kind: grammar::TokenKind = 0;
}
"#,
        },
        CompileFailProbe {
            contract: "grammar-erased facade token kinds",
            diagnostic_subject: "TokenKind",
            source: r#"
fn main() {
    let _kind: recursa::TokenKind = 0;
}
"#,
        },
        CompileFailProbe {
            contract: "raw token-record construction",
            diagnostic_subject: "TokenRecord",
            source: r#"
fn main() {
    let _record = recursa::TokenRecord {
        kind: 0,
        span: recursa::Span::new(0, 0).unwrap(),
    };
}
"#,
        },
        CompileFailProbe {
            contract: "caller-constructed grammar metadata",
            diagnostic_subject: "GrammarInfo",
            source: r#"
use recursa_generated_one_node::r#type as grammar;

fn main() {
    let _grammar = grammar::GrammarInfo { _private: () };
}
"#,
        },
        CompileFailProbe {
            contract: "grammar-erased facade metadata",
            diagnostic_subject: "GrammarInfo",
            source: r#"
fn main() {
    let _grammar: Option<recursa::GrammarInfo> = None;
}
"#,
        },
        CompileFailProbe {
            contract: "caller-constructed raw lexing results",
            diagnostic_subject: "LexResult",
            source: r#"
fn main() {
    let _lexed: recursa::LexResult<'_> = recursa::LexResult::new("", Vec::new());
}
"#,
        },
        CompileFailProbe {
            contract: "grammar-erased facade inputs",
            diagnostic_subject: "Input",
            source: r#"
fn main() {
    let _input: Option<recursa::Input<'_>> = None;
}
"#,
        },
        CompileFailProbe {
            contract: "raw token cursor access on grammar-bound input",
            diagnostic_subject: "peek",
            source: r#"
use recursa_generated_one_node::r#type as grammar;

fn main() {
    let lexed = grammar::lex("alpha");
    let mut input = lexed.input();
    let _record = input.peek(0);
    let _record = input.advance();
}
"#,
        },
        CompileFailProbe {
            contract: "bare span-block storage and indexing",
            diagnostic_subject: "SpanBlock",
            source: r#"
fn main() {
    let block = recursa::SpanBlock::<1>::synthetic();
    let _node_span = block.node();
}
"#,
        },
        CompileFailProbe {
            contract: "handwritten bare-value HasSpan implementations",
            diagnostic_subject: "HasSpan",
            source: r#"
struct BareAst;

impl recursa::HasSpan for BareAst {
    fn span(&self) -> Option<recursa::Span> {
        None
    }
}

fn main() {}
"#,
        },
        CompileFailProbe {
            contract: "HasSpan on a bare generated AST value",
            diagnostic_subject: "span",
            source: r#"
use recursa::HasSpan as _;
use recursa_generated_one_node::r#type as grammar;

fn main() {
    let lexed = grammar::lex("alpha");
    let mut input = lexed.input();
    let ast = grammar::r#match::parse(&mut input).unwrap().into_ast();
    let _span = ast.span();
}
"#,
        },
    ];

    let main = package.path().join("src/main.rs");
    let mut accessible = Vec::new();
    let mut unrelated_failures = Vec::new();
    for probe in probes {
        fs::write(&main, probe.source).expect("write downstream compile-fail probe");
        let output = check_fixture(&package);
        if output.status.success() {
            accessible.push(probe.contract);
            continue;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains(probe.diagnostic_subject) {
            unrelated_failures.push(format!(
                "{} failed without mentioning `{}`:\n{}",
                probe.contract, probe.diagnostic_subject, stderr,
            ));
        }
    }

    assert!(
        unrelated_failures.is_empty(),
        "compile-fail probes failed for unrelated reasons:\n{}",
        unrelated_failures.join("\n\n"),
    );
    assert!(
        accessible.is_empty(),
        "retired surfaces remain accessible to downstream consumers:\n- {}",
        accessible.join("\n- "),
    );
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    copy(&fixture, package.path(), "build.rs");
    copy(&fixture, package.path(), "Cargo.lock");
    copy(&fixture, package.path(), "src/lib.rs");
    copy(&fixture, package.path(), "src/grammar.rs");
    copy(&fixture, package.path(), "src/main.rs");

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

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generated grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
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
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("check generated grammar fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn assert_success(output: &Output, subject: &str) {
    assert!(
        output.status.success(),
        "{subject} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
