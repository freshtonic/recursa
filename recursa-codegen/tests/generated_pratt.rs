use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/pratt";

#[test]
fn a_downstream_arithmetic_grammar_parses_the_frozen_pratt_plan() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);

    assert_success(&output);
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "pratt-ok");
}

#[test]
fn pratt_dispatch_regressions_share_one_downstream_fixture() {
    let package = scaffold_pratt_dispatch_overlap_fixture();
    let output = run_fixture(&package);

    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "pratt-dispatch-regressions-ok"
    );
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
            ("Custom", 3),
            ("IsNot", 4),
            ("Like", 3),
            ("Cast", 3),
            ("IsNull", 3),
        ],
    );
    let parsed = explorer.parse("a + 2 * 3").expect("Pratt expression parses");
    assert_eq!(parsed.root().active_variant().unwrap().name(), "Add");
    assert_eq!(parsed.root().elements().len(), 3);
    assert_eq!(
        parsed.root().elements().next().unwrap().children().len(),
        1,
    );
    let grouped = explorer.parse("BEGIN GROUP a END GROUP").expect("group parses");
    assert_eq!(grouped.root().active_variant().unwrap().name(), "MultiGroup");
    let grouped_child = grouped.root().elements().next().unwrap().children().next().unwrap();
    assert_eq!(grouped_child.active_variant().unwrap().name(), "Name");
    let grouped_token = grouped_child.elements().next().unwrap()
        .children().next().unwrap()
        .elements().next().unwrap()
        .children().next().unwrap();
    assert_eq!(grouped_token.token_text(), Some("a"));
    let incomplete = explorer.parse("a +").err().expect("incomplete Pratt expression fails");
    assert_eq!(incomplete.diagnostics().len(), 1);
    assert_eq!(
        incomplete.diagnostics()[0].kind(),
        recursa::explorer::ParseDiagnosticKind::UnexpectedEof,
    );
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
    let diagram = diagram_svg(&expression.1);
    assert!(
        contains_text_sequence(diagram, &["atom Paren", "[", "grammar::Expr", "]"]),
        "attached grouping tokens must come from the frozen materialized body:\n{}",
        expression.1,
    );
    assert!(
        !expression.1.contains("__recursa_attachment"),
        "private attachment units must not surface in Pratt diagrams",
    );
    assert!(
        contains_text_sequence(diagram, &["prefix bp=35 Neg", "-", "grammar::Expr"]),
        "prefix role and binding power must be rendered:\n{}",
        expression.1,
    );
    assert!(
        contains_text_sequence(diagram, &["postfix bp=40 Factorial", "grammar::Expr", "!"],),
        "postfix lhs placement and binding power must be rendered:\n{}",
        expression.1,
    );
    assert!(
        contains_text_sequence(
            diagram,
            &[
                "infix lbp=30 rbp=30 Power",
                "grammar::Expr",
                "^",
                "grammar::Expr",
            ],
        ),
        "infix lhs, tail, and binding powers must be rendered:\n{}",
        expression.1,
    );
}

fn diagram_svg(document: &str) -> &str {
    let start = document.find("<svg ").expect("diagram SVG start");
    let end = document[start..]
        .find("</svg>")
        .map(|offset| start + offset + "</svg>".len())
        .expect("diagram SVG end");
    &document[start..end]
}

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

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary Pratt grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative);
    }
    copy_as(&fixture, package.path(), "Cargo.core.lock", "Cargo.lock");

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

fn scaffold_pratt_dispatch_overlap_fixture() -> TempDir {
    let package = scaffold_fixture();
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"
recursa::grammar! {
    module = crate::grammar,
    max_lookahead = 4,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        BEGIN => "BEGIN",
        WRAP => "WRAP",
        GROUP => "GROUP",
        OTHER => "OTHER",
        END => "END",
        DEEP => "DEEP",
        ONE => "ONE",
        TWO => "TWO",
        LEFT => "LEFT",
        RIGHT => "RIGHT",
        FROM => "FROM",
        ANY => "ANY",
        BETWEEN => "BETWEEN",
        SIMILAR => "SIMILAR",
        TO => "TO",
        POSITION => "POSITION",
        IN => "IN",
        AND => "AND",
        BOOL => "BOOL",
        TEXT => "TEXT",
        WHEN => "WHEN",
        THEN => "THEN",
        CASE => "CASE",
    }
    admissions {
        NameWords = { WHEN },
    }
    punctuation {
        PLUS => "+",
        CONCAT => "||",
        DOUBLE_COLON => "::", spacing = tight,
        GT => ">",
        COMMA => ",",
        DOT => ".", spacing = tight,
        LPAREN => "(",
        RPAREN => ")",
    }
}

#[derive(recursa::Node)]
pub struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub value: IntegerText<'input>,
}

#[derive(recursa::Node)]
pub struct Name<'input> {
    #[lex(pattern = r"[a-z]+", admits(NameWords))]
    pub value: NameText<'input>,
}

#[derive(recursa::Node)]
pub enum NameAtom<'input> {
    Plain(Name<'input>),
}

#[derive(recursa::Node)]
pub struct QualifiedName<'input> {
    pub qualifier: Name<'input>,
    #[tok(DOT, this)]
    pub name: Name<'input>,
}

#[derive(recursa::Node)]
pub enum TypeName {
    #[tok(BOOL)]
    Bool,
    #[tok(TEXT)]
    Text,
}

#[derive(recursa::Node)]
#[tok(BEGIN)]
pub struct BeginAtom;

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'input> {
    Integer(Integer<'input>),
    Qualified(QualifiedName<'input>),
    Name(NameAtom<'input>),
    BeginAtom(BeginAtom),
    #[tok(CASE)]
    CaseWord,
    Case(
        #[tok(CASE, WHEN, this)] Box<Self>,
        #[tok(THEN, this, END)] Box<Self>,
    ),
    Parenthesized(#[tok(LPAREN, this, RPAREN)] Box<Self>),
    MultiGroup(#[tok(BEGIN, GROUP, this, END, GROUP)] Box<Self>),
    Wrapped(#[tok(WRAP, GROUP, this, END)] Box<Self>),
    Rival(#[tok(WRAP, OTHER, this, END)] Integer<'input>),
    #[tok(DEEP, ONE, TWO, LEFT)]
    DeepLeft,
    #[tok(DEEP, ONE, TWO, RIGHT)]
    DeepRight,
    #[tok(FROM, LEFT)]
    FromLeft,
    #[tok(FROM, RIGHT)]
    FromRight,

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),
    #[parse(infix, lbp = 5, rbp = 6)]
    LowPlus(Box<Self>, #[tok(PLUS, OTHER, this)] Box<Self>),
    #[parse(infix, lbp = 8, rbp = 9)]
    Concat(Box<Self>, #[tok(CONCAT, this)] Box<Self>),
    #[parse(infix, lbp = 5, rbp = 6)]
    Greater(Box<Self>, #[tok(GT, this)] Box<Self>),
    #[parse(postfix, bp = 5)]
    QuantifiedGreater(
        Box<Self>,
        #[tok(GT, ANY, LPAREN, this, RPAREN)] Integer<'input>,
    ),
    #[parse(postfix, bp = 6)]
    Between(
        Box<Self>,
        #[tok(BETWEEN, this)] Box<Self>,
        #[tok(AND, this)] Box<Self>,
    ),
    #[parse(postfix, bp = 5)]
    SimilarTo(Box<Self>, #[tok(SIMILAR, TO, this)] Box<Self>),
    #[parse(infix, lbp = 10, rbp = 11)]
    In(Box<Self>, #[tok(IN, this)] Box<Self>),
    #[parse(infix, lbp = 2, rbp = 3)]
    And(Box<Self>, #[tok(AND, this)] Box<Self>),
    #[parse(postfix, bp = 20)]
    Cast(Box<Self>, #[tok(DOUBLE_COLON, this)] TypeName),
}

#[derive(recursa::Node)]
pub struct GroupFollowed<'input> {
    #[tok(this, GROUP)]
    pub expression: Expr<'input>,
    pub tail: Integer<'input>,
}

#[derive(recursa::Node)]
pub struct FollowedExpr<'input> {
    #[tok(WHEN, this, THEN)]
    pub expression: Expr<'input>,
}

#[derive(recursa::Node)]
pub struct QualifiedFollowed<'input> {
    #[tok(this, WHEN)]
    pub expression: Expr<'input>,
}

#[derive(recursa::Node)]
pub enum OrdinaryChoice<'input> {
    Expression(Expr<'input>),
    Rival(#[tok(this, OTHER)] Name<'input>),
}

#[derive(recursa::Node)]
pub struct OrdinaryChoiceFollowed<'input> {
    #[tok(this, WHEN)]
    pub choice: OrdinaryChoice<'input>,
}

#[derive(recursa::Node)]
pub enum ExprOrQuery<'input> {
    Expressions(#[sep(COMMA)] recursa::Vec1<Expr<'input>>),
    Query(ParenthesizedQuery<'input>),
}

#[derive(recursa::Node)]
pub struct ParenthesizedQuery<'input> {
    #[tok(LPAREN, this, RPAREN, OTHER)]
    pub expression: Box<Expr<'input>>,
}

#[derive(recursa::Node)]
pub struct SimilarDelimited<'input> {
    #[tok(BEGIN, this, SIMILAR)]
    pub source: Box<Expr<'input>>,
    pub pattern: Box<Expr<'input>>,
}

#[derive(recursa::Node)]
pub struct RestrictedOperand<'input> {
    #[parse(pratt(exclude(In, And)))]
    #[tok(POSITION, this, IN)]
    pub needle: Box<Expr<'input>>,
    pub haystack: Box<Expr<'input>>,
}
"#,
    )
    .expect("write caller-FOLLOW overlap grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"
use recursa_generated_pratt::grammar;

fn main() {
    let lexed = grammar::lex("name + 1");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .expect("a wrapped atom receives the Pratt NUD follow while parsing");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast(),
        grammar::Expr::Add(lhs, rhs)
            if matches!(*lhs, grammar::Expr::Name(grammar::NameAtom::Plain(_)))
                && matches!(*rhs, grammar::Expr::Integer(_))
    ));

    let lexed = grammar::lex("name || name + 1");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .expect("an active tighter LED is not mistaken for recursive-operand FOLLOW");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast(),
        grammar::Expr::Concat(_, rhs)
            if matches!(*rhs, grammar::Expr::Add(_, _))
    ));

    let lexed = grammar::lex("name BETWEEN 1 + 2 AND 3 + 4");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .expect("a postfix recursive operand stops at a lower-binding delimiter");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast(),
        grammar::Expr::Between(_, low, high)
            if matches!(*low, grammar::Expr::Add(_, _))
                && matches!(*high, grammar::Expr::Add(_, _))
    ));

    let lexed = grammar::lex("name::BOOL::TEXT");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .expect("a completed postfix remains eligible for another postfix extender");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast(),
        grammar::Expr::Cast(lhs, grammar::TypeName::Text)
            if matches!(*lhs, grammar::Expr::Cast(_, grammar::TypeName::Bool))
    ));

    let lexed = grammar::lex("name SIMILAR TO name");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .expect("a complete multi-token LED remains viable");
    assert!(input.is_eof());
    assert!(matches!(parsed.into_ast(), grammar::Expr::SimilarTo(_, _)));

    let lexed = grammar::lex("BEGIN name SIMILAR name");
    let mut input = lexed.input();
    grammar::SimilarDelimited::parse(&mut input)
        .expect("an incomplete multi-token LED yields to the caller delimiter");
    assert!(input.is_eof());

    let lexed = grammar::lex("name IN other");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .expect("an excluded LED remains available to ordinary Pratt callers");
    assert!(input.is_eof());
    assert!(matches!(parsed.into_ast(), grammar::Expr::In(_, _)));

    let lexed = grammar::lex("POSITION name + other IN target");
    let mut input = lexed.input();
    let parsed = grammar::RestrictedOperand::parse(&mut input)
        .expect("a restricted Pratt operand yields its excluded LED to the caller delimiter");
    assert!(input.is_eof());
    assert!(matches!(
        *parsed.into_ast().needle,
        grammar::Expr::Add(_, rhs) if matches!(*rhs, grammar::Expr::Name(_))
    ));

    let lexed = grammar::lex("BEGIN GROUP 1 END GROUP GROUP 2");
    let mut input = lexed.input();
    let parsed = grammar::GroupFollowed::parse(&mut input)
        .expect("the longer NUD suffix parses before caller-FOLLOW fallback");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast().expression,
        grammar::Expr::MultiGroup(_),
    ));

    let lexed = grammar::lex("WRAP GROUP 1 + 2 END");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .expect("the unique wrapper NUD admits an infix in its nested expression");
    assert!(input.is_eof());
    assert!(matches!(parsed.into_ast(), grammar::Expr::Wrapped(_)));

    let lexed = grammar::lex("CASE WHEN 1 > 0 THEN 1 END");
    let mut input = lexed.input();
    let parsed = grammar::ExprOrQuery::parse(&mut input)
        .expect("a balanced enum delegates its unique non-opener Pratt branch");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast(),
        grammar::ExprOrQuery::Expressions(expressions)
            if matches!(expressions.first(), grammar::Expr::Case(_, _))
    ));

    let lexed = grammar::lex("(name)::BOOL");
    let mut input = lexed.input();
    let parsed = grammar::ExprOrQuery::parse(&mut input)
        .expect("a crossed-Pratt post-close fallback leaves its extender to the expression");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast(),
        grammar::ExprOrQuery::Expressions(expressions)
            if matches!(expressions.first(), grammar::Expr::Cast(_, grammar::TypeName::Bool))
    ));

    let lexed = grammar::lex("(name) OTHER");
    let mut input = lexed.input();
    let parsed = grammar::ExprOrQuery::parse(&mut input)
        .expect("an exact post-close competitor wins before the crossed-Pratt fallback");
    assert!(input.is_eof());
    assert!(matches!(parsed.into_ast(), grammar::ExprOrQuery::Query(_)));

    let lexed = grammar::lex("FROM");
    let mut input = lexed.input();
    let error = match grammar::Expr::parse(&mut input) {
        Ok(_) => panic!("FROM requires a distinguishing suffix"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), recursa::ParseErrorKind::UnexpectedEof);
    assert_eq!(error.span(), recursa::Span::new(4, 4).unwrap());
    assert_eq!(error.found(), None);
    assert_eq!(input.cursor(), 0, "the public parse remains transactional");

    let lexed = grammar::lex("WHEN 1 > 0 THEN");
    let mut input = lexed.input();
    grammar::FollowedExpr::parse(&mut input)
        .expect("a uniquely selected infix validates its nested right operand");
    assert!(input.is_eof());

    let lexed = grammar::lex("left.right WHEN");
    let mut input = lexed.input();
    let parsed = grammar::QualifiedFollowed::parse(&mut input)
        .expect("a qualified Pratt atom yields to the caller keyword");
    assert!(input.is_eof());
    assert!(matches!(parsed.into_ast().expression, grammar::Expr::Qualified(_)));

    let lexed = grammar::lex("name");
    let mut input = lexed.input();
    let parsed = grammar::OrdinaryChoice::parse(&mut input)
        .expect("an ordinary crossed-Pratt fallback selects at EOF");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast(),
        grammar::OrdinaryChoice::Expression(grammar::Expr::Name(_))
    ));

    let lexed = grammar::lex("name WHEN");
    let mut input = lexed.input();
    let parsed = grammar::OrdinaryChoiceFollowed::parse(&mut input)
        .expect("an ordinary crossed-Pratt fallback selects on unmatched caller FOLLOW");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast().choice,
        grammar::OrdinaryChoice::Expression(grammar::Expr::Name(_))
    ));

    let lexed = grammar::lex("name OTHER");
    let mut input = lexed.input();
    let parsed = grammar::OrdinaryChoice::parse(&mut input)
        .expect("an exact ordinary competitor wins before the crossed-Pratt fallback");
    assert!(input.is_eof());
    assert!(matches!(parsed.into_ast(), grammar::OrdinaryChoice::Rival(_)));

    let lexed = grammar::lex("WHEN 1 > ANY (0) THEN");
    let mut input = lexed.input();
    let parsed = grammar::FollowedExpr::parse(&mut input)
        .expect("the longer quantified LED parses instead of the ordinary infix");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.into_ast().expression,
        grammar::Expr::QuantifiedGreater(_, _),
    ));

    let lexed = grammar::lex("WHEN 1 > 0 FROM THEN");
    let mut input = lexed.input();
    let error = match grammar::FollowedExpr::parse(&mut input) {
        Ok(_) => panic!("the selected ordinary LED must preserve the deeper caller error"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), recursa::ParseErrorKind::UnexpectedToken);
    assert_eq!(error.span(), recursa::Span::new(11, 15).unwrap());
    assert_eq!(error.found(), Some("fixed:FROM"));
    assert_eq!(error.expected().collect::<Vec<_>>(), ["fixed:THEN"]);
    assert_eq!(input.cursor(), 0, "the public parse remains transactional");
    println!("pratt-dispatch-regressions-ok");
}
"#,
    )
    .expect("write shared Pratt dispatch regression consumer");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy Pratt grammar fixture file");
}

fn copy_as(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy Pratt grammar fixture file");
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
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
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
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
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
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::copy(
        manifest_dir.join(FIXTURE).join("Cargo.lock"),
        package.path().join("Cargo.lock"),
    )
    .expect("select diagrams-capable fixture lock");
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
