#![allow(dead_code)]

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
    let mut input = Input::new("let x = 42;");
    let binding = LetBinding::parse(&mut input, &WsRules).unwrap();
    assert_eq!(binding.name.0, "x");
    assert_eq!(binding.value.0, "42");
    assert_eq!(input.cursor(), 11);
}

#[test]
fn parse_struct_peek() {
    let input = Input::new("let x = 42;");
    assert!(LetBinding::peek(&input, &WsRules));
}

#[test]
fn parse_struct_peek_fails() {
    let input = Input::new("var x = 42;");
    assert!(!LetBinding::peek(&input, &WsRules));
}

#[test]
fn parse_struct_error_on_bad_field() {
    let mut input = Input::new("let 123 = 42;");
    let err = LetBinding::parse(&mut input, &WsRules);
    assert!(err.is_err());
    // Cursor should NOT have advanced (fork was not committed)
    assert_eq!(input.cursor(), 0);
}

#[test]
fn parse_struct_is_not_terminal() {
    const { assert!(!<LetBinding as Parse>::IS_TERMINAL) };
}

#[test]
fn parse_struct_first_pattern_consecutive_terminals() {
    // LetBinding fields: LetKw, Ident, Eq, IntLit, Semi
    // All are Scan (terminal) types, so first_pattern joins all with IGNORE separator.
    let pattern = <LetBinding as Parse>::first_pattern();
    assert_eq!(
        pattern,
        r"let(?:\s+)?[a-zA-Z_][a-zA-Z0-9_]*(?:\s+)?=(?:\s+)?[0-9]+(?:\s+)?;"
    );
}

#[derive(Parse)]
#[parse(rules = WsRules)]
struct NestedStmt<'input> {
    let_kw: LetKw,
    binding: LetBinding<'input>,
}

#[test]
fn parse_struct_first_pattern_stops_at_non_terminal() {
    // NestedStmt fields: LetKw (terminal), LetBinding (non-terminal)
    // Walk: include LetKw's pattern, LetKw is terminal so continue,
    // include LetBinding's first_pattern (the full joined pattern), LetBinding is NOT terminal so stop.
    let pattern = <NestedStmt as Parse>::first_pattern();
    let expected = format!("let(?:\\s+)?{}", <LetBinding as Parse>::first_pattern());
    assert_eq!(pattern, expected);
}

#[derive(Parse)]
#[parse(rules = WsRules)]
struct NestedWithTrailing<'input> {
    let_kw: LetKw,
    binding: LetBinding<'input>,
    semi: Semi, // should NOT appear in first_patterns
}

#[test]
fn parse_struct_first_pattern_does_not_include_fields_after_non_terminal() {
    // NestedWithTrailing: LetKw (terminal), LetBinding (non-terminal), Semi (terminal)
    // Walk stops after LetBinding (non-terminal), so Semi's pattern ";" is NOT included.
    let pattern = <NestedWithTrailing as Parse>::first_pattern();
    // Same as NestedStmt — the trailing Semi is not visited
    assert_eq!(pattern, <NestedStmt as Parse>::first_pattern());
}
