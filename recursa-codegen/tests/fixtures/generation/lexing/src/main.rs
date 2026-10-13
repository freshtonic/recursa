use recursa::NodeView as _;
use recursa_generated_lexing::grammar;

fn assert_token_kind_value<T: Copy + Eq + std::hash::Hash + std::fmt::Debug>() {}

fn assert_content_token_value<T>()
where
    T: Clone + std::fmt::Debug + PartialEq + Eq + PartialOrd + Ord + std::hash::Hash,
{
}

fn downstream_error_code(code: grammar::LexErrorCode) -> &'static str {
    match code {
        grammar::LexErrorCode::Unrecognized => "RCA4000",
        grammar::LexErrorCode::UnterminatedSameDelimiter => "RCA4001",
        grammar::LexErrorCode::UnterminatedNested => "RCA4002",
        grammar::LexErrorCode::ExcludedFollowingCharacter => "RCA4004",
        _ => "future lexical error",
    }
}

fn main() {
    assert_token_kind_value::<grammar::TokenKind>();
    assert_content_token_value::<grammar::WordText<'static>>();

    let source = String::from("MiXeD sElEcT @");
    let lexed: grammar::LexResult<'_> = grammar::lex(&source);
    assert_eq!(lexed.source(), source);

    let tokens = lexed.tokens().collect::<Vec<grammar::Token<'_>>>();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind(), grammar::TokenKind::WordText);
    assert_eq!(tokens[0].span(), recursa::Span::new(0, 5).unwrap());
    assert_eq!(tokens[0].text(), "MiXeD");
    assert_eq!(tokens[1].kind(), grammar::TokenKind::SELECT);
    assert_eq!(tokens[1].span(), recursa::Span::new(6, 12).unwrap());
    assert_eq!(tokens[1].text(), "sElEcT");
    assert_eq!(tokens[1].to_string(), "sElEcT");
    assert_eq!(grammar::TokenKind::SELECT.to_string(), "SELECT");
    assert_eq!(grammar::TokenKind::COLON.to_string(), ":");
    assert_eq!(grammar::TokenKind::WordText.to_string(), "WordText");

    let errors = lexed.errors().collect::<Vec<grammar::LexError<'_>>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind(), grammar::TokenKind::LexicalError);
    assert_eq!(errors[0].span(), recursa::Span::new(13, 14).unwrap());
    assert_eq!(errors[0].text(), "@");
    assert_eq!(errors[0].code(), "RCA4000");
    assert_eq!(
        downstream_error_code(errors[0].diagnostic_code()),
        "RCA4000"
    );
    assert_eq!(errors[0].anchor(), recursa::Span::new(13, 14).unwrap());

    let retained_source = " /* lead */\nselect -- one\n\t/* two */ alpha\r\n";
    let retained = grammar::lex(retained_source);
    let gaps = retained.trivia_gaps().collect::<Vec<_>>();
    assert_eq!(gaps.len(), 3);
    assert_eq!(gaps[0].text(), " /* lead */\n");
    assert_eq!(gaps[0].span(), recursa::Span::new(0, 12).unwrap());
    assert_eq!(
        gaps[0]
            .trivia()
            .map(|trivia| (trivia.kind(), trivia.text(), trivia.span()))
            .collect::<Vec<_>>(),
        vec![
            (
                grammar::TriviaKind::Whitespace,
                " ",
                recursa::Span::new(0, 1).unwrap(),
            ),
            (
                grammar::TriviaKind::BlockComment,
                "/* lead */",
                recursa::Span::new(1, 11).unwrap(),
            ),
            (
                grammar::TriviaKind::Whitespace,
                "\n",
                recursa::Span::new(11, 12).unwrap(),
            ),
        ],
    );
    assert_eq!(gaps[1].text(), " -- one\n\t/* two */ ");
    assert_eq!(gaps[2].text(), "\r\n");
    let retained_tokens = retained.tokens().collect::<Vec<_>>();
    let mut reconstructed = String::new();
    for (index, gap) in gaps.iter().enumerate() {
        reconstructed.push_str(gap.text());
        if let Some(token) = retained_tokens.get(index) {
            reconstructed.push_str(token.text());
        }
    }
    assert_eq!(reconstructed, retained_source);

    let mut retained_input = retained.input();
    let parsed = grammar::Selection::parse(&mut retained_input)
        .expect("significant parsing ignores retained trivia");
    assert!(retained_input.is_eof());
    assert_eq!(
        parsed
            .root()
            .trivia_gaps()
            .map(|gap| gap.text())
            .collect::<Vec<_>>(),
        vec![" /* lead */\n", " -- one\n\t/* two */ ", "\r\n"],
    );

    // Comment-only input: with no significant records the beginning and end
    // gaps coincide in one source gap that covers the complete source, and its
    // ordered classified-trivia records tile that gap without overlap or loss.
    let comments_only_source = "-- first\n/* second */";
    let comments_only = grammar::lex(comments_only_source);
    assert_eq!(comments_only.tokens().count(), 0);
    assert_eq!(comments_only.errors().count(), 0);
    let comments_only_gaps = comments_only.trivia_gaps().collect::<Vec<_>>();
    assert_eq!(comments_only_gaps.len(), 1);
    assert_eq!(comments_only_gaps[0].text(), comments_only_source);
    assert_eq!(
        comments_only_gaps[0].span(),
        recursa::Span::new(0, comments_only_source.len()).unwrap(),
    );
    assert_eq!(
        comments_only_gaps[0]
            .trivia()
            .map(|trivia| (trivia.kind(), trivia.text(), trivia.span()))
            .collect::<Vec<_>>(),
        vec![
            (
                grammar::TriviaKind::LineComment,
                "-- first",
                recursa::Span::new(0, 8).unwrap(),
            ),
            (
                grammar::TriviaKind::Whitespace,
                "\n",
                recursa::Span::new(8, 9).unwrap(),
            ),
            (
                grammar::TriviaKind::BlockComment,
                "/* second */",
                recursa::Span::new(9, 21).unwrap(),
            ),
        ],
    );
    assert_eq!(
        comments_only_gaps[0]
            .trivia()
            .map(|trivia| trivia.text())
            .collect::<String>(),
        comments_only_source,
    );
    let empty = grammar::lex("");
    let empty_gaps = empty.trivia_gaps().collect::<Vec<_>>();
    assert_eq!(empty_gaps.len(), 1);
    assert_eq!(empty_gaps[0].span(), recursa::Span::new(0, 0).unwrap());
    assert_eq!(empty_gaps[0].text(), "");

    let alternative_trivia_source = " select -- tail";
    let mut alternative_trivia = grammar::LexBuilder::new(alternative_trivia_source);
    alternative_trivia
        .append_trivia(
            grammar::TriviaKind::Whitespace,
            recursa::Span::new(0, 1).unwrap(),
        )
        .unwrap()
        .append(
            grammar::TokenKind::WordText,
            recursa::Span::new(1, 7).unwrap(),
        )
        .unwrap()
        .append_trivia(
            grammar::TriviaKind::Whitespace,
            recursa::Span::new(7, 8).unwrap(),
        )
        .unwrap()
        .append_trivia(
            grammar::TriviaKind::LineComment,
            recursa::Span::new(8, 15).unwrap(),
        )
        .unwrap();
    let alternative_trivia = alternative_trivia.finish();
    assert_eq!(
        alternative_trivia
            .tokens()
            .map(|token| (token.kind(), token.text(), token.span()))
            .collect::<Vec<_>>(),
        vec![(
            grammar::TokenKind::SELECT,
            "select",
            recursa::Span::new(1, 7).unwrap(),
        )],
    );
    assert_eq!(
        alternative_trivia
            .trivia_gaps()
            .map(|gap| {
                (
                    gap.text(),
                    gap.trivia().map(|trivia| trivia.kind()).collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (" ", vec![grammar::TriviaKind::Whitespace]),
            (
                " -- tail",
                vec![
                    grammar::TriviaKind::Whitespace,
                    grammar::TriviaKind::LineComment,
                ],
            ),
        ],
    );

    // Alternative-lexer parity: the same source observed through generated
    // lexing and through an alternative lexer exposes identical significant
    // records and identical classified-trivia source gaps.
    let generated_parity = grammar::lex(alternative_trivia_source);
    assert_eq!(
        generated_parity
            .tokens()
            .map(|token| (token.kind(), token.text(), token.span()))
            .collect::<Vec<_>>(),
        alternative_trivia
            .tokens()
            .map(|token| (token.kind(), token.text(), token.span()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        generated_parity
            .trivia_gaps()
            .map(|gap| {
                (
                    gap.span(),
                    gap.text(),
                    gap.trivia()
                        .map(|trivia| (trivia.kind(), trivia.text(), trivia.span()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
        alternative_trivia
            .trivia_gaps()
            .map(|gap| {
                (
                    gap.span(),
                    gap.text(),
                    gap.trivia()
                        .map(|trivia| (trivia.kind(), trivia.text(), trivia.span()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
    );

    // Alternative-lexer comment-only parity: appending only classified trivia
    // still exposes the coinciding beginning and end gap that generated lexing
    // exposes for the same trivia-only source.
    let mut alternative_comments_only = grammar::LexBuilder::new(comments_only_source);
    alternative_comments_only
        .append_trivia(
            grammar::TriviaKind::LineComment,
            recursa::Span::new(0, 8).unwrap(),
        )
        .unwrap()
        .append_trivia(
            grammar::TriviaKind::Whitespace,
            recursa::Span::new(8, 9).unwrap(),
        )
        .unwrap()
        .append_trivia(
            grammar::TriviaKind::BlockComment,
            recursa::Span::new(9, 21).unwrap(),
        )
        .unwrap();
    let alternative_comments_only = alternative_comments_only.finish();
    assert_eq!(alternative_comments_only.tokens().count(), 0);
    assert_eq!(
        alternative_comments_only
            .trivia_gaps()
            .map(|gap| {
                (
                    gap.span(),
                    gap.text(),
                    gap.trivia()
                        .map(|trivia| (trivia.kind(), trivia.text(), trivia.span()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
        comments_only
            .trivia_gaps()
            .map(|gap| {
                (
                    gap.span(),
                    gap.text(),
                    gap.trivia()
                        .map(|trivia| (trivia.kind(), trivia.text(), trivia.span()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
    );

    let mut alternative_overlap = grammar::LexBuilder::new(" ab");
    alternative_overlap
        .append_trivia(
            grammar::TriviaKind::Whitespace,
            recursa::Span::new(0, 1).unwrap(),
        )
        .unwrap();
    assert!(
        alternative_overlap
            .append(
                grammar::TokenKind::WordText,
                recursa::Span::new(0, 2).unwrap()
            )
            .is_err()
    );
    let mut alternative_invalid_utf8 = grammar::LexBuilder::new("é");
    assert!(
        alternative_invalid_utf8
            .append_trivia(
                grammar::TriviaKind::Whitespace,
                recursa::Span::new(1, 2).unwrap(),
            )
            .is_err()
    );

    let rejected_after_trivia = grammar::lex(" /* retained */ @");
    let rejected = rejected_after_trivia.errors().next().unwrap();
    assert_eq!(rejected.text(), "@");
    assert_eq!(rejected.code(), "RCA4000");
    assert_eq!(
        rejected_after_trivia
            .trivia_gaps()
            .flat_map(|gap| gap.trivia())
            .map(|trivia| trivia.text())
            .collect::<Vec<_>>(),
        vec![" ", "/* retained */", " "],
    );

    let alternative_source = String::from("sElEcT");
    let mut alternative = grammar::LexBuilder::new(&alternative_source);
    alternative
        .append(
            grammar::TokenKind::WordText,
            recursa::Span::new(0, 6).unwrap(),
        )
        .expect("a source-bounded typed token can be appended");
    let alternative = alternative.finish();
    let classified = alternative
        .tokens()
        .next()
        .expect("the alternative lexer emitted one token");
    assert_eq!(classified.kind(), grammar::TokenKind::SELECT);
    assert_eq!(classified.text(), "sElEcT");
    assert_eq!(alternative.errors().count(), 0);

    let mut standalone_keyword = grammar::LexBuilder::new("Kk");
    standalone_keyword
        .append(
            grammar::TokenKind::WordText,
            recursa::Span::new(0, 2).unwrap(),
        )
        .expect("an alternative lexer may initially report a content kind");
    let standalone_keyword = standalone_keyword.finish();
    assert_eq!(
        standalone_keyword.tokens().next().unwrap().kind(),
        grammar::TokenKind::CUSTOM_KEYWORD,
    );

    let fixed_over_content = grammar::lex("Kk");
    let fixed_over_content = fixed_over_content.tokens().collect::<Vec<_>>();
    assert_eq!(fixed_over_content.len(), 1);
    assert_eq!(
        fixed_over_content[0].kind(),
        grammar::TokenKind::CUSTOM_KEYWORD,
    );

    let mut invalid = grammar::LexBuilder::new("abcd");
    invalid
        .append(
            grammar::TokenKind::WordText,
            recursa::Span::new(2, 4).unwrap(),
        )
        .unwrap();
    assert!(
        invalid
            .append(
                grammar::TokenKind::WordText,
                recursa::Span::new(0, 1).unwrap(),
            )
            .is_err()
    );
    let mut invalid_utf8 = grammar::LexBuilder::new("é");
    assert!(
        invalid_utf8
            .append(
                grammar::TokenKind::WordText,
                recursa::Span::new(1, 2).unwrap(),
            )
            .is_err()
    );
    let mut alternative_error = grammar::LexBuilder::new("oops");
    alternative_error
        .append(
            grammar::TokenKind::LexicalError,
            recursa::Span::new(0, 4).unwrap(),
        )
        .unwrap();
    let alternative_error = alternative_error.finish().errors().next().unwrap();
    assert_eq!(alternative_error.code(), "RCA4000");
    assert_eq!(
        alternative_error.anchor(),
        recursa::Span::new(0, 4).unwrap()
    );

    let gap_source = "sElEcT unscanned ?";
    let error_start = gap_source.rfind('?').unwrap();
    let mut sparse = grammar::LexBuilder::new(gap_source);
    sparse
        .append(
            grammar::TokenKind::WordText,
            recursa::Span::new(0, 6).unwrap(),
        )
        .unwrap();
    sparse
        .append(
            grammar::TokenKind::LexicalError,
            recursa::Span::new(error_start, error_start + 1).unwrap(),
        )
        .unwrap();
    let sparse = sparse.finish();
    let sparse_tokens = sparse.tokens().collect::<Vec<_>>();
    assert_eq!(sparse_tokens.len(), 1);
    assert_eq!(sparse_tokens[0].kind(), grammar::TokenKind::SELECT);
    assert_eq!(sparse_tokens[0].span(), recursa::Span::new(0, 6).unwrap());
    assert_eq!(sparse_tokens[0].text(), "sElEcT");
    let sparse_errors = sparse.errors().collect::<Vec<_>>();
    assert_eq!(sparse_errors.len(), 1);
    assert_eq!(sparse_errors[0].kind(), grammar::TokenKind::LexicalError);
    assert_eq!(
        sparse_errors[0].span(),
        recursa::Span::new(error_start, error_start + 1).unwrap()
    );
    assert_eq!(sparse_errors[0].text(), "?");
    assert_eq!(sparse_errors[0].code(), "RCA4000");
    let sparse_gaps = sparse.trivia_gaps().collect::<Vec<_>>();
    assert_eq!(
        sparse_gaps.iter().map(|gap| gap.text()).collect::<Vec<_>>(),
        vec!["", " unscanned ", ""],
    );
    assert!(sparse_gaps.iter().all(|gap| gap.trivia().len() == 0));

    let dollar = grammar::lex("$tag$body$tag$");
    assert_eq!(dollar.errors().count(), 0);
    let dollar = dollar.tokens().next().unwrap();
    assert_eq!(dollar.kind(), grammar::TokenKind::DollarRegion);
    assert_eq!(dollar.text(), "$tag$body$tag$");
    assert_eq!(dollar.span(), recursa::Span::new(0, 14).unwrap());

    let nested = grammar::lex("{-outer {-inner-} tail-}");
    assert_eq!(nested.errors().count(), 0);
    let nested = nested.tokens().next().unwrap();
    assert_eq!(nested.kind(), grammar::TokenKind::NestedRegion);
    assert_eq!(nested.text(), "{-outer {-inner-} tail-}");
    assert_eq!(nested.span(), recursa::Span::new(0, 24).unwrap());

    let ignored_nested = grammar::lex("/* outer /* inner */ tail */");
    assert_eq!(ignored_nested.tokens().count(), 0);
    assert_eq!(ignored_nested.errors().count(), 0);

    let bounded = grammar::lex("123 ");
    assert_eq!(bounded.errors().count(), 0);
    let bounded = bounded.tokens().next().unwrap();
    assert_eq!(bounded.kind(), grammar::TokenKind::BoundedNumber);
    assert_eq!(bounded.text(), "123");
    assert_eq!(bounded.span(), recursa::Span::new(0, 3).unwrap());

    let excluded = grammar::lex("123a");
    let excluded_error = excluded.errors().next().unwrap();
    assert_eq!(excluded_error.code(), "RCA4004");
    assert_eq!(
        excluded_error.diagnostic_code(),
        grammar::LexErrorCode::ExcludedFollowingCharacter
    );
    assert_eq!(excluded_error.text(), "123");
    assert_eq!(excluded_error.span(), recursa::Span::new(0, 3).unwrap());
    assert_eq!(excluded_error.anchor(), recursa::Span::new(0, 3).unwrap());
    let excluded_tail = excluded.tokens().next().unwrap();
    assert_eq!(excluded_tail.text(), "a");
    assert_eq!(excluded_tail.span(), recursa::Span::new(3, 4).unwrap());

    let fixed_operator = grammar::lex("!=");
    assert_eq!(
        fixed_operator.tokens().next().unwrap().kind(),
        grammar::TokenKind::NOT_EQUAL
    );
    let qualified_operator = grammar::lex("!=+");
    let qualified_operator = qualified_operator.tokens().next().unwrap();
    assert_eq!(qualified_operator.kind(), grammar::TokenKind::OperatorText);
    assert_eq!(qualified_operator.text(), "!=+");
    assert_eq!(qualified_operator.span(), recursa::Span::new(0, 3).unwrap());
    let stripped_operators = grammar::lex("++").tokens().collect::<Vec<_>>();
    assert_eq!(stripped_operators.len(), 2);
    assert!(
        stripped_operators
            .iter()
            .all(|token| token.kind() == grammar::TokenKind::OperatorText && token.text() == "+")
    );
    for fenced_source in ["+-- ignored\n", "+/* ignored */"] {
        let fenced = grammar::lex(fenced_source);
        assert_eq!(fenced.errors().count(), 0);
        let fenced = fenced.tokens().collect::<Vec<_>>();
        assert_eq!(fenced.len(), 1);
        assert_eq!(fenced[0].kind(), grammar::TokenKind::OperatorText);
        assert_eq!(fenced[0].text(), "+");
        assert_eq!(fenced[0].span(), recursa::Span::new(0, 1).unwrap());
    }

    let line = grammar::lex("\\set value\r\nx");
    let line_tokens = line.tokens().collect::<Vec<_>>();
    assert_eq!(line_tokens[0].kind(), grammar::TokenKind::PsqlLine);
    assert_eq!(line_tokens[0].text(), "\\set value\r\n");
    assert_eq!(line_tokens[0].span(), recursa::Span::new(0, 12).unwrap());
    assert_eq!(line_tokens[1].text(), "x");
    assert_eq!(line_tokens[1].span(), recursa::Span::new(12, 13).unwrap());
    let eof_line = grammar::lex("\\echo value");
    let eof_line = eof_line.tokens().next().unwrap();
    assert_eq!(eof_line.kind(), grammar::TokenKind::PsqlLine);
    assert_eq!(eof_line.span(), recursa::Span::new(0, 11).unwrap());

    let priority = grammar::lex("§line\n");
    let priority = priority.tokens().next().unwrap();
    assert_eq!(priority.kind(), grammar::TokenKind::PriorityHigh);
    assert_eq!(priority.text(), "§line\n");
    assert_eq!(priority.span(), recursa::Span::new(0, 7).unwrap());
    let priority_reversed = grammar::lex("¤line\n");
    let priority_reversed = priority_reversed.tokens().next().unwrap();
    assert_eq!(
        priority_reversed.kind(),
        grammar::TokenKind::PriorityHighFirst,
        "the same priority pair must select identically after declaration-order permutation"
    );
    assert_eq!(priority_reversed.text(), "¤line\n");
    assert_eq!(priority_reversed.span(), priority.span());

    let unterminated_dollar = grammar::lex("$tag$body");
    let unterminated_dollar = unterminated_dollar.errors().next().unwrap();
    assert_eq!(unterminated_dollar.code(), "RCA4001");
    assert_eq!(
        unterminated_dollar.diagnostic_code(),
        grammar::LexErrorCode::UnterminatedSameDelimiter
    );
    assert_eq!(unterminated_dollar.text(), "$tag$body");
    assert_eq!(
        unterminated_dollar.span(),
        recursa::Span::new(0, 9).unwrap()
    );
    assert_eq!(
        unterminated_dollar.anchor(),
        recursa::Span::new(0, 5).unwrap()
    );

    for source in ["{-outer", "/* open"] {
        let unterminated_nested = grammar::lex(source);
        let unterminated_nested = unterminated_nested.errors().next().unwrap();
        assert_eq!(unterminated_nested.code(), "RCA4002");
        assert_eq!(
            unterminated_nested.diagnostic_code(),
            grammar::LexErrorCode::UnterminatedNested
        );
        assert_eq!(unterminated_nested.text(), source);
        assert_eq!(
            unterminated_nested.span(),
            recursa::Span::new(0, source.len()).unwrap()
        );
        assert_eq!(
            unterminated_nested.anchor(),
            recursa::Span::new(0, 2).unwrap()
        );
    }

    let mut deep_source = "{-".repeat(10_000);
    deep_source.push_str("body");
    deep_source.push_str(&"-}".repeat(10_000));
    let deep = grammar::lex(&deep_source);
    assert_eq!(deep.errors().count(), 0);
    let deep = deep.tokens().next().unwrap();
    assert_eq!(deep.kind(), grammar::TokenKind::NestedRegion);
    assert_eq!(deep.text(), deep_source);
    assert_eq!(
        deep.span(),
        recursa::Span::new(0, deep_source.len()).unwrap()
    );
    let deep_unterminated_source = "{-".repeat(10_000);
    let deep_unterminated = grammar::lex(&deep_unterminated_source);
    let deep_unterminated = deep_unterminated.errors().next().unwrap();
    assert_eq!(deep_unterminated.code(), "RCA4002");
    assert_eq!(
        deep_unterminated.span(),
        recursa::Span::new(0, deep_unterminated_source.len()).unwrap()
    );
    assert_eq!(
        deep_unterminated.anchor(),
        recursa::Span::new(0, 2).unwrap()
    );

    // `unterminated = to_end_of_input` leaves a terminated region unchanged.
    let tolerated_dollar_terminated = grammar::lex("~~tag~~body~~tag~~");
    assert_eq!(tolerated_dollar_terminated.errors().count(), 0);
    let tolerated_dollar_terminated = tolerated_dollar_terminated.tokens().next().unwrap();
    assert_eq!(
        tolerated_dollar_terminated.kind(),
        grammar::TokenKind::TildeRegion
    );
    assert_eq!(tolerated_dollar_terminated.text(), "~~tag~~body~~tag~~");
    assert_eq!(
        tolerated_dollar_terminated.span(),
        recursa::Span::new(0, 18).unwrap()
    );

    // Without a closing copy of the opener, the tolerant region succeeds to end of input
    // instead of reporting RCA4001: no error, and the token's text and span run from the
    // opener through end of input.
    let tolerated_dollar = grammar::lex("~~tag~~body");
    assert_eq!(tolerated_dollar.errors().count(), 0);
    let tolerated_dollar = tolerated_dollar.tokens().next().unwrap();
    assert_eq!(tolerated_dollar.kind(), grammar::TokenKind::TildeRegion);
    assert_eq!(tolerated_dollar.text(), "~~tag~~body");
    assert_eq!(
        tolerated_dollar.span(),
        recursa::Span::new(0, 11).unwrap()
    );

    // A tolerant `nested` matcher still honors depth when the closer does exist.
    let tolerated_nested_terminated = grammar::lex("<<outer <<inner>> tail>>");
    assert_eq!(tolerated_nested_terminated.errors().count(), 0);
    let tolerated_nested_terminated = tolerated_nested_terminated.tokens().next().unwrap();
    assert_eq!(
        tolerated_nested_terminated.kind(),
        grammar::TokenKind::ToleratedNestedRegion
    );
    assert_eq!(
        tolerated_nested_terminated.text(),
        "<<outer <<inner>> tail>>"
    );
    assert_eq!(
        tolerated_nested_terminated.span(),
        recursa::Span::new(0, 24).unwrap()
    );

    // Without a closer anywhere, the tolerant nested region succeeds to end of input instead
    // of reporting RCA4002.
    let tolerated_nested = grammar::lex("<<outer");
    assert_eq!(tolerated_nested.errors().count(), 0);
    let tolerated_nested = tolerated_nested.tokens().next().unwrap();
    assert_eq!(
        tolerated_nested.kind(),
        grammar::TokenKind::ToleratedNestedRegion
    );
    assert_eq!(tolerated_nested.text(), "<<outer");
    assert_eq!(tolerated_nested.span(), recursa::Span::new(0, 7).unwrap());

    let declarative = grammar::lex(":NULL :true :FaLsE :'TRUE' :\"FALSE\" !=--comment\n!=-");
    assert_eq!(declarative.errors().count(), 0);
    let declarative = declarative.tokens().collect::<Vec<_>>();
    assert_eq!(
        declarative
            .iter()
            .map(|token| token.kind())
            .collect::<Vec<_>>(),
        [
            grammar::TokenKind::COLON,
            grammar::TokenKind::NULL,
            grammar::TokenKind::COLON,
            grammar::TokenKind::TRUE,
            grammar::TokenKind::COLON,
            grammar::TokenKind::FALSE,
            grammar::TokenKind::COLON,
            grammar::TokenKind::QuotedText,
            grammar::TokenKind::COLON,
            grammar::TokenKind::QuotedText,
            grammar::TokenKind::NOT_EQUAL,
            grammar::TokenKind::BANG_EQ_MINUS,
        ]
    );
    assert_eq!(
        declarative
            .iter()
            .map(|token| token.text())
            .collect::<Vec<_>>(),
        [
            ":",
            "NULL",
            ":",
            "true",
            ":",
            "FaLsE",
            ":",
            "'TRUE'",
            ":",
            "\"FALSE\"",
            "!=",
            "!=-",
        ]
    );

    let fixed_fences_without_trivia = grammar::lex("--\n??");
    assert_eq!(fixed_fences_without_trivia.errors().count(), 0);
    let fixed_fences_without_trivia = fixed_fences_without_trivia.tokens().collect::<Vec<_>>();
    assert_eq!(
        fixed_fences_without_trivia
            .iter()
            .map(|token| token.kind())
            .collect::<Vec<_>>(),
        [
            grammar::TokenKind::DASH_DASH,
            grammar::TokenKind::QUESTION_QUESTION,
        ]
    );
    assert_eq!(
        fixed_fences_without_trivia
            .iter()
            .map(|token| token.text())
            .collect::<Vec<_>>(),
        ["--", "??"]
    );

    let block_fence = grammar::lex("!/*block*/ !/");
    assert_eq!(block_fence.errors().count(), 0);
    let block_fence = block_fence.tokens().collect::<Vec<_>>();
    assert_eq!(
        block_fence
            .iter()
            .map(|token| token.kind())
            .collect::<Vec<_>>(),
        [
            grammar::TokenKind::OperatorText,
            grammar::TokenKind::BANG_SLASH,
        ]
    );
    assert_eq!(
        block_fence
            .iter()
            .map(|token| token.text())
            .collect::<Vec<_>>(),
        ["!", "!/"]
    );

    let declarative_eof_comment = grammar::lex("!=--comment @");
    assert_eq!(declarative_eof_comment.errors().count(), 0);
    let declarative_eof_comment = declarative_eof_comment.tokens().collect::<Vec<_>>();
    assert_eq!(declarative_eof_comment.len(), 1);
    assert_eq!(
        declarative_eof_comment[0].kind(),
        grammar::TokenKind::NOT_EQUAL
    );
    assert_eq!(declarative_eof_comment[0].text(), "!=");

    let admitted = grammar::lex("select null");
    let mut admitted_input = admitted.input();
    let admitted = grammar::Selection::parse(&mut admitted_input)
        .expect("an admitted keyword remains usable as content");
    assert_eq!(
        admitted.root().value().name.kind(),
        grammar::TokenKind::NULL
    );
    assert_eq!(admitted.root().value().name.text(), "null");
    let synthetic = grammar::WordText::with_kind(grammar::TokenKind::SELECT, "owned")
        .expect("an admitted kind is valid for public synthesis");
    assert_eq!(synthetic.kind(), grammar::TokenKind::SELECT);
    assert!(grammar::WordText::with_kind(grammar::TokenKind::COLON, "invalid").is_err());

    let admission_membership = [
        (grammar::TokenKind::SELECT, true, true, false),
        (grammar::TokenKind::NULL, true, false, false),
        (grammar::TokenKind::TRUE, true, false, false),
        (grammar::TokenKind::FALSE, true, false, false),
        (grammar::TokenKind::CUSTOM_KEYWORD, true, true, false),
        (grammar::TokenKind::WINDOW, true, true, true),
        (grammar::TokenKind::OTHER, true, true, false),
        (grammar::TokenKind::ROWS, false, true, false),
        (grammar::TokenKind::RANGE, false, true, false),
        (grammar::TokenKind::GROUPS, false, true, false),
    ];
    for (kind, window, variable, bare) in admission_membership {
        assert_eq!(
            grammar::WindowName::with_kind(kind, "value").is_ok(),
            window
        );
        assert_eq!(
            grammar::VariableName::with_kind(kind, "value").is_ok(),
            variable
        );
        assert_eq!(grammar::BareName::with_kind(kind, "value").is_ok(), bare);
    }

    for (source, admitted) in [("WiNdOw", true), ("rows", false)] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::WindowOrRows::parse(&mut input)
            .expect("admission membership participates in generated dispatch");
        assert_eq!(
            matches!(parsed.root().value(), grammar::WindowOrRows::Window(_)),
            admitted
        );
        assert!(input.is_eof());
    }

    for (source, admitted) in [("rows", true), ("null", false)] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::VariableOrNull::parse(&mut input)
            .expect("all-word subtraction participates in generated dispatch");
        assert_eq!(
            matches!(parsed.root().value(), grammar::VariableOrNull::Variable(_)),
            admitted
        );
        assert!(input.is_eof());
    }

    for (source, admitted) in [("window", true), ("other", false)] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::BareOrOther::parse(&mut input)
            .expect("flag-derived membership participates in generated dispatch");
        assert_eq!(
            matches!(parsed.root().value(), grammar::BareOrOther::Bare(_)),
            admitted
        );
        assert!(input.is_eof());
    }

    for (source, window) in [
        ("select null true false window", true),
        ("select null true false other", false),
    ] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::FiveTokenChoice::parse(&mut input)
            .expect("the fixture's configured FIRST(5) dispatch parses");
        assert_eq!(
            matches!(parsed.root().value(), grammar::FiveTokenChoice::Window),
            window
        );
        assert!(input.is_eof());
    }

    // Perfect-hash word classification: every ASCII-case variant of a keyword
    // classifies identically, a non-keyword identifier and a keyword's strict
    // prefix or a keyword plus a trailing character all remain the unclassified
    // content kind, and a zero-length word-shaped record classifies to `None`
    // without probing the map on out-of-range input.
    for variant in ["select", "SELECT", "SeLeCt"] {
        let classified = grammar::lex(variant).tokens().collect::<Vec<_>>();
        assert_eq!(classified.len(), 1);
        assert_eq!(
            classified[0].kind(),
            grammar::TokenKind::SELECT,
            "case variant {variant:?} must classify as SELECT"
        );
        assert_eq!(classified[0].text(), variant);
    }
    for (source, reason) in [
        ("banana", "an unrelated word is not a keyword"),
        ("sel", "a strict prefix of a keyword is not the keyword"),
        (
            "selects",
            "a keyword plus a trailing character is not the keyword",
        ),
    ] {
        let classified = grammar::lex(source).tokens().collect::<Vec<_>>();
        assert_eq!(classified.len(), 1);
        assert_eq!(
            classified[0].kind(),
            grammar::TokenKind::WordText,
            "{reason}"
        );
        assert_eq!(classified[0].text(), source);
    }
    let mut empty_word = grammar::LexBuilder::new("");
    empty_word
        .append(
            grammar::TokenKind::WordText,
            recursa::Span::new(0, 0).unwrap(),
        )
        .expect("a zero-length word-shaped record is a well-formed alternative-lexer append");
    let empty_word = empty_word.finish();
    let empty_word = empty_word.tokens().collect::<Vec<_>>();
    assert_eq!(empty_word.len(), 1);
    assert_eq!(
        empty_word[0].kind(),
        grammar::TokenKind::WordText,
        "an empty word never matches a keyword"
    );
    assert_eq!(empty_word[0].text(), "");

    println!("lexing-ok RCA4000 RCA4001 RCA4002 RCA4004");
}
