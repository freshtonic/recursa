use recursa_core::{Input, NoRules, Parse, Scan};
use recursa_derive::Scan;

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKeyword;

#[test]
fn scan_unit_struct_peek() {
    let input = Input::new("let x = 1");
    assert!(<LetKeyword as Scan>::peek(&input));
}

#[test]
fn scan_unit_struct_peek_fails() {
    let input = Input::new("var x = 1");
    assert!(!<LetKeyword as Scan>::peek(&input));
}

#[test]
fn scan_unit_struct_parse() {
    let mut input = Input::new("let x = 1");
    let _kw = <LetKeyword as Scan>::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_unit_struct_parse_fails() {
    let mut input = Input::new("var x = 1");
    let err = <LetKeyword as Scan>::parse(&mut input);
    assert!(err.is_err());
}

#[test]
fn scan_unit_struct_is_terminal() {
    let is_terminal = <LetKeyword as Parse>::IS_TERMINAL;
    assert!(is_terminal);
}

#[test]
fn scan_unit_struct_first_pattern() {
    assert_eq!(<LetKeyword as Parse>::first_pattern(), "let");
}

#[test]
fn scan_unit_struct_parse_through_parse_trait() {
    let mut input = Input::new("let x = 1");
    let _kw = <LetKeyword as Parse>::parse(&mut input, &NoRules).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_unit_struct_peek_through_parse_trait() {
    let input = Input::new("let x = 1");
    assert!(<LetKeyword as Parse>::peek(&input, &NoRules));
}
