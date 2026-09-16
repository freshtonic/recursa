//! End-to-end proof of flat Pretty through a generated grammar: one `FlatPretty` implementation
//! per flat Node type, the container dispatch its fields use, and the `pretty`, `render`,
//! `render_with` and `pretty_node` surface of the `FlatAst` owner.
//!
//! The seam under test is `FlatAst::render` compared against the render of a detached nested AST
//! of the same input. The fixture binary parses every container shape of the grammar twice, prints
//! what each representation rendered, and asserts the two strings are equal; it also proves the
//! rendering round trip, the caller-selected width, and rendering from an interior handle. The
//! assertions below pin one rendered line per container shape, so a drift in either renderer fails
//! here rather than only inside the fixture. The mutation cases replace the grammar to cover
//! declared-precedence parenthesization and a grammar that selects `arena_ast` beside `flat`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// The generation fixture this test scaffolds, runs, and mutates.
const FIXTURE: &str = "tests/fixtures/generation/flat-pretty";

#[test]
fn a_flat_ast_renders_exactly_what_a_detached_nested_ast_renders() {
    let package = scaffold(FIXTURE, None);
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("flat-pretty-ok\n"), "{stdout}");

    // The fixture asserts nested and flat equality for every positive input. These lines pin what
    // the two agreed on, one per container shape, so the pair cannot drift together.
    for expected in [
        // An envelope presence, an erased optional group that neither representation can render
        // without provenance, a `boxed!` child, and an inlined optional Node.
        "let if not exists x= 2 as y;",
        // A zero-or-more list with a trailing separator the render drops, and a one-or-more list
        // whose separator it keeps.
        "f(1, 2);",
        "with recursive a| b;",
        "trail a, b;",
        // A list whose following optional tail owns the same separator.
        "vtail 1, 2, variadic 3;",
        // Inlined optional Nodes and a helper optional child.
        "fn as(in x, out only, temp) returns 1;",
        // A recursive Node through a nested statement list.
        "{print 1; {let if exists x= 2;} f();}",
        // Presences that are true and false, inlined and behind helpers.
        "mods four six x;",
        // Erased optional groups leave nothing to render, however many of them occurred.
        "erased x;",
        // Five optionals of fixed-token unit Nodes, which the parent allocated.
        "options as in out only temp;",
        // A helper zero-or-more list and a helper optional one-or-more list.
        "marks: ! ! ! ;",
        // A one-or-more list without a separator and a repeated attachment with an erased group.
        "filter! ! : 1 : 2;",
        // A list inside a list item, and a per-element attachment.
        "print[[], [1, [2]]];",
        "print(1)| (as)| ([]);",
        // Content-token text comes out of the text pool, including a keyword an admission set
        // reclaims as a name.
        "boxes 1, as, [2];",
    ] {
        for renderer in ["pretty", "flat-pretty"] {
            let prefix = format!("{renderer} \"");
            assert!(
                stdout
                    .lines()
                    .any(|line| line.starts_with(&prefix) && line.ends_with(expected)),
                "no {renderer} line renders as {expected:?}\n{stdout}",
            );
        }
    }

    // `render_with` honors a caller-selected width: the same statement that fits on one line under
    // the grammar's own width breaks under a narrow one, and the broken render still reparses.
    assert!(
        stdout.contains(
            "flat-pretty-width \"{ print 1; { let if exists x = 2; } f(); }\" => \
             \"{\\n  print 1;\\n  {\\n    let\\n    if\\n    exists x= 2;\\n  }\\n  f();\\n}\"\n"
        ),
        "{stdout}"
    );

    // `pretty_node` renders one interior Node of a larger result, at two depths.
    assert!(
        stdout.contains("flat-pretty-node \"print [1, [as, only]];\" => [1, [as, only]]\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("flat-pretty-node inner => [as, only]\n"),
        "{stdout}"
    );

    // A Flat AST rooted at a Node other than the grammar root renders from that root.
    for expected in [
        "flat-pretty-entry value => [1, [as, only]]\n",
        "flat-pretty-entry params => (in x, out temp)\n",
        "flat-pretty-entry name => only\n",
    ] {
        assert!(stdout.contains(expected), "{expected}\n{stdout}");
    }
}

#[test]
fn flat_pretty_parenthesizes_a_declared_precedence_expression_as_the_nested_renderer_does() {
    // Declared precedence is the one Pretty decision that reads a child's variant rather than its
    // own elements, so a flat operand must be resolved through the store before the decision can
    // be made. The fixture asserts the two renders are equal for every input; the lines below pin
    // the parenthesization each one agreed on.
    let package = scaffold(FIXTURE, Some(PRECEDENCE_GRAMMAR));
    fs::write(package.path().join("src/main.rs"), PRECEDENCE_DRIVER)
        .expect("write the precedence consumer");
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("flat-precedence-ok\n"), "{stdout}");
    for expected in [
        // Grouping the parse did not need is not synthesized.
        "prec \"1 + 2 * 3\" => 1 + 2 * 3 | 1 + 2 * 3\n",
        "prec \"1 + 2 + 3\" => 1 + 2 + 3 | 1 + 2 + 3\n",
        // Grouping the semantics require is synthesized, around an infix, a prefix and a postfix
        // operand.
        "prec \"(1 + 2) * 3\" => (1 + 2) * 3 | (1 + 2) * 3\n",
        "prec \"-(1 + 2)\" => -(1 + 2) | -(1 + 2)\n",
        "prec \"(1 + 2)!\" => (1 + 2)! | (1 + 2)!\n",
        "prec \"1 * (2 + 3) * 4\" => 1 * (2 + 3) * 4 | 1 * (2 + 3) * 4\n",
    ] {
        assert!(stdout.contains(expected), "{expected}\n{stdout}");
    }
}

#[test]
fn a_grammar_with_both_arena_ast_and_flat_renders_either_representation() {
    // The two backends own separate stores, and each has its own renderer: the nested one walks
    // arena-allocated children, the flat one resolves handles. This grammar selects both, so both
    // renderers must be generated and must agree.
    let package = scaffold(FIXTURE, Some(ARENA_FLAT_GRAMMAR));
    fs::write(package.path().join("src/main.rs"), ARENA_FLAT_DRIVER)
        .expect("write the arena and flat consumer");
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("arena-flat-pretty-ok\n"), "{stdout}");
    assert!(
        stdout.contains("arena => let not x= [a, ! b];\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("flat => let not x= [a, ! b];\n"),
        "{stdout}"
    );
}

#[test]
fn a_generic_pooled_node_renders_at_every_application() {
    // A generic Node's rendering implementation is emitted once and covers every application, so
    // it carries the predicates its own parameters owe the store: a child of a parameter type is
    // reached through a handle, which only the store can resolve.
    let package = scaffold(FIXTURE, Some(GENERIC_FLAT_GRAMMAR));
    fs::write(package.path().join("src/main.rs"), GENERIC_FLAT_DRIVER)
        .expect("write the generic consumer");
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("generic-flat-pretty-ok\n"), "{stdout}");
    assert!(
        stdout.contains("generic => pair[a, b][1, 2]; | pair[a, b][1, 2];\n"),
        "both applications render, and as the nested renderer does\n{stdout}"
    );
    assert!(
        stdout.contains("generic entry => [x, y]\n"),
        "a generic application renders from its own entry point\n{stdout}"
    );
}

/// A grammar whose pooled Node is generic and materialized at two arguments, with Pretty on.
const GENERIC_FLAT_GRAMMAR: &str = r#"//! A generic Node materialized at two arguments, with `flat` and Pretty on.

recursa::grammar! {
    module = crate::grammar,
    flat,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords { PAIR => "pair" }
    punctuation { COMMA => ",", LBRACKET => "[", RBRACKET => "]", SEMI => ";" }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(PAIR, this, SEMI)]
    pub struct Root {
        pub names: Holder<Name>,
        pub numbers: Holder<Number>,
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[flat(pool)]
    #[tok(LBRACKET, this, RBRACKET)]
    pub struct Holder<T> {
        #[sep(COMMA)]
        pub values: one_or_many!(T),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}
"#;

/// The consumer that renders both applications of the generic grammar's pooled Node.
const GENERIC_FLAT_DRIVER: &str = r#"//! Proves flat Pretty renders a generic Node materialized twice.

use recursa::Pretty as _;
use recursa_generated_flat_pretty::grammar;

fn main() {
    let source = "pair [a, b] [1, 2];";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let parsed = grammar::Root::parse(&mut input).expect("the nested parse succeeds");
    let nested = parsed.into_ast().render();

    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Root::parse_flat(&mut input).expect("the flat parse succeeds");
    let rendered = flat.render();
    println!("generic => {nested} | {rendered}");
    assert_eq!(nested, rendered, "every application renders as the nested one does");

    let lexed = grammar::lex("[x, y]");
    let mut input = lexed.input();
    let holder = grammar::Holder::<grammar::Name>::parse_flat(&mut input)
        .expect("the generic entry point parses");
    println!("generic entry => {}", holder.render());
    println!("generic-flat-pretty-ok");
}
"#;

#[test]
fn a_node_declared_without_pub_renders_from_its_own_module() {
    // A rendering implementation is emitted beside its declaration, at the declaration's own
    // visibility, because the grammar root cannot name a Node declared without `pub`. This
    // grammar hides one Node in a private module and renders it through its public parent.
    let package = scaffold(FIXTURE, Some(PRIVATE_NODE_GRAMMAR));
    fs::write(package.path().join("src/main.rs"), PRIVATE_NODE_DRIVER)
        .expect("write the private-Node consumer");
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("private-flat-pretty-ok\n"), "{stdout}");
    assert!(
        stdout.contains("private => show a, b; | show a, b;\n"),
        "a private Node renders in both representations\n{stdout}"
    );
}

/// A grammar whose child Node is declared without `pub`, with `flat` and Pretty on.
const PRIVATE_NODE_GRAMMAR: &str = r#"//! A Node declared without `pub`, with `flat` and Pretty on.

recursa::grammar! {
    module = crate::grammar,
    flat,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords { SHOW => "show" }
    punctuation { SEMI => ";", COMMA => "," }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(SHOW, this, SEMI)]
    pub struct Root {
        #[sep(COMMA)]
        pub items: one_or_many!(private_nodes::Hidden),
    }
}

mod private_nodes {
    recursa::ast_node! {
        #[derive(Debug)]
        pub(super) struct Hidden {
            #[lex(pattern = r"[a-z]+")]
            pub(super) text: HiddenText,
        }
    }
}
"#;

/// The consumer that renders a grammar whose child Node is private.
const PRIVATE_NODE_DRIVER: &str = r#"//! Proves a Node declared without `pub` renders in both representations.

use recursa::Pretty as _;
use recursa_generated_flat_pretty::grammar;

fn main() {
    let source = "show a, b;";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let parsed = grammar::Root::parse(&mut input).expect("the nested parse succeeds");
    let nested = parsed.into_ast().render();

    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Root::parse_flat(&mut input).expect("the flat parse succeeds");
    let rendered = flat.render();
    println!("private => {nested} | {rendered}");
    assert_eq!(nested, rendered, "a private Node renders alike in both representations");
    println!("private-flat-pretty-ok");
}
"#;

/// A declared-precedence expression grammar that selects `flat` and Pretty.
const PRECEDENCE_GRAMMAR: &str = r#"//! A declared-precedence expression grammar with `flat` and Pretty.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    flat,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation {
        PLUS => "+",
        STAR => "*",
        MINUS => "-",
        BANG => "!",
        LPAREN => "(",
        RPAREN => ")",
    }
    precedence {
        left(bp = 10) { PLUS },
        left(bp = 20) { STAR },
        right(bp = 30) { UMINUS },
        left(bp = 40) { BANG },
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[flat(pool)]
    pub enum Expr {
        Number(Number),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),
        #[parse(prec = UMINUS)]
        Negate(#[tok(MINUS, this)] boxed!(Self)),
        Factorial(#[tok(this, BANG)] boxed!(Self)),
        Multiply(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}
"#;

/// The consumer that renders one declared-precedence expression through both representations.
const PRECEDENCE_DRIVER: &str = r#"//! Proves flat Pretty parenthesizes a declared-precedence expression as the nested renderer does.

use recursa::Pretty as _;
use recursa_generated_flat_pretty::grammar;

const INPUTS: &[&str] = &[
    "1 + 2 * 3",
    "(1 + 2) * 3",
    "-1 * 2",
    "-(1 + 2)",
    "1 + 2 + 3",
    "(1 + 2)!",
    "1 * (2 + 3) * 4",
];

fn main() {
    for source in INPUTS {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::Expr::parse(&mut input).expect("the input parses");
        let nested = parsed.into_ast().render();

        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let flat = grammar::Expr::parse_flat(&mut input).expect("the input parses flat");
        let rendered = flat.render();
        println!("prec {source:?} => {nested} | {rendered}");
        assert_eq!(
            rendered, nested,
            "flat parenthesization matches nested: {source:?}",
        );

        let lexed = grammar::lex(&rendered);
        let mut input = lexed.input();
        let again = grammar::Expr::parse_flat(&mut input).expect("the render reparses");
        assert_eq!(again.render(), rendered, "flat rendering is idempotent");
    }
    println!("flat-precedence-ok");
}
"#;

/// A grammar that selects `arena_ast`, `flat` and Pretty together, with one pooled Node.
const ARENA_FLAT_GRAMMAR: &str = r#"//! A grammar with `flat`, `arena_ast` and Pretty.

recursa::grammar! {
    module = crate::grammar,
    flat,
    arena_ast,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords { LET => "let", NOT => "not" }
    punctuation {
        EQ => "=",
        SEMI => ";",
        COMMA => ",",
        LBRACKET => "[",
        RBRACKET => "]",
        BANG => "!",
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(LET, this, SEMI)]
    pub struct Root {
        #[presence(NOT)]
        pub negated: bool,
        pub name: Name,
        #[tok(EQ)]
        pub value: boxed!(Value),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[flat(pool)]
    pub enum Value {
        Name { name: Name },
        Negated { mark: Bang, inner: boxed!(Value) },
        List(List),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(LBRACKET, this, RBRACKET)]
    pub struct List {
        #[sep(COMMA)]
        pub items: zero_or_many!(Value),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(BANG)]
    pub struct Bang;
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}
"#;

/// The consumer that renders one input through each backend of the `arena_ast` and `flat` grammar.
const ARENA_FLAT_DRIVER: &str = r#"//! Proves a grammar with `arena_ast` and `flat` renders either representation.

use recursa::Pretty as _;
use recursa_generated_flat_pretty::grammar;

fn main() {
    let source = "let not x = [a, !b];";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let parsed = grammar::Root::parse(&mut input).expect("the arena parse succeeds");
    println!("arena => {}", parsed.ast().render());

    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Root::parse_flat(&mut input).expect("the flat parse succeeds");
    println!("flat => {}", flat.render());
    println!("arena-flat-pretty-ok");
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
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
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
