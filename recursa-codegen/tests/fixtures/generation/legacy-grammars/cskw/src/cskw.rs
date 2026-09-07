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

#[derive(recursa::Node)]
pub enum Stmt<'input> {
    Select(SelectStmt<'input>),
}

#[derive(recursa::Node)]
pub struct SelectStmt<'input> {
    #[tok(SELECT)]
    #[sep(COMMA)]
    pub cols: recursa::Vec1<Col<'input>>,
    #[tok(FROM)]
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", admits(AS))]
    pub table: TableName<'input>,
}

#[derive(recursa::Node)]
pub struct Col<'input> {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", admits(AS))]
    pub name: ColumnName<'input>,
    pub as_alias: Option<AsAlias<'input>>,
}

#[derive(recursa::Node)]
pub struct AsAlias<'input> {
    #[tok(AS)]
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub alias: Alias<'input>,
}
