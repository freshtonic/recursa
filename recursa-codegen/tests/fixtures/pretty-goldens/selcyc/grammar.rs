//! Pretty-golden fixture for the unified parenthesized-content shape (issue #130).
//!
//! Mirrors PostgreSQL `gram.y`'s `select_with_parens: '(' select_no_parens ')'
//! | '(' select_with_parens ')'` (gram.y:12682-12685) held behind one
//! parenthesized-content nonterminal, which is the shape pg-sql needs in order
//! to unify `select_clause`. The Pratt atom `Expr::Paren` holds `ParenContent`,
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
}

/// Declares the integer token type used by the expression atom.
#[derive(recursa::Node)]
pub struct IntAtom {
    #[lex(pattern = r"[0-9]+")]
    pub value: IntLit,
}

/// gram.y's `simple_select`: a target list whose entries are expressions.
#[derive(recursa::Node)]
#[tok(SELECT)]
pub struct Select {
    #[sep(COMMA)]
    pub targets: recursa::Vec1<Expr>,
}

/// gram.y's `select_with_parens: '(' select_with_parens ')'`, the self-cycle.
#[derive(recursa::Node)]
pub struct QueryParens {
    #[tok(LBRACKET, this, RBRACKET)]
    pub query: Box<Query>,
}

/// gram.y's `select_with_parens`/`select_no_parens` pair as one nonterminal.
#[derive(recursa::Node)]
pub enum Query {
    Select(Select),
    Parens(QueryParens),
}

/// The unified content of one pair of parentheses: gram.y's `c_expr: '(' a_expr
/// ')'` and `c_expr: select_with_parens` merged so descent commits at `(`.
#[derive(recursa::Node)]
pub enum ParenContent {
    Exprs(#[sep(COMMA)] recursa::Vec1<Expr>),
    Query(Box<Query>),
}

/// The Pratt grouping atom: the only enclosure Pretty can repair precedence with.
#[derive(recursa::Node)]
pub struct ParenExpr {
    #[tok(LPAREN, this, RPAREN)]
    pub content: ParenContent,
}

/// Expressions with the grouping atom reached through the cyclic content enum.
#[derive(recursa::Node)]
#[pratt]
pub enum Expr {
    IntLit(IntAtom),
    Paren(ParenExpr),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(STAR)]
    Mul(Box<Self>, Box<Self>),
}
