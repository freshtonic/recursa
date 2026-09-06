//! The Pratt shapes gram.y writes that the attribute forms did not admit before #122, #123 and
//! #124, each generated once with both parsers so that the twin test can compare them:
//!
//! - `a like b [escape c]`: an optional operand attached behind a fixed token on an infix
//!   variant (`#[tok(ESCAPE, this)] Option<Box<Self>>`), gram.y's
//!   `a_expr LIKE a_expr ESCAPE a_expr %prec LIKE` with `%nonassoc ESCAPE` just above `LIKE`;
//!   `a not like b [escape c]` is the same operand on pg-sql's postfix spelling (#123).
//! - `@ a`: a content token, the user-defined operator class, as the operator of a prefix
//!   variant with a binding power, gram.y's `qual_Op a_expr %prec Op`; `a ~@ b` is the same
//!   class as an infix operator (#124).
//!
//! The integration test generates this file once and compares what the two parsers of the
//! generation produce; the LR fixture tests lower the same file to assert the rules directly.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
    parsers(rd = recursive_descent, lr = table_driven),
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        LIKE => "like",
        ESCAPE => "escape",
        NOT => "not",
        AND => "and",
    }
    punctuation {
        PLUS => "+",
        STAR => "*",
        LPAREN => "(",
        RPAREN => ")",
    }
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub text: NameText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
#[pratt]
pub enum Expr<'input> {
    Name(Name<'input>),
    Parenthesized(#[tok(LPAREN, this, RPAREN)] Box<Self>),

    /// `a like b [escape c]`: the escape operand is optional and attached behind `escape`, so
    /// it takes the operand minimum `rbp` in both styles and, in table mode, `escape` shifts
    /// against the variant's own reduce (the variant's level plus one), as gram.y's
    /// `%nonassoc ESCAPE` above `%nonassoc LIKE` decides it.
    #[parse(infix, lbp = 6, rbp = 7)]
    Like(
        Box<Self>,
        #[tok(LIKE, this)] Box<Self>,
        /// Greedy: `escape` also follows an expression (the operand before it is one), and on
        /// `escape` the parser commits to this operand, as gram.y's `%nonassoc ESCAPE` shifts.
        #[greedy(ESCAPE)]
        #[tok(ESCAPE, this)]
        Option<Box<Self>>,
    ),

    /// The same operand on the postfix spelling pg-sql uses: every tail operand takes `bp` as
    /// its minimum, the attached optional one included.
    #[parse(postfix, bp = 6)]
    NotLike(
        Box<Self>,
        #[tok(NOT, LIKE, this)] Box<Self>,
        /// Greedy: as on `Like`.
        #[greedy(ESCAPE)]
        #[tok(ESCAPE, this)]
        Option<Box<Self>>,
    ),

    /// `@ a`: the operator is a content token, so the nud dispatch keys on its kind and the
    /// rule takes `bp` as its precedence, as gram.y's `qual_Op a_expr %prec Op`: the operand
    /// binds `+` and `*` (above 8) and stops before `like` and `and` (below 8).
    #[parse(prefix, bp = 8)]
    CustomPrefix(#[lex(pattern = r"[~@#]+")] OperatorText<'input>, Box<Self>),

    /// `a ~@ b`: the same operator class as an infix operator, gram.y's
    /// `a_expr qual_Op a_expr %prec Op`.
    #[parse(infix, lbp = 8, rbp = 9)]
    CustomInfix(Box<Self>, OperatorText<'input>, Box<Self>),

    #[parse(infix, lbp = 3, rbp = 4)]
    #[tok(AND)]
    And(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(STAR)]
    Multiply(Box<Self>, Box<Self>),
}
