recursa::grammar! {
    module = crate::grammar,
    derives(Debug, Clone, PartialEq, Pretty),
    spans = optional,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation {
        MINUS => "-",
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
pub struct Atom<T> {
    pub value: T,
}

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'input> {
    Integer(Atom<Integer<'input>>),
    Name(Atom<Name<'input>>),

    #[parse(prefix, bp = 20)]
    #[tok(MINUS)]
    Neg(Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(MINUS)]
    Sub(Box<Self>, Box<Self>),
}
