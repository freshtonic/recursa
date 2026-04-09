use std::sync::OnceLock;

use recursa::{Input, Parse, ParseError, ParseRules, Scan};
use regex::Regex;

// Keywords (case-insensitive, with word boundary)
recursa::keywords! {
    Select  => r"SELECT\b",
    From    => r"FROM\b",
    Where   => r"WHERE\b",
    As      => r"AS\b",
    And     => r"AND\b",
    Or      => r"OR\b",
    Not     => r"NOT\b",
    True    => r"TRUE\b",
    False   => r"FALSE\b",
    Null    => r"NULL\b",
    Is      => r"IS\b",
    Unknown => r"UNKNOWN\b",
    Create  => r"CREATE\b",
    Table   => r"TABLE\b",
    Insert  => r"INSERT\b",
    Into    => r"INTO\b",
    Values  => r"VALUES\b",
    Drop    => r"DROP\b",
    Order   => r"ORDER\b",
    By      => r"BY\b",
    Bool    => r"BOOL\b",
    Boolean => r"BOOLEAN\b",
    Text    => r"TEXT\b",
    Int     => r"INT\b",
}

// Punctuation
recursa::punctuation! {
    Semi      => ";",
    Comma     => ",",
    LParen    => r"\(",
    RParen    => r"\)",
    Star      => r"\*",
    Dot       => r"\.",
    Eq        => "=",
    Neq       => "<>",
    Lte       => "<=",
    Gte       => ">=",
    Lt        => "<",
    Gt        => ">",
    ColonColon => "::",
}

// --- String literal ---

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StringLit(pub String);

impl Scan<'_> for StringLit {
    const PATTERN: &'static str = r"'[^']*(?:''[^']*)*'";

    fn regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| Regex::new(r"\A(?:'[^']*(?:''[^']*)*')").unwrap())
    }

    fn from_match(matched: &str) -> Result<Self, ParseError> {
        Ok(StringLit(matched.to_string()))
    }
}

recursa::impl_parse_for_scan!(StringLit);

impl recursa::Visit for StringLit {
    fn visit<V: recursa::Visitor>(
        &self,
        visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::Break<V::Error>> {
        match visitor.enter(self) {
            std::ops::ControlFlow::Continue(()) | std::ops::ControlFlow::Break(recursa::Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}

impl recursa::AsNodeKey for StringLit {}

// --- Integer literal ---

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IntegerLit(pub String);

impl Scan<'_> for IntegerLit {
    const PATTERN: &'static str = r"[0-9]+";

    fn regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| Regex::new(r"\A(?:[0-9]+)").unwrap())
    }

    fn from_match(matched: &str) -> Result<Self, ParseError> {
        Ok(IntegerLit(matched.to_string()))
    }
}

recursa::impl_parse_for_scan!(IntegerLit);

impl recursa::Visit for IntegerLit {
    fn visit<V: recursa::Visitor>(
        &self,
        visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::Break<V::Error>> {
        match visitor.enter(self) {
            std::ops::ControlFlow::Continue(()) | std::ops::ControlFlow::Break(recursa::Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}

impl recursa::AsNodeKey for IntegerLit {}

// --- Identifier ---

/// SQL identifier: [a-zA-Z_][a-zA-Z0-9_]* but NOT a keyword.
///
/// The regex uses a negative lookahead to reject any string that matches
/// a SQL keyword followed by a word boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ident(pub String);

/// All SQL keywords (uppercase) for identifier exclusion.
const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AS", "AND", "OR", "NOT", "TRUE", "FALSE",
    "NULL", "IS", "UNKNOWN", "CREATE", "TABLE", "INSERT", "INTO", "VALUES",
    "DROP", "ORDER", "BY", "BOOL", "BOOLEAN", "TEXT", "INT",
];

fn is_keyword(s: &str) -> bool {
    let upper = s.to_uppercase();
    SQL_KEYWORDS.contains(&upper.as_str())
}

impl Scan<'_> for Ident {
    const PATTERN: &'static str = r"[a-zA-Z_][a-zA-Z0-9_]*";

    fn regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| Regex::new(r"\A[a-zA-Z_][a-zA-Z0-9_]*").unwrap())
    }

    fn from_match(matched: &str) -> Result<Self, ParseError> {
        Ok(Ident(matched.to_string()))
    }
}

impl<'input> Parse<'input> for Ident {
    const IS_TERMINAL: bool = true;

    fn first_pattern() -> &'static str {
        <Self as Scan>::PATTERN
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        match Self::regex().find(input.remaining()) {
            Some(m) if m.start() == 0 => !is_keyword(m.as_str()),
            _ => false,
        }
    }

    fn parse<R: ParseRules>(
        input: &mut Input<'input>,
        _rules: &R,
    ) -> Result<Self, ParseError> {
        match Self::regex().find(input.remaining()) {
            Some(m) if m.start() == 0 => {
                let matched = &input.source()[input.cursor()..input.cursor() + m.len()];
                if is_keyword(matched) {
                    return Err(ParseError::new(
                        input.source().to_string(),
                        input.cursor()..input.cursor() + m.len(),
                        "identifier (not a keyword)",
                    ));
                }
                let result = Self::from_match(matched)?;
                input.advance(m.len());
                Ok(result)
            }
            _ => Err(ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                <Self as Scan>::PATTERN,
            )),
        }
    }
}

impl recursa::Visit for Ident {
    fn visit<V: recursa::Visitor>(
        &self,
        visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::Break<V::Error>> {
        match visitor.enter(self) {
            std::ops::ControlFlow::Continue(()) | std::ops::ControlFlow::Break(recursa::Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}

impl recursa::AsNodeKey for Ident {}

#[cfg(test)]
mod tests {
    use recursa::{Input, NoRules, Parse};

    use super::*;

    // --- Keyword tests ---

    #[test]
    fn keyword_select_uppercase() {
        let input = Input::new("SELECT");
        assert!(<Select as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_select_lowercase() {
        let input = Input::new("select");
        assert!(<Select as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_select_mixed_case() {
        let input = Input::new("SeLeCt");
        assert!(<Select as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_select_not_prefix_of_identifier() {
        let input = Input::new("SELECTED");
        assert!(!<Select as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_bool_not_prefix_of_booleq() {
        let input = Input::new("booleq");
        assert!(!<Bool as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_bool_matches_standalone() {
        let input = Input::new("bool");
        assert!(<Bool as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_boolean_matches() {
        let input = Input::new("BOOLEAN");
        assert!(<Boolean as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_not_matches() {
        let input = Input::new("NOT");
        assert!(<Not as Parse>::peek(&input, &NoRules));
    }

    // --- Punctuation tests ---

    #[test]
    fn punctuation_semicolon() {
        let mut input = Input::new(";");
        let _ = <Semi as Parse>::parse(&mut input, &NoRules).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn punctuation_neq() {
        let input = Input::new("<>");
        assert!(<Neq as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn punctuation_colon_colon() {
        let input = Input::new("::");
        assert!(<ColonColon as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn punctuation_lte() {
        let input = Input::new("<=");
        assert!(<Lte as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn punctuation_gte() {
        let input = Input::new(">=");
        assert!(<Gte as Parse>::peek(&input, &NoRules));
    }

    // --- String literal tests ---

    #[test]
    fn string_literal_simple() {
        let mut input = Input::new("'hello world'");
        let lit = <StringLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'hello world'");
        assert!(input.is_empty());
    }

    #[test]
    fn string_literal_with_escaped_quote() {
        let mut input = Input::new("'it''s'");
        let lit = <StringLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'it''s'");
    }

    #[test]
    fn string_literal_empty() {
        let mut input = Input::new("''");
        let lit = <StringLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "''");
    }

    #[test]
    fn string_literal_with_spaces() {
        let mut input = Input::new("'   f           '");
        let lit = <StringLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'   f           '");
    }

    // --- Integer literal tests ---

    #[test]
    fn integer_literal() {
        let mut input = Input::new("42");
        let lit = <IntegerLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "42");
    }

    #[test]
    fn integer_literal_zero() {
        let mut input = Input::new("0");
        let lit = <IntegerLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "0");
    }

    // --- Identifier tests ---

    #[test]
    fn identifier_simple() {
        let mut input = Input::new("my_table");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "my_table");
    }

    #[test]
    fn identifier_with_digits() {
        let mut input = Input::new("f1");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "f1");
    }

    #[test]
    fn identifier_uppercase() {
        let mut input = Input::new("BOOLTBL1");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "BOOLTBL1");
    }

    #[test]
    fn identifier_rejects_keyword_select() {
        let input = Input::new("SELECT");
        assert!(!<Ident as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn identifier_rejects_keyword_true() {
        let input = Input::new("true");
        assert!(!<Ident as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn identifier_rejects_keyword_null() {
        let input = Input::new("NULL");
        assert!(!<Ident as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn identifier_accepts_keyword_prefix() {
        // "isfalse" starts with "is" but is not a keyword
        let mut input = Input::new("isfalse");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "isfalse");
    }

    #[test]
    fn identifier_accepts_booleq() {
        let mut input = Input::new("booleq");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "booleq");
    }

    #[test]
    fn identifier_accepts_boolne() {
        let mut input = Input::new("boolne");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "boolne");
    }

    #[test]
    fn identifier_accepts_isnul() {
        let mut input = Input::new("isnul");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "isnul");
    }

    #[test]
    fn identifier_accepts_istrue() {
        let mut input = Input::new("istrue");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "istrue");
    }

    #[test]
    fn identifier_accepts_pg_input_is_valid() {
        let mut input = Input::new("pg_input_is_valid");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "pg_input_is_valid");
    }
}
