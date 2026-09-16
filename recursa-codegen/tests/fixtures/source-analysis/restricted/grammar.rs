//! Source-analysis fixture for restricted expressions (`docs/declared-precedence.md`,
//! "Restricted expressions").
//!
//! `BExpr` is `gram.y`'s `b_expr`: the expression language without the boolean-keyword
//! productions, so that the `AND` of `a BETWEEN b AND c` closes the `BETWEEN` clause instead of
//! opening a boolean operand. Declared precedence alone cannot express that, exactly as
//! `gram.y:14762-14766` records; the restriction is written by hand, and the parenthesized atom
//! keeps the full language, as `b_expr: '(' a_expr ')'` does.

recursa::grammar! {
    module = crate::restricted,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        BETWEEN => "between",
        AND => "and",
    }
    punctuation {
        PLUS => "+",
        LPAREN => "(",
        RPAREN => ")",
    }
    precedence {
        left(bp = 10) { AND },
        nonassoc(bp = 20) { BETWEEN },
        left(bp = 30) { PLUS },
    }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[a-z_][a-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {

    pub enum Expr {
        Name(Name),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),
        And(boxed!(Self), #[tok(AND, this)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
        Between(
            boxed!(Self),
            #[tok(BETWEEN, this)] boxed!(BExpr),
            #[tok(AND, this)] boxed!(Self),
        ),
    }
}

recursa::ast_node! {
    /// `gram.y` `b_expr`: `a_expr` without the boolean-keyword productions.

    #[restricts(Expr)]
    pub enum BExpr {
        Name,
        Parenthesized,
        Add,
    }
}
