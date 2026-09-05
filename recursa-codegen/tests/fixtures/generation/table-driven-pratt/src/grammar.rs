//! The table-driven Pratt fixture: the textbook expression grammar written as a `#[pratt]`
//! enum, with infix `+ - * ^`, prefix `-`, postfix `!`, and one exclusion site, the needle of
//! `position(needle in haystack)`, which restricts the expression language the way gram.y's
//! `b_expr` does inside `POSITION`.
//!
//! The integration test generates this file twice, once as written and once with the parser
//! style flipped to `recursive_descent`, and compares what the two parsers produce. The LR
//! fixture tests lower the same file to assert the precedence resolution directly.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
    parser_style = table_driven,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        POSITION => "position",
        IN => "in",
    }
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        CARET => "^",
        BANG => "!",
        LPAREN => "(",
        RPAREN => ")",
    }
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: IntegerText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub text: NameText<'input>,
}

/// `position(needle in haystack)`: the needle cannot itself be an `in` expression, so the
/// keyword belongs to the enclosing form.
#[derive(recursa::Node, Debug, Clone)]
#[tok(POSITION, LPAREN, this, RPAREN)]
pub struct Position<'input> {
    #[parse(pratt(exclude(In)))]
    pub needle: Box<Expr<'input>>,
    #[tok(IN)]
    pub haystack: Box<Expr<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[pratt]
pub enum Expr<'input> {
    Integer(Integer<'input>),
    Name(Name<'input>),
    Position(Position<'input>),
    Parenthesized(#[tok(LPAREN, this, RPAREN)] Box<Self>),

    #[parse(prefix, bp = 30)]
    #[tok(MINUS)]
    Negate(Box<Self>),

    #[parse(postfix, bp = 50)]
    #[tok(BANG)]
    Factorial(Box<Self>),

    #[parse(infix, lbp = 40, rbp = 40)]
    #[tok(CARET)]
    Power(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(STAR)]
    Multiply(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(MINUS)]
    Subtract(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 5, rbp = 6)]
    #[tok(IN)]
    In(Box<Self>, Box<Self>),
}
