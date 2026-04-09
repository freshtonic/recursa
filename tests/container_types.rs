#![allow(dead_code)]

use recursa::seq::{NonEmpty, OptionalTrailing, Seq};
use recursa::{Input, Parse, ParseRules, Scan};

#[derive(Scan, Debug, Clone)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = "=")]
struct Eq;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ",")]
struct Comma;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\[")]
struct LBracket;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\]")]
struct RBracket;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

/// Array literal with optional trailing comma.
#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct ArrayLit<'input> {
    lbracket: LBracket,
    elements: Seq<IntLit<'input>, Comma, OptionalTrailing>,
    rbracket: RBracket,
}

/// Let binding with optional type annotation, using Box for the value.
#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct LetStmt<'input> {
    let_kw: LetKw,
    name: Ident<'input>,
    eq: Eq,
    value: Box<ArrayLit<'input>>,
    semi: Semi,
}

#[test]
fn integration_array_with_trailing_comma() {
    let mut input = Input::new("let x = [1, 2, 3,];");
    let stmt = LetStmt::parse(&mut input, &Lang).unwrap();
    assert_eq!(stmt.name.0, "x");
    assert_eq!(stmt.value.elements.len(), 3);
    assert!(input.is_empty());
}

#[test]
fn integration_array_without_trailing_comma() {
    let mut input = Input::new("let x = [1, 2, 3];");
    let stmt = LetStmt::parse(&mut input, &Lang).unwrap();
    assert_eq!(stmt.value.elements.len(), 3);
}

#[test]
fn integration_empty_array() {
    let mut input = Input::new("let x = [];");
    let stmt = LetStmt::parse(&mut input, &Lang).unwrap();
    assert!(stmt.value.elements.is_empty());
}

/// Non-empty array that must have at least one element.
#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct NonEmptyArrayLit<'input> {
    lbracket: LBracket,
    elements: Seq<IntLit<'input>, Comma, OptionalTrailing, NonEmpty>,
    rbracket: RBracket,
}

#[test]
fn integration_non_empty_array_parses() {
    let mut input = Input::new("[1, 2, 3]");
    let arr = NonEmptyArrayLit::parse(&mut input, &Lang).unwrap();
    assert_eq!(arr.elements.len(), 3);
    // Deref to slice works
    let slice: &[IntLit] = &arr.elements;
    assert_eq!(slice[0].0, "1");
}

#[test]
fn integration_non_empty_array_rejects_empty() {
    let mut input = Input::new("[]");
    let result = NonEmptyArrayLit::parse(&mut input, &Lang);
    assert!(result.is_err());
}

/// Option inside a struct -- optional semicolon.
#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct MaybeTerminated<'input> {
    name: Ident<'input>,
    semi: Option<Semi>,
}

#[test]
fn integration_option_some() {
    let mut input = Input::new("foo;");
    let mt = MaybeTerminated::parse(&mut input, &Lang).unwrap();
    assert_eq!(mt.name.0, "foo");
    assert!(mt.semi.is_some());
    assert!(input.is_empty());
}

#[test]
fn integration_option_none() {
    let mut input = Input::new("foo");
    let mt = MaybeTerminated::parse(&mut input, &Lang).unwrap();
    assert_eq!(mt.name.0, "foo");
    assert!(mt.semi.is_none());
    assert!(input.is_empty());
}
