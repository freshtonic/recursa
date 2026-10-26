//! The `expressions` grammar: the whole authoring surface of
//! `docs/declared-precedence.md` in one small language, proved through the generated parser.
//!
//! - A prefix override through a precedence-only name: `Negate` is `MINUS Expr`, whose only
//!   terminal is the binary `MINUS`, so `#[parse(prec = UMINUS)]` is what lifts it above the
//!   multiplicative operators, exactly as `a_expr: '-' a_expr %prec UMINUS` does.
//! - Left (`PLUS`, `STAR`), right (`CARET`) and nonassociative (`EQ`) infix levels.
//! - A postfix operator (`BANG`) at the tightest level.
//! - An interior operand through a restricted expression: `BExpr` is `gram.y`'s `b_expr`, so
//!   the `AND` of `a between b and c` closes the clause instead of opening a boolean operand,
//!   while the parenthesized atom keeps the full language.
//! - An `ESCAPE`-style keyword one level above its operator: `ESCAPE` above `LIKE` shifts the
//!   optional clause, while any token below `LIKE` reduces the comparison first.
//! - A rule whose level is not the level of the token after its first operand: `Quant` takes
//!   `PLUS` through `#[parse(prec = PLUS)]`, as `gram.y`'s `a_expr subquery_Op sub_type
//!   select_with_parens %prec Op` does, but the parser compares the level of its operator token,
//!   `*` or `=`, when it decides to reduce the operand before it.
//! - Prefix operators whose spellings the lexer joins: `@` beside `@` is `@@`, and `-` beside
//!   `-` opens a comment, so `Pretty` writes a space between two of them.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty, Visit, Arbitrary),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+|--[^\n]*",
    keywords {
        AND => "and",
        BETWEEN => "between",
        LIKE => "like",
        ESCAPE => "escape",
        NULL => "null",
        ANY => "any",
    }
    punctuation {
        EQ => "=",
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        CARET => "^",
        BANG => "!",
        AT => "@",
        ATAT => "@@",
        LPAREN => "(",
        RPAREN => ")",
    }
    precedence {
        nonassoc(bp = 5) { EQ },
        left(bp = 10) { AND },
        nonassoc(bp = 15) { BETWEEN, LIKE },
        nonassoc(bp = 18) { ESCAPE },
        left(bp = 20) { PLUS, MINUS },
        left(bp = 30) { STAR },
        right(bp = 40) { CARET },
        right(bp = 45) { UMINUS, AT, ATAT },
        left(bp = 50) { BANG },
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
        #[lex(pattern = r"[a-z_][a-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Expr {
        Integer(Integer),
        Name(Name),
        /// `a_expr: NULL_P`: a keyword atom, whose token is an element of the rule rather than
        /// syntax attached to an operand.
        #[tok(NULL)]
        Null,
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),
        /// `a_expr: '-' a_expr %prec UMINUS`.
        #[parse(prec = UMINUS)]
        Negate(#[tok(MINUS, this)] boxed!(Self)),
        Factorial(#[tok(this, BANG)] boxed!(Self)),
        /// A prefix `@`, whose spelling joins a second one into `@@`.
        Abs(#[tok(AT, this)] boxed!(Self)),
        /// A prefix `@@`.
        Center(#[tok(ATAT, this)] boxed!(Self)),
        Power(boxed!(Self), #[tok(CARET, this)] boxed!(Self)),
        Multiply(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
        Subtract(boxed!(Self), #[tok(MINUS, this)] boxed!(Self)),
        And(boxed!(Self), #[tok(AND, this)] boxed!(Self)),
        Equals(boxed!(Self), #[tok(EQ, this)] boxed!(Self)),
        /// `a_expr: a_expr LIKE a_expr`.
        Like(boxed!(Self), #[tok(LIKE, this)] boxed!(Self)),
        /// `a_expr: a_expr LIKE a_expr ESCAPE a_expr %prec LIKE`: the escape keyword is one
        /// level above `LIKE`, so it is shifted out of the completed comparison.
        #[parse(prec = LIKE)]
        LikeEscape(
            boxed!(Self),
            #[tok(LIKE, this)] boxed!(Self),
            #[tok(ESCAPE, this)] Name,
        ),
        /// `a_expr: a_expr BETWEEN b_expr AND a_expr %prec BETWEEN`: the override is what keeps
        /// the rule at `BETWEEN`'s level instead of taking `AND`'s, its last declared terminal.
        #[parse(prec = BETWEEN)]
        Between(
            boxed!(Self),
            #[tok(BETWEEN, this)] boxed!(BExpr),
            #[tok(AND, this)] boxed!(Self),
        ),
        /// `a_expr: a_expr subquery_Op sub_type select_with_parens %prec Op`: the rule takes
        /// `PLUS`'s level, while the token after its operand is `*` or `=`.
        #[parse(prec = PLUS)]
        Quant(boxed!(Self), QuantSuffix),
    }
}

recursa::ast_node! {
    /// The operator and the operand of a quantified comparison: `* any(name)`.
    #[derive(Debug, Clone)]
    pub struct QuantSuffix {
        pub op: QuantOp,
        #[tok(ANY, LPAREN, this, RPAREN)]
        pub name: Name,
    }
}

recursa::ast_node! {
    /// The operator of a quantified comparison: a token whose level differs from the rule's.
    #[derive(Debug, Clone)]
    pub enum QuantOp {
        #[tok(STAR)]
        Star,
        #[tok(EQ)]
        Eq,
    }
}

recursa::ast_node! {
    /// `gram.y` `b_expr`: `a_expr` without the boolean-keyword productions, so that the `AND`
    /// of a `BETWEEN` clause is never the start of a boolean operand.
    #[restricts(Expr)]
    pub enum BExpr {
        Integer,
        Name,
        Null,
        Parenthesized,
        Negate,
        Factorial,
        Power,
        Multiply,
        Add,
        Subtract,
    }
}
