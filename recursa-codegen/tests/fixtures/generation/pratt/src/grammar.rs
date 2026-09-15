recursa::grammar! {
    module = crate::grammar,
    derives(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Pretty, Visit, VisitMut, Arbitrary, serde::Serialize, serde::Deserialize),
    spans = optional,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        IS => "IS",
        KW_NOT => "NOT",
        NEVER => "NEVER",
        EXISTS => "EXISTS",
        HAS => "HAS",
        LIKE => "LIKE",
        ESCAPE => "ESCAPE",
        NULL => "NULL",
        BEGIN => "BEGIN",
        GROUP => "GROUP",
        END => "END",
    }
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        CARET => "^",
        BANG => "!",
        DOUBLE_COLON => "::", spacing = tight,
        GROUP_OPEN => "[",
        GROUP_CLOSE => "]",
    }
}

recursa::ast_node! {

    pub struct Integer {
        #[lex(pattern = r"[0-9]+")]
        pub value: IntegerText,
    }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub value: NameText,
    }
}

recursa::ast_node! {

    pub struct EscapeClause {
        #[tok(ESCAPE, this)]
        pub value: Name,
    }
}

recursa::ast_node! {

    pub enum LikeSuffix {
        #[parse(lr_conflict(
        action = shift,
        against = grammar::EscapeClause,
        lookahead = { ESCAPE },
        expect = 2
    ))]
        Plain,
        Escaped(EscapeClause),
    }
}

recursa::ast_node! {

    #[pratt]
    #[pretty(group = inconsistent)]
    pub enum Expr {
        Integer(Integer),
        Name(Name),
        MultiGroup(#[tok(BEGIN, GROUP, this, END, GROUP)] boxed!(Self)),
        Paren(#[tok(GROUP_OPEN, this, GROUP_CLOSE)] boxed!(Self)),

        #[parse(prefix, bp = 35)]
        #[tok(MINUS)]
        Neg(boxed!(Self)),

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

        #[parse(infix, lbp = 15, rbp = 16)]
        Custom(
            boxed!(Self),
            #[lex(pattern = r"[~@]+")] OperatorText,
            boxed!(Self),
        ),

        #[parse(prefix, bp = 35)]
        #[tok(NEVER, EXISTS)]
        NotExists(boxed!(Self)),

        #[parse(infix, lbp = 5, rbp = 6)]
        #[tok(IS, KW_NOT)]
        #[pretty(group = inconsistent)]
        IsNot(boxed!(Self), #[pretty(break_before = soft)] boxed!(Self)),

        #[parse(postfix, bp = 5)]
        Like(boxed!(Self), #[tok(LIKE, this)] boxed!(Self), LikeSuffix),

        #[parse(infix, lbp = 25, rbp = 26)]
        #[tok(DOUBLE_COLON)]
        Cast(boxed!(Self), boxed!(Self)),

        #[parse(postfix, bp = 40)]
        #[tok(HAS, NULL)]
        IsNull(boxed!(Self)),
    }
}

recursa::ast_node! {

    pub struct FollowedHas {
        #[tok(this, HAS)]
        pub expression: Expr,
        pub tail: Integer,
    }
}
