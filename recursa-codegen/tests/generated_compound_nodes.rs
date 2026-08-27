use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/compound-nodes";

#[test]
fn a_downstream_crate_parses_authored_compound_nodes_and_views() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);

    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "compound-nodes-ok"
    );
}

#[test]
fn configured_feature_recipes_require_the_matching_facade_capability_once() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read fixture manifest");
    fs::write(
        &manifest,
        source.replace("\", features = [\"arbitrary\", \"serde\"] }", "\" }"),
    )
    .expect("disable facade capabilities");

    let output = run_fixture_unlocked(&package);
    assert!(!output.status.success(), "missing capabilities must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("error: error[RCA9005]").count(),
        1,
        "stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("E0433"),
        "root gating suppresses cascades:\n{stderr}"
    );
}

#[test]
fn extra_facade_capabilities_are_harmless() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read grammar");
    fs::write(
        &grammar,
        source.replace(", Arbitrary, serde::Serialize, serde::Deserialize", ""),
    )
    .expect("remove feature-backed recipes while retaining facade features");
    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read downstream consumer");
    let source = source.replace(
        "    fn assert_arbitrary<T: for<'arbitrary> recursa::Arbitrary<'arbitrary>>() {}\n    assert_arbitrary::<grammar::Record<'static>>();\n\n",
        "",
    );
    fs::write(
        &main,
        source.replace(
            "    let mut arbitrary_input = recursa::__private::arbitrary::Unstructured::new(&[0; 64]);\n    let arbitrary_record =\n        <grammar::Record<'static> as recursa::Arbitrary>::arbitrary(&mut arbitrary_input)\n            .expect(\"configured Arbitrary creates a Record\");\n    let arbitrary_rendered = recursa::Parsed::synthetic(arbitrary_record).render();\n    let _roundtrip = parse_spanned!(grammar::Record<'_>, arbitrary_rendered.as_str());\n\n",
            "",
        ),
    )
    .expect("remove assertion for the removed recipe");
    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "extra capabilities failed\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn unrequested_diagram_capability_is_rejected_exactly_once() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read fixture manifest");
    fs::write(
        &manifest,
        source.replace(
            "features = [\"arbitrary\", \"serde\"]",
            "features = [\"arbitrary\", \"serde\", \"diagrams\"]",
        ),
    )
    .expect("enable an unrequested facade capability");

    let output = run_fixture_unlocked(&package);
    assert!(!output.status.success(), "unrequested diagrams must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("error: error[RCA9005]").count(),
        1,
        "{stderr}"
    );
}

#[test]
fn explorer_adapters_expose_erased_schema_occurrences_lexing_pretty_and_samples() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read downstream consumer");
    let assertions = r#"
    let adapter = grammar::Record::explorer();
    let schema = adapter.schema();
    assert_eq!(schema.root().name(), "Record");
    assert!(schema.root().qualified_name().ends_with("grammar::Record"));
    assert!(adapter.capabilities().pretty());
    assert!(adapter.capabilities().arbitrary());
    assert!(schema.nodes().any(|node| node.qualified_name().ends_with("private_nodes::Hidden")));
    let duplicate_names = schema.nodes().filter(|node| node.name() == "Duplicate")
        .map(|node| node.qualified_name()).collect::<Vec<_>>();
    assert!(duplicate_names.len() >= 2);

    let lexical = adapter.lex("ReCoRd:alpha 42");
    assert_eq!(lexical.tokens().len(), 4);
    assert_eq!(lexical.errors().len(), 0);
    let first = lexical.tokens().next().unwrap();
    assert_eq!(first.text(), "ReCoRd");
    assert_eq!(first.schema().canonical_text(), Some("RECORD"));
    assert_eq!(first.schema().highlight_class(), recursa::explorer::HighlightClass::Keyword);

    let parsed = parse_spanned!(grammar::Record<'_>, "ReCoRd:alpha 42");
    let document = recursa::explorer::ExplorerDocument::from_parsed(parsed);
    assert_eq!(document.provenance(), recursa::explorer::Provenance::Parsed);
    let pretty = document.pretty(&recursa::PrettyConfig::default()).unwrap();
    assert_eq!(pretty.rendered, "ReCoRd:alpha 42");
    assert_eq!(pretty.source_map.source_at(0), Some(recursa::Span::new(0, 6).unwrap()));
    let root = document.root();
    let recursa::explorer::OccurrenceSchema::Node(root_schema) = root.schema() else {
        panic!("root is a Node schema")
    };
    assert_eq!(root_schema.name(), "Record");
    assert_eq!(root.span(), Some(recursa::Span::new(0, 15).unwrap()));
    assert_eq!(root.token_text(), None);
    let mut elements = root.elements();
    assert_eq!(elements.len(), 4);
    assert_eq!(elements.next().unwrap().text(), Some("ReCoRd"));
    assert_eq!(elements.next().unwrap().text(), Some(":"));
    let label = elements.next().unwrap();
    assert_eq!(label.schema().name(), "label");
    assert_eq!(label.children().len(), 1);
    let label_node = label.children().next().unwrap();
    let recursa::explorer::OccurrenceSchema::Node(label_schema) = label_node.schema() else {
        panic!("label is a Node")
    };
    assert_eq!(label_schema.name(), "Label");
    let token = label_node.elements().next().unwrap().children().next().unwrap();
    assert_eq!(token.token_text(), Some("alpha"));
    assert!(matches!(token.schema(), recursa::explorer::OccurrenceSchema::ContentToken(_)));
    let count = elements.next().unwrap();
    assert_eq!(count.schema().cardinality(), recursa::explorer::Cardinality::Optional);
    assert_eq!(count.children().len(), 1);

    let absent = recursa::explorer::ExplorerDocument::from_parsed(
        parse_spanned!(grammar::Record<'_>, "record:beta"),
    );
    assert_eq!(absent.root().elements().nth(3).unwrap().children().len(), 0);

    fn exact_fused<I: ExactSizeIterator + std::iter::FusedIterator>(_: &I) {}
    exact_fused(&schema.nodes());
    exact_fused(&schema.tokens());
    exact_fused(&document.root().elements());
    exact_fused(&lexical.tokens());
    exact_fused(&lexical.errors());

    let entry_adapter = grammar::Entry::explorer();
    let variants = entry_adapter.schema().root().variants()
        .map(|variant| (variant.name(), variant.elements().len()))
        .collect::<Vec<_>>();
    assert_eq!(variants, [("Record", 1), ("Pair", 1), ("Empty", 1)]);
    let empty = recursa::explorer::ExplorerDocument::from_parsed(
        parse_spanned!(grammar::Entry<'_>, "empty"),
    );
    assert_eq!(empty.root().active_variant().unwrap().name(), "Empty");

    let repeated_adapter = grammar::SomeCounts::explorer();
    let repeated_schema = repeated_adapter.schema().root().elements().nth(1).unwrap();
    assert_eq!(repeated_schema.cardinality(), recursa::explorer::Cardinality::OptionalOneOrMore);
    let repeated = recursa::explorer::ExplorerDocument::from_parsed(
        parse_spanned!(grammar::SomeCounts<'_>, "right 1:2"),
    );
    assert_eq!(repeated.root().elements().nth(1).unwrap().children().len(), 2);

    let private = recursa::explorer::ExplorerDocument::from_parsed(
        parse_spanned!(grammar::PrivateRoot<'_>, "left ~secret"),
    );
    let hidden = private.root().elements().nth(1).unwrap().children().next().unwrap();
    let recursa::explorer::OccurrenceSchema::Node(hidden_schema) = hidden.schema() else {
        panic!("hidden descendant is a Node")
    };
    assert_eq!(hidden_schema.name(), "Hidden");
    assert_eq!(hidden.elements().next().unwrap().children().next().unwrap().token_text(), Some("~secret"));

    let entropy = [7_u8; 512];
    let sample = adapter.sample(&entropy).expect("Arbitrary supported")
        .expect("deterministic synthetic sample");
    let repeated_sample = adapter.sample(&entropy).expect("Arbitrary supported")
        .expect("same entropy remains a valid synthetic sample");
    assert_eq!(sample.provenance(), recursa::explorer::Provenance::Synthetic);
    assert!(sample.source().is_none());
    let sample_pretty = sample.pretty(&recursa::PrettyConfig::default()).unwrap();
    assert_eq!(
        sample_pretty,
        repeated_sample.pretty(&recursa::PrettyConfig::default()).unwrap(),
    );
"#;
    fs::write(
        main,
        source.replace(
            "    grammar::assert_private_sibling_spans();",
            &format!("{assertions}\n    grammar::assert_private_sibling_spans();"),
        ),
    )
    .expect("add explorer assertions");

    let output = run_fixture_unlocked(&package);
    assert!(
        output.status.success(),
        "explorer fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn explorer_capability_requires_a_source_visible_visit_root() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    fs::write(
        &grammar,
        r#"recursa::grammar! {
    module = crate::grammar,
    derives(Debug),
}

recursa::tokens! { keywords { ONLY => "only" } }

#[derive(recursa::Node)]
#[tok(ONLY)]
pub struct Only;
"#,
    )
    .expect("create a grammar without Visit");
    fs::write(package.path().join("src/main.rs"), "fn main() {}\n").expect("replace consumer");

    let output = run_fixture_unlocked(&package);
    assert!(!output.status.success(), "explorer without Visit must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("error: error[RCA9005]").count(),
        1,
        "{stderr}"
    );
}

#[test]
fn explorer_public_api_and_generated_payload_are_absent_when_the_feature_is_off() {
    let package = scaffold_fixture();
    let main = package.path().join("src/main.rs");
    fs::write(
        &main,
        r#"use recursa_generated_compound_nodes::grammar;

fn main() {
    let _: Option<recursa::explorer::Adapter> = None;
}
"#,
    )
    .expect("attempt to use the disabled explorer API");

    let output = run_fixture_unlocked(&package);
    assert!(
        !output.status.success(),
        "disabled explorer must expose no API"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not find `explorer` in `recursa`")
            || stderr.contains("could not find `explorer`"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn explorer_names_are_unreserved_when_the_generator_capability_is_off() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let mut source = fs::read_to_string(&grammar).expect("read compound grammar");
    source.push_str(
        "\nstatic __RECURSA_EXPLORER_SCHEMA_0: () = ();\nimpl<'input> Record<'input> { pub fn explorer() {} }\n",
    );
    fs::write(grammar, source).expect("insert names reserved only by explorer generation");

    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "feature-off explorer names must remain authored:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let generation = generation_directories(&target_dir(&package))
        .into_iter()
        .next()
        .expect("feature-off fixture publishes one Generation");
    for entry in fs::read_dir(generation).expect("read feature-off Generation") {
        let path = entry.expect("read feature-off artifact").path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let emitted = fs::read_to_string(&path).expect("read feature-off Rust artifact");
            assert!(
                !emitted.contains("RECURSA_EXPLORER")
                    && !emitted.contains("__recursa_explorer")
                    && !emitted.contains("ExplorerPlan"),
                "feature-off Generation contains explorer payload in {}:\n{emitted}",
                path.display(),
            );
        }
    }
}

#[test]
fn diagrams_are_published_in_aggregate_and_declaration_local_rustdoc() {
    let package = scaffold_fixture();
    configure_diagrams(&package, true, true);

    let output = run_fixture_doc(&package, true);
    assert!(
        output.status.success(),
        "diagram rustdoc failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let docs = rendered_rustdoc(&package);
    let aggregate = docs
        .iter()
        .find(|(path, source)| {
            path.ends_with("grammar_docs/index.html") && source.contains("RECURSA_GRAMMAR_DIAGRAM")
        })
        .expect("aggregate grammar documentation page");
    assert!(
        aggregate.1.contains("struct.Record.html"),
        "aggregate rustdoc:\n{}",
        aggregate.1
    );
    assert!(aggregate.1.contains("struct.Pair.html"));
    assert!(
        !aggregate.1.contains("struct.Hidden.html"),
        "the public aggregate must not link a private Node"
    );
    assert!(
        aggregate
            .1
            .contains("Pair ::= \"PAIR\" grammar::Label Option&lt;grammar::Count&gt;"),
        "tuple fields must use referenced frozen terminal names:\n{}",
        aggregate.1,
    );
    assert!(
        aggregate.1.contains(
            "ManyLabels ::= \"LEFT\" Vec&lt;grammar::Label&gt; separated by \":\" (optional trailing \":\")"
        ),
        "zero-or-more separator semantics must be rendered:\n{}",
        aggregate.1,
    );
    assert!(
        aggregate.1.contains(
            "SomeCounts ::= \"RIGHT\" Option&lt;Vec1&lt;grammar::Count&gt;&gt; separated by \":\""
        ),
        "OptionalOneOrMore semantics must remain distinct:\n{}",
        aggregate.1,
    );

    let record = docs
        .iter()
        .find(|(path, _)| path.ends_with("grammar/struct.Record.html"))
        .expect("public Record documentation page");
    assert!(record.1.contains("RECURSA_AUTHORED_RECORD_DOC"));
    assert!(record.1.contains("RECURSA_NODE_DIAGRAM"));

    assert!(
        docs.iter().any(|(path, source)| {
            path.ends_with("struct.Hidden.html") && source.contains("RECURSA_NODE_DIAGRAM")
        }),
        "private Nodes receive declaration-local diagrams under private-item docs"
    );

    let label_text = docs
        .iter()
        .find(|(path, _)| path.ends_with("grammar/type.LabelText.html"))
        .expect("generated content-token binding documentation page");
    assert!(label_text.1.contains("exact matched input text"));
    assert!(label_text.1.contains("Fixed tokens are syntax-only"));
}

#[test]
fn core_only_rustdoc_contains_no_diagram_payload() {
    let package = scaffold_fixture();
    let output = run_fixture_doc(&package, true);
    assert!(
        output.status.success(),
        "core rustdoc failed\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let docs = rendered_rustdoc(&package);
    assert!(
        docs.iter().all(|(_, source)| {
            !source.contains("RECURSA_GRAMMAR_DIAGRAM") && !source.contains("RECURSA_NODE_DIAGRAM")
        }),
        "a core-only Generation must contain no diagram documentation"
    );
    assert!(
        docs.iter()
            .all(|(path, _)| !path.ends_with("grammar_docs/index.html")),
        "a core-only Generation must not publish an aggregate diagram module"
    );
}

#[test]
fn a_diagram_generation_requires_the_matching_facade_capability_once() {
    let package = scaffold_fixture();
    configure_diagrams(&package, true, false);

    let output = run_fixture_unlocked(&package);
    assert!(
        !output.status.success(),
        "missing diagrams facade must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("error: error[RCA9005]").count(),
        1,
        "stderr:\n{stderr}"
    );
}

#[test]
fn a_core_generation_does_not_claim_an_unselected_diagram_payload() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read fixture manifest");
    fs::write(
        manifest,
        source.replace(
            "features = [\"arbitrary\", \"serde\"]",
            "features = [\"arbitrary\", \"serde\", \"diagrams\"]",
        ),
    )
    .expect("request an unavailable diagram loader");

    let output = run_fixture_unlocked(&package);
    assert!(!output.status.success(), "unselected diagrams must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("error: error[RCA9005]").count(), 1);
}

#[test]
fn an_unused_generator_diagram_capability_does_not_change_core_identity() {
    let core = scaffold_fixture();
    let output = run_fixture(&core);
    assert!(
        output.status.success(),
        "core fixture failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let core_identity = only_generation_identity(&core);

    let capable = scaffold_fixture();
    let manifest = capable.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read fixture manifest");
    fs::write(
        manifest,
        source.replace(
            "recursa-codegen = { path = \"",
            "recursa-codegen = { features = [\"diagrams\"], path = \"",
        ),
    )
    .expect("enable the unused generator capability");
    let output = run_fixture_unlocked(&capable);
    assert!(
        output.status.success(),
        "capable core fixture failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(core_identity, only_generation_identity(&capable));
}

#[test]
fn diagram_generations_support_a_renamed_facade_and_deterministic_reselection() {
    let package = scaffold_fixture();
    configure_diagrams(&package, true, true);
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read fixture manifest");
    fs::write(
        &manifest,
        source.replacen("recursa = {", "my_recursa = { package = \"recursa\",", 1),
    )
    .expect("rename facade dependency");
    for relative in ["src/grammar.rs", "src/main.rs"] {
        let path = package.path().join(relative);
        let source = fs::read_to_string(&path).expect("read fixture source");
        let mut renamed = source.replace("recursa::", "my_recursa::");
        if relative == "src/grammar.rs" {
            renamed = renamed.replace(
                "module = crate::grammar,",
                "module = crate::grammar,\n    facade = my_recursa,",
            );
        }
        fs::write(path, renamed).expect("rewrite facade references");
    }

    let first = run_fixture_unlocked(&package);
    assert!(
        first.status.success(),
        "renamed diagram facade failed\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let before = generation_directories(&target_dir(&package));
    let second = run_fixture_unlocked(&package);
    assert!(
        second.status.success(),
        "diagram reselection failed\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(before, generation_directories(&target_dir(&package)));
}

#[test]
fn requesting_diagrams_without_the_generator_capability_fails_deterministically() {
    let package = scaffold_fixture();
    configure_diagrams(&package, false, true);

    let output = run_fixture_unlocked(&package);
    assert!(
        !output.status.success(),
        "missing generator capability must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("diagram generation was requested but recursa-codegen was built without its `diagrams` capability"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn a_renamed_facade_is_used_by_generated_roots_nodes_and_feature_recipes() {
    let package = scaffold_fixture();
    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read fixture manifest");
    fs::write(
        &manifest,
        source.replacen("recursa = {", "my_recursa = { package = \"recursa\",", 1),
    )
    .expect("rename facade dependency");
    for relative in ["src/grammar.rs", "src/main.rs"] {
        let path = package.path().join(relative);
        let source = fs::read_to_string(&path).expect("read fixture source");
        let mut renamed = source.replace("recursa::", "my_recursa::");
        if relative == "src/grammar.rs" {
            renamed = renamed.replace(
                "module = crate::grammar,",
                "module = crate::grammar,\n    facade = my_recursa,",
            );
        }
        fs::write(path, renamed).expect("rewrite facade references");
    }

    let output = run_fixture_unlocked(&package);
    assert!(
        output.status.success(),
        "renamed facade failed\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn private_protocol_mismatch_is_reported_once_at_the_grammar_root() {
    let package = scaffold_fixture();
    let first = run_fixture(&package);
    assert!(
        first.status.success(),
        "initial fixture failed\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let generation = generation_directories(&target_dir(&package))
        .into_iter()
        .next()
        .expect("published generation");
    let root = generation.join("root.rs");
    let source = fs::read_to_string(&root).expect("read generated root");
    fs::write(&root, source.replace("protocol_1", "protocol_999"))
        .expect("inject private protocol mismatch");

    let output = run_fixture(&package);
    assert!(!output.status.success(), "protocol mismatch must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("error: error[RCA9003]").count(),
        1,
        "stderr:\n{stderr}"
    );
    assert!(!stderr.contains("E0308"), "stderr:\n{stderr}");
}

#[test]
fn canonical_content_token_types_are_not_exposed_at_the_grammar_root() {
    let package = scaffold_fixture();
    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read downstream consumer");
    fs::write(
        &main,
        source.replace(
            "fn main() {",
            "fn expose_canonical(_: grammar::__RecursaContentToken0<'static>) {}\n\nfn main() {",
        ),
    )
    .expect("try to name a canonical content token");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "canonical token must remain private"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot find type `__RecursaContentToken0`")
            || stderr.contains("not found in `grammar`"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn visitor_events_cannot_substitute_or_retain_borrowed_input_as_static() {
    let package = scaffold_fixture();
    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read downstream consumer");
    let proof = r#"
fn substitute_events<'input>(
    events: grammar::__RecursaVisitEvents<'input>,
) -> grammar::__RecursaVisitEvents<'static> {
    events
}

struct StaticRetainer(Option<&'static str>);

impl<'input> recursa::Visitor<grammar::LabelText<'input>> for StaticRetainer {
    type Error = ();

    fn enter(
        &mut self,
        token: &grammar::LabelText<'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>> {
        self.0 = Some(token.text());
        std::ops::ControlFlow::Continue(())
    }
}

"#;
    fs::write(
        &main,
        source.replace("fn main() {", &format!("{proof}fn main() {{")),
    )
    .expect("insert lifetime-safety proof");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "lifetime erasure must not compile"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lifetime may not live long enough")
            || stderr.contains("requires that `'input` must outlive `'static`"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn root_visit_selection_keeps_event_families_when_every_node_omits_the_impl() {
    let package = scaffold_fixture();
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::grammar,
    derives(Visit, VisitMut),
}

recursa::tokens! {
    keywords { ONLY => "only" }
}

#[derive(recursa::Node)]
#[node(omit(Visit, VisitMut))]
#[tok(ONLY)]
pub struct Only;

impl recursa::Visit for Only {
    type Events = __RecursaVisitEvents<'static>;

    fn visit<V>(
        &self,
        visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<V::Error>>
    where
        V: recursa::TotalVisitor<Self::Events>,
    {
        match visitor.total_enter(__RecursaVisitEvent::__RecursaTarget_4_Only(self)) {
            std::ops::ControlFlow::Continue(())
            | std::ops::ControlFlow::Break(recursa::VisitBreak::SkipChildren) => {}
            other => return other,
        }
        visitor.total_exit(__RecursaVisitEvent::__RecursaTarget_4_Only(self))
    }
}

impl recursa::VisitMut for Only {
    type EventsMut = __RecursaVisitEventsMut<'static>;

    fn visit_mut<V>(
        &mut self,
        visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<V::Error>>
    where
        V: recursa::TotalVisitorMut<Self::EventsMut>,
    {
        match visitor.total_enter_mut(
            __RecursaVisitEventMut::__RecursaTarget_4_Only(self),
        ) {
            std::ops::ControlFlow::Continue(())
            | std::ops::ControlFlow::Break(recursa::VisitBreak::SkipChildren) => {}
            other => return other,
        }
        visitor.total_exit_mut(__RecursaVisitEventMut::__RecursaTarget_4_Only(self))
    }
}
"#,
    )
    .expect("write all-omitted visitor grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa_generated_compound_nodes::grammar;

fn main() {
    fn assert_visit<T: recursa::Visit + recursa::VisitMut>() {}
    assert_visit::<grammar::Only>();
}
"#,
    )
    .expect("write all-omitted visitor consumer");

    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "source-owned visitors need the generated event families\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explorer_root_eligibility_keeps_source_owned_visit_after_local_omit() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::grammar,
    derives(Visit),
}

recursa::tokens! { keywords { ONLY => "only" } }

#[derive(recursa::Node)]
#[node(omit(Visit))]
#[tok(ONLY)]
pub struct Only;

impl recursa::Visit for Only {
    type Events = __RecursaVisitEvents<'static>;

    fn visit<V>(
        &self,
        visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<V::Error>>
    where
        V: recursa::TotalVisitor<Self::Events>,
    {
        match visitor.total_enter(__RecursaVisitEvent::__RecursaTarget_4_Only(self)) {
            std::ops::ControlFlow::Continue(())
            | std::ops::ControlFlow::Break(recursa::VisitBreak::SkipChildren) => {}
            other => return other,
        }
        visitor.total_exit(__RecursaVisitEvent::__RecursaTarget_4_Only(self))
    }
}
"#,
    )
    .expect("write source-owned Visit explorer grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa_generated_compound_nodes::grammar;

fn main() {
    let adapter = grammar::Only::explorer();
    assert_eq!(adapter.schema().root().name(), "Only");
}
"#,
    )
    .expect("use source-owned Visit explorer root");

    let output = run_fixture_unlocked(&package);
    assert!(
        output.status.success(),
        "source-owned Visit must retain explorer eligibility:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn source_owned_visit_root_reserves_its_explorer_entry_method() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::grammar,
    derives(Visit),
}

recursa::tokens! { keywords { ONLY => "only" } }

#[derive(recursa::Node)]
#[node(omit(Visit))]
#[tok(ONLY)]
pub struct Only;

impl recursa::Visit for Only {
    type Events = __RecursaVisitEvents<'static>;

    fn visit<V>(
        &self,
        visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<V::Error>>
    where
        V: recursa::TotalVisitor<Self::Events>,
    {
        match visitor.total_enter(__RecursaVisitEvent::__RecursaTarget_4_Only(self)) {
            std::ops::ControlFlow::Continue(())
            | std::ops::ControlFlow::Break(recursa::VisitBreak::SkipChildren) => {}
            other => return other,
        }
        visitor.total_exit(__RecursaVisitEvent::__RecursaTarget_4_Only(self))
    }
}

impl Only { pub fn explorer() {} }
"#,
    )
    .expect("write source-owned Visit explorer collision");

    let output = run_fixture_unlocked(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "explorer collision must fail");
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("generated explorer entry method `Only::explorer` collides"),
        "stderr:\n{stderr}",
    );
}

#[test]
fn omitted_visit_without_a_source_impl_is_an_ordinary_rust_trait_error() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! {
    module = crate::grammar,
    derives(Visit),
}

recursa::tokens! { keywords { ONLY => "only" } }

#[derive(recursa::Node)]
#[node(omit(Visit))]
#[tok(ONLY)]
pub struct Only;
"#,
    )
    .expect("write missing source-owned Visit grammar");
    fs::write(
        package.path().join("src/main.rs"),
        r#"use recursa_generated_compound_nodes::grammar;

fn main() {
    let _ = grammar::Only::explorer();
}
"#,
    )
    .expect("request explorer root without its source-owned Visit impl");

    let output = run_fixture_unlocked(&package);
    assert!(!output.status.success(), "missing source Visit must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("RCA9005"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("Visit")
            && (stderr.contains("trait bound") || stderr.contains("unsatisfied")),
        "stderr:\n{stderr}",
    );
}

#[test]
fn parsed_values_and_views_exclude_owned_or_mutable_semantic_traits() {
    let package = scaffold_fixture();
    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read downstream consumer");
    fs::write(
        &main,
        source.replace(
            "let record = parse_spanned!(grammar::Record<'_>, \"ReCoRd:alpha 42\");",
            r#"let record = parse_spanned!(grammar::Record<'_>, "ReCoRd:alpha 42");
    fn requires_clone<T: Clone>(_: &T) {}
    fn requires_visit_mut<T: recursa::VisitMut>(_: &mut T) {}
    requires_clone(&record);
    let mut forbidden = record.root();
    requires_visit_mut(&mut forbidden);"#,
        ),
    )
    .expect("request excluded forwarding traits");

    let output = run_fixture(&package);
    assert!(!output.status.success(), "excluded traits must not compile");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Parsed<'_, recursa_generated_compound_nodes::grammar::Record<'_>>: Clone")
            || stderr.contains("Parsed<'_, Record<'_>>: Clone"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("RecordView<'_, '_>: VisitMut"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn a_concrete_view_name_collision_is_a_deterministic_generation_error() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            "pub struct RecordView;\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        ),
    )
    .expect("insert an authored view-name collision");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "the generated view collision must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "generated concrete view `RecordView` collides with an authored item beside Node `Record`"
        ),
        "stderr:\n{stderr}",
    );
}

#[test]
fn an_imported_view_name_collision_is_a_deterministic_generation_error() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            "mod authored { pub struct RecordView; }\nuse authored::RecordView;\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        ),
    )
    .expect("insert an imported view-name collision");

    assert_view_collision(run_fixture(&package), "RecordView");
}

#[test]
fn a_generated_token_binding_collision_is_a_deterministic_generation_error() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "pub text: CountText<'input>",
            "pub text: RecordView<'input>",
        ),
    )
    .expect("insert a generated token-binding collision");

    assert_view_collision(run_fixture(&package), "RecordView");
}

#[test]
fn an_explorer_value_name_collision_is_a_deterministic_generation_error() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            "static __RECURSA_EXPLORER_SCHEMA_0: () = ();\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        ),
    )
    .expect("insert explorer static collision");

    let output = run_fixture(&package);
    assert!(!output.status.success(), "explorer collision must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "generated explorer item `__RECURSA_EXPLORER_SCHEMA_0` collides with an authored value"
        ),
        "stderr:\n{stderr}"
    );
}

#[test]
fn an_authored_legacy_explorer_parse_name_is_legal() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    let mut source = fs::read_to_string(&grammar).expect("read compound grammar");
    source.push_str("\nfn __recursa_explorer_parse_0() {}\n");
    fs::write(&grammar, source).expect("insert the retired explorer parse name");

    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "retired explorer parse symbols are not reserved\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn an_explorer_aggregate_schema_collision_is_diagnosed_before_emission() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            "static __RECURSA_EXPLORER_NODE_SCHEMAS: () = ();\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        ),
    )
    .expect("insert aggregate explorer schema collision");

    assert_explorer_collision(run_fixture(&package), "__RECURSA_EXPLORER_NODE_SCHEMAS");
}

#[test]
fn an_authored_explorer_entry_method_collision_is_diagnosed_before_emission() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    let mut source = fs::read_to_string(&grammar).expect("read compound grammar");
    source.push_str("\nimpl<'input> Record<'input> { pub fn explorer() {} }\n");
    fs::write(&grammar, source).expect("insert explorer method collision");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "explorer method collision must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("generated explorer entry method `Record::explorer` collides"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn an_imported_value_binding_collision_is_diagnosed_before_emission() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    let mut source = fs::read_to_string(&grammar).expect("read compound grammar");
    source.push_str(
        "\nmod explorer_import_collision { pub fn value() {} }\nuse explorer_import_collision::value as __RECURSA_EXPLORER_SCHEMA_0;\n",
    );
    fs::write(&grammar, source).expect("insert imported explorer value collision");

    assert_explorer_collision(run_fixture(&package), "__RECURSA_EXPLORER_SCHEMA_0");
}

#[test]
fn a_unit_constructor_collision_is_diagnosed_before_emission() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            "struct __RECURSA_EXPLORER_SCHEMA_0;\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        ),
    )
    .expect("insert unit-constructor explorer collision");

    assert_explorer_collision(run_fixture(&package), "__RECURSA_EXPLORER_SCHEMA_0");
}

#[test]
fn an_explorer_method_in_a_descendant_impl_is_diagnosed_before_emission() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    let source = source.replace(
        "    pub struct Duplicate;\n}\n\npub mod public_right",
        "    pub struct Duplicate;\n\n    impl<'input> super::Record<'input> { pub fn explorer() {} }\n}\n\npub mod public_right",
    );
    fs::write(&grammar, source).expect("insert descendant explorer method collision");

    let output = run_fixture(&package);
    assert!(!output.status.success(), "explorer collision must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("generated explorer entry method `Record::explorer` collides"),
        "stderr:\n{stderr}"
    );
}

fn assert_explorer_collision(output: Output, name: &str) {
    assert!(!output.status.success(), "explorer collision must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains(&format!("generated explorer item `{name}` collides")),
        "stderr:\n{stderr}"
    );
}

#[test]
fn source_edits_select_a_fresh_generation_in_the_same_build_directory() {
    let package = scaffold_fixture();
    let first = run_fixture(&package);
    assert!(
        first.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let before = generation_directories(&target_dir(&package));
    assert!(
        !before.is_empty(),
        "the first build must publish a Generation"
    );

    let grammar = package.path().join("src/grammar.rs");
    let mut source = fs::read_to_string(&grammar).expect("read compound grammar");
    source.push_str("\n// identity-changing source edit\n");
    fs::write(&grammar, source).expect("edit compound grammar");
    let second = run_fixture(&package);
    assert!(
        second.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let after = generation_directories(&target_dir(&package));
    assert!(
        after.len() > before.len(),
        "before={before:?}\nafter={after:?}"
    );
}

fn assert_view_collision(output: Output, name: &str) {
    assert!(
        !output.status.success(),
        "the generated view collision must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(stderr.contains(name), "stderr:\n{stderr}");
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary compound grammar package");
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
        .expect("read compound grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write compound grammar fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy compound grammar fixture file");
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
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run compound grammar fixture")
}

fn run_fixture_unlocked(package: &TempDir) -> Output {
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
        .expect("run compound grammar fixture with a mutated manifest")
}

fn run_fixture_doc(package: &TempDir, private_items: bool) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("doc")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .arg("--no-deps");
    if private_items {
        command.arg("--document-private-items");
    }
    command
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("RUSTDOCFLAGS", "-D rustdoc::broken_intra_doc_links")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("render compound grammar fixture documentation")
}

fn configure_diagrams(package: &TempDir, generator: bool, facade: bool) {
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read fixture grammar");
    fs::write(
        &grammar,
        source.replace(
            "    module = crate::grammar,",
            "    module = crate::grammar,\n    diagrams,",
        ),
    )
    .expect("request diagrams in the selected grammar");

    let manifest = package.path().join("Cargo.toml");
    let mut source = fs::read_to_string(&manifest).expect("read fixture manifest");
    if facade {
        source = source.replace(
            "features = [\"arbitrary\", \"serde\"]",
            "features = [\"arbitrary\", \"serde\", \"diagrams\"]",
        );
    }
    if generator {
        source = source.replace(
            "recursa-codegen = { path = \"",
            "recursa-codegen = { features = [\"diagrams\"], path = \"",
        );
    }
    fs::write(manifest, source).expect("configure diagram capabilities");
}

fn configure_explorer(package: &TempDir, generator: bool, facade: bool) {
    let manifest = package.path().join("Cargo.toml");
    let mut source = fs::read_to_string(&manifest).expect("read fixture manifest");
    if facade {
        source = source.replace(
            "features = [\"arbitrary\", \"serde\"]",
            "features = [\"arbitrary\", \"serde\", \"explorer\"]",
        );
    }
    if generator {
        source = source.replace(
            "recursa-codegen = { path = \"",
            "recursa-codegen = { features = [\"explorer\"], path = \"",
        );
    }
    fs::write(manifest, source).expect("configure explorer capabilities");
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

fn only_generation_identity(package: &TempDir) -> std::ffi::OsString {
    let generations = generation_directories(&target_dir(package));
    assert_eq!(generations.len(), 1, "generations={generations:?}");
    generations
        .iter()
        .next()
        .and_then(|path| path.file_name())
        .expect("published Generation identity")
        .to_owned()
}
