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

//! The Pratt expression grammar `examples/expressions` carried while it was named
//! `examples/pratt-expressions`, frozen here so `generated_pratt_table_driven.rs` keeps
//! proving Pratt lowering until `docs/declared-precedence.md` step 3 removes it. The
//! example itself is now a plain enum with a `precedence { ... }` declaration.

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

    #[pratt]
    pub enum Expr {
        Integer(Integer),
        Name(Name),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),

        #[parse(prefix, bp = 35)]
        #[tok(MINUS)]
        Negate(boxed!(Self)),

        #[parse(postfix, bp = 40)]
        #[tok(BANG)]
        Factorial(boxed!(Self)),

        #[parse(infix, lbp = 30, rbp = 30)]
        #[tok(CARET)]
        Power(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 20, rbp = 21)]
        #[tok(STAR)]
        Multiply(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
