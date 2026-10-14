//! The merged-kind enclosure grammar: a parenthesized set query whose closing delimiter names a
//! `lookahead { ... }` merged kind beside the token it merges, which is the shape Pretty's
//! precedence repair has to read as fixed syntax (#142).
//!
//! `RPAREN_SELECT_LA` marks a `)` that a set operator follows, which is what lets a completed
//! parenthesized query part from a grouped set query on one token. The `)` ends
//! `ParenthesizedQuery`'s rule, so the rule itself says nothing about what follows it and the
//! substitution pass, which reads `FIRST` of the rest of the rule only, cannot reach the
//! position: the grammar names the merged kind at the site, as gram.y writes
//! `any_with: WITH | WITH_LA` by hand (#139). Both arms of `QueryClose` render `)`, because a
//! merged kind is the same record read differently and keeps the source token's spelling.
//!
//! `Query` declares precedence and has operator variants, so Pretty must find an atom that
//! encloses `Self` between fixed tokens to repair precedence with parentheses.
//! `ParenthesizedQuery` is that atom, and it is one only if the two-armed `QueryClose` counts as
//! the fixed syntax `)`.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        UNION => "union",
        EXCEPT => "except",
        INTERSECT => "intersect",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        SEMI => ";",
    }
    lookahead { RPAREN_SELECT_LA = RPAREN before { UNION, EXCEPT, INTERSECT } }
    precedence {
        left(bp = 10) { UNION, EXCEPT },
        left(bp = 20) { INTERSECT },
    }
}

recursa::ast_node! {
    /// Required opening delimiter of a parenthesized query.
    #[derive(Debug, Clone)]
    pub enum QueryOpen {
        #[tok(LPAREN)]
        Value,
    }
}

recursa::ast_node! {
    /// Required closing delimiter of a parenthesized query, pg-sql's `ParenthesizedClose`: one
    /// arm per close kind the feed can produce here, both rendering `)`.
    #[derive(Debug, Clone)]
    pub enum QueryClose {
        #[tok(RPAREN)]
        Plain,
        #[tok(RPAREN_SELECT_LA)]
        Merged,
    }
}

recursa::ast_node! {
    /// The grouping atom: a query enclosed between the two delimiter Nodes.
    #[derive(Debug, Clone)]
    pub struct ParenthesizedQuery {
        pub open: QueryOpen,
        pub query: boxed!(Query),
        pub close: QueryClose,
    }
}

recursa::ast_node! {
    /// A relation name.
    #[derive(Debug, Clone)]
    pub struct Name {
        #[lex(pattern = r"[a-z_][a-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {
    /// PostgreSQL's `select_clause` shape: left-associative set operators over a name or a
    /// parenthesized query, with `INTERSECT` one level above `UNION` and `EXCEPT`.
    #[derive(Debug, Clone)]
    pub enum Query {
        Name(Name),
        Parenthesized(ParenthesizedQuery),
        Union(boxed!(Self), #[tok(UNION, this)] boxed!(Self)),
        Except(boxed!(Self), #[tok(EXCEPT, this)] boxed!(Self)),
        Intersect(boxed!(Self), #[tok(INTERSECT, this)] boxed!(Self)),
    }
}

recursa::ast_node! {
    /// `query ;`, the root of the language.
    #[derive(Debug, Clone)]
    #[tok(this, SEMI)]
    pub struct Statement {
        pub query: Query,
    }
}
