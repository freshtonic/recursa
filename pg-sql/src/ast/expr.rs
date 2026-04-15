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
pub struct StringLitSeq<'input> {
    pub parts: Seq<literal::StringLit<'input>, (), OptionalTrailing, NonEmpty>,
}

/// Content inside IN parentheses: either a subquery or expression list.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InContent<'input> {
    Subquery(Box<crate::ast::values::CompoundQuery>),
    Exprs(Seq<Expr<'input>, punct::Comma>),
}

/// `IN (expr, ...)` or `IN (subquery)` postfix suffix.
pub type InList<'input> = Surrounded<punct::LParen, InContent<'input>, punct::RParen>;

/// Parenthesized precision/scale for type names: `(10,2)` or `(3)`.
pub type TypePrecision<'input> =
    Surrounded<punct::LParen, Seq<literal::IntegerLit<'input>, punct::Comma>, punct::RParen>;

/// Array type suffix: `[]`
#[derive(Debug, Clone, FormatTokens, PartialEq, Eq, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ArrayTypeSuffix(pub punct::LBracket, pub punct::RBracket);

/// Type name for casts.
#[derive(Debug, Clone, FormatTokens, PartialEq, Eq, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TypeName<'input> {
    Bool(keyword::Bool),
    Boolean(keyword::Boolean),
    Text(keyword::Text),
    Integer(keyword::Integer),
    Int(keyword::Int),
    Serial(keyword::Serial),
    Numeric(keyword::Numeric),
    Varchar(keyword::Varchar),
    Ident(literal::Ident<'input>),
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
pub struct QualifiedRef<'input> {
    pub table: literal::AliasName<'input>,
    pub dot: punct::Dot,
    pub column: literal::AliasName<'input>,
}

/// Qualified wildcard: `table.*`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct QualifiedWildcard<'input> {
    pub table: literal::AliasName<'input>,
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
pub struct WindowSpec<'input> {
    pub _over: keyword::Over,
    pub body: WindowSpecBody<'input>,
}

/// Body of an OVER clause.
///
/// Variant ordering: Inline (starts with `(`) before Named (starts with an
/// identifier). They start with different tokens so peek disambiguation is
/// trivial.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum WindowSpecBody<'input> {
    Inline(Surrounded<punct::LParen, InlineWindowSpec<'input>, punct::RParen>),
    Named(literal::Ident<'input>),
}

/// Interior of an inline window spec (between the parens).
///
/// The optional `ref_name` is an existing-window reference (e.g.
/// `WINDOW w2 AS (w1 ORDER BY x)`). It relies on `Option<literal::Ident>`
/// peek-disambiguating cleanly against `PARTITION`/`ORDER`/`ROWS`/etc.
/// because keywords are rejected by `literal::Ident`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct InlineWindowSpec<'input> {
    pub ref_name: Option<literal::Ident<'input>>,
    pub partition_by: Option<WindowPartitionBy<'input>>,
    pub order_by: Option<crate::ast::select::OrderByClause>,
    pub frame: Option<WindowFrameClause<'input>>,
}

/// PARTITION BY in window: `PARTITION BY expr, ...`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowPartitionBy<'input> {
    pub _partition: keyword::Partition,
    pub _by: keyword::By,
    pub exprs: Seq<Expr<'input>, punct::Comma>,
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
pub enum WindowFrameClause<'input> {
    Between(WindowFrameBetween<'input>),
    Single(WindowFrameSingle<'input>),
}

/// `unit BETWEEN start AND end [EXCLUDE ...]`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowFrameBetween<'input> {
    pub unit: WindowFrameUnit,
    pub _between: keyword::Between,
    pub start: WindowFrameBound<'input>,
    pub _and: keyword::And,
    pub end: WindowFrameBound<'input>,
    pub exclude: Option<WindowFrameExclude>,
}

/// `unit start [EXCLUDE ...]`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowFrameSingle<'input> {
    pub unit: WindowFrameUnit,
    pub bound: WindowFrameBound<'input>,
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
pub enum WindowFrameBound<'input> {
    UnboundedPreceding(UnboundedPreceding),
    UnboundedFollowing(UnboundedFollowing),
    CurrentRow(CurrentRow),
    ExprPreceding(ExprPreceding<'input>),
    ExprFollowing(ExprFollowing<'input>),
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
pub struct ExprPreceding<'input> {
    pub expr: Box<Expr<'input>>,
    pub _preceding: keyword::Preceding,
}

#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ExprFollowing<'input> {
    pub expr: Box<Expr<'input>>,
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
pub enum FuncArg<'input> {
    Variadic(VariadicArg<'input>),
    Plain(Box<Expr<'input>>),
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VariadicArg<'input> {
    pub _variadic: keyword::Variadic,
    pub value: Box<Expr<'input>>,
}

/// `WITHIN GROUP (ORDER BY ...)` clause for ordered-set aggregate functions.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithinGroupClause {
    pub _within: PhantomData<keyword::Within>,
    pub _group: PhantomData<keyword::Group>,
    pub order_by:
        Surrounded<punct::LParen, Box<crate::ast::select::OrderByClause>, punct::RParen>,
}

/// `FILTER (WHERE condition)` clause for filtered aggregates.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FilterClause {
    pub _filter: PhantomData<keyword::Filter>,
    pub body: Surrounded<punct::LParen, Box<crate::ast::select::WhereClause>, punct::RParen>,
}

/// Function call: `name([*] [DISTINCT] args [ORDER BY ...]) [WITHIN GROUP (...)] [FILTER (...)] [OVER (...)]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncCall<'input> {
    pub name: literal::AliasName<'input>,
    pub lparen: punct::LParen,
    pub star_arg: Option<punct::Star>,
    pub distinct: Option<DistinctKw>,
    pub args: Seq<FuncArg<'input>, punct::Comma>,
    pub order_by: Option<Box<crate::ast::select::OrderByClause>>,
    pub rparen: punct::RParen,
    pub within_group: Option<WithinGroupClause>,
    pub filter: Option<FilterClause>,
    pub window: Option<WindowSpec<'input>>,
}

/// Content inside parentheses: either a subquery or a comma-separated expression list.
/// Subquery (CompoundQuery) must come first so SELECT/VALUES/WITH keywords are matched
/// before trying to parse as a regular expression.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ParenContent<'input> {
    Subquery(Box<crate::ast::values::CompoundQuery>),
    Exprs(Seq<Expr<'input>, punct::Comma>),
}

/// Parenthesized expression: `(expr)`, `(expr, expr, ...)`, or `(SELECT/VALUES ...)`
pub type ParenExpr<'input> = Surrounded<punct::LParen, ParenContent<'input>, punct::RParen>;

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
pub struct ArrayBracket<'input> {
    pub _array: PhantomData<keyword::Array>,
    pub lbracket: punct::LBracket,
    pub elements: Seq<Expr<'input>, punct::Comma>,
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
pub enum ArrayExpr<'input> {
    Bracket(ArrayBracket<'input>),
    Subquery(ArraySubquery),
}

/// ROW constructor: `ROW(expr, ...)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct RowExpr<'input> {
    pub _row: keyword::Row,
    pub values: Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
}

/// Cast type with optional precision and zero-or-more array suffixes:
/// `numeric(10,0)`, `integer[]`, `int4[][][]`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct CastType<'input> {
    pub base: TypeName<'input>,
    pub precision: Option<TypePrecision<'input>>,
    pub array_suffixes: Vec<ArrayTypeSuffix>,
}

/// NOT IN list: `expr NOT IN (val, ...)` suffix.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct NotInSuffix<'input> {
    pub _not: keyword::Not,
    pub _in: keyword::In,
    pub list: InList<'input>,
}

/// Function-style type cast: `bool 'value'`, `text 'hello'`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct TypeCastFunc<'input> {
    pub type_name: TypeName<'input>,
    pub value: literal::StringLit<'input>,
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
pub struct TimestampLit<'input> {
    pub _timestamp: keyword::Timestamp,
    pub tz: Option<TimeZoneQualifier>,
    pub value: literal::StringLit<'input>,
}

/// `TIME [WITH|WITHOUT TIME ZONE] 'string'`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct TimeLit<'input> {
    pub _time: keyword::Time,
    pub tz: Option<TimeZoneQualifier>,
    pub value: literal::StringLit<'input>,
}

// --- XML function atoms ---
//
// Postgres `xmlelement` / `xmlattributes` / `xmlforest` use special syntax
// that does not fit a plain `FuncCall` (positional comma-separated exprs):
//
//   xmlelement(NAME ident [, xmlattributes(...)] [, content_exprs])
//   xmlattributes(expr [AS alias] [, ...])
//   xmlforest(expr [AS alias] [, ...])
//
// They are modeled here as dedicated atoms declared before `FuncCall`.

/// A `name [AS alias]` argument to `xmlattributes` / `xmlforest`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlNamedArg<'input> {
    pub value: Box<Expr<'input>>,
    pub alias: Option<XmlNamedArgAlias<'input>>,
}

/// `AS alias` suffix on an XML named argument.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlNamedArgAlias<'input> {
    pub _as: PhantomData<keyword::As>,
    pub name: literal::AliasName<'input>,
}

/// `xmlattributes(expr [AS alias], ...)` — used as a positional argument
/// to `xmlelement`, but also can be parsed standalone.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlAttributes<'input> {
    pub _kw: PhantomData<keyword::XmlAttributesKw>,
    pub args: Surrounded<punct::LParen, Seq<XmlNamedArg<'input>, punct::Comma>, punct::RParen>,
}

/// Optional `, xmlattributes(...) [, content_exprs]` tail of `xmlelement`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlElementAttrsTail<'input> {
    pub _comma: punct::Comma,
    pub attrs: XmlAttributes<'input>,
    pub content: Option<XmlElementContentTail<'input>>,
}

/// Optional `, content_exprs` tail of `xmlelement`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlElementContentTail<'input> {
    pub _comma: punct::Comma,
    pub exprs: Seq<Expr<'input>, punct::Comma>,
}

/// Body of `xmlelement(NAME ident [, xmlattributes(...)] [, content_exprs])`.
///
/// Variant ordering: the `WithAttrs` form starts with `, xmlattributes(`
/// (longer match) and must be tried before `WithContent` which starts with
/// just `,`. Both trail an `xmlelement(NAME ident` head.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum XmlElementTail<'input> {
    WithAttrs(XmlElementAttrsTail<'input>),
    WithContent(XmlElementContentTail<'input>),
}

/// Inner contents of an `xmlelement(...)` call.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlElementInner<'input> {
    pub _name: PhantomData<keyword::NameKw>,
    pub element_name: literal::AliasName<'input>,
    pub tail: Option<XmlElementTail<'input>>,
}

/// `xmlelement(NAME ident [, xmlattributes(...)] [, content_exprs])`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlElement<'input> {
    pub _kw: PhantomData<keyword::XmlElementKw>,
    pub inner: Surrounded<punct::LParen, XmlElementInner<'input>, punct::RParen>,
}

/// `xmlforest(expr [AS alias], ...)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlForest<'input> {
    pub _kw: PhantomData<keyword::XmlForestKw>,
    pub args: Surrounded<punct::LParen, Seq<XmlNamedArg<'input>, punct::Comma>, punct::RParen>,
}

/// `xmlpi(NAME ident [, content])`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlPi<'input> {
    pub _kw: PhantomData<keyword::XmlPiKw>,
    pub inner: Surrounded<punct::LParen, XmlPiInner<'input>, punct::RParen>,
}

/// Inner contents of an `xmlpi(...)` call.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlPiInner<'input> {
    pub _name: PhantomData<keyword::NameKw>,
    pub target: literal::AliasName<'input>,
    pub content: Option<XmlPiContentTail<'input>>,
}

/// Optional `, content_expr` tail of `xmlpi`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct XmlPiContentTail<'input> {
    pub _comma: punct::Comma,
    pub expr: Box<Expr<'input>>,
}

// --- SQL-standard string function atoms ---
//
// TRIM/SUBSTRING/POSITION/OVERLAY use special syntax with FROM/IN/PLACING/FOR
// separators inside parens that don't fit a comma-separated FuncCall.

/// Trim direction: `LEADING | TRAILING | BOTH`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TrimDir {
    Leading(keyword::Leading),
    Trailing(keyword::Trailing),
    Both(keyword::BothKw),
}

/// `[chars] FROM source`: the optional chars-to-trim before `FROM`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TrimChars<'input> {
    pub chars: Box<Expr<'input>>,
}

/// Inside of `TRIM(...)`. Forms:
///   `[LEADING|TRAILING|BOTH] [chars] FROM source`
///   (a fully-positional `TRIM(src, chars)` form is left to ordinary FuncCall).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TrimInner<'input> {
    pub dir: Option<TrimDir>,
    pub chars: Option<TrimChars<'input>>,
    pub _from: PhantomData<keyword::From>,
    pub source: Box<Expr<'input>>,
}

/// `TRIM([LEADING|TRAILING|BOTH] [chars] FROM source)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TrimCall<'input> {
    pub _kw: PhantomData<keyword::TrimKw>,
    pub inner: Surrounded<punct::LParen, TrimInner<'input>, punct::RParen>,
}

/// `FOR len` suffix in `SUBSTRING(... FROM ... FOR ...)` / `OVERLAY(...)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ForCount<'input> {
    pub _for: PhantomData<keyword::For>,
    pub count: Box<Expr<'input>>,
}

/// `FROM start [FOR len]` form for SUBSTRING.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubstringFromFor<'input> {
    pub _from: PhantomData<keyword::From>,
    pub start: Box<Expr<'input>>,
    pub for_count: Option<ForCount<'input>>,
}

/// `SIMILAR pattern ESCAPE escape` form for SUBSTRING.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubstringSimilar<'input> {
    pub _similar: PhantomData<keyword::Similar>,
    pub pattern: Box<Expr<'input>>,
    pub _escape: PhantomData<keyword::EscapeKw>,
    pub escape: Box<Expr<'input>>,
}

/// Tail of a SUBSTRING call after the source expression.
///
/// Variant ordering: `Similar` (`SIMILAR`) before `FromFor` (`FROM`) — distinct
/// first tokens, so order is not strictly required, but listed by length.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SubstringTail<'input> {
    Similar(SubstringSimilar<'input>),
    FromFor(SubstringFromFor<'input>),
}

/// Inner of `SUBSTRING(...)`: `source` followed by FROM/SIMILAR tail.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubstringInner<'input> {
    pub source: Box<Expr<'input>>,
    pub tail: SubstringTail<'input>,
}

/// `SUBSTRING(source FROM start [FOR len])` /
/// `SUBSTRING(source SIMILAR pattern ESCAPE escape)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubstringCall<'input> {
    pub _kw: PhantomData<keyword::SubstringKw>,
    pub inner: Surrounded<punct::LParen, SubstringInner<'input>, punct::RParen>,
}

/// Inner of `POSITION(needle IN haystack)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PositionInner<'input> {
    pub needle: Box<Expr<'input>>,
    pub _in: PhantomData<keyword::In>,
    pub haystack: Box<Expr<'input>>,
}

/// `POSITION(needle IN haystack)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PositionCall<'input> {
    pub _kw: PhantomData<keyword::PositionKw>,
    pub inner: Surrounded<punct::LParen, PositionInner<'input>, punct::RParen>,
}

/// Inner of `OVERLAY(source PLACING new FROM start [FOR len])`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OverlayInner<'input> {
    pub source: Box<Expr<'input>>,
    pub _placing: PhantomData<keyword::Placing>,
    pub new: Box<Expr<'input>>,
    pub _from: PhantomData<keyword::From>,
    pub start: Box<Expr<'input>>,
    pub for_count: Option<ForCount<'input>>,
}

/// `OVERLAY(source PLACING new FROM start [FOR len])`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OverlayCall<'input> {
    pub _kw: PhantomData<keyword::OverlayKw>,
    pub inner: Surrounded<punct::LParen, OverlayInner<'input>, punct::RParen>,
}

/// `UESCAPE 'c'` suffix that may follow a `U&'...'` literal.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UescapeSuffix<'input> {
    pub _uescape: PhantomData<keyword::Uescape>,
    pub escape_char: literal::StringLit<'input>,
}

/// `U&'...'` unicode string literal with optional `UESCAPE 'c'` suffix.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnicodeStringLitWithEscape<'input> {
    pub lit: literal::UnicodeStringLit<'input>,
    pub uescape: Option<UescapeSuffix<'input>>,
}

// --- Pratt expression enum ---

/// SQL expression with Pratt-derived parsing.
#[derive(FormatTokens, Parse, Debug, Clone, Visit)]
#[parse(rules = SqlRules, pratt)]
pub enum Expr<'input> {
    // --- Prefix ---
    #[parse(prefix, bp = 15)]
    Not(keyword::Not, Box<Expr<'input>>),
    #[parse(prefix, bp = 12)]
    Neg(punct::Minus, Box<Expr<'input>>),

    // --- Postfix ---
    /// Postgres-style cast: `expr::type`
    #[parse(postfix, bp = 20)]
    Cast(Box<Expr<'input>>, punct::ColonColon, CastType<'input>),
    /// Array subscript: `expr[idx]`
    #[parse(postfix, bp = 20)]
    Subscript(
        Box<Expr<'input>>,
        punct::LBracket,
        Box<Expr<'input>>,
        punct::RBracket,
    ),
    /// `expr COLLATE "collation"` — collation specifier. Binds tighter than
    /// comparisons (bp 5) but looser than `::` cast (bp 20).
    #[parse(postfix, bp = 18)]
    Collate(Box<Expr<'input>>, keyword::Collate, literal::Ident<'input>),
    /// Boolean test: `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`
    #[parse(postfix, bp = 8)]
    BoolTest(Box<Expr<'input>>, keyword::Is, BoolTestKind),
    /// NOT IN list: `expr NOT IN (val, ...)`
    #[parse(postfix, bp = 6)]
    NotInExpr(Box<Expr<'input>>, NotInSuffix<'input>),
    /// `expr NOT ILIKE pattern`. Declared before `NotLike` so the longer
    /// `NOT ILIKE` is tried first (matters only if any rule shares a prefix;
    /// here `NOT ILIKE` vs `NOT LIKE` differ on the second token).
    #[parse(postfix, bp = 5, inner_bp = 6)]
    NotIlike(
        Box<Expr<'input>>,
        keyword::Not,
        keyword::Ilike,
        Box<Expr<'input>>,
    ),
    /// `expr NOT LIKE pattern`. Must come before the `Not` prefix atom so
    /// longest-match-wins prefers the postfix form.
    #[parse(postfix, bp = 5, inner_bp = 6)]
    NotLike(
        Box<Expr<'input>>,
        keyword::Not,
        keyword::Like,
        Box<Expr<'input>>,
    ),
    /// `expr ILIKE pattern`
    #[parse(infix, bp = 5)]
    Ilike(Box<Expr<'input>>, keyword::Ilike, Box<Expr<'input>>),
    /// `expr LIKE pattern`
    #[parse(infix, bp = 5)]
    Like(Box<Expr<'input>>, keyword::Like, Box<Expr<'input>>),
    /// `expr !~* pattern` — POSIX case-insensitive negated regex match.
    #[parse(infix, bp = 5)]
    RegexNotIMatch(Box<Expr<'input>>, punct::BangTildeStar, Box<Expr<'input>>),
    /// `expr ~* pattern` — POSIX case-insensitive regex match.
    #[parse(infix, bp = 5)]
    RegexIMatch(Box<Expr<'input>>, punct::TildeStar, Box<Expr<'input>>),
    /// `expr !~ pattern` — POSIX negated regex match.
    #[parse(infix, bp = 5)]
    RegexNotMatch(Box<Expr<'input>>, punct::BangTilde, Box<Expr<'input>>),
    /// `expr ~ pattern` — POSIX regex match.
    #[parse(infix, bp = 5)]
    RegexMatch(Box<Expr<'input>>, punct::Tilde, Box<Expr<'input>>),
    /// IN list: `expr IN (val, ...)`
    #[parse(postfix, bp = 6)]
    InExpr(Box<Expr<'input>>, keyword::In, InList<'input>),
    /// `expr NOT BETWEEN low AND high`. Declared before `BetweenExpr` so
    /// the longer `NOT BETWEEN` prefix wins disambiguation. `inner_bp = 3`
    /// keeps the low/high operands from swallowing the literal `AND` that
    /// separates them (the `AND` infix has `bp = 2`).
    #[parse(postfix, bp = 6, inner_bp = 3)]
    NotBetweenExpr(
        Box<Expr<'input>>,
        keyword::Not,
        keyword::Between,
        Box<Expr<'input>>,
        keyword::And,
        Box<Expr<'input>>,
    ),
    /// `expr BETWEEN low AND high`. See `NotBetweenExpr` for the
    /// `inner_bp` rationale.
    #[parse(postfix, bp = 6, inner_bp = 3)]
    BetweenExpr(
        Box<Expr<'input>>,
        keyword::Between,
        Box<Expr<'input>>,
        keyword::And,
        Box<Expr<'input>>,
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
    JsonPathText(Box<Expr<'input>>, punct::HashArrowArrow, Box<Expr<'input>>),
    /// JSON path: `expr #> path`
    #[parse(infix, bp = 10)]
    JsonPath(Box<Expr<'input>>, punct::HashArrow, Box<Expr<'input>>),
    /// JSON field as text: `expr ->> field`
    #[parse(infix, bp = 10)]
    JsonFieldText(Box<Expr<'input>>, punct::ArrowArrow, Box<Expr<'input>>),
    /// JSON field: `expr -> field`
    #[parse(infix, bp = 10)]
    JsonField(Box<Expr<'input>>, punct::Arrow, Box<Expr<'input>>),
    /// JSON any-key-exists: `expr ?| keys`
    #[parse(infix, bp = 10)]
    JsonAnyKey(Box<Expr<'input>>, punct::QuestionPipe, Box<Expr<'input>>),
    /// JSON all-keys-exist: `expr ?& keys`
    #[parse(infix, bp = 10)]
    JsonAllKeys(Box<Expr<'input>>, punct::QuestionAmp, Box<Expr<'input>>),
    /// Geometric intersect: `a ?# b`. Must precede `JsonKey` (`?`).
    #[parse(infix, bp = 5)]
    Intersect(Box<Expr<'input>>, punct::QuestionHash, Box<Expr<'input>>),
    /// Geometric horizontal: `a ?- b`. Must precede `JsonKey` (`?`).
    #[parse(infix, bp = 5)]
    Horizontal(Box<Expr<'input>>, punct::QuestionDash, Box<Expr<'input>>),
    /// JSON key-exists: `expr ? key`
    #[parse(infix, bp = 10)]
    JsonKey(Box<Expr<'input>>, punct::Question, Box<Expr<'input>>),
    /// JSONB contains: `expr @> expr`
    #[parse(infix, bp = 10)]
    JsonContains(Box<Expr<'input>>, punct::AtGt, Box<Expr<'input>>),
    /// JSONB contained-by: `expr <@ expr`
    #[parse(infix, bp = 10)]
    JsonContainedBy(Box<Expr<'input>>, punct::LtAt, Box<Expr<'input>>),

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
    TsMatch3(Box<Expr<'input>>, punct::AtAtAt, Box<Expr<'input>>),
    /// Geometric strictly-below: `a <<| b`. Before `StrictlyLeft` (`<<`).
    #[parse(infix, bp = 5)]
    StrictlyBelow(Box<Expr<'input>>, punct::LtLtPipe, Box<Expr<'input>>),
    /// Inet is-subset-or-equal: `a <<= b`. Before `StrictlyLeft` (`<<`).
    #[parse(infix, bp = 5)]
    SubsetEq(Box<Expr<'input>>, punct::LtLtEq, Box<Expr<'input>>),
    /// Distance: `a <-> b`. Before any `<` variant.
    #[parse(infix, bp = 10)]
    Distance(Box<Expr<'input>>, punct::LtMinusGt, Box<Expr<'input>>),
    /// Inet is-superset-or-equal: `a >>= b`. Before `StrictlyRight` (`>>`).
    #[parse(infix, bp = 5)]
    SupersetEq(Box<Expr<'input>>, punct::GtGtEq, Box<Expr<'input>>),
    /// Range adjacent: `a -|- b`. Before `Sub` (`-`).
    #[parse(infix, bp = 5)]
    Adjacent(Box<Expr<'input>>, punct::MinusPipeMinus, Box<Expr<'input>>),
    /// Geometric strictly-above: `a |>> b`. Before `Concat` (`||`).
    #[parse(infix, bp = 5)]
    StrictlyAbove(Box<Expr<'input>>, punct::PipeGtGt, Box<Expr<'input>>),
    /// Geometric no-extend-below: `a |&> b`. Before `Concat` (`||`).
    #[parse(infix, bp = 5)]
    NoExtendBelow(Box<Expr<'input>>, punct::PipeAmpGt, Box<Expr<'input>>),
    /// Geometric no-extend-above: `a &<| b`. Before `NoExtendRight` (`&<`).
    #[parse(infix, bp = 5)]
    NoExtendAbove(Box<Expr<'input>>, punct::AmpLtPipe, Box<Expr<'input>>),

    // --- 2-char operators ---
    /// Text-search / jsonb path match: `expr @@ expr`.
    #[parse(infix, bp = 5)]
    TsMatch(Box<Expr<'input>>, punct::AtAt, Box<Expr<'input>>),
    /// Jsonpath exists: `expr @? path`.
    #[parse(infix, bp = 5)]
    JsonPathExists(Box<Expr<'input>>, punct::AtQuestion, Box<Expr<'input>>),
    /// Range / array overlap: `a && b`.
    #[parse(infix, bp = 10)]
    Overlap(Box<Expr<'input>>, punct::AmpAmp, Box<Expr<'input>>),
    /// Range does-not-extend-right: `a &< b`.
    #[parse(infix, bp = 5)]
    NoExtendRight(Box<Expr<'input>>, punct::AmpLt, Box<Expr<'input>>),
    /// Range does-not-extend-left: `a &> b`.
    #[parse(infix, bp = 5)]
    NoExtendLeft(Box<Expr<'input>>, punct::AmpGt, Box<Expr<'input>>),
    /// Range strictly-left-of: `a << b`.
    #[parse(infix, bp = 5)]
    StrictlyLeft(Box<Expr<'input>>, punct::LtLt, Box<Expr<'input>>),
    /// Range strictly-right-of: `a >> b`.
    #[parse(infix, bp = 5)]
    StrictlyRight(Box<Expr<'input>>, punct::GtGt, Box<Expr<'input>>),

    #[parse(infix, bp = 1)]
    Or(Box<Expr<'input>>, keyword::Or, Box<Expr<'input>>),
    #[parse(infix, bp = 2)]
    And(Box<Expr<'input>>, keyword::And, Box<Expr<'input>>),
    #[parse(infix, bp = 5)]
    BangEq(Box<Expr<'input>>, punct::BangEq, Box<Expr<'input>>),
    #[parse(infix, bp = 5)]
    Neq(Box<Expr<'input>>, punct::Neq, Box<Expr<'input>>),
    #[parse(infix, bp = 5)]
    Lte(Box<Expr<'input>>, punct::Lte, Box<Expr<'input>>),
    #[parse(infix, bp = 5)]
    Gte(Box<Expr<'input>>, punct::Gte, Box<Expr<'input>>),
    #[parse(infix, bp = 5)]
    Eq(Box<Expr<'input>>, punct::Eq, Box<Expr<'input>>),
    #[parse(infix, bp = 5)]
    Lt(Box<Expr<'input>>, punct::Lt, Box<Expr<'input>>),
    #[parse(infix, bp = 5)]
    Gt(Box<Expr<'input>>, punct::Gt, Box<Expr<'input>>),
    /// String concatenation: `expr || expr`
    #[parse(infix, bp = 10)]
    Concat(Box<Expr<'input>>, punct::Concat, Box<Expr<'input>>),
    #[parse(infix, bp = 10)]
    Add(Box<Expr<'input>>, punct::Plus, Box<Expr<'input>>),
    #[parse(infix, bp = 10)]
    Sub(Box<Expr<'input>>, punct::Minus, Box<Expr<'input>>),
    /// Multiplication: `expr * expr`
    #[parse(infix, bp = 11)]
    Mul(Box<Expr<'input>>, punct::Star, Box<Expr<'input>>),
    /// Division: `expr / expr`
    #[parse(infix, bp = 11)]
    Div(Box<Expr<'input>>, punct::Slash, Box<Expr<'input>>),
    /// Modulo: `expr % expr`
    #[parse(infix, bp = 11)]
    Mod(Box<Expr<'input>>, punct::Percent, Box<Expr<'input>>),

    // --- Atoms ---
    /// EXISTS subquery: `EXISTS (SELECT ...)`
    #[parse(atom)]
    Exists(ExistsExpr),
    /// ARRAY constructor: `ARRAY[...]` or `ARRAY(...)`
    #[parse(atom)]
    Array(ArrayExpr<'input>),
    /// ROW constructor: `ROW(...)`
    #[parse(atom)]
    RowExpr(RowExpr<'input>),
    /// Unicode string literal: `U&'...'` with optional `UESCAPE 'c'`. Must
    /// come before `CastFunc` and `StringLit` for the same reason as
    /// `EscapeStringLit`.
    #[parse(atom)]
    UnicodeStringLit(UnicodeStringLitWithEscape<'input>),
    /// Escape string literal: `E'foo\n'`. Must come before `CastFunc` and
    /// `StringLit` — `CastFunc` is `TypeName StringLit` and would match `e`
    /// as a type name followed by the string literal.
    #[parse(atom)]
    EscapeStringLit(literal::EscapeStringLit<'input>),
    /// `TIMESTAMP [WITH|WITHOUT TIME ZONE] 'string'`. Must come before `CastFunc`
    /// since `timestamp` is also an identifier.
    #[parse(atom)]
    TimestampLit(TimestampLit<'input>),
    /// `TIME [WITH|WITHOUT TIME ZONE] 'string'`. Must come before `CastFunc`.
    #[parse(atom)]
    TimeLit(TimeLit<'input>),
    /// Function-style type cast: `bool 't'` -- must come before ColumnRef
    /// since type keywords like `bool` overlap with identifiers
    #[parse(atom)]
    CastFunc(TypeCastFunc<'input>),
    /// `xmlelement(NAME ident [, xmlattributes(...)] [, content])`. Must come
    /// before `Func` so `xmlelement(` is matched as the special form.
    #[parse(atom)]
    XmlElement(XmlElement<'input>),
    /// `xmlforest(expr [AS alias], ...)`. Before `Func` for the same reason.
    #[parse(atom)]
    XmlForest(XmlForest<'input>),
    /// `xmlattributes(expr [AS alias], ...)`. Before `Func`.
    #[parse(atom)]
    XmlAttributes(XmlAttributes<'input>),
    /// `xmlpi(NAME ident [, content])`. Before `Func`.
    #[parse(atom)]
    XmlPi(XmlPi<'input>),
    /// `TRIM([LEADING|TRAILING|BOTH] [chars] FROM source)`. Before `Func`
    /// since `trim` is also a valid function-call identifier.
    #[parse(atom)]
    Trim(TrimCall<'input>),
    /// `SUBSTRING(source FROM ... | SIMILAR ...)`. Before `Func`.
    #[parse(atom)]
    Substring(SubstringCall<'input>),
    /// `POSITION(needle IN haystack)`. Before `Func`.
    #[parse(atom)]
    Position(PositionCall<'input>),
    /// `OVERLAY(source PLACING new FROM start [FOR len])`. Before `Func`.
    #[parse(atom)]
    Overlay(OverlayCall<'input>),
    /// Function call: `func(args)` -- must come before ColumnRef
    #[parse(atom)]
    Func(FuncCall<'input>),
    /// Qualified wildcard: `table.*` -- must come before QualRef and ColumnRef
    #[parse(atom)]
    QualWild(QualifiedWildcard<'input>),
    /// Qualified column reference: `table.column` -- must come before ColumnRef
    #[parse(atom)]
    QualRef(QualifiedRef<'input>),
    /// Parenthesized expression: `(expr)`
    #[parse(atom)]
    Paren(ParenExpr<'input>),
    /// Numeric literal: `77.7` -- must come before IntegerLit for longest match
    #[parse(atom)]
    NumericLit(literal::NumericLit<'input>),
    /// Integer literal: `42`
    #[parse(atom)]
    IntegerLit(literal::IntegerLit<'input>),
    /// String literal sequence: `'hello'` or `'first' 'second' ...` —
    /// Postgres concatenates adjacent string literals into one.
    #[parse(atom)]
    StringLit(StringLitSeq<'input>),
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
    ColumnRef(literal::Ident<'input>),
    /// psql client variable substitution: `:foo`, `:'foo'`, `:"foo"`.
    #[parse(atom)]
    PsqlVar(literal::PsqlVar<'input>),
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
    fn parse_three_part_string_concat() {
        // 3-part adjacent string literal concatenation. Postgres concatenates
        // these into a single value at parse time.
        let mut input = Input::new("'first' 'second' 'third'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        if let Expr::StringLit(seq) = &expr {
            assert_eq!(seq.parts.len(), 3);
        } else {
            panic!("expected StringLit, got {:?}", expr);
        }
        assert!(input.is_empty());
    }

    #[test]
    fn parse_four_part_string_concat() {
        let mut input = Input::new("'a' 'b' 'c' 'd'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        if let Expr::StringLit(seq) = &expr {
            assert_eq!(seq.parts.len(), 4);
        } else {
            panic!("expected StringLit");
        }
    }

    #[test]
    fn parse_three_adjacent_strings_with_quoted_alias() {
        use crate::ast::select::SelectStmt;
        let mut input = Input::new(
            "SELECT 'first line' ' - next line' ' - third line' AS \"Three lines to one\"",
        );
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
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
    fn parse_xmlelement_simple() {
        let mut input = Input::new("xmlelement(name foo, 'content')");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::XmlElement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_xmlelement_with_attributes() {
        let mut input =
            Input::new("xmlelement(name foo, xmlattributes(1 as a, 2 as b), 'content')");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::XmlElement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_xmlpi_basic() {
        let mut input = Input::new("xmlpi(name foo)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::XmlPi(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_xmlpi_with_content() {
        let mut input = Input::new("xmlpi(name foo, 'bar')");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::XmlPi(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_unicode_string_lit_basic() {
        let mut input = Input::new(r"U&'d\0061t\+000061'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::UnicodeStringLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_unicode_string_lit_uescape() {
        let mut input = Input::new(r"U&'d!0061t\+000061' UESCAPE '!'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::UnicodeStringLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_xmlforest() {
        let mut input = Input::new("xmlforest(a, b AS bee, c)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::XmlForest(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_exponent_numeric() {
        use crate::ast::select::SelectStmt;
        for sql in [
            "SELECT 4.5e10",
            "SELECT 4.4e131071",
            "SELECT 1.5e-5",
            "SELECT round(4.5e10, -5)",
            "SELECT .5",
            "SELECT 2e3",
        ] {
            let mut input = Input::new(sql);
            let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
            assert!(input.is_empty(), "leftover for {sql}");
        }
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
    fn parse_psql_var() {
        let mut input = Input::new(":foo_oid");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_psql_var_in_func_call() {
        let mut input = Input::new("pg_stat_get_function_calls(:func_oid)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_trim_both_from() {
        let mut input = Input::new("TRIM(BOTH FROM '  hi  ')");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_trim_leading_from() {
        let mut input = Input::new("TRIM(LEADING FROM '  hi  ')");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_trim_trailing_from() {
        let mut input = Input::new("TRIM(TRAILING FROM '  hi  ')");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_trim_both_chars_from() {
        let mut input = Input::new("TRIM(BOTH 'x' FROM 'xxhixx')");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_substring_from() {
        let mut input = Input::new("SUBSTRING('1234567890' FROM 3)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_substring_from_for() {
        let mut input = Input::new("SUBSTRING('1234567890' FROM 4 FOR 3)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_substring_similar_escape() {
        let mut input = Input::new("SUBSTRING('abcdefg' SIMILAR 'a#\"%#\"g' ESCAPE '#')");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_position_in() {
        let mut input = Input::new("POSITION('4' IN '1234567890')");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_overlay_placing_from() {
        let mut input = Input::new("OVERLAY('abcdef' PLACING '45' FROM 4)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_overlay_placing_from_for() {
        let mut input = Input::new("OVERLAY('abcdef' PLACING '45' FROM 4 FOR 2)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_func_call_within_group() {
        let mut input = Input::new("percentile_disc(0.5) WITHIN GROUP (ORDER BY v)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_func_call_within_group_multi() {
        let mut input = Input::new("rank(1, 2) WITHIN GROUP (ORDER BY a, b)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_func_call_filter() {
        let mut input = Input::new("sum(x) FILTER (WHERE y > 0)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_func_call_filter_over() {
        let mut input = Input::new("sum(x) FILTER (WHERE y > 0) OVER (PARTITION BY z)");
        let _expr = Expr::parse::<SqlRules>(&mut input).unwrap();
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
