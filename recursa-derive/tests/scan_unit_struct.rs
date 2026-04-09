use recursa_core::{Input, NoRules, Parse};
use recursa_derive::Scan;

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKeyword;

#[test]
fn scan_unit_struct_peek() {
    let input = Input::new("let x = 1");
    assert!(LetKeyword::peek(&input, &NoRules));
}

#[test]
fn scan_unit_struct_peek_fails() {
    let input = Input::new("var x = 1");
    assert!(!LetKeyword::peek(&input, &NoRules));
}

#[test]
fn scan_unit_struct_parse() {
    let mut input = Input::new("let x = 1");
    let _kw = LetKeyword::parse(&mut input, &NoRules).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_unit_struct_parse_fails() {
    let mut input = Input::new("var x = 1");
    let err = LetKeyword::parse(&mut input, &NoRules);
    assert!(err.is_err());
}

#[test]
fn scan_unit_struct_is_terminal() {
    let is_terminal = LetKeyword::IS_TERMINAL;
    assert!(is_terminal);
}

#[test]
fn scan_unit_struct_first_pattern() {
    assert_eq!(LetKeyword::first_pattern(), "let");
}
