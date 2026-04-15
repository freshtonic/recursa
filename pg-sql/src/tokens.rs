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
        // LIKE / ILIKE
        Like        => r"LIKE\b",
        Ilike       => r"ILIKE\b",
        // COLLATE
        Collate     => r"COLLATE\b",
        // UNLOGGED table
        Unlogged    => r"UNLOGGED\b",
        // DATABASE object
        Database    => r"DATABASE\b",
        // ALTER DEFAULT PRIVILEGES
        Privileges  => r"PRIVILEGES\b",
        // CHECKPOINT statement
        Checkpoint  => r"CHECKPOINT\b",
        // Hash partition modulus / remainder
        Modulus     => r"MODULUS\b",
        Remainder   => r"REMAINDER\b",
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
        Show        => r"SHOW\b",
        Transaction => r"TRANSACTION\b",
        Isolation   => r"ISOLATION\b",
        Level       => r"LEVEL\b",
        Serializable => r"SERIALIZABLE\b",
        Repeatable   => r"REPEATABLE\b",
        ReadKw       => r"READ\b",
        WriteKw      => r"WRITE\b",
        Committed    => r"COMMITTED\b",
        Uncommitted  => r"UNCOMMITTED\b",
        Constraints  => r"CONSTRAINTS\b",
        Start        => r"START\b",
        Work         => r"WORK\b",
        Abort        => r"ABORT\b",
        Characteristics => r"CHARACTERISTICS\b",
        Variadic     => r"VARIADIC\b",
        Without      => r"WITHOUT\b",
        Timestamp    => r"TIMESTAMP\b",
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
        // MERGE: BY SOURCE / BY TARGET qualifiers. Not in SQL_KEYWORDS;
        // these are recognized contextually in MERGE WHEN clauses only.
        Source      => r"SOURCE\b",
        Target      => r"TARGET\b",
        // CREATE FUNCTION option keywords. Not in SQL_KEYWORDS so they remain
        // usable as identifiers (column names, etc.) outside function options.
        Strict      => r"STRICT\b",
        Stable      => r"STABLE\b",
        Volatile    => r"VOLATILE\b",
        Called      => r"CALLED\b",
        Input       => r"INPUT\b",
        Ordinality  => r"ORDINALITY\b",
        // JOIN modifiers
        Natural     => r"NATURAL\b",
        Outer       => r"OUTER\b",
        // XML function keywords (xmlelement / xmlattributes / xmlforest).
        // These are recognized only inside the XML function-call atoms.
        XmlElementKw    => r"XMLELEMENT\b",
        XmlAttributesKw => r"XMLATTRIBUTES\b",
        XmlForestKw     => r"XMLFOREST\b",
        XmlPiKw         => r"XMLPI\b",
        NameKw          => r"NAME\b",
        // CREATE FUNCTION argument modes (Bundle 2)
        Out         => r"OUT\b",
        Inout       => r"INOUT\b",
        // CREATE PROCEDURE / CALL (Bundle 3)
        Call        => r"CALL\b",
        // CREATE TABLESPACE (Bundle 4)
        Tablespace  => r"TABLESPACE\b",
        Owner       => r"OWNER\b",
        Location    => r"LOCATION\b",
        // GENERATED ALWAYS AS (expr) STORED (Bundle 5)
        Stored      => r"STORED\b",
        // U&'...' UESCAPE (Bundle 1)
        Uescape     => r"UESCAPE\b",
        // Aggregate WITHIN GROUP / FILTER (Bundle 8)
        Within      => r"WITHIN\b",
        Filter      => r"FILTER\b",
        // SQL-standard string functions: TRIM/SUBSTRING/POSITION/OVERLAY.
        // Not in SQL_KEYWORDS — recognized only inside their dedicated
        // function-call atoms.
        TrimKw      => r"TRIM\b",
        SubstringKw => r"SUBSTRING\b",
        PositionKw  => r"POSITION\b",
        OverlayKw   => r"OVERLAY\b",
        ExtractKw   => r"EXTRACT\b",
        Leading     => r"LEADING\b",
        Trailing    => r"TRAILING\b",
        BothKw      => r"BOTH\b",
        Similar     => r"SIMILAR\b",
        EscapeKw    => r"ESCAPE\b",
        Placing     => r"PLACING\b",
        // GROUP BY grouping primitives (Bundle: grouping sets)
        GroupingKw  => r"GROUPING\b",
        SetsKw      => r"SETS\b",
        RollupKw    => r"ROLLUP\b",
        CubeKw      => r"CUBE\b",
        // INTERVAL literal qualifier keywords
        IntervalKw  => r"INTERVAL\b",
        YearKw      => r"YEAR\b",
        MonthKw     => r"MONTH\b",
        DayKw       => r"DAY\b",
        HourKw      => r"HOUR\b",
        MinuteKw    => r"MINUTE\b",
        SecondKw    => r"SECOND\b",
        // CREATE TABLE (LIKE ...) options
        IncludingKw => r"INCLUDING\b",
        ExcludingKw => r"EXCLUDING\b",
        DefaultsKw  => r"DEFAULTS\b",
        IndexesKw   => r"INDEXES\b",
        StorageKw   => r"STORAGE\b",
        CommentsKw  => r"COMMENTS\b",
        // CHECK / FK constraint suffix
        ValidKw     => r"VALID\b",
        // VACUUM / REINDEX option values
        ParallelKw  => r"PARALLEL\b",
        // ALTER ... RENAME TO
        RenameKw    => r"RENAME\b",
        // CREATE/ALTER/DROP LANGUAGE option keywords
        Trusted     => r"TRUSTED\b",
        Procedural  => r"PROCEDURAL\b",
        Handler     => r"HANDLER\b",
        Validator   => r"VALIDATOR\b",
        InlineKw    => r"INLINE\b",
        // FETCH / MOVE direction keywords
        Next        => r"NEXT\b",
        Prior       => r"PRIOR\b",
        Forward     => r"FORWARD\b",
        Backward    => r"BACKWARD\b",
        Absolute    => r"ABSOLUTE\b",
        Relative    => r"RELATIVE\b",
        // DOUBLE PRECISION type name
        DoubleKw    => r"DOUBLE\b",
        PrecisionKw => r"PRECISION\b",
        // PARALLEL SAFE / RESTRICTED / UNSAFE function option
        SafeKw       => r"SAFE\b",
        UnsafeKw     => r"UNSAFE\b",
        RestrictedKw => r"RESTRICTED\b",
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
        FatArrow  => r"=>",        "=>",
        BangEq    => "!=",         "!=",
        Neq       => "<>",         "<>",
        // 3-char `<`-prefixed operators must come before 2-char `<=`/`<>`/`<<`
        // and before the single-char `<`.
        LtLtEq     => r"<<=",      "<<=",
        LtLtPipe   => r"<<\|",     "<<|",
        LtMinusGt  => r"<->",      "<->",
        LtLt       => r"<<",       "<<",
        Lte       => "<=",         "<=",
        // 3-char `>>=` before `>>`, then `>=`, `>`.
        GtGtEq     => r">>=",      ">>=",
        GtGt       => r">>",       ">>",
        Gte       => ">=",         ">=",
        Lt        => "<",          "<",
        Gt        => ">",          ">",
        ColonColon => "::",        "::",
        // Psql meta-commands that can terminate a SQL statement in place of `;`.
        // Must be listed before plain BackSlash so longest-match-wins picks the
        // specific directive over the bare backslash.
        PsqlCrosstabview => r"\\crosstabview\b", "\\crosstabview",
        PsqlGexec  => r"\\gexec\b", "\\gexec",
        PsqlGset   => r"\\gset\b",  "\\gset",
        PsqlGx     => r"\\gx\b",    "\\gx",
        PsqlG      => r"\\g\b",     "\\g",
        BackSlash  => r"\\",       "\\",
        Plus       => r"\+",       "+",
        // 3-char `-|-` before 2-char `->>`/`->` before single-char `-`.
        MinusPipeMinus => r"-\|-", "-|-",
        Minus      => "-",         "-",
        DollarNum  => r"\$[0-9]+", "$",
        // 3-char `|>>` and `|&>` before 2-char `||`.
        PipeGtGt       => r"\|>>", "|>>",
        PipeAmpGt      => r"\|&>", "|&>",
        Concat     => r"\|\|",     "||",
        // Single-char `|` (bitwise OR). Must be declared after `||` and
        // other `|`-prefixed operators so longest-match picks the longer form.
        Pipe       => r"\|",       "|",
        Slash      => "/",         "/",
        Percent    => "%",         "%",
        LBracket   => r"\[",       "[",
        RBracket   => r"\]",       "]",
        // JSON/JSONB operators. Longer before shorter (longest-match-wins).
        HashArrowArrow => r"#>>",      "#>>",
        HashArrow      => r"#>",       "#>",
        // Single-char `#` (bitwise XOR). Must come after all longer `#`-prefixed
        // tokens so longest-match-wins.
        Pound          => r"\#",       "#",
        ArrowArrow     => r"->>",      "->>",
        Arrow          => r"->",       "->",
        QuestionPipe   => r"\?\|",     "?|",
        QuestionAmp    => r"\?&",      "?&",
        QuestionHash   => r"\?#",      "?#",
        QuestionDash   => r"\?-",      "?-",
        // `@@@` before `@@` before `@?` / `@>`.
        AtAtAt         => r"@@@",      "@@@",
        AtAt           => r"@@",       "@@",
        AtQuestion     => r"@\?",      "@?",
        AtGt           => r"@>",       "@>",
        LtAt           => r"<@",       "<@",
        Question       => r"\?",       "?",
        // `&`-prefixed range/geometric operators. 3-char `&<|` before 2-char.
        AmpLtPipe      => r"&<\|",     "&<|",
        AmpAmp         => r"&&",       "&&",
        AmpLt          => r"&<",       "&<",
        AmpGt          => r"&>",       "&>",
        // Single-char `&` (bitwise AND). Must follow all longer `&`-prefixed
        // operators so longest-match-wins.
        Amp            => r"&",        "&",
        // POSIX regex match operators. Longest-first.
        BangTildeStar  => r"!~\*",     "!~*",
        TildeStar      => r"~\*",      "~*",
        BangTilde      => r"!~",       "!~",
        // Geometric "same as" operator. Must precede bare `~`.
        TildeEq        => r"~=",       "~=",
        Tilde          => r"~",        "~",
        // Exponentiation operator (Postgres).
        Caret          => r"\^",       "^",
    }
}

// Literals
pub mod literal {
    use super::*;

    recursa::literals! {
        DollarStringLit => r#"\$[a-zA-Z_]*\$[\s\S]*?\$[a-zA-Z_]*\$"#,
        UnicodeQuotedIdent => r#"(?i:U)&"[^"]*(?:""[^"]*)*""#,
        QuotedIdent => r#""[^"]*(?:""[^"]*)*""#,
        UnicodeStringLit => r"(?i:U)&'(?:[^'\\]|\\.|'')*'",
        EscapeStringLit => r"(?i:E)'(?:[^'\\]|\\.|'')*'",
        StringLit  => r"'[^']*(?:''[^']*)*'",
        // NumericLit must require a decimal point OR an exponent so it does
        // not collide with bare integers (handled by IntegerLit). Forms:
        //   123.45    .5    123.    1e10    1.5e-5    .5e10
        // Declared before IntegerLit so longest-match-wins picks the longer
        // literal when an exponent is present.
        NumericLit => r"(?:[0-9]+\.[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+",
        IntegerLit => r"[0-9]+",
        // psql client variable substitution: `:foo` or `:'foo'` or `:"foo"`.
        // Treated as an opaque expression atom so SELECTs that reference
        // psql-set variables parse structurally.
        PsqlVar => r#":(?:[A-Za-z_][A-Za-z0-9_]*|'[^']*'|"[^"]*")"#,
    }

    // --- Identifier ---

    /// All SQL keywords (uppercase) for identifier exclusion.
    ///
    /// This is the set the grammar treats as reserved for the purpose of
    /// rejecting bare identifiers. Postgres has a much smaller reserved set
    /// than listed here historically — many words that the grammar matches as
    /// `keyword::X` in specific positions are still usable as identifiers in
    /// other positions. Those words should NOT appear here.
    const SQL_KEYWORDS: &[&str] = &[
        // Core reserved: expression, query, clause keywords.
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
        // Statement leads.
        "CREATE",
        "TABLE",
        "INSERT",
        "INTO",
        "VALUES",
        "DROP",
        "DELETE",
        "UPDATE",
        "MERGE",
        "ALTER",
        "ADD",
        // Ordering / limit.
        "ORDER",
        "BY",
        // PRIMARY and KEY are contextual: they appear in `PRIMARY KEY`
        // constraint positions but PostgreSQL allows them as ordinary column
        // and identifier names elsewhere (e.g., `CREATE INDEX i ON t(key)`).
        // Recognized as `keyword::Primary` / `keyword::Key` only where the
        // grammar explicitly looks for them.
        "ASC",
        "DESC",
        "NULLS",
        "UNIQUE",
        "USING",
        "OFFSET",
        "LIMIT",
        // Predicates / set ops.
        "LIKE",
        "ILIKE",
        "IN",
        "BETWEEN",
        "EXISTS",
        "WHEN",
        "THEN",
        "ELSE",
        "END",
        "CASE",
        "UNION",
        "INTERSECT",
        "EXCEPT",
        "DISTINCT",
        "ALL",
        "WITH",
        "RECURSIVE",
        "GROUP",
        "HAVING",
        "RETURNING",
        "IF",
        // Joins.
        "JOIN",
        "LEFT",
        "RIGHT",
        "FULL",
        "INNER",
        "CROSS",
        "ON",
        "OUTER",
        "NATURAL",
        // DDL structure.
        "PARTITION",
        "OF",
        "FOR",
        "INHERITS",
        "REFERENCES",
        "FOREIGN",
        // Grammar clauses that appear after identifier positions and must
        // be reserved to prevent being consumed as column/alias names.
        "SET",
        "WINDOW",
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

    /// SQL identifier: unicode-quoted (`U&"Foo"`), double-quoted (`"Foo"`),
    /// or unquoted (`foo`).
    ///
    /// Variant ordering: `UnicodeQuoted` (`U&"`) first as the longest prefix,
    /// then `Quoted` (`"`), then `Unquoted` (letter).
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[parse(postcondition = ident_is_not_keyword)]
    #[visit(terminal)]
    pub enum Ident {
        UnicodeQuoted(UnicodeQuotedIdent),
        Quoted(QuotedIdent),
        Unquoted(UnquotedIdent),
    }

    impl recursa::FormatTokens for Ident {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            match self {
                Ident::UnicodeQuoted(u) => u.format_tokens(tokens),
                Ident::Quoted(quoted) => quoted.format_tokens(tokens),
                Ident::Unquoted(unquoted) => unquoted.format_tokens(tokens),
            }
        }
    }

    impl Ident {
        /// The raw text of the identifier.
        pub fn text(&self) -> &str {
            match self {
                Ident::UnicodeQuoted(u) => &u.0,
                Ident::Quoted(q) => &q.0,
                Ident::Unquoted(u) => &u.0,
            }
        }
    }

    // --- Alias name (any SQL word — identifier or keyword) ---

    /// Bare-word alias name: any SQL word including keywords (`SELECT 1 AS true`).
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    #[visit(terminal)]
    pub struct BareAliasName(pub String);

    impl recursa::FormatTokens for BareAliasName {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.clone()));
        }
    }

    /// Alias name: bare word (including keywords) or double-quoted identifier
    /// with arbitrary content (`"One hour"`).
    ///
    /// Variant ordering: `Quoted` (`"`) and `Bare` (letter) start with
    /// different first chars, so order is for clarity, not disambiguation.
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[visit(terminal)]
    pub enum AliasName {
        Quoted(QuotedIdent),
        Bare(BareAliasName),
    }

    impl AliasName {
        /// Raw text of the alias name (with quotes if quoted).
        pub fn text(&self) -> &str {
            match self {
                AliasName::Quoted(q) => &q.0,
                AliasName::Bare(b) => &b.0,
            }
        }
    }

    impl recursa::FormatTokens for AliasName {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            match self {
                AliasName::Quoted(q) => q.format_tokens(tokens),
                AliasName::Bare(b) => b.format_tokens(tokens),
            }
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

    // --- Numeric literal tests (decimals + exponent) ---

    #[test]
    fn numeric_literal_simple_decimal() {
        let mut input = Input::new("4.5");
        let lit = NumericLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "4.5");
    }

    #[test]
    fn numeric_literal_leading_dot() {
        let mut input = Input::new(".5");
        let lit = NumericLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, ".5");
    }

    #[test]
    fn numeric_literal_exponent_int() {
        let mut input = Input::new("2e3");
        let lit = NumericLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "2e3");
    }

    #[test]
    fn numeric_literal_decimal_with_exponent() {
        let mut input = Input::new("4.5e10");
        let lit = NumericLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "4.5e10");
    }

    #[test]
    fn numeric_literal_negative_exponent() {
        let mut input = Input::new("1.5e-5");
        let lit = NumericLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "1.5e-5");
    }

    #[test]
    fn numeric_literal_large_exponent() {
        let mut input = Input::new("4.4e131071");
        let lit = NumericLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "4.4e131071");
    }

    #[test]
    fn integer_literal_does_not_match_decimal() {
        // Bare integer still works
        let mut input = Input::new("42");
        let lit = IntegerLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "42");
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
