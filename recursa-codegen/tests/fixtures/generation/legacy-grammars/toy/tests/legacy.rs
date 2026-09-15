use recursa::{HasSpan as _, NodeView as _};
use recursa_legacy_toy::toy;

fn parse(source: &str) -> Result<toy::SemicolonStmt, recursa::ParseError> {
    let lexed = toy::lex(source);
    assert_eq!(lexed.errors().count(), 0, "lex errors in {source:?}");
    let mut input = lexed.input();
    let parsed = toy::SemicolonStmt::parse(&mut input)?;
    assert!(input.is_eof(), "accepted parse must consume explicit EOF");
    Ok(parsed.into_ast())
}

#[test]
fn parses_a_single_semicolon() {
    let lexed = toy::lex(";");
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind(), toy::TokenKind::SEMI);

    let mut input = lexed.input();
    let parsed = toy::SemicolonStmt::parse_spanned(&mut input).expect("semicolon parses");
    assert!(input.is_eof(), "accepted parse must consume explicit EOF");
    assert_eq!(parsed.root().span(), recursa::Span::new(0, 1).ok());
}

#[test]
fn rejects_non_semicolon() {
    assert!(parse("foo").is_err());
}

#[test]
fn rejects_eof() {
    let Err(error) = parse("") else {
        panic!("EOF unexpectedly parsed as a semicolon")
    };
    assert_eq!(error.kind(), recursa::ParseErrorKind::UnexpectedEof);
}

#[test]
fn tokenize_skips_whitespace() {
    let lexed = toy::lex("  ;\n,\t (");
    assert_eq!(lexed.errors().count(), 0);
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].kind(), toy::TokenKind::SEMI);
    assert_eq!(tokens[0].text(), ";");
    assert_eq!(tokens[1].kind(), toy::TokenKind::COMMA);
    assert_eq!(tokens[1].text(), ",");
    assert_eq!(tokens[2].kind(), toy::TokenKind::LPAREN);
    assert_eq!(tokens[2].text(), "(");
}
