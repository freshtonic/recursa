use recursa_core::{Input, NoRules, Scan};
use recursa_derive::Scan;

#[derive(Scan, Debug)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan, Debug)]
#[scan(pattern = "if")]
struct IfKw;

#[derive(Scan, Debug)]
#[scan(pattern = "while")]
struct WhileKw;

#[derive(Scan, Debug)]
enum Keyword {
    Let(LetKw),
    If(IfKw),
    While(WhileKw),
}

#[test]
fn scan_enum_let() {
    let mut input = Input::<NoRules>::new("let x");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::Let(_)));
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_enum_if() {
    let mut input = Input::<NoRules>::new("if true");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::If(_)));
    assert_eq!(input.cursor(), 2);
}

#[test]
fn scan_enum_while() {
    let mut input = Input::<NoRules>::new("while true");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::While(_)));
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_enum_longest_match() {
    #[derive(Scan, Debug)]
    #[scan(pattern = r"[a-zA-Z]+")]
    struct Word<'input>(&'input str);

    #[derive(Scan, Debug)]
    #[scan(pattern = r"[0-9]+")]
    #[allow(dead_code)]
    struct Num<'input>(&'input str);

    #[derive(Scan, Debug)]
    #[allow(dead_code)]
    enum Token<'input> {
        Word(Word<'input>),
        Num(Num<'input>),
    }

    let mut input = Input::<NoRules>::new("hello123");
    let tok = Token::parse(&mut input).unwrap();
    assert!(matches!(tok, Token::Word(w) if w.0 == "hello"));
}

#[test]
fn scan_enum_peek() {
    let input = Input::<NoRules>::new("let x");
    assert!(Keyword::peek(&input));
}

#[test]
fn scan_enum_peek_fails() {
    let input = Input::<NoRules>::new("123");
    assert!(!Keyword::peek(&input));
}

#[test]
fn scan_enum_no_match_returns_error() {
    let mut input = Input::<NoRules>::new("123");
    let result = Keyword::parse(&mut input);
    assert!(result.is_err());
}

#[test]
fn scan_enum_declaration_order_tiebreaker() {
    // When two variants match with the same length, declaration order wins
    #[derive(Scan, Debug)]
    #[scan(pattern = r"ab")]
    struct Ab;

    #[derive(Scan, Debug)]
    #[scan(pattern = r"ab")]
    struct AbAlt;

    #[derive(Scan, Debug)]
    enum AbToken {
        First(Ab),
        Second(AbAlt),
    }

    let mut input = Input::<NoRules>::new("ab");
    let tok = AbToken::parse(&mut input).unwrap();
    assert!(matches!(tok, AbToken::First(_)));
}
