//! The tracer grammar of the `table-driven` fixture declared with two named parsers: `rd`, the
//! default, owns the unqualified surface and `lr` is reached through `parsers::lr::input`.
//! `Value::Sum` shares its first symbol with `Value::Number`, so on `print(1 + 2` the two
//! parsers name different expected sets: that difference is how a test tells which parser ran.
//!
//! The integration test runs `src/main.rs`, which uses only the default surface, once with
//! this configuration and once with `parser_style = recursive_descent`, and requires the same
//! bytes on stdout; `src/bin/lr.rs` drives the same inputs through the `lr` parser.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 2,
    parsers(rd = recursive_descent, lr = table_driven),
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
