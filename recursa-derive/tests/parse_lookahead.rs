#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::Parse;

#[derive(Parse, Debug)]
#[parse(pattern = "fn")]
struct FnKw;

#[derive(Parse, Debug)]
#[parse(pattern = "struct")]
struct StructKw;

#[derive(Parse, Debug)]
#[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Parse, Debug)]
#[parse(pattern = r"\{")]
struct LBrace;

#[derive(Parse, Debug)]
#[parse(pattern = r"\}")]
struct RBrace;

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct FnDecl<'input> {
    fn_kw: FnKw,
    name: Ident<'input>,
    lbrace: LBrace,
    rbrace: RBrace,
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct StructDecl<'input> {
    struct_kw: StructKw,
    name: Ident<'input>,
    lbrace: LBrace,
    rbrace: RBrace,
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
enum Declaration<'input> {
    Fn(FnDecl<'input>),
    Struct(StructDecl<'input>),
}

#[test]
fn lookahead_parses_fn_decl() {
    let mut input = Input::new("fn foo {}");
    let decl = Declaration::parse::<WsRules>(&mut input).unwrap();
    assert!(matches!(decl, Declaration::Fn(_)));
}

#[test]
fn lookahead_parses_struct_decl() {
    let mut input = Input::new("struct Bar {}");
    let decl = Declaration::parse::<WsRules>(&mut input).unwrap();
    assert!(matches!(decl, Declaration::Struct(_)));
}

#[test]
fn lookahead_peek_fn() {
    let input = Input::new("fn foo {}");
    assert!(Declaration::peek::<WsRules>(&input));
}

#[test]
fn lookahead_peek_struct() {
    let input = Input::new("struct Bar {}");
    assert!(Declaration::peek::<WsRules>(&input));
}

#[test]
fn lookahead_peek_fails() {
    let input = Input::new("let x = 1;");
    assert!(!Declaration::peek::<WsRules>(&input));
}

#[test]
fn lookahead_error_on_mismatch() {
    let mut input = Input::new("let x;");
    let err = Declaration::parse::<WsRules>(&mut input);
    assert!(err.is_err());
}
