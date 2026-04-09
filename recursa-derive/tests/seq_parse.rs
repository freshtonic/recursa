#![allow(dead_code)]

use recursa_core::seq::{OptionalTrailing, RequiredTrailing, Seq};
use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ",")]
struct Comma;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\(")]
struct LParen;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\)")]
struct RParen;

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct ArgList<'input> {
    lparen: LParen,
    args: Seq<Ident<'input>, Comma, WsRules>,
    rparen: RParen,
}

#[test]
fn seq_parse_no_trailing_allow_empty_with_elements() {
    let mut input = Input::<WsRules>::new("(a, b, c)");
    let arglist = ArgList::parse(&mut input).unwrap();
    let args: &Vec<Ident> = &arglist.args;
    assert_eq!(args.len(), 3);
    assert_eq!(args[0].0, "a");
    assert_eq!(args[1].0, "b");
    assert_eq!(args[2].0, "c");
}

#[test]
fn seq_parse_no_trailing_allow_empty_empty() {
    let mut input = Input::<WsRules>::new("()");
    let arglist = ArgList::parse(&mut input).unwrap();
    assert!(arglist.args.is_empty());
}

#[test]
fn seq_parse_no_trailing_single_element() {
    let mut input = Input::<WsRules>::new("(x)");
    let arglist = ArgList::parse(&mut input).unwrap();
    let args: &Vec<Ident> = &arglist.args;
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].0, "x");
}

// -- OptionalTrailing tests --

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct ArrayLit<'input> {
    lparen: LParen,
    elements: Seq<Ident<'input>, Comma, WsRules, OptionalTrailing>,
    rparen: RParen,
}

#[test]
fn seq_optional_trailing_no_trailing() {
    let mut input = Input::<WsRules>::new("(a, b, c)");
    let arr = ArrayLit::parse(&mut input).unwrap();
    assert_eq!(arr.elements.len(), 3);
}

#[test]
fn seq_optional_trailing_with_trailing() {
    let mut input = Input::<WsRules>::new("(a, b, c,)");
    let arr = ArrayLit::parse(&mut input).unwrap();
    assert_eq!(arr.elements.len(), 3);
    // Last element should have Some separator (trailing comma)
    let pairs = arr.elements.pairs();
    assert!(pairs[2].1.is_some());
}

#[test]
fn seq_optional_trailing_empty() {
    let mut input = Input::<WsRules>::new("()");
    let arr = ArrayLit::parse(&mut input).unwrap();
    assert!(arr.elements.is_empty());
}

// -- RequiredTrailing tests --

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct StmtBlock<'input> {
    lparen: LParen,
    stmts: Seq<Ident<'input>, Semi, WsRules, RequiredTrailing>,
    rparen: RParen,
}

#[test]
fn seq_required_trailing_with_elements() {
    let mut input = Input::<WsRules>::new("(a; b; c;)");
    let block = StmtBlock::parse(&mut input).unwrap();
    assert_eq!(block.stmts.len(), 3);
    // All elements should have Some separator
    for (_, sep) in block.stmts.pairs() {
        assert!(sep.is_some());
    }
}

#[test]
fn seq_required_trailing_empty() {
    let mut input = Input::<WsRules>::new("()");
    let block = StmtBlock::parse(&mut input).unwrap();
    assert!(block.stmts.is_empty());
}

#[test]
fn seq_required_trailing_error_on_missing_sep() {
    let mut input = Input::<WsRules>::new("(a; b)");
    let result = StmtBlock::parse(&mut input);
    // "b" has no trailing semicolon -- should error
    assert!(result.is_err());
}
