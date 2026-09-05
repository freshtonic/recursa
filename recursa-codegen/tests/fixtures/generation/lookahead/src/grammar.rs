//! The lookahead-filter grammar: PostgreSQL's `NOT BETWEEN` shape beside unary `NOT cond`, with
//! two conditions juxtaposed so that a bare `NOT` can follow a condition.
//!
//! After `name`, an LALR(1) parser must either reduce the condition (a unary `NOT` follows) or
//! shift `NOT` (a `NOT BETWEEN`, `NOT IN` or `NOT LIKE` test follows); one token of lookahead
//! cannot tell. The `lookahead` declaration makes the token feed read `NOT` before `BETWEEN`,
//! `IN` or `LIKE` as `NOT_LA`, which is the token the test rules shift.
//!
//! The integration test generates this file as written, with the parser style flipped to
//! `recursive_descent`, and with the `lookahead` block removed, and compares what the parsers
//! produce.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 3,
    parser_style = table_driven,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        NOT => "not",
        BETWEEN => "between",
        AND => "and",
        IN => "in",
        LIKE => "like",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        SEMI => ";",
    }
    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(this, SEMI)]
pub struct Rule<'input> {
    pub guard: Cond<'input>,
    pub action: Cond<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Cond<'input> {
    Not {
        mark: NotMark,
        inner: Box<Cond<'input>>,
    },
    Test(Test<'input>),
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(NOT)]
pub struct NotMark;

#[derive(recursa::Node, Debug, Clone)]
pub enum Test<'input> {
    NotBetween(NotBetween<'input>),
    NotIn(NotIn<'input>),
    NotLike(NotLike<'input>),
    Between(Between<'input>),
    Name { name: Name<'input> },
}

#[derive(recursa::Node, Debug, Clone)]
pub struct NotBetween<'input> {
    pub subject: Name<'input>,
    #[tok(NOT, BETWEEN)]
    pub low: Number<'input>,
    #[tok(AND)]
    pub high: Number<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(this, RPAREN)]
pub struct NotIn<'input> {
    pub subject: Name<'input>,
    #[tok(NOT, IN, LPAREN)]
    pub member: Number<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct NotLike<'input> {
    pub subject: Name<'input>,
    #[tok(NOT, LIKE)]
    pub pattern: Name<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Between<'input> {
    pub subject: Name<'input>,
    #[tok(BETWEEN)]
    pub low: Number<'input>,
    #[tok(AND)]
    pub high: Number<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub text: NameText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Number<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: NumberText<'input>,
}
