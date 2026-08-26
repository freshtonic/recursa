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

    let errors = lexed.errors().collect::<Vec<grammar::LexError<'_>>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind(), grammar::TokenKind::LexicalError);
    assert_eq!(errors[0].span(), recursa::Span::new(13, 14).unwrap());
    assert_eq!(errors[0].text(), "@");

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

    let callback_lexed = grammar::lex("~emit ~as ~skip ~error ~tail");
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

    println!("lexing-ok");
}
