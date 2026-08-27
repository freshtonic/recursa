#![allow(
    dead_code,
    clippy::blocks_in_conditions,
    clippy::collapsible_if,
    clippy::needless_borrow,
    clippy::needless_return,
    clippy::possible_missing_else,
    reason = "the generated grammar is checked before its compact token stream is formatted"
)]

recursa::grammar! {
    module = crate::grammar,
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
pub enum Value<'input> {
    Object(Object<'input>),
    Array(Array<'input>),
    String(
        #[lex(pattern = r#""([^"\\\x00-\x1F]|\\(["\\/bfnrt]|u[0-9a-fA-F]{4}))*""#)] String<'input>,
    ),
    Number(#[lex(pattern = r"-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")] Number<'input>),
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
pub struct Object<'input> {
    #[sep(COMMA)]
    pub members: Vec<Member<'input>>,
}

#[derive(recursa::Node)]
pub struct Member<'input> {
    pub key: String<'input>,
    #[tok(COLON)]
    pub value: Value<'input>,
}

#[derive(recursa::Node)]
#[tok(LBRACKET, this, RBRACKET)]
pub struct Array<'input> {
    #[sep(COMMA)]
    pub values: Option<recursa::Vec1<Value<'input>>>,
}
