use recursa_core::{Input, NoRules, Parse};
use recursa_derive::Parse;

#[derive(Parse)]
#[parse(pattern = "let")]
struct LetKeyword;

#[test]
fn scan_unit_struct_peek() {
    let input = Input::new("let x = 1");
    assert!(LetKeyword::peek::<NoRules>(&input));
}

#[test]
fn scan_unit_struct_peek_fails() {
    let input = Input::new("var x = 1");
    assert!(!LetKeyword::peek::<NoRules>(&input));
}

#[test]
fn scan_unit_struct_parse() {
    let mut input = Input::new("let x = 1");
    let _kw = LetKeyword::parse::<NoRules>(&mut input).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_unit_struct_parse_fails() {
    let mut input = Input::new("var x = 1");
    let err = LetKeyword::parse::<NoRules>(&mut input);
    assert!(err.is_err());
}
