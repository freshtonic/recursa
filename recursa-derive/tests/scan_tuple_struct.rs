use recursa_core::{Input, NoRules, Parse, Scan};
use recursa_derive::Scan;

#[derive(Scan)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan)]
#[scan(pattern = r"[0-9]+")]
struct IntLiteral<'input>(&'input str);

#[test]
fn scan_tuple_struct_parse_captures() {
    let mut input = Input::new("hello world");
    let ident = <Ident as Scan>::parse(&mut input).unwrap();
    assert_eq!(ident.0, "hello");
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_tuple_struct_int_literal() {
    let mut input = Input::new("42 + 1");
    let lit = <IntLiteral as Scan>::parse(&mut input).unwrap();
    assert_eq!(lit.0, "42");
    assert_eq!(input.cursor(), 2);
}

#[test]
fn scan_tuple_struct_is_terminal() {
    let is_terminal = <Ident as Parse>::IS_TERMINAL;
    assert!(is_terminal);
}

#[test]
fn scan_tuple_struct_first_pattern() {
    assert_eq!(<Ident as Parse>::first_pattern(), r"[a-zA-Z_][a-zA-Z0-9_]*");
}

#[test]
fn scan_tuple_struct_parse_through_parse_trait() {
    let mut input = Input::new("hello world");
    let ident = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
    assert_eq!(ident.0, "hello");
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_tuple_struct_peek_through_parse_trait() {
    let input = Input::new("hello world");
    assert!(<Ident as Parse>::peek(&input, &NoRules));
}
