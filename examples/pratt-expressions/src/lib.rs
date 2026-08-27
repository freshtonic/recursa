//! A compiled expression grammar showing Recursa's Pratt declarations.

pub mod grammar;

use std::fmt;

#[derive(Debug)]
pub enum ParseFailure {
    /// One or more source ranges were not recognized by the lexer.
    Lexical(Vec<recursa::Span>),
    /// The recognized tokens did not form an expression.
    Syntax(recursa::ParseError),
    /// A valid expression was followed by additional tokens.
    TrailingInput,
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lexical(spans) => write!(formatter, "{} lexical error(s)", spans.len()),
            Self::Syntax(error) => error.fmt(formatter),
            Self::TrailingInput => formatter.write_str("trailing input after the expression"),
        }
    }
}

impl std::error::Error for ParseFailure {}

/// Lexes and parses one complete expression through generated public APIs.
pub fn parse(source: &str) -> Result<grammar::Expr<'_>, ParseFailure> {
    let lexed = grammar::lex(source);
    let lexical_errors = lexed.errors().map(|error| error.span()).collect::<Vec<_>>();
    if !lexical_errors.is_empty() {
        return Err(ParseFailure::Lexical(lexical_errors));
    }
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse_spanned(&mut input).map_err(ParseFailure::Syntax)?;
    if !input.is_eof() {
        return Err(ParseFailure::TrailingInput);
    }
    Ok(parsed.into_ast())
}
