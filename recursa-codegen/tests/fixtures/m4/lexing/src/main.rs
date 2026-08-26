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

    println!("lexing-ok");
}
