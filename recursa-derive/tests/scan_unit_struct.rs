use recursa_core::{Input, NoRules, Scan};
use recursa_derive::Scan;

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKeyword;

#[test]
fn scan_unit_struct_peek() {
    let input = Input::<NoRules>::new("let x = 1");
    assert!(LetKeyword::peek(&input));
}

#[test]
fn scan_unit_struct_peek_fails() {
    let input = Input::<NoRules>::new("var x = 1");
    assert!(!LetKeyword::peek(&input));
}

#[test]
fn scan_unit_struct_parse() {
    let mut input = Input::<NoRules>::new("let x = 1");
    let _kw = LetKeyword::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_unit_struct_parse_fails() {
    let mut input = Input::<NoRules>::new("var x = 1");
    let err = LetKeyword::parse(&mut input);
    assert!(err.is_err());
}

#[test]
fn scan_unit_struct_is_terminal() {
    let is_terminal = <LetKeyword as recursa_core::Parse>::IS_TERMINAL;
    assert!(is_terminal);
}

#[test]
fn scan_unit_struct_first_patterns() {
    let patterns = <LetKeyword as recursa_core::Parse>::first_patterns();
    assert_eq!(patterns, &["let"]);
}
