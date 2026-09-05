//! The table-driven Pratt fixture: the textbook expression grammar written as a `#[pratt]`
//! enum, with infix `+ - * ^` and `in`, prefix `-`, postfix `!`, the interleaved
//! `a between b and c` (pg-sql's attachment form) and `a from b for c` (bare operands), the
//! delimited `a[b]`, and one exclusion site, the needle of `position(needle in haystack)`,
//! which restricts the expression language the way gram.y's `b_expr` does inside `POSITION`.
//!
//! The integration test generates this file twice, once as written and once with the parser
//! style flipped to `recursive_descent`, and compares what the two parsers produce. The LR
//! fixture tests lower the same file to assert the precedence resolution directly.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
    parser_style = table_driven,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        POSITION => "position",
        IN => "in",
        BETWEEN => "between",
        AND => "and",
        FROM => "from",
        FOR => "for",
    }
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        CARET => "^",
        BANG => "!",
        LPAREN => "(",
        RPAREN => ")",
        LBRACKET => "[",
        RBRACKET => "]",
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

    /// An interleaved shape, written as pg-sql writes it: each operand carries the fixed token
    /// before it, so its own reduce decides where the operand stops (before any operator
    /// binding weaker than `between`); inside an exclusion clone both operands carry the
    /// exclusion through their cloned attachments, as the recursive-descent operands do.
    #[parse(postfix, bp = 8)]
    Between(Box<Self>, #[tok(BETWEEN, this)] Box<Self>, #[tok(AND, this)] Box<Self>),

    /// An interleaved shape with bare operands: the middle one is parsed with the postfix
    /// binding power as its minimum, so it stops before `in`; the last extends by precedence.
    #[parse(postfix, bp = 7)]
    Slice(Box<Self>, #[tok(FROM)] Box<Self>, #[tok(FOR)] Box<Self>),

    /// A delimited postfix operand, parsed with the postfix binding power as its minimum: only
    /// `!` and another index may extend it, `a[b + c]` is an error in both styles.
    #[parse(postfix, bp = 50)]
    Index(Box<Self>, #[tok(LBRACKET, this, RBRACKET)] Box<Self>),

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
