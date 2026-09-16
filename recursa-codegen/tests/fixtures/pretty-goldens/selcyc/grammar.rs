//! Pretty-golden fixture for the unified parenthesized-content shape (issue #130).
//!
//! Mirrors PostgreSQL `gram.y`'s `select_with_parens: '(' select_no_parens ')'
//! | '(' select_with_parens ')'` (gram.y:12682-12685) held behind one
//! parenthesized-content nonterminal, which is the shape pg-sql needs in order
//! to unify `select_clause`. The grouping atom `Expr::Paren` holds `ParenContent`,
//! whose `Exprs` variant supplies the only grouping witness while its `Query`
//! variant re-enters `Query` through `QueryParens`. That cycle owns no `Expr`,
//! so Pretty's grouping proof must keep the sibling's witness rather than
//! reporting RCA3101.
//!
//! The nested query form is spelled with brackets rather than a second pair of
//! parentheses: `'(' select ...` and `'(' '(' ...` share an unbounded prefix
//! that no lookahead resolves, and that admission question is not the grouping
//! question this fixture pins. The grouping proof counts required fixed tokens
//! and never reads their spelling, so the substitution leaves it untouched.

recursa::grammar! {
    module = crate::selcyc,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        SELECT => "select",
    }
    punctuation {
        PLUS => "+",
        STAR => "*",
        LPAREN => "(",
        RPAREN => ")",
        LBRACKET => "[",
        RBRACKET => "]",
        COMMA => ",",
    }
    precedence {
        left(bp = 10) { PLUS },
        left(bp = 20) { STAR },
    }
}

recursa::ast_node! {
    /// Declares the integer token type used by the expression atom.

    pub struct IntAtom {
        #[lex(pattern = r"[0-9]+")]
        pub value: IntLit,
    }
}

recursa::ast_node! {
    /// gram.y's `simple_select`: a target list whose entries are expressions.

    #[tok(SELECT)]
    pub struct Select {
        #[sep(COMMA)]
        pub targets: one_or_many!(Expr),
    }
}

recursa::ast_node! {
    /// gram.y's `select_with_parens: '(' select_with_parens ')'`, the self-cycle.

    pub struct QueryParens {
        #[tok(LBRACKET, this, RBRACKET)]
        pub query: boxed!(Query),
    }
}

recursa::ast_node! {
    /// gram.y's `select_with_parens`/`select_no_parens` pair as one nonterminal.

    pub enum Query {
        Select(Select),
        Parens(QueryParens),
    }
}

recursa::ast_node! {
    /// The unified content of one pair of parentheses: gram.y's `c_expr: '(' a_expr
    /// ')'` and `c_expr: select_with_parens` merged so descent commits at `(`.

    pub enum ParenContent {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Query(boxed!(Query)),
    }
}

recursa::ast_node! {
    /// The grouping atom: the only enclosure Pretty can repair precedence with.

    pub struct ParenExpr {
        #[tok(LPAREN, this, RPAREN)]
        pub content: ParenContent,
    }
}

recursa::ast_node! {
    /// Expressions with the grouping atom reached through the cyclic content enum.

    pub enum Expr {
        IntLit(IntAtom),
        Paren(ParenExpr),

        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),

        Mul(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),
    }
}
