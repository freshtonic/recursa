use recursa::NodeView as _;
use recursa_generated_lexing::grammar;

fn assert_token_kind_value<T: Copy + Eq + std::hash::Hash + std::fmt::Debug>() {}

fn main() {
    assert_token_kind_value::<grammar::TokenKind>();

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
    assert_eq!(errors[0].anchor(), recursa::Span::new(13, 14).unwrap());
    assert_eq!(
        grammar::LexErrorCode::NestedDepthOverflow.as_str(),
        "RCA4003"
    );

    let alternative_source = String::from("sElEcT");
    grammar::reset_post_lex_calls();
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
    assert_eq!(grammar::post_lex_calls(), 1);

    let mut standalone_keyword = grammar::LexBuilder::new("Kk");
    standalone_keyword
        .append(
            grammar::TokenKind::CallbackText,
            recursa::Span::new(0, 2).unwrap(),
        )
        .expect("an alternative lexer may initially report a content kind");
    let standalone_keyword = standalone_keyword.finish();
    assert_eq!(
        standalone_keyword.tokens().next().unwrap().kind(),
        grammar::TokenKind::CALLBACK_KEYWORD,
    );

    let unicode_callback = grammar::lex("~é");
    let unicode_callback = unicode_callback
        .tokens()
        .next()
        .expect("the callback accepted one complete non-ASCII scalar");
    assert_eq!(unicode_callback.text(), "~é");

    let repaired_error = grammar::lex(":ERROR");
    assert_eq!(repaired_error.tokens().count(), 0);
    assert_eq!(repaired_error.errors().next().unwrap().text(), ":ERROR");

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
    assert_eq!(alternative_error.anchor(), recursa::Span::new(0, 4).unwrap());

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
    assert!(stripped_operators
        .iter()
        .all(|token| token.kind() == grammar::TokenKind::OperatorText && token.text() == "+"));
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
    assert_eq!(unterminated_dollar.span(), recursa::Span::new(0, 9).unwrap());
    assert_eq!(unterminated_dollar.anchor(), recursa::Span::new(0, 5).unwrap());

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
        assert_eq!(unterminated_nested.anchor(), recursa::Span::new(0, 2).unwrap());
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
    assert_eq!(deep_unterminated.anchor(), recursa::Span::new(0, 2).unwrap());

    grammar::reset_post_lex_calls();
    let callback_lexed = grammar::lex("~emit ~as ~skip ~error ~tail");
    assert_eq!(grammar::post_lex_calls(), 1);
    let callback_tokens = callback_lexed.tokens().collect::<Vec<_>>();
    assert_eq!(callback_tokens.len(), 3);
    assert_eq!(
        callback_tokens[0].kind(),
        grammar::TokenKind::CallbackText
    );
    assert_eq!(callback_tokens[0].text(), "~emit");
    assert_eq!(callback_tokens[1].kind(), grammar::TokenKind::SELECT);
    assert_eq!(callback_tokens[1].text(), "~as");
    assert_eq!(
        callback_tokens[2].kind(),
        grammar::TokenKind::CallbackText
    );
    assert_eq!(callback_tokens[2].text(), "~tail");
    let callback_errors = callback_lexed.errors().collect::<Vec<_>>();
    assert_eq!(callback_errors.len(), 1);
    assert_eq!(callback_errors[0].text(), "~error");
    assert_eq!(
        callback_errors[0].span(),
        recursa::Span::new(16, 22).unwrap()
    );

    let repaired = grammar::lex(":NULL :true :FaLsE :'TRUE' :\"FALSE\" !=--comment\n!=-");
    assert_eq!(repaired.errors().count(), 0);
    let repaired = repaired.tokens().collect::<Vec<_>>();
    assert_eq!(
        repaired.iter().map(|token| token.kind()).collect::<Vec<_>>(),
        [
            grammar::TokenKind::COLON,
            grammar::TokenKind::NULL,
            grammar::TokenKind::COLON,
            grammar::TokenKind::TRUE,
            grammar::TokenKind::COLON,
            grammar::TokenKind::FALSE,
            grammar::TokenKind::PseudoText,
            grammar::TokenKind::PseudoText,
            grammar::TokenKind::NOT_EQUAL,
            grammar::TokenKind::LEGIT_NOT_EQUAL,
        ]
    );
    assert_eq!(
        repaired.iter().map(|token| token.text()).collect::<Vec<_>>(),
        [
            ":", "NULL", ":", "true", ":", "FaLsE", ":'TRUE'", ":\"FALSE\"", "!=",
            "!=-",
        ]
    );

    let repaired_eof_comment = grammar::lex("!=--comment @");
    assert_eq!(repaired_eof_comment.errors().count(), 0);
    let repaired_eof_comment = repaired_eof_comment.tokens().collect::<Vec<_>>();
    assert_eq!(repaired_eof_comment.len(), 1);
    assert_eq!(repaired_eof_comment[0].kind(), grammar::TokenKind::NOT_EQUAL);
    assert_eq!(repaired_eof_comment[0].text(), "!=");

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
    assert!(
        grammar::WordText::with_kind(grammar::TokenKind::COLON, "invalid").is_err()
    );

    let admission_membership = [
        (grammar::TokenKind::SELECT, true, true, false),
        (grammar::TokenKind::NULL, true, false, false),
        (grammar::TokenKind::TRUE, true, false, false),
        (grammar::TokenKind::FALSE, true, false, false),
        (grammar::TokenKind::CALLBACK_KEYWORD, true, true, false),
        (grammar::TokenKind::WINDOW, true, true, true),
        (grammar::TokenKind::OTHER, true, true, false),
        (grammar::TokenKind::ROWS, false, true, false),
        (grammar::TokenKind::RANGE, false, true, false),
        (grammar::TokenKind::GROUPS, false, true, false),
    ];
    for (kind, window, variable, bare) in admission_membership {
        assert_eq!(grammar::WindowName::with_kind(kind, "value").is_ok(), window);
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
            matches!(
                parsed.root().value(),
                grammar::VariableOrNull::Variable(_)
            ),
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

    println!("lexing-ok RCA4000 RCA4001 RCA4002 RCA4003 RCA4004");
}
