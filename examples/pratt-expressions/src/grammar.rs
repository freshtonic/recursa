#![allow(
    dead_code,
    unused_variables,
    clippy::blocks_in_conditions,
    clippy::collapsible_if,
    clippy::needless_borrow,
    clippy::needless_return,
    clippy::possible_missing_else,
    reason = "the generated grammar is checked before its compact token stream is formatted"
)]

recursa::grammar! {
    module = crate::grammar,
    diagrams,
    derives(Visit),
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
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

#[derive(recursa::Node)]
pub struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: IntegerText<'input>,
}

#[derive(recursa::Node)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub text: NameText<'input>,
}

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'input> {
    Integer(Integer<'input>),
    Name(Name<'input>),
    Parenthesized(#[tok(LPAREN, this, RPAREN)] Box<Self>),

    #[parse(prefix, bp = 35)]
    #[tok(MINUS)]
    Negate(Box<Self>),

    #[parse(postfix, bp = 40)]
    #[tok(BANG)]
    Factorial(Box<Self>),

    #[parse(infix, lbp = 30, rbp = 30)]
    #[tok(CARET)]
    Power(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(STAR)]
    Multiply(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),
}
