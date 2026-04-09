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
    let mut input = Input::<WsRules>::new("pub fn foo {}");
    let decl = Declaration::parse(&mut input).unwrap();
    assert!(matches!(decl, Declaration::Fn(_)));
}

#[test]
fn lookahead_parses_struct_decl() {
    let mut input = Input::<WsRules>::new("pub struct Bar {}");
    let decl = Declaration::parse(&mut input).unwrap();
    assert!(matches!(decl, Declaration::Struct(_)));
}

#[test]
fn lookahead_peek_fn() {
    let input = Input::<WsRules>::new("pub fn foo {}");
    assert!(Declaration::peek(&input));
}

#[test]
fn lookahead_peek_struct() {
    let input = Input::<WsRules>::new("pub struct Bar {}");
    assert!(Declaration::peek(&input));
}

#[test]
fn lookahead_peek_fails() {
    let input = Input::<WsRules>::new("let x = 1;");
    assert!(!Declaration::peek(&input));
}

#[test]
fn lookahead_error_on_mismatch() {
    let mut input = Input::<WsRules>::new("pub let x;");
    let err = Declaration::parse(&mut input);
    assert!(err.is_err());
}

#[test]
fn lookahead_first_patterns_fn_decl() {
    // FnDecl: PubKw, FnKw, Ident, LBrace, RBrace -- all terminal
    let patterns = <FnDecl as Parse>::first_patterns();
    assert_eq!(
        patterns,
        &["pub", "fn", r"[a-zA-Z_][a-zA-Z0-9_]*", r"\{", r"\}"]
    );
}

#[test]
fn lookahead_first_patterns_struct_decl() {
    let patterns = <StructDecl as Parse>::first_patterns();
    assert_eq!(
        patterns,
        &["pub", "struct", r"[a-zA-Z_][a-zA-Z0-9_]*", r"\{", r"\}"]
    );
}

#[test]
fn lookahead_enum_first_patterns_collects_all_variants() {
    // Declaration enum should collect patterns from both FnDecl and StructDecl
    let patterns = <Declaration as Parse>::first_patterns();
    // FnDecl patterns: ["pub", "fn", ident, "{", "}"]
    // StructDecl patterns: ["pub", "struct", ident, "{", "}"]
    assert_eq!(
        patterns,
        &[
            "pub",
            "fn",
            r"[a-zA-Z_][a-zA-Z0-9_]*",
            r"\{",
            r"\}",
            "pub",
            "struct",
            r"[a-zA-Z_][a-zA-Z0-9_]*",
            r"\{",
            r"\}",
        ]
    );
}

#[test]
fn lookahead_enum_is_not_terminal() {
    assert!(!<Declaration as Parse>::IS_TERMINAL);
}
