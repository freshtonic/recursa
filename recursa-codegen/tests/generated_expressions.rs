//! End-to-end proof of `docs/declared-precedence.md` through a generated parser: a prefix
//! override taken from a precedence-only name, left, right and nonassociative infix levels, a
//! postfix operator, an interior operand behind a restricted expression, an `ESCAPE`-style
//! keyword one level above its operator, and the `RCA0400` build error an undeclared operator
//! earns.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// The generation fixture this test scaffolds, runs, and mutates.
const FIXTURE: &str = "tests/fixtures/generation/expressions";

/// The level the variant grammar drops so that `STAR` has no declared precedence.
const STAR_LEVEL: &str = "        left(bp = 30) { STAR },\n";

#[test]
fn the_expressions_fixture_decides_every_operator_by_its_declared_level() {
    let package = scaffold(FIXTURE, None);
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("expressions-ok\n"), "{stdout}");
    let lines = parser_lines(&stdout);

    // A prefix override through a precedence-only name. `MINUS` is the rule's only terminal, so
    // without `#[parse(prec = UMINUS)]` the prefix form would sit at the additive level and
    // negate the product instead of multiplying the negation.
    for expected in [
        "ok \"- a * b\" => mul(neg(a), b) eof=true\n",
        "ok \"- a + b\" => add(neg(a), b) eof=true\n",
        // `UMINUS` is below `BANG` and above `CARET`, exactly as its declared position says.
        "ok \"- a ^ b\" => pow(neg(a), b) eof=true\n",
        "ok \"- a !\" => neg(fac(a)) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // Left, right and nonassociative infix levels, and a postfix operator above all of them.
    for expected in [
        "ok \"a + b + c\" => add(add(a, b), c) eof=true\n",
        "ok \"a + b * c\" => add(a, mul(b, c)) eof=true\n",
        "ok \"a ^ b ^ c\" => pow(a, pow(b, c)) eof=true\n",
        "ok \"a = b + c\" => eq(a, add(b, c)) eof=true\n",
        "ok \"a !\" => fac(a) eof=true\n",
        "ok \"a ^ b !\" => pow(a, fac(b)) eof=true\n",
        "ok \"a ! !\" => fac(fac(a)) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }
    // A nonassociative tie is an error action, not a parse.
    assert!(
        lines.contains("err \"a = b = c\" => code=RCA4100 kind=UnexpectedToken"),
        "{lines}"
    );
    assert!(
        lines.contains("err \"a like b like c\" => code=RCA4100 kind=UnexpectedToken"),
        "{lines}"
    );

    // An interior operand through a restricted expression: the `and` of a `between` clause
    // closes it, and the parenthesized atom keeps the full language, as `b_expr` does.
    for expected in [
        "ok \"a and b\" => and(a, b) eof=true\n",
        "ok \"a between b and c\" => between(a, b, c) eof=true\n",
        "ok \"a between b + c and d\" => between(a, add(b, c), d) eof=true\n",
        "ok \"a between ( b and c ) and d\" => between(a, paren(and(b, c)), d) eof=true\n",
        // `#[parse(prec = BETWEEN)]` keeps the completed clause above boolean `and`.
        "ok \"a between b and c and d\" => and(between(a, b, c), d) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // `ESCAPE` one level above `LIKE`: the keyword is shifted out of the completed comparison,
    // while a token below `LIKE` reduces it first.
    for expected in [
        "ok \"a like b escape c\" => like(a, b, escape c) eof=true\n",
        "ok \"a like b\" => like(a, b) eof=true\n",
        "ok \"a like b and c\" => and(like(a, b), c) eof=true\n",
        "ok \"a like b + c escape d\" => like(a, add(b, c), escape d) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }
    assert!(
        lines.contains("err \"a like b escape\" => code=RCA4101 kind=UnexpectedEof"),
        "{lines}"
    );
    assert!(
        lines.contains("err \"a between b and\" => code=RCA4101 kind=UnexpectedEof"),
        "{lines}"
    );

    // The runtime `tail` flag. An operator rule is a left-recursive continuation of its own
    // Node and clears the flag, so a token outside the root FOLLOW ends the parse instead of
    // failing it; a one-token atom sets the flag, so the same token is an error there.
    assert!(
        lines.contains("ok \"a + b c\" => add(a, b) eof=false\n"),
        "{lines}"
    );
    assert!(
        lines.contains("err \"1 2\" => code=RCA4100 kind=UnexpectedToken"),
        "{lines}"
    );

    // Pretty writes what a parse of its rendering needs (`docs/declared-precedence.md`,
    // "Consumers"), and each rendering parses back to the value: the parentheses of a value that
    // a restricted site refuses, also on the retyped edge of an admitted variant; the parentheses
    // that the level of the token after a first operand needs (a rule's own level still adds
    // the parentheses it added before, so `a + (b* any(q))` keeps them); and a space between two
    // texts that the lexer would join. Every sampled value renders as text that parses back to the
    // same text.
    for expected in [
        "repair \"a between (b and c) and d\"\n",
        "repair \"a between b + (c and e) and d\"\n",
        "repair \"(a + b)* any(q)\"\n",
        "repair \"a + (b* any(q))\"\n",
        "repair \"a like (b= any(q))\"\n",
        "repair \"(a like b)= any(q)\"\n",
        "repair \"@ @a\"\n",
        "repair \"@@a\"\n",
        "repair \"- -a\"\n",
        "arbitrary round trip ok\n",
    ] {
        assert!(stdout.contains(expected), "{expected}\n{stdout}");
    }

    // A field of a Node with no level, before a token with a level: Pretty parenthesizes a
    // source that the token can continue and that is below the token's level, and no other.
    for expected in [
        "followed \"search((a like b) escape c)\"\n",
        "followed \"search(a + b escape c)\"\n",
        "followed \"search(a like b using c)\"\n",
        "followed \"find((a like b) escape c)\"\n",
        "followed round trip ok\n",
    ] {
        assert!(stdout.contains(expected), "{expected}\n{stdout}");
    }
}

#[test]
fn an_undeclared_operator_is_an_rca0400_build_error_with_an_authoring_remedy() {
    let grammar = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(FIXTURE)
            .join("src/grammar.rs"),
    )
    .expect("read the expressions grammar");
    assert_eq!(grammar.matches(STAR_LEVEL).count(), 1, "{grammar}");
    let undeclared = grammar.replace(STAR_LEVEL, "");

    let package = scaffold(FIXTURE, Some(&undeclared));
    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "an undeclared operator must fail the build\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[RCA0400]"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("shift/reduce conflict"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("on lookahead `STAR`"), "stderr:\n{stderr}");
    // The remedy is an authoring instruction, not an internal-invariant report.
    assert!(
        stderr.contains("declare a precedence level for the lookahead token"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("add `#[parse(prec = NAME)]` to the rule that reduces"),
        "stderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9105"), "stderr:\n{stderr}");
}

#[test]
fn a_level_less_variant_leaves_no_self_comparison_for_a_downstream_lint() {
    // Every atom of an expression Node has no declared level, so its Pretty body never
    // parenthesizes. The test must fold to the literal `false`: the two-sided spelling
    // `false || false` trips `clippy::eq_op`, which is deny-by-default, and so fails every
    // downstream crate whose expression enum has an atom.
    let package = scaffold(FIXTURE, None);
    let output = lint_fixture(&package);
    assert!(
        output.status.success(),
        "generated downstream code must be warning-free under clippy\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let generated = generated_sources(&package.path().join("target"));
    assert!(
        !generated.is_empty(),
        "the fixture publishes its generated Rust"
    );
    assert!(
        generated
            .iter()
            .any(|source| source.contains("__recursa_parenthesize = false ;")),
        "a level-less variant folds its parenthesization test to `false`",
    );
    for source in &generated {
        assert!(
            !source.contains("false || false"),
            "no generated parenthesization test compares a literal with itself",
        );
    }
}

/// Reads every generated Rust artifact the fixture published under `root`.
fn generated_sources(root: &Path) -> Vec<String> {
    let mut sources = Vec::new();
    collect_generated_sources(root, &mut sources);
    sources
}

/// Walks `root` for the published `recursa-generations` Rust artifacts.
fn collect_generated_sources(root: &Path, sources: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_generated_sources(&path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path
                .components()
                .any(|component| component.as_os_str() == "recursa-generations")
        {
            sources.push(fs::read_to_string(&path).expect("read a generated Rust artifact"));
        }
    }
}

/// Lints the scaffolded fixture the way a downstream crate's own CI does.
fn lint_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("clippy")
        .arg("--offline")
        .arg("--locked")
        .arg("--all-targets")
        .arg("--message-format=short")
        .arg("--")
        .arg("-D")
        .arg("warnings")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", package.path().join("target"))
        .output()
        .expect("lint the generated grammar fixture")
}

#[test]
fn expression_variants_render_as_the_rules_the_author_wrote() {
    // A variant of an expression Node is an ordinary enum alternative, so its diagram is its
    // rule under its own name. No `atom`/`prefix`/`infix`/`postfix` role label and no binding
    // power is rendered, and the attachment units that once carried an operator are inlined,
    // so no private `__recursa_attachment` nonterminal reaches the diagram either.
    let package = scaffold(FIXTURE, None);
    configure_diagrams(&package);
    let output = document_fixture(&package);
    assert_success(&output);

    let expression = rendered_rustdoc(&package)
        .into_iter()
        .find(|(path, source)| {
            path.ends_with("grammar/enum.Expr.html") && source.contains("RECURSA_NODE_DIAGRAM")
        })
        .expect("the expression Node has a documentation page");
    let diagram = diagram_svg(&expression.1);

    for expected in [
        // The author wrote `#[tok(MINUS, this)] boxed!(Self)`; the rule is `- Expr`.
        vec!["Negate", "-", "Expr"],
        vec!["Factorial", "Expr", "!"],
        vec!["Power", "Expr", "^", "Expr"],
        vec!["Parenthesized", "(", "Expr", ")"],
        // A restricted operand renders as the base type its rules construct.
        vec!["Between", "Expr", "between", "Expr", "and", "Expr"],
    ] {
        assert!(
            contains_text_sequence(diagram, &expected),
            "{expected:?} is the rule the author wrote:\n{}",
            expression.1,
        );
    }
    for role in ["atom", "prefix", "infix", "postfix", "bp=", "lbp=", "rbp="] {
        assert!(
            !diagram.contains(role),
            "a variant renders as its rule, with no `{role}` label:\n{}",
            expression.1,
        );
    }
    assert!(
        !expression.1.contains("__recursa_attachment"),
        "an inlined attachment unit never reaches a diagram:\n{}",
        expression.1,
    );
}

/// Selects the one SVG of a rendered documentation page.
fn diagram_svg(document: &str) -> &str {
    let start = document.find("<svg ").expect("diagram SVG start");
    let end = document[start..]
        .find("</svg>")
        .map(|offset| start + offset + "</svg>".len())
        .expect("diagram SVG end");
    &document[start..end]
}

/// Reports whether the SVG's text nodes contain `expected` as a consecutive run.
fn contains_text_sequence(svg: &str, expected: &[&str]) -> bool {
    let document = roxmltree::Document::parse(svg).expect("diagram is valid XML");
    let text = document
        .descendants()
        .filter(|node| node.has_tag_name("text"))
        .filter_map(|node| node.text())
        .collect::<Vec<_>>();
    text.windows(expected.len())
        .any(|window| window == expected)
}

/// Requests diagrams from the scaffolded fixture and selects its diagrams-capable lock.
fn configure_diagrams(package: &TempDir) {
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read the scaffolded grammar");
    fs::write(
        grammar,
        source.replace(
            "    module = crate::grammar,",
            "    module = crate::grammar,\n    diagrams,",
        ),
    )
    .expect("request diagrams");

    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read the scaffolded manifest");
    fs::write(
        manifest,
        source.replace("\"spans\"]", "\"spans\", \"diagrams\"]"),
    )
    .expect("enable the diagram capability");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(FIXTURE)
            .join("Cargo.lock"),
        package.path().join("Cargo.lock"),
    )
    .expect("select the diagrams-capable fixture lock");
}

/// Renders the scaffolded fixture's documentation.
fn document_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("doc")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .arg("--no-deps")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("RUSTDOCFLAGS", "-D rustdoc::broken_intra_doc_links")
        .env("CARGO_TARGET_DIR", package.path().join("target"))
        .output()
        .expect("render the generated grammar fixture documentation")
}

/// Reads every rendered documentation page the fixture produced.
fn rendered_rustdoc(package: &TempDir) -> Vec<(PathBuf, String)> {
    let mut rendered = Vec::new();
    collect_rendered_rustdoc(&package.path().join("target/doc"), &mut rendered);
    rendered
}

/// Walks `root` for rendered documentation pages.
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
            let source = fs::read_to_string(&path).expect("read a rendered documentation page");
            rendered.push((path, source));
        }
    }
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

/// Selects the generated parser's tagged fixture lines and terminates each with a newline so
/// the assertions can match whole lines.
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
