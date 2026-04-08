use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan)]
#[scan(pattern = "return")]
struct ReturnKw;

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

#[derive(Parse)]
#[parse(rules = WsRules)]
struct ReturnStmt<'input> {
    return_kw: ReturnKw,
    value: IntLit<'input>,
    semi: Semi,
}

#[derive(Parse)]
#[parse(rules = WsRules)]
enum Statement<'input> {
    Let(LetBinding<'input>),
    Return(ReturnStmt<'input>),
}

#[test]
fn parse_enum_let_variant() {
    let mut input = Input::<WsRules>::new("let x = 42;");
    let stmt = Statement::parse(&mut input).unwrap();
    assert!(matches!(stmt, Statement::Let(_)));
}

#[test]
fn parse_enum_return_variant() {
    let mut input = Input::<WsRules>::new("return 42;");
    let stmt = Statement::parse(&mut input).unwrap();
    assert!(matches!(stmt, Statement::Return(_)));
}

#[test]
fn parse_enum_peek() {
    let input = Input::<WsRules>::new("let x = 42;");
    assert!(Statement::peek(&input));

    let input2 = Input::<WsRules>::new("return 42;");
    assert!(Statement::peek(&input2));
}

#[test]
fn parse_enum_peek_fails() {
    let input = Input::<WsRules>::new("if true {}");
    assert!(!Statement::peek(&input));
}

#[test]
fn parse_enum_error_reports_all_variants() {
    let mut input = Input::<WsRules>::new("if true {}");
    let result = Statement::parse(&mut input);
    match result {
        Err(err) => {
            let msg = format!("{}", err);
            // Error should mention both expected alternatives
            assert!(msg.contains("one of"), "expected 'one of' in error: {msg}");
        }
        Ok(_) => panic!("expected parse to fail"),
    }
}
