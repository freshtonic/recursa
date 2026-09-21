//! Source-analysis fixture for restricted expressions (`docs/declared-precedence.md`,
//! "Restricted expressions").
//!
//! `BExpr` is `gram.y`'s `b_expr`: the expression language without the boolean-keyword
//! productions, so that the `AND` of `a BETWEEN b AND c` closes the `BETWEEN` clause instead of
//! opening a boolean operand. Declared precedence alone cannot express that, exactly as
//! `gram.y:14762-14766` records; the restriction is written by hand, and the parenthesized atom
//! keeps the full language, as `b_expr: '(' a_expr ')'` does.
//!
//! `Action` is a `leading` restriction: the position after `do` excludes a leading `(`, as
//! `gram.y`'s single rule action does because `(` there opens the action list. Only the operand
//! that leads a rule is restricted, so `do a + (b)` is an action and `do (a) + b` is not.

recursa::grammar! {
    module = crate::restricted,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        BETWEEN => "between",
        AND => "and",
        DO => "do",
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

recursa::ast_node! {
    /// An expression that cannot lead with `(`. Its right operand keeps the full language.
    #[restricts(Expr, leading)]
    pub enum Action {
        Name,
        Add,
    }
}

recursa::ast_node! {
    /// `do action` or the parenthesized list `do ( expr )`, which owns the leading `(`.
    pub enum Do {
        One(#[tok(DO, this)] boxed!(Action)),
        List(#[tok(DO, LPAREN, this, RPAREN)] boxed!(Expr)),
    }
}
