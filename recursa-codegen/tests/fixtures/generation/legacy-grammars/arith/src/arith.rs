//! Runtime form of the frozen M3 arithmetic grammar from recursa-spike b410d2f.

recursa::grammar! {
    module = crate::arith,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        SLASH => "/",
        LPAREN => "(",
        RPAREN => ")",
    }
}

#[derive(recursa::Node)]
pub struct IntAtom<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub value: IntLit<'input>,
}

#[derive(recursa::Node)]
pub struct IdentAtom<'input> {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub value: Ident<'input>,
}

#[derive(recursa::Node)]
pub struct Atom<T> {
    pub value: T,
}

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'input> {
    IntLit(Atom<IntAtom<'input>>),
    Ident(Atom<IdentAtom<'input>>),
    Paren(#[tok(LPAREN, this, RPAREN)] Box<Expr<'input>>),

    #[parse(prefix, bp = 30)]
    #[tok(MINUS)]
    Neg(Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(MINUS)]
    Sub(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(STAR)]
    Mul(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(SLASH)]
    Div(Box<Self>, Box<Self>),
}
