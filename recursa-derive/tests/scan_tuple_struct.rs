use recursa_core::{Input, NoRules, Parse};
use recursa_derive::Parse;

#[derive(Parse)]
#[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Parse)]
#[parse(pattern = r"[0-9]+")]
struct IntLiteral<'input>(&'input str);

#[test]
fn scan_tuple_struct_parse_captures() {
    let mut input = Input::new("hello world");
    let ident = Ident::parse::<NoRules>(&mut input).unwrap();
    assert_eq!(ident.0, "hello");
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_tuple_struct_int_literal() {
    let mut input = Input::new("42 + 1");
    let lit = IntLiteral::parse::<NoRules>(&mut input).unwrap();
    assert_eq!(lit.0, "42");
    assert_eq!(input.cursor(), 2);
}

#[derive(Parse)]
#[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct OwnedIdent(String);

#[derive(Parse)]
#[parse(pattern = r"[0-9]+")]
struct OwnedInt(String);

#[test]
fn scan_owned_tuple_struct_parse_captures() {
    let mut input = Input::new("hello world");
    let ident = OwnedIdent::parse::<NoRules>(&mut input).unwrap();
    assert_eq!(ident.0, "hello");
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_owned_tuple_struct_int() {
    let mut input = Input::new("42 + 1");
    let lit = OwnedInt::parse::<NoRules>(&mut input).unwrap();
    assert_eq!(lit.0, "42");
    assert_eq!(input.cursor(), 2);
}
