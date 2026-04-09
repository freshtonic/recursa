#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

// -- Tokens --

#[derive(Scan, Debug)]
#[scan(pattern = "pub")]
struct PubKw;

#[derive(Scan, Debug)]
#[scan(pattern = "fn")]
struct FnKw;

#[derive(Scan, Debug)]
#[scan(pattern = "struct")]
struct StructKw;

#[derive(Scan, Debug)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug)]
#[scan(pattern = r"\{")]
struct LBrace;

#[derive(Scan, Debug)]
#[scan(pattern = r"\}")]
struct RBrace;

// -- Rules --

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

// -- AST: two structs that share the same first token (pub) --

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct FnDecl<'input> {
    pub_kw: PubKw,
    fn_kw: FnKw,
    name: Ident<'input>,
    lbrace: LBrace,
    rbrace: RBrace,
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct StructDecl<'input> {
    pub_kw: PubKw,
    struct_kw: StructKw,
    name: Ident<'input>,
    lbrace: LBrace,
    rbrace: RBrace,
}

// -- Enum with ambiguous first token --

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
enum Declaration<'input> {
    Fn(FnDecl<'input>),
    Struct(StructDecl<'input>),
}

// -- Tests --

#[test]
fn lookahead_parses_fn_decl() {
    let mut input = Input::new("pub fn foo {}");
    let decl = Declaration::parse(&mut input, &WsRules).unwrap();
    assert!(matches!(decl, Declaration::Fn(_)));
}

#[test]
fn lookahead_parses_struct_decl() {
    let mut input = Input::new("pub struct Bar {}");
    let decl = Declaration::parse(&mut input, &WsRules).unwrap();
    assert!(matches!(decl, Declaration::Struct(_)));
}

#[test]
fn lookahead_peek_fn() {
    let input = Input::new("pub fn foo {}");
    assert!(Declaration::peek(&input, &WsRules));
}

#[test]
fn lookahead_peek_struct() {
    let input = Input::new("pub struct Bar {}");
    assert!(Declaration::peek(&input, &WsRules));
}

#[test]
fn lookahead_peek_fails() {
    let input = Input::new("let x = 1;");
    assert!(!Declaration::peek(&input, &WsRules));
}

#[test]
fn lookahead_error_on_mismatch() {
    let mut input = Input::new("pub let x;");
    let err = Declaration::parse(&mut input, &WsRules);
    assert!(err.is_err());
}

#[test]
fn lookahead_first_pattern_fn_decl() {
    // FnDecl: PubKw, FnKw, Ident, LBrace, RBrace -- all terminal
    let pattern = <FnDecl as Parse>::first_pattern();
    assert_eq!(
        pattern,
        r"pub(?:\s+)?fn(?:\s+)?[a-zA-Z_][a-zA-Z0-9_]*(?:\s+)?\{(?:\s+)?\}"
    );
}

#[test]
fn lookahead_first_pattern_struct_decl() {
    let pattern = <StructDecl as Parse>::first_pattern();
    assert_eq!(
        pattern,
        r"pub(?:\s+)?struct(?:\s+)?[a-zA-Z_][a-zA-Z0-9_]*(?:\s+)?\{(?:\s+)?\}"
    );
}

#[test]
fn lookahead_enum_first_pattern_is_alternation() {
    // Declaration enum should wrap each variant's pattern in groups and join with |
    let pattern = <Declaration as Parse>::first_pattern();
    let fn_pattern = <FnDecl as Parse>::first_pattern();
    let struct_pattern = <StructDecl as Parse>::first_pattern();
    let expected = format!("({fn_pattern})|({struct_pattern})");
    assert_eq!(pattern, expected);
}

#[test]
fn lookahead_enum_is_not_terminal() {
    const { assert!(!<Declaration as Parse>::IS_TERMINAL) };
}
