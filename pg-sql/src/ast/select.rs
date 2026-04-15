/// SELECT statement AST.
use recursa::seq::{NoTrailing, NonEmpty, OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::common::QualifiedName;
use crate::ast::expr::{Expr, FuncCall};
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};

use crate::tokens::keyword::*;
/// A single item in the SELECT list: `expr [AS alias]` or `expr alias`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectItem<'input> {
    pub expr: Expr<'input>,
    pub alias: Option<Alias<'input>>,
}

/// Alias with explicit AS keyword: `AS name`.
/// Uses AliasName so keywords are accepted (e.g., `SELECT 1 AS true`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsAlias<'input> {
    pub r#as: AS,
    pub name: literal::AliasName<'input>,
}

/// AS alias clause, or bare alias.
///
/// Variant ordering: WithAs (`AS name`) has a longer first_pattern than
/// Bare (`ident`), so longest-match-wins picks it when AS is present.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum Alias<'input> {
    WithAs(AsAlias<'input>),
    Bare(literal::Ident<'input>),
}

impl<'input> Alias<'input> {
    /// Returns the alias name regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            Alias::WithAs(a) => a.name.text(),
            Alias::Bare(ident) => ident.text(),
        }
    }
}

/// FROM clause: `FROM table [, table ...]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FromClause<'input> {
    pub from: FROM,
    pub tables: Seq<TableRef<'input>, punct::Comma>,
}

/// Table name with inheritance marker and optional alias: `person* p`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InheritedTable<'input> {
    pub name: QualifiedName<'input>,
    pub star: punct::Star,
    pub alias: Option<literal::Ident<'input>>,
}

/// `AS name [(col1, col2)]` table alias form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableAliasWithAs<'input> {
    pub r#as: AS,
    pub name: literal::AliasName<'input>,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>>,
}

/// Bare `name [(col1, col2)]` table alias form. Bare alias names must not
/// be reserved keywords (uses `Ident`, not `AliasName`), otherwise clauses
/// like `FROM unnest(a) ORDER BY 1` would consume `ORDER` as an alias.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableAliasBare<'input> {
    pub name: literal::Ident<'input>,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>>,
}

/// Table alias: `AS name [(col1, col2)]` or bare `name [(col1, col2)]`.
///
/// Variant ordering: `WithAs` (`AS`) before `Bare` (ident).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TableAlias<'input> {
    WithAs(TableAliasWithAs<'input>),
    Bare(TableAliasBare<'input>),
}

/// Subquery in FROM: `(SELECT ...) AS alias`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubqueryRef<'input> {
    pub lparen: punct::LParen,
    pub query: Box<crate::ast::values::CompoundQuery<'input>>,
    pub rparen: punct::RParen,
    pub alias: Option<TableAlias<'input>>,
}

/// Parenthesized join tree in FROM: `(t1 CROSS JOIN t2) AS alias`.
///
/// Distinguished from `SubqueryRef` by what the `(` contains: a subquery
/// starts with `SELECT` / `VALUES` / `TABLE` / `WITH` (all keywords),
/// whereas a parenthesized join tree starts with a table name (ident).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ParenJoinRef<'input> {
    pub lparen: punct::LParen,
    pub table: Box<TableRef<'input>>,
    pub rparen: punct::RParen,
    pub alias: Option<PlainTableAlias<'input>>,
}

/// LATERAL subquery in FROM: `LATERAL (VALUES(...)) v`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LateralRef<'input> {
    pub lateral: LATERAL,
    pub lparen: punct::LParen,
    pub query: Box<crate::ast::values::CompoundQuery<'input>>,
    pub rparen: punct::RParen,
    pub alias: Option<literal::AliasName<'input>>,
}

/// Plain table reference with optional alias: `[ONLY] tablename [AS] alias`
///
/// `ONLY` means do not recurse into inheritance children (the opposite
/// of the `table *` `InheritedTable` form).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PlainTable<'input> {
    pub only: Option<ONLY>,
    pub name: QualifiedName<'input>,
    pub alias: Option<PlainTableAlias<'input>>,
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
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum PlainTableAlias<'input> {
    WithAs(PlainTableAliasWithAs<'input>),
    Bare(PlainTableAliasBare<'input>),
}

/// `AS name [(col, ...)]` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PlainTableAliasWithAs<'input> {
    pub r#as: AS,
    pub name: literal::AliasName<'input>,
    pub columns: Option<
        Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>,
    >,
}

/// Bare `name [(col, ...)]` form. Uses `literal::Ident` to reject keywords.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PlainTableAliasBare<'input> {
    pub name: literal::Ident<'input>,
    pub columns: Option<
        Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>,
    >,
}


/// A column definition inside a function-table column-def-list:
/// `name type` (e.g., `a int`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDef<'input> {
    pub name: literal::AliasName<'input>,
    pub type_name: crate::ast::expr::TypeName<'input>,
}

/// `[AS] alias (col type, ...)` or just `(col type, ...)` -- the
/// column definition list form for table-returning functions.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDefList<'input> {
    pub r#as: Option<AS>,
    pub name: Option<literal::AliasName<'input>>,
    pub columns: Surrounded<punct::LParen, Seq<ColumnDef<'input>, punct::Comma>, punct::RParen>,
}

/// Alias of a function table reference: either a regular `TableAlias`
/// or a column-definition list form.
///
/// Variant ordering: `ColumnDefList` is more specific (its inner uses
/// `name type` pairs requiring at least one type token after each name)
/// so list it first.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncTableAlias<'input> {
    ColumnDefList(ColumnDefList<'input>),
    Plain(TableAlias<'input>),
}

/// Function call used as table reference with optional WITH ORDINALITY and alias.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncTableRef<'input> {
    pub func: FuncCall<'input>,
    pub ordinality: Option<(WITH, ORDINALITY)>,
    pub alias: Option<FuncTableAlias<'input>>,
}

/// A single table reference (no joins). Used as building block for JoinTableRef.
///
/// Variant ordering matters for disambiguation via longest-match-wins:
/// - Lateral before Func: both match `keyword(` pattern length, Lateral
///   wins via declaration order since LATERAL is a keyword (not an Ident).
/// - Func before Inherited/Table: FuncCall's `ident(` pattern is longer
///   than bare ident.
/// - Inherited before Table: `person*` matches longer than `person`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SimpleTableRef<'input> {
    Lateral(LateralRef<'input>),
    Func(Box<FuncTableRef<'input>>),
    // Subquery must come before ParenJoin: both start with `(`, but a
    // subquery body begins with a keyword (`SELECT`/`VALUES`/`TABLE`/`WITH`)
    // while a parenthesized join tree begins with an identifier. The parser
    // forks and tries in declaration order, so try the more restrictive
    // (keyword-leading) form first.
    Subquery(SubqueryRef<'input>),
    ParenJoin(ParenJoinRef<'input>),
    Inherited(InheritedTable<'input>),
    Table(PlainTable<'input>),
}

/// Join type: LEFT, RIGHT, FULL, INNER, CROSS, or plain JOIN.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinType {
    Left(LEFT),
    Right(RIGHT),
    Full(FULL),
    Inner(INNER),
    Cross(CROSS),
}

/// JOIN condition: ON expr or USING (col, ...)
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinCondition<'input> {
    On(JoinOn<'input>),
    Using(JoinUsing<'input>),
}

/// ON condition for JOIN
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinOn<'input> {
    pub on: ON,
    pub condition: Box<Expr<'input>>,
}

/// `AS alias` form of a JOIN USING alias.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinUsingAliasWithAs<'input> {
    pub r#as: AS,
    pub name: literal::AliasName<'input>,
}

/// `[AS] alias` suffix on a JOIN ... USING column list.
///
/// Variant ordering: `WithAs` (`AS name`) is longer than `Bare`
/// (`ident`); list it first.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinUsingAlias<'input> {
    WithAs(JoinUsingAliasWithAs<'input>),
    Bare(literal::Ident<'input>),
}

/// USING clause for JOIN: `USING (col, ...) [[AS] alias]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinUsing<'input> {
    pub using: USING,
    pub columns: Surrounded<
        punct::LParen,
        Seq<literal::AliasName<'input>, punct::Comma>,
        punct::RParen,
    >,
    pub alias: Option<JoinUsingAlias<'input>>,
}

/// A single join suffix:
/// `[NATURAL] [LEFT|RIGHT|FULL|INNER|CROSS] [OUTER] JOIN table [ON expr | USING (...)]`.
///
/// `OUTER` is allowed (and traditionally written) after `LEFT`/`RIGHT`/`FULL`.
/// Postgres accepts but does not require it; the grammar accepts it after any
/// join type for simplicity.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinSuffix<'input> {
    pub natural: Option<NATURAL>,
    pub join_type: Option<JoinType>,
    pub outer: Option<OUTER>,
    pub join: JOIN,
    pub table: SimpleTableRef<'input>,
    pub condition: Option<JoinCondition<'input>>,
}

/// A table reference that may have zero or more JOIN suffixes.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableRef<'input> {
    pub base: SimpleTableRef<'input>,
    pub joins: Seq<JoinSuffix<'input>, (), OptionalTrailing>,
}

/// WHERE clause: `WHERE expr`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhereClause<'input> {
    pub r#where: WHERE,
    pub condition: Expr<'input>,
}

/// USING operator in ORDER BY: `USING > | USING <`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum UsingOp {
    Gt(punct::Gt),
    Lt(punct::Lt),
}

/// USING clause in ORDER BY: `USING op`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UsingClause {
    pub using: USING,
    pub op: UsingOp,
}

/// Sort direction: ASC or DESC.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SortDir {
    Asc(ASC),
    Desc(DESC),
}

/// NULLS FIRST or NULLS LAST.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NullsOrder {
    First((NULLS, FIRST)),
    Last((NULLS, LAST)),
}

/// A single ORDER BY item: `expr [ASC|DESC] [USING op] [NULLS FIRST|LAST]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByItem<'input> {
    pub expr: Expr<'input>,
    pub dir: Option<SortDir>,
    pub using: Option<UsingClause>,
    pub nulls: Option<NullsOrder>,
}

/// ORDER BY clause: `ORDER BY item [, item ...]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByClause<'input> {
    pub order: ORDER,
    pub by: BY,
    pub items: Seq<OrderByItem<'input>, punct::Comma>,
}

/// OFFSET clause: `OFFSET expr`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OffsetClause<'input> {
    pub offset: OFFSET,
    pub count: Expr<'input>,
}

/// LIMIT clause: `LIMIT expr`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LimitClause<'input> {
    pub limit: LIMIT,
    pub count: Expr<'input>,
}

/// FOR UPDATE / FOR SHARE / FOR NO KEY UPDATE / FOR KEY SHARE locking clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ForUpdateClause {
    pub r#for: FOR,
    pub mode: LockingMode,
}

/// Lock strength for `SELECT ... FOR ...` locking clauses.
///
/// Variant ordering: longer (`NO KEY UPDATE`, `KEY SHARE`) before shorter
/// (`UPDATE`, `SHARE`) so longest-match wins.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum LockingMode {
    NoKeyUpdate((NO, KEY, UPDATE)),
    KeyShare((KEY, SHARE)),
    Update(UPDATE),
    Share(SHARE),
}

/// GROUP BY clause: `GROUP BY item, ...` where each item is an expression
/// or one of the grouping primitives (GROUPING SETS, ROLLUP, CUBE).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GroupByClause<'input> {
    pub group: GROUP,
    pub by: BY,
    /// Optional `DISTINCT` / `ALL` modifier (Postgres 16+).
    pub modifier: Option<GroupByModifier>,
    pub items: Seq<GroupByItem<'input>, punct::Comma>,
}

/// `GROUP BY [DISTINCT|ALL]` modifier.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum GroupByModifier {
    Distinct(DISTINCT),
    All(ALL),
}

/// `GROUPING SETS ( item, ... )`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GroupingSetsItem<'input> {
    pub grouping: GROUPING,
    pub sets: SETS,
    pub groups:
        Surrounded<punct::LParen, Seq<Box<GroupByItem<'input>>, punct::Comma>, punct::RParen>,
}

/// `ROLLUP ( item, ... )`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RollupItem<'input> {
    pub rollup: ROLLUP,
    pub items:
        Surrounded<punct::LParen, Seq<Box<GroupByItem<'input>>, punct::Comma>, punct::RParen>,
}

/// `CUBE ( item, ... )`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CubeItem<'input> {
    pub cube: CUBE,
    pub items:
        Surrounded<punct::LParen, Seq<Box<GroupByItem<'input>>, punct::Comma>, punct::RParen>,
}

/// A single element in a GROUP BY clause.
///
/// Variant ordering: two-keyword primitives first (`GROUPING SETS`), then
/// single-keyword primitives (`ROLLUP`, `CUBE`), then the catch-all `Expr`
/// which also handles `(a, b)` row-style groupings.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum GroupByItem<'input> {
    GroupingSets(GroupingSetsItem<'input>),
    Rollup(RollupItem<'input>),
    Cube(CubeItem<'input>),
    Expr(Box<Expr<'input>>),
}

/// HAVING clause: `HAVING expr`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct HavingClause<'input> {
    pub having: HAVING,
    pub condition: Expr<'input>,
}

/// A single named window definition: `name AS (inline_window_spec)`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WindowDef<'input> {
    pub name: literal::Ident<'input>,
    pub r#as: AS,
    pub spec: Surrounded<
        punct::LParen,
        crate::ast::expr::InlineWindowSpec<'input>,
        punct::RParen,
    >,
}

/// `WINDOW name AS (...)[, name AS (...), ...]` clause in SELECT.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WindowClause<'input> {
    pub window: WINDOW,
    pub defs: Seq<WindowDef<'input>, punct::Comma, NoTrailing, NonEmpty>,
}

/// `INTO [TEMP|TEMPORARY|UNLOGGED] [TABLE] target` clause for the
/// Postgres `SELECT ... INTO new_table` statement form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectIntoClause<'input> {
    pub into: INTO,
    pub temp: Option<crate::ast::create_table::TempKw>,
    pub unlogged: Option<UNLOGGED>,
    pub table: Option<TABLE>,
    pub target: crate::ast::common::QualifiedName<'input>,
}

/// `DISTINCT` or `DISTINCT ON (exprs)` qualifier on a SELECT.
///
/// Variant ordering: `On` (longer, starts with `DISTINCT ON`) before `All`
/// (just `DISTINCT`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SelectDistinct<'input> {
    On(SelectDistinctOn<'input>),
    All(DISTINCT),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectDistinctOn<'input> {
    pub distinct: DISTINCT,
    pub on: ON,
    pub exprs: Surrounded<
        punct::LParen,
        Seq<crate::ast::expr::Expr<'input>, punct::Comma>,
        punct::RParen,
    >,
}

/// SELECT statement.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[format_tokens(group(consistent))]
pub struct SelectStmt<'input> {
    pub select: SELECT,
    pub distinct: Option<Box<SelectDistinct<'input>>>,
    #[format_tokens(group(consistent), indent, break(flat = " ", broken = "\n"))]
    pub items: Seq<SelectItem<'input>, punct::Comma>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub into: Option<Box<SelectIntoClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub from_clause: Option<Box<FromClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub where_clause: Option<Box<WhereClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub group_by: Option<Box<GroupByClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub having: Option<Box<HavingClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub window: Option<Box<WindowClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub order_by: Option<Box<OrderByClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub limit: Option<Box<LimitClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub offset: Option<Box<OffsetClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub for_update: Option<Box<ForUpdateClause>>,
}

/// A SELECT body that can appear in subqueries -- WITH, SELECT, or VALUES.
/// WithBody must come before Select so `WITH ... SELECT` matches before bare `SELECT`.
/// SelectStmt must come before ValuesStmt so `SELECT` keyword wins over ambiguity.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SelectBody<'input> {
    WithBody(Box<crate::ast::with_clause::WithStatement<'input>>),
    Select(Box<SelectStmt<'input>>),
    Values(ValuesBody<'input>),
}

/// VALUES body: `VALUES (expr, ...), (expr, ...)`
/// Can appear standalone or inside subqueries.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ValuesBody<'input> {
    pub values: VALUES,
    pub rows: Seq<
        Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
        punct::Comma,
    >,
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
        let mut input =
            Input::new("SELECT * FROM (a t1 (x, y) CROSS JOIN b t2 (p, q)) AS tx (a, b, c, d)");
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
        let mut input =
            Input::new("SELECT sum(x) OVER w FROM t WINDOW w AS (PARTITION BY y ORDER BY z)");
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
        let mut input = Input::new("SELECT * FROM rngfunct(1) WITH ORDINALITY AS z(a, b, ord)");
        let _stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_func_column_def_list() {
        let mut input =
            Input::new("SELECT * FROM test_ret_set_rec_dyn(1500) AS (a int, b int, c int)");
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
        let mut input = Input::new("SELECT sum(c) FROM t GROUP BY GROUPING SETS ((), (a), (a,b))");
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
    fn parse_group_by_distinct_modifier() {
        // Regression: groupingsets.sql uses `GROUP BY DISTINCT ROLLUP(a, b), ROLLUP(a, c)`
        // and `GROUP BY ALL ROLLUP(a, b), ROLLUP(a, c)`.
        for src in [
            "SELECT a FROM t GROUP BY DISTINCT ROLLUP(a, b), ROLLUP(a, c)",
            "SELECT a FROM t GROUP BY ALL ROLLUP(a, b), ROLLUP(a, c)",
        ] {
            let mut input = Input::new(src);
            SelectStmt::parse::<SqlRules>(&mut input).unwrap();
            assert!(input.is_empty(), "leftover for {src:?}");
        }
    }

    #[test]
    fn parse_select_for_update() {
        let mut input = Input::new("SELECT f1 FROM t FOR UPDATE");
        let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.for_update.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_locking_variants() {
        // Regression: matview.sql uses `FOR SHARE`. Postgres also supports
        // `FOR NO KEY UPDATE` and `FOR KEY SHARE`.
        for src in [
            "SELECT * FROM t FOR SHARE",
            "SELECT * FROM t FOR NO KEY UPDATE",
            "SELECT * FROM t FOR KEY SHARE",
        ] {
            let mut input = Input::new(src);
            let stmt = SelectStmt::parse::<SqlRules>(&mut input).unwrap();
            assert!(stmt.for_update.is_some(), "no locking clause: {src:?}");
            assert!(input.is_empty(), "leftover for {src:?}");
        }
    }
}
