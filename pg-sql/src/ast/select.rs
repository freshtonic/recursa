/// SELECT statement AST.
use std::marker::PhantomData;

use recursa::seq::{NoTrailing, NonEmpty, OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::common::QualifiedName;
use crate::ast::expr::{Expr, FuncCall};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// A single item in the SELECT list: `expr [AS alias]` or `expr alias`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<Alias>,
}

/// Alias with explicit AS keyword: `AS name`.
/// Uses AliasName so keywords are accepted (e.g., `SELECT 1 AS true`).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsAlias {
    pub _as: PhantomData<keyword::As>,
    pub name: literal::AliasName,
}

/// AS alias clause, or bare alias.
///
/// Variant ordering: WithAs (`AS name`) has a longer first_pattern than
/// Bare (`ident`), so longest-match-wins picks it when AS is present.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum Alias {
    WithAs(AsAlias),
    Bare(literal::Ident),
}

impl Alias {
    /// Returns the alias name regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            Alias::WithAs(a) => a.name.text(),
            Alias::Bare(ident) => ident.text(),
        }
    }
}

/// FROM clause: `FROM table [, table ...]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FromClause {
    pub _from: PhantomData<keyword::From>,
    pub tables: Seq<TableRef, punct::Comma>,
}

/// Table name with inheritance marker and optional alias: `person* p`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InheritedTable {
    pub name: QualifiedName,
    pub _star: punct::Star,
    pub alias: Option<literal::Ident>,
}

/// Table alias: `AS name [(col1, col2)]` or bare `name [(col1, col2)]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableAlias {
    pub _as: Option<PhantomData<keyword::As>>,
    pub name: literal::AliasName,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
}

/// Subquery in FROM: `(SELECT ...) AS alias`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubqueryRef {
    pub _lparen: punct::LParen,
    pub query: Box<crate::ast::values::CompoundQuery>,
    pub _rparen: punct::RParen,
    pub alias: TableAlias,
}

/// Parenthesized join tree in FROM: `(t1 CROSS JOIN t2) AS alias`.
///
/// Distinguished from `SubqueryRef` by what the `(` contains: a subquery
/// starts with `SELECT` / `VALUES` / `TABLE` / `WITH` (all keywords),
/// whereas a parenthesized join tree starts with a table name (ident).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ParenJoinRef {
    pub _lparen: punct::LParen,
    pub table: Box<TableRef>,
    pub _rparen: punct::RParen,
    pub alias: Option<PlainTableAlias>,
}

/// LATERAL subquery in FROM: `LATERAL (VALUES(...)) v`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LateralRef {
    pub _lateral: PhantomData<keyword::Lateral>,
    pub _lparen: punct::LParen,
    pub query: Box<crate::ast::values::CompoundQuery>,
    pub _rparen: punct::RParen,
    pub alias: Option<literal::AliasName>,
}

/// Plain table reference with optional alias: `[ONLY] tablename [AS] alias`
///
/// `ONLY` means do not recurse into inheritance children (the opposite
/// of the `table *` `InheritedTable` form).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PlainTable {
    pub only: Option<PhantomData<keyword::Only>>,
    pub name: QualifiedName,
    pub alias: Option<PlainTableAlias>,
}

/// Alias of a plain table reference in FROM: `[AS] name [(col, col, ...)]`.
///
/// Unlike `TableAlias` (which is used for subqueries, function tables, etc.,
/// where an alias is mandatory), this one uses `literal::Ident` for the bare
/// form so that SQL keywords like `WHERE`, `ORDER`, `GROUP` are not swallowed
/// as the alias name when the alias is absent. The `WithAs` variant can still
/// use `literal::AliasName` because the `AS` keyword disambiguates.
///
/// Variant ordering: `WithAs` (starts with `AS`) must be listed before `Bare`
/// so longest-match-wins picks it when `AS` is present.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum PlainTableAlias {
    WithAs(PlainTableAliasWithAs),
    Bare(PlainTableAliasBare),
}

/// `AS name [(col, ...)]` form.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PlainTableAliasWithAs {
    pub _as: PhantomData<keyword::As>,
    pub name: literal::AliasName,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
}

/// Bare `name [(col, ...)]` form. Uses `literal::Ident` to reject keywords.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PlainTableAliasBare {
    pub name: literal::Ident,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
}

/// `WITH ORDINALITY` suffix on a function table.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithOrdinality {
    pub _with: PhantomData<keyword::With>,
    pub _ordinality: PhantomData<keyword::Ordinality>,
}

/// A column definition inside a function-table column-def-list:
/// `name type` (e.g., `a int`).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDef {
    pub name: literal::AliasName,
    pub type_name: crate::ast::expr::TypeName,
}

/// `[AS] alias (col type, ...)` or just `(col type, ...)` -- the
/// column definition list form for table-returning functions.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDefList {
    pub _as: Option<PhantomData<keyword::As>>,
    pub name: Option<literal::AliasName>,
    pub columns: Surrounded<punct::LParen, Seq<ColumnDef, punct::Comma>, punct::RParen>,
}

/// Alias of a function table reference: either a regular `TableAlias`
/// or a column-definition list form.
///
/// Variant ordering: `ColumnDefList` is more specific (its inner uses
/// `name type` pairs requiring at least one type token after each name)
/// so list it first.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncTableAlias {
    ColumnDefList(ColumnDefList),
    Plain(TableAlias),
}

/// Function call used as table reference with optional WITH ORDINALITY and alias.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncTableRef {
    pub func: FuncCall,
    pub ordinality: Option<WithOrdinality>,
    pub alias: Option<FuncTableAlias>,
}

/// A single table reference (no joins). Used as building block for JoinTableRef.
///
/// Variant ordering matters for disambiguation via longest-match-wins:
/// - Lateral before Func: both match `keyword(` pattern length, Lateral
///   wins via declaration order since LATERAL is a keyword (not an Ident).
/// - Func before Inherited/Table: FuncCall's `ident(` pattern is longer
///   than bare ident.
/// - Inherited before Table: `person*` matches longer than `person`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SimpleTableRef {
    Lateral(LateralRef),
    Func(Box<FuncTableRef>),
    // Subquery must come before ParenJoin: both start with `(`, but a
    // subquery body begins with a keyword (`SELECT`/`VALUES`/`TABLE`/`WITH`)
    // while a parenthesized join tree begins with an identifier. The parser
    // forks and tries in declaration order, so try the more restrictive
    // (keyword-leading) form first.
    Subquery(SubqueryRef),
    ParenJoin(ParenJoinRef),
    Inherited(InheritedTable),
    Table(PlainTable),
}

/// Join type: LEFT, RIGHT, FULL, INNER, CROSS, or plain JOIN.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinType {
    Left(keyword::Left),
    Right(keyword::Right),
    Full(keyword::Full),
    Inner(keyword::Inner),
    Cross(keyword::Cross),
}

/// JOIN condition: ON expr or USING (col, ...)
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinCondition {
    On(JoinOn),
    Using(JoinUsing),
}

/// ON condition for JOIN
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinOn {
    pub _on: PhantomData<keyword::On>,
    pub condition: Box<Expr>,
}

/// `AS alias` form of a JOIN USING alias.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinUsingAliasWithAs {
    pub _as: PhantomData<keyword::As>,
    pub name: literal::AliasName,
}

/// `[AS] alias` suffix on a JOIN ... USING column list.
///
/// Variant ordering: `WithAs` (`AS name`) is longer than `Bare`
/// (`ident`); list it first.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinUsingAlias {
    WithAs(JoinUsingAliasWithAs),
    Bare(literal::Ident),
}

/// USING clause for JOIN: `USING (col, ...) [[AS] alias]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinUsing {
    pub _using: PhantomData<keyword::Using>,
    pub columns: Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>,
    pub alias: Option<JoinUsingAlias>,
}

/// A single join suffix:
/// `[NATURAL] [LEFT|RIGHT|FULL|INNER|CROSS] [OUTER] JOIN table [ON expr | USING (...)]`.
///
/// `OUTER` is allowed (and traditionally written) after `LEFT`/`RIGHT`/`FULL`.
/// Postgres accepts but does not require it; the grammar accepts it after any
/// join type for simplicity.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinSuffix {
    pub natural: Option<PhantomData<keyword::Natural>>,
    pub join_type: Option<JoinType>,
    pub _outer: Option<PhantomData<keyword::Outer>>,
    pub _join: PhantomData<keyword::Join>,
    pub table: SimpleTableRef,
    pub condition: Option<JoinCondition>,
}

/// A table reference that may have zero or more JOIN suffixes.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableRef {
    pub base: SimpleTableRef,
    pub joins: Seq<JoinSuffix, (), OptionalTrailing>,
}

/// WHERE clause: `WHERE expr`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhereClause {
    pub _where: PhantomData<keyword::Where>,
    pub condition: Expr,
}

/// USING operator in ORDER BY: `USING > | USING <`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum UsingOp {
    Gt(punct::Gt),
    Lt(punct::Lt),
}

/// USING clause in ORDER BY: `USING op`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UsingClause {
    pub _using: PhantomData<keyword::Using>,
    pub op: UsingOp,
}

/// Sort direction: ASC or DESC.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SortDir {
    Asc(keyword::Asc),
    Desc(keyword::Desc),
}

/// NULLS FIRST or NULLS LAST.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NullsOrder {
    First(NullsFirst),
    Last(NullsLast),
}

/// NULLS FIRST
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NullsFirst(PhantomData<keyword::Nulls>, PhantomData<keyword::First>);

/// NULLS LAST
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NullsLast(PhantomData<keyword::Nulls>, PhantomData<keyword::Last>);

/// A single ORDER BY item: `expr [ASC|DESC] [USING op] [NULLS FIRST|LAST]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByItem {
    pub expr: Expr,
    pub dir: Option<SortDir>,
    pub using: Option<UsingClause>,
    pub nulls: Option<NullsOrder>,
}

/// ORDER BY clause: `ORDER BY item [, item ...]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByClause {
    pub _order: PhantomData<keyword::Order>,
    pub _by: PhantomData<keyword::By>,
    pub items: Seq<OrderByItem, punct::Comma>,
}

/// OFFSET clause: `OFFSET expr`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OffsetClause {
    pub _offset: PhantomData<keyword::Offset>,
    pub count: Expr,
}

/// LIMIT clause: `LIMIT expr`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LimitClause {
    pub _limit: PhantomData<keyword::Limit>,
    pub count: Expr,
}

/// FOR UPDATE clause.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ForUpdateClause {
    pub _for: PhantomData<keyword::For>,
    pub _update: PhantomData<keyword::Update>,
}

/// GROUP BY clause: `GROUP BY item, ...` where each item is an expression
/// or one of the grouping primitives (GROUPING SETS, ROLLUP, CUBE).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GroupByClause {
    pub _group: PhantomData<keyword::Group>,
    pub _by: PhantomData<keyword::By>,
    pub items: Seq<GroupByItem, punct::Comma>,
}

/// `GROUPING SETS ( item, ... )`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GroupingSetsItem {
    pub _grouping: PhantomData<keyword::GroupingKw>,
    pub _sets: PhantomData<keyword::SetsKw>,
    pub groups: Surrounded<punct::LParen, Seq<Box<GroupByItem>, punct::Comma>, punct::RParen>,
}

/// `ROLLUP ( item, ... )`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RollupItem {
    pub _rollup: PhantomData<keyword::RollupKw>,
    pub items: Surrounded<punct::LParen, Seq<Box<GroupByItem>, punct::Comma>, punct::RParen>,
}

/// `CUBE ( item, ... )`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CubeItem {
    pub _cube: PhantomData<keyword::CubeKw>,
    pub items: Surrounded<punct::LParen, Seq<Box<GroupByItem>, punct::Comma>, punct::RParen>,
}

/// A single element in a GROUP BY clause.
///
/// Variant ordering: two-keyword primitives first (`GROUPING SETS`), then
/// single-keyword primitives (`ROLLUP`, `CUBE`), then the catch-all `Expr`
/// which also handles `(a, b)` row-style groupings.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum GroupByItem {
    GroupingSets(GroupingSetsItem),
    Rollup(RollupItem),
    Cube(CubeItem),
    Expr(Expr),
}

/// HAVING clause: `HAVING expr`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct HavingClause {
    pub _having: PhantomData<keyword::Having>,
    pub condition: Expr,
}

/// A single named window definition: `name AS (inline_window_spec)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WindowDef {
    pub name: literal::Ident,
    pub _as: PhantomData<keyword::As>,
    pub spec: Surrounded<
        punct::LParen,
        crate::ast::expr::InlineWindowSpec,
        punct::RParen,
    >,
}

/// `WINDOW name AS (...)[, name AS (...), ...]` clause in SELECT.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WindowClause {
    pub _window: PhantomData<keyword::Window>,
    pub defs: Seq<WindowDef, punct::Comma, NoTrailing, NonEmpty>,
}

/// SELECT statement.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[format_tokens(group(consistent))]
pub struct SelectStmt {
    pub _select: PhantomData<keyword::Select>,
    pub distinct: Option<PhantomData<keyword::Distinct>>,
    #[format_tokens(group(consistent), indent, break(flat = " ", broken = "\n"))]
    pub items: Seq<SelectItem, punct::Comma>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub from_clause: Option<FromClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub where_clause: Option<WhereClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub group_by: Option<GroupByClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub having: Option<HavingClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub window: Option<WindowClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub order_by: Option<OrderByClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub limit: Option<LimitClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub offset: Option<OffsetClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub for_update: Option<ForUpdateClause>,
}

/// A SELECT body that can appear in subqueries -- WITH, SELECT, or VALUES.
/// WithBody must come before Select so `WITH ... SELECT` matches before bare `SELECT`.
/// SelectStmt must come before ValuesStmt so `SELECT` keyword wins over ambiguity.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SelectBody {
    WithBody(Box<crate::ast::with_clause::WithStatement>),
    Select(Box<SelectStmt>),
    Values(ValuesBody),
}

/// VALUES body: `VALUES (expr, ...), (expr, ...)`
/// Can appear standalone or inside subqueries.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ValuesBody {
    pub _values: PhantomData<keyword::Values>,
    pub rows: Seq<Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>, punct::Comma>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::select::SelectStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_simple_select() {
        let mut input = Input::new("SELECT 1 AS one");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_paren_join_cross() {
        let mut input = Input::new("SELECT * FROM (a CROSS JOIN b) AS tx");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_paren_join_using() {
        let mut input = Input::new("SELECT * FROM (a JOIN b USING (i)) AS x");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_paren_join_with_col_aliases() {
        let mut input = Input::new(
            "SELECT * FROM (a t1 (x, y) CROSS JOIN b t2 (p, q)) AS tx (a, b, c, d)",
        );
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_from_where() {
        let mut input = Input::new("SELECT f1 FROM BOOLTBL1 WHERE f1 = true");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(stmt.from_clause.is_some());
        assert!(stmt.where_clause.is_some());
    }

    #[test]
    fn parse_select_star() {
        let mut input = Input::new("SELECT * FROM t");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.items.len(), 1);
    }

    #[test]
    fn parse_select_with_alias_keyword() {
        let mut input = Input::new("SELECT 1 AS true");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        let alias = stmt.items[0].alias.as_ref().unwrap();
        assert_eq!(alias.name(), "true");
    }

    #[test]
    fn parse_select_order_by() {
        let mut input = Input::new("SELECT f1 FROM t ORDER BY f1");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.order_by.is_some());
    }

    #[test]
    fn parse_select_from_function() {
        let mut input = Input::new("SELECT * FROM pg_input_error_info('junk', 'bool')");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.from_clause.is_some());
    }

    // --- ORDER BY enhancements ---

    #[test]
    fn parse_order_by_using() {
        let mut input = Input::new("SELECT f1 FROM t ORDER BY f1 using >");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_asc() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 ASC");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_desc() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 DESC");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_nulls_first() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 NULLS FIRST");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_desc_nulls_last() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 DESC NULLS LAST");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    // --- OFFSET/LIMIT ---

    #[test]
    fn parse_select_offset() {
        let mut input = Input::new("SELECT 1 OFFSET 0");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.offset.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_limit() {
        let mut input = Input::new("SELECT 1 LIMIT 1");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.limit.is_some());
        assert!(input.is_empty());
    }

    // --- FOR UPDATE ---

    #[test]
    fn parse_select_from_only() {
        let mut input = Input::new("SELECT f1 FROM ONLY t");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_from_only_with_alias() {
        let mut input = Input::new("SELECT f1 FROM ONLY t AS x");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_from_qualified_name() {
        let mut input = Input::new("SELECT * FROM myschema.mytable");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_window_clause_standalone() {
        use super::WindowClause;
        let mut input = Input::new("WINDOW w AS (PARTITION BY y ORDER BY z)");
        let wc = WindowClause::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(wc.defs.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_window_clause() {
        let mut input = Input::new(
            "SELECT sum(x) OVER w FROM t WINDOW w AS (PARTITION BY y ORDER BY z)",
        );
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.window.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_frame_rows_between() {
        let mut input = Input::new(
            "SELECT sum(x) OVER (ORDER BY y ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM t",
        );
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_over_named() {
        let mut input = Input::new("SELECT sum(x) OVER w FROM t");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_from_alias_with_column_list() {
        let mut input = Input::new("SELECT * FROM tbl AS t (a, b, c)");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_from_qualified_name_with_alias() {
        let mut input = Input::new("SELECT * FROM s.t AS x");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_join_using_alias() {
        let mut input = Input::new("SELECT * FROM a JOIN b USING (i) AS x");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_join_using_alias_where() {
        let mut input = Input::new("SELECT * FROM a JOIN b USING (i) AS x WHERE x.i = 1");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_func_with_ordinality() {
        let mut input =
            Input::new("SELECT * FROM rngfunct(1) WITH ORDINALITY AS z(a, b, ord)");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_func_column_def_list() {
        let mut input = Input::new(
            "SELECT * FROM test_ret_set_rec_dyn(1500) AS (a int, b int, c int)",
        );
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_func_with_ordinality_unnest() {
        let mut input =
            Input::new("SELECT * FROM unnest(array['a','b']) WITH ORDINALITY AS z(a, ord)");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_natural_join() {
        let mut input = Input::new("SELECT * FROM a NATURAL JOIN b");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_natural_left_join() {
        let mut input = Input::new("SELECT * FROM a NATURAL LEFT JOIN b");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_left_outer_join_using() {
        let mut input = Input::new("SELECT * FROM a LEFT OUTER JOIN b USING (i)");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_full_outer_join_using() {
        let mut input = Input::new("SELECT * FROM a FULL OUTER JOIN b USING (i)");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_right_outer_join_on() {
        let mut input = Input::new("SELECT * FROM a RIGHT OUTER JOIN b ON a.i = b.i");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_paren_join_simple() {
        let mut input =
            Input::new("SELECT * FROM a LEFT JOIN (b JOIN c ON b.x = c.x) ON a.y = b.y");
        SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_paren_join_with_subquery_inside() {
        let mut input = Input::new(
            "SELECT * FROM a LEFT JOIN (b JOIN (SELECT 1 AS x) s ON b.x = s.x) ON a.y = b.y",
        );
        SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_paren_join_leading_subquery() {
        let mut input = Input::new(
            "SELECT * FROM a LEFT JOIN ((SELECT * FROM b) s LEFT JOIN c ON s.x = c.x) ON a.y = s.y",
        );
        SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_group_by_grouping_sets_simple() {
        let mut input =
            Input::new("SELECT sum(c) FROM t GROUP BY GROUPING SETS ((), (a), (a,b))");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.group_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_group_by_rollup() {
        let mut input = Input::new("SELECT sum(c) FROM t GROUP BY ROLLUP (a, b)");
        SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_group_by_cube() {
        let mut input = Input::new("SELECT sum(c) FROM t GROUP BY CUBE (a, b)");
        SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_group_by_grouping_sets_nested() {
        let mut input =
            Input::new("SELECT sum(c) FROM t GROUP BY GROUPING SETS (ROLLUP(a), CUBE(b))");
        SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_group_by_mixed_primitives() {
        let mut input = Input::new("SELECT sum(c) FROM t GROUP BY a, ROLLUP(b), CUBE(c)");
        SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_for_update() {
        let mut input = Input::new("SELECT f1 FROM t FOR UPDATE");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.for_update.is_some());
        assert!(input.is_empty());
    }
}
