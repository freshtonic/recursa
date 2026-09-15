//! Source-analysis fixture for the arithmetic Pratt grammar.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/arith.rs` and `src/grammars/arith.rs`.

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
    /// Declares the integer token type used by the closed generic atom.

    pub struct IntAtom {
        #[lex(pattern = r"[0-9]+")]
        pub value: IntLit,
    }
}

recursa::ast_node! {
    /// Declares the identifier token type used by the closed generic atom.

    pub struct IdentAtom {
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
        pub value: Ident,
    }
}

recursa::ast_node! {
    /// A real closed generic grammar Node, materialized as `Atom<IntAtom>` and
    /// `Atom<IdentAtom>` below.

    pub struct Atom<T> {
        pub value: T,
    }
}

recursa::ast_node! {
    /// Arithmetic expressions with atom, prefix, and infix Pratt positions.

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
