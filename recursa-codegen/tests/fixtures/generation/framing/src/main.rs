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
    let complete = grammar::Entry::frame(complete_source)
        .unwrap_or_else(|failure| panic!("all islands are strict and payload is terminated: {failure:?}"));
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
    let payload = complete
        .parts()
        .find_map(|part| match part {
            grammar::CompletePart::Payload(payload) => Some(payload),
            _ => None,
        })
        .expect("the copy island opens one payload region");
    assert_eq!(payload.data_text(), "\r\none;two\r\n");
    assert_eq!(&complete_source[payload.terminator_span().range()], "\\.\r\n");
    assert_owned_once(complete_source, complete.spans());

    // The first failing island rejects the whole document with its own strict
    // diagnostic; later valid islands are never parsed into a partial result.
    let rejected_source = concat!(
        "put 'first';\n",
        "copy ;\n",
        "put 'not payload';\n",
        "broken;\n",
        "put 'later';",
    );
    let rejection = reject(rejected_source);
    assert_eq!(rejection.source(), rejected_source);
    assert_eq!(
        &rejected_source[rejection.island().range()],
        "copy ;",
        "the rejected extent is the first failing statement-sized candidate",
    );
    assert!(rejection.framing().is_none(), "a syntax failure has no framing cause");
    assert_eq!(rejection.diagnostics().len(), 1);
    assert_eq!(
        rejection.diagnostics()[0].kind(),
        recursa::explorer::ParseDiagnosticKind::UnexpectedToken,
    );
    assert_eq!(rejection.diagnostics()[0].span(), recursa::Span::new(18, 19).unwrap());
    assert_eq!(rejection.span(), recursa::Span::new(18, 19).unwrap());
    assert_eq!(rejection.to_string(), rejection.diagnostics()[0].message());

    // A boundary spelling consumed by nested syntax never becomes a framing
    // boundary, so the rejected extent ends at the first candidate at or after
    // the strict failure.
    let bounded_missing_source = "put ('a';'b';\nput 'later';";
    let bounded_missing = reject(bounded_missing_source);
    assert_eq!(
        &bounded_missing_source[bounded_missing.island().range()],
        "put ('a';'b';",
    );
    assert_eq!(
        bounded_missing.diagnostics()[0].span(),
        recursa::Span::new(12, 13).unwrap(),
        "the strict failure is the boundary spelling that closes no group",
    );

    let malformed_nested_source = "put ( ; 'b'); put 'later';";
    let malformed_nested = reject(malformed_nested_source);
    assert_eq!(
        &malformed_nested_source[malformed_nested.island().range()],
        "put ( ;",
        "a nested separator is not a framing boundary",
    );
    assert_eq!(
        malformed_nested.diagnostics()[0].span(),
        recursa::Span::new(6, 7).unwrap(),
    );

    for source in [";", ";;"] {
        let rejection = reject(source);
        assert_eq!(rejection.island(), recursa::Span::new(0, 1).unwrap());
        assert_eq!(rejection.diagnostics()[0].span(), recursa::Span::new(0, 1).unwrap());
        assert!(rejection.framing().is_none());
    }

    for source in ["", " \r\n", "-- comment only", "put 'no newline';"] {
        let framed = grammar::Entry::frame(source).expect("edge input has valid ownership");
        assert_eq!(framed.render_exact(), source);
        assert_owned_once(source, framed.spans());
    }

    let unterminated_source = "copy 'x';\r\ndata";
    let unterminated = reject(unterminated_source);
    assert!(unterminated.diagnostics().is_empty());
    let framing = unterminated
        .framing()
        .expect("a missing payload terminator names its framing cause");
    assert_eq!(framing.code(), "RCA5001");
    assert_eq!(
        framing.kind(),
        grammar::FrameDiagnosticKind::MissingPayloadTerminator
    );
    assert_eq!(
        &unterminated_source[unterminated.island().range()],
        "\r\ndata",
        "the rejected extent is the unterminated payload suffix",
    );
    assert_eq!(
        framing.span(),
        recursa::Span::new(unterminated_source.len(), unterminated_source.len()).unwrap(),
    );
    assert!(unterminated.to_string().contains("\\\\."));

    for source in ["copy 'x';\nbody\n\\.\n", "copy 'x';\r\nbody\r\n\\.\r\n"] {
        let framed = grammar::Entry::frame(source).expect("physical-line terminator is valid");
        assert_eq!(framed.typed_islands().count(), 1);
        assert_owned_once(source, framed.spans());
    }
    for source in ["copy 'x';\\.\n", "copy 'x';\nbody\n\\."] {
        let rejection = reject(source);
        assert_eq!(
            rejection.framing().map(grammar::FrameDiagnostic::code),
            Some("RCA5001"),
            "same-line or missing-newline terminator cannot complete the payload",
        );
    }

    let ambiguous_source = "put 'one' put 'two'";
    let ambiguous = reject(ambiguous_source);
    assert_eq!(ambiguous.island(), recursa::Span::new(0, 19).unwrap());
    let framing = ambiguous
        .framing()
        .expect("a missing island boundary names its framing cause");
    assert_eq!(framing.code(), "RCA5002");
    assert_eq!(framing.kind(), grammar::FrameDiagnosticKind::MissingBoundary);
    assert_eq!(
        framing.span(),
        recursa::Span::new(10, 13).unwrap(),
        "the leftover restart token is the missing-boundary location",
    );
    assert_eq!(ambiguous.span(), framing.span());
    let [leftover] = ambiguous.diagnostics() else {
        panic!("a missing boundary retains exactly one strict diagnostic")
    };
    assert_eq!(leftover.span(), recursa::Span::new(10, 13).unwrap());
    assert!(
        leftover.message().contains("fixed:put"),
        "framing must retain the strict trailing-token diagnostic: {}",
        leftover.message(),
    );
    let strict_failure = match grammar::Entry::explorer().parse(ambiguous_source) {
        Ok(_) => panic!("the same boundary-less source must fail strict Explorer parsing"),
        Err(failure) => failure,
    };
    assert_eq!(
        strict_failure.diagnostics(),
        ambiguous.diagnostics(),
        "framing and Explorer share one strict diagnostic conversion",
    );

    let unrelated_source = "put 'one' 'unrelated'";
    let unrelated = reject(unrelated_source);
    assert_eq!(
        unrelated.island(),
        recursa::Span::new(0, unrelated_source.len()).unwrap(),
    );
    assert_eq!(
        unrelated.framing().map(grammar::FrameDiagnostic::code),
        Some("RCA5002"),
    );
    assert_eq!(
        unrelated.diagnostics()[0].span(),
        recursa::Span::new(10, 21).unwrap(),
    );

    // A final island that parses strictly to end of input without its
    // boundary is complete and keeps its authored value.
    let final_island_source = "put 'one'";
    let final_island = grammar::Entry::frame(final_island_source)
        .expect("a strict boundary-less final island frames complete");
    assert_eq!(final_island.render_exact(), final_island_source);
    let final_typed = final_island.typed_islands().collect::<Vec<_>>();
    assert_eq!(final_typed.len(), 1);
    assert_eq!(
        final_typed[0].source().expect("final island provenance"),
        "put 'one'",
    );
    assert_owned_once(final_island_source, final_island.spans());

    // Earlier bounded islands and a strict boundary-less final island agree.
    let mixed_final_source = "put 'a';\r\nput 'b'";
    let mixed_final = grammar::Entry::frame(mixed_final_source)
        .expect("a mixed document with a strict final island frames complete");
    assert_eq!(mixed_final.render_exact(), mixed_final_source);
    assert_eq!(
        mixed_final
            .typed_islands()
            .map(|parsed| parsed.source().expect("mixed final island provenance"))
            .collect::<Vec<_>>(),
        ["put 'a';", "put 'b'"],
    );
    assert_owned_once(mixed_final_source, mixed_final.spans());

    // An earlier failure rejects the document before a strict final island.
    let failed_then_final_source = "broken;\nput 'later'";
    let failed_then_final = reject(failed_then_final_source);
    assert_eq!(
        &failed_then_final_source[failed_then_final.island().range()],
        "broken;",
    );
    assert!(!failed_then_final.diagnostics().is_empty());
    assert!(failed_then_final.framing().is_none());

    let trailing_trivia_source = "put 'one' \r\n-- trailing";
    let trailing_trivia = grammar::Entry::frame(trailing_trivia_source)
        .expect("trailing trivia after a strict final island frames complete");
    assert_eq!(trailing_trivia.render_exact(), trailing_trivia_source);
    let trailing_parts = trailing_trivia.parts().collect::<Vec<_>>();
    assert_eq!(trailing_parts.len(), 2);
    assert!(matches!(trailing_parts[0], grammar::CompletePart::Island(_)));
    let grammar::CompletePart::Gap(gap) = trailing_parts[1] else {
        panic!("pure trailing trivia must be the right EOF gap")
    };
    assert_eq!(gap.span(), recursa::Span::new(9, 23).unwrap());
    assert_eq!(gap.text(), " \r\n-- trailing");
    assert_eq!(trailing_trivia.typed_islands().count(), 1);
    assert_owned_once(trailing_trivia_source, trailing_trivia.spans());

    let shared_base_source = "put 'lead';\r\nALPHA BETA GAMMA;";
    let shared_base = grammar::Entry::frame(shared_base_source)
        .expect("semantic targets sharing a base kind frame strictly");
    assert_eq!(shared_base.render_exact(), shared_base_source);
    assert_eq!(
        shared_base
            .typed_islands()
            .map(|parsed| parsed.source().expect("shared-base island provenance"))
            .collect::<Vec<_>>(),
        ["put 'lead';", "ALPHA BETA GAMMA;"],
    );
    assert_owned_once(shared_base_source, shared_base.spans());

    println!("framing-ok");
}

/// Frames one document that must be rejected as ordinary invalid input.
fn reject(source: &str) -> grammar::FrameRejection<'_> {
    match grammar::Entry::frame(source) {
        Ok(_) => panic!("{source:?} must be rejected"),
        Err(grammar::FrameFailure::Rejected(rejection)) => rejection,
        Err(grammar::FrameFailure::Fatal(fatal)) => {
            panic!("{source:?} must reject as ordinary input, not fatally: {fatal}")
        }
    }
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
