//! Source-analysis fixture for the optional-qualifier LL(2) grammar.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/optq.rs` and `src/grammars/optq.rs`.

recursa::grammar! {
    module = crate::optq,
    keyword_matching = sensitive,
    max_lookahead = 2,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        DEBUG => "DEBUG",
        PRINT => "PRINT",
        READ => "READ",
        SET => "SET",
    }
}

/// Presence records whether the optional qualifier occurred.
#[derive(recursa::Node)]
#[tok(DEBUG)]
pub struct Debug;

#[derive(recursa::Node)]
pub enum Stmt {
    Print(PrintStmt),
    Read(ReadStmt),
    Set(SetStmt),
}

#[derive(recursa::Node)]
pub struct PrintStmt {
    pub debug: Option<Debug>,
    #[tok(PRINT)]
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub rest: Rest,
}

#[derive(recursa::Node)]
pub struct ReadStmt {
    pub debug: Option<Debug>,
    #[tok(READ)]
    pub rest: Rest,
}

#[derive(recursa::Node)]
pub struct SetStmt {
    pub debug: Option<Debug>,
    #[tok(SET)]
    pub rest: Rest,
}
