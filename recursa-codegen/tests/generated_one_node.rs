use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/one-node";

#[test]
fn generated_downstream_code_is_warning_free_under_clippy() {
    let package = scaffold_fixture();
    let output = lint_fixture(&package);

    assert_success(&output);

    let library = package.path().join("src/lib.rs");
    let authored = fs::read_to_string(&library).expect("read downstream library");
    fs::write(
        &library,
        format!("{authored}\nfn handwritten_dead_code() {{}}\n"),
    )
    .expect("introduce a handwritten warning");
    let rejected = lint_fixture(&package);
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(!rejected.status.success(), "handwritten warnings must fail");
    assert!(stderr.contains("src/lib.rs"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("handwritten_dead_code"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn a_downstream_crate_loads_and_parses_one_generated_node_through_the_facade() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let main = package.path().join("src/main.rs");
    let valid_source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    let valid_main = fs::read_to_string(&main).expect("read scaffolded downstream consumer");

    let initial = run_fixture(&package);

    assert_success(&initial);
    let selected = find_selected_generation(&target_dir(&package));
    assert_eq!(
        String::from_utf8(initial.stdout).unwrap().trim(),
        "one-node-ok RCA4100 RCA4101"
    );

    fs::write(
        &grammar,
        "recursa::grammar! { module = crate::grammar }\n\npub struct Broken {\n",
    )
    .expect("replace the selected grammar with invalid Rust");
    let failed = run_fixture(&package);
    assert!(
        !failed.status.success(),
        "a hard generation failure must stop the Cargo build"
    );
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("RCA2001"),
        "the invalid selected grammar must fail in generation, not only in downstream compilation"
    );
    assert!(
        !String::from_utf8_lossy(&failed.stdout).contains("one-node-ok"),
        "Cargo must not run the stale previously built binary after generation fails"
    );
    assert!(
        !selected.exists(),
        "a failed generation must invalidate the previous selector"
    );

    let revised_source = valid_source
        .replace("Word", "Name")
        .replace("r#match", "r#loop");
    let revised_main = valid_main
        .replace("Word", "Name")
        .replace("r#match", "r#loop");
    assert_ne!(revised_source, valid_source);
    assert_ne!(revised_main, valid_main);
    fs::write(&grammar, revised_source).expect("replace the failure with a revised valid grammar");
    fs::write(&main, revised_main).expect("use the revised generated grammar API");
    let restored = run_fixture(&package);
    assert_success(&restored);
    assert_eq!(
        String::from_utf8(restored.stdout).unwrap().trim(),
        "one-node-ok RCA4100 RCA4101"
    );
}

#[test]
fn a_node_body_rejects_the_same_semantic_key_from_a_different_physical_declaration() {
    let package = scaffold_fixture();
    fs::copy(
        package.path().join("src/grammar.rs"),
        package.path().join("src/analyzed.rs"),
    )
    .expect("copy the grammar to a physically distinct analyzed source");
    fs::write(
        package.path().join("build.rs"),
        r#"fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/analyzed.rs")?;
    Ok(())
}
"#,
    )
    .expect("select the replacement declaration for analysis");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "a replacement physical declaration must not load the compiled Node body"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("RCA9002"),
        "the declaration-local anchor mismatch must fail at the Node loader\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn structural_loaders_report_missing_output_without_cascades_or_fallbacks() {
    let package = scaffold_fixture();
    assert_success(&run_fixture(&package));

    let selected = find_selected_generation(&target_dir(&package));
    let selector = fs::read_to_string(&selected).expect("read selected Generation");
    let identity = selector
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// recursa-generation:"))
        .expect("selector carries its private Generation identity");
    let generation = selected
        .parent()
        .unwrap()
        .join("recursa-generations")
        .join(identity);
    let root = generation.join("root.rs");
    let node = generation.join("node-0.rs");
    let root_rust = fs::read(&root).expect("read generated root");
    let node_rust = fs::read(&node).expect("read generated Node body");

    fs::remove_file(&root).expect("remove generated root");
    force_consumer_recompile(&package, "root-absence");
    let missing_root = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&missing_root.stderr);
    assert!(!missing_root.status.success(), "missing root must fail");
    assert!(stderr.contains("RCA9001"), "stderr:\n{stderr}");
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");

    fs::write(&root, &root_rust).expect("restore generated root");
    fs::remove_file(&node).expect("remove generated Node body");
    force_consumer_recompile(&package, "node-absence");
    let missing_node = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&missing_node.stderr);
    assert!(
        !missing_node.status.success(),
        "missing Node body must fail"
    );
    assert!(stderr.contains("RCA9002"), "stderr:\n{stderr}");
    assert!(!stderr.contains("RCA9001"), "stderr:\n{stderr}");

    fs::write(&node, &node_rust).expect("restore generated Node body");
    let incompatible_root = String::from_utf8(root_rust.clone())
        .unwrap()
        .replace("protocol_25", "protocol_999");
    assert_ne!(incompatible_root.as_bytes(), root_rust);
    fs::write(&root, incompatible_root).unwrap();
    force_consumer_recompile(&package, "protocol-mismatch");
    let mismatch = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&mismatch.stderr);
    assert!(!mismatch.status.success(), "protocol mismatch must fail");
    assert_eq!(
        stderr.matches("error: error[RCA9003]").count(),
        1,
        "stderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");

    fs::write(&root, &root_rust).expect("restore protocol-compatible root");
    let conflicting_node = String::from_utf8(node_rust.clone())
        .unwrap()
        .replace(identity, "g_0000000000000000");
    assert_ne!(conflicting_node.as_bytes(), node_rust);
    fs::write(&node, conflicting_node).unwrap();
    force_consumer_recompile(&package, "generation-conflict");
    let conflict = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&conflict.stderr);
    assert!(!conflict.status.success(), "Generation conflict must fail");
    assert!(stderr.contains("RCA9004"), "stderr:\n{stderr}");

    fs::write(&node, &node_rust).expect("restore matching Node body");
    fs::write(&node, "this is externally malformed generated Rust").unwrap();
    force_consumer_recompile(&package, "malformed-node");
    let malformed = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(
        !malformed.status.success(),
        "malformed generated Node Rust must fail"
    );
    assert!(
        stderr.contains("expected one of") || stderr.contains("expected `!`"),
        "native Rust syntax diagnostics must remain authoritative\nstderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");

    fs::write(&node, &node_rust).expect("restore generated Node body after corruption");
    fs::write(&root, "this is externally malformed generated Rust").unwrap();
    force_consumer_recompile(&package, "malformed-root");
    let malformed = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(
        !malformed.status.success(),
        "malformed generated Rust must fail"
    );
    assert!(
        stderr.contains("expected one of") || stderr.contains("expected `!`"),
        "native Rust syntax diagnostics must remain authoritative\nstderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9001"), "stderr:\n{stderr}");
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");
    assert!(!stderr.contains("runtime fallback"), "stderr:\n{stderr}");

    fs::write(&root, root_rust).expect("restore generated root after corruption");
    force_consumer_recompile(&package, "clean-recovery");
    assert_success(&run_fixture(&package));
}

#[test]
fn a_view_variant_and_intrinsic_content_token_value_traits_compile_together() {
    let package = scaffold_fixture();
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::r#type,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {}

#[derive(recursa::Node, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Entry<'input> {
    View(
        #[lex(pattern = r"[A-Za-z]+")]
        Word<'input>,
    ),
}
"#,
    )
    .expect("write the View collision grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa_generated_one_node::r#type as grammar;

fn main() {
    let lexed = grammar::lex("alpha");
    let mut input = lexed.input();
    let parsed = grammar::Entry::parse(&mut input).expect("the View variant parses");
    let grammar::Entry::View(value) = parsed.into_ast();
    assert_eq!(value.text(), "alpha");
}
"#,
    )
    .expect("write the View collision consumer");

    assert_success(&run_fixture(&package));
}

#[test]
fn local_visit_traits_traverse_content_tokens_without_changing_token_defaults() {
    let package = scaffold_fixture();
    enable_arbitrary_and_serde(&package);
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::r#type,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {}

#[derive(recursa::Node)]
#[node(derive(Pretty, Visit, VisitMut, Arbitrary, serde::Serialize, serde::Deserialize))]
pub struct Entry<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub value: WordText<'input>,
}

#[derive(recursa::Node)]
#[node(derive(Arbitrary, serde::Serialize, serde::Deserialize))]
pub struct Wrapped<'input> {
    pub value: Box<WordText<'input>>,
}

pub mod nested {
    #[derive(recursa::Node)]
    #[node(derive(Arbitrary, serde::Serialize, serde::Deserialize))]
    pub struct Optional<'input> {
        pub value: Option<Box<super::WordText<'input>>>,
    }

    #[derive(recursa::Node)]
    #[node(derive(Arbitrary, serde::Serialize, serde::Deserialize))]
    pub struct Repeated<'input> {
        pub value: Vec<super::WordText<'input>>,
    }

    #[derive(recursa::Node)]
    #[node(derive(Arbitrary, serde::Serialize, serde::Deserialize))]
    pub struct Nonempty<'input> {
        pub value: recursa::Vec1<super::WordText<'input>>,
    }
}
"#,
    )
    .expect("write the local Visit grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa_generated_one_node::r#type as grammar;

#[derive(Default)]
struct Count {
    enters: usize,
    exits: usize,
}

impl<'input> recursa::TotalVisitor<grammar::__RecursaVisitEvents<'input>> for Count {
    type Error = ();

    fn total_enter<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEvent<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEvents<'input>: 'visit,
    {
        self.enters += 1;
        std::ops::ControlFlow::Continue(())
    }

    fn total_exit<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEvent<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEvents<'input>: 'visit,
    {
        self.exits += 1;
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> recursa::TotalVisitorMut<grammar::__RecursaVisitEventsMut<'input>> for Count {
    type Error = ();

    fn total_enter_mut<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEventMut<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEventsMut<'input>: 'visit,
    {
        self.enters += 1;
        std::ops::ControlFlow::Continue(())
    }

    fn total_exit_mut<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEventMut<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEventsMut<'input>: 'visit,
    {
        self.exits += 1;
        std::ops::ControlFlow::Continue(())
    }
}

fn main() {
    use recursa::Pretty as _;

    let lexed = grammar::lex("alpha");
    let mut input = lexed.input();
    let mut value = grammar::Entry::parse(&mut input)
        .expect("the local Visit Node parses")
        .into_ast();
    assert!(input.is_eof());

    let mut immutable = Count::default();
    assert!(recursa::Visit::visit(&value, &mut immutable).is_continue());
    assert_eq!(immutable.enters, 2, "the Node and content token are entered");
    assert_eq!(immutable.exits, 2, "the Node and content token are exited");

    let mut mutable = Count::default();
    assert!(recursa::VisitMut::visit_mut(&mut value, &mut mutable).is_continue());
    assert_eq!(mutable.enters, 2, "the Node and content token are entered mutably");
    assert_eq!(mutable.exits, 2, "the Node and content token are exited mutably");

    assert_eq!(value.render(), "alpha", "the content token renders through local Pretty");

    let encoded = serde_json::to_string(&value).expect("the local Serialize Node encodes");
    let decoded: grammar::Entry<'static> =
        serde_json::from_str(&encoded).expect("the local Deserialize Node decodes");
    assert_eq!(decoded.value.text(), "alpha");

    let mut bytes = recursa::__private::arbitrary::Unstructured::new(&[0]);
    let generated = <grammar::Entry<'static> as recursa::Arbitrary>::arbitrary(&mut bytes)
        .expect("the local Arbitrary Node constructs its content token");
    assert!(!generated.value.text().is_empty());

    let wrapped_lexed = grammar::lex("beta");
    let mut wrapped_input = wrapped_lexed.input();
    let wrapped = grammar::Wrapped::parse(&mut wrapped_input)
        .expect("the wrapped token Node parses")
        .into_ast();
    let wrapped_json = serde_json::to_string(&wrapped).expect("the boxed token encodes");
    let wrapped_decoded: grammar::Wrapped<'static> =
        serde_json::from_str(&wrapped_json).expect("the boxed token decodes");
    assert_eq!(wrapped_decoded.value.text(), "beta");

    let mut wrapped_bytes = recursa::__private::arbitrary::Unstructured::new(&[0]);
    let wrapped_generated =
        <grammar::Wrapped<'static> as recursa::Arbitrary>::arbitrary(&mut wrapped_bytes)
            .expect("the local Arbitrary Node constructs a boxed content token");
    assert!(!wrapped_generated.value.text().is_empty());

    let optional = grammar::nested::Optional {
        value: Some(Box::new(grammar::WordText::new("gamma"))),
    };
    let optional_json = serde_json::to_string(&optional).expect("the optional token encodes");
    let optional_decoded: grammar::nested::Optional<'static> =
        serde_json::from_str(&optional_json).expect("the optional token decodes");
    assert_eq!(optional_decoded.value.unwrap().text(), "gamma");

    let repeated = grammar::nested::Repeated {
        value: vec![grammar::WordText::new("delta")],
    };
    let repeated_json = serde_json::to_string(&repeated).expect("the repeated token encodes");
    let repeated_decoded: grammar::nested::Repeated<'static> =
        serde_json::from_str(&repeated_json).expect("the repeated token decodes");
    assert_eq!(repeated_decoded.value[0].text(), "delta");

    let nonempty = grammar::nested::Nonempty {
        value: recursa::Vec1::new(grammar::WordText::new("epsilon")),
    };
    let nonempty_json = serde_json::to_string(&nonempty).expect("the nonempty token encodes");
    let nonempty_decoded: grammar::nested::Nonempty<'static> =
        serde_json::from_str(&nonempty_json).expect("the nonempty token decodes");
    assert_eq!(nonempty_decoded.value.first().text(), "epsilon");

    let mut wrapper_bytes = recursa::__private::arbitrary::Unstructured::new(&[0]);
    assert!(
        <grammar::nested::Optional<'static> as recursa::Arbitrary>::arbitrary(
            &mut wrapper_bytes,
        )
        .is_ok()
    );
    let mut wrapper_bytes = recursa::__private::arbitrary::Unstructured::new(&[0]);
    assert!(
        <grammar::nested::Repeated<'static> as recursa::Arbitrary>::arbitrary(
            &mut wrapper_bytes,
        )
        .is_ok()
    );
    let mut wrapper_bytes = recursa::__private::arbitrary::Unstructured::new(&[0]);
    assert!(
        <grammar::nested::Nonempty<'static> as recursa::Arbitrary>::arbitrary(
            &mut wrapper_bytes,
        )
        .is_ok()
    );
}
"#,
    )
    .expect("write the local Visit consumer");

    assert_success(&run_fixture(&package));
    assert_no_public_optional_token_impls(&package);
    assert_success(&lint_fixture(&package));
}

#[test]
fn generated_pretty_keeps_content_repetition_separators_and_closing_delimiters_tight() {
    let package = scaffold_fixture();
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::r#type,
    derives(Pretty),
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords {
        LIST => "LIST",
        TAIL => "TAIL",
    }
    punctuation {
        COMMA => ",",
        LPAREN => "(",
        RPAREN => ")",
        STAR => "*",
    }
}

#[derive(recursa::Node)]
pub struct Word<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub value: WordText<'input>,
}

#[derive(recursa::Node)]
#[tok(LPAREN, this, RPAREN)]
pub struct TextList<'input> {
    #[sep(COMMA)]
    pub values: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
pub enum Open {
    #[tok(LPAREN)]
    Value,
}

#[derive(recursa::Node)]
pub enum Star {
    #[tok(STAR)]
    Value,
}

#[derive(recursa::Node)]
pub enum Close {
    #[tok(RPAREN)]
    Value,
}

#[derive(recursa::Node)]
pub struct StarCall {
    pub open: Open,
    pub star: Star,
    pub close: Close,
}

#[derive(recursa::Node)]
#[tok(TAIL)]
pub struct Tail;

#[derive(recursa::Node)]
pub struct WordWithOptionalStar<'input> {
    pub word: Word<'input>,
    #[presence(STAR)]
    #[pretty(break_before = soft)]
    pub star: bool,
}

#[derive(recursa::Node)]
#[tok(LIST)]
pub struct OptionalTailList<'input> {
    #[sep(COMMA)]
    pub values: recursa::Vec1<WordWithOptionalStar<'input>>,
    pub tail: Option<Tail>,
}
"#,
    )
    .expect("write the Pretty boundary grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa::Pretty as _;
use recursa_generated_one_node::r#type as grammar;

fn main() {
    let mut failures = Vec::new();

    let lexed = grammar::lex("(alpha,beta)");
    let mut input = lexed.input();
    let list = grammar::TextList::parse(&mut input).expect("content list parses");
    assert!(input.is_eof());
    let rendered = list.render();
    if rendered != "(alpha, beta)" {
        failures.push(format!("content repetition rendered as {rendered:?}"));
    }

    let lexed = grammar::lex("(*)");
    let mut input = lexed.input();
    let call = grammar::StarCall::parse(&mut input).expect("star call parses");
    assert!(input.is_eof());
    let rendered = call.render();
    if rendered != "(*)" {
        failures.push(format!("star call rendered as {rendered:?}"));
    }

    let lexed = grammar::lex("LIST alpha,beta");
    let mut input = lexed.input();
    let list = grammar::OptionalTailList::parse(&mut input)
        .expect("Node repetition followed by an absent optional tail parses");
    assert!(input.is_eof());
    let rendered = list.render();
    if rendered != "LIST alpha, beta" {
        failures.push(format!("optional-tail list rendered as {rendered:?}"));
    }

    assert!(failures.is_empty(), "{}", failures.join("; "));
}
"#,
    )
    .expect("write the Pretty boundary consumer");

    assert_success(&run_fixture(&package));
}

#[test]
fn generated_pretty_honors_bilateral_repetition_separator_spacing() {
    let package = scaffold_fixture();
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::r#type,
    derives(Pretty),
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords {
        WORD_SEP => "OR",
        TAIL => "TAIL",
    }
    punctuation {
        DOT => ".", spacing = tight,
        BEFORE_SEP => "@", spacing = before,
        AFTER_SEP => "%", spacing = after,
        AROUND_SEP => "&", spacing = around,
        OPEN_SEP => "(",
        ITEM_START => "<", spacing = before,
        ITEM_END => ">", spacing = after,
    }
}

#[derive(recursa::Node)]
pub struct Word<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub value: WordText<'input>,
}

#[derive(recursa::Node)]
pub struct TightPath<'input> {
    #[sep(DOT)]
    pub parts: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
pub struct BeforePath<'input> {
    #[sep(BEFORE_SEP)]
    pub parts: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
pub struct AfterPath<'input> {
    #[sep(AFTER_SEP)]
    pub parts: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
pub struct AroundPath<'input> {
    #[sep(AROUND_SEP)]
    pub parts: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
pub struct WordPath<'input> {
    #[sep(WORD_SEP)]
    pub parts: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
pub struct OpeningPath<'input> {
    #[sep(OPEN_SEP)]
    pub parts: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
pub struct LeadingAroundPath<'input> {
    #[sep(AROUND_SEP)]
    #[pretty(separator = leading)]
    pub parts: recursa::Vec1<WordText<'input>>,
}

#[derive(recursa::Node)]
#[tok(TAIL)]
pub struct Tail;

#[derive(recursa::Node)]
pub struct TrailingAroundPath<'input> {
    #[sep(AROUND_SEP, trailing)]
    #[pretty(trailing_separator = always)]
    pub parts: recursa::Vec1<WordText<'input>>,
    pub tail: Tail,
}

#[derive(recursa::Node)]
#[tok(ITEM_START, this, ITEM_END)]
pub struct BoundedWord<'input> {
    pub value: WordText<'input>,
}

#[derive(recursa::Node)]
pub struct BilateralPath<'input> {
    #[sep(DOT)]
    pub parts: recursa::Vec1<BoundedWord<'input>>,
}
"#,
    )
    .expect("write the separator-spacing grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa::Pretty as _;
use recursa_generated_one_node::r#type as grammar;

macro_rules! parse {
    ($ty:ty, $source:expr) => {{
        let lexed = grammar::lex($source);
        let mut input = lexed.input();
        let parsed = <$ty>::parse(&mut input).expect("separated path parses");
        assert!(input.is_eof());
        parsed
    }};
}

fn main() {
    let tight = parse!(grammar::TightPath<'_>, "alpha.beta");
    assert_eq!(tight.render(), "alpha.beta");
    assert_eq!(
        tight.render_with(recursa::PrettyConfig::new(3, 2)),
        "alpha.beta",
        "a Tight separator boundary is indivisible",
    );

    let before = parse!(grammar::BeforePath<'_>, "alpha @beta");
    assert_eq!(before.render(), "alpha @beta");
    assert_eq!(
        before.render_with(recursa::PrettyConfig::new(8, 2)),
        "alpha @\nbeta",
    );

    let after = parse!(grammar::AfterPath<'_>, "alpha% beta");
    assert_eq!(after.render(), "alpha% beta");
    assert_eq!(
        after.render_with(recursa::PrettyConfig::new(8, 2)),
        "alpha%\nbeta",
    );

    let around = parse!(grammar::AroundPath<'_>, "alpha & beta");
    assert_eq!(around.render(), "alpha & beta");
    assert_eq!(
        around.render_with(recursa::PrettyConfig::new(8, 2)),
        "alpha &\nbeta",
    );
    let (around_text, around_map) = around.render_mapped();
    assert_eq!(around_text, "alpha & beta");
    assert!(around_map.entries().iter().any(|entry| {
        entry.output() == (6..7) && entry.source() == recursa::Span::new(6, 7).unwrap()
    }));

    let word = parse!(grammar::WordPath<'_>, "alpha OR beta");
    assert_eq!(word.render(), "alpha OR beta");
    assert_eq!(
        word.render_with(recursa::PrettyConfig::new(8, 2)),
        "alpha OR\nbeta",
    );

    let opening = parse!(grammar::OpeningPath<'_>, "alpha(beta");
    assert_eq!(opening.render(), "alpha(beta");
    assert_eq!(
        opening.render_with(recursa::PrettyConfig::new(3, 2)),
        "alpha(beta",
        "an Auto opening delimiter stays tight after itself",
    );

    let leading = parse!(grammar::LeadingAroundPath<'_>, "alpha & beta");
    assert_eq!(
        leading.render_with(recursa::PrettyConfig::new(8, 2)),
        "alpha\n& beta",
    );

    let trailing = parse!(grammar::TrailingAroundPath<'_>, "alpha &TAIL");
    let (trailing_text, trailing_map) = trailing.render_mapped();
    assert_eq!(trailing_text, "alpha & TAIL");
    assert!(trailing_map.entries().iter().any(|entry| {
        entry.output() == (6..7) && entry.source() == recursa::Span::new(6, 7).unwrap()
    }));

    let synthetic_trailing = parse!(grammar::TrailingAroundPath<'_>, "alpha TAIL");
    let (synthetic_text, synthetic_map) = synthetic_trailing.render_mapped();
    assert_eq!(synthetic_text, "alpha & TAIL");
    assert!(synthetic_map.entries().iter().any(|entry| {
        entry.output() == (6..7) && entry.source().is_synthetic()
    }));

    let bilateral = parse!(grammar::BilateralPath<'_>, "<alpha>.<beta>");
    assert_eq!(
        bilateral.render(),
        "<alpha> . <beta>",
        "item After/Before requests combine with both sides of the separator",
    );
}
"#,
    )
    .expect("write the separator-spacing consumer");

    assert_success(&run_fixture(&package));
}

#[test]
fn debug_emission_preserves_compiled_semantics_for_every_capability_combination() {
    for (name, diagrams, explorer) in [
        ("core", false, false),
        ("diagrams", true, false),
        ("explorer", false, true),
        ("combined", true, true),
    ] {
        let package = scaffold_fixture();
        configure_capabilities(&package, diagrams, explorer);
        assert_success(&run_fixture(&package));
        let selected = find_selected_generation(&target_dir(&package));
        let selector = fs::read_to_string(&selected).unwrap();
        let identity = selector
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("// recursa-generation:"))
            .unwrap()
            .to_owned();
        let generation = selected
            .parent()
            .unwrap()
            .join("recursa-generations")
            .join(&identity);
        let compiled = read_generation_files(&generation);

        let destination = tempfile::tempdir().unwrap();
        assert_success(&run_fixture_with_debug(&package, destination.path()));
        assert_eq!(
            fs::read_to_string(&selected).unwrap(),
            selector,
            "{name}: debug environment changed the selected identity or handshake"
        );
        assert_eq!(
            read_generation_files(&generation),
            compiled,
            "{name}: debug environment changed authoritative compiled payload"
        );

        let index_path = find_one_debug_index(destination.path());
        let index = fs::read_to_string(&index_path).unwrap();
        let bundle = index_path.parent().unwrap();
        assert_eq!(index.matches("root\t").count(), 1, "{name}: one root view");
        assert_eq!(
            index.matches("node\t").count(),
            compiled
                .iter()
                .filter(|(file, _)| file.starts_with("node-") && file.ends_with(".rs"))
                .count(),
            "{name}: every compiled Node shard has one logical view"
        );
        assert_eq!(
            index.contains("capability\tdiagrams:aggregate\t"),
            diagrams,
            "{name}: diagram fragment selection"
        );
        assert_eq!(
            index.contains("capability\texplorer:static-schema\t"),
            explorer,
            "{name}: explorer schema fragment selection"
        );
        assert_eq!(
            index.contains("capability\texplorer:lexer\t"),
            explorer,
            "{name}: explorer lexer fragment selection"
        );
        for line in index.lines().skip(2) {
            let private_name = line.split('\t').nth(2).expect("indexed private filename");
            let source = fs::read_to_string(bundle.join(private_name)).unwrap();
            let parsed = syn::parse_file(&source).expect("every logical output is parseable Rust");
            assert_eq!(
                prettyplease::unparse(&parsed),
                source,
                "{name}: every logical output is deterministically formatted"
            );
        }

        if name == "core" {
            let second_destination = tempfile::tempdir().unwrap();
            assert_success(&run_fixture_with_debug(&package, second_destination.path()));
            assert_eq!(
                fs::read_to_string(find_one_debug_index(second_destination.path())).unwrap(),
                index,
                "the destination is not part of deterministic debug content"
            );

            fs::write(bundle.join("root.rs"), "human inspection notes").unwrap();
            fs::write(bundle.join("extra-notes.txt"), "temporary notes").unwrap();
            assert_success(&run_fixture_with_debug(&package, destination.path()));
            assert_eq!(fs::read_to_string(&selected).unwrap(), selector);
            assert!(!bundle.join("extra-notes.txt").exists());
            syn::parse_file(&fs::read_to_string(bundle.join("root.rs")).unwrap())
                .expect("a rerun replaces human edits instead of loading them");
        }
        if name == "explorer" {
            assert_success(&lint_fixture(&package));
        }
    }
}

fn configure_capabilities(package: &TempDir, diagrams: bool, explorer: bool) {
    let mut features = Vec::new();
    let grammar = package.path().join("src/grammar.rs");
    let mut source = fs::read_to_string(&grammar).expect("read capability matrix grammar");
    source = source
        .replace("module = crate::r#type", "module = crate::grammar")
        .replace("pub struct r#match", "pub struct Root");
    let library = package.path().join("src/lib.rs");
    let library_source = fs::read_to_string(&library)
        .expect("read capability matrix library")
        .replace("pub mod r#type", "pub mod grammar");
    fs::write(library, library_source).expect("configure capability matrix library");
    let main = package.path().join("src/main.rs");
    let main_source = fs::read_to_string(&main)
        .expect("read capability matrix consumer")
        .replace("r#type as grammar", "grammar")
        .replace("grammar::r#match", "grammar::Root")
        .replace("Some(\"type\")", "Some(\"grammar\")");
    fs::write(main, main_source).expect("configure capability matrix consumer");
    if diagrams {
        source = source.replacen(
            "    module = crate::grammar,\n",
            "    module = crate::grammar,\n    diagrams,\n",
            1,
        );
        features.push("\"diagrams\"");
    }
    if explorer {
        features.push("\"explorer\"");
    }
    if diagrams {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        fs::copy(
            manifest_dir.join(FIXTURE).join("Cargo.lock"),
            package.path().join("Cargo.lock"),
        )
        .expect("select capability-complete fixture lock");
    }
    fs::write(grammar, source).expect("configure capability matrix grammar");
    if features.is_empty() {
        return;
    }

    let manifest_path = package.path().join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).expect("read capability matrix manifest");
    let features = features.join(", ");
    let manifest = manifest
        .lines()
        .map(|line| {
            if line.starts_with("recursa = { path = ")
                || line.starts_with("recursa-codegen = { path = ")
            {
                format!(
                    "{}, features = [{features}] }}",
                    line.trim_end_matches(" }")
                )
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(manifest_path, format!("{manifest}\n"))
        .expect("configure capability matrix manifest");
}

fn read_generation_files(generation: &Path) -> Vec<(String, Vec<u8>)> {
    fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(directory).expect("read authoritative Generation") {
            let path = entry.expect("read authoritative entry").path();
            if path.is_dir() {
                collect(root, &path, files);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .expect("authoritative payload lives below the Generation")
                .to_string_lossy()
                .into_owned();
            files.push((
                relative,
                fs::read(path).expect("read authoritative payload"),
            ));
        }
    }
    let mut files = Vec::new();
    collect(generation, generation, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    copy(&fixture, package.path(), "build.rs");
    copy_as(&fixture, package.path(), "Cargo.core.lock", "Cargo.lock");
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

fn assert_no_public_optional_token_impls(package: &TempDir) {
    let selected = find_selected_generation(&target_dir(package));
    let selector = fs::read_to_string(&selected).expect("read selected Generation");
    let identity = selector
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// recursa-generation:"))
        .expect("selected Generation identity");
    let root = selected
        .parent()
        .unwrap()
        .join("recursa-generations")
        .join(identity)
        .join("root.rs");
    let root = syn::parse_file(&fs::read_to_string(root).expect("read generated root"))
        .expect("parse generated root");

    #[derive(Default)]
    struct PublicOptionalTokenImpls(Vec<String>);

    impl<'ast> syn::visit::Visit<'ast> for PublicOptionalTokenImpls {
        fn visit_item_impl(&mut self, implementation: &'ast syn::ItemImpl) {
            if let syn::Type::Path(self_type) = implementation.self_ty.as_ref()
                && let Some(self_ident) =
                    self_type.path.segments.last().map(|segment| &segment.ident)
                && let Some((_, trait_path, _)) = &implementation.trait_
                && let Some(trait_ident) = trait_path.segments.last().map(|segment| &segment.ident)
                && self_ident.to_string().starts_with("__RecursaContentToken")
                && matches!(
                    trait_ident.to_string().as_str(),
                    "Arbitrary" | "Serialize" | "Deserialize"
                )
            {
                self.0.push(trait_ident.to_string());
            }
            syn::visit::visit_item_impl(self, implementation);
        }

        fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
            if let Ok(nested) = syn::parse2::<syn::File>(item.mac.tokens.clone()) {
                let mut nested_public = Self::default();
                syn::visit::Visit::visit_file(&mut nested_public, &nested);
                self.0.extend(nested_public.0);
            }
            syn::visit::visit_item_macro(self, item);
        }
    }

    let mut public = PublicOptionalTokenImpls::default();
    syn::visit::Visit::visit_file(&mut public, &root);
    assert!(
        public.0.is_empty(),
        "private support must not change public token recipes: {:?}",
        public.0,
    );
}

fn enable_arbitrary_and_serde(package: &TempDir) {
    let manifest_path = package.path().join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .expect("read generated grammar fixture manifest")
        .replace(
            "recursa = { path = ",
            "recursa = { features = [\"arbitrary\", \"serde\"], path = ",
        )
        .replace(
            "\n[build-dependencies]",
            "\nserde_json = \"1\"\n\n[build-dependencies]",
        );
    fs::write(&manifest_path, manifest).expect("enable optional local-trait capabilities");

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .arg("generate-lockfile")
        .arg("--offline")
        .current_dir(package.path())
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("generate optional-capability fixture lock");
    assert_success(&output);
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generated grammar fixture file");
}

fn copy_as(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy generated grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run generated grammar fixture")
}

fn lint_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
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
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("lint generated grammar fixture")
}

fn run_fixture_with_debug(package: &TempDir, debug_directory: &Path) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("RECURSA_DEBUG_DIR", debug_directory)
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command
        .output()
        .expect("run generated grammar fixture with debug emission")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn find_selected_generation(directory: &Path) -> PathBuf {
    let mut found = Vec::new();
    find_named(directory, "recursa-selected.rs", &mut found);
    assert_eq!(found.len(), 1, "one selected Generation: {found:?}");
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

fn find_one_debug_index(directory: &Path) -> PathBuf {
    let mut found = Vec::new();
    find_named(directory, "index", &mut found);
    assert_eq!(found.len(), 1, "one generated-code debug index: {found:?}");
    found.pop().unwrap()
}

fn force_consumer_recompile(package: &TempDir, marker: &str) {
    let main = package.path().join("src/main.rs");
    let mut source = fs::read_to_string(&main).expect("read downstream consumer");
    source.push_str(&format!("\n// {marker}\n"));
    fs::write(main, source).expect("touch downstream consumer");
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
