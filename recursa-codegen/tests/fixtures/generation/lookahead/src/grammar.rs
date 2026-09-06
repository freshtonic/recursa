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
//! `ExprRule` is gram.y's `a_expr` shape: a `#[pratt]` enum with prefix `NOT`, infix `LIKE`
//! and the tests `NOT LIKE`, `NOT BETWEEN` and `NOT IN`, over a name that admits `like` and
//! `between` the way `ColId` admits `BETWEEN` and `func_name` admits `LIKE`. The operand of
//! prefix `NOT` can therefore begin with a trigger or with anything else, so the rule is
//! emitted twice, `NOT expr | NOT_LA expr`, as gram.y writes `NOT a_expr | NOT_LA a_expr` by
//! hand; the filter itself is what lets `a NOT LIKE b` be a test while `NOT a` is a negation.
//!
//! The integration test generates this file as written, with the parser style flipped to
//! `recursive_descent`, and with the `lookahead` block removed, and compares what the parsers
//! produce.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 3,
    parsers(rd = recursive_descent, lr = table_driven),
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

/// `expr ;`, the root of the expression language.
#[derive(recursa::Node, Debug, Clone)]
#[tok(this, SEMI)]
pub struct ExprRule<'input> {
    pub expr: Expr<'input>,
}

/// gram.y's `a_expr` shape: `NOT` binds weaker than every test, so `NOT a LIKE b` is
/// `NOT (a LIKE b)` and `NOT a NOT LIKE b` is `NOT (a NOT LIKE b)`; the tests and `LIKE`
/// share one level.
#[derive(recursa::Node, Debug, Clone)]
#[pratt]
pub enum Expr<'input> {
    Ident(Ident<'input>),
    Number(Number<'input>),
    Parenthesized(#[tok(LPAREN, this, RPAREN)] Box<Self>),

    /// `NOT expr`, pg-sql's `Expr::Not`: the operand can begin with `like` or `between` as a
    /// name, so the lowering twins the rule with `NOT_LA`.
    #[parse(prefix, bp = 10)]
    Not(#[tok(NOT, this)] Box<Self>),

    #[parse(postfix, bp = 20)]
    NotLike(Box<Self>, #[tok(NOT, LIKE, this)] Box<Self>),

    #[parse(postfix, bp = 20)]
    NotBetween(Box<Self>, #[tok(NOT, BETWEEN, this)] Box<Self>, #[tok(AND, this)] Box<Self>),

    #[parse(postfix, bp = 20)]
    NotIn(Box<Self>, #[tok(NOT, IN, LPAREN, this, RPAREN)] Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(LIKE)]
    Like(Box<Self>, Box<Self>),

    #[parse(postfix, bp = 20)]
    Between(Box<Self>, #[tok(BETWEEN, this)] Box<Self>, #[tok(AND, this)] Box<Self>),
}

/// A name of the expression language, admitting the keywords `like` and `between`.
#[derive(recursa::Node, Debug, Clone)]
pub struct Ident<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(LIKE, BETWEEN))]
    pub text: IdentText<'input>,
}
