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
