//! Pretty-golden fixture proving declared-precedence parenthesization for a plain (non-Pratt)
//! expression enum, per `docs/declared-precedence.md`'s "Consumers -> Pretty" paragraph.
//!
//! `Expr` carries no `#[pratt]` role at all: every operator variant takes its level from the
//! `precedence { ... }` declaration below, either from its last declared terminal (`Add`,
//! `Subtract`, `Multiply`, `Power`, `Factorial`, `Equals`) or from a `#[parse(prec = UMINUS)]`
//! override (`Negate`). Pretty must derive parenthesization from each variant's level and
//! associativity, exactly as `lr::declared_precedence` derives LR rule precedence from the same
//! declaration.

recursa::grammar! {
    module = crate::declared_precedence,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        CARET => "^",
        BANG => "!",
        EQ => "==",
        LPAREN => "(",
        RPAREN => ")",
    }
    precedence {
        nonassoc(bp = 5) { EQ },
        left(bp = 10) { PLUS, MINUS },
        left(bp = 20) { STAR },
        right(bp = 30) { CARET },
        right(bp = 35) { UMINUS },
        left(bp = 40) { BANG },
    }
}

recursa::ast_node! {

    pub struct Integer {
        #[lex(pattern = r"[0-9]+")]
        pub text: IntegerText,
    }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {

    /// An ordinary enum: no `#[pratt]`, no Pratt role attributes, no binding powers. Every
    /// operator token attaches to the operand field it precedes or follows, per the "Attachment
    /// units inline" authoring surface.
    pub enum Expr {
        Integer(Integer),
        Name(Name),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),

        #[parse(prec = UMINUS)]
        Negate(#[tok(MINUS, this)] boxed!(Self)),

        Factorial(#[tok(this, BANG)] boxed!(Self)),

        Power(boxed!(Self), #[tok(CARET, this)] boxed!(Self)),

        Multiply(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),

        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),

        Subtract(boxed!(Self), #[tok(MINUS, this)] boxed!(Self)),

        Equals(boxed!(Self), #[tok(EQ, this)] boxed!(Self)),
    }
}
