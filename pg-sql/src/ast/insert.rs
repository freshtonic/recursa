/// INSERT INTO statement AST.
///
/// Supports: `INSERT INTO table [(cols)] source [ON CONFLICT ...] [RETURNING ...]`
/// where source is DEFAULT VALUES, VALUES rows, or SELECT query.
use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::common::QualifiedName;
use crate::ast::expr::Expr;
use crate::ast::select::WhereClause;
use crate::ast::update::{ReturningClause, SetAssignment};
use crate::ast::values::Subquery;
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};

use crate::tokens::keyword::*;

/// `[AS] alias` on INSERT target table, e.g. `INSERT INTO t AS x`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertTableAlias<'input> {
    pub r#as: AS,
    pub name: literal::Ident<'input>,
}

/// `OVERRIDING {SYSTEM | USER} VALUE` clause on an INSERT statement.
///
/// Variant ordering: distinct first tokens (`SYSTEM` vs `USER`), so
/// declaration order is cosmetic.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OverridingClause {
    pub overriding: OVERRIDING,
    pub which: OverridingKind,
    pub value: VALUE,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum OverridingKind {
    System(SYSTEM),
    User(USER),
}

/// Multiple value rows: `VALUES (row1), (row2), ...`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertValueRows<'input> {
    pub values: VALUES,
    pub rows: Seq<ValueList<'input>, punct::Comma>,
}

/// Insert source: DEFAULT VALUES, VALUES (row), ..., or SELECT query.
///
/// Variant ordering: Default (`DEFAULT VALUES`) is longer than Rows (`VALUES`),
/// so longest-match-wins picks it when DEFAULT is present.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InsertSource<'input> {
    Default((DEFAULT, VALUES)),
    Rows(InsertValueRows<'input>),
    Select(Box<Subquery<'input>>),
}

/// DO UPDATE SET ... [WHERE ...] action.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoUpdateAction<'input> {
    pub r#do: DO,
    pub update: UPDATE,
    pub set: SET,
    pub assignments: Seq<SetAssignment<'input>, punct::Comma>,
    pub where_clause: Option<WhereClause<'input>>,
}

/// ON CONFLICT action: DO UPDATE SET ... [WHERE ...] or DO NOTHING.
///
/// Variant ordering: DoUpdate (`DO UPDATE SET`) is longer than
/// DoNothing (`DO NOTHING`), but both start with `DO` and diverge
/// at the next keyword, so the regex disambiguates.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ConflictAction<'input> {
    DoUpdate(Box<DoUpdateAction<'input>>),
    DoNothing((DO, NOTHING)),
}

/// One entry in an `ON CONFLICT (...)` target list.
///
/// Matches the index-element grammar: an expression (plain column name,
/// qualified name, parenthesized expression, or function call) optionally
/// followed by a `COLLATE "name"` clause and an optional opclass ident.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ConflictTargetItem<'input> {
    pub expr: Expr<'input>,
    pub collate: Option<crate::ast::create_table::CollateClause<'input>>,
    pub opclass: Option<literal::Ident<'input>>,
}

/// ON CONFLICT clause: `ON CONFLICT [(col, ...)] DO UPDATE SET ... | DO NOTHING`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnConflictClause<'input> {
    pub on: ON,
    pub conflict: CONFLICT,
    pub target: Option<
        Surrounded<punct::LParen, Seq<ConflictTargetItem<'input>, punct::Comma>, punct::RParen>,
    >,
    /// `WHERE predicate` after the arbiter target list, restricting the
    /// partial-index arbiter to matching rows.
    pub where_clause: Option<WhereClause<'input>>,
    pub action: ConflictAction<'input>,
}

/// INSERT INTO statement with optional ON CONFLICT and RETURNING.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[format_tokens(group(consistent))]
pub struct InsertStmt<'input> {
    pub insert: INSERT,
    pub into: INTO,
    pub table_name: QualifiedName<'input>,
    /// Optional `[AS] alias` after the target table, used to rebind the
    /// target in ON CONFLICT DO UPDATE expressions.
    pub alias: Option<InsertTableAlias<'input>>,
    pub columns: Option<Box<ColumnList<'input>>>,
    /// `OVERRIDING {SYSTEM|USER} VALUE` between the column list and the
    /// source. Controls whether explicit values override GENERATED ALWAYS
    /// identity columns.
    pub overriding: Option<OverridingClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub source: Box<InsertSource<'input>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub on_conflict: Option<Box<OnConflictClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub returning: Option<Box<ReturningClause<'input>>>,
}

/// Column list: `(col1, col2, ...)`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit, derive_more::Deref)]
#[parse(rules = SqlRules)]
pub struct ColumnList<'input>(
    #[deref] pub Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
);

/// Value list: `(col1, col2, ...)`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit, derive_more::Deref)]
#[parse(rules = SqlRules)]
pub struct ValueList<'input>(
    #[deref] pub Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
);

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::insert::InsertStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_insert_qualified_table() {
        let mut input = Input::new("INSERT INTO pg_catalog.foo VALUES (1)");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "foo");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_with_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't')");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "BOOLTBL1");
        assert!(stmt.columns.is_some());
        assert_eq!(stmt.columns.as_ref().unwrap().inner.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_multiple_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL3 (d, b, o) VALUES ('true', true, 1)");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.columns.as_ref().unwrap().inner.len(), 3);
    }

    #[test]
    fn parse_insert_without_columns() {
        let mut input = Input::new("INSERT INTO booltbl4 VALUES (false, true, null)");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.columns.is_none());
    }

    #[test]
    fn parse_insert_default_values_returning() {
        let mut input = Input::new("INSERT INTO t DEFAULT VALUES RETURNING *");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(*stmt.source, super::InsertSource::Default(_)));
        assert!(stmt.returning.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_select() {
        let mut input = Input::new("INSERT INTO y SELECT generate_series(1, 10)");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(*stmt.source, super::InsertSource::Select(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_on_conflict_do_nothing() {
        let mut input = Input::new("INSERT INTO t VALUES (1) ON CONFLICT (k) DO NOTHING");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.on_conflict.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_on_conflict_do_update() {
        let mut input =
            Input::new("INSERT INTO t VALUES (1) ON CONFLICT (k) DO UPDATE SET v = 'updated'");
        let stmt = InsertStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.on_conflict.is_some());
        assert!(input.is_empty());
    }
}
