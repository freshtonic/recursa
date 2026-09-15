//! Runtime form of the frozen M3 toy grammar from recursa-spike b410d2f.

recursa::grammar! {
    module = crate::toy,
    keyword_matching = sensitive,
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

recursa::ast_node! {

    #[tok(SEMI)]
    pub struct SemicolonStmt;
}

recursa::ast_node! {

    pub struct IdentHolder {
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
        pub ident: Ident,
    }
}
