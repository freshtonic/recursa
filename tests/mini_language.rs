//! End-to-end test: parse a tiny language with let bindings and expressions.

#![allow(dead_code)]

use recursa::{Input, Parse, ParseRules};

// -- Token types --

#[derive(Parse, Debug)]
#[parse(pattern = "let")]
struct LetKw;

#[derive(Parse, Debug)]
#[parse(pattern = "=")]
struct Eq;

#[derive(Parse, Debug)]
#[parse(pattern = ";")]
struct Semi;

#[derive(Parse, Debug)]
#[parse(pattern = r"\+")]
struct Plus;

#[derive(Parse, Debug)]
#[parse(pattern = r"\*")]
struct Star;

#[derive(Parse, Debug)]
#[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Parse, Debug)]
#[parse(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

// -- Grammar rules --

struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

// -- AST --

#[derive(Parse, Debug)]
#[parse(rules = Lang, pratt)]
enum Expr<'input> {
    #[parse(infix, bp = 5)]
    Add(Box<Expr<'input>>, Plus, Box<Expr<'input>>),

    #[parse(infix, bp = 6)]
    Mul(Box<Expr<'input>>, Star, Box<Expr<'input>>),

    #[parse(atom)]
    Lit(IntLit<'input>),

    #[parse(atom)]
    Var(Ident<'input>),
}

#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct LetStmt<'input> {
    let_kw: LetKw,
    name: Ident<'input>,
    eq: Eq,
    value: Expr<'input>,
    semi: Semi,
}

// -- Tests --

#[test]
fn parse_let_with_expression() {
    let mut input = Input::new("let x = 1 + 2 * 3;");
    let stmt = LetStmt::parse::<Lang>(&mut input).unwrap();
    assert_eq!(stmt.name.0, "x");
    // value should be Add(Lit(1), Mul(Lit(2), Lit(3)))
    match stmt.value {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Lit(ref l) if l.0 == "1"));
            assert!(matches!(*right, Expr::Mul(_, _, _)));
        }
        _ => panic!("expected Add, got {:?}", stmt.value),
    }
    assert!(input.is_empty());
}

#[test]
fn parse_error_has_span() {
    use recursa::miette::Diagnostic;
    let mut input = Input::new("let 123 = 1;");
    let err = LetStmt::parse::<Lang>(&mut input).unwrap_err();
    // Error should have labels (spans)
    assert!(err.labels().is_some());
}
