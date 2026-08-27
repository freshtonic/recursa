//! Source-analysis fixture for context-sensitive keyword reclamation.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/cskw.rs` and `src/grammars/cskw.rs`. The spike's text
//! predicates are transformed into concrete keyword kinds and parser target
//! kind sets while preserving its accepted language.

recursa::grammar! {
    module = crate::cskw,
    keyword_matching = ascii_insensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        SELECT => "SELECT",
        FROM => "FROM",
        AS => "AS",
    }
    punctuation {
        COMMA => ",",
    }
    lexer_tokens {
        Word => r"[a-zA-Z_][a-zA-Z0-9_]*",
    }
}

#[derive(recursa::Node)]
pub enum Stmt {
    Select(SelectStmt),
}

#[derive(recursa::Node)]
pub struct SelectStmt {
    #[tok(SELECT)]
    #[sep(COMMA)]
    pub cols: recursa::Vec1<Col>,
    #[tok(FROM)]
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", admits(AS))]
    pub table: TableName,
}

#[derive(recursa::Node)]
pub struct Col {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", admits(AS))]
    pub name: ColumnName,
    pub as_alias: Option<AsAlias>,
}

#[derive(recursa::Node)]
pub struct AsAlias {
    #[tok(AS)]
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub alias: Alias,
}
