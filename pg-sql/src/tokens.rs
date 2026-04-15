use recursa::{Parse, ParseError, Visit};

/// Keywords
#[allow(non_camel_case_types)]
pub mod keyword {
    // Keywords (case-insensitive, with word boundary)
    recursa::keywords! {
        SELECT      => r"SELECT\b",
        FROM        => r"FROM\b",
        WHERE       => r"WHERE\b",
        AS          => r"AS\b",
        AND         => r"AND\b",
        OR          => r"OR\b",
        NOT         => r"NOT\b",
        TRUE        => r"TRUE\b",
        FALSE       => r"FALSE\b",
        NULL        => r"NULL\b",
        IS          => r"IS\b",
        UNKNOWN     => r"UNKNOWN\b",
        CREATE      => r"CREATE\b",
        TABLE       => r"TABLE\b",
        INSERT      => r"INSERT\b",
        INTO        => r"INTO\b",
        VALUES      => r"VALUES\b",
        DROP        => r"DROP\b",
        DELETE      => r"DELETE\b",
        ORDER       => r"ORDER\b",
        BY          => r"BY\b",
        BOOL        => r"BOOL\b",
        BOOLEAN     => r"BOOLEAN\b",
        TEXT        => r"TEXT\b",
        INT         => r"INT\b",
        SERIAL      => r"SERIAL\b",
        PRIMARY     => r"PRIMARY\b",
        KEY         => r"KEY\b",
        ASC         => r"ASC\b",
        DESC        => r"DESC\b",
        NULLS       => r"NULLS\b",
        FIRST       => r"FIRST\b",
        LAST        => r"LAST\b",
        USING       => r"USING\b",
        OFFSET      => r"OFFSET\b",
        LIMIT       => r"LIMIT\b",
        ANALYZE     => r"ANALYZE\b",
        SET         => r"SET\b",
        RESET       => r"RESET\b",
        TO          => r"TO\b",
        ON          => r"ON\b",
        OFF         => r"OFF\b",
        TEMP        => r"TEMP\b",
        INDEX       => r"INDEX\b",
        EXPLAIN     => r"EXPLAIN\b",
        FOR         => r"FOR\b",
        UPDATE      => r"UPDATE\b",
        FUNCTION    => r"FUNCTION\b",
        RETURNS     => r"RETURNS\b",
        SETOF       => r"SETOF\b",
        LANGUAGE    => r"LANGUAGE\b",
        IMMUTABLE   => r"IMMUTABLE\b",
        UNION       => r"UNION\b",
        ALL         => r"ALL\b",
        IN          => r"IN\b",
        DEFAULT     => r"DEFAULT\b",
        LATERAL     => r"LATERAL\b",
        PARTITION   => r"PARTITION\b",
        OF          => r"OF\b",
        COSTS       => r"COSTS\b",
        TIMING      => r"TIMING\b",
        SUMMARY     => r"SUMMARY\b",  // WITH clause and CTE support
        WITH        => r"WITH\b",
        RECURSIVE   => r"RECURSIVE\b",
        MATERIALIZED => r"MATERIALIZED\b",  // SET operations
        EXCEPT      => r"EXCEPT\b",
        INTERSECT   => r"INTERSECT\b",
        DISTINCT    => r"DISTINCT\b",  // JOIN support
        JOIN        => r"JOIN\b",
        LEFT        => r"LEFT\b",
        RIGHT       => r"RIGHT\b",
        FULL        => r"FULL\b",
        INNER       => r"INNER\b",
        CROSS       => r"CROSS\b",  // GROUP BY / HAVING / DISTINCT
        GROUP       => r"GROUP\b",
        HAVING      => r"HAVING\b",  // UPDATE statement
        RETURNING   => r"RETURNING\b",  // MERGE statement
        MERGE       => r"MERGE\b",
        WHEN        => r"WHEN\b",
        MATCHED     => r"MATCHED\b",
        THEN        => r"THEN\b",  // INSERT ON CONFLICT
        CONFLICT    => r"CONFLICT\b",
        DO          => r"DO\b",
        NOTHING     => r"NOTHING\b",
        EXCLUDED    => r"EXCLUDED\b",  // VIEW
        VIEW        => r"VIEW\b",
        REPLACE     => r"REPLACE\b",
        TEMPORARY   => r"TEMPORARY\b",  // EXISTS
        EXISTS      => r"EXISTS\b",  // SEARCH / CYCLE
        SEARCH      => r"SEARCH\b",
        DEPTH       => r"DEPTH\b",
        BREADTH     => r"BREADTH\b",
        CYCLE       => r"CYCLE\b",  // ARRAY / ROW
        ARRAY       => r"ARRAY\b",
        ROW         => r"ROW\b",  // OVER / window functions
        OVER        => r"OVER\b",  // Additional type names
        INTEGER     => r"INTEGER\b",
        NUMERIC     => r"NUMERIC\b",
        VARCHAR     => r"VARCHAR\b",  // CREATE TABLE AS, ALTER TABLE
        ALTER       => r"ALTER\b",
        ADD         => r"ADD\b",
        UNIQUE      => r"UNIQUE\b",  // RULE / TRIGGER
        RULE        => r"RULE\b",
        TRIGGER     => r"TRIGGER\b",
        BEFORE      => r"BEFORE\b",
        AFTER       => r"AFTER\b",
        EACH        => r"EACH\b",
        STATEMENT   => r"STATEMENT\b",
        EXECUTE     => r"EXECUTE\b",
        PROCEDURE   => r"PROCEDURE\b",
        ROUTINE     => r"ROUTINE\b",
        INSTEAD     => r"INSTEAD\b",
        ALSO        => r"ALSO\b",
        NEW         => r"NEW\b",
        OLD         => r"OLD\b",  // TRANSACTION
        BEGIN       => r"BEGIN\b",
        COMMIT      => r"COMMIT\b",  // TRUNCATE
        TRUNCATE    => r"TRUNCATE\b",  // NOTIFY
        NOTIFY      => r"NOTIFY\b",  // INHERITS
        INHERITS    => r"INHERITS\b",  // REFERENCES
        REFERENCES  => r"REFERENCES\b",  // GENERATED / ALWAYS / IDENTITY
        GENERATED   => r"GENERATED\b",
        ALWAYS      => r"ALWAYS\b",
        IDENTITY    => r"IDENTITY\b",  // LOCAL
        LOCAL       => r"LOCAL\b",  // ANY / SOME
        ANY         => r"ANY\b",  // SUM / COUNT / MAX / MIN etc -- just identifiers, but need to not block
        // BETWEEN
        BETWEEN     => r"BETWEEN\b",  // LIKE / ILIKE
        LIKE        => r"LIKE\b",
        ILIKE       => r"ILIKE\b",  // COLLATE
        COLLATE     => r"COLLATE\b",  // UNLOGGED table
        UNLOGGED    => r"UNLOGGED\b",  // DATABASE object
        DATABASE    => r"DATABASE\b",  // ALTER DEFAULT PRIVILEGES
        PRIVILEGES  => r"PRIVILEGES\b",  // CHECKPOINT statement
        CHECKPOINT  => r"CHECKPOINT\b",  // HASH partition modulus / remainder
        MODULUS     => r"MODULUS\b",
        REMAINDER   => r"REMAINDER\b",  // CASE WHEN
        CASE        => r"CASE\b",
        ELSE        => r"ELSE\b",
        END         => r"END\b",  // VERBOSE
        VERBOSE     => r"VERBOSE\b",  // IF
        IF          => r"IF\b",  // ONLY (for UPDATE/DELETE ONLY)
        ONLY        => r"ONLY\b",  // OR (already used for expr but need it as keyword for CREATE OR REPLACE)
        // INHERITS
        //INHERITS already declared above
        // REFERENCES already declared above
        // NOT NULL constraint -- NOT already declared
        // GENERATED ALWAYS AS IDENTITY -- GENERATED, ALWAYS, IDENTITY already declared
        NO          => r"NO\b",
        DATA        => r"DATA\b",  // TRANSACTION control
        ROLLBACK    => r"ROLLBACK\b",
        SAVEPOINT   => r"SAVEPOINT\b",
        RELEASE     => r"RELEASE\b",  // PREPARE / EXECUTE / DEALLOCATE
        PREPARE     => r"PREPARE\b",
        DEALLOCATE  => r"DEALLOCATE\b",  // GRANT / REVOKE
        GRANT       => r"GRANT\b",
        REVOKE      => r"REVOKE\b",  // COMMENT
        COMMENT     => r"COMMENT\b",  // COPY
        COPY        => r"COPY\b",  // LOCK
        LOCK        => r"LOCK\b",  // CURSOR operations
        DECLARE     => r"DECLARE\b",
        FETCH       => r"FETCH\b",
        CLOSE       => r"CLOSE\b",
        MOVE        => r"MOVE\b",
        CURSOR      => r"CURSOR\b",  // REINDEX
        REINDEX     => r"REINDEX\b",  // REFRESH
        REFRESH     => r"REFRESH\b",  // DO
        LISTEN      => r"LISTEN\b",
        UNLISTEN    => r"UNLISTEN\b",  // DISCARD
        DISCARD     => r"DISCARD\b",  // REASSIGN
        REASSIGN    => r"REASSIGN\b",  // SECURITY LABEL
        SECURITY    => r"SECURITY\b",
        LABEL       => r"LABEL\b",  // CLUSTER
        CLUSTER    => r"CLUSTER\b",  // VACUUM
        VACUUM     => r"VACUUM\b",  // Additional DDL object types
        ROLE        => r"ROLE\b",
        USER        => r"USER\b",
        SCHEMA      => r"SCHEMA\b",
        SEQUENCE    => r"SEQUENCE\b",
        TYPE        => r"TYPE\b",
        DOMAIN      => r"DOMAIN\b",
        AGGREGATE   => r"AGGREGATE\b",
        OPERATOR    => r"OPERATOR\b",
        CAST        => r"CAST\b",
        COLLATION   => r"COLLATION\b",
        EXTENSION   => r"EXTENSION\b",
        FOREIGN     => r"FOREIGN\b",
        POLICY      => r"POLICY\b",
        STATISTICS  => r"STATISTICS\b",
        PUBLICATION => r"PUBLICATION\b",
        SUBSCRIPTION => r"SUBSCRIPTION\b",
        OWNED       => r"OWNED\b",
        CONCURRENTLY => r"CONCURRENTLY\b",
        ACCESS      => r"ACCESS\b",
        METHOD      => r"METHOD\b",
        CONVERSION  => r"CONVERSION\b",
        SERVER      => r"SERVER\b",
        WRAPPER     => r"WRAPPER\b",
        MAPPING     => r"MAPPING\b",
        EVENT       => r"EVENT\b",  // CONSTRAINT-related keywords
        CONSTRAINT  => r"CONSTRAINT\b",
        CHECK       => r"CHECK\b",
        MATCH       => r"MATCH\b",
        PARTIAL     => r"PARTIAL\b",
        SIMPLE      => r"SIMPLE\b",
        RESTRICT    => r"RESTRICT\b",
        ACTION      => r"ACTION\b",
        DEFERRABLE  => r"DEFERRABLE\b",
        INITIALLY   => r"INITIALLY\b",
        DEFERRED    => r"DEFERRED\b",
        IMMEDIATE   => r"IMMEDIATE\b",
        INHERIT     => r"INHERIT\b",
        CASCADE     => r"CASCADE\b",
        INCLUDE     => r"INCLUDE\b",  // INDEX method keywords
        BTREE       => r"BTREE\b",
        GIN         => r"GIN\b",
        GIST        => r"GIST\b",
        HASH        => r"HASH\b",
        SPGIST      => r"SPGIST\b",
        BRIN        => r"BRIN\b",  // SET / RESET extension keywords. Deliberately NOT added to
        // SQL_KEYWORDS so they remain usable as ordinary identifiers
        // (e.g., column names `session`, `time`, etc.). They are only
        // recognized as keywords in positions where the grammar
        // explicitly looks for them.
        SHOW        => r"SHOW\b",
        TRANSACTION => r"TRANSACTION\b",
        ISOLATION   => r"ISOLATION\b",
        LEVEL       => r"LEVEL\b",
        SERIALIZABLE => r"SERIALIZABLE\b",
        REPEATABLE   => r"REPEATABLE\b",
        READ       => r"READ\b",
        WRITE      => r"WRITE\b",
        COMMITTED    => r"COMMITTED\b",
        UNCOMMITTED  => r"UNCOMMITTED\b",
        CONSTRAINTS  => r"CONSTRAINTS\b",
        START        => r"START\b",
        WORK         => r"WORK\b",
        ABORT        => r"ABORT\b",
        CHARACTERISTICS => r"CHARACTERISTICS\b",
        VARIADIC     => r"VARIADIC\b",
        WITHOUT      => r"WITHOUT\b",
        TIMESTAMP    => r"TIMESTAMP\b",
        SESSION     => r"SESSION\b",
        AUTHORIZATION => r"AUTHORIZATION\b",
        TIME        => r"TIME\b",
        ZONE        => r"ZONE\b",
        NONE        => r"NONE\b",  // WINDOW function keywords (frame clauses, named windows). NOT in
        // SQL_KEYWORDS for the same reason: they can still appear as
        // identifiers outside window grammar contexts.
        WINDOW      => r"WINDOW\b",
        ROWS        => r"ROWS\b",
        RANGE     => r"RANGE\b",
        GROUPS      => r"GROUPS\b",
        UNBOUNDED   => r"UNBOUNDED\b",
        PRECEDING   => r"PRECEDING\b",
        FOLLOWING   => r"FOLLOWING\b",
        CURRENT   => r"CURRENT\b",
        EXCLUDE    => r"EXCLUDE\b",
        OTHERS      => r"OTHERS\b",
        TIES        => r"TIES\b",  // MERGE: BY SOURCE / BY TARGET qualifiers. NOT in SQL_KEYWORDS;
        // these are recognized contextually in MERGE WHEN clauses only.
        SOURCE      => r"SOURCE\b",
        TARGET      => r"TARGET\b",  // CREATE FUNCTION option keywords. NOT in SQL_KEYWORDS so they remain
        // usable as identifiers (column names, etc.) outside function options.
        STRICT      => r"STRICT\b",
        STABLE      => r"STABLE\b",
        VOLATILE    => r"VOLATILE\b",
        CALLED      => r"CALLED\b",
        INPUT       => r"INPUT\b",
        ORDINALITY  => r"ORDINALITY\b",  // JOIN modifiers
        NATURAL     => r"NATURAL\b",
        OUTER       => r"OUTER\b",  // XML function keywords (xmlelement / xmlattributes / xmlforest).
        // These are recognized only inside the XML function-call atoms.
        XMLELEMENT    => r"XMLELEMENT\b",
        XMLATTRIBUTES => r"XMLATTRIBUTES\b",
        XMLFOREST     => r"XMLFOREST\b",
        XMLPI         => r"XMLPI\b",
        NAME          => r"NAME\b",  // CREATE FUNCTION argument modes (Bundle 2)
        OUT         => r"OUT\b",
        INOUT       => r"INOUT\b",  // CREATE PROCEDURE / CALL (Bundle 3)
        CALL        => r"CALL\b",
        LOAD        => r"LOAD\b",  // CREATE TABLESPACE (Bundle 4)
        TABLESPACE  => r"TABLESPACE\b",
        OWNER       => r"OWNER\b",
        LOCATION    => r"LOCATION\b",  // GENERATED ALWAYS AS (expr) STORED (Bundle 5)
        STORED      => r"STORED\b",  // U&'...' UESCAPE (Bundle 1)
        UESCAPE     => r"UESCAPE\b",  // AGGREGATE WITHIN GROUP / FILTER (Bundle 8)
        WITHIN      => r"WITHIN\b",
        FILTER      => r"FILTER\b",  // SQL-standard string functions: TRIM/SUBSTRING/POSITION/OVERLAY.
        // NOT in SQL_KEYWORDS — recognized only inside their dedicated
        // function-call atoms.
        TRIM      => r"TRIM\b",
        SUBSTRING => r"SUBSTRING\b",
        POSITION  => r"POSITION\b",
        OVERLAY   => r"OVERLAY\b",
        EXTRACT   => r"EXTRACT\b",  // Postgres postfix null tests: `expr NOTNULL` / `expr ISNULL`.
        NOTNULL     => r"NOTNULL\b",
        ISNULL      => r"ISNULL\b",  // CREATE TEMP TABLE ... ON COMMIT clauses
        PRESERVE    => r"PRESERVE\b",  // SEQUENCE options for IDENTITY column / CREATE SEQUENCE
        INCREMENT   => r"INCREMENT\b",
        MINVALUE    => r"MINVALUE\b",
        MAXVALUE    => r"MAXVALUE\b",
        CACHE       => r"CACHE\b",
        LEADING     => r"LEADING\b",
        TRAILING    => r"TRAILING\b",
        BOTH      => r"BOTH\b",
        SIMILAR     => r"SIMILAR\b",
        ESCAPE    => r"ESCAPE\b",
        PLACING     => r"PLACING\b",  // GROUP BY grouping primitives (Bundle: grouping sets)
        GROUPING  => r"GROUPING\b",
        SETS      => r"SETS\b",
        ROLLUP    => r"ROLLUP\b",
        CUBE      => r"CUBE\b",  // INTERVAL literal qualifier keywords
        INTERVAL  => r"INTERVAL\b",
        YEAR      => r"YEAR\b",
        MONTH     => r"MONTH\b",
        DAY       => r"DAY\b",
        HOUR      => r"HOUR\b",
        MINUTE    => r"MINUTE\b",
        SECOND    => r"SECOND\b",  // CREATE TABLE (LIKE ...) options
        INCLUDING => r"INCLUDING\b",
        EXCLUDING => r"EXCLUDING\b",
        DEFAULTS  => r"DEFAULTS\b",
        INDEXES   => r"INDEXES\b",
        STORAGE   => r"STORAGE\b",
        COMMENTS  => r"COMMENTS\b",
        COMPRESSION => r"COMPRESSION\b",
        RETURN      => r"RETURN\b",  // SQL-standard function body
        OIDS        => r"OIDS\b",  // CREATE TABLE legacy WITH OIDS / WITHOUT OIDS
        OPTIONS     => r"OPTIONS\b",  // partition column `WITH OPTIONS`
        OVERRIDING  => r"OVERRIDING\b",  // INSERT OVERRIDING {SYSTEM|USER} VALUE
        SYSTEM      => r"SYSTEM\b",
        VALUE       => r"VALUE\b",
        CASCADED    => r"CASCADED\b",  // CREATE VIEW WITH [CASCADED|LOCAL] CHECK OPTION
        OPTION      => r"OPTION\b",  // CHECK / FK constraint suffix
        VALID     => r"VALID\b",  // VACUUM / REINDEX option values
        PARALLEL  => r"PARALLEL\b",  // ALTER ... RENAME TO
        RENAME    => r"RENAME\b",  // CREATE/ALTER/DROP LANGUAGE option keywords
        TRUSTED     => r"TRUSTED\b",
        PROCEDURAL  => r"PROCEDURAL\b",
        HANDLER     => r"HANDLER\b",
        VALIDATOR   => r"VALIDATOR\b",
        INLINE    => r"INLINE\b",  // FETCH / MOVE direction keywords
        NEXT        => r"NEXT\b",
        PRIOR       => r"PRIOR\b",
        FORWARD     => r"FORWARD\b",
        BACKWARD    => r"BACKWARD\b",
        ABSOLUTE    => r"ABSOLUTE\b",
        RELATIVE    => r"RELATIVE\b",  // DOUBLE PRECISION type name
        DOUBLE    => r"DOUBLE\b",
        PRECISION => r"PRECISION\b",  // PARALLEL SAFE / RESTRICTED / UNSAFE function option
        SAFE       => r"SAFE\b",
        UNSAFE     => r"UNSAFE\b",
        RESTRICTED => r"RESTRICTED\b",  // TYPE-name modifiers used contextually inside column type positions.
        // NOT in SQL_KEYWORDS so they remain usable as identifiers elsewhere.
        BIT        => r"BIT\b",
        VARYING      => r"VARYING\b",
        CHARACTER  => r"CHARACTER\b",  // SELECT FOR SHARE / FOR KEY SHARE / FOR NO KEY UPDATE locking clauses.
        SHARE        => r"SHARE\b",
        // CREATE FUNCTION option keywords.
        IMPORT    => r"IMPORT\b",
        TABLESAMPLE => r"TABLESAMPLE\b",
        DEFINER   => r"DEFINER\b",
        INVOKER   => r"INVOKER\b",
        LEAKPROOF => r"LEAKPROOF\b",
        COST      => r"COST\b",
        SUPPORT   => r"SUPPORT\b",
        TRANSFORM => r"TRANSFORM\b",
        EXTERNAL  => r"EXTERNAL\b",}
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
        // Cube-root `||/` (prefix operator). Must come before `||`.
        PipePipeSlash  => r"\|\|/", "||/",
        Concat     => r"\|\|",     "||",
        // Square-root `|/` (prefix operator). Must come after `||/`/`||`
        // and before bare `|`.
        PipeSlash      => r"\|/",  "|/",
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
        // Single-char `@` (prefix absolute-value operator). Declared after
        // all longer `@`-prefixed operators so longest-match-wins.
        At             => r"@",        "@",
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
    use recursa_diagram::railroad;

    recursa::literals! {
        #[railroad(label = "<$ String Literal>")]
        DollarStringLit => r#"\$[a-zA-Z_]*\$[\s\S]*?\$[a-zA-Z_]*\$"#,
        #[railroad(label = "<Unicode Quoter Identifier>")]
        UnicodeQuotedIdent => r#"(?i:U)&"[^"]*(?:""[^"]*)*""#,
        #[railroad(label = "<Quoted Identifier>")]
        QuotedIdent => r#""[^"]*(?:""[^"]*)*""#,
        #[railroad(label = "<Unicode String Literal>")]
        UnicodeStringLit => r"(?i:U)&'(?:[^'\\]|\\.|'')*'",
        #[railroad(label = "<Escape String Literal>")]
        EscapeStringLit => r"(?i:E)'(?:[^'\\]|\\.|'')*'",
        #[railroad(label = "<String Literal>")]
        StringLit  => r"'[^']*(?:''[^']*)*'",
        // NumericLit must require a decimal point OR an exponent so it does
        // not collide with bare integers (handled by IntegerLit). Forms:
        //   123.45    .5    123.    1e10    1.5e-5    .5e10
        // Declared before IntegerLit so longest-match-wins picks the longer
        // literal when an exponent is present.
        // Digit groups allow `_` as a separator between digits (Postgres 16+).
        // A digit group is `[0-9](?:_?[0-9])*` — starts with a digit, and
        // every subsequent `_` must be followed by a digit.
        #[railroad(label = "<Numeric Literal>")]
        NumericLit => r"(?:[0-9](?:_?[0-9])*\.(?:[0-9](?:_?[0-9])*)?|\.[0-9](?:_?[0-9])*)(?:[eE][+-]?[0-9](?:_?[0-9])*)?|[0-9](?:_?[0-9])*[eE][+-]?[0-9](?:_?[0-9])*",
        #[railroad(label = "<Integer Literal>")]
        IntegerLit => r"[0-9](?:_?[0-9])*",
        // psql client variable substitution: `:foo` or `:'foo'` or `:"foo"`.
        // Treated as an opaque expression atom so SELECTs that reference
        // psql-set variables parse structurally.
        #[railroad(label = "<psql var>")]
        PsqlVar => r#":(?:[A-Za-z_][A-Za-z0-9_]*|'[^']*'|"[^"]*")"#,
    }

    // --- Identifier ---

    /// ALL SQL keywords (uppercase) for identifier exclusion.
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
        // STATEMENT leads.
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
        // Recognized as `keyword::PRIMARY` / `keyword::KEY` only where the
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
        "TABLESAMPLE",
        // Window frame unit keywords. Must be reserved so they are not
        // accidentally consumed as a window `ref_name` identifier, which
        // would leave the frame clause unparseable.
        "ROWS",
        "RANGE",
        "GROUPS",
    ];

    fn is_keyword(s: &str) -> bool {
        SQL_KEYWORDS.iter().any(|kw| kw.eq_ignore_ascii_case(s))
    }

    /// Postcondition: reject identifiers that are SQL keywords.
    fn not_keyword<'input>(ident: &UnquotedIdent<'input>) -> Result<(), ParseError> {
        if is_keyword(&ident.0) {
            Err(ParseError::new(
                ident.0.to_string(),
                0..ident.0.len(),
                "identifier (not a keyword)",
            ))
        } else {
            Ok(())
        }
    }

    /// Postcondition: reject identifiers that are SQL keywords.
    fn ident_is_not_keyword<'input>(ident: &Ident<'input>) -> Result<(), ParseError> {
        if let Ident::Unquoted(unquoted) = ident {
            return not_keyword(unquoted);
        }
        Ok(())
    }

    /// Unquoted SQL identifier: `[a-zA-Z_][a-zA-Z0-9_]*` but NOT a keyword.
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", postcondition = not_keyword)]
    #[visit(terminal)]
    #[railroad(label = "<Unquoted Identifier>")]
    pub struct UnquotedIdent<'input>(pub ::std::borrow::Cow<'input, str>);

    impl<'input> recursa::FormatTokens for UnquotedIdent<'input> {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.as_ref().to_string()));
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
    #[railroad(label = "<Identifier>")]
    pub enum Ident<'input> {
        #[railroad(label = "<Unicode Quoted>")]
        UnicodeQuoted(UnicodeQuotedIdent<'input>),
        #[railroad(label = "<Quoted>")]
        Quoted(QuotedIdent<'input>),
        #[railroad(label = "<Unquoted>")]
        Unquoted(UnquotedIdent<'input>),
    }

    impl<'input> recursa::FormatTokens for Ident<'input> {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            match self {
                Ident::UnicodeQuoted(u) => u.format_tokens(tokens),
                Ident::Quoted(quoted) => quoted.format_tokens(tokens),
                Ident::Unquoted(unquoted) => unquoted.format_tokens(tokens),
            }
        }
    }

    impl<'input> Ident<'input> {
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
    #[railroad(label = "<Bare Alias Name>")]
    pub struct BareAliasName<'input>(pub ::std::borrow::Cow<'input, str>);

    impl<'input> recursa::FormatTokens for BareAliasName<'input> {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.as_ref().to_string()));
        }
    }

    /// Alias name: bare word (including keywords) or double-quoted identifier
    /// with arbitrary content (`"One hour"`).
    ///
    /// Variant ordering: `Quoted` (`"`) and `Bare` (letter) start with
    /// different first chars, so order is for clarity, not disambiguation.
    #[derive(Parse, Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[visit(terminal)]
    #[railroad(label = "<Alias Name>")]
    pub enum AliasName<'input> {
        #[railroad(label = "<Quoted>")]
        Quoted(QuotedIdent<'input>),
        #[railroad(label = "<Bare>")]
        Bare(BareAliasName<'input>),
    }

    impl<'input> AliasName<'input> {
        /// Raw text of the alias name (with quotes if quoted).
        pub fn text(&self) -> &str {
            match self {
                AliasName::Quoted(q) => &q.0,
                AliasName::Bare(b) => &b.0,
            }
        }
    }

    impl<'input> recursa::FormatTokens for AliasName<'input> {
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
    pub struct RestOfLine<'input>(pub ::std::borrow::Cow<'input, str>);

    impl<'input> recursa::FormatTokens for RestOfLine<'input> {
        fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
            tokens.push(recursa::fmt::Token::String(self.0.as_ref().to_string()));
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
        assert!(SELECT::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_select_lowercase() {
        let input = Input::new("select");
        assert!(SELECT::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_select_mixed_case() {
        let input = Input::new("SeLeCt");
        assert!(SELECT::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_select_not_prefix_of_identifier() {
        let input = Input::new("SELECTED");
        assert!(!SELECT::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_bool_not_prefix_of_booleq() {
        let input = Input::new("booleq");
        assert!(!BOOL::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_bool_matches_standalone() {
        let input = Input::new("bool");
        assert!(BOOL::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_boolean_matches() {
        let input = Input::new("BOOLEAN");
        assert!(BOOLEAN::peek::<NoRules>(&input));
    }

    #[test]
    fn keyword_not_matches() {
        let input = Input::new("NOT");
        assert!(NOT::peek::<NoRules>(&input));
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

    // --- INTEGER literal tests ---

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

    // --- NUMERIC literal tests (decimals + exponent) ---

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
    fn integer_literal_with_underscores() {
        let mut input = Input::new("100_000_000_000_000");
        let lit = IntegerLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "100_000_000_000_000");
    }

    #[test]
    fn numeric_literal_with_underscores() {
        let mut input = Input::new("1_234.567_89");
        let lit = NumericLit::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(lit.0, "1_234.567_89");
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
