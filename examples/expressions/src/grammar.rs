#![allow(
    dead_code,
    unused_variables,
    clippy::blocks_in_conditions,
    clippy::collapsible_if,
    clippy::needless_borrow,
    clippy::needless_return,
    clippy::possible_missing_else,
    reason = "the generated grammar is checked before its compact token stream is formatted"
)]

recursa::grammar! {
    module = crate::grammar,
    diagrams,
    derives(Visit),
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
        LPAREN => "(",
        RPAREN => ")",
    }
    precedence {
        left(bp = 10) { PLUS },
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

    pub enum Expr {
        Integer(Integer),
        Name(Name),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),

        /// `MINUS` is the rule's only terminal, so the override is what gives the prefix form
        /// its own level: `UMINUS` is a precedence-only name above `CARET`.
        #[parse(prec = UMINUS)]
        Negate(#[tok(MINUS, this)] boxed!(Self)),

        Factorial(#[tok(this, BANG)] boxed!(Self)),

        Power(boxed!(Self), #[tok(CARET, this)] boxed!(Self)),

        Multiply(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),

        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
