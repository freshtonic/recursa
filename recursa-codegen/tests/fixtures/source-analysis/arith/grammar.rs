//! Source-analysis fixture for the arithmetic expression grammar.
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
    precedence {
        left(bp = 10) { PLUS, MINUS },
        left(bp = 20) { STAR, SLASH },
        right(bp = 30) { UMINUS },
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
    /// Arithmetic expressions with atom, prefix, and infix operator variants.

    pub enum Expr {
        IntLit(Atom<IntAtom>),
        Ident(Atom<IdentAtom>),
        Paren(#[tok(LPAREN, this, RPAREN)] boxed!(Expr)),

        #[parse(prec = UMINUS)]
        Neg(#[tok(MINUS, this)] boxed!(Self)),

        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),

        Sub(boxed!(Self), #[tok(MINUS, this)] boxed!(Self)),

        Mul(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),

        Div(boxed!(Self), #[tok(SLASH, this)] boxed!(Self)),
    }
}
