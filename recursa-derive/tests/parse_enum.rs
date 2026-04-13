#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::Parse;

#[derive(Parse)]
#[parse(pattern = "let")]
struct LetKw;

#[derive(Parse)]
#[parse(pattern = "return")]
struct ReturnKw;

#[derive(Parse)]
#[parse(pattern = "=")]
struct Eq;

#[derive(Parse)]
#[parse(pattern = ";")]
struct Semi;

#[derive(Parse)]
#[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Parse)]
#[parse(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
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
    let mut input = Input::new("let x = 42;");
    let stmt = Statement::parse::<WsRules>(&mut input).unwrap();
    assert!(matches!(stmt, Statement::Let(_)));
}

#[test]
fn parse_enum_return_variant() {
    let mut input = Input::new("return 42;");
    let stmt = Statement::parse::<WsRules>(&mut input).unwrap();
    assert!(matches!(stmt, Statement::Return(_)));
}

#[test]
fn parse_enum_peek() {
    let input = Input::new("let x = 42;");
    assert!(Statement::peek::<WsRules>(&input));

    let input2 = Input::new("return 42;");
    assert!(Statement::peek::<WsRules>(&input2));
}

#[test]
fn parse_enum_peek_fails() {
    let input = Input::new("if true {}");
    assert!(!Statement::peek::<WsRules>(&input));
}

#[test]
fn parse_enum_with_leading_whitespace() {
    let mut input = Input::new("  let x = 42;");
    let stmt = Statement::parse::<WsRules>(&mut input).unwrap();
    assert!(matches!(stmt, Statement::Let(_)));
}

#[test]
fn parse_enum_peek_with_leading_whitespace() {
    let input = Input::new("  return 42;");
    assert!(Statement::peek::<WsRules>(&input));
}

#[test]
fn parse_enum_error_reports_all_variants() {
    let mut input = Input::new("if true {}");
    let result = Statement::parse::<WsRules>(&mut input);
    assert!(result.is_err());
}
