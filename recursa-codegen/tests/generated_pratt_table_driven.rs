//! End-to-end proof of Pratt lowering to precedence (#115) through the sole Recursa parser.
//! It covers the textbook expression fixture and the `examples/pratt-expressions` grammar.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/table-driven-pratt";

#[test]
fn the_pratt_fixture_parses_and_dumps_the_clone() {
    let package = scaffold(FIXTURE, None);
    let debug = tempfile::tempdir().unwrap();

    let output = run_fixture(&package, Some(debug.path()));
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("table-driven-pratt-ok\n"), "{stdout}");
    let table_stdout = parser_lines(&stdout);
    let root = read_selected_root(&package);
    assert!(
        root.contains("__recursa_lr"),
        "root carries the tables and dispatcher"
    );

    // Associativity and precedence through parsed values: `1 - 2 - 3` is `(1 - 2) - 3`,
    // `2 ^ 3 ^ 2` is `2 ^ (3 ^ 2)`, `-a!` is `-(a!)`, and the needle of `position` stops at
    // `in` while the haystack takes the rest.
    let integer = |text: &str| {
        format!(
            "Integer(Integer {{ text: __RecursaContentToken0 {{ text: ContentText({text:?}) }} }})"
        )
    };
    let name = |text: &str| {
        format!("Name(Name {{ text: __RecursaContentToken1 {{ text: ContentText({text:?}) }} }})")
    };
    assert!(
        table_stdout.contains(&format!(
            "ok \"1 - 2 - 3\" => Subtract(Subtract({}, {}), {}) cursor=5 eof=true\n",
            integer("1"),
            integer("2"),
            integer("3")
        )),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains(&format!(
            "ok \"2 ^ 3 ^ 2\" => Power({}, Power({}, {})) cursor=5 eof=true\n",
            integer("2"),
            integer("3"),
            integer("2")
        )),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains(&format!(
            "ok \"-a!\" => Negate(Factorial({})) cursor=3 eof=true\n",
            name("a")
        )),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains(&format!(
            "ok \"position(a + b in c in d)\" => Position(Position {{ needle: Add({}, {}), haystack: In({}, {}) }}) cursor=10 eof=true\n",
            name("a"),
            name("b"),
            name("c"),
            name("d")
        )),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"1 2\" => ") && table_stdout.contains("cursor=1 eof=false\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"a between b in c and d\" => code=RCA4100"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains(&format!(
            "ok \"a between b and c in d\" => In(Between({}, {}, {}), {}) cursor=7 eof=true\n",
            name("a"),
            name("b"),
            name("c"),
            name("d")
        )),
        "{table_stdout}"
    );

    // The debug bundle's automaton dump shows the exclusion clone and its cloned attachment.
    let index_path = find_one_debug_index(debug.path());
    let dump = fs::read_to_string(index_path.parent().unwrap().join("automaton.txt"))
        .expect("the bundle carries the automaton dump");
    assert!(
        dump.contains(
            "grammar::Expr[excl In] -> grammar::Expr[excl In] PLUS grammar::Expr[excl In]"
        ),
        "{dump}"
    );
    assert!(
        dump.contains("[excl In] -> BETWEEN grammar::Expr[excl In, min 8]"),
        "{dump}"
    );
    assert!(
        dump.contains("[excl In] -> AND grammar::Expr[excl In, min 8]"),
        "{dump}"
    );
    assert!(
        dump.contains("-> LBRACKET grammar::Expr[min 50] RBRACKET"),
        "{dump}"
    );
    assert!(
        dump.contains("grammar::Expr -> grammar::Expr FROM grammar::Expr[min 7] FOR grammar::Expr"),
        "{dump}"
    );
    assert!(
        table_stdout.contains("err \"a[b + c]\" => code=RCA4100"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"a from b in c for d\" => code=RCA4100"),
        "{table_stdout}"
    );
    assert!(dump.contains("resolved by precedence (reduce)."), "{dump}");
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");
}

#[test]
fn the_pratt_expressions_example_parses_through_the_recursa_parser() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let example = fs::read_to_string(workspace.join("examples/pratt-expressions/src/grammar.rs"))
        .expect("read the example grammar");
    assert_eq!(example.matches("    diagrams,\n").count(), 1);
    assert_eq!(example.matches("    derives(Visit),\n").count(), 1);
    let grammar = example
        .replace("    diagrams,\n", "")
        .replace("    derives(Visit),\n", "    derives(Visit, Pretty),\n")
        .replace(
            "#[derive(recursa::Node)]",
            "#[derive(recursa::Node, Debug)]",
        );
    let package = scaffold(FIXTURE, Some(&grammar));
    fs::write(package.path().join("src/main.rs"), EXAMPLE_MAIN).unwrap();

    let output = run_fixture(&package, None);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("example-ok\n"), "{stdout}");
    let table_stdout = parser_lines(&stdout);
    assert!(
        table_stdout.contains("ok \"1 + 2 + 3\" => Add(Add("),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"2 ^ 3 ^ 4\" => Power(Integer("),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"-name!\" => Negate(Factorial("),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"-a! + 2 * 3 ^ 4\" => Add(Negate(Factorial("),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("lexical \"@\" => 1 error(s)"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"1 +\" => code=RCA4101"),
        "{table_stdout}"
    );
    assert!(read_selected_root(&package).contains("__recursa_lr"));
}

/// The driver of the example grammar: every input the example's tests and binary use, and a
/// few more that mix every operator.
const EXAMPLE_MAIN: &str = r#"//! Drives the example grammar through the sole Recursa parser.

use recursa::{HasSpan as _, NodeView as _, Pretty as _};
use recursa_generated_table_driven_pratt::grammar;

/// Drives every input through the Recursa parser: `$tag` prefixes each printed line and
/// `$input` builds that parser's input over a lexing result.
macro_rules! drive {
    ($tag:expr, $input:expr) => {{
        let tag: &str = $tag;
        // Prints the provenance of one positive input parsed with capture requested: the
        // occurrence tree, the root's elements and the Pretty rendering. The comparison
        // records all three facets of the sole parser's result.
        let provenance = |source: &str| {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Expr::parse_spanned(&mut input).expect("positive input parses with capture");
            println!("{tag} prov {source:?} => {}", parsed.debug_provenance());
            let elements: Vec<String> = parsed
                .root()
                .span_elements()
                .map(|element| {
                    format!(
                        "{}:{:?}@{}",
                        element.name().unwrap_or("?"),
                        element.role(),
                        element.span().map_or_else(|| "-".to_owned(), |span| format!("{}..{}", span.start(), span.end()))
                    )
                })
                .collect();
            println!("{tag} elements {source:?} => [{}]", elements.join(" "));
            println!("{tag} pretty {source:?} => {:?}", parsed.render());
        };
        for source in [
            "1 + 2 + 3",
            "-name!",
            "(1 + 2) * 3",
            "1 + 2 * 3",
            "2 ^ 3 ^ 4",
            "-a! + 2 * 3 ^ 4",
            "2 * 3 + 4",
            "-2 ^ 2",
            "1 + -2",
            "- - 1",
            "a ! !",
            "(a + b) ^ (c * d) !",
            "@",
            "1 +",
            "1 2",
            "",
            "(1 + 2",
            "* 1",
        ] {
            let lexed = grammar::lex(source);
            let lexical = lexed.errors().count();
            if lexical > 0 {
                println!("{tag} lexical {source:?} => {lexical} error(s)");
                continue;
            }
            let mut input = $input(&lexed);
            match grammar::Expr::parse(&mut input) {
                Ok(parsed) => {
                    println!(
                        "{tag} ok {source:?} => {:?} cursor={} eof={}",
                        parsed.into_ast(),
                        input.cursor(),
                        input.is_eof()
                    );
                    provenance(source);
                }
                Err(error) => println!(
                    "{tag} err {source:?} => code={} kind={:?} span={:?} found={:?} cursor={}",
                    error.code(),
                    error.kind(),
                    error.span(),
                    error.found(),
                    input.cursor()
                ),
            }
        }
    }};
}

/// Builds the sole generated parser input.
fn parser_input<'input>(lexed: &grammar::LexResult<'input>) -> grammar::Input<'input> {
    lexed.input()
}

fn main() {
    drive!("lr", parser_input);
    println!("example-ok");
}

"#;

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

/// Selects the sole parser's tagged fixture lines and terminates each with a newline so the
/// assertions can match whole lines.
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

fn read_selected_root(package: &TempDir) -> String {
    let mut found = Vec::new();
    find_named(
        &package.path().join("target"),
        "recursa-selected.rs",
        &mut found,
    );
    assert_eq!(found.len(), 1, "one selected Generation: {found:?}");
    let selected = found.pop().unwrap();
    let selector = fs::read_to_string(&selected).expect("read selected Generation");
    let identity = selector
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// recursa-generation:"))
        .expect("selector names its generation");
    let root = selected
        .parent()
        .unwrap()
        .join("recursa-generations")
        .join(identity)
        .join("root.rs");
    fs::read_to_string(&root).expect("read the selected root")
}

fn copy(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy generated grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir, debug_directory: Option<&Path>) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", package.path().join("target"));
    match debug_directory {
        Some(directory) => command.env("RECURSA_DEBUG_DIR", directory),
        None => command.env_remove("RECURSA_DEBUG_DIR"),
    };
    command.output().expect("run generated grammar fixture")
}

fn find_one_debug_index(directory: &Path) -> PathBuf {
    let mut found = Vec::new();
    find_named(directory, "index", &mut found);
    assert_eq!(found.len(), 1, "one generated-code debug index: {found:?}");
    found.pop().unwrap()
}

fn find_named(directory: &Path, name: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            find_named(&path, name, found);
        } else if path.file_name().is_some_and(|file| file == name) {
            found.push(path);
        }
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
