//! The table-driven tracer grammar: sequences, enum alternatives, fixed tokens and content
//! tokens, and nothing a later ticket owns (no `Vec`, no `Option`, no Pratt). `Value::Sum`
//! shares its first symbol with `Value::Number`, so the automaton state after a number both
//! shifts `+` and reduces by default: the case where a default reduction hides a continuation
//! from the failing state's row (#117).
//!
//! The integration test generates this file twice, once as written and once with the parser
//! style flipped to `recursive_descent`, and compares what the two parsers produce.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 2,
    parser_style = table_driven,
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

#[derive(recursa::Node, Debug, Clone)]
pub enum Statement<'input> {
    Let { assignment: Box<Assignment<'input>> },
    Print { print: Print<'input> },
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(LET, this, SEMI)]
pub struct Assignment<'input> {
    pub name: Name<'input>,
    #[tok(EQ)]
    pub value: Value<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(PRINT, LPAREN, this, RPAREN, SEMI)]
pub struct Print<'input> {
    pub value: Value<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Value<'input> {
    Name { name: Name<'input> },
    Number(Number<'input>),
    #[tok(TRUE)]
    True,
    #[tok(FALSE)]
    False,
    Negated {
        mark: Bang,
        inner: Box<Value<'input>>,
    },
    Sum {
        left: Number<'input>,
        #[tok(PLUS)]
        right: Box<Value<'input>>,
    },
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(BANG)]
pub struct Bang;

#[derive(recursa::Node, Debug, Clone)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub text: NameText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Number<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: NumberText<'input>,
}
