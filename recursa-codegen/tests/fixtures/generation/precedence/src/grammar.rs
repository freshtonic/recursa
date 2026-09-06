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
//! Recursive descent ignores the declaration and settles each choice by reading on to the
//! token that decides it, which agrees with the declaration on all but six of the driver's
//! inputs; on those six the integration test pins what each parser makes of the input, and
//! the declared outcome is PostgreSQL's. The test compares the two parsers of one generation
//! and the recursive-descent generation with and without the declaration.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 5,
    parsers(rd = recursive_descent, lr = table_driven),
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

#[derive(recursa::Node, Debug, Clone)]
#[tok(this, SEMI)]
pub struct Stmt<'input> {
    pub body: Body<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Body<'input> {
    Set(SetStmt<'input>),
    Select(SelectStmt<'input>),
    Window(WindowStmt<'input>),
    Json(JsonStmt<'input>),
}

/// `SET SESSION CHARACTERISTICS` or `SET [LOCAL | SESSION] name = expr`.
#[derive(recursa::Node, Debug, Clone)]
pub enum SetStmt<'input> {
    #[tok(SET, SESSION, CHARACTERISTICS)]
    Characteristics,
    Assign(SetAssign<'input>),
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(SET, this)]
pub struct SetAssign<'input> {
    pub scope: Option<SetScope>,
    pub name: Name<'input>,
    #[tok(EQ)]
    pub value: Expr<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum SetScope {
    #[tok(LOCAL)]
    Local,
    #[tok(SESSION)]
    Session,
}

/// `SELECT expr [label]`: a target with an optional bare label.
#[derive(recursa::Node, Debug, Clone)]
#[tok(SELECT, this)]
pub struct SelectStmt<'input> {
    pub target: Expr<'input>,
    pub label: Option<BareLabel<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[pratt]
pub enum Expr<'input> {
    Column(Name<'input>),
    Number(Number<'input>),
    Paren(#[tok(LPAREN, this, RPAREN)] Box<Self>),
    /// `c_expr: select_with_parens %prec UMINUS`.
    #[parse(prec = UMINUS)]
    Sub(SubSelect<'input>),
    /// `c_expr: select_with_parens indirection`.
    SubField(SubSelect<'input>, #[tok(DOT, this)] Name<'input>),
    #[parse(prefix, bp = 12)]
    Negate(#[tok(MINUS, this)] Box<Self>),
    #[parse(infix, lbp = 10, rbp = 11)]
    Add(Box<Self>, #[tok(PLUS, this)] Box<Self>),
    #[parse(infix, lbp = 10, rbp = 11)]
    Subtract(Box<Self>, #[tok(MINUS, this)] Box<Self>),
    #[parse(infix, lbp = 5, rbp = 6)]
    Like(
        Box<Self>,
        #[tok(LIKE, this)] Box<Self>,
        Option<EscapeClause<'input>>,
    ),
    #[parse(postfix, bp = 4)]
    IsJson(Box<Self>, #[tok(IS, this)] IsJsonTail),
    #[parse(postfix, bp = 20)]
    Field(Box<Self>, #[tok(DOT, this)] Name<'input>),
}

/// `ESCAPE 'c'` with `%nonassoc ESCAPE` one level above `LIKE`: the escape is a string
/// literal, as PostgreSQL's is in practice, so the clause ends where the literal ends and the
/// only decision is whether `ESCAPE` opens the clause or names a bare label. The declared
/// precedence of `ESCAPE` (6) above `LIKE` (5) makes the table-driven parser shift it;
/// recursive descent decides by the token after `ESCAPE`, so it reads a bare `escape` before
/// `;` as a label, which PostgreSQL and the table-driven parser reject.
#[derive(recursa::Node, Debug, Clone)]
#[tok(ESCAPE, this)]
pub struct EscapeClause<'input> {
    pub escape: StringLit<'input>,
}

/// `json_predicate_type_constraint: JSON %prec UNBOUNDED | JSON VALUE_P`.
#[derive(recursa::Node, Debug, Clone)]
pub enum IsJsonTail {
    #[greedy(VALUE)]
    #[tok(JSON, VALUE)]
    JsonValue,
    #[parse(prec = UNBOUNDED)]
    #[tok(JSON)]
    Json,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(LPAREN, SELECT, this, RPAREN)]
pub struct SubSelect<'input> {
    pub column: Name<'input>,
}

/// `ROWS frame_extent`.
#[derive(recursa::Node, Debug, Clone)]
#[tok(ROWS, this)]
pub struct WindowStmt<'input> {
    pub extent: FrameExtent<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum FrameExtent<'input> {
    Between {
        #[tok(BETWEEN)]
        low: FrameBound<'input>,
        #[tok(AND)]
        high: FrameBound<'input>,
    },
    Single(FrameBound<'input>),
}

/// `UNBOUNDED PRECEDING` is an offset whose expression is the column `unbounded`, as in
/// pg-sql; the keyword form of gram.y is what its `%nonassoc UNBOUNDED` settles.
#[derive(recursa::Node, Debug, Clone)]
pub enum FrameBound<'input> {
    #[tok(CURRENT, ROW)]
    CurrentRow,
    Offset(FrameOffset<'input>),
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(this, PRECEDING)]
pub struct FrameOffset<'input> {
    pub offset: Expr<'input>,
}

/// `COLUMNS ( json_table_column_definition )`.
#[derive(recursa::Node, Debug, Clone)]
#[tok(COLUMNS, LPAREN, this, RPAREN)]
pub struct JsonStmt<'input> {
    pub column: JsonColumn<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum JsonColumn<'input> {
    Nested(NestedColumn<'input>),
    Regular(RegularColumn<'input>),
}

/// `NESTED [PATH] 'path'`.
#[derive(recursa::Node, Debug, Clone)]
#[tok(NESTED, optional(PATH), this)]
pub struct NestedColumn<'input> {
    pub path: StringLit<'input>,
}

/// `name type`: a column whose name and type are both names.
#[derive(recursa::Node, Debug, Clone)]
pub struct RegularColumn<'input> {
    pub name: Name<'input>,
    pub ty: Name<'input>,
}

/// An identifier or an unreserved keyword.
#[derive(recursa::Node, Debug, Clone)]
pub struct Name<'input> {
    #[lex(pattern = r"[a-z_][a-z0-9_]*", admits(ColumnName))]
    pub text: NameText<'input>,
}

/// A bare label: an identifier or a bare-label keyword.
#[derive(recursa::Node, Debug, Clone)]
pub struct BareLabel<'input> {
    #[lex(pattern = r"[a-z_][a-z0-9_]*", admits(Label))]
    pub text: LabelText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Number<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: NumberText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct StringLit<'input> {
    #[lex(pattern = r"'[^']*'")]
    pub text: StringText<'input>,
}
