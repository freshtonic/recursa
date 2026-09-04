recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        SELECT => "SELECT",
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
