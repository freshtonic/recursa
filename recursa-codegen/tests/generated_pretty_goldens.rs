use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

use quote::ToTokens as _;
use syn::punctuated::Punctuated;
use syn::{GenericArgument, Item, PathArguments, Type};

struct Golden {
    /// Fixture directory, relative to the crate manifest, holding `grammar.rs`.
    fixtures: &'static str,
    fixture: &'static str,
    module: &'static str,
    root: &'static str,
    source: &'static str,
    expected: &'static str,
    fingerprint: &'static str,
    /// Extra statements run in the golden's `main`, after the round trip.
    ///
    /// Parsed input always carries the authored grouping, so a golden that
    /// needs to observe grouping Pretty synthesized itself builds the value
    /// here instead of reading it back out of a parse.
    repair: &'static str,
}

/// Directory holding the six representative source-analysis grammars.
const SOURCE_ANALYSIS_FIXTURES: &str = "tests/fixtures/source-analysis";

/// Directory holding grammars authored only as Pretty goldens.
const PRETTY_GOLDEN_FIXTURES: &str = "tests/fixtures/pretty-goldens";

const GOLDENS: &[Golden] = &[
    Golden {
        fixtures: SOURCE_ANALYSIS_FIXTURES,
        fixture: "toy",
        module: "toy",
        root: "IdentHolder",
        source: "alpha",
        expected: "alpha",
        fingerprint: r#"
fn fingerprint(value: &toy::IdentHolder<'_>) -> String {
    format!("ident:{}", value.ident.text())
}
"#,
        repair: "",
    },
    Golden {
        fixtures: SOURCE_ANALYSIS_FIXTURES,
        fixture: "json",
        module: "json",
        root: "Value",
        source: r#"{"a":[true,null]}"#,
        expected: r#"{"a": [true, null]}"#,
        fingerprint: r#"
fn fingerprint(value: &json::Value<'_>) -> String {
    match value {
        json::Value::Object(object) => format!(
            "object({})",
            object.members.iter().map(|member| format!(
                "{}:{}",
                member.key.text(),
                fingerprint(&member.value),
            )).collect::<Vec<_>>().join(","),
        ),
        json::Value::Array(array) => format!(
            "array({})",
            array.values.iter().map(fingerprint).collect::<Vec<_>>().join(","),
        ),
        json::Value::String(text) => format!("string:{}", text.text()),
        json::Value::Number(text) => format!("number:{}", text.text()),
        json::Value::Bool(json::Bool::True) => "true".to_owned(),
        json::Value::Bool(json::Bool::False) => "false".to_owned(),
        json::Value::Null => "null".to_owned(),
    }
}
"#,
        repair: "",
    },
    Golden {
        fixtures: SOURCE_ANALYSIS_FIXTURES,
        fixture: "sql",
        module: "sql",
        root: "Statement",
        source: "CREATE TABLE thing",
        expected: "CREATE TABLE thing",
        fingerprint: r#"
fn fingerprint(value: &sql::Statement<'_>) -> String {
    match value {
        sql::Statement::CreateTable(node) => format!("create-table:{}", node.rest.text()),
        sql::Statement::CreateView(node) => format!("create-view:{}", node.rest.text()),
        sql::Statement::CreateIndex(node) => format!("create-index:{}", node.rest.text()),
        sql::Statement::CreateSchema(node) => format!("create-schema:{}", node.rest.text()),
        sql::Statement::CreateFunction(node) => format!("create-function:{}", node.rest.text()),
        sql::Statement::CreateProcedure(node) => format!("create-procedure:{}", node.rest.text()),
        sql::Statement::DropTable(node) => format!("drop-table:{}", node.rest.text()),
        sql::Statement::DropView(node) => format!("drop-view:{}", node.rest.text()),
        sql::Statement::DropIndex(node) => format!("drop-index:{}", node.rest.text()),
        sql::Statement::DropSchema(node) => format!("drop-schema:{}", node.rest.text()),
        sql::Statement::DropFunction(node) => format!("drop-function:{}", node.rest.text()),
        sql::Statement::DropProcedure(node) => format!("drop-procedure:{}", node.rest.text()),
    }
}
"#,
        repair: "",
    },
    Golden {
        fixtures: SOURCE_ANALYSIS_FIXTURES,
        fixture: "optq",
        module: "optq",
        root: "Stmt",
        source: "DEBUG PRINT thing",
        expected: "DEBUG PRINT thing",
        fingerprint: r#"
fn fingerprint(value: &optq::Stmt<'_>) -> String {
    match value {
        optq::Stmt::Print(node) => format!("print:{}:{}", node.debug.is_some(), node.rest.text()),
        optq::Stmt::Read(node) => format!("read:{}:{}", node.debug.is_some(), node.rest.text()),
        optq::Stmt::Set(node) => format!("set:{}:{}", node.debug.is_some(), node.rest.text()),
    }
}
"#,
        repair: "",
    },
    Golden {
        fixtures: SOURCE_ANALYSIS_FIXTURES,
        fixture: "cskw",
        module: "cskw",
        root: "Stmt",
        source: "select a,b from t",
        expected: "select a, b from t",
        fingerprint: r#"
fn fingerprint(value: &cskw::Stmt<'_>) -> String {
    match value {
        cskw::Stmt::Select(node) => format!(
            "select:{}:from:{}",
            node.cols.iter().map(|column| match &column.as_alias {
                Some(alias) => format!("{} as {}", column.name.text(), alias.alias.text()),
                None => column.name.text().to_owned(),
            }).collect::<Vec<_>>().join(","),
            node.table.text(),
        ),
    }
}
"#,
        repair: "",
    },
    Golden {
        fixtures: SOURCE_ANALYSIS_FIXTURES,
        fixture: "arith",
        module: "arith",
        root: "Expr",
        source: "1+2*3",
        expected: "1 + 2 * 3",
        fingerprint: r#"
fn fingerprint(value: &arith::Expr<'_>) -> String {
    match value {
        arith::Expr::IntLit(atom) => format!("int:{}", atom.value.value.text()),
        arith::Expr::Ident(atom) => format!("ident:{}", atom.value.value.text()),
        arith::Expr::Paren(inner) => format!("paren({})", fingerprint(inner)),
        arith::Expr::Neg(inner) => format!("neg({})", fingerprint(inner)),
        arith::Expr::Add(left, right) => format!("add({},{})", fingerprint(left), fingerprint(right)),
        arith::Expr::Sub(left, right) => format!("sub({},{})", fingerprint(left), fingerprint(right)),
        arith::Expr::Mul(left, right) => format!("mul({},{})", fingerprint(left), fingerprint(right)),
        arith::Expr::Div(left, right) => format!("div({},{})", fingerprint(left), fingerprint(right)),
    }
}
"#,
        repair: "",
    },
    Golden {
        fixtures: PRETTY_GOLDEN_FIXTURES,
        fixture: "selcyc",
        module: "selcyc",
        root: "Expr",
        source: "(1+2)*([select 3,4])",
        expected: "(1 + 2) * ([select 3, 4])",
        fingerprint: r#"
fn query(value: &selcyc::Query<'_>) -> String {
    match value {
        selcyc::Query::Select(select) => format!(
            "select({})",
            select.targets.iter().map(fingerprint).collect::<Vec<_>>().join(","),
        ),
        selcyc::Query::Parens(parens) => format!("parens({})", query(&parens.query)),
    }
}

fn content(value: &selcyc::ParenContent<'_>) -> String {
    match value {
        selcyc::ParenContent::Exprs(exprs) => format!(
            "exprs({})",
            exprs.iter().map(fingerprint).collect::<Vec<_>>().join(","),
        ),
        selcyc::ParenContent::Query(inner) => format!("query({})", query(inner)),
    }
}

fn fingerprint(value: &selcyc::Expr<'_>) -> String {
    match value {
        selcyc::Expr::IntLit(atom) => format!("int:{}", atom.value.text()),
        selcyc::Expr::Paren(paren) => format!("paren({})", content(&paren.content)),
        selcyc::Expr::Add(left, right) => format!("add({},{})", fingerprint(left), fingerprint(right)),
        selcyc::Expr::Mul(left, right) => format!("mul({},{})", fingerprint(left), fingerprint(right)),
    }
}

fn int(text: &'static str) -> selcyc::Expr<'static> {
    selcyc::Expr::IntLit(selcyc::IntAtom { value: selcyc::IntLit::new(text) })
}
"#,
        repair: r#"
    // The witnessless `ParenContent::Query` cycle must not cost the atom its
    // grouping: an addition under a multiplication has no authored parentheses,
    // so Pretty has to synthesize the ones `ParenContent::Exprs` witnesses.
    let repaired = selcyc::Expr::Mul(
        Box::new(selcyc::Expr::Add(Box::new(int("1")), Box::new(int("2")))),
        Box::new(int("3")),
    );
    let rendered = repaired.render();
    assert_eq!(rendered, "(1 + 2) * 3", "Pretty repairs precedence with the atom's parentheses");
    // Reparsing the repair yields the authored `Paren` node the synthesized
    // delimiters spell, so the fixed point, not node equality, is the check.
    assert_eq!(parse(&rendered).render(), rendered, "repaired grouping is idempotent");
"#,
    },
    Golden {
        fixtures: PRETTY_GOLDEN_FIXTURES,
        fixture: "declared-precedence",
        module: "declared_precedence",
        root: "Expr",
        source: "1 - 2 - 3",
        expected: "1 - 2 - 3",
        fingerprint: r#"
fn fingerprint(value: &declared_precedence::Expr<'_>) -> String {
    match value {
        declared_precedence::Expr::Integer(atom) => format!("int:{}", atom.text.text()),
        declared_precedence::Expr::Name(atom) => format!("name:{}", atom.text.text()),
        declared_precedence::Expr::Parenthesized(inner) => format!("paren({})", fingerprint(inner)),
        declared_precedence::Expr::Negate(inner) => format!("neg({})", fingerprint(inner)),
        declared_precedence::Expr::Factorial(inner) => format!("fact({})", fingerprint(inner)),
        declared_precedence::Expr::Power(left, right) => format!("pow({},{})", fingerprint(left), fingerprint(right)),
        declared_precedence::Expr::Multiply(left, right) => format!("mul({},{})", fingerprint(left), fingerprint(right)),
        declared_precedence::Expr::Add(left, right) => format!("add({},{})", fingerprint(left), fingerprint(right)),
        declared_precedence::Expr::Subtract(left, right) => format!("sub({},{})", fingerprint(left), fingerprint(right)),
        declared_precedence::Expr::Equals(left, right) => format!("eq({},{})", fingerprint(left), fingerprint(right)),
    }
}

fn int(text: &'static str) -> declared_precedence::Expr<'static> {
    declared_precedence::Expr::Integer(declared_precedence::Integer {
        text: declared_precedence::IntegerText::new(text),
    })
}

fn name(text: &'static str) -> declared_precedence::Expr<'static> {
    declared_precedence::Expr::Name(declared_precedence::Name {
        text: declared_precedence::NameText::new(text),
    })
}
"#,
        repair: r#"
    // The parsed round trip above already proves an enum with a declared
    // `precedence { ... }` block generates and pretty-prints a left-assoc chain on its first
    // operand without parentheses. Every other declared-precedence case needs a value no parse
    // of this grammar can produce without an authored `Parenthesized` node, so it is built
    // directly, exactly like the grouping repair above.
    use declared_precedence::Expr;

    // Left-assoc: chaining on the last operand keeps parentheses the first-operand chain does
    // not need.
    let left_nested_right = Expr::Subtract(
        Box::new(int("1")),
        Box::new(Expr::Subtract(Box::new(int("2")), Box::new(int("3")))),
    );
    assert_eq!(
        left_nested_right.render(),
        "1 - (2 - 3)",
        "left-assoc chaining on the last operand keeps parentheses",
    );

    // Right-assoc: the mirror image of left-assoc.
    let right_chain = Expr::Power(
        Box::new(int("2")),
        Box::new(Expr::Power(Box::new(int("3")), Box::new(int("4")))),
    );
    assert_eq!(
        right_chain.render(),
        "2 ^ 3 ^ 4",
        "right-assoc chaining on the last operand needs no parentheses",
    );
    let right_nested_left = Expr::Power(
        Box::new(Expr::Power(Box::new(int("2")), Box::new(int("3")))),
        Box::new(int("4")),
    );
    assert_eq!(
        right_nested_left.render(),
        "(2 ^ 3) ^ 4",
        "right-assoc chaining on the first operand keeps parentheses",
    );

    // Prefix: `#[parse(prec = UMINUS)]` gives `Negate` a level between `Add` and `Power`, so it
    // parenthesizes a looser-binding operand and does not parenthesize a tighter one.
    let negate_looser = Expr::Negate(Box::new(Expr::Add(Box::new(name("a")), Box::new(name("b")))));
    assert_eq!(
        negate_looser.render(),
        "-(a + b)",
        "a prefix variant parenthesizes a looser-binding operand",
    );
    let negate_tighter = Expr::Negate(Box::new(Expr::Factorial(Box::new(name("a")))));
    assert_eq!(
        negate_tighter.render(),
        "-a!",
        "a prefix variant needs no parentheses around a tighter-binding operand",
    );

    // Postfix: the mirror image of prefix.
    let factorial_looser =
        Expr::Factorial(Box::new(Expr::Add(Box::new(name("a")), Box::new(name("b")))));
    assert_eq!(
        factorial_looser.render(),
        "(a + b)!",
        "a postfix variant parenthesizes a looser-binding operand",
    );

    // Nonassoc: unlike left- or right-assoc, a same-level tie parenthesizes on both sides.
    let nonassoc_first = Expr::Equals(
        Box::new(Expr::Equals(Box::new(int("1")), Box::new(int("2")))),
        Box::new(int("3")),
    );
    assert_eq!(
        nonassoc_first.render(),
        "(1 == 2) == 3",
        "a nonassoc level parenthesizes a same-level first operand",
    );
    let nonassoc_last = Expr::Equals(
        Box::new(int("1")),
        Box::new(Expr::Equals(Box::new(int("2")), Box::new(int("3")))),
    );
    assert_eq!(
        nonassoc_last.render(),
        "1 == (2 == 3)",
        "a nonassoc level parenthesizes a same-level last operand",
    );

    for rendered in [
        left_nested_right.render(),
        right_chain.render(),
        right_nested_left.render(),
        negate_looser.render(),
        negate_tighter.render(),
        factorial_looser.render(),
        nonassoc_first.render(),
        nonassoc_last.render(),
    ] {
        assert_eq!(parse(&rendered).render(), rendered, "repaired precedence is idempotent");
    }
"#,
    },
    Golden {
        fixtures: PRETTY_GOLDEN_FIXTURES,
        fixture: "comments",
        module: "comments",
        root: "Script",
        // The infallible renderer writes no comments, exactly as before comments were kept.
        source: "-- header\nselect a /* c */, b from t",
        expected: "select a, b from t",
        fingerprint: COMMENTS_CHECKS,
        repair: COMMENTS_CASES,
    },
];

/// Checks shared by every retained-comment case of the `comments` golden.
const COMMENTS_CHECKS: &str = r#"
fn fingerprint(value: &comments::Script<'_>) -> String {
    format!("{value:?}")
}

/// The retained comments of `source`, in source order, as exact source spans.
fn comment_spans(source: &str) -> Vec<recursa::Span> {
    comments::lex(source)
        .trivia_gaps()
        .flat_map(|gap| gap.trivia().collect::<Vec<_>>())
        .filter(|trivia| trivia.kind().comment_style().is_some())
        .map(|trivia| {
            let text = trivia.text().strip_suffix('\r').unwrap_or(trivia.text());
            let start = trivia.span().start() as usize;
            recursa::Span::new(start, start + text.len()).unwrap()
        })
        .collect()
}

/// Formats `source` at `config` with its comments and checks every comment contract.
fn format(source: &str, config: recursa::PrettyConfig) -> String {
    let parsed = parse(source);
    let (output, map) = parsed
        .try_render_mapped_with(config)
        .unwrap_or_else(|conflict| panic!("{source:?} formats: {conflict}"));
    assert_eq!(output, parsed.try_render_with(config).unwrap(), "{source:?}: mapping changes no text");

    // Every retained comment is written exactly once, in source order, with its exact text, and
    // its mapping points to its exact source span.
    let entries = map.entries();
    let written = entries
        .iter()
        .filter(|entry| entry.is_comment())
        .map(|entry| entry.source())
        .collect::<Vec<_>>();
    assert_eq!(written, comment_spans(source), "{source:?}: each comment once, in order");
    for entry in entries {
        if !entry.source().is_synthetic() {
            assert_eq!(&output[entry.output()], &source[entry.source().range()], "{source:?}");
        }
    }
    for (index, entry) in entries.iter().enumerate().filter(|(_, entry)| entry.is_comment()) {
        // The comment stays between the syntax around it in the source.
        let comment = entry.source();
        for token in entries.iter().filter(|token| !token.is_comment() && !token.source().is_synthetic()) {
            if token.output().start < entry.output().start {
                assert!(token.source().end() <= comment.start(), "{source:?}: syntax moved after a comment");
            } else {
                assert!(token.source().start() >= comment.end(), "{source:?}: syntax moved before a comment");
            }
        }
        // A line comment is always followed by a line break.
        if source[comment.range()].starts_with("--") {
            assert_eq!(output[entry.output().end..].chars().next(), Some('\n'), "{source:?}: {index}");
        }
    }
    // Whitespace is regenerated: no carriage returns, no trailing spaces, no blank lines.
    assert!(!output.contains('\r'), "{source:?}: LF line endings only");
    assert!(!output.lines().any(|line| line.ends_with(' ')), "{source:?}: {output:?}");
    assert!(!output.contains("\n\n"), "{source:?}: {output:?}");

    // The output parses to the same AST, and formatting it again changes nothing.
    let reparsed = parse(&output);
    assert_eq!(fingerprint(&reparsed.into_ast()), fingerprint(&parse(source).into_ast()), "{source:?}");
    assert_eq!(parse(&output).try_render_with(config).unwrap(), output, "{source:?}: idempotent");
    output
}
"#;

/// The retained-comment cases of the `comments` golden, at a wide and a narrow width.
const COMMENTS_CASES: &str = r#"
    let wide = recursa::PrettyConfig::new(88, 2);
    let narrow = recursa::PrettyConfig::new(12, 2);
    let cases: &[(&str, &str, &str)] = &[
        // Beginning of input: the root owns it and writes it before its own layout.
        ("-- header\nselect a, b from t", "-- header\nselect a, b from t", "-- header\nselect a,\nb\nfrom t"),
        // Before and after a separator.
        ("select a /* c */, b from t", "select a /* c */, b from t", "select a /* c */,\nb\nfrom t"),
        ("select a, -- c\n b from t", "select a,\n-- c\nb\nfrom t", "select a,\n-- c\nb\nfrom t"),
        ("select a -- c\n; select b", "select a -- c\n;\nselect b", "select a -- c\n;\nselect b"),
        // Before an optional occurrence that is present, and where one is absent.
        ("select a from t /* c */ where x = y", "select a from t /* c */ where x = y", "select a\nfrom t\n/* c */\nwhere x\n= y"),
        ("select a /* c */ where x = y", "select a /* c */ where x = y", "select a\n/* c */\nwhere x\n= y"),
        // End of input belongs to the root, also after a trailing separator that canonical
        // layout drops.
        ("select a from t -- end", "select a from t -- end\n", "select a\nfrom t -- end\n"),
        ("select a; /* end */", "select a /* end */", "select a /* end */"),
        // Inside nested Nodes.
        ("select f(a, /* c */ b) from t", "select f(a, /* c */ b) from t", "select f(\n  a,\n  /* c */ b\n)\nfrom t"),
        ("select f( -- c\n a) from t", "select f(\n  -- c\n  a\n)\nfrom t", "select f(\n  -- c\n  a\n)\nfrom t"),
        // A tight boundary keeps a block comment tight.
        ("select s./* c */t from x", "select s./* c */t from x", "select s./* c */t\nfrom x"),
        // Only comments.
        ("-- only", "-- only\n", "-- only\n"),
        ("/* one */ /* two */", "/* one */ /* two */", "/* one */ /* two */"),
        // CRLF input renders with LF line endings.
        ("select a, -- c\r\n b\r\nfrom t\r\n", "select a,\n-- c\nb\nfrom t", "select a,\n-- c\nb\nfrom t"),
    ];
    let mut report = String::new();
    for (source, wide_expected, narrow_expected) in cases {
        let wide_output = format(source, wide);
        let narrow_output = format(source, narrow);
        if wide_output != *wide_expected || narrow_output != *narrow_expected {
            report.push_str(&format!("{source:?}\n  wide:   {wide_output:?}\n  narrow: {narrow_output:?}\n"));
        }
    }
    assert!(report.is_empty(), "comment goldens differ:\n{report}");

    // A line comment where the layout never breaks is a conflict, not moved or commented-out
    // syntax.
    for source in ["select s -- c\n.t from x", "select s. -- c\nt from x"] {
        let conflict = parse(source).try_render().expect_err("a line comment in a tight boundary");
        assert_eq!(conflict.kind(), recursa::PrettyConflictKind::LineCommentInTightBoundary);
        assert_eq!(&source[conflict.comment().range()], "-- c", "{source:?}");
    }

    // A value taken out of its parse, or built in code, renders as before: no comments.
    let source = "select a, -- c\n b from t";
    let detached = parse(source).into_ast();
    assert_eq!(detached.try_render().unwrap(), detached.render());
    assert_eq!(detached.render(), "select a, b from t");
    let synthetic = recursa::Parsed::synthetic(parse(source).into_ast());
    assert_eq!(synthetic.try_render().unwrap(), "select a, b from t");

    // Changing the shape of a parsed value needs `into_ast`, which drops its provenance, so the
    // comment cannot land beside syntax it was never next to.
    let mut changed = parse(source).into_ast();
    let first = parse("select z").into_ast().statements.into_iter().next().unwrap();
    changed.statements.insert(0, first);
    assert_eq!(changed.try_render().unwrap(), "select z; select a, b from t");
"#;

#[test]
fn every_representative_grammar_has_semantic_pretty_goldens() {
    let selected = std::env::var("RECURSA_PRETTY_GOLDEN").ok();
    for golden in GOLDENS {
        if selected
            .as_deref()
            .is_some_and(|selected| selected != golden.fixture)
        {
            continue;
        }
        let package = scaffold(golden);
        let output = run(&package);
        assert!(
            output.status.success(),
            "{} Pretty golden failed\nstdout:\n{}\nstderr:\n{}",
            golden.fixture,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

/// An arena-backed parse keeps its comments exactly as an owned parse does.
#[test]
fn an_arena_ast_keeps_the_same_comments() {
    let golden = GOLDENS
        .iter()
        .find(|golden| golden.fixture == "comments")
        .expect("the comments golden");
    let package = scaffold(golden);
    let grammar = package.path().join("src/comments.rs");
    let source = fs::read_to_string(&grammar).expect("read the comments grammar");
    fs::write(
        &grammar,
        source.replacen(
            "recursa::grammar! {",
            "recursa::grammar! {\n    arena_ast,",
            1,
        ),
    )
    .expect("select arena output");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa::Pretty as _;
use recursa_pretty_golden_comments::comments;

fn main() {
    for (source, expected) in [
        ("-- header\nselect a, b from t", "-- header\nselect a, b from t"),
        ("select f( -- c\n a) from t", "select f(\n  -- c\n  a\n)\nfrom t"),
        ("select a /* c */ where x = y", "select a /* c */ where x = y"),
        ("select a from t -- end", "select a from t -- end\n"),
        ("-- only", "-- only\n"),
    ] {
        let lexed = comments::lex(source);
        let mut input = lexed.input();
        let parsed = comments::Script::parse_spanned(&mut input).expect("parses");
        assert_eq!(parsed.try_render().unwrap(), expected, "{source:?}");
        assert!(!parsed.render().contains("--"), "{source:?}: render writes no comments");
    }
    let lexed = comments::lex("select s -- c\n.t from x");
    let mut input = lexed.input();
    let parsed = comments::Script::parse_spanned(&mut input).expect("parses");
    assert_eq!(
        parsed.try_render().unwrap_err().kind(),
        recursa::PrettyConflictKind::LineCommentInTightBoundary,
    );
}
"#,
    )
    .expect("write the arena consumer");
    let output = run(&package);
    assert!(
        output.status.success(),
        "arena comments failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn scaffold(golden: &Golden) -> TempDir {
    let package = tempfile::tempdir().expect("temporary representative grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("codegen belongs to the workspace");
    let source = fs::read_to_string(
        manifest_dir
            .join(golden.fixtures)
            .join(golden.fixture)
            .join("grammar.rs"),
    )
    .expect("read representative grammar");
    let source = source.replacen(
        "recursa::grammar! {",
        "recursa::grammar! {\n    derives(Pretty),",
        1,
    );
    let source = add_input_lifetimes(&source);

    fs::create_dir_all(package.path().join("src")).expect("create package source directory");
    fs::write(
        package.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "recursa-pretty-golden-{}"
version = "0.0.0"
edition = "2024"
rust-version = "1.88"
build = "build.rs"

[dependencies]
recursa = {{ path = {:?}, features = ["spans"] }}

[build-dependencies]
recursa-codegen = {{ path = {:?}, features = ["spans"] }}

[workspace]
"#,
            golden.fixture,
            workspace.join("recursa"),
            workspace.join("recursa-codegen"),
        ),
    )
    .expect("write package manifest");
    fs::write(
        package.path().join("build.rs"),
        format!(
            "fn main() -> Result<(), recursa_codegen::GenerateError> {{ recursa_codegen::generate(\"src/{}.rs\")?; Ok(()) }}\n",
            golden.module,
        ),
    )
    .expect("write build script");
    fs::write(
        package.path().join("src/lib.rs"),
        format!("pub mod {};\n", golden.module),
    )
    .expect("write library root");
    fs::write(
        package
            .path()
            .join("src")
            .join(format!("{}.rs", golden.module)),
        source,
    )
    .expect("write grammar source");
    fs::write(package.path().join("src/main.rs"), main_source(golden))
        .expect("write golden executable");
    package
}

fn main_source(golden: &Golden) -> String {
    format!(
        r#"use recursa::Pretty as _;
use recursa_pretty_golden_{crate_name}::{module};

fn parse(source: &str) -> recursa::Parsed<'_, {module}::{root}<'_>> {{
    let lexed = {module}::lex(source);
    assert!(lexed.errors().next().is_none());
    let mut input = lexed.input();
    let parsed = {module}::{root}::parse_spanned(&mut input).expect("golden parses");
    assert!(input.is_eof(), "Pretty round trip requires explicit EOF");
    parsed
}}

{fingerprint}

fn main() {{
    let original = {source:?};
    let parsed = parse(original);
    let formatted = parsed.render();
    assert_eq!(formatted, {expected:?});
    let (mapped, source_map) = parsed.render_mapped();
    assert_eq!(mapped, formatted, "mapped rendering preserves Pretty output");
    assert!(!source_map.entries().is_empty(), "rendered tokens are mapped");
    assert!(
        source_map.entries().iter().any(|entry| !entry.source().is_synthetic()),
        "captured parsed tokens retain source spans",
    );
    for entry in source_map.entries() {{
        let output = &mapped[entry.output()];
        if !entry.source().is_synthetic() {{
            assert_eq!(output, &original[entry.source().range()]);
        }}
    }}
    if {retains_no_comments} {{
        // Without retained comments the provenance-aware renderer writes the same bytes and
        // the same mappings as the infallible one.
        assert_eq!(
            parsed.try_render_mapped().expect("no comment can conflict"),
            (mapped.clone(), source_map.clone()),
        );
    }}
    let meaning = fingerprint(&parsed.into_ast());
    let reparsed = parse(&formatted);
    assert_eq!(reparsed.render(), formatted, "Pretty rendering is idempotent");
    assert_eq!(
        fingerprint(&reparsed.into_ast()),
        meaning,
        "parse/format/parse preserves AST meaning",
    );
    {repair}
}}
"#,
        crate_name = golden.fixture.replace('-', "_"),
        module = golden.module,
        root = golden.root,
        source = golden.source,
        expected = golden.expected,
        fingerprint = golden.fingerprint,
        repair = golden.repair,
        // Only the `comments` grammar retains comment trivia.
        retains_no_comments = golden.fixture != "comments",
    )
}

fn add_input_lifetimes(source: &str) -> String {
    let mut file = syn::parse_file(source).expect("representative grammar is valid Rust");
    let mut dependencies = BTreeMap::<String, BTreeSet<String>>::new();
    let mut required = BTreeSet::new();
    let mut content_tokens = BTreeSet::new();
    for item in &file.items {
        let Some((name, fields)) = node_fields(item) else {
            continue;
        };
        let mut used = BTreeSet::new();
        for field in fields {
            collect_type_idents(&field.ty, &mut used);
            if field
                .attrs
                .iter()
                .any(|attribute| attribute.path().is_ident("lex"))
            {
                required.insert(name.clone());
                collect_type_idents(&field.ty, &mut content_tokens);
            }
        }
        dependencies.insert(name, used);
    }
    required.extend(
        dependencies
            .iter()
            .filter(|(_, used)| used.iter().any(|used| content_tokens.contains(used)))
            .map(|(name, _)| name.clone()),
    );
    loop {
        let additions = dependencies
            .iter()
            .filter(|(name, used)| {
                !required.contains(*name) && used.iter().any(|used| required.contains(used))
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        if additions.is_empty() {
            break;
        }
        required.extend(additions);
    }

    let lifetime_types = required
        .iter()
        .chain(&content_tokens)
        .cloned()
        .collect::<BTreeSet<_>>();

    for item in &mut file.items {
        let name = match item {
            Item::Struct(item) if is_node(&item.attrs) => Some(item.ident.to_string()),
            Item::Enum(item) if is_node(&item.attrs) => Some(item.ident.to_string()),
            _ => None,
        };
        let Some(name) = name else { continue };
        if required.contains(&name) {
            match item {
                Item::Struct(item) => item.generics.params.insert(0, syn::parse_quote!('input)),
                Item::Enum(item) => item.generics.params.insert(0, syn::parse_quote!('input)),
                _ => unreachable!(),
            }
        }
        match item {
            Item::Struct(item) => {
                for field in &mut item.fields {
                    add_lifetimes_to_field(field, &lifetime_types);
                }
            }
            Item::Enum(item) => {
                for variant in &mut item.variants {
                    for field in &mut variant.fields {
                        add_lifetimes_to_field(field, &lifetime_types);
                    }
                }
            }
            _ => {}
        }
    }
    file.into_token_stream().to_string()
}

fn is_node(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("derive")
            && attribute
                .meta
                .to_token_stream()
                .to_string()
                .contains("Node")
    })
}

fn node_fields(item: &Item) -> Option<(String, Vec<&syn::Field>)> {
    match item {
        Item::Struct(item) if is_node(&item.attrs) => {
            Some((item.ident.to_string(), item.fields.iter().collect()))
        }
        Item::Enum(item) if is_node(&item.attrs) => Some((
            item.ident.to_string(),
            item.variants
                .iter()
                .flat_map(|variant| &variant.fields)
                .collect(),
        )),
        _ => None,
    }
}

fn collect_type_idents(ty: &Type, output: &mut BTreeSet<String>) {
    match ty {
        Type::Path(path) => {
            for segment in &path.path.segments {
                output.insert(segment.ident.to_string());
                if let PathArguments::AngleBracketed(arguments) = &segment.arguments {
                    for argument in &arguments.args {
                        if let GenericArgument::Type(ty) = argument {
                            collect_type_idents(ty, output);
                        }
                    }
                }
            }
        }
        Type::Tuple(tuple) => {
            for element in &tuple.elems {
                collect_type_idents(element, output);
            }
        }
        _ => {}
    }
}

fn add_lifetimes_to_field(field: &mut syn::Field, required: &BTreeSet<String>) {
    if field
        .attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("lex"))
    {
        add_lifetime_to_terminal(&mut field.ty);
    }
    add_required_lifetimes(&mut field.ty, required);
}

fn add_lifetime_to_terminal(ty: &mut Type) {
    let Type::Path(path) = ty else { return };
    let Some(segment) = path.path.segments.last_mut() else {
        return;
    };
    if matches!(segment.arguments, PathArguments::None) {
        segment.arguments = input_lifetime_arguments();
    }
}

fn add_required_lifetimes(ty: &mut Type, required: &BTreeSet<String>) {
    let Type::Path(path) = ty else { return };
    for segment in &mut path.path.segments {
        if let PathArguments::AngleBracketed(arguments) = &mut segment.arguments {
            for argument in &mut arguments.args {
                if let GenericArgument::Type(ty) = argument {
                    add_required_lifetimes(ty, required);
                }
            }
        }
        if required.contains(&segment.ident.to_string())
            && matches!(segment.arguments, PathArguments::None)
        {
            segment.arguments = input_lifetime_arguments();
        }
    }
}

fn input_lifetime_arguments() -> PathArguments {
    let mut arguments = Punctuated::new();
    arguments.push(GenericArgument::Lifetime(syn::parse_quote!('input)));
    PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
        colon2_token: None,
        lt_token: Default::default(),
        args: arguments,
        gt_token: Default::default(),
    })
}

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
        .expect("run representative Pretty golden")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}
