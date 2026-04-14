/// SQL expression AST with derived Pratt parsing for operator precedence.
///
/// Handles atoms, prefix (NOT, unary minus), infix (AND, OR, comparisons,
/// arithmetic), and postfix operators (::type cast, IS [NOT] TRUE/FALSE/UNKNOWN/NULL,
/// IN (list)).
use std::marker::PhantomData;

use recursa::seq::{NonEmpty, OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// One or more adjacent string literals, concatenated by Postgres into a
/// single value: `'first' ' - next' 'third'`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StringLitSeq {
    pub parts: Seq<literal::StringLit, (), OptionalTrailing, NonEmpty>,
}

/// Content inside IN parentheses: either a subquery or expression list.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InContent {
    Subquery(Box<crate::ast::values::CompoundQuery>),
    Exprs(Seq<Expr, punct::Comma>),
}

/// `IN (expr, ...)` or `IN (subquery)` postfix suffix.
pub type InList = Surrounded<punct::LParen, InContent, punct::RParen>;

/// Parenthesized precision/scale for type names: `(10,2)` or `(3)`.
pub type TypePrecision =
    Surrounded<punct::LParen, Seq<literal::IntegerLit, punct::Comma>, punct::RParen>;

/// Array type suffix: `[]`
#[derive(Debug, Clone, FormatTokens, PartialEq, Eq, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ArrayTypeSuffix(pub punct::LBracket, pub punct::RBracket);

/// Type name for casts.
#[derive(Debug, Clone, FormatTokens, PartialEq, Eq, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TypeName {
    Bool(keyword::Bool),
    Boolean(keyword::Boolean),
    Text(keyword::Text),
    Integer(keyword::Integer),
    Int(keyword::Int),
    Serial(keyword::Serial),
    Numeric(keyword::Numeric),
    Varchar(keyword::Varchar),
    Ident(literal::Ident),
}

// --- Boolean test suffix structs ---
// NOT variants listed before non-NOT variants so the longer pattern wins via
// longest-match lookahead (e.g., "NOT TRUE" matches before "TRUE").

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotTrue(pub keyword::Not, pub keyword::True);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotFalse(pub keyword::Not, pub keyword::False);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotUnknown(pub keyword::Not, pub keyword::Unknown);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotNull(pub keyword::Not, pub keyword::Null);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsTrue(pub keyword::True);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsFalse(pub keyword::False);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsUnknown(pub keyword::Unknown);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNull(pub keyword::Null);

/// Boolean test suffix: the part after `IS` in `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`.
///
/// NOT variants are listed first so the combined peek regex disambiguates
/// via longest match (e.g., `NOT TRUE` is longer than `TRUE`).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum BoolTestKind {
    IsNotTrue(IsNotTrue),
    IsNotFalse(IsNotFalse),
    IsNotUnknown(IsNotUnknown),
    IsNotNull(IsNotNull),
    IsTrue(IsTrue),
    IsFalse(IsFalse),
    IsUnknown(IsUnknown),
    IsNull(IsNull),
}

// --- Atom wrapper structs ---

/// Qualified column reference: `table.column`
///
/// Uses AliasName for the table part to allow keywords like EXCLUDED, NEW, OLD.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct QualifiedRef {
    pub table: literal::AliasName,
    pub dot: punct::Dot,
    pub column: literal::AliasName,
}

/// Qualified wildcard: `table.*`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct QualifiedWildcard {
    pub table: literal::AliasName,
    pub dot: punct::Dot,
    pub star: punct::Star,
}

/// Optional DISTINCT keyword in function calls: `count(DISTINCT x)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct DistinctKw(pub keyword::Distinct);

/// Window specification: `OVER window_name` or `OVER (inline_spec)`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowSpec {
    pub _over: keyword::Over,
    pub body: WindowSpecBody,
}

/// Body of an OVER clause.
///
/// Variant ordering: Inline (starts with `(`) before Named (starts with an
/// identifier). They start with different tokens so peek disambiguation is
/// trivial.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum WindowSpecBody {
    Inline(Surrounded<punct::LParen, InlineWindowSpec, punct::RParen>),
    Named(literal::Ident),
}

/// Interior of an inline window spec (between the parens).
///
/// The optional `ref_name` is an existing-window reference (e.g.
/// `WINDOW w2 AS (w1 ORDER BY x)`). It relies on `Option<literal::Ident>`
/// peek-disambiguating cleanly against `PARTITION`/`ORDER`/`ROWS`/etc.
/// because keywords are rejected by `literal::Ident`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct InlineWindowSpec {
    pub ref_name: Option<literal::Ident>,
    pub partition_by: Option<WindowPartitionBy>,
    pub order_by: Option<crate::ast::select::OrderByClause>,
    pub frame: Option<WindowFrameClause>,
}

/// PARTITION BY in window: `PARTITION BY expr, ...`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowPartitionBy {
    pub _partition: keyword::Partition,
    pub _by: keyword::By,
    pub exprs: Seq<Expr, punct::Comma>,
}

/// Frame unit: `ROWS | RANGE | GROUPS`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum WindowFrameUnit {
    Rows(keyword::Rows),
    Range(keyword::RangeKw),
    Groups(keyword::Groups),
}

/// `WINDOW` frame clause: `unit BETWEEN start AND end [EXCLUDE ...]`
/// or `unit start`.
///
/// Variant ordering: `Between` (starts with `unit BETWEEN`) before `Single`
/// (starts with `unit <bound>`). Longest-match-wins.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum WindowFrameClause {
    Between(WindowFrameBetween),
    Single(WindowFrameSingle),
}

/// `unit BETWEEN start AND end [EXCLUDE ...]`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowFrameBetween {
    pub unit: WindowFrameUnit,
    pub _between: keyword::Between,
    pub start: WindowFrameBound,
    pub _and: keyword::And,
    pub end: WindowFrameBound,
    pub exclude: Option<WindowFrameExclude>,
}

/// `unit start [EXCLUDE ...]`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowFrameSingle {
    pub unit: WindowFrameUnit,
    pub bound: WindowFrameBound,
    pub exclude: Option<WindowFrameExclude>,
}

/// A single frame bound.
///
/// Variant ordering: two-token forms first (`UNBOUNDED PRECEDING`,
/// `CURRENT ROW`, `UNBOUNDED FOLLOWING`), then the expr-prefixed forms
/// (`expr PRECEDING` / `expr FOLLOWING`). The expr forms start with an
/// expression and can't be confused with keyword-prefixed forms.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum WindowFrameBound {
    UnboundedPreceding(UnboundedPreceding),
    UnboundedFollowing(UnboundedFollowing),
    CurrentRow(CurrentRow),
    ExprPreceding(ExprPreceding),
    ExprFollowing(ExprFollowing),
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct UnboundedPreceding {
    pub _unbounded: keyword::Unbounded,
    pub _preceding: keyword::Preceding,
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct UnboundedFollowing {
    pub _unbounded: keyword::Unbounded,
    pub _following: keyword::Following,
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct CurrentRow {
    pub _current: keyword::CurrentKw,
    pub _row: keyword::Row,
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ExprPreceding {
    pub expr: Box<Expr>,
    pub _preceding: keyword::Preceding,
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ExprFollowing {
    pub expr: Box<Expr>,
    pub _following: keyword::Following,
}

/// `EXCLUDE { CURRENT ROW | GROUP | TIES | NO OTHERS }` frame exclusion.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowFrameExclude {
    pub _exclude: keyword::Excludew,
    pub target: WindowFrameExcludeTarget,
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum WindowFrameExcludeTarget {
    CurrentRow(CurrentRow),
    Group(keyword::Group),
    Ties(keyword::Ties),
    NoOthers(NoOthers),
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct NoOthers {
    pub _no: keyword::No,
    pub _others: keyword::Others,
}

/// Function call: `name(arg1, arg2, ...)`
///
/// Keeps explicit `lparen` field rather than using `Surrounded` because the
/// derive macro chains `IS_TERMINAL` fields for `first_pattern` — the
/// `Ident + LParen` pattern is what disambiguates `FuncCall` from a plain
/// `Ident` in `TableRef` enum lookahead.
///
/// Function argument: optionally prefixed with `VARIADIC`.
///
/// Variant ordering: `Variadic` before `Plain` since `VARIADIC` keyword is
/// longer than starting an expression.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncArg {
    Variadic(VariadicArg),
    Plain(Box<Expr>),
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VariadicArg {
    pub _variadic: keyword::Variadic,
    pub value: Box<Expr>,
}

/// Function call: `name([*] [DISTINCT] args [ORDER BY ...]) [OVER (...)]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncCall {
    pub name: literal::AliasName,
    pub lparen: punct::LParen,
    pub star_arg: Option<punct::Star>,
    pub distinct: Option<DistinctKw>,
    pub args: Seq<FuncArg, punct::Comma>,
    pub order_by: Option<Box<crate::ast::select::OrderByClause>>,
    pub rparen: punct::RParen,
    pub window: Option<WindowSpec>,
}

/// Content inside parentheses: either a subquery or a comma-separated expression list.
/// Subquery (CompoundQuery) must come first so SELECT/VALUES/WITH keywords are matched
/// before trying to parse as a regular expression.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ParenContent {
    Subquery(Box<crate::ast::values::CompoundQuery>),
    Exprs(Seq<Expr, punct::Comma>),
}

/// Parenthesized expression: `(expr)`, `(expr, expr, ...)`, or `(SELECT/VALUES ...)`
pub type ParenExpr = Surrounded<punct::LParen, ParenContent, punct::RParen>;

/// EXISTS subquery: `EXISTS (SELECT ...)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ExistsExpr {
    pub _exists: keyword::Exists,
    pub subquery: Surrounded<punct::LParen, Box<crate::ast::values::CompoundQuery>, punct::RParen>,
}

/// ARRAY bracket constructor: `ARRAY[expr, ...]`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ArrayBracket {
    pub _array: PhantomData<keyword::Array>,
    pub lbracket: punct::LBracket,
    pub elements: Seq<Expr, punct::Comma>,
    pub rbracket: punct::RBracket,
}

/// ARRAY subquery constructor: `ARRAY(subquery)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ArraySubquery {
    pub _array: PhantomData<keyword::Array>,
    pub subquery: Surrounded<punct::LParen, Box<crate::ast::values::CompoundQuery>, punct::RParen>,
}

/// ARRAY constructor: `ARRAY[expr, ...]` or `ARRAY(subquery)`
///
/// Variant ordering: Bracket (`ARRAY[`) has a longer first_pattern than
/// Subquery (`ARRAY(`) because `[` is a different token than `(`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum ArrayExpr {
    Bracket(ArrayBracket),
    Subquery(ArraySubquery),
}

/// ROW constructor: `ROW(expr, ...)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct RowExpr {
    pub _row: keyword::Row,
    pub values: Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>,
}

/// Cast type with optional precision and zero-or-more array suffixes:
/// `numeric(10,0)`, `integer[]`, `int4[][][]`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct CastType {
    pub base: TypeName,
    pub precision: Option<TypePrecision>,
    pub array_suffixes: Vec<ArrayTypeSuffix>,
}

/// NOT IN list: `expr NOT IN (val, ...)` suffix.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct NotInSuffix {
    pub _not: keyword::Not,
    pub _in: keyword::In,
    pub list: InList,
}

/// Function-style type cast: `bool 'value'`, `text 'hello'`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct TypeCastFunc {
    pub type_name: TypeName,
    pub value: literal::StringLit,
}

/// `WITH TIME ZONE` or `WITHOUT TIME ZONE` suffix for `TIMESTAMP`/`TIME`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum TimeZoneQualifier {
    With(WithTimeZone),
    Without(WithoutTimeZone),
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WithTimeZone {
    pub _with: keyword::With,
    pub _time: keyword::Time,
    pub _zone: keyword::Zone,
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WithoutTimeZone {
    pub _without: keyword::Without,
    pub _time: keyword::Time,
    pub _zone: keyword::Zone,
}

/// `TIMESTAMP [WITH|WITHOUT TIME ZONE] 'string'`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct TimestampLit {
    pub _timestamp: keyword::Timestamp,
    pub tz: Option<TimeZoneQualifier>,
    pub value: literal::StringLit,
}

/// `TIME [WITH|WITHOUT TIME ZONE] 'string'`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct TimeLit {
    pub _time: keyword::Time,
    pub tz: Option<TimeZoneQualifier>,
    pub value: literal::StringLit,
}

// --- Pratt expression enum ---

/// SQL expression with Pratt-derived parsing.
#[derive(FormatTokens, Parse, Debug, Clone, Visit)]
#[parse(rules = SqlRules, pratt)]
pub enum Expr {
    // --- Prefix ---
    #[parse(prefix, bp = 15)]
    Not(keyword::Not, Box<Expr>),
    #[parse(prefix, bp = 12)]
    Neg(punct::Minus, Box<Expr>),

    // --- Postfix ---
    /// Postgres-style cast: `expr::type`
    #[parse(postfix, bp = 20)]
    Cast(Box<Expr>, punct::ColonColon, CastType),
    /// Array subscript: `expr[idx]`
    #[parse(postfix, bp = 20)]
    Subscript(Box<Expr>, punct::LBracket, Box<Expr>, punct::RBracket),
    /// `expr COLLATE "collation"` — collation specifier. Binds tighter than
    /// comparisons (bp 5) but looser than `::` cast (bp 20).
    #[parse(postfix, bp = 18)]
    Collate(Box<Expr>, keyword::Collate, literal::Ident),
    /// Boolean test: `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`
    #[parse(postfix, bp = 8)]
    BoolTest(Box<Expr>, keyword::Is, BoolTestKind),
    /// NOT IN list: `expr NOT IN (val, ...)`
    #[parse(postfix, bp = 6)]
    NotInExpr(Box<Expr>, NotInSuffix),
    /// `expr NOT ILIKE pattern`. Declared before `NotLike` so the longer
    /// `NOT ILIKE` is tried first (matters only if any rule shares a prefix;
    /// here `NOT ILIKE` vs `NOT LIKE` differ on the second token).
    #[parse(postfix, bp = 5, inner_bp = 6)]
    NotIlike(Box<Expr>, keyword::Not, keyword::Ilike, Box<Expr>),
    /// `expr NOT LIKE pattern`. Must come before the `Not` prefix atom so
    /// longest-match-wins prefers the postfix form.
    #[parse(postfix, bp = 5, inner_bp = 6)]
    NotLike(Box<Expr>, keyword::Not, keyword::Like, Box<Expr>),
    /// `expr ILIKE pattern`
    #[parse(infix, bp = 5)]
    Ilike(Box<Expr>, keyword::Ilike, Box<Expr>),
    /// `expr LIKE pattern`
    #[parse(infix, bp = 5)]
    Like(Box<Expr>, keyword::Like, Box<Expr>),
    /// `expr !~* pattern` — POSIX case-insensitive negated regex match.
    #[parse(infix, bp = 5)]
    RegexNotIMatch(Box<Expr>, punct::BangTildeStar, Box<Expr>),
    /// `expr ~* pattern` — POSIX case-insensitive regex match.
    #[parse(infix, bp = 5)]
    RegexIMatch(Box<Expr>, punct::TildeStar, Box<Expr>),
    /// `expr !~ pattern` — POSIX negated regex match.
    #[parse(infix, bp = 5)]
    RegexNotMatch(Box<Expr>, punct::BangTilde, Box<Expr>),
    /// `expr ~ pattern` — POSIX regex match.
    #[parse(infix, bp = 5)]
    RegexMatch(Box<Expr>, punct::Tilde, Box<Expr>),
    /// IN list: `expr IN (val, ...)`
    #[parse(postfix, bp = 6)]
    InExpr(Box<Expr>, keyword::In, InList),
    /// `expr NOT BETWEEN low AND high`. Declared before `BetweenExpr` so
    /// the longer `NOT BETWEEN` prefix wins disambiguation. `inner_bp = 3`
    /// keeps the low/high operands from swallowing the literal `AND` that
    /// separates them (the `AND` infix has `bp = 2`).
    #[parse(postfix, bp = 6, inner_bp = 3)]
    NotBetweenExpr(
        Box<Expr>,
        keyword::Not,
        keyword::Between,
        Box<Expr>,
        keyword::And,
        Box<Expr>,
    ),
    /// `expr BETWEEN low AND high`. See `NotBetweenExpr` for the
    /// `inner_bp` rationale.
    #[parse(postfix, bp = 6, inner_bp = 3)]
    BetweenExpr(
        Box<Expr>,
        keyword::Between,
        Box<Expr>,
        keyword::And,
        Box<Expr>,
    ),

    // --- Infix ---
    // Multi-char operators before single-char to avoid partial matching.
    //
    // JSON / JSONB operators are listed FIRST among infix so that their
    // longer tokens are peeked before conflicting shorter ones
    // (e.g. `<@` before `<`, `->` before `-`). All use bp = 10 — same tier
    // as Concat/Add/Sub (which is Postgres's convention for these ops).
    /// JSON path as text: `expr #>> path`
    #[parse(infix, bp = 10)]
    JsonPathText(Box<Expr>, punct::HashArrowArrow, Box<Expr>),
    /// JSON path: `expr #> path`
    #[parse(infix, bp = 10)]
    JsonPath(Box<Expr>, punct::HashArrow, Box<Expr>),
    /// JSON field as text: `expr ->> field`
    #[parse(infix, bp = 10)]
    JsonFieldText(Box<Expr>, punct::ArrowArrow, Box<Expr>),
    /// JSON field: `expr -> field`
    #[parse(infix, bp = 10)]
    JsonField(Box<Expr>, punct::Arrow, Box<Expr>),
    /// JSON any-key-exists: `expr ?| keys`
    #[parse(infix, bp = 10)]
    JsonAnyKey(Box<Expr>, punct::QuestionPipe, Box<Expr>),
    /// JSON all-keys-exist: `expr ?& keys`
    #[parse(infix, bp = 10)]
    JsonAllKeys(Box<Expr>, punct::QuestionAmp, Box<Expr>),
    /// Geometric intersect: `a ?# b`. Must precede `JsonKey` (`?`).
    #[parse(infix, bp = 5)]
    Intersect(Box<Expr>, punct::QuestionHash, Box<Expr>),
    /// Geometric horizontal: `a ?- b`. Must precede `JsonKey` (`?`).
    #[parse(infix, bp = 5)]
    Horizontal(Box<Expr>, punct::QuestionDash, Box<Expr>),
    /// JSON key-exists: `expr ? key`
    #[parse(infix, bp = 10)]
    JsonKey(Box<Expr>, punct::Question, Box<Expr>),
    /// JSONB contains: `expr @> expr`
    #[parse(infix, bp = 10)]
    JsonContains(Box<Expr>, punct::AtGt, Box<Expr>),
    /// JSONB contained-by: `expr <@ expr`
    #[parse(infix, bp = 10)]
    JsonContainedBy(Box<Expr>, punct::LtAt, Box<Expr>),

    // --- Postgres text-search / jsonpath / range / geometric 3-char operators ---
    //
    // These must come BEFORE any variant whose infix token is a 2-char prefix
    // (e.g. `<<|` before `<<`, `&<|` before `&<`, `?#` before JsonKey `?`).
    // The scanner is longest-match at the token level, but Pratt operator
    // dispatch chooses variants in declaration order — so a shorter-prefix
    // variant declared first would swallow the `&<` / `<<` / `?` and leave
    // the trailing `|` / `#` dangling.
    /// Text-search / jsonb path match: `expr @@@ expr`.
    #[parse(infix, bp = 5)]
    TsMatch3(Box<Expr>, punct::AtAtAt, Box<Expr>),
    /// Geometric strictly-below: `a <<| b`. Before `StrictlyLeft` (`<<`).
    #[parse(infix, bp = 5)]
    StrictlyBelow(Box<Expr>, punct::LtLtPipe, Box<Expr>),
    /// Inet is-subset-or-equal: `a <<= b`. Before `StrictlyLeft` (`<<`).
    #[parse(infix, bp = 5)]
    SubsetEq(Box<Expr>, punct::LtLtEq, Box<Expr>),
    /// Distance: `a <-> b`. Before any `<` variant.
    #[parse(infix, bp = 10)]
    Distance(Box<Expr>, punct::LtMinusGt, Box<Expr>),
    /// Inet is-superset-or-equal: `a >>= b`. Before `StrictlyRight` (`>>`).
    #[parse(infix, bp = 5)]
    SupersetEq(Box<Expr>, punct::GtGtEq, Box<Expr>),
    /// Range adjacent: `a -|- b`. Before `Sub` (`-`).
    #[parse(infix, bp = 5)]
    Adjacent(Box<Expr>, punct::MinusPipeMinus, Box<Expr>),
    /// Geometric strictly-above: `a |>> b`. Before `Concat` (`||`).
    #[parse(infix, bp = 5)]
    StrictlyAbove(Box<Expr>, punct::PipeGtGt, Box<Expr>),
    /// Geometric no-extend-below: `a |&> b`. Before `Concat` (`||`).
    #[parse(infix, bp = 5)]
    NoExtendBelow(Box<Expr>, punct::PipeAmpGt, Box<Expr>),
    /// Geometric no-extend-above: `a &<| b`. Before `NoExtendRight` (`&<`).
    #[parse(infix, bp = 5)]
    NoExtendAbove(Box<Expr>, punct::AmpLtPipe, Box<Expr>),

    // --- 2-char operators ---
    /// Text-search / jsonb path match: `expr @@ expr`.
    #[parse(infix, bp = 5)]
    TsMatch(Box<Expr>, punct::AtAt, Box<Expr>),
    /// Jsonpath exists: `expr @? path`.
    #[parse(infix, bp = 5)]
    JsonPathExists(Box<Expr>, punct::AtQuestion, Box<Expr>),
    /// Range / array overlap: `a && b`.
    #[parse(infix, bp = 10)]
    Overlap(Box<Expr>, punct::AmpAmp, Box<Expr>),
    /// Range does-not-extend-right: `a &< b`.
    #[parse(infix, bp = 5)]
    NoExtendRight(Box<Expr>, punct::AmpLt, Box<Expr>),
    /// Range does-not-extend-left: `a &> b`.
    #[parse(infix, bp = 5)]
    NoExtendLeft(Box<Expr>, punct::AmpGt, Box<Expr>),
    /// Range strictly-left-of: `a << b`.
    #[parse(infix, bp = 5)]
    StrictlyLeft(Box<Expr>, punct::LtLt, Box<Expr>),
    /// Range strictly-right-of: `a >> b`.
    #[parse(infix, bp = 5)]
    StrictlyRight(Box<Expr>, punct::GtGt, Box<Expr>),

    #[parse(infix, bp = 1)]
    Or(Box<Expr>, keyword::Or, Box<Expr>),
    #[parse(infix, bp = 2)]
    And(Box<Expr>, keyword::And, Box<Expr>),
    #[parse(infix, bp = 5)]
    BangEq(Box<Expr>, punct::BangEq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Neq(Box<Expr>, punct::Neq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lte(Box<Expr>, punct::Lte, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gte(Box<Expr>, punct::Gte, Box<Expr>),
    #[parse(infix, bp = 5)]
    Eq(Box<Expr>, punct::Eq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lt(Box<Expr>, punct::Lt, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gt(Box<Expr>, punct::Gt, Box<Expr>),
    /// String concatenation: `expr || expr`
    #[parse(infix, bp = 10)]
    Concat(Box<Expr>, punct::Concat, Box<Expr>),
    #[parse(infix, bp = 10)]
    Add(Box<Expr>, punct::Plus, Box<Expr>),
    #[parse(infix, bp = 10)]
    Sub(Box<Expr>, punct::Minus, Box<Expr>),
    /// Multiplication: `expr * expr`
    #[parse(infix, bp = 11)]
    Mul(Box<Expr>, punct::Star, Box<Expr>),
    /// Division: `expr / expr`
    #[parse(infix, bp = 11)]
    Div(Box<Expr>, punct::Slash, Box<Expr>),
    /// Modulo: `expr % expr`
    #[parse(infix, bp = 11)]
    Mod(Box<Expr>, punct::Percent, Box<Expr>),

    // --- Atoms ---
    /// EXISTS subquery: `EXISTS (SELECT ...)`
    #[parse(atom)]
    Exists(ExistsExpr),
    /// ARRAY constructor: `ARRAY[...]` or `ARRAY(...)`
    #[parse(atom)]
    Array(ArrayExpr),
    /// ROW constructor: `ROW(...)`
    #[parse(atom)]
    RowExpr(RowExpr),
    /// Escape string literal: `E'foo\n'`. Must come before `CastFunc` and
    /// `StringLit` — `CastFunc` is `TypeName StringLit` and would match `e`
    /// as a type name followed by the string literal.
    #[parse(atom)]
    EscapeStringLit(literal::EscapeStringLit),
    /// `TIMESTAMP [WITH|WITHOUT TIME ZONE] 'string'`. Must come before `CastFunc`
    /// since `timestamp` is also an identifier.
    #[parse(atom)]
    TimestampLit(TimestampLit),
    /// `TIME [WITH|WITHOUT TIME ZONE] 'string'`. Must come before `CastFunc`.
    #[parse(atom)]
    TimeLit(TimeLit),
    /// Function-style type cast: `bool 't'` -- must come before ColumnRef
    /// since type keywords like `bool` overlap with identifiers
    #[parse(atom)]
    CastFunc(TypeCastFunc),
    /// Function call: `func(args)` -- must come before ColumnRef
    #[parse(atom)]
    Func(FuncCall),
    /// Qualified wildcard: `table.*` -- must come before QualRef and ColumnRef
    #[parse(atom)]
    QualWild(QualifiedWildcard),
    /// Qualified column reference: `table.column` -- must come before ColumnRef
    #[parse(atom)]
    QualRef(QualifiedRef),
    /// Parenthesized expression: `(expr)`
    #[parse(atom)]
    Paren(ParenExpr),
    /// Numeric literal: `77.7` -- must come before IntegerLit for longest match
    #[parse(atom)]
    NumericLit(literal::NumericLit),
    /// Integer literal: `42`
    #[parse(atom)]
    IntegerLit(literal::IntegerLit),
    /// String literal sequence: `'hello'` or `'first' 'second' ...` —
    /// Postgres concatenates adjacent string literals into one.
    #[parse(atom)]
    StringLit(StringLitSeq),
    /// Boolean true
    #[parse(atom)]
    BoolTrue(keyword::True),
    /// Boolean false
    #[parse(atom)]
    BoolFalse(keyword::False),
    /// NULL
    #[parse(atom)]
    Null(keyword::Null),
    /// `DEFAULT` — placeholder usable in INSERT/UPDATE value positions.
    #[parse(atom)]
    Default(keyword::Default),
    /// Unqualified column reference: `f1` or `"Foo"`
    #[parse(atom)]
    ColumnRef(literal::Ident),
    /// Bare wildcard: `*`
    #[parse(atom)]
    Star(punct::Star),
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::expr::Expr;
    use crate::rules::SqlRules;

    // --- Atom tests ---

    #[test]
    fn parse_integer_literal() {
        let mut input = Input::new("42");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::IntegerLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_string_literal() {
        let mut input = Input::new("'hello'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::StringLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_adjacent_string_literals() {
        let mut input = Input::new("'a' 'b'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        if let Expr::StringLit(seq) = &expr {
            assert_eq!(seq.parts.len(), 2);
        } else {
            panic!("expected Expr::StringLit, got {:?}", expr);
        }
        assert!(input.is_empty());
    }

    #[test]
    fn parse_three_adjacent_strings_with_alias() {
        // SELECT 'first line' ' - next line' AS foo
        use crate::ast::select::SelectStmt;
        let mut input = Input::new("SELECT 'first line' ' - next line' AS foo");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_escape_string_literal() {
        let mut input = Input::new(r"E'r_\_view%'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::EscapeStringLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_func_call_order_by() {
        let mut input = Input::new("jsonb_agg(q ORDER BY x, y)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_func_call_order_by_nulls_first() {
        let mut input = Input::new("jsonb_agg(q ORDER BY x NULLS FIRST, y)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_func_call_variadic() {
        let mut input = Input::new("jsonb_build_array(VARIADIC a)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_timestamp_with_tz_literal() {
        let mut input = Input::new("timestamp with time zone '2001-12-27 04:05:06+08'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::TimestampLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_time_literal() {
        let mut input = Input::new("time '12:34'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::TimeLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_date_literal_as_castfunc() {
        let mut input = Input::new("date '2024-01-01'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        // `date` is an Ident-based TypeName, so this parses as CastFunc.
        assert!(matches!(expr, Expr::CastFunc(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_interval_literal_as_castfunc() {
        let mut input = Input::new("interval '1 hour'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::CastFunc(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_escape_string_literal_lowercase_e() {
        let mut input = Input::new("e'foo'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::EscapeStringLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_bool_true() {
        let mut input = Input::new("true");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTrue(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_bool_false() {
        let mut input = Input::new("false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolFalse(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_null() {
        let mut input = Input::new("null");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Null(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_column_ref() {
        let mut input = Input::new("f1");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::ColumnRef(_)));
    }

    #[test]
    fn parse_qualified_column_ref() {
        let mut input = Input::new("BOOLTBL1.f1");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::QualRef(_)));
    }

    #[test]
    fn parse_qualified_wildcard() {
        let mut input = Input::new("BOOLTBL1.*");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::QualWild(_)));
    }

    #[test]
    fn parse_star() {
        let mut input = Input::new("*");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Star(_)));
    }

    #[test]
    fn parse_function_call_no_args() {
        let mut input = Input::new("foo()");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_function_call_with_args() {
        let mut input = Input::new("pg_input_is_valid('true', 'bool')");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_function_call_booleq() {
        let mut input = Input::new("booleq(bool 'false', f1)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_parenthesized_expr() {
        let mut input = Input::new("(1)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Paren(_)));
    }

    // --- Type cast function-style: bool 'foo' ---

    #[test]
    fn parse_type_cast_bool_string() {
        let mut input = Input::new("bool 't'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::CastFunc(_)));
    }

    #[test]
    fn parse_type_cast_boolean_string() {
        let mut input = Input::new("boolean 'false'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::CastFunc(_)));
    }

    // --- Prefix operators ---

    #[test]
    fn parse_not_expr() {
        let mut input = Input::new("not false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Not(_, _)));
    }

    // --- Infix operators ---

    #[test]
    fn parse_and_expr() {
        let mut input = Input::new("true AND false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::And(..)));
    }

    #[test]
    fn parse_or_expr() {
        let mut input = Input::new("true OR false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Or(..)));
    }

    #[test]
    fn parse_eq_expr() {
        let mut input = Input::new("f1 = true");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Eq(..)));
    }

    #[test]
    fn parse_neq_expr() {
        let mut input = Input::new("f1 <> false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Neq(..)));
    }

    // --- Postfix: :: type cast ---

    #[test]
    fn parse_cast_colon_colon() {
        let mut input = Input::new("0::boolean");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Cast(..)));
    }

    #[test]
    fn parse_chained_cast() {
        let mut input = Input::new("'TrUe'::text::boolean");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        // Outer should be Cast
        assert!(matches!(expr, Expr::Cast(..)));
    }

    // --- Postfix: IS [NOT] TRUE/FALSE/UNKNOWN/NULL ---

    #[test]
    fn parse_is_true() {
        let mut input = Input::new("f1 IS TRUE");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_not_false() {
        let mut input = Input::new("f1 IS NOT FALSE");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_unknown() {
        let mut input = Input::new("b IS UNKNOWN");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_not_unknown() {
        let mut input = Input::new("b IS NOT UNKNOWN");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    // --- Postfix: BETWEEN / NOT BETWEEN ---

    #[test]
    fn parse_between_expr() {
        let mut input = Input::new("a BETWEEN 12 AND 17");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BetweenExpr(..)));
    }

    #[test]
    fn parse_not_between_expr() {
        let mut input = Input::new("a NOT BETWEEN 1 AND 5");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NotBetweenExpr(..)));
    }

    #[test]
    fn parse_between_as_value() {
        // BETWEEN yields a boolean value that can appear in a SELECT list.
        let mut input = Input::new("x BETWEEN a AND b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BetweenExpr(..)));
    }

    #[test]
    fn between_does_not_break_and_parse() {
        // A plain AND expression must still parse as And, not be confused
        // with the BETWEEN postfix.
        let mut input = Input::new("a AND b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::And(..)));
    }

    // --- Precedence ---

    #[test]
    fn and_binds_tighter_than_or() {
        // a OR b AND c should parse as a OR (b AND c)
        let mut input = Input::new("true OR false AND true");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        // Top-level should be OR
        match &expr {
            Expr::Or(..) => {}
            other => panic!("expected OR at top level, got {other:?}"),
        }
    }

    #[test]
    fn comparison_binds_tighter_than_and() {
        // a AND b = c should parse as a AND (b = c)
        let mut input = Input::new("true AND f1 = false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        match &expr {
            Expr::And(..) => {}
            other => panic!("expected AND at top level, got {other:?}"),
        }
    }

    #[test]
    fn bool_cast_or_expr() {
        // bool 't' or bool 'f' should parse as (bool 't') OR (bool 'f')
        let mut input = Input::new("bool 't' or bool 'f'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Or(..)));
    }

    #[test]
    fn is_true_in_select_item() {
        // b IS TRUE should parse without consuming AS that follows
        let mut input = Input::new("b IS TRUE");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn cast_chain_in_expression() {
        // true::boolean::text should chain
        let mut input = Input::new("true::boolean::text");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Cast(..)));
    }

    // --- Arithmetic operators ---

    #[test]
    fn parse_addition() {
        let mut input = Input::new("4+4");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Add(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_subtraction() {
        let mut input = Input::new("10-3");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Sub(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_unary_minus() {
        let mut input = Input::new("-1");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Neg(..)));
        assert!(input.is_empty());
    }

    // --- Numeric literal ---

    #[test]
    fn parse_numeric_literal() {
        let mut input = Input::new("77.7");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NumericLit(_)));
        assert!(input.is_empty());
    }

    // --- IN expression ---

    #[test]
    fn parse_in_expr() {
        let mut input = Input::new("f1 IN (1, 2, 3)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::InExpr(..)));
        assert!(input.is_empty());
    }

    // --- JSON / JSONB operators ---

    #[test]
    fn parse_json_field() {
        let mut input = Input::new("data -> 'key'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonField(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_json_field_text() {
        let mut input = Input::new("data ->> 'key'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonFieldText(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_json_path() {
        let mut input = Input::new("data #> '{a,b}'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonPath(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_json_path_text() {
        let mut input = Input::new("data #>> '{a,b}'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonPathText(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_contains() {
        let mut input = Input::new("a @> b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonContains(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_contained_by() {
        let mut input = Input::new("a <@ b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonContainedBy(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_key_exists() {
        let mut input = Input::new("a ? 'k'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonKey(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_any_key() {
        let mut input = Input::new("a ?| b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonAnyKey(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_all_keys() {
        let mut input = Input::new("a ?& b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonAllKeys(..)));
        assert!(input.is_empty());
    }

    // --- Postgres text-search / range / geometric operators ---

    #[test]
    fn parse_ts_match() {
        let mut input = Input::new("a @@ 'foo|bar'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::TsMatch(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_ts_match3() {
        let mut input = Input::new("a @@@ b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::TsMatch3(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_json_path_exists() {
        let mut input = Input::new("j @? '$.a'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonPathExists(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_overlap() {
        let mut input = Input::new("r && s");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Overlap(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_strictly_left() {
        let mut input = Input::new("a << b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::StrictlyLeft(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_strictly_right() {
        let mut input = Input::new("a >> b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::StrictlyRight(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_subset_eq() {
        let mut input = Input::new("a <<= b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::SubsetEq(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_superset_eq() {
        let mut input = Input::new("a >>= b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::SupersetEq(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_adjacent() {
        let mut input = Input::new("a -|- b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Adjacent(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_distance() {
        let mut input = Input::new("p1 <-> p2");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Distance(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_no_extend_right() {
        let mut input = Input::new("a &< b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NoExtendRight(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_no_extend_left() {
        let mut input = Input::new("a &> b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NoExtendLeft(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_strictly_above() {
        let mut input = Input::new("a |>> b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::StrictlyAbove(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_strictly_below() {
        let mut input = Input::new("a <<| b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::StrictlyBelow(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_no_extend_above() {
        let mut input = Input::new("a &<| b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NoExtendAbove(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_no_extend_below() {
        let mut input = Input::new("a |&> b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NoExtendBelow(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_intersect() {
        let mut input = Input::new("a ?# b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Intersect(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_horizontal() {
        let mut input = Input::new("a ?- b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Horizontal(..)));
        assert!(input.is_empty());
    }

    // --- LIKE / ILIKE ---

    #[test]
    fn parse_like_expr() {
        let mut input = Input::new("table_name LIKE 'foo%'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Like(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_like_escape_string() {
        let mut input = Input::new(r"table_name LIKE E'r_\_view%'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Like(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_not_like_expr() {
        let mut input = Input::new("table_name NOT LIKE 'bar%'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NotLike(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_ilike_expr() {
        let mut input = Input::new("name ILIKE '%FOO%'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Ilike(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_not_ilike_expr() {
        let mut input = Input::new("name NOT ILIKE '%bar%'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NotIlike(..)));
        assert!(input.is_empty());
    }

    // --- Regex match operators ---

    #[test]
    fn parse_regex_match() {
        let mut input = Input::new("relname ~ '^foo'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::RegexMatch(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_regex_not_match() {
        let mut input = Input::new("name !~ 'bar'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::RegexNotMatch(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_regex_imatch() {
        let mut input = Input::new("name ~* 'FOO'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::RegexIMatch(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_regex_not_imatch() {
        let mut input = Input::new("name !~* '.*'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::RegexNotIMatch(..)));
        assert!(input.is_empty());
    }

    // --- COLLATE postfix ---

    #[test]
    fn parse_collate_postfix() {
        let mut input = Input::new("a COLLATE \"C\"");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Collate(..)));
        assert!(input.is_empty());
    }

    // --- DEFAULT atom ---

    #[test]
    fn parse_default_atom() {
        let mut input = Input::new("DEFAULT");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Default(_)));
        assert!(input.is_empty());
    }

    // --- Subquery expression ---

    #[test]
    fn parse_subquery_expr() {
        let mut input = Input::new("(SELECT 1)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Paren(_)));
        assert!(input.is_empty());
    }
}
