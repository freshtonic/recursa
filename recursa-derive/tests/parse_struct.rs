#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::Parse;

#[derive(Parse)]
#[parse(pattern = "let")]
struct LetKw;

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

#[test]
fn parse_struct_sequence() {
    let mut input = Input::new("let x = 42;");
    let binding = LetBinding::parse::<WsRules>(&mut input).unwrap();
    assert_eq!(binding.name.0, "x");
    assert_eq!(binding.value.0, "42");
    assert_eq!(input.cursor(), 11);
}

#[test]
fn parse_struct_peek() {
    let input = Input::new("let x = 42;");
    assert!(LetBinding::peek::<WsRules>(&input));
}

#[test]
fn parse_struct_peek_fails() {
    let input = Input::new("var x = 42;");
    assert!(!LetBinding::peek::<WsRules>(&input));
}

#[test]
fn parse_struct_error_on_bad_field() {
    let mut input = Input::new("let 123 = 42;");
    let err = LetBinding::parse::<WsRules>(&mut input);
    assert!(err.is_err());
    assert_eq!(input.cursor(), 0);
}
