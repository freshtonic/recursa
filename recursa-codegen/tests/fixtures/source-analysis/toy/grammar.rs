//! Source-analysis fixture for the toy grammar.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/toy.rs` and `src/grammars/toy.rs`.

recursa::grammar! {
    module = crate::toy,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation {
        SEMI => ";",
        COMMA => ",",
        LPAREN => "(",
        RPAREN => ")",
    }
}

/// Presence of a semicolon statement; fixed syntax carries no AST text.
#[derive(recursa::Node)]
#[tok(SEMI)]
pub struct SemicolonStmt;

/// Gives the regex-backed `Ident` token a semantic use in this small fixture.
#[derive(recursa::Node)]
pub struct IdentHolder {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub ident: Ident,
}
