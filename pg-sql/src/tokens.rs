use recursa::{ParseError, Scan, Visit};

/// Keywords
pub mod keyword {
    // Keywords (case-insensitive, with word boundary)
    recursa::keywords! {
        Select      => r"SELECT\b",
        From        => r"FROM\b",
        Where       => r"WHERE\b",
        As          => r"AS\b",
        And         => r"AND\b",
        Or          => r"OR\b",
        Not         => r"NOT\b",
        True        => r"TRUE\b",
        False       => r"FALSE\b",
        Null        => r"NULL\b",
        Is          => r"IS\b",
        Unknown     => r"UNKNOWN\b",
        Create      => r"CREATE\b",
        Table       => r"TABLE\b",
        Insert      => r"INSERT\b",
        Into        => r"INTO\b",
        Values      => r"VALUES\b",
        Drop        => r"DROP\b",
        Delete      => r"DELETE\b",
        Order       => r"ORDER\b",
        By          => r"BY\b",
        Bool        => r"BOOL\b",
        Boolean     => r"BOOLEAN\b",
        Text        => r"TEXT\b",
        Int         => r"INT\b",
        Serial      => r"SERIAL\b",
        Primary     => r"PRIMARY\b",
        Key         => r"KEY\b",
        Asc         => r"ASC\b",
        Desc        => r"DESC\b",
        Nulls       => r"NULLS\b",
        First       => r"FIRST\b",
        Last        => r"LAST\b",
        Using       => r"USING\b",
        Offset      => r"OFFSET\b",
        Limit       => r"LIMIT\b",
        Analyze     => r"ANALYZE\b",
        Set         => r"SET\b",
        Reset       => r"RESET\b",
        To          => r"TO\b",
        On          => r"ON\b",
        Off         => r"OFF\b",
        Temp        => r"TEMP\b",
        Index       => r"INDEX\b",
        Explain     => r"EXPLAIN\b",
        For         => r"FOR\b",
        Update      => r"UPDATE\b",
        Function    => r"FUNCTION\b",
        Returns     => r"RETURNS\b",
        Setof       => r"SETOF\b",
        Language    => r"LANGUAGE\b",
        Immutable   => r"IMMUTABLE\b",
        Union       => r"UNION\b",
        All         => r"ALL\b",
        In          => r"IN\b",
        Default     => r"DEFAULT\b",
        Lateral     => r"LATERAL\b",
        Partition   => r"PARTITION\b",
        Of          => r"OF\b",
        Costs       => r"COSTS\b",
        Timing      => r"TIMING\b",
        Summary     => r"SUMMARY\b",
        // WITH clause and CTE support
        With        => r"WITH\b",
        Recursive   => r"RECURSIVE\b",
        Materialized => r"MATERIALIZED\b",
        // Set operations
        Except      => r"EXCEPT\b",
        Intersect   => r"INTERSECT\b",
        Distinct    => r"DISTINCT\b",
        // JOIN support
        Join        => r"JOIN\b",
        Left        => r"LEFT\b",
        Right       => r"RIGHT\b",
        Full        => r"FULL\b",
        Inner       => r"INNER\b",
        Cross       => r"CROSS\b",
        // GROUP BY / HAVING / DISTINCT
        Group       => r"GROUP\b",
        Having      => r"HAVING\b",
        // UPDATE statement
        Returning   => r"RETURNING\b",
        // MERGE statement
        Merge       => r"MERGE\b",
        When        => r"WHEN\b",
        Matched     => r"MATCHED\b",
        Then        => r"THEN\b",
        // INSERT ON CONFLICT
        Conflict    => r"CONFLICT\b",
        Do          => r"DO\b",
        Nothing     => r"NOTHING\b",
        Excluded    => r"EXCLUDED\b",
        // VIEW
        View        => r"VIEW\b",
        Replace     => r"REPLACE\b",
        Temporary   => r"TEMPORARY\b",
        // EXISTS
        Exists      => r"EXISTS\b",
        // SEARCH / CYCLE
        Search      => r"SEARCH\b",
        Depth       => r"DEPTH\b",
        Breadth     => r"BREADTH\b",
        Cycle       => r"CYCLE\b",
        // ARRAY / ROW
        Array       => r"ARRAY\b",
        Row         => r"ROW\b",
        // OVER / window functions
        Over        => r"OVER\b",
        // Additional type names
        Integer     => r"INTEGER\b",
        Numeric     => r"NUMERIC\b",
        Varchar     => r"VARCHAR\b",
        // CREATE TABLE AS, ALTER TABLE
        Alter       => r"ALTER\b",
        Add         => r"ADD\b",
        Unique      => r"UNIQUE\b",
        // RULE / TRIGGER
        Rule        => r"RULE\b",
        Trigger     => r"TRIGGER\b",
        Before      => r"BEFORE\b",
        After       => r"AFTER\b",
        Each        => r"EACH\b",
        Statement   => r"STATEMENT\b",
        Execute     => r"EXECUTE\b",
        Procedure   => r"PROCEDURE\b",
        Instead     => r"INSTEAD\b",
        Also        => r"ALSO\b",
        New         => r"NEW\b",
        Old         => r"OLD\b",
        // Transaction
        Begin       => r"BEGIN\b",
        Commit      => r"COMMIT\b",
        // TRUNCATE
        Truncate    => r"TRUNCATE\b",
        // NOTIFY
        Notify      => r"NOTIFY\b",
        // INHERITS
        Inherits    => r"INHERITS\b",
        // REFERENCES
        References  => r"REFERENCES\b",
        // GENERATED / ALWAYS / IDENTITY
        Generated   => r"GENERATED\b",
        Always      => r"ALWAYS\b",
        Identity    => r"IDENTITY\b",
        // LOCAL
        Local       => r"LOCAL\b",
        // ANY / SOME
        Any         => r"ANY\b",
        // SUM / COUNT / MAX / MIN etc -- just identifiers, but need to not block
        // BETWEEN
        Between     => r"BETWEEN\b",
        // LIKE
        Like        => r"LIKE\b",
        // CASE WHEN
        Case        => r"CASE\b",
        Else        => r"ELSE\b",
        End         => r"END\b",
        // Verbose
        Verbose     => r"VERBOSE\b",
        // IF
        If          => r"IF\b",
        // ONLY (for UPDATE/DELETE ONLY)
        Only        => r"ONLY\b",
        // Or (already used for expr but need it as keyword for CREATE OR REPLACE)
        // INHERITS
        //Inherits already declared above
        // REFERENCES already declared above
        // NOT NULL constraint -- Not already declared
        // GENERATED ALWAYS AS IDENTITY -- Generated, Always, Identity already declared
        No          => r"NO\b",
        // Transaction control
        Rollback    => r"ROLLBACK\b",
        Savepoint   => r"SAVEPOINT\b",
        Release     => r"RELEASE\b",
        // PREPARE / EXECUTE / DEALLOCATE
        Prepare     => r"PREPARE\b",
        Deallocate  => r"DEALLOCATE\b",
        // GRANT / REVOKE
        Grant       => r"GRANT\b",
        Revoke      => r"REVOKE\b",
        // COMMENT
        Comment     => r"COMMENT\b",
        // COPY
        Copy        => r"COPY\b",
        // LOCK
        Lock        => r"LOCK\b",
        // Cursor operations
        Declare     => r"DECLARE\b",
        Fetch       => r"FETCH\b",
        Close       => r"CLOSE\b",
        Move        => r"MOVE\b",
        Cursor      => r"CURSOR\b",
        // REINDEX
        Reindex     => r"REINDEX\b",
        // REFRESH
        Refresh     => r"REFRESH\b",
        // DO
        DoBlock     => r"DO\b",
        // LISTEN / UNLISTEN
        Listen      => r"LISTEN\b",
        Unlisten    => r"UNLISTEN\b",
        // DISCARD
        Discard     => r"DISCARD\b",
        // REASSIGN
        Reassign    => r"REASSIGN\b",
        // SECURITY LABEL
        Security    => r"SECURITY\b",
        Label       => r"LABEL\b",
        // CLUSTER
        Clusterw    => r"CLUSTER\b",
        // VACUUM
        Vacuumw     => r"VACUUM\b",
    }
}

/// Punctuation
pub mod punct {
    recursa::punctuation! {
        Semi      => ";",          ";",
        Comma     => ",",          ",",
        LParen    => r"\(",        "(",
        RParen    => r"\)",        ")",
        Star      => r"\*",        "*",
        Dot       => r"\.",        ".",
        Eq        => "=",          "=",
        BangEq    => "!=",         "!=",
        Neq       => "<>",         "<>",
        Lte       => "<=",         "<=",
        Gte       => ">=",         ">=",
        Lt        => "<",          "<",
        Gt        => ">",          ">",
        ColonColon => "::",        "::",
        BackSlash  => r"\\",       "\\",
        Plus       => r"\+",       "+",
        Minus      => "-",         "-",
        DollarNum  => r"\$[0-9]+", "$",
        Concat     => r"\|\|",     "||",
        Slash      => "/",         "/",
        Percent    => "%",         "%",
        LBracket   => r"\[",       "[",
        RBracket   => r"\]",       "]",
    }
}

// Literals
pub mod literal {
    use super::*;

    recursa::literals! {
        DollarStringLit => r"\$[a-zA-Z_]*\$[\s\S]*?\$[a-zA-Z_]*\$",
        StringLit  => r"'[^']*(?:''[^']*)*'",
        NumericLit => r"[0-9]+\.[0-9]+",
        IntegerLit => r"[0-9]+",
    }

    // --- Identifier ---

    /// All SQL keywords (uppercase) for identifier exclusion.
    const SQL_KEYWORDS: &[&str] = &[
        "SELECT",
        "FROM",
        "WHERE",
        "AS",
        "AND",
        "OR",
        "NOT",
        "TRUE",
        "FALSE",
        "NULL",
        "IS",
        "UNKNOWN",
        "CREATE",
        "TABLE",
        "INSERT",
        "INTO",
        "VALUES",
        "DROP",
        "DELETE",
        "ORDER",
        "BY",
        "BOOL",
        "BOOLEAN",
        "TEXT",
        "INT",
        "SERIAL",
        "PRIMARY",
        "KEY",
        "ASC",
        "DESC",
        "NULLS",
        "FIRST",
        "LAST",
        "USING",
        "OFFSET",
        "LIMIT",
        "ANALYZE",
        "SET",
        "RESET",
        "TO",
        "ON",
        "OFF",
        "TEMP",
        "INDEX",
        "EXPLAIN",
        "FOR",
        "UPDATE",
        "FUNCTION",
        "RETURNS",
        "SETOF",
        "LANGUAGE",
        "IMMUTABLE",
        "UNION",
        "ALL",
        "IN",
        "DEFAULT",
        "LATERAL",
        "PARTITION",
        "OF",
        "COSTS",
        "TIMING",
        "SUMMARY",
        // New keywords for WITH support
        "WITH",
        "RECURSIVE",
        "MATERIALIZED",
        "EXCEPT",
        "INTERSECT",
        "DISTINCT",
        "JOIN",
        "LEFT",
        "RIGHT",
        "FULL",
        "INNER",
        "CROSS",
        "GROUP",
        "HAVING",
        "RETURNING",
        "MERGE",
        "WHEN",
        "MATCHED",
        "THEN",
        "CONFLICT",
        "DO",
        "NOTHING",
        "EXCLUDED",
        "VIEW",
        "REPLACE",
        "TEMPORARY",
        "EXISTS",
        "SEARCH",
        "DEPTH",
        "BREADTH",
        "CYCLE",
        "ARRAY",
        "ROW",
        "OVER",
        "INTEGER",
        "NUMERIC",
        "VARCHAR",
        "ALTER",
        "ADD",
        "UNIQUE",
        "RULE",
        "TRIGGER",
        "BEFORE",
        "AFTER",
        "EACH",
        "STATEMENT",
        "EXECUTE",
        "PROCEDURE",
        "INSTEAD",
        "ALSO",
        "NEW",
        "OLD",
        "BEGIN",
        "COMMIT",
        "TRUNCATE",
        "NOTIFY",
        "INHERITS",
        "REFERENCES",
        "GENERATED",
        "ALWAYS",
        "IDENTITY",
        "LOCAL",
        "ANY",
        "BETWEEN",
        "LIKE",
        "CASE",
        "ELSE",
        "END",
        "VERBOSE",
        "IF",
        "ONLY",
        "NO",
        "ROLLBACK",
        "SAVEPOINT",
        "RELEASE",
        "PREPARE",
        "DEALLOCATE",
        "GRANT",
        "REVOKE",
        "COMMENT",
        "COPY",
        "LOCK",
        "DECLARE",
        "FETCH",
        "CLOSE",
        "MOVE",
        "CURSOR",
        "REINDEX",
        "REFRESH",
        "LISTEN",
        "UNLISTEN",
        "DISCARD",
        "REASSIGN",
        "SECURITY",
        "LABEL",
        "CLUSTER",
        "VACUUM",
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

    impl recursa::FormatTokens for Ident {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.clone()));
        }
    }

    // --- Alias name (any SQL word — identifier or keyword) ---

    /// Matches any SQL word including keywords. Used for alias names where
    /// SQL allows keywords (e.g., `SELECT 1 AS true`).
    #[derive(Scan, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    #[visit(terminal)]
    pub struct AliasName(pub String);

    impl recursa::FormatTokens for AliasName {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.clone()));
        }
    }

    // --- Rest of line ---

    /// Matches the remainder of text on the current line (up to newline or end of input).
    #[derive(Scan, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[scan(pattern = r"[^\n]*")]
    #[visit(terminal)]
    pub struct RestOfLine(pub String);

    impl recursa::FormatTokens for RestOfLine {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.clone()));
        }
    }
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
