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

recursa::ast_node! {

    pub struct IntAtom {
        #[lex(pattern = r"[0-9]+")]
        pub value: IntLit,
    }
}

recursa::ast_node! {

    pub struct IdentAtom {
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
        pub value: Ident,
    }
}

recursa::ast_node! {

    pub struct Atom<T> {
        pub value: T,
    }
}

recursa::ast_node! {

    #[pratt]
    pub enum Expr {
        IntLit(Atom<IntAtom>),
        Ident(Atom<IdentAtom>),
        Paren(#[tok(LPAREN, this, RPAREN)] boxed!(Expr)),

        #[parse(prefix, bp = 30)]
        #[tok(MINUS)]
        Neg(boxed!(Self)),

        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(MINUS)]
        Sub(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 20, rbp = 21)]
        #[tok(STAR)]
        Mul(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 20, rbp = 21)]
        #[tok(SLASH)]
        Div(boxed!(Self), boxed!(Self)),
    }
}
