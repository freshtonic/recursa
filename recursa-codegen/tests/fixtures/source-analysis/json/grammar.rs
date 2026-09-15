//! Source-analysis fixture for the JSON grammar.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/json.rs` and `src/grammars/json.rs`. The string and
//! number patterns are copied exactly from that semantic reference.

recursa::grammar! {
    module = crate::json,
    keyword_matching = sensitive,
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

recursa::ast_node! {

    pub enum Value {
        Object(Object),
        Array(Array),
        String(#[lex(pattern = r#""([^"\\\x00-\x1F]|\\(["\\/bfnrt]|u[0-9a-fA-F]{4}))*""#)] String),
        Number(#[lex(pattern = r"-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")] Number),
        Bool(Bool),
        #[tok(NULL)]
        Null,
    }
}

recursa::ast_node! {

    pub enum Bool {
        #[tok(TRUE)]
        True,
        #[tok(FALSE)]
        False,
    }
}

recursa::ast_node! {

    #[tok(LBRACE, this, RBRACE)]
    pub struct Object {
        #[sep(COMMA)]
        pub members: zero_or_many!(Member),
    }
}

recursa::ast_node! {

    pub struct Member {
        pub key: String,
        #[tok(COLON)]
        pub value: Value,
    }
}

recursa::ast_node! {

    #[tok(LBRACKET, this, RBRACKET)]
    pub struct Array {
        #[sep(COMMA)]
        pub values: zero_or_many!(Value),
    }
}
