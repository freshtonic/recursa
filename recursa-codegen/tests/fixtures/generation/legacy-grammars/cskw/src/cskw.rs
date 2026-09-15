//! Runtime form of the frozen M3 contextual-keyword grammar from recursa-spike b410d2f.

recursa::grammar! {
    module = crate::cskw,
    keyword_matching = ascii_insensitive,
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

recursa::ast_node! {

    pub enum Stmt {
        Select(SelectStmt),
    }
}

recursa::ast_node! {

    pub struct SelectStmt {
        #[tok(SELECT)]
        #[sep(COMMA)]
        pub cols: one_or_many!(Col),
        #[tok(FROM)]
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", admits(AS))]
        pub table: TableName,
    }
}

recursa::ast_node! {

    pub struct Col {
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", admits(AS))]
        pub name: ColumnName,
        pub as_alias: Option<AsAlias>,
    }
}

recursa::ast_node! {

    pub struct AsAlias {
        #[tok(AS)]
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
        pub alias: Alias,
    }
}
