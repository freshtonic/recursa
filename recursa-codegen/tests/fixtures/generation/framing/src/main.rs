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
    let complete = grammar::Entry::frame(complete_source).unwrap_or_else(|failure| {
        panic!("all islands are strict and payload is terminated: {failure:?}")
    });
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
    assert!(
        later_bounds.start() > 0,
        "the regression must use a later island"
    );
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
    assert_eq!(
        &complete_source[payload.terminator_span().range()],
        "\\.\r\n"
    );
    assert_owned_once(complete_source, complete.spans());

    // A payload interior is not tokenized by the grammar, so an unbalanced quote,
    // dollar tag or comment opener inside one must not change how the text after the
    // payload terminator lexes. This is what forces framing to restart its lex after a
    // payload rather than reuse one lex of the whole document.
    for interior in ["it's raw", "$q$ unclosed", "/* unclosed", "'"] {
        let source = format!("copy 'table';\r\n{interior}\r\n\\.\r\nput 'tail';");
        let complete = grammar::Entry::frame(&source).unwrap_or_else(|failure| {
            panic!("payload interior {interior:?} must not leak into later lexing: {failure:?}")
        });
        let sources = complete
            .typed_islands()
            .map(|parsed| parsed.source().expect("framing requests island provenance"))
            .collect::<Vec<_>>();
        assert_eq!(
            sources,
            ["copy 'table';", "put 'tail';"],
            "payload interior {interior:?} changed the islands after it",
        );
        let payload = complete
            .parts()
            .find_map(|part| match part {
                grammar::CompletePart::Payload(payload) => Some(payload),
                _ => None,
            })
            .expect("the copy island opens one payload region");
        assert_eq!(payload.data_text(), format!("\r\n{interior}\r\n"));
        assert_owned_once(&source, complete.spans());
    }

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
    assert!(
        rejection.framing().is_none(),
        "a syntax failure has no framing cause"
    );
    assert_eq!(rejection.diagnostics().len(), 1);
    assert_eq!(
        rejection.diagnostics()[0].kind(),
        recursa::explorer::ParseDiagnosticKind::UnexpectedToken,
    );
    assert_eq!(
        rejection.diagnostics()[0].span(),
        recursa::Span::new(18, 19).unwrap()
    );
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
        assert_eq!(
            rejection.diagnostics()[0].span(),
            recursa::Span::new(0, 1).unwrap()
        );
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
    assert_eq!(
        framing.kind(),
        grammar::FrameDiagnosticKind::MissingBoundary
    );
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
    assert!(matches!(
        trailing_parts[0],
        grammar::CompletePart::Island(_)
    ));
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

    frame_windows();

    println!("framing-ok");
}

/// Frames windows of one document and compares each with the frame of its text placed alone.
fn frame_windows() {
    let document = concat!(
        "put 'a';\r\n",
        "-- c ; d\r\n",
        "put ('x';'y');\r\n",
        "broken;\r\n",
        "copy 'table';\r\none;two\r\n\\.\r\n",
        "put 'caf\u{e9}';\r\n",
        "put 'tail'",
    );
    let at = |text: &str| document.find(text).expect("the document contains the text");
    let after = |text: &str| at(text) + text.len();

    // Every window on character boundaries gives the frame of its text placed alone, with
    // the spans offset to the complete document.
    let boundaries = (0..=document.len())
        .filter(|index| document.is_char_boundary(*index))
        .collect::<Vec<_>>();
    for (position, start) in boundaries.iter().copied().enumerate() {
        for end in boundaries[position..].iter().copied() {
            assert_window_is_alone(document, start..end);
        }
    }

    // A window in the middle of the document.
    let middle = after("put 'a';")..after("put ('x';'y');");
    let frame = window(document, middle.clone());
    assert_eq!(frame.source(), document);
    assert_eq!(frame.render_exact(), &document[middle.clone()]);
    assert_owned_once_in(document, middle.clone(), frame.spans());
    assert_eq!(
        frame
            .typed_islands()
            .map(|parsed| parsed.source().expect("window island provenance"))
            .collect::<Vec<_>>(),
        ["put ('x';'y');"],
    );
    let island = frame
        .typed_islands()
        .next()
        .expect("the middle window has one island");
    assert_eq!(
        island.source_bounds(),
        Some(recursa::Span::new(at("put ('x'"), middle.end).unwrap()),
    );

    // A rejected window in the middle: its spans are spans of the complete document.
    let rejected = after("put ('x';'y');")..after("broken;");
    let rejection = window_rejection(document, rejected.clone());
    assert_eq!(rejection.source(), document);
    assert_eq!(&document[rejection.island().range()], "broken;");
    assert_eq!(
        rejection.diagnostics()[0].span().start() as usize,
        at("broken;"),
        "the diagnostic names the lexical error at its position in the document",
    );

    // Windows at the start and at the end of the document.
    let start = window(document, 0..after("put 'a';"));
    assert_eq!(start.typed_islands().count(), 1);
    let end = window(document, after("put 'caf\u{e9}';")..document.len());
    assert_eq!(
        end.typed_islands()
            .map(|parsed| parsed.source().expect("final island provenance"))
            .collect::<Vec<_>>(),
        ["put 'tail'"],
    );

    // An empty window owns one empty gap.
    let empty_at = at("broken;");
    let empty = window(document, empty_at..empty_at);
    assert_eq!(empty.render_exact(), "");
    assert_eq!(
        empty.spans().collect::<Vec<_>>(),
        [recursa::Span::new(empty_at, empty_at).unwrap()],
    );

    // A window that cuts a token lexes the part of the token in the window. A quoted name
    // that the window cuts does not close, so the rejection ends at the end of the window,
    // not at the end of the document.
    let cut = at("put 'tail'")..at("put 'tail'") + "put 'ta".len();
    let rejection = window_rejection(document, cut.clone());
    assert_eq!(rejection.island().end() as usize, cut.end);
    assert!(
        rejection
            .diagnostics()
            .iter()
            .all(|diagnostic| (diagnostic.span().end() as usize) <= cut.end)
    );
    // A window that starts in a keyword lexes the rest of the keyword alone.
    let cut_keyword = at("put 'a';") + 1..after("put 'a';");
    window_rejection(document, cut_keyword);

    // A payload terminator outside the window does not close a payload in the window.
    let open_payload = at("copy 'table';")..after("one;two\r\n");
    let rejection = window_rejection(document, open_payload.clone());
    let framing = rejection.framing().expect("the payload has no terminator");
    assert_eq!(framing.code(), "RCA5001");
    assert_eq!(
        framing.span(),
        recursa::Span::new(open_payload.end, open_payload.end).unwrap(),
    );
    let closed_payload = at("copy 'table';")..after("\\.\r\n");
    let frame = window(document, closed_payload.clone());
    assert_owned_once_in(document, closed_payload, frame.spans());

    // A window that is not a range of character boundaries is a fatal failure.
    let in_character = at("\u{e9}") + 1;
    #[allow(clippy::reversed_empty_ranges)]
    for invalid in [
        3..2,
        0..document.len() + 1,
        in_character..document.len(),
        0..in_character,
    ] {
        match grammar::Entry::frame_window(document, invalid.clone()) {
            Err(grammar::FrameFailure::Fatal(fatal)) => {
                assert_eq!(fatal.code(), "RCA5090");
                assert_eq!(fatal.source(), document);
            }
            _ => panic!("window {invalid:?} must be a fatal failure"),
        }
    }
}

/// Asserts that a window frame equals the frame of the window text placed alone.
fn assert_window_is_alone(document: &str, window: std::ops::Range<usize>) {
    let offset = window.start;
    let text = &document[window.clone()];
    let framed = describe(
        &grammar::Entry::frame_window(document, window.clone()),
        offset,
    );
    let alone = describe(&grammar::Entry::frame(text), 0);
    assert_eq!(framed, alone, "window {window:?} ({text:?})");
}

/// Describes one framing result with spans relative to `offset`.
fn describe(
    result: &Result<grammar::CompleteFrame<'_, grammar::Entry<'_>>, grammar::FrameFailure<'_>>,
    offset: usize,
) -> Vec<String> {
    let relative =
        |span: recursa::Span| (span.start() as usize - offset, span.end() as usize - offset);
    match result {
        Ok(frame) => {
            let mut lines = vec![format!("frame {:?}", frame.render_exact())];
            for part in frame.parts() {
                lines.push(match part {
                    grammar::CompletePart::Gap(region) => {
                        format!("gap {:?} {:?}", relative(region.span()), region.text())
                    }
                    grammar::CompletePart::Line(region) => {
                        format!("line {:?} {:?}", relative(region.span()), region.text())
                    }
                    grammar::CompletePart::Delimited(region) => {
                        format!(
                            "delimited {:?} {:?}",
                            relative(region.span()),
                            region.text()
                        )
                    }
                    grammar::CompletePart::Payload(payload) => format!(
                        "payload {:?} {:?} {:?}",
                        relative(payload.span()),
                        relative(payload.data_span()),
                        relative(payload.terminator_span()),
                    ),
                    grammar::CompletePart::Island(island) => {
                        let parsed = island.parsed();
                        let root = recursa::HasSpan::span(&parsed.root()).map(relative);
                        let gaps = parsed
                            .root()
                            .trivia_gaps()
                            .map(|gap| relative(gap.span()))
                            .collect::<Vec<_>>();
                        format!(
                            "island {:?} {:?} {:?} root={root:?} gaps={gaps:?}",
                            relative(island.span()),
                            parsed.source(),
                            parsed.source_bounds().map(relative),
                        )
                    }
                });
            }
            lines
        }
        Err(grammar::FrameFailure::Rejected(rejection)) => {
            let mut lines = vec![format!(
                "rejected {:?} span={:?} {rejection}",
                relative(rejection.island()),
                relative(rejection.span()),
            )];
            if let Some(framing) = rejection.framing() {
                lines.push(format!(
                    "framing {} {:?} {}",
                    framing.code(),
                    relative(framing.span()),
                    framing.message(),
                ));
            }
            for diagnostic in rejection.diagnostics() {
                lines.push(format!(
                    "diagnostic {:?} {:?} {:?} {:?} {:?} {}",
                    diagnostic.kind(),
                    relative(diagnostic.span()),
                    diagnostic.text(),
                    diagnostic.found(),
                    diagnostic.expected().collect::<Vec<_>>(),
                    diagnostic.message(),
                ));
            }
            lines
        }
        Err(grammar::FrameFailure::Fatal(fatal)) => vec![format!("fatal {}", fatal.message())],
    }
}

/// Frames one window that must frame complete.
fn window(
    document: &str,
    window: std::ops::Range<usize>,
) -> grammar::CompleteFrame<'_, grammar::Entry<'_>> {
    grammar::Entry::frame_window(document, window.clone())
        .unwrap_or_else(|failure| panic!("window {window:?} must frame complete: {failure:?}"))
}

/// Frames one window that must be rejected as ordinary invalid input.
fn window_rejection(document: &str, window: std::ops::Range<usize>) -> grammar::FrameRejection<'_> {
    match grammar::Entry::frame_window(document, window.clone()) {
        Ok(_) => panic!("window {window:?} must be rejected"),
        Err(grammar::FrameFailure::Rejected(rejection)) => rejection,
        Err(grammar::FrameFailure::Fatal(fatal)) => {
            panic!("window {window:?} must reject as ordinary input, not fatally: {fatal}")
        }
    }
}

/// Asserts that ordered spans own one window of the source exactly once.
fn assert_owned_once_in(
    source: &str,
    window: std::ops::Range<usize>,
    spans: impl IntoIterator<Item = recursa::Span>,
) {
    let mut cursor = window.start;
    let mut rendered = String::new();
    for span in spans {
        assert_eq!(span.start() as usize, cursor);
        rendered.push_str(&source[span.range()]);
        cursor = span.end() as usize;
    }
    assert_eq!(cursor, window.end);
    assert_eq!(rendered, &source[window]);
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
