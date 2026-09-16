//! The lookahead-filter grammar: PostgreSQL's `NOT BETWEEN`, `NOT IN` and `NOT LIKE` tests
//! beside unary `NOT cond` and a conjunction `name NOT cond` ("name and not cond").
//!
//! The name before `NOT BETWEEN` is a `Subject` and the name before `NOT cond` an `Operand`,
//! two nonterminals an LALR(1) parser must reduce before it shifts `NOT`, which one token of
//! lookahead cannot decide: without the filter the construction has a reduce/reduce conflict on
//! `NOT`. The `lookahead` declaration makes the token feed read `NOT` before `BETWEEN`, `IN` or
//! `LIKE` as `NOT_LA`, the token the test rules shift, so `Subject` reduces on `NOT_LA` and
//! `Operand` on `NOT`.
//!
//! `ExprRule` is gram.y's `a_expr` shape: a left-recursive enum with prefix `NOT`, infix `LIKE`
//! and the tests `NOT LIKE`, `NOT BETWEEN` and `NOT IN`, over a name that admits `like` and
//! `between` the way `ColId` admits `BETWEEN` and `func_name` admits `LIKE`. The operand of
//! prefix `NOT` can therefore begin with a trigger or with anything else, so the rule is
//! emitted twice, `NOT expr | NOT_LA expr`, as gram.y writes `NOT a_expr | NOT_LA a_expr` by
//! hand; the filter itself is what lets `a NOT LIKE b` be a test while `NOT a` is a negation.
//!
//! The integration test exercises this grammar through Recursa's sole LR parser.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        NOT => "not",
        BETWEEN => "between",
        AND => "and",
        IN => "in",
        LIKE => "like",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        SEMI => ";",
    }
    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }
    precedence {
        left(bp = 10) { NOT },
        nonassoc(bp = 20) { BETWEEN, IN, LIKE, NOT_LA },
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(this, SEMI)]
    pub struct Rule {
        pub cond: Cond,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Cond {
        Not { mark: NotMark, inner: boxed!(Cond) },
        NotBetween(NotBetween),
        NotIn(NotIn),
        NotLike(NotLike),
        Between(Between),
        AndNot(AndNot),
        Name { name: Name },
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(NOT)]
    pub struct NotMark;
}

recursa::ast_node! {
    /// The left side of a test operator.
    #[derive(Debug, Clone)]
    pub struct Subject {
        pub name: Name,
    }
}

recursa::ast_node! {
    /// The left operand of a conjunction with a negated condition.
    #[derive(Debug, Clone)]
    pub struct Operand {
        pub name: Name,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct NotBetween {
        pub subject: Subject,
        #[tok(NOT, BETWEEN)]
        pub low: Number,
        #[tok(AND)]
        pub high: Number,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(this, RPAREN)]
    pub struct NotIn {
        pub subject: Subject,
        #[tok(NOT, IN, LPAREN)]
        pub member: Number,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct NotLike {
        pub subject: Subject,
        #[tok(NOT, LIKE)]
        pub pattern: Name,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Between {
        pub subject: Subject,
        #[tok(BETWEEN)]
        pub low: Number,
        #[tok(AND)]
        pub high: Number,
    }
}

recursa::ast_node! {
    /// `name NOT cond`: the name and the negation of the condition.
    #[derive(Debug, Clone)]
    pub struct AndNot {
        pub operand: Operand,
        #[tok(NOT)]
        pub inner: boxed!(Cond),
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
    #[derive(Debug, Clone)]
    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}

recursa::ast_node! {
    /// `expr ;`, the root of the expression language.
    #[derive(Debug, Clone)]
    #[tok(this, SEMI)]
    pub struct ExprRule {
        pub expr: Expr,
    }
}

recursa::ast_node! {
    /// gram.y's `a_expr` shape: `NOT` binds weaker than every test, so `NOT a LIKE b` is
    /// `NOT (a LIKE b)` and `NOT a NOT LIKE b` is `NOT (a NOT LIKE b)`; the tests and `LIKE`
    /// share one level.
    #[derive(Debug, Clone)]
    pub enum Expr {
        Ident(Ident),
        Number(Number),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),

        /// `NOT expr`, pg-sql's `Expr::Not`: the operand can begin with `like` or `between` as a
        /// name, so the lowering twins the rule with `NOT_LA`. The override keeps both twins at
        /// `NOT`'s level, as `gram.y`'s `a_expr: NOT a_expr | NOT_LA a_expr` does.
        #[parse(prec = NOT)]
        Not(#[tok(NOT, this)] boxed!(Self)),

        NotLike(boxed!(Self), #[tok(NOT, LIKE, this)] boxed!(Self)),

        #[parse(prec = BETWEEN)]
        NotBetween(
            boxed!(Self),
            #[tok(NOT, BETWEEN, this)] boxed!(Self),
            #[tok(AND, this)] boxed!(Self),
        ),

        #[parse(prec = IN)]
        NotIn(
            boxed!(Self),
            #[tok(NOT, IN, LPAREN, this, RPAREN)] boxed!(Self),
        ),

        Like(boxed!(Self), #[tok(LIKE, this)] boxed!(Self)),

        #[parse(prec = BETWEEN)]
        Between(
            boxed!(Self),
            #[tok(BETWEEN, this)] boxed!(Self),
            #[tok(AND, this)] boxed!(Self),
        ),
    }
}

recursa::ast_node! {
    /// A name of the expression language, admitting the keywords `like` and `between`.
    #[derive(Debug, Clone)]
    pub struct Ident {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(LIKE, BETWEEN))]
        pub text: IdentText,
    }
}
