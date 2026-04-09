#![allow(dead_code)]

use recursa_core::seq::Seq;
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
