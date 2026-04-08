use recursa_core::{Input, NoRules, Scan};

recursa_core::keywords! {
    Let   => "let",
    If    => "if",
    While => "while",
}

recursa_core::punctuation! {
    Plus   => r"\+",
    Minus  => r"\-",
    LParen => r"\(",
}

recursa_core::literals! {
    IntLit   => r"[0-9]+",
    IdentLit => r"[a-zA-Z_][a-zA-Z0-9_]*",
}

#[test]
fn keyword_macro_creates_types() {
    let mut input = Input::<NoRules>::new("let x");
    let _kw = Let::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn keyword_macro_creates_enum() {
    let mut input = Input::<NoRules>::new("if x");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::If(_)));
}

#[test]
fn punctuation_macro_parses() {
    let mut input = Input::<NoRules>::new("+ 1");
    let _p = Plus::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 1);
}

#[test]
fn punctuation_macro_creates_enum() {
    let mut input = Input::<NoRules>::new("(");
    let p = Punctuation::parse(&mut input).unwrap();
    assert!(matches!(p, Punctuation::LParen(_)));
}

#[test]
fn literals_macro_captures() {
    let mut input = Input::<NoRules>::new("42 hello");
    let lit = IntLit::parse(&mut input).unwrap();
    assert_eq!(lit.0, "42");
}

#[test]
fn literals_macro_creates_enum() {
    let mut input = Input::<NoRules>::new("hello");
    let lit = Literal::parse(&mut input).unwrap();
    assert!(matches!(lit, Literal::IdentLit(_)));
}
