recursa::grammar! {
    module = crate::grammar,
    configurations(v1, v2 = feature("since-v2"), v3 = feature("since-v3")),
    derives(Requires),
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\n\r]+",
    }
    categories { UNRESERVED, RESERVED }
    keywords {
        SELECT => "select" in RESERVED,
        FROM => "from" in RESERVED,
        AS => "as" in RESERVED,
        WHERE => "where" in RESERVED,
        #[config(since = v3)]
        NEARBY => "nearby" in RESERVED,
    }
    admissions { Unreserved = UNRESERVED }
    punctuation {
        STAR => "*",
        COMMA => ",",
        LPAREN => "(",
        RPAREN => ")",
    }
}

recursa::ast_node! {
    /// `SELECT * FROM <source> [WHERE <name>]`.
    pub struct Query {
        #[tok(SELECT, STAR, FROM, this)]
        pub source: TableRef,
        pub filter: Option<Filter>,
    }
}

recursa::ast_node! {
    /// A table source: a name, or a parenthesized subquery.
    pub enum TableRef {
        Named(TableName),
        Subquery(boxed!(Subquery)),
    }
}

recursa::ast_node! {
    /// A parenthesized subquery.
    ///
    /// The alias is optional from `v2`; before it the grammar required one, so a missing
    /// alias is what needs `v2`.
    pub struct Subquery {
        #[tok(LPAREN, this, RPAREN)]
        pub query: boxed!(Query),
        #[config(
            since = v2,
            on = absent,
            code = "42601",
            message = "subquery in FROM must have an alias",
            hint = "For example, FROM (SELECT ...) [AS] foo.",
            cite = "grammar.rs, the v2 alias change"
        )]
        pub alias: Option<Alias>,
        // Before v2 the grammar required the alias, so the pre-v2 shape is not optional.
        #[config(before = v2)]
        pub alias: Alias,
    }
}

recursa::ast_node! {
    /// `AS <name>`, and from `v3` also `AS <name>, <name>...`.
    pub struct Alias {
        #[tok(AS, this)]
        pub name: TableName,
        #[config(since = v3)]
        #[tok(COMMA, this)]
        pub columns: Option<ColumnList>,
    }
}

recursa::ast_node! {
    /// One or more further names after an alias.
    pub struct ColumnList {
        #[sep(COMMA)]
        pub names: one_or_many!(TableName),
    }
}

recursa::ast_node! {
    /// A bare table or column name.
    pub struct TableName {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(Unreserved))]
        pub text: Word,
    }
}

recursa::ast_node! {
    /// `WHERE <name>`, and from `v3` also `WHERE NEARBY <name>`.
    pub enum Filter {
        Plain(PlainFilter),
        #[config(since = v3)]
        Nearby(NearbyFilter),
    }
}

recursa::ast_node! {
    /// `WHERE <name>`.
    pub struct PlainFilter {
        #[tok(WHERE, this)]
        pub name: TableName,
    }
}

recursa::ast_node! {
    /// `WHERE NEARBY <name>`, added in `v3`.
    #[config(since = v3)]
    pub struct NearbyFilter {
        #[tok(WHERE, NEARBY, this)]
        pub name: TableName,
    }
}
