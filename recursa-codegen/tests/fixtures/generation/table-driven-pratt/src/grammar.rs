//! The table-driven Pratt fixture: the textbook expression grammar written as a `#[pratt]`
//! enum, with infix `+ - * ^` and `in`, prefix `-`, postfix `!`, the interleaved
//! `a between b and c` (pg-sql's attachment form) and `a from b for c` (bare operands), the
//! delimited `a[b]`, and one exclusion site, the needle of `position(needle in haystack)`,
//! which restricts the expression language the way gram.y's `b_expr` does inside `POSITION`.
//!
//! The integration test exercises the sole parser; the LR fixture asserts its precedence
//! resolution directly.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        POSITION => "position",
        IN => "in",
        BETWEEN => "between",
        AND => "and",
        FROM => "from",
        FOR => "for",
    }
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        CARET => "^",
        BANG => "!",
        LPAREN => "(",
        RPAREN => ")",
        LBRACKET => "[",
        RBRACKET => "]",
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Integer {
        #[lex(pattern = r"[0-9]+")]
        pub text: IntegerText,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {
    /// `position(needle in haystack)`: the needle cannot itself be an `in` expression, so the
    /// keyword belongs to the enclosing form.
    #[derive(Debug, Clone)]
    #[tok(POSITION, LPAREN, this, RPAREN)]
    pub struct Position {
        #[parse(pratt(exclude(In)))]
        pub needle: boxed!(Expr),
        #[tok(IN)]
        pub haystack: boxed!(Expr),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[pratt]
    pub enum Expr {
        Integer(Integer),
        Name(Name),
        Position(Position),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),

        #[parse(prefix, bp = 30)]
        #[tok(MINUS)]
        Negate(boxed!(Self)),

        #[parse(postfix, bp = 50)]
        #[tok(BANG)]
        Factorial(boxed!(Self)),

        /// An interleaved shape, written as pg-sql writes it: each operand carries the fixed token
        /// before it, so its own reduce decides where the operand stops (before any operator
        /// binding weaker than `between`); inside an exclusion clone both operands carry the
        /// exclusion through their cloned attachments.
        #[parse(postfix, bp = 8)]
        Between(
            boxed!(Self),
            #[tok(BETWEEN, this)] boxed!(Self),
            #[tok(AND, this)] boxed!(Self),
        ),

        /// An interleaved shape with bare operands: the middle one is parsed with the postfix
        /// binding power as its minimum, so it stops before `in`; the last extends by precedence.
        #[parse(postfix, bp = 7)]
        Slice(
            boxed!(Self),
            #[tok(FROM)] boxed!(Self),
            #[tok(FOR)] boxed!(Self),
        ),

        /// A delimited postfix operand, parsed with the postfix binding power as its minimum: only
        /// `!` and another index may extend it, `a[b + c]` is an error in both styles.
        #[parse(postfix, bp = 50)]
        Index(boxed!(Self), #[tok(LBRACKET, this, RBRACKET)] boxed!(Self)),

        #[parse(infix, lbp = 40, rbp = 40)]
        #[tok(CARET)]
        Power(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 20, rbp = 21)]
        #[tok(STAR)]
        Multiply(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(MINUS)]
        Subtract(boxed!(Self), boxed!(Self)),

        #[parse(infix, lbp = 5, rbp = 6)]
        #[tok(IN)]
        In(boxed!(Self), boxed!(Self)),
    }
}
