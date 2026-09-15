//! The table-driven tracer grammar: sequences, enum alternatives, fixed tokens and content
//! tokens, and nothing a later ticket owns (no `Vec`, no `Option`, no Pratt). `Value::Sum`
//! shares its first symbol with `Value::Number`, so the automaton state after a number both
//! shifts `+` and reduces by default: the case where a default reduction hides a continuation
//! from the failing state's row (#117).
//!
//! The integration test exercises the sole Recursa parser's values, provenance, and errors.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        LET => "let",
        PRINT => "print",
        TRUE => "true",
        FALSE => "false",
    }
    punctuation {
        EQ => "=",
        SEMI => ";",
        LPAREN => "(",
        RPAREN => ")",
        BANG => "!",
        PLUS => "+",
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Statement {
        Let { assignment: boxed!(Assignment) },
        Print { print: Print },
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(LET, this, SEMI)]
    pub struct Assignment {
        pub name: Name,
        #[tok(EQ)]
        pub value: Value,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(PRINT, LPAREN, this, RPAREN, SEMI)]
    pub struct Print {
        pub value: Value,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Value {
        Name {
            name: Name,
        },
        Number(Number),
        #[tok(TRUE)]
        True,
        #[tok(FALSE)]
        False,
        Negated {
            mark: Bang,
            inner: boxed!(Value),
        },
        Sum {
            left: Number,
            #[tok(PLUS)]
            right: boxed!(Value),
        },
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(BANG)]
    pub struct Bang;
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
