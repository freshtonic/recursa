//! The lookahead-filter grammar: PostgreSQL's `NOT BETWEEN`, `NOT IN` and `NOT LIKE` tests
//! beside unary `NOT cond` and a conjunction `name NOT cond` ("name and not cond").
//!
//! The name before `NOT BETWEEN` is a `Subject` and the name before `NOT cond` an `Operand`,
//! two nonterminals an LALR(1) parser must reduce before it shifts `NOT`, which one token of
//! lookahead cannot decide: without the filter the construction has a reduce/reduce conflict on
//! `NOT`. The `lookahead` declaration makes the token feed read `NOT` before `BETWEEN`, `IN` or
//! `LIKE` as `NOT_LA`, the token the test rules shift, so `Subject` reduces on `NOT_LA` and
//! `Operand` on `NOT`.
//!
//! Recursive descent decides `Cond` by its third token and never at a caller-FOLLOW boundary:
//! its dispatch commits to a shorter alternative as soon as the lookahead is in the caller
//! FOLLOW, so the plain juxtaposition `cond cond` with unary `NOT` would make it read `a not
//! between` as `a` and `not between`. That boundary is the Pratt loop's job in pg-sql.
//!
//! The integration test generates this file as written, with the parser style flipped to
//! `recursive_descent`, and with the `lookahead` block removed, and compares what the parsers
//! produce.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 3,
    parser_style = table_driven,
    derives(Pretty),
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
    pub cond: Cond<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Cond<'input> {
    Not {
        mark: NotMark,
        inner: Box<Cond<'input>>,
    },
    NotBetween(NotBetween<'input>),
    NotIn(NotIn<'input>),
    NotLike(NotLike<'input>),
    Between(Between<'input>),
    AndNot(AndNot<'input>),
    Name { name: Name<'input> },
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(NOT)]
pub struct NotMark;

/// The left side of a test operator.
#[derive(recursa::Node, Debug, Clone)]
pub struct Subject<'input> {
    pub name: Name<'input>,
}

/// The left operand of a conjunction with a negated condition.
#[derive(recursa::Node, Debug, Clone)]
pub struct Operand<'input> {
    pub name: Name<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct NotBetween<'input> {
    pub subject: Subject<'input>,
    #[tok(NOT, BETWEEN)]
    pub low: Number<'input>,
    #[tok(AND)]
    pub high: Number<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(this, RPAREN)]
pub struct NotIn<'input> {
    pub subject: Subject<'input>,
    #[tok(NOT, IN, LPAREN)]
    pub member: Number<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct NotLike<'input> {
    pub subject: Subject<'input>,
    #[tok(NOT, LIKE)]
    pub pattern: Name<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Between<'input> {
    pub subject: Subject<'input>,
    #[tok(BETWEEN)]
    pub low: Number<'input>,
    #[tok(AND)]
    pub high: Number<'input>,
}

/// `name NOT cond`: the name and the negation of the condition.
#[derive(recursa::Node, Debug, Clone)]
pub struct AndNot<'input> {
    pub operand: Operand<'input>,
    #[tok(NOT)]
    pub inner: Box<Cond<'input>>,
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
