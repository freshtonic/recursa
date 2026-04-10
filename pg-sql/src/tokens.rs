use recursa::{ParseError, Scan, Visit};

/// Keywords
pub mod keyword {
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
}

/// Punctuation
pub mod punct {
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
        BackSlash  => r"\\",
    }
}

// Literals
pub mod literal {
    use super::*;

    recursa::literals! {
        StringLit  => r"'[^']*(?:''[^']*)*'",
        IntegerLit => r"[0-9]+",
    }

    // --- Identifier ---

    /// All SQL keywords (uppercase) for identifier exclusion.
    const SQL_KEYWORDS: &[&str] = &[
        "SELECT", "FROM", "WHERE", "AS", "AND", "OR", "NOT", "TRUE", "FALSE", "NULL", "IS",
        "UNKNOWN", "CREATE", "TABLE", "INSERT", "INTO", "VALUES", "DROP", "ORDER", "BY", "BOOL",
        "BOOLEAN", "TEXT", "INT",
    ];

    fn is_keyword(s: &str) -> bool {
        SQL_KEYWORDS.iter().any(|kw| kw.eq_ignore_ascii_case(s))
    }

    /// Postcondition: reject identifiers that are SQL keywords.
    fn not_keyword(ident: &Ident) -> Result<(), ParseError> {
        if is_keyword(&ident.0) {
            Err(ParseError::new(
                ident.0.clone(),
                0..ident.0.len(),
                "identifier (not a keyword)",
            ))
        } else {
            Ok(())
        }
    }

    /// SQL identifier: `[a-zA-Z_][a-zA-Z0-9_]*` but NOT a keyword.
    #[derive(Scan, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    #[parse(postcondition = not_keyword)]
    #[visit(terminal)]
    pub struct Ident(pub String);

    // --- Alias name (any SQL word — identifier or keyword) ---

    /// Matches any SQL word including keywords. Used for alias names where
    /// SQL allows keywords (e.g., `SELECT 1 AS true`).
    #[derive(Scan, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    #[visit(terminal)]
    pub struct AliasName(pub String);

    // --- Rest of line ---

    /// Matches the remainder of text on the current line (up to newline or end of input).
    #[derive(Scan, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[scan(pattern = r"[^\n]*")]
    #[visit(terminal)]
    pub struct RestOfLine(pub String);
}

#[cfg(test)]
mod tests {
    use recursa::{Input, NoRules, Parse};

    use super::keyword::*;
    use super::literal::*;
    use super::punct::*;

    // --- Keyword tests ---

    #[test]
    fn keyword_select_uppercase() {
        let input = Input::new("SELECT");
        assert!(Select::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_select_lowercase() {
        let input = Input::new("select");
        assert!(Select::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_select_mixed_case() {
        let input = Input::new("SeLeCt");
        assert!(Select::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_select_not_prefix_of_identifier() {
        let input = Input::new("SELECTED");
        assert!(!Select::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_bool_not_prefix_of_booleq() {
        let input = Input::new("booleq");
        assert!(!Bool::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_bool_matches_standalone() {
        let input = Input::new("bool");
        assert!(Bool::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_boolean_matches() {
        let input = Input::new("BOOLEAN");
        assert!(Boolean::peek(&input, &NoRules));
    }

    #[test]
    fn keyword_not_matches() {
        let input = Input::new("NOT");
        assert!(Not::peek(&input, &NoRules));
    }

    // --- Punctuation tests ---

    #[test]
    fn punctuation_semicolon() {
        let mut input = Input::new(";");
        let _ = Semi::parse(&mut input, &NoRules).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn punctuation_neq() {
        let input = Input::new("<>");
        assert!(Neq::peek(&input, &NoRules));
    }

    #[test]
    fn punctuation_colon_colon() {
        let input = Input::new("::");
        assert!(ColonColon::peek(&input, &NoRules));
    }

    #[test]
    fn punctuation_lte() {
        let input = Input::new("<=");
        assert!(Lte::peek(&input, &NoRules));
    }

    #[test]
    fn punctuation_gte() {
        let input = Input::new(">=");
        assert!(Gte::peek(&input, &NoRules));
    }

    // --- String literal tests ---

    #[test]
    fn string_literal_simple() {
        let mut input = Input::new("'hello world'");
        let lit = StringLit::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'hello world'");
        assert!(input.is_empty());
    }

    #[test]
    fn string_literal_with_escaped_quote() {
        let mut input = Input::new("'it''s'");
        let lit = StringLit::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'it''s'");
    }

    #[test]
    fn string_literal_empty() {
        let mut input = Input::new("''");
        let lit = StringLit::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "''");
    }

    #[test]
    fn string_literal_with_spaces() {
        let mut input = Input::new("'   f           '");
        let lit = StringLit::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'   f           '");
    }

    // --- Integer literal tests ---

    #[test]
    fn integer_literal() {
        let mut input = Input::new("42");
        let lit = IntegerLit::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "42");
    }

    #[test]
    fn integer_literal_zero() {
        let mut input = Input::new("0");
        let lit = IntegerLit::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "0");
    }

    // --- Identifier tests ---

    #[test]
    fn identifier_simple() {
        let mut input = Input::new("my_table");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "my_table");
    }

    #[test]
    fn identifier_with_digits() {
        let mut input = Input::new("f1");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "f1");
    }

    #[test]
    fn identifier_uppercase() {
        let mut input = Input::new("BOOLTBL1");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "BOOLTBL1");
    }

    #[test]
    fn identifier_rejects_keyword_select() {
        let input = Input::new("SELECT");
        assert!(!Ident::peek(&input, &NoRules));
    }

    #[test]
    fn identifier_rejects_keyword_true() {
        let input = Input::new("true");
        assert!(!Ident::peek(&input, &NoRules));
    }

    #[test]
    fn identifier_rejects_keyword_null() {
        let input = Input::new("NULL");
        assert!(!Ident::peek(&input, &NoRules));
    }

    #[test]
    fn identifier_accepts_keyword_prefix() {
        let mut input = Input::new("isfalse");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "isfalse");
    }

    #[test]
    fn identifier_accepts_booleq() {
        let mut input = Input::new("booleq");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "booleq");
    }

    #[test]
    fn identifier_accepts_boolne() {
        let mut input = Input::new("boolne");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "boolne");
    }

    #[test]
    fn identifier_accepts_isnul() {
        let mut input = Input::new("isnul");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "isnul");
    }

    #[test]
    fn identifier_accepts_istrue() {
        let mut input = Input::new("istrue");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "istrue");
    }

    #[test]
    fn identifier_accepts_pg_input_is_valid() {
        let mut input = Input::new("pg_input_is_valid");
        let id = Ident::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "pg_input_is_valid");
    }
}
