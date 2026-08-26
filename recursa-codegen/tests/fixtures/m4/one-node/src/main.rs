use recursa::{HasSpan as _, NodeView as _};
use recursa_generated_one_node::r#type as grammar;

fn assert_token_kind_value<T: Copy + Eq + std::hash::Hash + std::fmt::Debug>() {}

fn main() {
    assert_token_kind_value::<grammar::TokenKind>();
    assert!(option_env!("RECURSA_GENERATION").is_none());
    let source = String::from("alpha");
    let lexed = grammar::lex(&source);
    let token = lexed.tokens().next().expect("one generated token");
    assert_eq!(token.kind(), grammar::TokenKind::WordText);
    assert_eq!(token.text(), "alpha");
    assert_eq!(token.span(), recursa::Span::new(0, 5).unwrap());
    assert_eq!(lexed.tokens().count(), 1);

    let mut input = lexed.input();
    let parsed = grammar::r#match::parse(&mut input).expect("generated parse succeeds");
    assert_eq!(parsed.root().value().text.text(), "alpha");
    assert!(std::ptr::eq(
        parsed.root().value().text.text().as_ptr(),
        source.as_ptr(),
    ));
    assert_eq!(parsed.root().span(), None);
    assert_eq!(input.cursor(), 1);
    assert!(input.is_eof());
    let ast = parsed.into_ast();
    assert_eq!(ast.text.text(), "alpha");

    let spanned = grammar::lex("beta");
    let mut spanned_input = spanned.input();
    let spanned = grammar::r#match::parse_spanned(&mut spanned_input)
        .expect("generated spanned parse succeeds");
    assert_eq!(
        spanned.root().span(),
        Some(recursa::Span::new(0, 4).unwrap())
    );

    let empty = grammar::lex("");
    let mut empty_input = empty.input();
    let Err(error) = grammar::r#match::parse(&mut empty_input) else {
        panic!("empty input must not parse as the raw-named Node");
    };
    assert_eq!(error.kind(), recursa::ParseErrorKind::UnexpectedEof);
    assert_eq!(empty_input.cursor(), 0);

    println!("one-node-ok");
}
