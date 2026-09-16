//! The precedence grammar: the six gram.y conflict families of recursa#120 in one small
//! language, each settled the way PostgreSQL settles it, by the `precedence` declaration.
//!
//! - `a like b . escape`: the escape clause of `LIKE` against `escape` as a bare label
//!   (`%nonassoc ESCAPE` just above `LIKE`).
//! - `a is json . value`: the type of an `IS JSON` test against `value` as a bare label
//!   (`VALUE` at `IDENT`'s level, the bare `JSON` alternative `%prec UNBOUNDED`).
//! - `unbounded . preceding` and `nested . path`: an unreserved keyword that names a column
//!   against the syntax it opens (`%nonassoc UNBOUNDED NESTED` below `IDENT`).
//! - `rows between . - 1 preceding`: `between` as a column name against the frame extent it
//!   opens, decided by an operator token (`BETWEEN` below `'+' '-'`).
//! - `( select x ) . y`: the parenthesised sub-select alternative, which has no terminal of
//!   its own, against its indirection (`%prec UMINUS` below `'.'`).
//! - `set session . characteristics`: the literal scope keyword against the transaction form
//!   (two keywords outside the operator hierarchy).
//!
//! The integration test pins the LR result for each conflict family. The declared outcome
//! matches PostgreSQL's grammar.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    categories { UNRESERVED, RESERVED }
    flags { bare_label }
    keywords {
        SET => "set" in RESERVED,
        LOCAL => "local" in UNRESERVED,
        SESSION => "session" in UNRESERVED,
        CHARACTERISTICS => "characteristics" in UNRESERVED,
        SELECT => "select" in RESERVED,
        LIKE => "like" in RESERVED,
        ESCAPE => "escape" in UNRESERVED + bare_label,
        IS => "is" in RESERVED,
        JSON => "json" in UNRESERVED,
        VALUE => "value" in UNRESERVED + bare_label,
        ROWS => "rows" in UNRESERVED,
        BETWEEN => "between" in UNRESERVED,
        AND => "and" in RESERVED,
        UNBOUNDED => "unbounded" in UNRESERVED,
        PRECEDING => "preceding" in RESERVED,
        CURRENT => "current" in RESERVED,
        ROW => "row" in RESERVED,
        NESTED => "nested" in UNRESERVED,
        PATH => "path" in UNRESERVED,
        COLUMNS => "columns" in RESERVED,
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        DOT => ".",
        PLUS => "+",
        MINUS => "-",
        EQ => "=",
        SEMI => ";",
    }
    admissions {
        ColumnName = UNRESERVED,
        Label = bare_label,
    }
    precedence {
        nonassoc(bp = 1) { LOCAL, SESSION },
        nonassoc(bp = 2) { CHARACTERISTICS },
        nonassoc(bp = 4) { IS },
        nonassoc(bp = 5) { BETWEEN, LIKE },
        nonassoc(bp = 6) { ESCAPE },
        nonassoc(bp = 8) { UNBOUNDED, NESTED },
        nonassoc(bp = 9) { NameText, PRECEDING, VALUE, PATH },
        left(bp = 10) { PLUS, MINUS },
        right(bp = 12) { UMINUS },
        left(bp = 20) { DOT },
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(this, SEMI)]
    pub struct Stmt {
        pub body: Body,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Body {
        Set(SetStmt),
        Select(SelectStmt),
        Window(WindowStmt),
        Json(JsonStmt),
    }
}

recursa::ast_node! {
    /// `SET SESSION CHARACTERISTICS` or `SET [LOCAL | SESSION] name = expr`.
    #[derive(Debug, Clone)]
    pub enum SetStmt {
        #[tok(SET, SESSION, CHARACTERISTICS)]
        Characteristics,
        Assign(SetAssign),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(SET, this)]
    pub struct SetAssign {
        pub scope: Option<SetScope>,
        pub name: Name,
        #[tok(EQ)]
        pub value: Expr,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum SetScope {
        #[tok(LOCAL)]
        Local,
        #[tok(SESSION)]
        Session,
    }
}

recursa::ast_node! {
    /// `SELECT expr [label]`: a target with an optional bare label.
    #[derive(Debug, Clone)]
    #[tok(SELECT, this)]
    pub struct SelectStmt {
        pub target: Expr,
        pub label: Option<BareLabel>,
    }
}

recursa::ast_node! {
    /// An ordinary left-recursive enum: every operator takes its level from the `precedence`
    /// declaration above, through the rule's last declared terminal or through an override.
    #[derive(Debug, Clone)]
    pub enum Expr {
        Column(Name),
        Number(Number),
        Paren(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),
        /// `c_expr: select_with_parens %prec UMINUS`: the rule has no terminal of its own.
        #[parse(prec = UMINUS)]
        Sub(SubSelect),
        /// `c_expr: select_with_parens indirection`.
        SubField(SubSelect, #[tok(DOT, this)] Name),
        /// `a_expr: '-' a_expr %prec UMINUS`: the override lifts the prefix form above the
        /// level `MINUS` gives the rule as a binary operator.
        #[parse(prec = UMINUS)]
        Negate(#[tok(MINUS, this)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
        Subtract(boxed!(Self), #[tok(MINUS, this)] boxed!(Self)),
        Like(
            boxed!(Self),
            #[tok(LIKE, this)] boxed!(Self),
            Option<EscapeClause>,
        ),
        IsJson(boxed!(Self), #[tok(IS, this)] IsJsonTail),
        Field(boxed!(Self), #[tok(DOT, this)] Name),
    }
}

recursa::ast_node! {
    /// `ESCAPE 'c'` with `%nonassoc ESCAPE` one level above `LIKE`: the escape is a string
    /// literal, as PostgreSQL's is in practice, so the clause ends where the literal ends and the
    /// only decision is whether `ESCAPE` opens the clause or names a bare label. The declared
    /// precedence of `ESCAPE` (6) above `LIKE` (5) makes the table-driven parser shift it;
    /// the retired predictive parser decided by the token after `ESCAPE`, so it read a bare
    /// `escape` before `;` as a label, which PostgreSQL and the LR parser reject.
    #[derive(Debug, Clone)]
    #[tok(ESCAPE, this)]
    pub struct EscapeClause {
        pub escape: StringLit,
    }
}

recursa::ast_node! {
    /// `json_predicate_type_constraint: JSON %prec UNBOUNDED | JSON VALUE_P`.
    #[derive(Debug, Clone)]
    pub enum IsJsonTail {
        #[tok(JSON, VALUE)]
        JsonValue,
        #[parse(prec = UNBOUNDED)]
        #[tok(JSON)]
        Json,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(LPAREN, SELECT, this, RPAREN)]
    pub struct SubSelect {
        pub column: Name,
    }
}

recursa::ast_node! {
    /// `ROWS frame_extent`.
    #[derive(Debug, Clone)]
    #[tok(ROWS, this)]
    pub struct WindowStmt {
        pub extent: FrameExtent,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum FrameExtent {
        Between {
            #[tok(BETWEEN)]
            low: FrameBound,
            #[tok(AND)]
            high: FrameBound,
        },
        Single(FrameBound),
    }
}

recursa::ast_node! {
    /// `UNBOUNDED PRECEDING` is an offset whose expression is the column `unbounded`, as in
    /// pg-sql; the keyword form of gram.y is what its `%nonassoc UNBOUNDED` settles.
    #[derive(Debug, Clone)]
    pub enum FrameBound {
        #[tok(CURRENT, ROW)]
        CurrentRow,
        Offset(FrameOffset),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(this, PRECEDING)]
    pub struct FrameOffset {
        pub offset: Expr,
    }
}

recursa::ast_node! {
    /// `COLUMNS ( json_table_column_definition )`.
    #[derive(Debug, Clone)]
    #[tok(COLUMNS, LPAREN, this, RPAREN)]
    pub struct JsonStmt {
        pub column: JsonColumn,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum JsonColumn {
        Nested(NestedColumn),
        Regular(RegularColumn),
    }
}

recursa::ast_node! {
    /// `NESTED [PATH] 'path'`.
    #[derive(Debug, Clone)]
    #[tok(NESTED, optional(PATH), this)]
    pub struct NestedColumn {
        pub path: StringLit,
    }
}

recursa::ast_node! {
    /// `name type`: a column whose name and type are both names.
    #[derive(Debug, Clone)]
    pub struct RegularColumn {
        pub name: Name,
        pub ty: Name,
    }
}

recursa::ast_node! {
    /// An identifier or an unreserved keyword.
    #[derive(Debug, Clone)]
    pub struct Name {
        #[lex(pattern = r"[a-z_][a-z0-9_]*", admits(ColumnName))]
        pub text: NameText,
    }
}

recursa::ast_node! {
    /// A bare label: an identifier or a bare-label keyword.
    #[derive(Debug, Clone)]
    pub struct BareLabel {
        #[lex(pattern = r"[a-z_][a-z0-9_]*", admits(Label))]
        pub text: LabelText,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct StringLit {
        #[lex(pattern = r"'[^']*'")]
        pub text: StringText,
    }
}
