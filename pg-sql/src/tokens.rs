use recursa::{Parse, ParseError, Visit};

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
        // Additional DDL object types
        Role        => r"ROLE\b",
        User        => r"USER\b",
        Schema      => r"SCHEMA\b",
        Sequence    => r"SEQUENCE\b",
        Type        => r"TYPE\b",
        Domain      => r"DOMAIN\b",
        Aggregate   => r"AGGREGATE\b",
        Operator    => r"OPERATOR\b",
        Cast        => r"CAST\b",
        Collation   => r"COLLATION\b",
        Extension   => r"EXTENSION\b",
        Foreign     => r"FOREIGN\b",
        Policy      => r"POLICY\b",
        Statistics  => r"STATISTICS\b",
        Publication => r"PUBLICATION\b",
        Subscription => r"SUBSCRIPTION\b",
        Owned       => r"OWNED\b",
        Concurrently => r"CONCURRENTLY\b",
        Access      => r"ACCESS\b",
        Method      => r"METHOD\b",
        Conversion  => r"CONVERSION\b",
        Server      => r"SERVER\b",
        Wrapper     => r"WRAPPER\b",
        Mapping     => r"MAPPING\b",
        Event       => r"EVENT\b",
        // Constraint-related keywords
        Constraint  => r"CONSTRAINT\b",
        Check       => r"CHECK\b",
        Match       => r"MATCH\b",
        Partial     => r"PARTIAL\b",
        Simple      => r"SIMPLE\b",
        Restrict    => r"RESTRICT\b",
        Action      => r"ACTION\b",
        Deferrable  => r"DEFERRABLE\b",
        Initially   => r"INITIALLY\b",
        Deferred    => r"DEFERRED\b",
        Immediate   => r"IMMEDIATE\b",
        Inherit     => r"INHERIT\b",
        Cascade     => r"CASCADE\b",
        Include     => r"INCLUDE\b",
        // Index method keywords
        Btree       => r"BTREE\b",
        Gin         => r"GIN\b",
        Gist        => r"GIST\b",
        Hash        => r"HASH\b",
        Spgist      => r"SPGIST\b",
        Brin        => r"BRIN\b",
        // SET / RESET extension keywords. Deliberately NOT added to
        // SQL_KEYWORDS so they remain usable as ordinary identifiers
        // (e.g., column names `session`, `time`, etc.). They are only
        // recognized as keywords in positions where the grammar
        // explicitly looks for them.
        Session     => r"SESSION\b",
        Authorization => r"AUTHORIZATION\b",
        Time        => r"TIME\b",
        Zone        => r"ZONE\b",
        None        => r"NONE\b",
        // Window function keywords (frame clauses, named windows). Not in
        // SQL_KEYWORDS for the same reason: they can still appear as
        // identifiers outside window grammar contexts.
        Window      => r"WINDOW\b",
        Rows        => r"ROWS\b",
        RangeKw     => r"RANGE\b",
        Groups      => r"GROUPS\b",
        Unbounded   => r"UNBOUNDED\b",
        Preceding   => r"PRECEDING\b",
        Following   => r"FOLLOWING\b",
        CurrentKw   => r"CURRENT\b",
        Excludew    => r"EXCLUDE\b",
        Others      => r"OTHERS\b",
        Ties        => r"TIES\b",
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
        // JSON/JSONB operators. Longer before shorter (longest-match-wins).
        HashArrowArrow => r"#>>",      "#>>",
        HashArrow      => r"#>",       "#>",
        ArrowArrow     => r"->>",      "->>",
        Arrow          => r"->",       "->",
        QuestionPipe   => r"\?\|",     "?|",
        QuestionAmp    => r"\?&",      "?&",
        AtGt           => r"@>",       "@>",
        LtAt           => r"<@",       "<@",
        Question       => r"\?",       "?",
    }
}

// Literals
pub mod literal {
    use super::*;

    recursa::literals! {
        DollarStringLit => r#"\$[a-zA-Z_]*\$[\s\S]*?\$[a-zA-Z_]*\$"#,
        QuotedIdent => r#""[^"]*(?:""[^"]*)*""#,
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
        "ROLE",
        "USER",
        "SCHEMA",
        "SEQUENCE",
        "TYPE",
        "DOMAIN",
        "AGGREGATE",
        "OPERATOR",
        "CAST",
        "COLLATION",
        "EXTENSION",
        "FOREIGN",
        "POLICY",
        "STATISTICS",
        "PUBLICATION",
        "SUBSCRIPTION",
        "OWNED",
        "CONCURRENTLY",
        "ACCESS",
        "METHOD",
        "CONVERSION",
        "SERVER",
        "WRAPPER",
        "MAPPING",
        "EVENT",
        "CONSTRAINT",
        "CHECK",
        "MATCH",
        "PARTIAL",
        "SIMPLE",
        "RESTRICT",
        "ACTION",
        "DEFERRABLE",
        "INITIALLY",
        "DEFERRED",
        "IMMEDIATE",
        "INHERIT",
        "CASCADE",
        "INCLUDE",
        "BTREE",
        "GIN",
        "GIST",
        "HASH",
        "SPGIST",
        "BRIN",
        // Window function keywords (Bundle 6).
        "WINDOW",
        "ROWS",
        "RANGE",
        "GROUPS",
        "UNBOUNDED",
        "PRECEDING",
        "FOLLOWING",
        "CURRENT",
        "EXCLUDE",
        "OTHERS",
        "TIES",
    ];

    fn is_keyword(s: &str) -> bool {
        SQL_KEYWORDS.iter().any(|kw| kw.eq_ignore_ascii_case(s))
    }

    /// Postcondition: reject identifiers that are SQL keywords.
    fn not_keyword(ident: &UnquotedIdent) -> Result<(), ParseError> {
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

    /// Postcondition: reject identifiers that are SQL keywords.
    fn ident_is_not_keyword(ident: &Ident) -> Result<(), ParseError> {
        if let Ident::Unquoted(unquoted) = ident {
            return not_keyword(unquoted);
        }
        Ok(())
    }

    /// Unquoted SQL identifier: `[a-zA-Z_][a-zA-Z0-9_]*` but NOT a keyword.
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", postcondition = not_keyword)]
    #[visit(terminal)]
    pub struct UnquotedIdent(pub String);

    impl recursa::FormatTokens for UnquotedIdent {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.clone()));
        }
    }

    /// SQL identifier: unquoted (`foo`) or double-quoted (`"Foo"`).
    ///
    /// Quoted before Unquoted so `"` is tried first (different first char).
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[parse(postcondition = ident_is_not_keyword)]
    #[visit(terminal)]
    pub enum Ident {
        Quoted(QuotedIdent),
        Unquoted(UnquotedIdent),
    }

    impl recursa::FormatTokens for Ident {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            match self {
                Ident::Quoted(quoted) => quoted.format_tokens(tokens),
                Ident::Unquoted(unquoted) => unquoted.format_tokens(tokens),
            }
        }
    }

    impl Ident {
        /// The raw text of the identifier.
        pub fn text(&self) -> &str {
            match self {
                Ident::Quoted(q) => &q.0,
                Ident::Unquoted(u) => &u.0,
            }
        }
    }

    // --- Alias name (any SQL word — identifier or keyword) ---

    /// Matches any SQL word including keywords. Used for alias names where
    /// SQL allows keywords (e.g., `SELECT 1 AS true`).
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    #[visit(terminal)]
    pub struct AliasName(pub String);

    impl recursa::FormatTokens for AliasName {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.clone()));
        }
    }

    // --- Rest of line ---

    /// Matches the remainder of text on the current line (up to newline or end of input).
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[parse(pattern = r"[^\n]*")]
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
        assert!(Select::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_select_lowercase() {
        let input = Input::new("select");
        assert!(Select::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_select_mixed_case() {
        let input = Input::new("SeLeCt");
        assert!(Select::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_select_not_prefix_of_identifier() {
        let input = Input::new("SELECTED");
        assert!(!Select::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_bool_not_prefix_of_booleq() {
        let input = Input::new("booleq");
        assert!(!Bool::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_bool_matches_standalone() {
        let input = Input::new("bool");
        assert!(Bool::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_boolean_matches() {
        let input = Input::new("BOOLEAN");
        assert!(Boolean::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_not_matches() {
        let input = Input::new("NOT");
        assert!(Not::peek::<NoRules>(&input));
    }

    // --- Punctuation tests ---

    #[test]
    fn punctuation_semicolon() {
        let mut input = Input::new(";");
        let _ = Semi::parse::<NoRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn punctuation_neq() {
        let input = Input::new("<>");
        assert!(Neq::peek::<NoRules>(&input));
    }

    #[test]
    fn punctuation_colon_colon() {
        let input = Input::new("::");
        assert!(ColonColon::peek::<NoRules>(&input));
    }

    #[test]
    fn punctuation_lte() {
        let input = Input::new("<=");
        assert!(Lte::peek::<NoRules>(&input));
    }

    #[test]
    fn punctuation_gte() {
        let input = Input::new(">=");
        assert!(Gte::peek::<NoRules>(&input));
    }

    // --- String literal tests ---

    #[test]
    fn string_literal_simple() {
        let mut input = Input::new("'hello world'");
        let lit = StringLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "'hello world'");
        assert!(input.is_empty());
    }

    #[test]
    fn string_literal_with_escaped_quote() {
        let mut input = Input::new("'it''s'");
        let lit = StringLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "'it''s'");
    }

    #[test]
    fn string_literal_empty() {
        let mut input = Input::new("''");
        let lit = StringLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "''");
    }

    #[test]
    fn string_literal_with_spaces() {
        let mut input = Input::new("'   f           '");
        let lit = StringLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "'   f           '");
    }

    // --- Integer literal tests ---

    #[test]
    fn integer_literal() {
        let mut input = Input::new("42");
        let lit = IntegerLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "42");
    }

    #[test]
    fn integer_literal_zero() {
        let mut input = Input::new("0");
        let lit = IntegerLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "0");
    }

    // --- Identifier tests ---

    #[test]
    fn identifier_simple() {
        let mut input = Input::new("my_table");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "my_table");
    }

    #[test]
    fn identifier_with_digits() {
        let mut input = Input::new("f1");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "f1");
    }

    #[test]
    fn identifier_uppercase() {
        let mut input = Input::new("BOOLTBL1");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "BOOLTBL1");
    }

    #[test]
    fn unquoted_rejects_keyword_select() {
        let input = Input::new("SELECT");
        assert!(!UnquotedIdent::peek::<NoRules>(&input));
    }

    #[test]
    fn unquoted_rejects_keyword_true() {
        let input = Input::new("true");
        assert!(!UnquotedIdent::peek::<NoRules>(&input));
    }

    #[test]
    fn unquoted_rejects_keyword_null() {
        let input = Input::new("NULL");
        assert!(!UnquotedIdent::peek::<NoRules>(&input));
    }

    #[test]
    fn ident_enum_rejects_keyword() {
        // Under the new Parse semantics, postcondition on enum wraps both peek and parse:
        // peek forks+runs parse, so peek returns false for a keyword input.
        let input = Input::new("SELECT");
        assert!(!Ident::peek::<NoRules>(&input));
        let mut input2 = Input::new("SELECT");
        assert!(Ident::parse::<NoRules>(&mut input2).is_err());
    }

    #[test]
    fn ident_enum_parses_quoted() {
        let mut input = Input::new("\"SELECT\"");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "\"SELECT\"");
        assert!(input.is_empty());
    }

    #[test]
    fn identifier_accepts_keyword_prefix() {
        let mut input = Input::new("isfalse");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "isfalse");
    }

    #[test]
    fn identifier_accepts_booleq() {
        let mut input = Input::new("booleq");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "booleq");
    }

    #[test]
    fn identifier_accepts_boolne() {
        let mut input = Input::new("boolne");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "boolne");
    }

    #[test]
    fn identifier_accepts_isnul() {
        let mut input = Input::new("isnul");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "isnul");
    }

    #[test]
    fn identifier_accepts_istrue() {
        let mut input = Input::new("istrue");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "istrue");
    }

    #[test]
    fn identifier_accepts_pg_input_is_valid() {
        let mut input = Input::new("pg_input_is_valid");
        let id = Ident::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(id.text(), "pg_input_is_valid");
    }
}

#[cfg(test)]
mod ident_enum_tests {
    use super::literal::*;
    use crate::rules::SqlRules;
    use recursa::{Input, Parse};

    #[test]
    fn ident_peek_rejects_from_keyword() {
        let input = Input::new("FROM");
        eprintln!(
            "Ident::peek(FROM, SqlRules) = {}",
            Ident::peek::<SqlRules>(&input)
        );
        assert!(
            !Ident::peek::<SqlRules>(&input),
            "Ident should not peek true for FROM"
        );
    }
}
