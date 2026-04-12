/// SELECT statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::delete::TableAlias as IdentAlias;
use crate::ast::expr::{Expr, FuncCall};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// A single item in the SELECT list: `expr [AS alias]` or `expr alias`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<Alias>,
}

/// Alias with explicit AS keyword: `AS name`.
/// Uses AliasName so keywords are accepted (e.g., `SELECT 1 AS true`).
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsAlias {
    pub _as: PhantomData<keyword::As>,
    pub name: literal::AliasName,
}

/// AS alias clause, or bare alias.
///
/// Variant ordering: WithAs (`AS name`) has a longer first_pattern than
/// Bare (`ident`), so longest-match-wins picks it when AS is present.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum Alias {
    WithAs(AsAlias),
    Bare(literal::Ident),
}

impl Alias {
    /// Returns the alias name regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            Alias::WithAs(a) => &a.name.0,
            Alias::Bare(ident) => &ident.0,
        }
    }
}

/// FROM clause: `FROM table [, table ...]`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FromClause {
    pub _from: PhantomData<keyword::From>,
    pub tables: Seq<TableRef, punct::Comma>,
}

/// Table name with inheritance marker and optional alias: `person* p`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InheritedTable {
    pub name: literal::Ident,
    pub _star: punct::Star,
    pub alias: Option<literal::Ident>,
}

/// Table alias: `AS name [(col1, col2)]` or bare `name [(col1, col2)]`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableAlias {
    pub _as: Option<PhantomData<keyword::As>>,
    pub name: literal::AliasName,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
}

/// Subquery in FROM: `(SELECT ...) AS alias`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubqueryRef {
    pub _lparen: punct::LParen,
    pub query: Box<crate::ast::values::CompoundQuery>,
    pub _rparen: punct::RParen,
    pub alias: TableAlias,
}

/// LATERAL subquery in FROM: `LATERAL (VALUES(...)) v`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LateralRef {
    pub _lateral: PhantomData<keyword::Lateral>,
    pub _lparen: punct::LParen,
    pub query: Box<crate::ast::values::CompoundQuery>,
    pub _rparen: punct::RParen,
    pub alias: Option<literal::AliasName>,
}

/// Plain table reference with optional alias: `tablename [AS] alias`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PlainTable {
    pub name: literal::Ident,
    pub alias: Option<IdentAlias>,
}

/// Function call used as table reference with optional alias.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncTableRef {
    pub func: FuncCall,
    pub alias: Option<TableAlias>,
}

/// A single table reference (no joins). Used as building block for JoinTableRef.
///
/// Variant ordering matters for disambiguation via longest-match-wins:
/// - Lateral before Func: both match `keyword(` pattern length, Lateral
///   wins via declaration order since LATERAL is a keyword (not an Ident).
/// - Func before Inherited/Table: FuncCall's `ident(` pattern is longer
///   than bare ident.
/// - Inherited before Table: `person*` matches longer than `person`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SimpleTableRef {
    Lateral(LateralRef),
    Func(FuncTableRef),
    Subquery(SubqueryRef),
    Inherited(InheritedTable),
    Table(PlainTable),
}

/// Join type: LEFT, RIGHT, FULL, INNER, CROSS, or plain JOIN.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinType {
    Left(keyword::Left),
    Right(keyword::Right),
    Full(keyword::Full),
    Inner(keyword::Inner),
    Cross(keyword::Cross),
}

/// JOIN condition: ON expr or USING (col, ...)
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum JoinCondition {
    On(JoinOn),
    Using(JoinUsing),
}

/// ON condition for JOIN
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinOn {
    pub _on: PhantomData<keyword::On>,
    pub condition: Expr,
}

/// USING clause for JOIN: `USING (col, ...)`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinUsing {
    pub _using: PhantomData<keyword::Using>,
    pub columns: Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>,
}

/// A single join suffix: `[LEFT|RIGHT|FULL|INNER|CROSS] JOIN table [ON expr | USING (...)]`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct JoinSuffix {
    pub join_type: Option<JoinType>,
    pub _join: PhantomData<keyword::Join>,
    pub table: SimpleTableRef,
    pub condition: Option<JoinCondition>,
}

/// A table reference that may have zero or more JOIN suffixes.
///
/// Manual Parse impl needed because JOINs are left-associative and we need
/// to parse a sequence: `table JOIN table ON ... LEFT JOIN table ON ...`
/// To eliminate this, recursa would need postfix-style sequence parsing for
/// non-Pratt types.
#[derive(Debug, Clone, Visit)]
pub struct TableRef {
    pub base: SimpleTableRef,
    pub joins: Vec<JoinSuffix>,
}

impl<'input> Parse<'input> for TableRef {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        SimpleTableRef::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        SimpleTableRef::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let base = SimpleTableRef::parse(input, rules)?;
        let mut joins = Vec::new();
        loop {
            R::consume_ignored(input);
            // Check for JOIN keyword or join type keyword (LEFT, RIGHT, FULL, INNER, CROSS)
            if keyword::Join::peek(input, rules)
                || keyword::Left::peek(input, rules)
                || keyword::Right::peek(input, rules)
                || keyword::Full::peek(input, rules)
                || keyword::Inner::peek(input, rules)
                || keyword::Cross::peek(input, rules)
            {
                let join = JoinSuffix::parse(input, rules)?;
                joins.push(join);
            } else {
                break;
            }
        }
        Ok(TableRef { base, joins })
    }
}

/// WHERE clause: `WHERE expr`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhereClause {
    pub _where: PhantomData<keyword::Where>,
    pub condition: Expr,
}

/// USING operator in ORDER BY: `USING > | USING <`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum UsingOp {
    Gt(punct::Gt),
    Lt(punct::Lt),
}

/// USING clause in ORDER BY: `USING op`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UsingClause {
    pub _using: PhantomData<keyword::Using>,
    pub op: UsingOp,
}

/// Sort direction: ASC or DESC.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SortDir {
    Asc(keyword::Asc),
    Desc(keyword::Desc),
}

/// NULLS FIRST or NULLS LAST.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NullsOrder {
    First(NullsFirst),
    Last(NullsLast),
}

/// NULLS FIRST
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NullsFirst(PhantomData<keyword::Nulls>, PhantomData<keyword::First>);

/// NULLS LAST
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NullsLast(PhantomData<keyword::Nulls>, PhantomData<keyword::Last>);

/// A single ORDER BY item: `expr [ASC|DESC] [USING op] [NULLS FIRST|LAST]`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByItem {
    pub expr: Expr,
    pub dir: Option<SortDir>,
    pub using: Option<UsingClause>,
    pub nulls: Option<NullsOrder>,
}

/// ORDER BY clause: `ORDER BY item [, item ...]`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByClause {
    pub _order: PhantomData<keyword::Order>,
    pub _by: PhantomData<keyword::By>,
    pub items: Seq<OrderByItem, punct::Comma>,
}

/// OFFSET clause: `OFFSET expr`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OffsetClause {
    pub _offset: PhantomData<keyword::Offset>,
    pub count: Expr,
}

/// LIMIT clause: `LIMIT expr`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LimitClause {
    pub _limit: PhantomData<keyword::Limit>,
    pub count: Expr,
}

/// FOR UPDATE clause.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ForUpdateClause {
    pub _for: PhantomData<keyword::For>,
    pub _update: PhantomData<keyword::Update>,
}

/// GROUP BY clause: `GROUP BY expr, ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GroupByClause {
    pub _group: PhantomData<keyword::Group>,
    pub _by: PhantomData<keyword::By>,
    pub exprs: Seq<Expr, punct::Comma>,
}

/// HAVING clause: `HAVING expr`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct HavingClause {
    pub _having: PhantomData<keyword::Having>,
    pub condition: Expr,
}

/// SELECT statement.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectStmt {
    pub _select: PhantomData<keyword::Select>,
    pub distinct: Option<PhantomData<keyword::Distinct>>,
    pub items: Seq<SelectItem, punct::Comma>,
    pub from_clause: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub group_by: Option<GroupByClause>,
    pub having: Option<HavingClause>,
    pub order_by: Option<OrderByClause>,
    pub limit: Option<LimitClause>,
    pub offset: Option<OffsetClause>,
    pub for_update: Option<ForUpdateClause>,
}

/// A SELECT body that can appear in subqueries -- WITH, SELECT, or VALUES.
/// WithBody must come before Select so `WITH ... SELECT` matches before bare `SELECT`.
/// SelectStmt must come before ValuesStmt so `SELECT` keyword wins over ambiguity.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SelectBody {
    WithBody(Box<crate::ast::with_clause::WithStatement>),
    Select(Box<SelectStmt>),
    Values(ValuesBody),
}

/// VALUES body: `VALUES (expr, ...), (expr, ...)`
/// Can appear standalone or inside subqueries.
#[derive(Debug, Clone, Parse, Visit)]
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
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_from_where() {
        let mut input = Input::new("SELECT f1 FROM BOOLTBL1 WHERE f1 = true");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(stmt.from_clause.is_some());
        assert!(stmt.where_clause.is_some());
    }

    #[test]
    fn parse_select_star() {
        let mut input = Input::new("SELECT * FROM t");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
    }

    #[test]
    fn parse_select_with_alias_keyword() {
        let mut input = Input::new("SELECT 1 AS true");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        let alias = stmt.items[0].alias.as_ref().unwrap();
        assert_eq!(alias.name(), "true");
    }

    #[test]
    fn parse_select_order_by() {
        let mut input = Input::new("SELECT f1 FROM t ORDER BY f1");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.order_by.is_some());
    }

    #[test]
    fn parse_select_from_function() {
        let mut input = Input::new("SELECT * FROM pg_input_error_info('junk', 'bool')");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.from_clause.is_some());
    }

    // --- ORDER BY enhancements ---

    #[test]
    fn parse_order_by_using() {
        let mut input = Input::new("SELECT f1 FROM t ORDER BY f1 using >");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_asc() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 ASC");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_desc() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 DESC");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_nulls_first() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 NULLS FIRST");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_order_by_desc_nulls_last() {
        let mut input = Input::new("SELECT * FROM t ORDER BY f1 DESC NULLS LAST");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.order_by.is_some());
        assert!(input.is_empty());
    }

    // --- OFFSET/LIMIT ---

    #[test]
    fn parse_select_offset() {
        let mut input = Input::new("SELECT 1 OFFSET 0");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.offset.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_limit() {
        let mut input = Input::new("SELECT 1 LIMIT 1");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.limit.is_some());
        assert!(input.is_empty());
    }

    // --- FOR UPDATE ---

    #[test]
    fn parse_select_for_update() {
        let mut input = Input::new("SELECT f1 FROM t FOR UPDATE");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.for_update.is_some());
        assert!(input.is_empty());
    }
}
