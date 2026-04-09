#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

#[derive(Scan, Debug)]
#[scan(pattern = r"\+")]
struct Plus;

#[derive(Scan, Debug)]
#[scan(pattern = r"\*")]
struct Star;

#[derive(Scan, Debug)]
#[scan(pattern = r"-")]
struct Minus;

#[derive(Scan, Debug)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

#[derive(Scan, Debug)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug)]
#[scan(pattern = r"\^")]
struct Caret;

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules, pratt)]
enum Expr<'input> {
    #[parse(prefix, bp = 9)]
    Neg(Minus, Box<Expr<'input>>),

    #[parse(infix, bp = 5)]
    Add(Box<Expr<'input>>, Plus, Box<Expr<'input>>),

    #[parse(infix, bp = 6)]
    Mul(Box<Expr<'input>>, Star, Box<Expr<'input>>),

    #[parse(infix, bp = 7, assoc = "right")]
    Pow(Box<Expr<'input>>, Caret, Box<Expr<'input>>),

    #[parse(atom)]
    Lit(IntLit<'input>),

    #[parse(atom)]
    Name(Ident<'input>),
}

#[test]
fn pratt_atom() {
    let mut input = Input::<WsRules>::new("42");
    let expr = Expr::parse(&mut input).unwrap();
    assert!(matches!(expr, Expr::Lit(_)));
}

#[test]
fn pratt_simple_add() {
    let mut input = Input::<WsRules>::new("1 + 2");
    let expr = Expr::parse(&mut input).unwrap();
    assert!(matches!(expr, Expr::Add(_, _, _)));
}

#[test]
fn pratt_precedence_mul_over_add() {
    // 1 + 2 * 3 should parse as 1 + (2 * 3)
    let mut input = Input::<WsRules>::new("1 + 2 * 3");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Lit(_)));
            assert!(matches!(*right, Expr::Mul(_, _, _)));
        }
        _ => panic!("expected Add at top level"),
    }
}

#[test]
fn pratt_left_associativity() {
    // 1 + 2 + 3 should parse as (1 + 2) + 3
    let mut input = Input::<WsRules>::new("1 + 2 + 3");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Add(_, _, _)));
            assert!(matches!(*right, Expr::Lit(_)));
        }
        _ => panic!("expected Add at top level"),
    }
}

#[test]
fn pratt_prefix_neg() {
    let mut input = Input::<WsRules>::new("-42");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Neg(_, inner) => assert!(matches!(*inner, Expr::Lit(_))),
        _ => panic!("expected Neg"),
    }
}

#[test]
fn pratt_prefix_in_expression() {
    // -1 + 2 should parse as (-1) + 2 because prefix bp=9 > infix bp=5
    let mut input = Input::<WsRules>::new("-1 + 2");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Neg(_, _)));
            assert!(matches!(*right, Expr::Lit(_)));
        }
        _ => panic!("expected Add at top level"),
    }
}

#[test]
fn pratt_right_associativity() {
    // 2 ^ 3 ^ 4 should parse as 2 ^ (3 ^ 4) because ^ is right-associative
    let mut input = Input::<WsRules>::new("2 ^ 3 ^ 4");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Pow(left, _, right) => {
            assert!(matches!(*left, Expr::Lit(_)));
            assert!(matches!(*right, Expr::Pow(_, _, _)));
        }
        _ => panic!("expected Pow at top level"),
    }
}

#[test]
fn pratt_peek_valid() {
    let input = Input::<WsRules>::new("42");
    assert!(Expr::peek(&input));

    let input = Input::<WsRules>::new("foo");
    assert!(Expr::peek(&input));

    // Prefix operator is a valid start
    let input = Input::<WsRules>::new("-1");
    assert!(Expr::peek(&input));
}

#[test]
fn pratt_peek_invalid() {
    let input = Input::<WsRules>::new("+ 1");
    assert!(!Expr::peek(&input));

    let input = Input::<WsRules>::new("");
    assert!(!Expr::peek(&input));
}

#[test]
fn pratt_error_on_empty() {
    let mut input = Input::<WsRules>::new("");
    let result = Expr::parse(&mut input);
    assert!(result.is_err());
}
