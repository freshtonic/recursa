use recursa_core::{Input, NoRules, Parse};
use recursa_derive::Parse;

#[derive(Parse, Debug)]
#[parse(pattern = "let")]
struct LetKw;

#[derive(Parse, Debug)]
#[parse(pattern = "if")]
struct IfKw;

#[derive(Parse, Debug)]
#[parse(pattern = "while")]
struct WhileKw;

#[derive(Parse, Debug)]
enum Keyword {
    Let(LetKw),
    If(IfKw),
    While(WhileKw),
}

#[test]
fn scan_enum_let() {
    let mut input = Input::new("let x");
    let kw = Keyword::parse::<NoRules>(&mut input).unwrap();
    assert!(matches!(kw, Keyword::Let(_)));
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_enum_if() {
    let mut input = Input::new("if true");
    let kw = Keyword::parse::<NoRules>(&mut input).unwrap();
    assert!(matches!(kw, Keyword::If(_)));
    assert_eq!(input.cursor(), 2);
}

#[test]
fn scan_enum_while() {
    let mut input = Input::new("while true");
    let kw = Keyword::parse::<NoRules>(&mut input).unwrap();
    assert!(matches!(kw, Keyword::While(_)));
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_enum_peek() {
    let input = Input::new("let x");
    assert!(Keyword::peek::<NoRules>(&input));
}

#[test]
fn scan_enum_peek_fails() {
    let input = Input::new("123");
    assert!(!Keyword::peek::<NoRules>(&input));
}

#[test]
fn scan_enum_no_match_returns_error() {
    let mut input = Input::new("123");
    let result = Keyword::parse::<NoRules>(&mut input);
    assert!(result.is_err());
}

#[test]
fn scan_enum_declaration_order() {
    #[derive(Parse, Debug)]
    #[parse(pattern = r"ab")]
    struct Ab;

    #[derive(Parse, Debug)]
    #[parse(pattern = r"ab")]
    struct AbAlt;

    #[derive(Parse, Debug)]
    enum AbToken {
        First(Ab),
        #[allow(dead_code)]
        Second(AbAlt),
    }

    let mut input = Input::new("ab");
    let tok = AbToken::parse::<NoRules>(&mut input).unwrap();
    assert!(matches!(tok, AbToken::First(_)));
}
