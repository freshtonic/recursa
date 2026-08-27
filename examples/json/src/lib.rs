//! A compiled JSON grammar showing Recursa's AST-first public setup.

pub mod grammar;

use std::fmt;

#[derive(Debug)]
pub enum ParseFailure {
    /// One or more source ranges were not recognized by the lexer.
    Lexical(Vec<recursa::Span>),
    /// The recognized tokens did not form a JSON value.
    Syntax(recursa::ParseError),
    /// A valid value was followed by additional tokens.
    TrailingInput,
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lexical(spans) => write!(formatter, "{} lexical error(s)", spans.len()),
            Self::Syntax(error) => error.fmt(formatter),
            Self::TrailingInput => formatter.write_str("trailing input after the JSON value"),
        }
    }
}

impl std::error::Error for ParseFailure {}

/// Lexes and parses one complete JSON value through generated public APIs.
pub fn parse(source: &str) -> Result<grammar::Value<'_>, ParseFailure> {
    let lexed = grammar::lex(source);
    let lexical_errors = lexed.errors().map(|error| error.span()).collect::<Vec<_>>();
    if !lexical_errors.is_empty() {
        return Err(ParseFailure::Lexical(lexical_errors));
    }
    let mut input = lexed.input();
    let parsed = grammar::Value::parse_spanned(&mut input).map_err(ParseFailure::Syntax)?;
    if !input.is_eof() {
        return Err(ParseFailure::TrailingInput);
    }
    Ok(parsed.into_ast())
}
