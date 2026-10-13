use super::*;

pub(super) fn validated_generation_is_not_revalidated_during_rendering() {
    let package = tempfile::tempdir().expect("temporary validated rendering fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[tok("root")]
    struct Root;
}
"#,
    )
    .expect("write validated rendering fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze validated rendering fixture");

    reset_emission_plan_validation_count();
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a validated Emission plan");
    assert_eq!(emission_plan_validation_count(), 1);

    render(&plan).expect("render the already validated Emission plan");
    assert_eq!(
        emission_plan_validation_count(),
        1,
        "rendering must consume the validation proof established while joining",
    );
}

pub(super) fn checked_rendering_retains_invalid_rust_diagnostics() {
    let (_package, mut plan) = joined_expressions_fixture();
    plan.module_anchor.segments[0].arguments =
        PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: Default::default(),
            gt_token: Default::default(),
        });

    let detail = match render(&plan) {
        Err(detail) => detail,
        Ok(_) => panic!("checked rendering must reject invalid generated Rust"),
    };
    assert!(
        detail.starts_with("root rendering is invalid Rust:"),
        "the syntax failure retains its stable rendering role: {detail}",
    );
}

pub(super) fn debug_directory_environment_is_always_tracked_by_cargo() {
    let mut cargo = Vec::new();
    emit_debug_directory_invalidation(&mut cargo);
    assert_eq!(
        cargo,
        [
            "cargo::rerun-if-env-changed=RECURSA_DEBUG_DIR",
            "cargo::rerun-if-env-changed=RECURSA_PROFILE",
        ]
    );
}

pub(super) fn debug_bundle_is_deterministic_formatted_complete_and_identity_neutral() {
    let (_package, plan) = joined_expressions_fixture();
    let rendered = render(&plan).unwrap();
    let identity = plan.identity.clone();
    let authoritative_root = rendered.root.clone();
    let authoritative_nodes = rendered.nodes.clone();
    let destination = tempfile::tempdir().unwrap();
    fs::write(destination.path().join("owned-by-caller"), "preserve me").unwrap();

    let staged = stage_debug_bundle(destination.path(), &plan, &rendered).unwrap();
    let published = staged.published.clone();
    assert!(!published.exists(), "a staged bundle is not yet observable");
    staged.commit().unwrap();

    assert_eq!(plan.identity, identity);
    assert_eq!(rendered.root, authoritative_root);
    assert_eq!(rendered.nodes, authoritative_nodes);
    assert_eq!(
        fs::read_to_string(destination.path().join("owned-by-caller")).unwrap(),
        "preserve me"
    );
    let index = fs::read_to_string(published.join("index")).unwrap();
    assert!(index.starts_with(DEBUG_BUNDLE_FORMAT));
    assert_eq!(
        index
            .lines()
            .filter(|line| line.starts_with("root\t"))
            .count(),
        1
    );
    assert_eq!(
        index
            .lines()
            .filter(|line| line.starts_with("node\t"))
            .count(),
        plan.nodes.len()
    );
    assert!(
        index
            .lines()
            .any(|line| line.starts_with("capability\tconfigured:")),
        "configured capability bodies are indexed"
    );
    assert!(
        index.lines().any(|line| {
            line.starts_with("capability\tintrinsic-token:")
                && line.contains(":Debug+Clone+PartialEq+Eq+PartialOrd+Ord+Hash\t")
        }),
        "intrinsic content-token value traits are indexed separately from configured recipes",
    );
    for line in index.lines().skip(2) {
        let [role, _, private_name] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("debug index row is malformed: {line}");
        };
        let source = fs::read_to_string(published.join(private_name)).unwrap();
        if role == "automaton" {
            assert_eq!(
                Some(source.as_str()),
                rendered.automaton_dump.as_deref(),
                "the indexed automaton is the authoritative LR dump",
            );
            continue;
        }
        let syntax = syn::parse_file(&source).unwrap_or_else(|error| {
            panic!("logical view {private_name} is not parseable Rust: {error}\n{source}")
        });
        assert_eq!(
            prettyplease::unparse(&syntax),
            source,
            "every logical view is stable under the formatter"
        );
    }

    fs::write(published.join("root.rs"), "human notes, not Rust").unwrap();
    fs::write(published.join("index"), "human index edit").unwrap();
    fs::write(
        published.join("extra-notes.txt"),
        "keep out of generated view",
    )
    .unwrap();
    let repeated = stage_debug_bundle(destination.path(), &plan, &rendered).unwrap();
    assert!(repeated.staging.is_some());
    assert!(repeated.replacement.is_some());
    repeated.commit().unwrap();
    assert_eq!(fs::read_to_string(published.join("index")).unwrap(), index);
    assert!(!published.join("extra-notes.txt").exists());
    assert_eq!(plan.identity, identity);
    let regenerated_root = fs::read_to_string(published.join("root.rs")).unwrap();
    syn::parse_file(&regenerated_root).expect("human edits are replaced, not loaded");
}

pub(super) fn arbitrary_identity_collision_is_preserved_and_rejected_without_replacement() {
    let (_package, plan) = joined_expressions_fixture();
    let rendered = render(&plan).unwrap();
    let destination = tempfile::tempdir().unwrap();
    let collision = destination
        .path()
        .join(format!(".{DEBUG_BUNDLE_FORMAT}"))
        .join(&plan.identity);
    fs::create_dir_all(&collision).unwrap();
    fs::write(collision.join("caller-data"), "do not remove").unwrap();

    let error = match stage_debug_bundle(destination.path(), &plan, &rendered) {
        Ok(_) => panic!("an unowned identity collision must not be replaced"),
        Err(error) => error,
    };
    assert!(error.contains("not owned by Recursa"), "{error}");
    assert_eq!(
        fs::read_to_string(collision.join("caller-data")).unwrap(),
        "do not remove"
    );
}

#[cfg(all(feature = "diagrams", feature = "explorer"))]
pub(super) fn debug_bundle_includes_every_enabled_diagram_and_explorer_capability_body() {
    let (_package, plan) = joined_expressions_diagram_fixture();
    let rendered = render(&plan).unwrap();
    let files = render_debug_logical_files(&plan, &rendered).unwrap();
    let index = render_debug_index(&plan.identity, &files);
    assert!(index.contains("capability\tdiagrams:aggregate\t"));
    for node in &plan.nodes {
        assert!(
            index.contains(&format!("capability\tdiagrams:{}\t", node.semantic_key)),
            "missing diagram body for {}\n{index}",
            node.semantic_key
        );
    }
    for diagram in &plan.diagrams.as_ref().expect("diagram plan").tokens {
        assert!(
            index.contains(&format!(
                "capability\tdiagrams:token:{}\t",
                diagram.semantic_key
            )),
            "missing content-token diagram body for {}\n{index}",
            diagram.semantic_key
        );
    }
    assert!(index.contains("capability\texplorer:static-schema\t"));
    assert!(index.contains("capability\texplorer:lexer\t"));
    for token in &plan.tokens {
        assert!(
            index.contains(&format!(
                "capability\texplorer:token:{}\t",
                token.kind_variant
            )),
            "missing explorer token body for {}\n{index}",
            token.kind_variant
        );
    }
    for node in &plan.nodes {
        for application in &node.parse_applications {
            assert!(
                index.contains(&format!(
                    "capability\texplorer:node:{}:application-{}\t",
                    node.semantic_key,
                    application.application.index()
                )),
                "missing explorer application body for {} application {}\n{index}",
                node.semantic_key,
                application.application.index()
            );
        }
    }
}

pub(super) fn failed_debug_commit_is_cleaned_and_makes_the_generation_unselectable() {
    let (_package, plan) = joined_expressions_fixture();
    let rendered = render(&plan).unwrap();
    let destination = tempfile::tempdir().unwrap();
    let staged = stage_debug_bundle(destination.path(), &plan, &rendered).unwrap();
    let staging = staged.staging.clone().unwrap();
    fs::create_dir(&staged.published).unwrap();
    fs::write(staged.published.join("caller-data"), "do not remove").unwrap();
    let output = tempfile::tempdir().unwrap();

    let detail =
        publish_generation_and_debug(output.path(), &plan.identity, &rendered, Some(staged))
            .unwrap_err();
    assert!(detail.contains("debug bundle"), "{detail}");
    assert_eq!(selected_generation(output.path()), None);
    assert!(!staging.exists());
    assert_eq!(
        fs::read_to_string(
            destination
                .path()
                .join(format!(".{DEBUG_BUNDLE_FORMAT}"))
                .join(&plan.identity)
                .join("caller-data")
        )
        .unwrap(),
        "do not remove"
    );
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one debug emission failure is required");
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
}

#[cfg(unix)]
pub(super) fn debug_destination_and_bundle_entries_reject_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;

    let (_package, plan) = joined_expressions_fixture();
    let rendered = render(&plan).unwrap();
    let parent = tempfile::tempdir().unwrap();
    let target = parent.path().join("target");
    fs::create_dir(&target).unwrap();
    let destination = parent.path().join("debug-link");
    symlink(&target, &destination).unwrap();
    let error = match stage_debug_bundle(&destination, &plan, &rendered) {
        Ok(_) => panic!("a symlink destination must be rejected"),
        Err(error) => error,
    };
    assert!(error.contains("symbolic link"), "{error}");
    assert!(fs::read_dir(target).unwrap().next().is_none());
}

pub(super) fn nested_visit_variant_uses_the_path_relative_to_the_grammar_module() {
    assert_eq!(
        visit_variant_name("grammar", "grammar::public_left::Duplicate"),
        "__RecursaTarget_11_public_left_9_Duplicate"
    );
}

pub(super) fn root_visit_variants_are_encoded_away_from_reserved_and_nested_names() {
    assert_eq!(
        visit_variant_name("grammar", "grammar::Opaque"),
        "__RecursaTarget_6_Opaque"
    );
    assert_ne!(
        visit_variant_name(
            "grammar",
            "grammar::__RecursaTarget_11_public_left_9_Duplicate"
        ),
        visit_variant_name("grammar", "grammar::public_left::Duplicate")
    );
}

pub(super) fn snake_case_produces_stable_generated_element_names() {
    assert_eq!(snake_case("RECORD"), "record");
    assert_eq!(snake_case("LabelColon"), "label_colon");
    assert_eq!(snake_case(":"), "element");
}

pub(super) fn synthetic_fixed_spelling_uppercases_only_ascii_insensitive_keywords() {
    assert_eq!(
        fixed_pretty_spelling("record", TextEquivalence::AsciiInsensitive),
        "RECORD"
    );
    assert_eq!(
        fixed_pretty_spelling(":", TextEquivalence::AsciiInsensitive),
        ":"
    );
    assert_eq!(
        fixed_pretty_spelling("record", TextEquivalence::Sensitive),
        "record"
    );
}

pub(super) fn fixed_accessor_names_remain_valid_for_numeric_spellings() {
    assert_eq!(
        super::fixed_accessor_ident("123").unwrap().to_string(),
        "element_123_span"
    );
}

#[cfg(feature = "diagrams")]
pub(super) fn semantic_diagrams_cover_expression_nodes_content_tokens_and_local_docs() {
    let (_package, plan) = joined_expressions_diagram_fixture();
    let diagrams = plan.diagrams.as_ref().expect("diagram plan");
    assert_eq!(diagrams.nodes.len(), plan.nodes.len());
    assert_eq!(diagrams.tokens.len(), plan.tokens.len());
    assert!(
        diagrams
            .tokens
            .iter()
            .all(|token| token.label == token.pattern)
    );
    let expression = diagrams
        .nodes
        .iter()
        .find(|node| node.ident == "Expr")
        .expect("the expression diagram");
    let topology = format!("{:?}", expression.production);
    assert!(topology.contains("Choice"));
    // Each variant renders as the rule its author wrote, under its own name.
    for variant in ["Negate", "Factorial", "Power", "Add", "Between"] {
        assert!(topology.contains(variant), "{variant}\n{topology}");
    }
    let aggregate = recursa_diagrams::render_aggregate_document(diagrams);
    assert!(aggregate.contains("## Content tokens"));
    for binding in &plan.token_bindings {
        let rendered = render_token_binding(&plan, binding).to_string();
        assert!(
            rendered.contains("cfg_attr (doc , recursa :: __recursa_diagram_doc (token ="),
            "token bindings defer diagram rendering to `cargo doc`:\n{rendered}"
        );
        assert!(!rendered.contains("<svg"));
    }
    let decoded = DiagramPlan::decode(&diagrams.encode()).expect("published plans round-trip");
    assert_eq!(&decoded, diagrams);
    let rendered = render(&plan).expect("render the diagram-enabled plan");
    assert_eq!(
        rendered.diagram_plan.as_deref(),
        Some(diagrams.encode().as_str())
    );
    assert!(
        rendered
            .root
            .contains("cfg_attr (doc , recursa :: __recursa_diagram_doc (aggregate))"),
        "the root defers the aggregate document to `cargo doc`:\n{}",
        rendered.root
    );
    assert!(!rendered.root.contains("<svg"));
    for (index, (name, source)) in rendered.nodes.iter().enumerate() {
        assert!(
            source.contains(&format!(
                "cfg_attr (doc , recursa :: __recursa_diagram_doc (node = {index}usize))"
            )),
            "{name} defers its Node diagram to `cargo doc`:\n{source}"
        );
        assert!(!source.contains("<svg"), "{name} embeds SVG");
    }
}

#[cfg(feature = "diagrams")]
#[cfg(feature = "diagrams")]
pub(super) fn semantic_diagrams_preserve_generic_applications_and_repetition_topology() {
    let (_package, plan) = joined_expressions_fixture_from(include_str!(
        "../../tests/fixtures/generation/generic-applications/src/grammar.rs"
    ));
    let diagrams = plan.diagrams.as_ref().expect("diagram plan");
    let wrap = diagrams
        .nodes
        .iter()
        .find(|node| node.ident == "Wrap")
        .expect("generic Wrap diagram");
    assert!(matches!(wrap.production, DiagramNode::Choice(_)));
    let separator = diagrams
        .nodes
        .iter()
        .find(|node| node.ident == "Separated")
        .expect("separated repetition diagram");
    let topology = format!("{:?}", separator.production);
    assert!(topology.contains("Repeat"));
    assert!(topology.contains("Terminal(\"and\")"));
    assert!(
        topology.contains("Optional"),
        "Vec is zero-or-more: {topology}"
    );
}

fn joined_expressions_fixture() -> (tempfile::TempDir, EmissionPlan) {
    joined_expressions_fixture_from(include_str!(
        "../../tests/fixtures/generation/expressions/src/grammar.rs"
    ))
}

#[cfg(feature = "diagrams")]
fn joined_expressions_diagram_fixture() -> (tempfile::TempDir, EmissionPlan) {
    let source = include_str!("../../tests/fixtures/generation/expressions/src/grammar.rs")
        .replacen(
            "module = crate::grammar,",
            "module = crate::grammar,\n    diagrams,",
            1,
        );
    joined_expressions_fixture_from(&source)
}

fn joined_expressions_fixture_from(source: &str) -> (tempfile::TempDir, EmissionPlan) {
    let package = tempfile::tempdir().expect("temporary expressions plan fixture");
    fs::write(package.path().join("grammar.rs"), source).expect("write expressions plan fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze expressions plan fixture");
    let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a complete expressions Emission plan");
    render(&plan).expect("a complete expressions plan renders");
    (package, plan)
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_plan_freezes_the_complete_typed_static_schema() {
    let (_, plan) = joined_expressions_fixture();
    let explorer = plan
        .explorer
        .as_ref()
        .expect("Visit fixture has an explorer plan");

    assert_eq!(explorer.grammar.name, plan.grammar_name);
    assert_eq!(explorer.tokens.len(), plan.kind_count - 1);
    assert!(explorer.tokens.iter().any(|token| {
        token.canonical_text.as_deref() == Some("+")
            && token.kind == ExplorerTokenKindPlan::Fixed
            && token.highlight == ExplorerHighlightClassPlan::Punctuation
    }));
    assert!(explorer.tokens.iter().any(|token| {
        token.canonical_text.is_none()
            && token.kind == ExplorerTokenKindPlan::Content
            && token.highlight == ExplorerHighlightClassPlan::Content
    }));

    let node = explorer
        .nodes
        .iter()
        .find(|node| matches!(node.body, ExplorerNodeBodyPlan::Enum(_)))
        .expect("the expression application has a Node descriptor");
    assert!(
        explorer
            .roots
            .iter()
            .any(|root| root.application == node.application)
    );
    assert!(node.qualified_name.ends_with(&format!("::{}", node.name)));
    let variants = match &node.body {
        ExplorerNodeBodyPlan::Enum(variants) => variants,
        ExplorerNodeBodyPlan::Struct(_) => {
            panic!("an enum root keeps its complete variant table")
        }
    };
    assert!(variants.iter().all(|variant| {
        variant
            .qualified_name
            .as_ref()
            .is_some_and(|qualified| qualified.starts_with(&node.qualified_name))
    }));
    assert!(
        variants
            .iter()
            .flat_map(|variant| &variant.elements)
            .any(|element| matches!(element.target, Some(ExplorerElementTargetPlan::Node(_))))
    );
    let all_variants = explorer.nodes.iter().flat_map(|node| match &node.body {
        ExplorerNodeBodyPlan::Struct(variant) => std::slice::from_ref(variant),
        ExplorerNodeBodyPlan::Enum(variants) => variants.as_slice(),
    });
    assert!(
        all_variants
            .clone()
            .flat_map(|variant| &variant.elements)
            .any(|element| matches!(
                element.target,
                Some(ExplorerElementTargetPlan::ContentToken(_))
            ))
    );
    assert!(
        all_variants
            .flat_map(|variant| &variant.elements)
            .any(|element| element.target.is_none() && element.token_kind.is_some())
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_rendering_consumes_its_frozen_schema_plan() {
    let (_, mut plan) = joined_expressions_fixture();
    let explorer = plan
        .explorer
        .as_mut()
        .expect("Visit fixture has an explorer plan");
    explorer.nodes[0].name = "FrozenExplorerSchema".to_owned();
    let element = explorer
        .nodes
        .iter_mut()
        .flat_map(|node| match &mut node.body {
            ExplorerNodeBodyPlan::Struct(variant) => std::slice::from_mut(variant),
            ExplorerNodeBodyPlan::Enum(variants) => variants.as_mut_slice(),
        })
        .flat_map(|variant| &mut variant.elements)
        .find(|element| matches!(element.display, ExplorerDisplayPlan::Fixed(_)))
        .expect("fixture contains fixed explorer syntax");
    element.name = "frozen_element".to_owned();
    element.cardinality = ExplorerCardinalityPlan::OptionalOneOrMore;
    explorer
        .tokens
        .iter_mut()
        .find(|token| token.canonical_text.is_some())
        .expect("fixture contains a fixed Token descriptor")
        .canonical_text = Some("FROZEN_DISPLAY".to_owned());
    plan.plan_families.explorer = Some(plan_family_identity(explorer));

    let rendered = render(&plan).expect("self-consistent frozen explorer plan renders");
    let generated = std::iter::once(rendered.root.as_str())
        .chain(rendered.nodes.iter().map(|(_, source)| source.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    for frozen in [
        "FrozenExplorerSchema",
        "frozen_element",
        "OptionalOneOrMore",
        "FROZEN_DISPLAY",
    ] {
        assert!(
            generated.contains(frozen),
            "rendered explorer content must consume frozen decision {frozen}"
        );
    }
    assert!(
        generated.contains("Option < recursa :: __private :: ExplorerSampleFn >"),
        "capable roots must name the runtime sample callback alias instead of emitting a lint-prone nested function type"
    );
    assert!(
        generated.contains("explorer_sample_root :: <"),
        "capable roots must instantiate the shared generic sample path"
    );
    assert!(
        !generated.contains("Option < fn (& [u8]) -> Result"),
        "generated adapter bindings must not restore the type-complexity shape"
    );

    let schema_source = render_explorer_static_schema(
        &plan,
        plan.explorer
            .as_ref()
            .expect("Visit fixture has an explorer plan"),
    )
    .to_string();
    assert!(
        !schema_source.contains("'static"),
        "static schema aggregate references must use their inferred static lifetime"
    );

    let expr = plan
        .nodes
        .iter()
        .find(|node| node.ident == "Expr")
        .expect("fixture has the expression Node");
    let application = expr
        .parse_applications
        .first()
        .expect("the expression has a materialized application");
    let node_source = render_node_application_explorer(
        &plan,
        plan.explorer
            .as_ref()
            .expect("Visit fixture has an explorer plan"),
        expr,
        application,
    )
    .to_string();
    let (node_dispatch, _) = node_source
        .split_once("fn __recursa_explorer_field")
        .expect("Explorer implementation has a field dispatch method");
    assert!(
        node_dispatch.contains("Expr :: Add (..) => Some"),
        "enum schema dispatch must use a binding-free variant pattern: {node_dispatch}"
    );
    assert!(
        !node_dispatch.contains("__recursa_explorer_field_"),
        "enum schema dispatch must not duplicate field bindings: {node_dispatch}"
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_output_omits_only_redundant_lifetimes() {
    let (_, plan) = joined_expressions_fixture();
    let explorer = plan
        .explorer
        .as_ref()
        .expect("Visit fixture has an explorer plan");
    let application_id = explorer
        .roots
        .first()
        .expect("Visit fixture has an explorer root")
        .application;
    let (node, application) = explorer_application_parse_plan(&plan.nodes, application_id);

    let mut lifetime_free = application.clone();
    lifetime_free.ty = syn::parse_quote!(LifetimeFree);
    let lifetime_free_source =
        render_node_application_explorer(&plan, explorer, node, &lifetime_free).to_string();
    assert!(
        !lifetime_free_source.contains("impl < '__recursa_input >"),
        "lifetime-free adapters must not emit an unconstrained input lifetime"
    );

    let mut borrowed = application.clone();
    borrowed.ty = syn::parse_quote!(Borrowed<'__recursa_input>);
    let borrowed_source =
        render_node_application_explorer(&plan, explorer, node, &borrowed).to_string();
    assert!(
        borrowed_source.contains("impl < '__recursa_input >"),
        "borrowed adapters must retain the input lifetime used by their application type"
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_generation_supports_semantic_targets_sharing_a_base_kind() {
    let shared_pattern_pair = r#"recursa::grammar! {
    module = crate,
    spans = mandatory,
    framing(island = Item, boundary = SEMI),
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\n\r]+",
    }
    punctuation {
        SEMI => ";",
    }
}

recursa::ast_node! {

    pub struct Item {
        #[lex(pattern = r"[a-z]+")]
        pub first: FirstWord,
        #[lex(pattern = r"[a-z]+")]
        pub second: SecondWord,
    }
}
"#;
    let keyword_admission_trio = r#"recursa::grammar! {
    module = crate,
    spans = mandatory,
    framing(island = Item, boundary = SEMI),
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\n\r]+",
    }
    categories { UNRESERVED, RESERVED }
    keywords {
        SELECT => "select" in RESERVED,
        WINDOW => "window" in UNRESERVED,
    }
    admissions {
        NonReservedWord = UNRESERVED,
        ColId = NonReservedWord + RESERVED,
    }
    punctuation {
        SEMI => ";",
    }
}

recursa::ast_node! {

    pub struct Item {
        #[lex(pattern = r"[a-z]+", admits(ColId))]
        pub name: ColIdWord,
        #[lex(pattern = r"[a-z]+", admits(NonReservedWord))]
        pub label: NonReservedWordText,
        #[lex(pattern = r"[a-z]+")]
        pub bare: BareWord,
    }
}
"#;
    for (label, source) in [
        ("two shared-pattern targets", shared_pattern_pair),
        ("three keyword-admission targets", keyword_admission_trio),
    ] {
        let package = tempfile::tempdir().expect("temporary shared-base-kind fixture");
        fs::write(package.path().join("grammar.rs"), source)
            .expect("write shared-base-kind fixture");
        let success = analyze_source_tree(
            SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .unwrap_or_else(|report| panic!("analyze the {label} fixture: {report:?}"));
        let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
            .unwrap_or_else(|detail| panic!("join the {label} plan: {detail}"));
        render(&plan).unwrap_or_else(|detail| panic!("render the {label} plan: {detail}"));
    }
}

#[cfg(feature = "explorer")]
fn compound_fixture_join_error(source: String) -> String {
    let package = tempfile::tempdir().expect("temporary compound plan fixture");
    fs::write(package.path().join("grammar.rs"), source).expect("write compound plan fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("collision fixture reaches emission planning");
    match EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default()) {
        Ok(_) => panic!("the authored name must collide with generated output"),
        Err(detail) => detail,
    }
}

#[cfg(feature = "explorer")]
fn compound_fixture_source() -> String {
    include_str!("../../tests/fixtures/generation/compound-nodes/src/grammar.rs").to_owned()
}

#[cfg(feature = "explorer")]
pub(super) fn view_collisions_are_rejected_at_the_emission_plan_seam() {
    let imported = format!(
        "{}\nmod authored {{ pub struct View; }}\nuse authored::View;\n",
        compound_fixture_source(),
    );
    let detail = compound_fixture_join_error(imported);
    assert!(
        detail.contains("generated view type `View` collides with an authored import"),
        "{detail}"
    );

    let source = compound_fixture_source();
    let token_binding = source.replace("pub text: CountText,", "pub text: View,");
    assert_ne!(token_binding, source, "the token binding must be renamed");
    let detail = compound_fixture_join_error(token_binding);
    assert!(
        detail
            .contains("generated view type `View` collides with a generated content-token binding"),
        "{detail}"
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_value_collisions_are_rejected_at_the_emission_plan_seam() {
    for (source, name) in [
        (
            format!(
                "{}\nstatic __RECURSA_EXPLORER_GRAMMAR: () = ();\n",
                compound_fixture_source()
            ),
            "__RECURSA_EXPLORER_GRAMMAR",
        ),
        (
            format!(
                "{}\nstatic __RECURSA_EXPLORER_NODE_SCHEMAS: () = ();\n",
                compound_fixture_source(),
            ),
            "__RECURSA_EXPLORER_NODE_SCHEMAS",
        ),
        (
            format!(
                "{}\nmod collision {{ pub fn value() {{}} }}\nuse collision::value as __RECURSA_EXPLORER_SCHEMA_0;\n",
                compound_fixture_source()
            ),
            "__RECURSA_EXPLORER_SCHEMA_0",
        ),
        (
            format!(
                "{}\nstruct __RECURSA_EXPLORER_SCHEMA_0;\n",
                compound_fixture_source(),
            ),
            "__RECURSA_EXPLORER_SCHEMA_0",
        ),
    ] {
        let detail = compound_fixture_join_error(source);
        assert!(
            detail.contains(&format!("generated explorer item `{name}` collides")),
            "{detail}"
        );
    }
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_method_collisions_are_rejected_at_the_emission_plan_seam() {
    let root = format!(
        "{}\nimpl<'input> Record<'input> {{ pub fn explorer() {{}} }}\n",
        compound_fixture_source()
    );
    let detail = compound_fixture_join_error(root);
    assert!(
        detail.contains("generated explorer entry method `Record::explorer` collides"),
        "{detail}"
    );

    let descendant = format!(
        "{}\nmod collision {{ impl<'input> super::Record<'input> {{ pub fn explorer() {{}} }} }}\n",
        compound_fixture_source(),
    );
    let detail = compound_fixture_join_error(descendant);
    assert!(
        detail.contains("generated explorer entry method `Record::explorer` collides"),
        "{detail}"
    );
}

pub(super) fn authored_pretty_authority_rejects_emitted_declaration_corruption() {
    let package = tempfile::tempdir().expect("temporary Pretty authority fixture");
    fs::write(
        package.path().join("grammar.rs"),
        include_str!("../../tests/fixtures/generation/expressions/src/grammar.rs"),
    )
    .expect("write Pretty authority fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze Pretty authority fixture");
    let lowered = &success.analyzed.lowered;
    let authority = authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
        .expect("derive Pretty authority from source");
    let mut plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a complete Pretty plan");
    let element = plan
        .nodes
        .iter_mut()
        .flat_map(|node| match &mut node.body {
            NodeBodyPlan::Struct { elements, .. } => elements.iter_mut().collect::<Vec<_>>(),
            NodeBodyPlan::Enum { variants, .. } => variants
                .iter_mut()
                .flat_map(|variant| variant.elements.iter_mut())
                .collect(),
        })
        .find(|element| matches!(element.kind, ElementParseKind::Semantic { .. }))
        .expect("Pretty fixture contains a semantic declaration element");
    let ElementParseKind::Semantic { projection, .. } = &mut element.kind else {
        unreachable!("selected a semantic element")
    };
    let SemanticParseProjection::Direct {
        opaque_provenance, ..
    } = projection.as_mut()
    else {
        unreachable!("selected fixture semantic field is direct")
    };
    *opaque_provenance = !*opaque_provenance;
    plan.pretty = emitted_pretty_plan(&plan.nodes, &plan.tokens)
        .expect("fixture-derived Pretty plan has a reachable grouping atom");

    let authority_after_corruption =
        authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
            .expect("source authority remains derivable");
    assert_eq!(authority, authority_after_corruption);
    assert_ne!(Some(plan.pretty.clone()), authority);
    assert!(
        plan.validate()
            .expect_err("corrupt emitted Pretty plan must diverge")
            .contains("Pretty plan family")
    );
}

pub(super) fn pretty_recursion_uses_typed_node_identity_not_the_final_path_segment() {
    let package = tempfile::tempdir().expect("temporary typed Pretty fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
recursa::grammar! {
    module = crate::grammar,
    derives(Pretty),
    keyword_matching = sensitive,
}
recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation { OPEN => "(", CLOSE => ")", PLUS => "+" }
    precedence { left(bp = 10) { PLUS } }
}
pub mod foreign {
    recursa::ast_node! {

        pub struct Expr {
            #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
            pub value: ForeignExprText,
        }
    }
}
recursa::ast_node! {

    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub value: NumberText,
    }
}
recursa::ast_node! {

    pub enum Expr<T> {
        Foreign(foreign::Expr),
        Generic(T),
        Group(#[tok(OPEN, this, CLOSE)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
recursa::ast_node! {

    pub struct Root {
        pub expression: Expr<Number>,
    }
}
"#,
    )
    .expect("write typed Pretty fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze typed Pretty fixture");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join typed Pretty fixture");
    let index = plan
        .nodes
        .iter()
        .position(|node| node.semantic_key == "grammar::Expr")
        .expect("fixture has the owning Expr");
    let behavior = plan.pretty.nodes[index]
        .as_ref()
        .expect("owning Expr has Pretty behavior");
    let declared = behavior
        .declared
        .as_ref()
        .expect("Expr has typed declared-precedence behavior");

    assert_eq!(
        declared.variants[0].operands,
        vec![PrettyDeclaredOperandBehavior::Ordinary],
        "foreign::Expr must not become recursive merely because its final ident matches",
    );
    assert_eq!(
        declared.variants[1].operands,
        vec![PrettyDeclaredOperandBehavior::Ordinary]
    );
    assert!(matches!(
        declared.variants[2].operands.as_slice(),
        [PrettyDeclaredOperandBehavior::Recursive { owner, applications, .. }]
            if *owner == behavior.node && applications == &behavior.applications
    ));
    assert!(matches!(
        declared.variants[3].operands.as_slice(),
        [
            PrettyDeclaredOperandBehavior::Recursive { .. },
            PrettyDeclaredOperandBehavior::Recursive { .. }
        ]
    ));
    assert!(
        declared.grouping.is_some(),
        "typed Self atom supplies grouping"
    );
}

pub(super) fn pretty_variants_are_typechecked_in_isolated_helpers() {
    let (_package, plan) = joined_expressions_fixture();
    let rendered = render(&plan).expect("the expressions fixture renders");
    let expr_index = plan
        .nodes
        .iter()
        .position(|node| node.ident == "Expr")
        .expect("the fixture contains its Expr");
    let variant_count = plan.pretty.nodes[expr_index]
        .as_ref()
        .and_then(|behavior| behavior.declared.as_ref())
        .expect("Expr has typed declared-precedence Pretty behavior")
        .variants
        .len();
    let expr = &rendered.nodes[expr_index].1;

    assert_eq!(
        expr.matches("fn __recursa_pretty_declared_variant_")
            .count(),
        variant_count,
        "each Pretty variant must have a separately typechecked function item",
    );
    assert_eq!(
        expr.matches("self . __recursa_pretty_declared_variant_")
            .count(),
        variant_count,
        "the compact Pretty dispatcher must call every isolated variant helper",
    );
}

pub(super) fn generated_pretty_scopes_do_not_emit_callback_boundaries() {
    let package = tempfile::tempdir().expect("temporary generated Pretty scope fixture");
    let source = include_str!("../../tests/fixtures/source-analysis/json/grammar.rs").replacen(
        "recursa::grammar! {",
        "recursa::grammar! {\n    derives(Pretty),",
        1,
    );
    fs::write(package.path().join("grammar.rs"), source)
        .expect("write generated Pretty scope fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze generated Pretty scope fixture");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join generated Pretty scope fixture");
    let rendered = render(&plan).expect("render generated Pretty scope fixture");
    let nodes = rendered
        .nodes
        .iter()
        .map(|(_, node)| node.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for callback in [
        "context . with_child (",
        "context . with_opaque_child (",
        "context . without_provenance (",
        "context . with_repetition_style (",
    ] {
        assert!(
            !nodes.contains(callback),
            "generated Pretty must not emit callback scope `{callback}`",
        );
    }
    assert!(nodes.contains("context . child_scope ("));
    assert!(nodes.contains("context . repetition_scope_style ("));
}

pub(super) fn pretty_grouping_follows_a_unique_authored_helper_path() {
    let (_package, plan) = joined_expressions_fixture_from(
        r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { ANY => "any", QUERY => "query" }
    punctuation { OPEN => "(", CLOSE => ")", COMMA => ",", DOT => ".", STAR => "*", PLUS => "+" }
    precedence { left(bp = 10) { PLUS } }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    enum ParenthesizedSuffix {
        #[tok(DOT, STAR)]
        Star,
    }
}

recursa::ast_node! {

    enum ParenthesizedOpen {
        #[tok(OPEN)]
        Value,
    }
}

recursa::ast_node! {

    enum ParenthesizedClose {
        #[tok(CLOSE)]
        Value,
    }
}

recursa::ast_node! {

    #[tok(QUERY)]
    struct Query {
        value: boxed!(Expr),
    }
}

recursa::ast_node! {

    enum ParenthesizedContent {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Query(Query),
    }
}

recursa::ast_node! {

    struct ParenthesizedExpr {
        open: ParenthesizedOpen,
        content: ParenthesizedContent,
        close: ParenthesizedClose,
        suffix: Option<ParenthesizedSuffix>,
    }
}

recursa::ast_node! {

    struct QuantifiedExpr {
        #[tok(ANY, OPEN, this, CLOSE)]
        content: ParenthesizedContent,
    }
}

recursa::ast_node! {

    enum Expr {
        Integer(Integer),
        Quantified(QuantifiedExpr),
        Parenthesized(ParenthesizedExpr),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
"#,
    );
    let index = plan
        .nodes
        .iter()
        .position(|node| node.semantic_key == "Expr")
        .expect("fixture has the owning Expr");
    let grouping = plan.pretty.nodes[index]
        .as_ref()
        .and_then(|behavior| behavior.declared.as_ref())
        .and_then(|declared| declared.grouping.as_ref())
        .expect("the helper path supplies grouping syntax");

    assert_eq!(
        grouping
            .before
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        ["("]
    );
    assert_eq!(
        grouping
            .after
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        [")"]
    );
}

/// Proves the emitted grouping survives the same witnessless cycle the source
/// model admits, so a grammar that clears RCA3101 still renders its delimiters.
pub(super) fn pretty_grouping_survives_a_witnessless_cyclic_sibling_variant() {
    let (_package, plan) = joined_expressions_fixture_from(
        r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { SELECT => "select" }
    punctuation {
        OPEN => "(", CLOSE => ")", LBRACKET => "[", RBRACKET => "]",
        COMMA => ",", PLUS => "+"
    }
    precedence { left(bp = 10) { PLUS } }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    #[tok(SELECT)]
    struct Simple {
        #[sep(COMMA)]
        targets: one_or_many!(Expr),
    }
}

recursa::ast_node! {

    struct Paren {
        #[tok(LBRACKET, this, RBRACKET)]
        query: boxed!(Q),
    }
}

recursa::ast_node! {

    enum Q {
        Simple(Simple),
        Paren(Paren),
    }
}

recursa::ast_node! {

    enum C {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Query(boxed!(Q)),
    }
}

recursa::ast_node! {

    struct P {
        #[tok(OPEN, this, CLOSE)]
        content: C,
    }
}

recursa::ast_node! {

    enum Expr {
        Integer(Integer),
        Parenthesized(P),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
"#,
    );
    let index = plan
        .nodes
        .iter()
        .position(|node| node.semantic_key == "Expr")
        .expect("fixture has the owning Expr");
    let grouping = plan.pretty.nodes[index]
        .as_ref()
        .and_then(|behavior| behavior.declared.as_ref())
        .and_then(|declared| declared.grouping.as_ref())
        .expect("the witnessless cycle still supplies grouping syntax");

    assert_eq!(
        grouping
            .before
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        ["("]
    );
    assert_eq!(
        grouping
            .after
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        [")"]
    );
}

/// Proves a live cycle mark stays behind a half-open fixed context.
///
/// `Q` owns a witness of its own, so its self-cycle is not witnessless and its
/// mark survives. Reaching it through `S` puts it behind required syntax with
/// nothing after, a context that can never contribute a witness, so the mark
/// must not reach `C` and cost the sibling `Exprs` witness its grouping.
pub(super) fn pretty_grouping_ignores_a_witnessed_cycle_behind_a_half_open_context() {
    let (_package, plan) = joined_expressions_fixture_from(
        r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { KW => "kw" }
    punctuation {
        OPEN => "(", CLOSE => ")", LBRACKET => "[", RBRACKET => "]",
        COMMA => ",", PLUS => "+"
    }
    precedence { left(bp = 10) { PLUS } }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    struct QParen {
        #[tok(LBRACKET, this, RBRACKET)]
        query: boxed!(Q),
    }
}

recursa::ast_node! {

    enum Q {
        Direct(#[sep(COMMA)] one_or_many!(Expr)),
        Paren(QParen),
    }
}

recursa::ast_node! {

    #[tok(KW, this)]
    struct S {
        query: boxed!(Q),
    }
}

recursa::ast_node! {

    enum C {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Sub(boxed!(S)),
    }
}

recursa::ast_node! {

    struct P {
        #[tok(OPEN, this, CLOSE)]
        content: C,
    }
}

recursa::ast_node! {

    enum Expr {
        Integer(Integer),
        Parenthesized(P),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
"#,
    );
    let index = plan
        .nodes
        .iter()
        .position(|node| node.semantic_key == "Expr")
        .expect("fixture has the owning Expr");
    let grouping = plan.pretty.nodes[index]
        .as_ref()
        .and_then(|behavior| behavior.declared.as_ref())
        .and_then(|declared| declared.grouping.as_ref())
        .expect("a half-open context withholds the cycle mark and keeps the grouping");

    assert_eq!(
        grouping
            .before
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        ["("]
    );
    assert_eq!(
        grouping
            .after
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        [")"]
    );
}

pub(super) fn every_typed_pretty_declared_decision_is_part_of_family_authority() {
    let (package, plan) = joined_expressions_fixture();

    fn behavior(plan: &mut EmissionPlan) -> &mut PrettyNodeBehavior {
        let index = plan
            .nodes
            .iter()
            .position(|node| node.ident == "Expr")
            .expect("fixture has Expr");
        plan.pretty.nodes[index].as_mut().expect("Expr has Pretty")
    }
    fn declared(plan: &mut EmissionPlan) -> &mut PrettyDeclaredBehavior {
        behavior(plan)
            .declared
            .as_mut()
            .expect("Expr has declared precedence")
    }
    fn reject(package: &Path, mut plan: EmissionPlan, corrupt: impl FnOnce(&mut EmissionPlan)) {
        corrupt(&mut plan);
        assert_plan_family_is_unselectable(package, &plan, "Pretty");
    }

    reject(package.path(), plan.clone(), |plan| {
        behavior(plan).node = NodeId::next(999);
    });
    reject(package.path(), plan.clone(), |plan| {
        behavior(plan).applications.clear();
    });
    reject(package.path(), plan.clone(), |plan| {
        let grouping = declared(plan).grouping.as_mut().expect("grouping");
        grouping.before[0].spelling.push('!');
    });
    reject(package.path(), plan.clone(), |plan| {
        let grouping = declared(plan).grouping.as_mut().expect("grouping");
        grouping.after.clear();
    });
    reject(package.path(), plan.clone(), |plan| {
        declared(plan).grouping.as_mut().expect("grouping").after[0].rendering =
            PrettyFixedRendering::Punctuation;
    });
    reject(package.path(), plan.clone(), |plan| {
        declared(plan).variants[0].level = Some(1);
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = declared(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| matches!(operand, PrettyDeclaredOperandBehavior::Recursive { .. }))
            .expect("recursive operand");
        let PrettyDeclaredOperandBehavior::Recursive { owner, .. } = recursive else {
            unreachable!()
        };
        *owner = NodeId::next(999);
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = declared(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| matches!(operand, PrettyDeclaredOperandBehavior::Recursive { .. }))
            .expect("recursive operand");
        let PrettyDeclaredOperandBehavior::Recursive { applications, .. } = recursive else {
            unreachable!()
        };
        applications.clear();
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = declared(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| {
                matches!(
                    operand,
                    PrettyDeclaredOperandBehavior::Recursive {
                        minimum: PrettyDeclaredMinimum::Binding(_),
                        ..
                    }
                )
            })
            .expect("a recursive operand with a left-edge bound");
        let PrettyDeclaredOperandBehavior::Recursive { minimum, .. } = recursive else {
            unreachable!()
        };
        *minimum = PrettyDeclaredMinimum::None;
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = declared(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| {
                matches!(
                    operand,
                    PrettyDeclaredOperandBehavior::Recursive {
                        next: PrettyDeclaredNext::Binding(_),
                        ..
                    }
                )
            })
            .expect("a recursive operand with a right-edge bound");
        let PrettyDeclaredOperandBehavior::Recursive { next, .. } = recursive else {
            unreachable!()
        };
        *next = PrettyDeclaredNext::None;
    });
    reject(package.path(), plan.clone(), |plan| {
        let attached = behavior(plan)
            .variants
            .iter_mut()
            .flatten()
            .find_map(|element| match element {
                PrettyElementBehavior::Semantic {
                    projection:
                        PrettySemanticProjection::Direct {
                            attachment: Some(attachment),
                            ..
                        },
                } => attachment.before.first_mut(),
                _ => None,
            })
            .expect("an operator token attached before a semantic operand");
        let PrettyAttachedSyntaxBehavior::Required(fixed) = attached else {
            unreachable!("the expressions fixture attaches required operator syntax")
        };
        fixed.spelling.push('!');
    });
    reject(package.path(), plan, |plan| {
        let semantic = behavior(plan)
            .variants
            .iter_mut()
            .flatten()
            .find(|element| matches!(element, PrettyElementBehavior::Semantic { .. }))
            .expect("semantic Pretty element");
        let PrettyElementBehavior::Semantic {
            projection: PrettySemanticProjection::Direct { provenance, .. },
        } = semantic
        else {
            unreachable!()
        };
        *provenance = PrettyProvenanceBehavior::Opaque;
    });
}

pub(super) fn recursive_concrete_pretty_impls_do_not_publish_cyclic_child_bounds() {
    let package = tempfile::tempdir().expect("temporary recursive Pretty fixture");
    let source = include_str!("../../tests/fixtures/source-analysis/json/grammar.rs").replacen(
        "recursa::grammar! {",
        "recursa::grammar! {\n    derives(Pretty),",
        1,
    );
    fs::write(package.path().join("grammar.rs"), source).expect("write recursive Pretty fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze recursive Pretty fixture");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join recursive Pretty fixture");
    let rendered = render(&plan).expect("render recursive Pretty fixture");
    let nodes = rendered
        .nodes
        .iter()
        .map(|(_, node)| node.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(nodes.contains("recursa :: Pretty for Value"));
    for concrete in ["Value", "Object", "Member", "Array"] {
        assert!(
            !nodes.contains(&format!("recursa :: Pretty for {concrete} where")),
            "concrete {concrete} Pretty impl must not carry child bounds",
        );
    }
}

pub(super) fn recursive_concrete_visit_impls_do_not_publish_cyclic_child_bounds() {
    let package = tempfile::tempdir().expect("temporary recursive Visit fixture");
    let source = include_str!("../../tests/fixtures/source-analysis/json/grammar.rs").replacen(
        "recursa::grammar! {",
        "recursa::grammar! {\n    derives(Visit),",
        1,
    );
    fs::write(package.path().join("grammar.rs"), source).expect("write recursive Visit fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze recursive Visit fixture");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join recursive Visit fixture");
    let nodes = render(&plan)
        .expect("render recursive Visit fixture")
        .nodes
        .into_iter()
        .map(|(_, node)| node)
        .collect::<Vec<_>>()
        .join("\n");

    for cyclic_child in ["Value", "Object", "Member", "Array"] {
        assert!(
            !nodes.contains(&format!(
                "crate :: grammar :: {cyclic_child} < 'input > : recursa :: Visit"
            )),
            "recursive child {cyclic_child} must not become a Visit where-bound",
        );
    }
}

pub(super) fn visit_bodies_use_the_runtime_step_and_child_list_macros() {
    let source =
        include_str!("../../tests/fixtures/generation/generic-applications/src/grammar.rs")
            .replacen("    diagrams,\n", "", 1)
            .replacen(
                "PartialEq, Pretty, serde::Serialize",
                "PartialEq, Pretty, Visit, VisitMut, serde::Serialize",
                1,
            );
    let (_package, plan) = joined_expressions_fixture_from(&source);
    let render_visit = |ident: &str| {
        let node = plan
            .nodes
            .iter()
            .find(|node| node.ident == ident)
            .unwrap_or_else(|| panic!("generic fixture contains {ident}"));
        format!(
            "{}\n{}",
            render_node_visit(&plan, node, false),
            render_node_visit(&plan, node, true),
        )
    };
    let single = render_visit("KeywordName");
    let pair = render_visit("Pair");

    assert!(single.contains("__private :: visit_node ! (visitor ,"));
    assert!(single.contains("__private :: visit_node_mut ! (visitor ,"));
    assert!(
        !single.contains("total_enter"),
        "the enter/exit plumbing lives in the runtime macro"
    );
    assert!(
        single.contains("recursa :: Visit :: visit (& self . value , visitor) ? ;"),
        "a single child stays a direct call: {single}"
    );
    assert!(
        pair.contains("visit_children ! (ref self visitor ; left , right) ;"),
        "two children render as one compact list: {pair}"
    );
    assert!(pair.contains("visit_children ! (mut self visitor ; left , right) ;"));
}

pub(super) fn visit_bounds_only_cover_node_type_parameters() {
    let source = include_str!(
        "../../tests/fixtures/generation/generic-applications/src/grammar.rs"
    )
    .replacen("    diagrams,\n", "", 1)
    .replacen(
        "PartialEq, Pretty, serde::Serialize",
        "PartialEq, Pretty, Visit, VisitMut, serde::Serialize",
        1,
    )
    .replacen(
        "    #[tok(LPAREN, this, RPAREN)]\n    pub struct Pair",
        "    #[omit(Visit, VisitMut)]\n    #[tok(LPAREN, this, RPAREN)]\n    pub struct Pair",
        1,
    );
    let (_package, plan) = joined_expressions_fixture_from(&source);
    let render_visit = |ident: &str| {
        let node = plan
            .nodes
            .iter()
            .find(|node| node.ident == ident)
            .unwrap_or_else(|| panic!("generic fixture contains {ident}"));
        format!(
            "{}\n{}",
            render_node_visit(&plan, node, false),
            render_node_visit(&plan, node, true),
        )
    };
    let generic = render_visit("LifetimeNode");
    let concrete = render_visit("GenericKeywordRoot");
    let pair = render_visit("Pair");
    let audit = render_visit("AuditRoot");

    assert!(concrete.contains("recursa :: Visit for GenericKeywordRoot"));
    assert!(generic.contains("T : recursa :: Visit"));
    assert!(generic.contains("T : recursa :: VisitMut"));
    assert!(!pair.contains("Name < 'input > : recursa :: Visit"));
    assert!(!pair.contains("Name < 'input > : recursa :: VisitMut"));
    assert!(audit.contains("Pair < 'input > : recursa :: Visit"));
    assert!(audit.contains("Pair < 'input > : recursa :: VisitMut"));
    assert!(
        !concrete
            .contains("Wrap < crate :: grammar :: KeywordName < 'input > > : recursa :: Visit"),
        "concrete lifetime-bearing child types resolve through their generated Visit impl",
    );
    assert!(
        !concrete
            .contains("Wrap < crate :: grammar :: KeywordName < 'input > > : recursa :: VisitMut"),
        "concrete lifetime-bearing child types resolve through their generated VisitMut impl",
    );
}

pub(super) fn every_local_pretty_hint_dimension_is_part_of_family_authority() {
    let source = include_str!("../../tests/fixtures/generation/repetitions/src/grammar.rs.in")
        .replace("__RECURSA_SPANS_MODE__", "off");
    let package = tempfile::tempdir().expect("temporary Pretty hint authority fixture");
    fs::write(package.path().join("grammar.rs"), source)
        .expect("write Pretty hint authority fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze Pretty hint authority fixture");
    let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a complete Pretty hint plan");

    fn pretty_node<'a>(plan: &'a mut EmissionPlan, ident: &str) -> &'a mut PrettyNodeBehavior {
        let index = plan
            .nodes
            .iter()
            .position(|node| node.ident == ident)
            .unwrap_or_else(|| panic!("fixture contains {ident}"));
        plan.pretty.nodes[index]
            .as_mut()
            .unwrap_or_else(|| panic!("{ident} has Pretty behavior"))
    }
    fn semantic(node: &mut PrettyNodeBehavior) -> &mut PrettyElementBehavior {
        node.variants
            .iter_mut()
            .flatten()
            .find_map(|element| match element {
                PrettyElementBehavior::Semantic {
                    projection: PrettySemanticProjection::Direct { .. },
                } => Some(element),
                PrettyElementBehavior::Fixed { .. }
                | PrettyElementBehavior::OptionalFixedTokenGroup { .. }
                | PrettyElementBehavior::Semantic { .. } => None,
            })
            .expect("selected fixture node has a semantic field")
    }
    fn repetition_behavior(node: &mut PrettyNodeBehavior) -> &mut PrettyRepetitionBehavior {
        let PrettyElementBehavior::Semantic {
            projection:
                PrettySemanticProjection::Direct {
                    repetition: Some(repetition),
                    ..
                },
        } = semantic(node)
        else {
            panic!("selected fixture node has a repeated semantic field")
        };
        repetition
    }

    let mut node_hint = plan.clone();
    assert!(!pretty_node(&mut node_hint, "NodeGroupPretty").pretty.indent);
    pretty_node(&mut node_hint, "NodeGroupPretty").pretty.indent = true;
    assert_plan_family_is_unselectable(package.path(), &node_hint, "Pretty");

    let mut variant_hint = plan.clone();
    pretty_node(&mut variant_hint, "VariantGroupPretty").variant_pretty[0].indent = true;
    assert_plan_family_is_unselectable(package.path(), &variant_hint, "Pretty");

    let mut field_hint = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection: PrettySemanticProjection::Direct { pretty, .. },
    } = semantic(pretty_node(&mut field_hint, "AttachmentPretty"))
    else {
        unreachable!("semantic helper returns semantic behavior")
    };
    pretty.trailing_separator = Some(PrettyTrailingSeparator::Never);
    assert_plan_family_is_unselectable(package.path(), &field_hint, "Pretty");

    let mut separator_policy = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection: PrettySemanticProjection::Direct { pretty, .. },
    } = semantic(pretty_node(&mut separator_policy, "LeadingPretty"))
    else {
        unreachable!("semantic helper returns semantic behavior")
    };
    pretty.separator = Some(PrettySeparatorPosition::Trailing);
    assert_plan_family_is_unselectable(package.path(), &separator_policy, "Pretty");

    let mut inconsistent_group = plan.clone();
    pretty_node(&mut inconsistent_group, "InconsistentIndentPretty")
        .pretty
        .group = Some(PrettyGroup::Consistent);
    assert_plan_family_is_unselectable(package.path(), &inconsistent_group, "Pretty");

    let mut sequence_layout = plan.clone();
    pretty_node(&mut sequence_layout, "NodeGroupPretty").variant_layouts[0] =
        PrettySequenceLayout::Delimited;
    assert_plan_family_is_unselectable(package.path(), &sequence_layout, "Pretty");

    let mut fixed_rendering = plan.clone();
    let rendering = pretty_node(&mut fixed_rendering, "NodeGroupPretty")
        .variants
        .iter_mut()
        .flatten()
        .find_map(|element| match element {
            PrettyElementBehavior::Fixed { rendering, .. } => Some(rendering),
            PrettyElementBehavior::OptionalFixedTokenGroup { .. }
            | PrettyElementBehavior::Semantic { .. } => None,
        })
        .expect("fixture has ordinary fixed syntax");
    assert_eq!(*rendering, PrettyFixedRendering::Word);
    *rendering = PrettyFixedRendering::Punctuation;
    assert_plan_family_is_unselectable(package.path(), &fixed_rendering, "Pretty");

    let mut attachment_layout = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection:
            PrettySemanticProjection::Direct {
                attachment: Some(attachment),
                ..
            },
    } = semantic(pretty_node(&mut attachment_layout, "AttachedOptional"))
    else {
        panic!("fixture has an attached field")
    };
    assert_eq!(attachment.layout, PrettySequenceLayout::Delimited);
    attachment.layout = PrettySequenceLayout::Linear;
    assert_plan_family_is_unselectable(package.path(), &attachment_layout, "Pretty");

    let mut attached_fixed_rendering = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection:
            PrettySemanticProjection::Direct {
                attachment: Some(attachment),
                ..
            },
    } = semantic(pretty_node(
        &mut attached_fixed_rendering,
        "AttachedOptional",
    ))
    else {
        panic!("fixture has an attached field")
    };
    let PrettyAttachedSyntaxBehavior::Required(fixed) = &mut attachment.before[0] else {
        panic!("fixture delimiter is required")
    };
    let rendering = &mut fixed.rendering;
    assert_eq!(*rendering, PrettyFixedRendering::Opening);
    *rendering = PrettyFixedRendering::Punctuation;
    assert_plan_family_is_unselectable(package.path(), &attached_fixed_rendering, "Pretty");

    let mut separator_spelling = plan.clone();
    repetition_behavior(pretty_node(&mut separator_spelling, "LeadingPretty"))
        .separator
        .as_mut()
        .expect("fixture has a separator")
        .push_str("-corrupt");
    assert_plan_family_is_unselectable(package.path(), &separator_spelling, "Pretty");

    let mut trailing_separator = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut trailing_separator, "LeadingPretty"));
    assert!(repetition.trailing_separator);
    repetition.trailing_separator = false;
    assert_plan_family_is_unselectable(package.path(), &trailing_separator, "Pretty");

    let mut leading_separator = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut leading_separator, "LeadingPretty"));
    assert!(repetition.leading_separator);
    repetition.leading_separator = false;
    assert_plan_family_is_unselectable(package.path(), &leading_separator, "Pretty");

    let mut word_separator_spacing = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut word_separator_spacing, "WordSeparated"));
    assert!(repetition.space_before_separator);
    repetition.space_before_separator = false;
    assert_plan_family_is_unselectable(package.path(), &word_separator_spacing, "Pretty");

    let mut separator_trailing_spacing = plan.clone();
    let repetition = repetition_behavior(pretty_node(
        &mut separator_trailing_spacing,
        "LeadingPretty",
    ));
    assert!(repetition.space_after_separator);
    repetition.space_after_separator = false;
    assert_plan_family_is_unselectable(package.path(), &separator_trailing_spacing, "Pretty");

    let mut top_level_break = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut top_level_break, "MixedProgram"));
    assert!(repetition.hard_break);
    repetition.hard_break = false;
    assert_plan_family_is_unselectable(package.path(), &top_level_break, "Pretty");

    let mut attachment_presence = plan;
    let presence = pretty_node(&mut attachment_presence, "HintAttachedOptional")
        .variants
        .iter_mut()
        .flatten()
        .find_map(|element| match element {
            PrettyElementBehavior::Semantic {
                projection:
                    PrettySemanticProjection::Direct {
                        attachment: Some(_),
                        presence,
                        ..
                    },
            } => Some(presence),
            PrettyElementBehavior::Fixed { .. }
            | PrettyElementBehavior::OptionalFixedTokenGroup { .. }
            | PrettyElementBehavior::Semantic { .. } => None,
        })
        .expect("fixture has a conditionally present attachment unit");
    assert_eq!(*presence, PrettyPresence::Optional);
    *presence = PrettyPresence::Always;
    assert_plan_family_is_unselectable(package.path(), &attachment_presence, "Pretty");
}

pub(super) fn ascii_insensitive_word_separators_share_canonical_pretty_family_authority() {
    let package = tempfile::tempdir().expect("temporary word-separator authority fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
recursa::grammar! {
    module = crate::grammar,
    derives(Pretty),
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords { AND => "and" }
    punctuation { LBRACKET => "[", RBRACKET => "]" }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub value: NameText,
    }
}

recursa::ast_node! {

    pub struct Separated<T> {
        #[tok(LBRACKET, this_each, RBRACKET)]
        #[sep(AND)]
        pub values: zero_or_many!(T),
    }
}

recursa::ast_node! {

    pub struct Root {
        pub value: Separated<Name>,
    }
}
"#,
    )
    .expect("write word-separator authority fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze word-separator authority fixture");
    let lowered = &success.analyzed.lowered;
    let authority = authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
        .expect("derive word-separator Pretty authority from source");
    let mut plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("canonical word separator joins one Pretty family");
    assert_eq!(Some(plan.pretty.clone()), authority);
    let application_separator =
        plan.nodes
            .iter()
            .flat_map(|node| &node.parse_applications)
            .flat_map(|application| match &application.body {
                NodeBodyPlan::Struct { elements, .. } => elements.iter().collect::<Vec<_>>(),
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter()
                    .flat_map(|variant| variant.elements.iter())
                    .collect(),
            })
            .find_map(|element| match &element.kind {
                ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
                    SemanticParseProjection::Direct {
                        separator: Some(separator),
                        attachment: Some(_),
                        ..
                    } => Some(separator),
                    SemanticParseProjection::Direct { .. }
                    | SemanticParseProjection::FixedPresence { .. } => None,
                },
                ElementParseKind::Fixed { .. }
                | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
            })
            .expect("fixture materializes an attached word-separated application");
    assert_eq!(application_separator, "AND");

    let separator =
        plan.nodes
            .iter_mut()
            .flat_map(|node| match &mut node.body {
                NodeBodyPlan::Struct { elements, .. } => elements.iter_mut().collect::<Vec<_>>(),
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter_mut()
                    .flat_map(|variant| variant.elements.iter_mut())
                    .collect(),
            })
            .find_map(|element| match &mut element.kind {
                ElementParseKind::Semantic { projection, .. } => match projection.as_mut() {
                    SemanticParseProjection::Direct {
                        separator: Some(separator),
                        attachment: Some(_),
                        ..
                    } => Some(separator),
                    SemanticParseProjection::Direct { .. }
                    | SemanticParseProjection::FixedPresence { .. } => None,
                },
                ElementParseKind::Fixed { .. }
                | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
            })
            .expect("fixture contains an attached word-separated repetition");
    assert_eq!(separator, "AND");
    *separator = "and".to_owned();
    plan.pretty = emitted_pretty_plan(&plan.nodes, &plan.tokens)
        .expect("fixture-derived Pretty plan has a reachable grouping atom");

    assert_ne!(Some(plan.pretty.clone()), authority);
    assert!(
        plan.validate()
            .expect_err("raw word separator must diverge from Pretty authority")
            .contains("Pretty plan family")
    );
}

pub(super) fn generic_top_level_breaks_keep_typed_family_and_closed_application_authority() {
    let package = tempfile::tempdir().expect("temporary generic top-level fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
recursa::grammar! {
    module = crate::grammar,
    derives(Pretty),
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation { SEMICOLON => ";" }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub value: NameText,
    }
}

recursa::ast_node! {

    pub struct Integer {
        #[lex(pattern = r"[0-9]+")]
        pub value: IntegerText,
    }
}

recursa::ast_node! {

    pub enum Statement<T> {
        Value(T),
    }
}

recursa::ast_node! {

    pub struct Program<T> {
        #[sep(SEMICOLON)]
        pub statements: zero_or_many!(Statement<T>),
    }
}

recursa::ast_node! {

    pub struct Root {
        pub names: Program<Name>,
        pub integers: Program<Integer>,
    }
}
"#,
    )
    .expect("write generic top-level fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze generic top-level fixture");
    let lowered = &success.analyzed.lowered;
    let authority = authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
        .expect("derive generic top-level Pretty authority");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join generic top-level applications");
    assert_eq!(Some(plan.pretty.clone()), authority);

    let program = plan
        .nodes
        .iter()
        .find(|node| node.ident == "Program")
        .expect("Program declaration plan");
    let NodeBodyPlan::Struct { elements, .. } = &program.body else {
        panic!("Program is a struct declaration")
    };
    let ElementParseKind::Semantic { projection, .. } = &elements[0].kind else {
        panic!("generic Program declaration keeps an application-independent enum-family break")
    };
    let SemanticParseProjection::Direct {
        repetition_break: PlannedRepetitionBreak::TopLevelStatementFamily(statement_family),
        terminal_application: None,
        ..
    } = projection.as_ref()
    else {
        panic!("generic Program declaration keeps an application-independent enum-family break")
    };

    let closed_breaks = program
        .parse_applications
        .iter()
        .map(|application| {
            let NodeBodyPlan::Struct { elements, .. } = &application.body else {
                panic!("Program application is a struct")
            };
            let ElementParseKind::Semantic { projection, .. } = &elements[0].kind else {
                panic!("closed Program application keeps an exact enum-application break")
            };
            let SemanticParseProjection::Direct {
                terminal_application: Some(terminal_application),
                repetition_break: PlannedRepetitionBreak::TopLevelStatement(break_application),
                ..
            } = projection.as_ref()
            else {
                panic!("closed Program application keeps an exact enum-application break")
            };
            assert_eq!(terminal_application, break_application);
            (*terminal_application, application.application)
        })
        .collect::<Vec<_>>();
    assert_eq!(closed_breaks.len(), 2);
    assert_ne!(closed_breaks[0].0, closed_breaks[1].0);
    assert!(plan.nodes.iter().any(|node| {
        node.parse_applications.iter().any(|application| {
            application.node == *statement_family
                && matches!(application.body, NodeBodyPlan::Enum { .. })
        })
    }));
    plan.validate()
        .expect("typed generic break family validates");
}

fn assert_plan_family_is_unselectable(package: &Path, plan: &EmissionPlan, family: &str) {
    let detail = match render(plan) {
        Err(detail) => detail,
        Ok(_) => panic!("a corrupted family cannot render"),
    };
    assert!(detail.contains(family), "detail={detail}");
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one pre-publication invariant diagnostic is required");
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
    assert!(!package.join(SELECTED_GENERATION).exists());
}

pub(super) fn incomplete_staging_is_unselectable() {
    let output = tempfile::tempdir().unwrap();
    let staging = output.path().join("staging");
    let published = output.path().join("published");
    fs::create_dir(&staging).unwrap();
    let rendered = RenderedGeneration {
        root: "fn root() {}".to_owned(),
        assets: vec![("asset.bin".to_owned(), vec![1, 2, 3])],
        nodes: vec![("node-0.rs".to_owned(), "fn node() {}".to_owned())],
        node_anchors: vec!["anchor-0".to_owned()],
        selector: "fn selected() {}".to_owned(),
        diagram_plan: None,
        automaton_dump: None,
    };
    assert_eq!(
        stage_and_publish(&staging, &published, &rendered, || {
            Err("injected staging interruption".to_owned())
        })
        .unwrap_err(),
        "injected staging interruption",
    );
    assert!(!published.exists());
    assert!(staging.join("root.rs").is_file());
    assert!(!staging.join("node-0.rs").exists());
}

pub(super) fn same_identity_cannot_reuse_a_different_generated_payload() {
    let output = tempfile::tempdir().unwrap();
    let original = publication_fixture("g_same");
    publish(output.path(), "g_same", &original).unwrap();
    invalidate_selection(output.path()).unwrap();

    let mut changed = publication_fixture("g_same");
    changed.root.push_str(" changed");
    let error = publish(output.path(), "g_same", &changed).unwrap_err();
    assert!(error.contains("Generation identity collision"));
    assert_eq!(selected_generation(output.path()), None);
    assert_eq!(
        fs::read_to_string(output.path().join("recursa-generations/g_same/root.rs")).unwrap(),
        original.root
    );

    publish(output.path(), "g_same", &original).unwrap();
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some("g_same")
    );

    invalidate_selection(output.path()).unwrap();
    let mut changed = publication_fixture("g_same");
    changed.assets[0].1.push(4);
    let error = publish(output.path(), "g_same", &changed).unwrap_err();
    assert!(error.contains("Generation identity collision"));
    assert_eq!(selected_generation(output.path()), None);
}

fn publication_fixture(identity: &str) -> RenderedGeneration {
    RenderedGeneration {
        root: format!("// {identity} root"),
        assets: vec![("asset.bin".to_owned(), identity.as_bytes().to_vec())],
        nodes: vec![("node-0.rs".to_owned(), format!("// {identity} node"))],
        node_anchors: vec!["anchor-0".to_owned()],
        selector: format!(
            "// recursa-generation:{identity}\ninclude!(concat!(env!(\"OUT_DIR\"), \"/recursa-generations/{identity}/root.rs\"));\n"
        ),
        automaton_dump: None,
        diagram_plan: Some(format!("// {identity} diagram plan")),
    }
}

pub(super) fn every_failed_publication_checkpoint_invalidates_stale_selection_and_recovers() {
    for checkpoint in [
        PublicationCheckpoint::BeforeStaging,
        PublicationCheckpoint::DuringStaging,
        PublicationCheckpoint::BeforeSelection,
        PublicationCheckpoint::DuringSelection,
    ] {
        let output = tempfile::tempdir().unwrap();
        let old = publication_fixture("g_old");
        publish_with_checkpoint(output.path(), "g_old", &old, |_| Ok(())).unwrap();
        assert_eq!(selected_generation(output.path()).as_deref(), Some("g_old"));

        invalidate_selection(output.path()).unwrap();
        let next = publication_fixture("g_next");
        let error = publish_with_checkpoint(output.path(), "g_next", &next, |point| {
            if point == checkpoint {
                Err(format!("injected {point:?} fault"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error, format!("injected {checkpoint:?} fault"));
        assert_eq!(selected_generation(output.path()), None);

        publish_with_checkpoint(output.path(), "g_next", &next, |_| Ok(())).unwrap();
        assert_eq!(
            selected_generation(output.path()).as_deref(),
            Some("g_next")
        );
        assert!(
            output
                .path()
                .join("recursa-generations/g_next/root.rs")
                .is_file()
        );
        assert!(
            output
                .path()
                .join("recursa-generations/g_next/node-0.rs")
                .is_file()
        );
    }
}

#[cfg(unix)]
pub(super) fn filesystem_invalidation_failure_poisons_stale_selection_and_recovers() {
    use std::os::unix::fs::PermissionsExt as _;

    let output = tempfile::tempdir().unwrap();
    let old = publication_fixture("g_old");
    publish_with_checkpoint(output.path(), "g_old", &old, |_| Ok(())).unwrap();
    assert_eq!(selected_generation(output.path()).as_deref(), Some("g_old"));

    let original_permissions = fs::metadata(output.path()).unwrap().permissions();
    let mut non_writable = original_permissions.clone();
    non_writable.set_mode(0o555);
    fs::set_permissions(output.path(), non_writable).unwrap();
    let error = invalidate_selection(output.path()).unwrap_err();
    fs::set_permissions(output.path(), original_permissions).unwrap();

    assert!(error.contains("cannot invalidate"), "{error}");
    assert_eq!(selected_generation(output.path()), None);
    let poisoned = fs::read_to_string(output.path().join(SELECTED_GENERATION)).unwrap();
    assert!(poisoned.contains("compile_error!"));
    assert!(!poisoned.contains("recursa-generation:g_old"));

    invalidate_selection(output.path()).unwrap();
    let next = publication_fixture("g_next");
    publish_with_checkpoint(output.path(), "g_next", &next, |_| Ok(())).unwrap();
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some("g_next")
    );
}

pub(super) fn filesystem_rename_failures_leave_no_loadable_or_staged_generation() {
    let output = tempfile::tempdir().unwrap();
    let rendered = publication_fixture("g_next");
    let published = output.path().join("recursa-generations/g_next");
    let error = publish_with_checkpoint(output.path(), "g_next", &rendered, |checkpoint| {
        if checkpoint == PublicationCheckpoint::DuringStaging {
            fs::create_dir_all(&published).unwrap();
            fs::write(published.join("block"), "external collision").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains("publication barrier"), "{error}");
    assert_eq!(selected_generation(output.path()), None);
    assert_no_transaction_staging(output.path());

    fs::remove_dir_all(&published).unwrap();
    fs::create_dir(output.path().join(SELECTED_GENERATION)).unwrap();
    let error =
        publish_with_checkpoint(output.path(), "g_next", &rendered, |_| Ok(())).unwrap_err();
    assert!(error.contains("atomically select"), "{error}");
    assert_eq!(selected_generation(output.path()), None);
    assert_no_transaction_staging(output.path());
}

fn assert_no_transaction_staging(out_dir: &Path) {
    let mut pending = vec![out_dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let name = path.file_name().unwrap().to_string_lossy();
                assert!(!name.starts_with(".recursa-selector-"), "{path:?}");
            }
        }
    }
    let generations = out_dir.join("recursa-generations");
    if let Ok(entries) = fs::read_dir(generations) {
        for entry in entries {
            let path = entry.unwrap().path();
            assert!(
                !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".staging-"),
                "{path:?}"
            );
        }
    }
}

/// Small conflict-free grammar used to exercise generation reuse.
const REUSE_GRAMMAR: &str = r#"recursa::grammar! { module = crate }
recursa::tokens! { keywords { SELECT => "SELECT" } }

recursa::ast_node! {

    #[tok(SELECT)]
    struct Select;
}

recursa::ast_node! {

    struct Root {
        items: zero_or_many!(Select),
    }
}
"#;

/// One in-process generation run over a package directory and an output directory.
fn generate_reuse_fixture(
    package: &Path,
    out_dir: &Path,
    debug_directory: Option<PathBuf>,
) -> (Generation, Vec<String>) {
    let request = GenerateRequest {
        out_dir: out_dir.to_path_buf(),
        manifest_dir: package.to_path_buf(),
        selected_root: PathBuf::from("grammar.rs"),
        debug_directory,
        cfg: CargoCfgSnapshot::default(),
    };
    let mut cargo = Vec::new();
    let generation = generate_with(&request, &mut cargo).expect("the reuse fixture generates");
    (generation, cargo)
}

/// Keeps only the Cargo warning directives of one build-script output.
fn warning_lines(cargo: &[String]) -> Vec<String> {
    cargo
        .iter()
        .filter(|line| line.starts_with("cargo::warning="))
        .cloned()
        .collect()
}

/// Keeps only the Cargo invalidation directives of one build-script output.
fn invalidation_lines(cargo: &[String]) -> Vec<String> {
    cargo
        .iter()
        .filter(|line| line.starts_with("cargo::rerun-if-"))
        .cloned()
        .collect()
}

pub(super) fn unchanged_sources_reuse_the_published_generation_without_analysis() {
    let package = tempfile::tempdir().unwrap();
    fs::write(package.path().join("grammar.rs"), REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();

    let (first, first_cargo) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    assert!(first.report().warnings().next().is_none());
    let identity = selected_generation(output.path()).expect("first run selects a Generation");
    assert!(warning_lines(&first_cargo).is_empty());
    assert!(output.path().join("recursa-reuse").is_dir());

    let (second, second_cargo) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(
        second.reused(),
        "unchanged sources must reuse the Generation"
    );
    assert!(second.report().warnings().next().is_none());
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str())
    );
    assert_eq!(
        warning_lines(&second_cargo),
        warning_lines(&first_cargo),
        "reuse must replay exactly the recorded warnings"
    );
    assert_eq!(
        invalidation_lines(&second_cargo),
        invalidation_lines(&first_cargo),
        "reuse must keep every Cargo invalidation directive"
    );
    assert_no_transaction_staging(output.path());
}

pub(super) fn comment_only_edits_regenerate_loader_anchors() {
    let package = tempfile::tempdir().unwrap();
    let grammar = package.path().join("grammar.rs");
    fs::write(&grammar, REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    let identity = selected_generation(output.path()).unwrap();

    let edited = format!("// leading comment\n{REUSE_GRAMMAR}\n/* trailing\n   block */\n");
    fs::write(&grammar, edited).unwrap();
    let (second, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(
        !second.reused(),
        "comment-only edits can move declaration spans and must regenerate loader anchors"
    );
    assert_ne!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str()),
        "moved declaration spans must select a distinct Generation"
    );
}

pub(super) fn semantic_edits_do_not_reuse_the_published_generation() {
    let package = tempfile::tempdir().unwrap();
    let grammar = package.path().join("grammar.rs");
    fs::write(&grammar, REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    let identity = selected_generation(output.path()).unwrap();

    let edited = format!(
        r#"{REUSE_GRAMMAR}
recursa::ast_node! {{

    struct Extra {{
        first: Select,
        second: Select,
    }}
}}
"#
    );
    fs::write(&grammar, edited).unwrap();
    let (second, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!second.reused(), "a semantic edit must analyze again");
    let relocated = selected_generation(output.path()).unwrap();
    assert_ne!(relocated, identity);

    // Going back to the first grammar hits the first entry again.
    fs::write(&grammar, REUSE_GRAMMAR).unwrap();
    let (third, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(third.reused());
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str())
    );
}

pub(super) fn missing_or_incomplete_generations_are_not_reused() {
    let package = tempfile::tempdir().unwrap();
    fs::write(package.path().join("grammar.rs"), REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    let identity = selected_generation(output.path()).unwrap();
    let published = output.path().join("recursa-generations").join(&identity);

    // A Generation directory that lost its loader map is never re-selected.
    let loader_map = fs::read(published.join("loader-map")).unwrap();
    fs::remove_file(published.join("loader-map")).unwrap();
    invalidate_selection(output.path()).unwrap();
    let key = fs::read_dir(output.path().join("recursa-reuse"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .find(|name| name.starts_with("r_"))
        .expect("the first run recorded a reuse entry");
    assert!(reuse_published(output.path(), &key).unwrap().is_none());
    assert!(!output.path().join(SELECTED_GENERATION).exists());

    // The binary LR tables are part of the same completeness barrier.
    fs::write(published.join("loader-map"), loader_map).unwrap();
    let asset = published.join(LR_TABLE_ASSET_NAMES[0]);
    let bytes = fs::read(&asset).unwrap();
    fs::remove_file(&asset).unwrap();
    assert!(reuse_published(output.path(), &key).unwrap().is_none());
    assert!(!output.path().join(SELECTED_GENERATION).exists());
    fs::write(asset, bytes).unwrap();

    // A missing Generation directory falls through to a fresh publication.
    fs::remove_dir_all(&published).unwrap();
    let (second, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(
        !second.reused(),
        "a missing Generation must be published again"
    );
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str())
    );
    assert!(published.join("loader-map").is_file());

    let (third, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(third.reused());
}

pub(super) fn debug_directory_requests_bypass_reuse() {
    let package = tempfile::tempdir().unwrap();
    fs::write(package.path().join("grammar.rs"), REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());

    let debug = tempfile::tempdir().unwrap();
    let (second, _) = generate_reuse_fixture(
        package.path(),
        output.path(),
        Some(debug.path().to_path_buf()),
    );
    assert!(
        !second.reused(),
        "a debug bundle request must run the complete pipeline"
    );
    assert!(second.report().warnings().next().is_none());

    let (third, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(third.reused());
}
