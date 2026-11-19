//! Pretty-golden fixture for retained comments (issue #99).
//!
//! The grammar retains whitespace, line comments and nested block comments as classified trivia
//! and declares which kinds are comments. Its shapes put comments before repetition separators,
//! before optional occurrences that are present and absent, inside nested Nodes, at the beginning
//! and end of input, and in a document of only comments. The tight `.` separator gives a boundary
//! that layout never breaks, where a line comment is a layout conflict.

recursa::grammar! {
    module = crate::comments,
    keyword_matching = sensitive,
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\r\n]+",
        #[comment(line)]
        LineComment => r"--[^\n]*",
        #[comment(block)]
        BlockComment => nested(opener = "/*", closer = "*/"),
    }
    keywords {
        SELECT => "select",
        FROM => "from",
        WHERE => "where",
    }
    punctuation {
        COMMA => ",",
        SEMI => ";",
        DOT => ".", spacing = tight,
        LPAREN => "(",
        RPAREN => ")",
        EQ => "=", spacing = around,
    }
}

recursa::ast_node! {
    /// Zero or more statements, so that a document may hold only comments.
    #[derive(Debug)]
    pub struct Script {
        #[sep(SEMI, trailing)]
        pub statements: zero_or_many!(Select),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(SELECT, this)]
    pub struct Select {
        #[sep(COMMA)]
        pub targets: one_or_many!(Target),
        #[pretty(break_before = soft)]
        pub from: Option<FromClause>,
        #[pretty(break_before = soft)]
        pub filter: Option<WhereClause>,
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Target {
        pub path: Path,
        pub args: Option<Args>,
    }
}

recursa::ast_node! {
    /// A nested, delimited Node.
    #[derive(Debug)]
    #[tok(LPAREN, this, RPAREN)]
    pub struct Args {
        #[sep(COMMA)]
        pub values: zero_or_many!(Path),
    }
}

recursa::ast_node! {
    /// Names joined by the tight `.` separator.
    #[derive(Debug)]
    pub struct Path {
        #[sep(DOT)]
        pub parts: one_or_many!(Name),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Name {
        #[lex(pattern = r"[a-z_][a-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(FROM, this)]
    pub struct FromClause {
        pub table: Path,
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(WHERE, this)]
    pub struct WhereClause {
        pub left: Path,
        #[tok(EQ)]
        pub right: Path,
    }
}
