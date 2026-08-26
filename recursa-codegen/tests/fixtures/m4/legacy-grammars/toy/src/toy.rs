//! Runtime form of the frozen M3 toy grammar from recursa-spike b410d2f.

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

#[derive(recursa::Node)]
#[tok(SEMI)]
pub struct SemicolonStmt;

#[derive(recursa::Node)]
pub struct IdentHolder<'input> {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub ident: Ident<'input>,
}
