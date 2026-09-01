use recursa_generated_framing::grammar;

fn main() {
    let complete_source = concat!(
        "-- lead\r\n",
        "put 'a;b';\r\n",
        "-- a boundary ; inside trivia\r\n",
        "put ('nested;left';'nested right');\r\n",
        "\\meta exact\r\n",
        "copy 'table';\r\n",
        "one;two\r\n",
        "\\.\r\n",
        "$raw$opaque; 'still raw'$raw$\r\n",
        "put 'tail';",
    );
    let complete = grammar::Entry::frame(complete_source).expect("framing invariants hold");
    let grammar::FrameResult::Complete(complete) = complete else {
        let grammar::FrameResult::Recovered(recovered) = complete else {
            unreachable!()
        };
        panic!(
            "all islands are strict and payload is terminated: framing={:?}; islands={:?}",
            recovered.diagnostics(),
            recovered
                .islands()
                .map(|island| (island.span(), island.diagnostics()))
                .collect::<Vec<_>>()
        )
    };
    assert_eq!(complete.source(), complete_source);
    assert_eq!(complete.render_exact(), complete_source);
    assert_eq!(complete.typed_islands().count(), 4);
    let typed_sources = complete
        .typed_islands()
        .map(|parsed| parsed.source().expect("framing requests island provenance"))
        .collect::<Vec<_>>();
    assert_eq!(
        typed_sources,
        [
            "put 'a;b';",
            "put ('nested;left';'nested right');",
            "copy 'table';",
            "put 'tail';",
        ],
    );
    let later = complete
        .typed_islands()
        .last()
        .expect("later complete framing island");
    let later_bounds = later.source_bounds().expect("later island source window");
    assert!(later_bounds.start() > 0, "the regression must use a later island");
    let later_root = recursa::HasSpan::span(&later.root()).expect("later island root span");
    let later_local = later
        .local_source_range(later_root)
        .expect("absolute root span translates into its bounded source");
    assert_eq!(
        &later.source().expect("later island source")[later_local],
        "put 'tail'",
    );
    let island_spans = complete
        .parts()
        .filter_map(|part| match part {
            grammar::CompletePart::Island(island) => Some(island.span()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (parsed, island) in complete.typed_islands().zip(island_spans) {
        let source = parsed.source().expect("typed island source");
        assert!(parsed
            .root()
            .trivia_gaps()
            .all(|gap| gap.span().start() >= island.start() && gap.span().end() <= island.end()));
        assert!(!source.contains("put 'tail';") || source == "put 'tail';");
    }
    assert_owned_once(complete_source, complete.parts().map(|part| part.span()));

    let recovered_source = concat!(
        "put 'first';\n",
        "copy ;\n",
        "put 'not payload';\n",
        "broken;\n",
        "put 'later';",
    );
    let recovered = grammar::Entry::frame(recovered_source).expect("syntax is recoverable");
    let grammar::FrameResult::Recovered(recovered) = recovered else {
        panic!("ordinary island failures erase the whole partition")
    };
    assert_eq!(recovered.source(), recovered_source);
    assert_eq!(recovered.render_exact(), recovered_source);
    assert_eq!(
        recovered.islands().count(),
        5,
        "parts={:?}",
        recovered.parts().map(|part| part.span()).collect::<Vec<_>>()
    );
    assert!(recovered.islands().all(|island| island.root().schema().name() == "Entry"));
    assert!(recovered
        .islands()
        .all(|island| island.progress_trace().is_within_bound()));
    let replayed = recovered.islands().collect::<Vec<_>>();
    for index in [0, 2, 4] {
        assert!(
            replayed[index].diagnostics().is_empty(),
            "strictly valid erased islands must not gain a spurious diagnostic",
        );
    }
    assert_owned_once(recovered_source, recovered.parts().map(|part| part.span()));

    let bounded_missing_source = "put ('a';'b';\nput 'later';";
    let bounded_missing = grammar::Entry::frame(bounded_missing_source)
        .expect("missing island syntax is recoverable");
    let grammar::FrameResult::Recovered(bounded_missing) = bounded_missing else {
        panic!("missing island syntax cannot be complete")
    };
    let first_missing_island = bounded_missing
        .islands()
        .next()
        .expect("first recovered island");
    let missing_at = first_missing_island
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.action() {
            Some(recursa::explorer::RecoveryAction::Missing { at }) => Some(at),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        missing_at.contains(&recursa::Span::new(12, 12).unwrap()),
        "missing actions must include the first island boundary: {missing_at:?}; diagnostics={:?}",
        first_missing_island.diagnostics(),
    );

    let malformed_nested_source = "put ( ; 'b'); put 'later';";
    let malformed_nested = grammar::Entry::frame(malformed_nested_source)
        .expect("malformed nested syntax is recoverable");
    let grammar::FrameResult::Recovered(malformed_nested) = malformed_nested else {
        panic!("missing nested syntax cannot be complete")
    };
    assert_eq!(
        malformed_nested
            .islands()
            .map(|island| island.span())
            .collect::<Vec<_>>(),
        [
            recursa::Span::new(0, 13).unwrap(),
            recursa::Span::new(14, 26).unwrap(),
        ],
        "a nested separator is not a framing boundary",
    );
    assert_owned_once(
        malformed_nested_source,
        malformed_nested.parts().map(|part| part.span()),
    );

    for (source, islands) in [(";", 1), (";;", 2)] {
        let framed = grammar::Entry::frame(source).expect("empty islands are recoverable");
        let grammar::FrameResult::Recovered(framed) = framed else {
            panic!("a boundary-only island cannot be complete")
        };
        assert_eq!(framed.islands().count(), islands);
        assert!(framed
            .islands()
            .all(|island| !island.diagnostics().is_empty()));
        assert_owned_once(source, framed.parts().map(|part| part.span()));
    }

    for source in ["", " \r\n", "-- comment only", "put 'no newline';"] {
        let framed = grammar::Entry::frame(source).expect("edge input has valid ownership");
        assert_eq!(framed.render_exact(), source);
        assert_owned_once(source, framed.spans());
    }

    let unterminated = grammar::Entry::frame("copy 'x';\r\ndata")
        .expect("missing payload termination is recoverable");
    let grammar::FrameResult::Recovered(unterminated) = unterminated else {
        panic!("missing payload terminator cannot be complete")
    };
    let payload = unterminated.payloads().next().expect("payload suffix retained");
    assert_eq!(payload.data_text(), "\r\ndata");
    assert_eq!(payload.terminator_span(), None);
    assert!(unterminated
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code() == "RCA5001"));

    for source in ["copy 'x';\nbody\n\\.\n", "copy 'x';\r\nbody\r\n\\.\r\n"] {
        let framed = grammar::Entry::frame(source).expect("physical-line terminator is valid");
        assert!(matches!(framed, grammar::FrameResult::Complete(_)));
    }
    for source in ["copy 'x';\\.\n", "copy 'x';\nbody\n\\."] {
        let framed = grammar::Entry::frame(source).expect("invalid terminator is recoverable");
        let grammar::FrameResult::Recovered(framed) = framed else {
            panic!("same-line or missing-newline terminator cannot be complete")
        };
        assert!(framed
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "RCA5001"));
    }

    let ambiguous_source = "put 'one' put 'two'";
    let ambiguous = grammar::Entry::frame(ambiguous_source)
        .expect("ambiguous ownership is recoverable and fail-closed");
    let grammar::FrameResult::Recovered(ambiguous) = ambiguous else {
        panic!("a missing island boundary cannot be complete")
    };
    assert_eq!(ambiguous.render_exact(), ambiguous_source);
    let ambiguous_diagnostics = ambiguous.diagnostics();
    assert_eq!(ambiguous_diagnostics.len(), 1);
    assert_eq!(ambiguous_diagnostics[0].code(), "RCA5002");
    assert_eq!(
        ambiguous_diagnostics[0].span(),
        recursa::Span::new(10, 13).unwrap(),
        "the strict restart-token failure belongs to the frame, not the shortened island",
    );
    assert!(
        ambiguous_diagnostics[0]
            .message()
            .contains("unexpected fixed:put"),
        "the frame must retain the strict restart-token diagnostic: {}",
        ambiguous_diagnostics[0].message(),
    );
    let strict_failure = match grammar::Entry::explorer().parse(ambiguous_source) {
        Ok(_) => panic!("the same boundary-less source must fail strict Explorer parsing"),
        Err(failure) => failure,
    };
    let [strict_diagnostic] = strict_failure.diagnostics() else {
        panic!("strict parsing must retain exactly one diagnostic")
    };
    assert_eq!(
        strict_diagnostic.kind(),
        recursa::explorer::ParseDiagnosticKind::UnexpectedToken,
    );
    assert_eq!(strict_diagnostic.span(), ambiguous_diagnostics[0].span());
    assert!(
        ambiguous_diagnostics[0]
            .message()
            .starts_with(strict_diagnostic.message()),
        "framing must preserve the shared strict diagnostic conversion",
    );
    let ambiguous_islands = ambiguous.islands().collect::<Vec<_>>();
    assert_eq!(ambiguous_islands.len(), 1);
    assert_eq!(
        ambiguous_islands[0].span(),
        recursa::Span::new(0, 9).unwrap()
    );
    assert!(ambiguous_islands[0].diagnostics().is_empty());
    for diagnostic in ambiguous_islands[0].diagnostics() {
        assert!(diagnostic.span().start() >= ambiguous_islands[0].span().start());
        assert!(diagnostic.span().end() <= ambiguous_islands[0].span().end());
        match diagnostic.action() {
            Some(recursa::explorer::RecoveryAction::Missing { at }) => {
                assert!(at.start() >= ambiguous_islands[0].span().start());
                assert!(at.end() <= ambiguous_islands[0].span().end());
            }
            Some(recursa::explorer::RecoveryAction::Skipped { span }) => {
                assert!(span.start() >= ambiguous_islands[0].span().start());
                assert!(span.end() <= ambiguous_islands[0].span().end());
            }
            None => {}
        }
    }
    let unresolved = ambiguous
        .parts()
        .find_map(|part| match part {
            grammar::RecoveredPart::Unresolved(region) => Some(*region),
            _ => None,
        })
        .expect("unassignable EOF suffix is retained explicitly");
    assert_eq!(unresolved.text(), " put 'two'");
    assert_eq!(unresolved.span(), recursa::Span::new(9, 19).unwrap());
    assert_owned_once(ambiguous_source, ambiguous.parts().map(|part| part.span()));

    let unrelated_source = "put 'one' 'unrelated'";
    let unrelated = grammar::Entry::frame(unrelated_source)
        .expect("unrelated trailing input is recoverable without guessing a restart");
    let grammar::FrameResult::Recovered(unrelated) = unrelated else {
        panic!("a missing island boundary cannot be complete")
    };
    assert_eq!(unrelated.islands().count(), 1);
    assert_eq!(
        unrelated.islands().next().unwrap().span(),
        recursa::Span::new(0, unrelated_source.len()).unwrap(),
    );
    assert!(!unrelated
        .parts()
        .any(|part| matches!(part, grammar::RecoveredPart::Unresolved(_))));
    assert_owned_once(unrelated_source, unrelated.parts().map(|part| part.span()));

    let trailing_trivia_source = "put 'one' \r\n-- trailing";
    let trailing_trivia = grammar::Entry::frame(trailing_trivia_source)
        .expect("trailing trivia has deterministic EOF-gap ownership");
    let grammar::FrameResult::Recovered(trailing_trivia) = trailing_trivia else {
        panic!("a missing island boundary cannot be complete")
    };
    let trailing_parts = trailing_trivia.parts().collect::<Vec<_>>();
    assert_eq!(trailing_parts.len(), 2);
    assert!(matches!(trailing_parts[0], grammar::RecoveredPart::Island(_)));
    let grammar::RecoveredPart::Gap(gap) = trailing_parts[1] else {
        panic!("pure trailing trivia must be the right EOF gap")
    };
    assert_eq!(gap.span(), recursa::Span::new(9, 23).unwrap());
    assert_eq!(gap.text(), " \r\n-- trailing");
    assert!(!trailing_parts
        .iter()
        .any(|part| matches!(part, grammar::RecoveredPart::Unresolved(_))));
    assert_owned_once(
        trailing_trivia_source,
        trailing_trivia.parts().map(|part| part.span()),
    );

    let shared_base_source = "put 'lead';\r\nALPHA BETA GAMMA;";
    let shared_base = grammar::Entry::frame(shared_base_source)
        .expect("shared-base-kind framing invariants hold");
    let grammar::FrameResult::Complete(shared_base) = shared_base else {
        panic!("semantic targets sharing a base kind must frame strictly")
    };
    assert_eq!(shared_base.render_exact(), shared_base_source);
    assert_eq!(
        shared_base
            .typed_islands()
            .map(|parsed| parsed.source().expect("shared-base island provenance"))
            .collect::<Vec<_>>(),
        ["put 'lead';", "ALPHA BETA GAMMA;"],
    );
    assert_owned_once(
        shared_base_source,
        shared_base.parts().map(|part| part.span()),
    );

    println!("framing-ok");
}

fn assert_owned_once(source: &str, spans: impl IntoIterator<Item = recursa::Span>) {
    let mut cursor = 0_usize;
    let mut rendered = String::new();
    for span in spans {
        assert_eq!(span.start() as usize, cursor);
        rendered.push_str(&source[span.range()]);
        cursor = span.end() as usize;
    }
    assert_eq!(cursor, source.len());
    assert_eq!(rendered, source);
}
