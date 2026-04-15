/// INSERT INTO statement AST.
///
/// Supports: `INSERT INTO table [(cols)] source [ON CONFLICT ...] [RETURNING ...]`
/// where source is DEFAULT VALUES, VALUES rows, or SELECT query.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::common::QualifiedName;
use crate::ast::expr::Expr;
use crate::ast::select::WhereClause;
use crate::ast::update::{ReturningClause, SetAssignment};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// DEFAULT VALUES variant.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DefaultValues {
    pub _default: PhantomData<keyword::Default>,
    pub _values: PhantomData<keyword::Values>,
}

/// Multiple value rows: `VALUES (row1), (row2), ...`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertValueRows<'input> {
    pub _values: PhantomData<keyword::Values>,
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
    Default(DefaultValues),
    Rows(InsertValueRows<'input>),
    Select(Box<crate::ast::values::CompoundQuery<'input>>),
}

/// DO UPDATE SET ... [WHERE ...] action.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoUpdateAction<'input> {
    pub _do: PhantomData<keyword::Do>,
    pub _update: PhantomData<keyword::Update>,
    pub _set: PhantomData<keyword::Set>,
    pub assignments: Seq<SetAssignment<'input>, punct::Comma>,
    pub where_clause: Option<WhereClause<'input>>,
}

/// DO NOTHING action.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoNothingAction {
    pub _do: PhantomData<keyword::Do>,
    pub _nothing: PhantomData<keyword::Nothing>,
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
    DoNothing(DoNothingAction),
}

/// ON CONFLICT clause: `ON CONFLICT [(col, ...)] DO UPDATE SET ... | DO NOTHING`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnConflictClause<'input> {
    pub _on: PhantomData<keyword::On>,
    pub _conflict: PhantomData<keyword::Conflict>,
    pub target: Option<
        Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>,
    >,
    pub action: ConflictAction<'input>,
}

/// INSERT INTO statement with optional ON CONFLICT and RETURNING.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[format_tokens(group(consistent))]
pub struct InsertStmt<'input> {
    pub _insert: PhantomData<keyword::Insert>,
    pub _into: PhantomData<keyword::Into>,
    pub table_name: QualifiedName<'input>,
    pub columns: Option<Box<ColumnList<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub source: Box<InsertSource<'input>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub on_conflict: Option<Box<OnConflictClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub returning: Option<Box<ReturningClause<'input>>>,
}

/// Column list: `(col1, col2, ...)`.
pub type ColumnList<'input> =
    Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>;

/// Value list: `(col1, col2, ...)`.
pub type ValueList<'input> =
    Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>;

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
