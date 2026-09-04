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

/// The repetition itself may consume a `SELECT` that a caller could have continued with.
#[derive(recursa::Node, Debug, Clone)]
pub struct Query {
    #[greedy(SELECT)]
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
/// The parser keeps extending the expression while it sees `!`.
#[derive(recursa::Node, Debug, Clone)]
pub struct Stmt<'input> {
    #[greedy(BANG)]
    pub expr: Expr<'input>,
    pub terminator: Bang,
}
