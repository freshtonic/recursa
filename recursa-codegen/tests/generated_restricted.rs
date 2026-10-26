//! A restricted expression proves itself through the generated parser, not through the lowered
//! rules alone: `a between b and c` closes its `BETWEEN` clause at the `and` that the boolean
//! operator would otherwise consume, and `a between (b and c) and d` reaches boolean `and`
//! through the parenthesized atom, which keeps the full language exactly as `gram.y`'s
//! `b_expr: '(' a_expr ')'` does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// The source-analysis fixture whose grammar this test compiles and runs.
const FIXTURE: &str = "tests/fixtures/source-analysis/restricted";

#[test]
fn a_downstream_crate_parses_a_restricted_interior_operand() {
    let package = scaffold(None);
    let output = run(&package);
    assert!(
        output.status.success(),
        "restricted fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "restricted-ok"
    );
}

/// `Arbitrary` samples only what a restricted site derives, and `Pretty` writes the parentheses
/// that make a value the site refuses parse there (`docs/declared-precedence.md`, "Consumers").
#[test]
fn a_restricted_site_samples_and_renders_only_values_that_parse_back() {
    let package = scaffold(Some(SAMPLING_MAIN));
    let output = run(&package);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "restricted sampling fixture failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    for expected in [
        // The restriction refuses `And`, and admits the parenthesized atom.
        "repair \"a between (b and c) and d\"\n",
        // The restriction applies again on the retyped edge of an admitted `Add`.
        "repair \"a between b + (c and e) and d\"\n",
        // The interior operand of the atom keeps the full language.
        "repair \"a between (b and c) + d and e\"\n",
        // `Action` refuses the parenthesized atom, so Pretty cannot repair a value it refuses
        // and keeps the rendering it had. `Arbitrary` never samples such a value.
        "kept \"do a and b\"\n",
        "restricted-sampling-ok\n",
    ] {
        assert!(stdout.contains(expected), "{expected}\n{stdout}");
    }
}

fn scaffold(main: Option<&str>) -> TempDir {
    let package = tempfile::tempdir().expect("temporary restricted grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let grammar = fs::read_to_string(manifest_dir.join(FIXTURE).join("grammar.rs"))
        .expect("read the restricted grammar fixture");
    let grammar = if main.is_some() {
        let configured = grammar.replace(
            "    keyword_matching = sensitive,\n}",
            "    keyword_matching = sensitive,\n    derives(Pretty, Arbitrary),\n}",
        );
        assert_ne!(
            configured, grammar,
            "the grammar configuration takes the derives"
        );
        configured
    } else {
        grammar
    };
    let features = if main.is_some() {
        ", features = [\"arbitrary\", \"spans\"]"
    } else {
        ""
    };

    fs::create_dir_all(package.path().join("src")).expect("create package source directory");
    fs::write(
        package.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "recursa-generated-restricted"
version = "0.0.0"
edition = "2024"
rust-version = "1.88"
build = "build.rs"

[dependencies]
recursa = {{ path = {:?}{features} }}

[build-dependencies]
recursa-codegen = {{ path = {:?} }}

[workspace]
"#,
            workspace.join("recursa"),
            workspace.join("recursa-codegen"),
        ),
    )
    .expect("write package manifest");
    fs::write(
        package.path().join("build.rs"),
        "fn main() -> Result<(), recursa_codegen::GenerateError> {\n    \
         recursa_codegen::generate(\"src/restricted.rs\")?;\n    Ok(())\n}\n",
    )
    .expect("write build script");
    fs::write(package.path().join("src/restricted.rs"), grammar)
        .expect("write the restricted grammar");
    fs::write(package.path().join("src/main.rs"), main.unwrap_or(MAIN))
        .expect("write the behaviour executable");
    package
}

/// The downstream behaviour the restriction must produce.
const MAIN: &str = r#"pub mod restricted;

fn parse(source: &str) -> restricted::Expr<'_> {
    let lexed = restricted::lex(source);
    assert!(lexed.errors().next().is_none(), "{source} lexes");
    let mut input = lexed.input();
    let parsed = restricted::Expr::parse(&mut input)
        .unwrap_or_else(|error| panic!("{source} parses: {error}"));
    assert!(input.is_eof(), "{source} is fully consumed");
    parsed.into_ast()
}

fn name<'a>(expression: &'a restricted::Expr<'_>) -> &'a str {
    let restricted::Expr::Name(name) = expression else {
        panic!("expected a name");
    };
    name.text.text()
}

fn main() {
    // The interior operand stops at `and`, which closes the clause instead of opening a
    // boolean operand: without the restriction this is a reduce/reduce conflict.
    let restricted::Expr::Between(low, middle, high) = parse("a between b and c") else {
        panic!("between is the root");
    };
    assert_eq!(name(&low), "a");
    assert_eq!(name(&middle), "b");
    assert_eq!(name(&high), "c");

    // The parenthesized atom keeps the full language, so boolean `and` is reachable inside it.
    let restricted::Expr::Between(low, middle, high) = parse("a between (b and c) and d") else {
        panic!("between is the root of the grouped form");
    };
    assert_eq!(name(&low), "a");
    let restricted::Expr::Parenthesized(inner) = *middle else {
        panic!("the restricted operand is the parenthesized atom");
    };
    let restricted::Expr::And(left, right) = *inner else {
        panic!("boolean AND is reachable inside the parenthesized atom");
    };
    assert_eq!(name(&left), "b");
    assert_eq!(name(&right), "c");
    assert_eq!(name(&high), "d");

    // The restriction removes nothing from the base: boolean `and` still parses on its own, and
    // `+` still parses inside the restricted operand.
    assert!(matches!(parse("a and b"), restricted::Expr::And(_, _)));
    let restricted::Expr::Between(_, middle, _) = parse("a between b + c and d") else {
        panic!("between is the root of the arithmetic form");
    };
    assert!(matches!(*middle, restricted::Expr::Add(_, _)));

    println!("restricted-ok");
}
"#;

/// The downstream sampling and rendering a restriction must produce.
const SAMPLING_MAIN: &str = r#"pub mod restricted;

use recursa::{Arbitrary, Pretty};

fn parse(source: &str) -> restricted::Expr<'_> {
    let lexed = restricted::lex(source);
    assert!(lexed.errors().next().is_none(), "{source} lexes");
    let mut input = lexed.input();
    let parsed = restricted::Expr::parse(&mut input)
        .unwrap_or_else(|error| panic!("{source} parses: {error}"));
    assert!(input.is_eof(), "{source} is fully consumed");
    parsed.into_ast()
}

fn parse_do(source: &str) -> restricted::Do<'_> {
    let lexed = restricted::lex(source);
    assert!(lexed.errors().next().is_none(), "{source} lexes");
    let mut input = lexed.input();
    let parsed = restricted::Do::parse(&mut input)
        .unwrap_or_else(|error| panic!("{source:?} parses: {error}"));
    assert!(input.is_eof(), "{source:?} is fully consumed");
    assert_eq!(parsed.render(), source, "{source:?} is a fixpoint");
    parsed.into_ast()
}

/// A value of `BExpr`: an admitted variant whose retyped operands are `BExpr` again.
fn in_b_expr(expression: &restricted::Expr<'_>) -> bool {
    use restricted::Expr;
    match expression {
        Expr::Name(_) | Expr::Parenthesized(_) => true,
        Expr::Add(left, right) => in_b_expr(left) && in_b_expr(right),
        Expr::And(..) | Expr::Between(..) => false,
    }
}

/// A value of the `leading` restriction `Action`: only the leading operand is retyped.
fn in_action(expression: &restricted::Expr<'_>) -> bool {
    use restricted::Expr;
    match expression {
        Expr::Name(_) => true,
        Expr::Add(left, _) => in_action(left),
        Expr::Parenthesized(_) | Expr::And(..) | Expr::Between(..) => false,
    }
}

fn check(expression: &restricted::Expr<'_>) {
    use restricted::Expr;
    match expression {
        Expr::Name(_) => {}
        Expr::Parenthesized(inner) => check(inner),
        Expr::And(left, right) | Expr::Add(left, right) => {
            check(left);
            check(right);
        }
        Expr::Between(low, middle, high) => {
            assert!(in_b_expr(middle), "a sampled BETWEEN operand is a BExpr");
            check(low);
            check(middle);
            check(high);
        }
    }
}

fn fixpoint(rendered: &str) {
    assert_eq!(parse(rendered).render(), rendered, "{rendered:?} is a fixpoint");
}

fn entropy(seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut bytes = Vec::with_capacity(64);
    while bytes.len() < 64 {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        bytes.extend_from_slice(&z.to_le_bytes());
    }
    bytes
}

fn main() {
    use restricted::Expr;
    let boxed = |source: &'static str| Box::new(parse(source));
    for value in [
        Expr::Between(boxed("a"), Box::new(Expr::And(boxed("b"), boxed("c"))), boxed("d")),
        Expr::Between(
            boxed("a"),
            Box::new(Expr::Add(boxed("b"), Box::new(Expr::And(boxed("c"), boxed("e"))))),
            boxed("d"),
        ),
        Expr::Between(
            boxed("a"),
            Box::new(Expr::Add(boxed("(b and c)"), boxed("d"))),
            boxed("e"),
        ),
    ] {
        let rendered = value.render();
        fixpoint(&rendered);
        println!("repair {rendered:?}");
    }
    let kept = restricted::Do::One(Box::new(Expr::And(boxed("a"), boxed("b"))));
    println!("kept {:?}", kept.render());

    let mut checked = 0_usize;
    for seed in 0..2_000_u64 {
        let bytes = entropy(seed);
        let mut input = recursa::__private::arbitrary::Unstructured::new(&bytes);
        if let Ok(value) = <Expr<'static> as Arbitrary>::arbitrary(&mut input) {
            check(&value);
            fixpoint(&value.render());
            checked += 1;
        }
        let mut input = recursa::__private::arbitrary::Unstructured::new(&bytes);
        if let Ok(value) = <restricted::Do<'static> as Arbitrary>::arbitrary(&mut input) {
            match &value {
                restricted::Do::One(action) => {
                    assert!(in_action(action), "a sampled action is an Action");
                    check(action);
                }
                restricted::Do::List(inner) => check(inner),
            }
            parse_do(&value.render());
            checked += 1;
        }
    }
    assert!(checked > 1_000, "the sampler yields values: {checked}");
    println!("restricted-sampling-ok");
}
"#;

fn run(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", "")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run the restricted grammar fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}
