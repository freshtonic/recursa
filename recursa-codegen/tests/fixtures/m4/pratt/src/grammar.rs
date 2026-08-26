recursa::grammar! {
    module = crate::grammar,
    spans = optional,
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
    pub value: IntegerText<'input>,
}

#[derive(recursa::Node)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub value: NameText<'input>,
}

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'input> {
    Integer(Integer<'input>),
    Name(Name<'input>),
    Paren(#[tok(LPAREN, this, RPAREN)] Box<Self>),

    #[parse(prefix, bp = 35)]
    #[tok(MINUS)]
    Neg(Box<Self>),

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

#[derive(recursa::Node)]
pub struct Followed<'input> {
    #[tok(this, PLUS)]
    pub expression: Expr<'input>,
    pub tail: Integer<'input>,
}

#[derive(recursa::Node)]
pub struct NestedFollowed<'input> {
    #[tok(this, STAR)]
    pub expression: Expr<'input>,
    pub tail: Integer<'input>,
}
