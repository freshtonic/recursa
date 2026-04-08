use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan)]
#[scan(pattern = "=")]
struct Eq;

#[derive(Scan)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

#[derive(Parse)]
#[parse(rules = WsRules)]
struct LetBinding<'input> {
    let_kw: LetKw,
    name: Ident<'input>,
    eq: Eq,
    value: IntLit<'input>,
    semi: Semi,
}

#[test]
fn parse_struct_sequence() {
    let mut input = Input::<WsRules>::new("let x = 42;");
    let binding = LetBinding::parse(&mut input).unwrap();
    assert_eq!(binding.name.0, "x");
    assert_eq!(binding.value.0, "42");
    assert_eq!(input.cursor(), 11);
}

#[test]
fn parse_struct_peek() {
    let input = Input::<WsRules>::new("let x = 42;");
    assert!(LetBinding::peek(&input));
}

#[test]
fn parse_struct_peek_fails() {
    let input = Input::<WsRules>::new("var x = 42;");
    assert!(!LetBinding::peek(&input));
}

#[test]
fn parse_struct_error_on_bad_field() {
    let mut input = Input::<WsRules>::new("let 123 = 42;");
    let err = LetBinding::parse(&mut input);
    assert!(err.is_err());
    // Cursor should NOT have advanced (fork was not committed)
    assert_eq!(input.cursor(), 0);
}
