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

#[derive(Parse, Debug)]
#[parse(pattern = r"@")]
struct At;

#[derive(Parse, Debug)]
#[parse(pattern = r"~")]
struct Tilde;

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

    // Infix `~` at a LOW bp (below the inner operands of the ternary postfix
    // below). This exists to create the ambiguity test: without inner_bp,
    // the inner Box<Self> parses would swallow `~`.
    #[parse(infix, bp = 3)]
    TildeInfix(Box<Expr<'input>>, Tilde, Box<Expr<'input>>),

    // Ternary postfix: `lhs @ inner1 ~ inner2`. Inner Box<Self> fields must
    // parse at min_bp = 4 (higher than TildeInfix) so the separator `~`
    // terminates inner1 and is consumed by the postfix instead of being
    // absorbed as an infix.
    #[parse(postfix, bp = 20, inner_bp = 4)]
    Ternary(
        Box<Expr<'input>>,
        At,
        Box<Expr<'input>>,
        Tilde,
        Box<Expr<'input>>,
    ),

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
fn pratt_ternary_postfix_inner_bp_bounds_middle_operand() {
    // `1 @ 2 ~ 3` must parse as Ternary(1, @, 2, ~, 3).
    // Without `inner_bp`, the inner Box<Expr> at position 2 would parse as
    // a TildeInfix(2, ~, 3), leaving no `~` for the postfix to consume.
    let mut input = Input::new("1 @ 2 ~ 3");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    match expr {
        Expr::Ternary(a, _, b, _, c) => {
            assert!(matches!(*a, Expr::Lit(_)));
            assert!(matches!(*b, Expr::Lit(_)));
            assert!(matches!(*c, Expr::Lit(_)));
        }
        other => panic!("expected Ternary, got {other:?}"),
    }
}

#[test]
fn pratt_ternary_postfix_inner_allows_higher_bp_infix() {
    // `1 @ 2 + 3 ~ 4` should still allow Add (bp=5) inside the inner
    // operand, since inner_bp=4 and 5 >= 4.
    let mut input = Input::new("1 @ 2 + 3 ~ 4");
    let expr = Expr::parse::<WsRules>(&mut input).unwrap();
    match expr {
        Expr::Ternary(_, _, mid, _, _) => {
            assert!(matches!(*mid, Expr::Add(_, _, _)));
        }
        other => panic!("expected Ternary, got {other:?}"),
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
