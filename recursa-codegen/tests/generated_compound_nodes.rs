use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

const FIXTURE: &str = "tests/fixtures/generation/compound-nodes";

/// Exclusive access to the stable downstream fixture workspace.
static FIXTURE_WORKSPACE: OnceLock<Mutex<()>> = OnceLock::new();

/// Monotonic input used to force the fixture build script to rerun per scenario.
static FIXTURE_SCENARIO: AtomicU64 = AtomicU64::new(0);

/// A reset fixture workspace that retains Cargo's compiled dependency cache.
struct Fixture {
    /// Stable package directory under the workspace target directory.
    path: PathBuf,
    /// Guard preventing concurrent scenarios from changing the shared package.
    _guard: MutexGuard<'static, ()>,
}

impl Fixture {
    /// Returns the stable fixture package directory.
    fn path(&self) -> &Path {
        &self.path
    }
}

#[test]
fn fixture_locks_use_the_workspace_registry_resolution_and_cover_capabilities() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let workspace_lock =
        fs::read_to_string(workspace.join("Cargo.lock")).expect("read workspace dependency lock");
    let workspace_packages = registry_package_records(&workspace_lock);
    for (lock, capability_dependencies) in [
        (
            "Cargo.core.lock",
            &["arbitrary", "serde", "serde_derive"][..],
        ),
        (
            "Cargo.lock",
            &[
                "arbitrary",
                "railroad",
                "serde",
                "serde_derive",
                "unicode-width",
            ][..],
        ),
    ] {
        let fixture_lock = fs::read_to_string(manifest_dir.join(FIXTURE).join(lock))
            .unwrap_or_else(|error| {
                panic!("read compound fixture dependency lock {lock}: {error}")
            });
        let fixture_packages = registry_package_records(&fixture_lock);
        for package in &fixture_packages {
            assert!(
                workspace_packages.contains(package),
                "{lock} registry resolution {package:?} is not preloaded by the workspace lock",
            );
        }
        for capability_dependency in capability_dependencies {
            assert!(
                fixture_packages
                    .iter()
                    .any(|(name, _, _)| name == capability_dependency),
                "{lock} omits optional capability dependency {capability_dependency}",
            );
        }
    }
}

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
fn fixture_scenarios_reuse_one_workspace_sequentially() {
    let first = scaffold_fixture();
    let first_path = first.path().to_owned();
    drop(first);

    let second = scaffold_fixture();
    assert_eq!(first_path, second.path());
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
        "    fn assert_arbitrary<T: for<'arbitrary> recursa::Arbitrary<'arbitrary>>() {}\n    fn assert_serde<T>()\n    where\n        T: recursa::__private::serde::Serialize,\n        T: for<'deserialize> recursa::__private::serde::Deserialize<'deserialize>,\n    {\n    }\n",
        "",
    );
    let source = source.replace(
        "    assert_arbitrary::<grammar::Record<'static>>();\n    assert_arbitrary::<grammar::RequiredThenPresence<'static>>();\n    assert_serde::<grammar::RequiredThenPresence<'static>>();\n",
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
    let assertions = r##"
    let adapter = grammar::Record::explorer();
    let schema = adapter.schema();
    assert_eq!(schema.root().name(), "Record");
    assert!(schema.root().qualified_name().ends_with("grammar::Record"));
    assert!(adapter.capabilities().pretty());
    assert!(adapter.capabilities().arbitrary());
    assert!(adapter.capabilities().recovery());
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

    let parsed_by_adapter = adapter.parse("ReCoRd:alpha 42")
        .expect("valid source parses through the erased adapter");
    assert_eq!(parsed_by_adapter.provenance(), recursa::explorer::Provenance::Parsed);
    assert_eq!(parsed_by_adapter.source(), Some("ReCoRd:alpha 42"));
    let recursa::explorer::OccurrenceSchema::Node(parsed_schema) =
        parsed_by_adapter.root().schema()
    else {
        panic!("adapter root is a Node schema")
    };
    assert_eq!(parsed_schema.qualified_name(), schema.root().qualified_name());
    assert_eq!(parsed_by_adapter.root().span(), Some(recursa::Span::new(0, 15).unwrap()));
    assert_eq!(
        parsed_by_adapter.pretty(&recursa::PrettyConfig::default()).unwrap().rendered,
        "ReCoRd:alpha 42",
    );

    let explored = adapter.explore("ReCoRd:alpha 42")
        .expect("compound roots advertise recovery")
        .expect("valid source explores");
    let recursa::explorer::Exploration::Complete(explored_document) = explored else {
        panic!("valid source produces a complete Explorer document")
    };
    assert_eq!(explored_document.source(), Some("ReCoRd:alpha 42"));
    assert_eq!(
        explored_document.root().span(),
        Some(recursa::Span::new(0, 15).unwrap()),
    );

    let lexical_failure = adapter.parse("record:alpha @ #").err().expect("lexical failure");
    assert_eq!(lexical_failure.source(), "record:alpha @ #");
    assert_eq!(lexical_failure.diagnostics().len(), 2);
    assert!(lexical_failure.diagnostics().iter().all(|diagnostic| {
        diagnostic.kind() == recursa::explorer::ParseDiagnosticKind::LexicalError
    }));
    assert_eq!(lexical_failure.diagnostics()[0].text(), "@");
    assert_eq!(lexical_failure.diagnostics()[0].span(), recursa::Span::new(13, 14).unwrap());
    assert_eq!(lexical_failure.diagnostics()[1].text(), "#");

    let lexical_recovery = adapter.explore("record:alpha @ #")
        .expect("compound roots advertise recovery")
        .expect("finalized lexical records recover without re-lexing");
    let recursa::explorer::Exploration::Recovered(lexical_recovery) = lexical_recovery else {
        panic!("lexical errors produce a recovery projection")
    };
    assert_eq!(lexical_recovery.diagnostics().len(), 2);
    for (strict, recovered) in lexical_failure
        .diagnostics()
        .iter()
        .zip(lexical_recovery.diagnostics())
    {
        assert_eq!(recovered.kind(), strict.kind());
        assert_eq!(recovered.span(), strict.span());
        assert_eq!(recovered.text(), strict.text());
        assert_eq!(recovered.found(), strict.found());
        assert_eq!(
            recovered.expected().collect::<Vec<_>>(),
            strict.expected().collect::<Vec<_>>(),
        );
        assert_eq!(recovered.message(), strict.message());
        assert_eq!(
            recovered.action(),
            Some(recursa::explorer::RecoveryAction::Skipped {
                span: recursa::Span::new(13, 16).unwrap(),
            }),
        );
    }
    let lexical_parts = lexical_recovery.root().parts().collect::<Vec<_>>();
    let recursa::explorer::RecoveryPart::Skipped(lexical_skipped) = lexical_parts[3] else {
        panic!("the lexical suffix is one maximal skipped part")
    };
    assert_eq!(lexical_skipped.span(), recursa::Span::new(13, 16).unwrap());
    assert!(lexical_skipped.items().all(|item| item.schema().is_none()));

    let token_failure = adapter.parse("record:42").err().expect("token failure");
    let [token_diagnostic] = token_failure.diagnostics() else {
        panic!("one unexpected-token diagnostic")
    };
    assert_eq!(
        token_diagnostic.kind(),
        recursa::explorer::ParseDiagnosticKind::UnexpectedToken,
    );
    assert_eq!(token_diagnostic.text(), "42");
    assert!(token_diagnostic.found().unwrap().contains("[0-9]+"));
    assert!(token_diagnostic.expected().any(|expected| expected.contains("[A-Za-zé]+")));

    let recovered_token = adapter.explore("record:42")
        .expect("compound roots advertise recovery")
        .expect("a missing required field before a valid suffix is recoverable");
    let recursa::explorer::Exploration::Recovered(recovered_token) = recovered_token else {
        panic!("strict token failure produces a recovery document")
    };
    assert_eq!(
        recovered_token.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Missing {
            at: recursa::Span::new(7, 7).unwrap(),
        }),
    );
    let token_parts = recovered_token.root().parts().collect::<Vec<_>>();
    assert_eq!(token_parts.len(), 4);
    let recursa::explorer::RecoveryPart::Missing(missing_label) = token_parts[2] else {
        panic!("the required label is missing before the optional count")
    };
    assert_eq!(missing_label.element().name(), "label");
    let recursa::explorer::RecoveryPart::Element(count) = token_parts[3] else {
        panic!("the valid optional count suffix remains recognized")
    };
    assert_eq!(count.schema().name(), "count");
    assert_eq!(count.span(), recursa::Span::new(7, 9).unwrap());

    let skipped = adapter.explore("record:!alpha")
        .expect("compound roots advertise recovery")
        .expect("unexpected syntax before a restart token is recoverable");
    let recursa::explorer::Exploration::Recovered(skipped) = skipped else {
        panic!("strict token failure produces a recovery document")
    };
    assert_eq!(
        skipped.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Skipped {
            span: recursa::Span::new(7, 8).unwrap(),
        }),
    );
    let skipped_parts = skipped.root().parts().collect::<Vec<_>>();
    assert_eq!(skipped_parts.len(), 4);
    let recursa::explorer::RecoveryPart::Skipped(discarded) = skipped_parts[2] else {
        panic!("the invalid prefix is owned by one skipped part")
    };
    assert_eq!(discarded.span(), recursa::Span::new(7, 8).unwrap());
    let discarded_items = discarded.items().collect::<Vec<_>>();
    assert_eq!(discarded_items.len(), 1);
    assert_eq!(discarded_items[0].schema().unwrap().canonical_text(), Some("!"));
    let recursa::explorer::RecoveryPart::Element(label) = skipped_parts[3] else {
        panic!("recovery retries and recognizes the label restart")
    };
    assert_eq!(label.schema().name(), "label");
    assert_eq!(label.span(), recursa::Span::new(8, 13).unwrap());

    let progress_source = format!("record:{}alpha", "!".repeat(256));
    let progress = adapter
        .explore(&progress_source)
        .expect("compound roots advertise recovery")
        .expect("a long invalid run terminates through the public seam");
    let recursa::explorer::Exploration::Recovered(progress) = progress else {
        panic!("a long invalid run produces recovery")
    };
    let trace = progress.progress_trace();
    assert_eq!(trace.input_records(), 259);
    assert!(trace.entered_frames() >= 2);
    assert_eq!(trace.entered_frames(), trace.closed_frames());
    assert!(trace.guarded_states() <= trace.state_bound());
    assert!(trace.is_within_bound());
    let progress_parts = progress.root().parts().collect::<Vec<_>>();
    assert_eq!(progress_parts.len(), 4, "skipped input is not rescanned");
    let recursa::explorer::RecoveryPart::Skipped(progress_skip) = progress_parts[2] else {
        panic!("the long invalid run is retained as one maximal skipped part")
    };
    assert_eq!(progress_skip.items().count(), 256);
    assert!(progress.diagnostics().iter().all(|diagnostic| {
        diagnostic.kind() != recursa::explorer::ParseDiagnosticKind::InvariantViolation
    }));

    let eof_failure = adapter.parse("record:").err().expect("EOF failure");
    let [eof_diagnostic] = eof_failure.diagnostics() else {
        panic!("one EOF diagnostic")
    };
    assert_eq!(eof_diagnostic.kind(), recursa::explorer::ParseDiagnosticKind::UnexpectedEof);
    assert_eq!(eof_diagnostic.span(), recursa::Span::new(7, 7).unwrap());
    assert_eq!(eof_diagnostic.text(), "");
    assert_eq!(eof_diagnostic.found(), None);

    let recovered = adapter.explore("record:")
        .expect("compound roots advertise recovery")
        .expect("missing required syntax is recoverable");
    let recursa::explorer::Exploration::Recovered(recovered) = recovered else {
        panic!("strict EOF failure produces a recovery document")
    };
    assert_eq!(recovered.source(), "record:");
    assert_eq!(recovered.diagnostics().len(), eof_failure.diagnostics().len());
    assert_eq!(recovered.diagnostics()[0].kind(), eof_diagnostic.kind());
    assert_eq!(recovered.diagnostics()[0].span(), eof_diagnostic.span());
    assert_eq!(recovered.diagnostics()[0].text(), eof_diagnostic.text());
    assert_eq!(recovered.diagnostics()[0].found(), eof_diagnostic.found());
    assert_eq!(
        recovered.diagnostics()[0].expected().collect::<Vec<_>>(),
        eof_diagnostic.expected().collect::<Vec<_>>(),
    );
    assert_eq!(recovered.diagnostics()[0].message(), eof_diagnostic.message());
    assert_eq!(recovered.diagnostics()[0].owner().unwrap().name(), "Record");
    assert_eq!(recovered.diagnostics()[0].site().unwrap().name(), "label");
    assert_eq!(
        recovered.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Missing {
            at: recursa::Span::new(7, 7).unwrap(),
        }),
    );
    let root = recovered.root();
    assert_eq!(root.schema().qualified_name(), schema.root().qualified_name());
    assert!(root.committed_variant().is_none());
    assert_eq!(root.extent(), recursa::Span::new(0, 7).unwrap());
    assert!(!root.is_complete());
    let parts = root.parts().collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    assert!(matches!(parts[0], recursa::explorer::RecoveryPart::Element(_)));
    assert!(matches!(parts[1], recursa::explorer::RecoveryPart::Element(_)));
    let recursa::explorer::RecoveryPart::Missing(missing) = parts[2] else {
        panic!("required label is represented by one missing part")
    };
    assert_eq!(missing.element().name(), "label");
    assert_eq!(missing.at(), recursa::Span::new(7, 7).unwrap());

    let trailing_failure = adapter.parse("record:alpha 42 beta").err().expect("trailing input");
    let [trailing_diagnostic] = trailing_failure.diagnostics() else {
        panic!("one trailing-input diagnostic")
    };
    assert_eq!(trailing_diagnostic.kind(), recursa::explorer::ParseDiagnosticKind::TrailingInput);
    assert_eq!(trailing_diagnostic.text(), "beta");
    assert_eq!(trailing_diagnostic.expected().collect::<Vec<_>>(), ["end of input"]);

    let trailing_recovery = adapter.explore("record:alpha 42 beta")
        .expect("compound roots advertise recovery")
        .expect("trailing input is recoverable");
    let recursa::explorer::Exploration::Recovered(trailing_recovery) = trailing_recovery else {
        panic!("trailing input produces a recovery projection")
    };
    assert_eq!(trailing_recovery.diagnostics().len(), 1);
    assert_eq!(
        trailing_recovery.diagnostics()[0].kind(),
        recursa::explorer::ParseDiagnosticKind::TrailingInput,
    );
    assert_eq!(trailing_recovery.diagnostics()[0].message(), trailing_diagnostic.message());
    assert_eq!(
        trailing_recovery.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Skipped {
            span: recursa::Span::new(16, 20).unwrap(),
        }),
    );

    let unicode = adapter.parse("record:café 42").expect("Unicode content parses");
    assert_eq!(unicode.source(), Some("record:café 42"));
    assert_eq!(unicode.root().span(), Some(recursa::Span::new(0, 15).unwrap()));
    let unicode_text = unicode.root().elements().nth(2).unwrap()
        .children().next().unwrap().elements().next().unwrap()
        .children().next().unwrap().token_text();
    assert_eq!(unicode_text, Some("café"));

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

    let nested_adapter = grammar::NestedRecovery::explorer();
    let nested = nested_adapter.explore("create pair view")
        .expect("compound nested roots advertise recovery")
        .expect("nested FOLLOW recovery succeeds");
    let recursa::explorer::Exploration::Recovered(nested) = nested else {
        panic!("a missing nested field produces a recovery document")
    };
    assert_eq!(
        nested.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Missing {
            at: recursa::Span::new(12, 12).unwrap(),
        }),
    );
    let nested_parts = nested.root().parts().collect::<Vec<_>>();
    assert_eq!(nested_parts.len(), 3);
    let recursa::explorer::RecoveryPart::Element(pair_element) = nested_parts[1] else {
        panic!("the owner retains its nested Pair")
    };
    let recursa::explorer::RecoveryValue::Node(pair) = pair_element.value() else {
        panic!("the Pair recovery value remains construction-free")
    };
    assert_eq!(pair.schema().name(), "Pair");
    assert_eq!(pair.extent(), recursa::Span::new(7, 12).unwrap());
    let pair_parts = pair.parts().collect::<Vec<_>>();
    assert_eq!(pair_parts.len(), 2);
    let recursa::explorer::RecoveryPart::Missing(nested_label) = pair_parts[1] else {
        panic!("the deepest Pair frame owns the missing label")
    };
    assert_eq!(nested_label.element().name(), "field_0");
    assert_eq!(nested_label.at(), recursa::Span::new(12, 12).unwrap());
    let recursa::explorer::RecoveryPart::Element(owner_suffix) = nested_parts[2] else {
        panic!("the owner recognizes its suffix after the child unwinds")
    };
    assert_eq!(owner_suffix.schema().name(), "view");
    assert_eq!(owner_suffix.span(), recursa::Span::new(12, 16).unwrap());

    let entry_adapter = grammar::Entry::explorer();
    let variants = entry_adapter.schema().root().variants()
        .map(|variant| (variant.name(), variant.elements().len()))
        .collect::<Vec<_>>();
    assert_eq!(variants, [("Record", 1), ("Pair", 1), ("Empty", 1)]);
    let empty = entry_adapter.parse("empty").expect("enum root parses");
    assert_eq!(empty.root().active_variant().unwrap().name(), "Empty");

    let empty_with_invalid_suffix = entry_adapter.explore("empty nope")
        .expect("fixed-only enum branches advertise recovery")
        .expect("a unique fixed leaf commits before invalid trailing input");
    let recursa::explorer::Exploration::Recovered(empty_with_invalid_suffix) =
        empty_with_invalid_suffix
    else {
        panic!("invalid trailing input produces recovery under the committed branch")
    };
    assert_eq!(
        empty_with_invalid_suffix.root().committed_variant().unwrap().name(),
        "Empty",
        "a consumed unique dispatch leaf commits even when it has no descendants",
    );

    let committed_adapter = grammar::RecoveryChoice::explorer();
    let empty_single_variant = committed_adapter.explore("")
        .expect("non-Pratt enum roots advertise recovery")
        .expect("EOF before dispatch commitment is recoverable");
    let recursa::explorer::Exploration::Recovered(empty_single_variant) = empty_single_variant else {
        panic!("empty enum input produces recovery")
    };
    assert!(
        empty_single_variant.root().committed_variant().is_none(),
        "shared descendants cannot precommit before a frozen dispatch edge is consumed",
    );

    let committed = committed_adapter.explore("right nope")
        .expect("non-Pratt enum roots advertise recovery")
        .expect("committed enum syntax recovers");
    let recursa::explorer::Exploration::Recovered(committed) = committed else {
        panic!("invalid committed enum source produces recovery")
    };
    assert_eq!(committed.root().committed_variant().unwrap().name(), "Right");
    assert!(!committed.root().is_complete());
    assert_eq!(
        committed.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Skipped {
            span: recursa::Span::new(6, 10).unwrap(),
        }),
    );
    assert_eq!(committed.diagnostics()[1].kind(), recursa::explorer::ParseDiagnosticKind::MissingSyntax);
    assert_eq!(
        committed.diagnostics()[1].action(),
        Some(recursa::explorer::RecoveryAction::Missing {
            at: recursa::Span::new(10, 10).unwrap(),
        }),
    );

    let uncommitted = entry_adapter.explore("42")
        .expect("non-Pratt enum roots advertise recovery")
        .expect("uncommitted enum syntax recovers without guessing");
    let recursa::explorer::Exploration::Recovered(uncommitted) = uncommitted else {
        panic!("invalid uncommitted enum source produces recovery")
    };
    assert!(uncommitted.root().committed_variant().is_none());
    let uncommitted_parts = uncommitted.root().parts().collect::<Vec<_>>();
    assert!(matches!(uncommitted_parts.as_slice(), [
        recursa::explorer::RecoveryPart::Skipped(_),
    ]));
    assert_eq!(uncommitted.diagnostics().len(), 2);
    assert_eq!(
        uncommitted.diagnostics()[1].kind(),
        recursa::explorer::ParseDiagnosticKind::UnresolvedAlternative,
    );
    assert_eq!(uncommitted.diagnostics()[1].owner().unwrap().name(), "Entry");
    assert!(uncommitted.diagnostics()[1].site().is_none());

    let repeated_adapter = grammar::SomeCounts::explorer();
    let repeated_schema = repeated_adapter.schema().root().elements().nth(1).unwrap();
    assert_eq!(repeated_schema.cardinality(), recursa::explorer::Cardinality::OptionalOneOrMore);
    let repeated = repeated_adapter.parse("right 1:2").expect("repetition root parses");
    assert_eq!(repeated.root().elements().nth(1).unwrap().children().len(), 2);

    let missing_after_separator = repeated_adapter.explore("right 1:")
        .expect("optional nonempty repetition advertises recovery")
        .expect("a committed repetition recovers at EOF");
    let recursa::explorer::Exploration::Recovered(missing_after_separator) =
        missing_after_separator
    else {
        panic!("a non-trailing separator produces recovery")
    };
    let repeated_parts = missing_after_separator.root().parts().collect::<Vec<_>>();
    assert!(matches!(repeated_parts.as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Separator(_),
        recursa::explorer::RecoveryPart::Missing(_),
    ]));
    let recursa::explorer::RecoveryPart::Separator(separator) = repeated_parts[2] else {
        unreachable!()
    };
    assert_eq!(separator.schema().name(), "COLON");
    assert_eq!(separator.span(), recursa::Span::new(7, 8).unwrap());
    assert_eq!(
        missing_after_separator.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Missing {
            at: recursa::Span::new(8, 8).unwrap(),
        }),
    );

    let resumed_repetition = repeated_adapter.explore("right 1::2")
        .expect("optional nonempty repetition advertises recovery")
        .expect("repetition resumes after skipped input");
    let recursa::explorer::Exploration::Recovered(resumed_repetition) = resumed_repetition else {
        panic!("an extra separator produces recovery")
    };
    assert!(matches!(resumed_repetition.root().parts().collect::<Vec<_>>().as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Separator(_),
        recursa::explorer::RecoveryPart::Skipped(_),
        recursa::explorer::RecoveryPart::Element(_),
    ]));

    let missing_separator = repeated_adapter.explore("right 1 2")
        .expect("separated repetitions advertise recovery")
        .expect("a missing separator recovers before the next occurrence");
    let recursa::explorer::Exploration::Recovered(missing_separator) = missing_separator else {
        panic!("a missing separator produces recovery")
    };
    assert!(matches!(missing_separator.root().parts().collect::<Vec<_>>().as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Missing(_),
        recursa::explorer::RecoveryPart::Element(_),
    ]));
    assert_eq!(
        missing_separator.diagnostics()[0].action(),
        Some(recursa::explorer::RecoveryAction::Missing {
            at: recursa::Span::new(8, 8).unwrap(),
        }),
    );

    let required_counts = grammar::RequiredCounts::explorer().explore("create")
        .expect("required nonempty repetition advertises recovery")
        .expect("required nonempty repetition recovers at EOF");
    let recursa::explorer::Exploration::Recovered(required_counts) = required_counts else {
        panic!("a missing required repetition produces recovery")
    };
    assert!(matches!(required_counts.root().parts().collect::<Vec<_>>().as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Missing(_),
    ]));

    let zero_values = grammar::ZeroLabelsThenSuffix::explorer().explore("left")
        .expect("zero-or-more containers advertise recovery")
        .expect("zero occurrences remain valid before a later failure");
    let recursa::explorer::Exploration::Recovered(zero_values) = zero_values else {
        panic!("the later suffix failure produces recovery")
    };
    assert!(matches!(zero_values.root().parts().collect::<Vec<_>>().as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Missing(_),
    ]));
    assert_eq!(zero_values.diagnostics().len(), 1);
    assert_eq!(zero_values.diagnostics()[0].site().unwrap().name(), "materialized");

    let absent_values = grammar::OptionalCountsThenSuffix::explorer().explore("right view")
        .expect("optional nonempty containers advertise recovery")
        .expect("an absent optional container remains valid before a later failure");
    let recursa::explorer::Exploration::Recovered(absent_values) = absent_values else {
        panic!("the later required suffix produces recovery")
    };
    assert!(matches!(absent_values.root().parts().collect::<Vec<_>>().as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Missing(_),
    ]));
    assert_eq!(absent_values.diagnostics().len(), 1);

    let fixed_steps = grammar::MultipleRequiredAttachment::explorer().explore("")
        .expect("fixed attachments advertise recovery")
        .expect("all required attachment sites recover at EOF");
    let recursa::explorer::Exploration::Recovered(fixed_steps) = fixed_steps else {
        panic!("missing fixed attachment sites produce recovery")
    };
    assert_eq!(
        fixed_steps.root().parts().filter(|part| {
            matches!(part, recursa::explorer::RecoveryPart::Missing(_))
        }).count(),
        5,
    );
    assert!(fixed_steps.progress_trace().guarded_states() >= 5);
    for (index, diagnostic) in fixed_steps.diagnostics().iter().enumerate() {
        assert!(fixed_steps.diagnostics()[index + 1..].iter().all(|other| {
            diagnostic.kind() != other.kind()
                || diagnostic.span() != other.span()
                || diagnostic.owner().map(|owner| owner.qualified_name())
                    != other.owner().map(|owner| owner.qualified_name())
                || diagnostic.site().map(|site| site.name())
                    != other.site().map(|site| site.name())
                || diagnostic.expected().collect::<Vec<_>>()
                    != other.expected().collect::<Vec<_>>()
                || diagnostic.action() != other.action()
        }), "exact diagnostic keys are deduplicated");
    }

    let duplicate_keys = grammar::DuplicateRequiredAttachment::explorer().explore("")
        .expect("duplicate required sites advertise recovery")
        .expect("duplicate required sites recover at EOF");
    let recursa::explorer::Exploration::Recovered(duplicate_keys) = duplicate_keys else {
        panic!("duplicate missing sites produce recovery")
    };
    let duplicate_missing_expected = duplicate_keys.root().parts().filter_map(|part| {
        let recursa::explorer::RecoveryPart::Missing(missing) = part else {
            return None;
        };
        Some(missing.expected().collect::<Vec<_>>())
    }).collect::<Vec<_>>();
    assert_eq!(
        duplicate_missing_expected,
        [
            vec!["fixed:CREATE"],
            vec!["fixed:CREATE"],
            vec!["fixed:CREATE"],
            vec![r#"content:"[A-Za-zé]+":priority=None"#],
        ],
        "all identical missing recovery parts remain in the projection",
    );
    assert_eq!(
        duplicate_keys.diagnostics().iter().filter(|diagnostic| {
            diagnostic.kind() == recursa::explorer::ParseDiagnosticKind::MissingSyntax
                && diagnostic.expected().eq(["fixed:CREATE"])
        }).count(),
        1,
        "two identical generated MissingSyntax keys collapse to one diagnostic",
    );

    let atomic_optional = grammar::StandaloneAtomicOptional::explorer().explore("or replace")
        .expect("atomic optional fixed syntax advertises recovery")
        .expect("a later required value recovers after the committed optional sequence");
    let recursa::explorer::Exploration::Recovered(atomic_optional) = atomic_optional else {
        panic!("the missing semantic value produces recovery")
    };
    let atomic_optional_part_count = atomic_optional.root().parts().count();
    let atomic_optional_trace = atomic_optional.progress_trace();
    assert_eq!(atomic_optional_part_count, 3);
    assert!(
        atomic_optional_part_count
            <= atomic_optional_trace.guarded_states() - atomic_optional_trace.closed_frames(),
        "each non-closing guarded transition emits at most one recovery part",
    );
    assert!(
        atomic_optional_trace.guarded_states() >= 6,
        "the committed two-token standalone sequence uses two guarded transitions",
    );

    let attached_atomic = grammar::AtomicOptionalAttachment::explorer().explore("or replace")
        .expect("atomic attached fixed syntax advertises recovery")
        .expect("a later required value recovers after the committed attached sequence");
    let recursa::explorer::Exploration::Recovered(attached_atomic) = attached_atomic else {
        panic!("the missing attached semantic value produces recovery")
    };
    let attached_atomic_part_count = attached_atomic.root().parts().count();
    let attached_atomic_trace = attached_atomic.progress_trace();
    assert_eq!(attached_atomic_part_count, 3);
    assert!(
        attached_atomic_part_count
            <= attached_atomic_trace.guarded_states() - attached_atomic_trace.closed_frames(),
        "attached optional tokens advance through distinct guarded transitions",
    );
    assert!(
        attached_atomic_trace.guarded_states() >= 5,
        "the committed two-token attached sequence uses two guarded transitions: {attached_atomic_trace:?}",
    );

    let atomic_presence = grammar::AtomicPresence::explorer().explore("or replace")
        .expect("atomic presence syntax advertises recovery")
        .expect("a later required value recovers after the committed presence sequence");
    let recursa::explorer::Exploration::Recovered(atomic_presence) = atomic_presence else {
        panic!("the missing presence tail produces recovery")
    };
    let atomic_presence_part_count = atomic_presence.root().parts().count();
    let atomic_presence_trace = atomic_presence.progress_trace();
    assert!(
        atomic_presence_part_count
            <= atomic_presence_trace.guarded_states() - atomic_presence_trace.closed_frames(),
        "presence sequence tokens advance through distinct guarded transitions",
    );
    assert!(
        atomic_presence_trace.guarded_states() >= 8,
        "the committed two-token presence sequence uses two guarded transitions",
    );

    let trailing_repetition = grammar::ManyLabels::explorer().explore("left alpha::")
        .expect("trailing repetition advertises recovery")
        .expect("trailing repetition skips one extra separator");
    let recursa::explorer::Exploration::Recovered(trailing_repetition) = trailing_repetition else {
        panic!("an extra trailing separator produces recovery")
    };
    assert!(matches!(trailing_repetition.root().parts().collect::<Vec<_>>().as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Separator(_),
        recursa::explorer::RecoveryPart::Skipped(_),
    ]));

    let optional_fixed = grammar::OptionalFillers::explorer().explore("record")
        .expect("optional fixed groups advertise recovery")
        .expect("recognized optional syntax survives a later failure");
    let recursa::explorer::Exploration::Recovered(optional_fixed) = optional_fixed else {
        panic!("a missing semantic tail produces recovery")
    };
    assert!(matches!(optional_fixed.root().parts().collect::<Vec<_>>().as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Missing(_),
    ]));

    let presence = grammar::WrappedPresence::explorer()
        .explore("create materialized view")
        .expect("fixed presence groups advertise recovery")
        .expect("presence attachments survive a later failure");
    let recursa::explorer::Exploration::Recovered(presence) = presence else {
        panic!("a missing field after fixed presence syntax produces recovery")
    };
    let presence_parts = presence.root().parts().collect::<Vec<_>>();
    assert!(matches!(presence_parts.as_slice(), [
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Element(_),
        recursa::explorer::RecoveryPart::Missing(_),
    ]));
    let presence_tokens = presence_parts[..3].iter().map(|part| {
        let recursa::explorer::RecoveryPart::Element(element) = part else {
            unreachable!()
        };
        let recursa::explorer::RecoveryValue::Token(token) = element.value() else {
            unreachable!()
        };
        token.schema().canonical_text().unwrap()
    }).collect::<Vec<_>>();
    assert_eq!(presence_tokens, ["CREATE", "MATERIALIZED", "VIEW"]);

    let attached = grammar::AttachedOptionalFillers::explorer()
        .explore("create materialized")
        .expect("semantic fixed attachments advertise recovery")
        .expect("attachment suffixes remain recognizable after a missing child");
    let recursa::explorer::Exploration::Recovered(attached) = attached else {
        panic!("a missing attached semantic child produces recovery")
    };
    let attached_parts = attached.root().parts().collect::<Vec<_>>();
    let attached_tags = attached_parts
        .iter()
        .map(|part| match part {
            recursa::explorer::RecoveryPart::Element(_) => "element",
            recursa::explorer::RecoveryPart::Separator(_) => "separator",
            recursa::explorer::RecoveryPart::Missing(_) => "missing",
            recursa::explorer::RecoveryPart::Skipped(_) => "skipped",
        })
        .collect::<Vec<_>>();
    assert_eq!(attached_tags, ["element", "missing", "element"]);
    let recursa::explorer::RecoveryPart::Missing(attached_label) = attached_parts[1] else {
        unreachable!()
    };
    assert_eq!(attached_label.element().name(), "label");

    let nested_attachment = grammar::NestedWith::explorer().explore("alpha with pair")
        .expect("nested semantic attachments advertise recovery")
        .expect("attachment syntax is retained before nested recovery unwinds");
    let recursa::explorer::Exploration::Recovered(nested_attachment) = nested_attachment else {
        panic!("a nested failure after an attachment produces recovery")
    };
    assert!(nested_attachment.root().parts().any(|part| {
        let recursa::explorer::RecoveryPart::Element(element) = part else {
            return false;
        };
        let recursa::explorer::RecoveryValue::Token(token) = element.value() else {
            return false;
        };
        token.schema().canonical_text() == Some("WITH")
    }));

    let unicode_recovery = adapter.explore("record:!café")
        .expect("compound roots advertise recovery")
        .expect("UTF-8 recovery boundaries are checked");
    let recursa::explorer::Exploration::Recovered(unicode_recovery) = unicode_recovery else {
        panic!("the skipped punctuation produces recovery")
    };
    let unicode_parts = unicode_recovery.root().parts().collect::<Vec<_>>();
    let recursa::explorer::RecoveryPart::Skipped(unicode_skip) = unicode_parts[2] else {
        panic!("the punctuation is retained as skipped input")
    };
    assert_eq!(unicode_skip.span(), recursa::Span::new(7, 8).unwrap());
    let recursa::explorer::RecoveryPart::Element(unicode_label) = unicode_parts[3] else {
        panic!("the UTF-8 label is retained after synchronization")
    };
    assert_eq!(unicode_label.span(), recursa::Span::new(8, 13).unwrap());

    let same_eof = grammar::NestedRecovery::explorer().explore("create pair")
        .expect("nested roots advertise recovery")
        .expect("distinct nested EOF sites recover");
    let recursa::explorer::Exploration::Recovered(same_eof) = same_eof else {
        panic!("nested and owner EOF failures produce recovery")
    };
    assert_eq!(same_eof.diagnostics().len(), 2);
    assert_eq!(same_eof.diagnostics()[0].span(), recursa::Span::new(11, 11).unwrap());
    assert_eq!(same_eof.diagnostics()[1].span(), recursa::Span::new(11, 11).unwrap());
    assert_eq!(same_eof.diagnostics()[0].owner().unwrap().name(), "Pair");
    assert_eq!(same_eof.diagnostics()[1].owner().unwrap().name(), "NestedRecovery");
    assert_ne!(
        same_eof.diagnostics()[0].site().unwrap().name(),
        same_eof.diagnostics()[1].site().unwrap().name(),
        "distinct EOF sites are not deduplicated",
    );

    let restart_stop = grammar::RestartStopOwner::explorer().explore("pair left")
        .expect("restart/stop overlap roots advertise recovery")
        .expect("restart/stop overlap is diagnosed without guessing");
    let recursa::explorer::Exploration::Recovered(restart_stop) = restart_stop else {
        panic!("restart/stop overlap produces recovery")
    };
    let child = restart_stop.root().parts().find_map(|part| {
        let recursa::explorer::RecoveryPart::Element(element) = part else {
            return None;
        };
        let recursa::explorer::RecoveryValue::Node(node) = element.value() else {
            return None;
        };
        Some(node)
    }).expect("the uncommitted child projection is retained");
    assert!(child.committed_variant().is_none());
    assert!(restart_stop.diagnostics().iter().any(|diagnostic| {
        diagnostic.kind() == recursa::explorer::ParseDiagnosticKind::UnresolvedAlternative
            && diagnostic.owner().is_some_and(|owner| owner.name() == "RestartStopChoice")
    }));

    let private = grammar::PrivateRoot::explorer()
        .parse("left ~secret")
        .expect("private nested node parses");
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
"##;
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
fn recovery_documents_cannot_construct_or_convert_to_complete_values() {
    let package = scaffold_fixture();
    configure_explorer(&package, true, true);
    let main = package.path().join("src/main.rs");
    let source = fs::read_to_string(&main).expect("read downstream consumer");
    let proof = r#"
fn recovery_into_authored(
    recovery: recursa::explorer::RecoveryDocument<'static>,
) -> grammar::Record<'static> {
    recovery.into()
}

fn recovery_into_parsed(
    recovery: recursa::explorer::RecoveryDocument<'static>,
) -> recursa::Parsed<'static, grammar::Record<'static>> {
    recovery.into()
}

fn recovery_into_complete_document(
    recovery: recursa::explorer::RecoveryDocument<'static>,
) -> recursa::explorer::ExplorerDocument<'static> {
    recovery.into()
}

fn recovery_node_into_complete_view(
    node: &'static recursa::explorer::RecoveryNode,
) -> recursa::explorer::Node<'static> {
    node.into()
}

fn recovery_node_cannot_be_reconstructed(
    node: recursa::explorer::RecoveryNode,
) -> recursa::explorer::RecoveryNode {
    recursa::explorer::RecoveryNode { ..node }
}

"#;
    fs::write(
        &main,
        source.replace("fn main() {", &format!("{proof}fn main() {{")),
    )
    .expect("add recovery construction boundary proof");

    let output = run_fixture_unlocked(&package);
    assert!(
        !output.status.success(),
        "recovery conversions must not compile"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.matches("the trait bound").count() >= 4
            && stderr.contains("RecoveryDocument")
            && stderr.contains("ExplorerDocument")
            && stderr.contains("Record")
            && stderr.contains("private"),
        "compile-fail proof failed for an unrelated reason:\n{stderr}",
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
    let pair = rendered_diagram(&aggregate.1, "grammar::Pair");
    assert!(pair.contains(">PAIR</text>"), "Pair diagram:\n{pair}");
    assert!(
        pair.contains(">Label</text>") && !pair.contains(">grammar::Label</text>"),
        "reference labels must drop module qualifiers:\n{pair}"
    );
    assert!(
        pair.contains("<g class=\"optional\">") && pair.contains(">Count</text>"),
        "tuple fields must use referenced frozen terminal names:\n{pair}",
    );
    assert!(
        pair.contains("class=\"labeledbox\"")
            && pair.contains(">LabelText</text>")
            && pair.contains(">CountText</text>"),
        "first references must expand inline as labeled boxes:\n{pair}",
    );

    let entry = rendered_diagram(&aggregate.1, "grammar::Entry");
    assert!(
        entry.matches(">LabelText</text>").count() == 1,
        "each application must expand at most once per diagram:\n{entry}",
    );
    assert!(
        entry.matches(">Label</text>").count() >= 2,
        "repeated references must stay opaque labeled references:\n{entry}",
    );

    let many_labels = rendered_diagram(&aggregate.1, "grammar::ManyLabels");
    assert!(
        many_labels.contains(">LEFT</text>"),
        "ManyLabels diagram:\n{many_labels}"
    );
    assert!(
        many_labels.contains(">0+</text>"),
        "ManyLabels diagram:\n{many_labels}"
    );
    assert!(
        many_labels.contains("<g class=\"repeat\">")
            && many_labels.contains(">Label</text>")
            && many_labels.matches(">:</text>").count() == 2,
        "zero-or-more separator and optional-trailing semantics must be rendered:\n{many_labels}",
    );

    let some_counts = rendered_diagram(&aggregate.1, "grammar::SomeCounts");
    assert!(
        some_counts.contains(">RIGHT</text>"),
        "SomeCounts diagram:\n{some_counts}"
    );
    assert!(
        some_counts.contains(">optional 1+</text>"),
        "SomeCounts diagram:\n{some_counts}"
    );
    assert!(
        some_counts.contains("<g class=\"optional\">")
            && some_counts.contains("<g class=\"repeat\">")
            && some_counts.contains(">Count</text>")
            && some_counts.contains(">:</text>"),
        "OptionalOneOrMore semantics must remain distinct:\n{some_counts}",
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
    drop(core);

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
    fs::write(&root, source.replace("protocol_10", "protocol_999"))
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
fn explorer_projection_does_not_require_an_authored_visit_impl() {
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
    let adapter = grammar::Only::explorer();
    assert_eq!(adapter.schema().root().name(), "Only");
    assert!(adapter.parse("only").is_ok());
}
"#,
    )
    .expect("use explorer root without its source-owned Visit impl");

    let output = run_fixture_unlocked(&package);
    assert!(
        output.status.success(),
        "Explorer projection must be independent of Visit:\n{}",
        String::from_utf8_lossy(&output.stderr),
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
fn semantic_source_edits_select_a_fresh_generation_in_the_same_build_directory() {
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
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(r#"ignore = r"[ \t\n\r]+","#, r#"ignore = r"[ \t\r\n]+","#),
    )
    .expect("edit compound grammar lexical plan");
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

#[test]
fn semantically_neutral_source_edits_reuse_the_published_generation() {
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
    source.push_str("\n// semantically neutral source edit\n");
    fs::write(&grammar, source).expect("edit compound grammar comment");
    let second = run_fixture(&package);
    assert!(
        second.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let after = generation_directories(&target_dir(&package));
    assert_eq!(
        after, before,
        "a comment-only edit must not relocate the published Generation"
    );
}

fn scaffold_fixture() -> Fixture {
    let guard = FIXTURE_WORKSPACE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);
    let package_path = workspace.join("target/generated-fixtures/compound-nodes/package");
    fs::create_dir_all(&package_path).expect("create stable compound fixture package");
    clear_generated_state(&package_path.join("target"));

    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, &package_path, relative);
    }
    copy_as(&fixture, &package_path, "Cargo.core.lock", "Cargo.lock");

    let build_script = package_path.join("build.rs");
    let mut source = fs::read_to_string(&build_script).expect("read copied fixture build script");
    source.push_str(&format!(
        "\n// fixture scenario {}\n",
        FIXTURE_SCENARIO.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(build_script, source).expect("stamp fixture build script scenario");

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read compound grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package_path.join("Cargo.toml"), manifest)
        .expect("write compound grammar fixture manifest");
    Fixture {
        path: package_path,
        _guard: guard,
    }
}

/// Removes scenario-owned generated output while retaining compiled dependencies.
fn clear_generated_state(target: &Path) {
    let docs = target.join("doc");
    if docs.exists() {
        fs::remove_dir_all(docs).expect("remove previous fixture rustdoc");
    }
    remove_generation_directories(target);
}

/// Recursively removes published Generation directories from a shared Cargo target.
fn remove_generation_directories(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
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
            fs::remove_dir_all(path).expect("remove previous fixture Generations");
        } else {
            remove_generation_directories(&path);
        }
    }
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    copy_as(fixture, package, relative, relative);
}

fn copy_as(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy compound grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &Fixture) -> Output {
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
        .expect("run compound grammar fixture")
}

fn run_fixture_unlocked(package: &Fixture) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run compound grammar fixture with a mutated manifest")
}

fn run_fixture_doc(package: &Fixture, private_items: bool) -> Output {
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
        .env("CARGO_TERM_COLOR", "never")
        .env("RUSTDOCFLAGS", "-D rustdoc::broken_intra_doc_links")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("render compound grammar fixture documentation")
}

fn configure_diagrams(package: &Fixture, generator: bool, facade: bool) {
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
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        fs::copy(
            manifest_dir.join(FIXTURE).join("Cargo.lock"),
            package.path().join("Cargo.lock"),
        )
        .expect("select diagrams-capable fixture lock");
    }
    fs::write(manifest, source).expect("configure diagram capabilities");
}

fn configure_explorer(package: &Fixture, generator: bool, facade: bool) {
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

fn rendered_rustdoc(package: &Fixture) -> Vec<(PathBuf, String)> {
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

fn registry_package_records(lock: &str) -> BTreeSet<(String, String, String)> {
    lock.split("[[package]]")
        .filter(|package| package.contains("source = \"registry+"))
        .map(|package| {
            let value = |field: &str| {
                package
                    .lines()
                    .find_map(|line| line.strip_prefix(field))
                    .and_then(|value| value.strip_prefix('"'))
                    .and_then(|value| value.strip_suffix('"'))
                    .unwrap_or_else(|| panic!("registry package omits {field:?}:\n{package}"))
                    .to_owned()
            };
            (value("name = "), value("version = "), value("checksum = "))
        })
        .collect()
}

fn rendered_diagram<'document>(document: &'document str, label: &str) -> &'document str {
    let title = format!(">{label} grammar diagram</title>");
    let title_start = document.find(&title).expect("diagram title");
    let svg_start = document[..title_start]
        .rfind("<svg")
        .expect("diagram SVG start");
    let svg_end = document[title_start..]
        .find("</svg>")
        .map(|offset| title_start + offset + "</svg>".len())
        .expect("diagram SVG end");
    &document[svg_start..svg_end]
}

fn target_dir(package: &Fixture) -> PathBuf {
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

fn only_generation_identity(package: &Fixture) -> std::ffi::OsString {
    let generations = generation_directories(&target_dir(package));
    assert_eq!(generations.len(), 1, "generations={generations:?}");
    generations
        .iter()
        .next()
        .and_then(|path| path.file_name())
        .expect("published Generation identity")
        .to_owned()
}
