use recursa_core::{Input, NoRules, Scan};
use recursa_derive::Scan;

#[derive(Scan)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan)]
#[scan(pattern = r"[0-9]+")]
struct IntLiteral<'input>(&'input str);

#[test]
fn scan_tuple_struct_parse_captures() {
    let mut input = Input::<NoRules>::new("hello world");
    let ident = Ident::parse(&mut input).unwrap();
    assert_eq!(ident.0, "hello");
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_tuple_struct_int_literal() {
    let mut input = Input::<NoRules>::new("42 + 1");
    let lit = IntLiteral::parse(&mut input).unwrap();
    assert_eq!(lit.0, "42");
    assert_eq!(input.cursor(), 2);
}

#[test]
fn scan_tuple_struct_is_terminal() {
    let is_terminal = <Ident as recursa_core::Parse>::IS_TERMINAL;
    assert!(is_terminal);
}

#[test]
fn scan_tuple_struct_first_pattern() {
    assert_eq!(
        <Ident as recursa_core::Parse>::first_pattern(),
        r"[a-zA-Z_][a-zA-Z0-9_]*"
    );
}
