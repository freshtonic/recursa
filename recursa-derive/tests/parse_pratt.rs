#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::Parse;

#[derive(Parse, Debug)]
#[parse(pattern = r"\+")]
struct Plus;

#[derive(Parse, Debug)]
#[parse(pattern = r"\*")]
struct Star;

#[derive(Parse, Debug)]
#[parse(pattern = r"-")]
struct Minus;

#[derive(Parse, Debug)]
#[parse(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

#[derive(Parse, Debug)]
#[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Parse, Debug)]
#[parse(pattern = r"\^")]
struct Caret;

#[derive(Parse, Debug)]
#[parse(pattern = r"\?")]
struct Question;

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
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

    #[parse(postfix, bp = 20)]
    PostfixQuestion(Box<Expr<'input>>, Question),

    #[parse(atom)]
    Lit(IntLit<'input>),

    #[parse(atom)]
    Name(Ident<'input>),
}

#[test]
fn pratt_atom() {
    let mut input = Input::new("42");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    assert!(matches!(expr, Expr::Lit(_)));
}

#[test]
fn pratt_simple_add() {
    let mut input = Input::new("1 + 2");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    assert!(matches!(expr, Expr::Add(_, _, _)));
}

#[test]
fn pratt_precedence_mul_over_add() {
    // 1 + 2 * 3 should parse as 1 + (2 * 3)
    let mut input = Input::new("1 + 2 * 3");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
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
    let mut input = Input::new("1 + 2 + 3");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
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
    let mut input = Input::new("-42");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    match expr {
        Expr::Neg(_, inner) => assert!(matches!(*inner, Expr::Lit(_))),
        _ => panic!("expected Neg"),
    }
}

#[test]
fn pratt_prefix_in_expression() {
    // -1 + 2 should parse as (-1) + 2 because prefix bp=9 > infix bp=5
    let mut input = Input::new("-1 + 2");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
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
    let mut input = Input::new("2 ^ 3 ^ 4");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
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
    let input = Input::new("42");
    assert!(Expr::peek::<WsRules>(&input));

    let input = Input::new("foo");
    assert!(Expr::peek::<WsRules>(&input));

    // Prefix operator is a valid start
    let input = Input::new("-1");
    assert!(Expr::peek::<WsRules>(&input));
}

#[test]
fn pratt_peek_invalid() {
    let input = Input::new("+ 1");
    assert!(!Expr::peek::<WsRules>(&input));

    let input = Input::new("");
    assert!(!Expr::peek::<WsRules>(&input));
}

#[test]
fn pratt_error_on_empty() {
    let mut input = Input::new("");
    let result = Expr::parse::<WsRules>(&mut input);
    assert!(result.is_err());
}

#[test]
fn pratt_postfix_simple() {
    let mut input = Input::new("42?");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    match expr {
        Expr::PostfixQuestion(inner, _) => {
            assert!(matches!(*inner, Expr::Lit(_)));
        }
        other => panic!("expected PostfixQuestion, got {other:?}"),
    }
}

#[test]
fn pratt_postfix_chains() {
    let mut input = Input::new("42??");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    match expr {
        Expr::PostfixQuestion(inner, _) => {
            assert!(matches!(*inner, Expr::PostfixQuestion(_, _)));
        }
        other => panic!("expected nested PostfixQuestion, got {other:?}"),
    }
}

#[test]
fn pratt_postfix_with_infix() {
    // 1 + 2? should parse as 1 + (2?) because postfix bp=20 > infix bp=5
    let mut input = Input::new("1 + 2?");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    match expr {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Lit(_)));
            assert!(matches!(*right, Expr::PostfixQuestion(_, _)));
        }
        other => panic!("expected Add at top level, got {other:?}"),
    }
}

#[test]
fn pratt_postfix_with_prefix() {
    // -42? should parse as -(42?) because postfix bp=20 > prefix bp=9
    let mut input = Input::new("-42?");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    match expr {
        Expr::Neg(_, inner) => {
            assert!(matches!(*inner, Expr::PostfixQuestion(_, _)));
        }
        other => panic!("expected Neg at top level, got {other:?}"),
    }
}
