recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        SELECT => "SELECT",
        LIKE => "LIKE",
        ESCAPE => "ESCAPE",
    }
    punctuation {
        BANG => "!",
        SEMI => ";",
    }
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(SELECT)]
pub struct Select;

#[derive(recursa::Node, Debug, Clone)]
#[tok(BANG)]
pub struct Bang;

#[derive(recursa::Node, Debug, Clone)]
pub struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: IntegerText<'input>,
}

/// The optional `SELECT` and the `SELECT` separator of the `Query` repetition share one
/// kind. The parser commits to the optional element when it sees `SELECT`.
#[derive(recursa::Node, Debug, Clone)]
pub struct Item {
    #[greedy(SELECT)]
    pub maybe: Option<Select>,
}

/// The repetition itself may consume a `SELECT` that a caller could have continued with;
/// `all` accepts whatever the overlap is.
#[derive(recursa::Node, Debug, Clone)]
pub struct Query {
    #[greedy(all)]
    #[sep(SELECT)]
    pub items: Vec<Item>,
}

#[derive(recursa::Node, Debug, Clone)]
#[pratt]
pub enum Expr<'input> {
    Integer(Integer<'input>),

    #[parse(postfix, bp = 10)]
    #[tok(BANG)]
    Factorial(Box<Self>),

    /// The pattern is an `Integer` rather than an operand so that neither `!` nor
    /// `ESCAPE` reaches `FOLLOW(Expr)` through it; the escape operand alone carries the
    /// continuation of this tail.
    #[parse(postfix, bp = 5)]
    Like(
        Box<Self>,
        #[tok(LIKE, this)] Integer<'input>,
        Option<EscapeClause<'input>>,
    ),
}

/// The escape operand ends the `Like` tail, so every extender of `Expr` may follow it as
/// the continuation of the enclosing expression. Analysis reports that continuation at
/// `char`, and the exact acceptance names both extenders: the expression keeps extending
/// on `!` and `LIKE` inside the escape instead of reporting `RCA4102`.
#[derive(recursa::Node, Debug, Clone)]
pub struct EscapeClause<'input> {
    #[greedy(LIKE, BANG)]
    #[tok(ESCAPE, this)]
    pub char: Box<Expr<'input>>,
}

/// The same shared `!` as `Stmt`, but the field excludes `Factorial`: the expression can
/// never take the `!`, so there is no overlap to accept and the terminator gets it.
#[derive(recursa::Node, Debug, Clone)]
pub struct Excluded<'input> {
    #[parse(pratt(exclude(Factorial)))]
    pub expr: Expr<'input>,
    pub terminator: Bang,
}

/// `!` is both a postfix extender of `Expr` and the statement terminator.
/// The acceptance makes the expression keep extending, so the required terminator
/// never gets its `!` and the statement runs out of input instead of reporting `RCA4102`.
#[derive(recursa::Node, Debug, Clone)]
pub struct Stmt<'input> {
    #[greedy(BANG)]
    pub expr: Expr<'input>,
    pub terminator: Bang,
}

/// The same shared `!`, but with a terminator the statement can do without. The acceptance
/// makes the expression keep extending at `expr`, so `1 !` parses as a factorial with no
/// terminator. Without it the extender is a runtime `RCA4102` ambiguity instead.
#[derive(recursa::Node, Debug, Clone)]
pub struct Tail<'input> {
    #[greedy(BANG)]
    pub expr: Expr<'input>,
    pub terminator: Option<Bang>,
}
