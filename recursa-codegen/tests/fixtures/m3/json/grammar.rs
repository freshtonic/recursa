//! M3 source fixture for the JSON grammar.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/json.rs` and `src/grammars/json.rs`. The string and
//! number patterns are copied exactly from that semantic reference.

recursa::grammar! {
    module = crate::json,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        TRUE => "true",
        FALSE => "false",
        NULL => "null",
    }
    punctuation {
        LBRACE => "{",
        RBRACE => "}",
        LBRACKET => "[",
        RBRACKET => "]",
        COMMA => ",",
        COLON => ":",
    }
}

#[derive(recursa::Node)]
pub enum Value {
    Object(Object),
    Array(Array),
    String(#[lex(pattern = r#""([^"\\\x00-\x1F]|\\(["\\/bfnrt]|u[0-9a-fA-F]{4}))*""#)] String),
    Number(#[lex(pattern = r"-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")] Number),
    Bool(Bool),
    #[tok(NULL)]
    Null,
}

#[derive(recursa::Node)]
pub enum Bool {
    #[tok(TRUE)]
    True,
    #[tok(FALSE)]
    False,
}

#[derive(recursa::Node)]
#[tok(LBRACE, this, RBRACE)]
pub struct Object {
    #[sep(COMMA)]
    pub members: Vec<Member>,
}

#[derive(recursa::Node)]
pub struct Member {
    pub key: String,
    #[tok(COLON)]
    pub value: Value,
}

#[derive(recursa::Node)]
#[tok(LBRACKET, this, RBRACKET)]
pub struct Array {
    #[sep(COMMA)]
    pub values: Vec<Value>,
}
