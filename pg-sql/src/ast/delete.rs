/// DELETE FROM statement AST.
use std::marker::PhantomData;

use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::common::QualifiedName;
use crate::ast::select::WhereClause;
use crate::ast::update::ReturningClause;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal};

/// Table alias with explicit AS keyword: `AS alias`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsAlias {
    pub _as: PhantomData<keyword::As>,
    pub name: literal::Ident,
}

/// Table alias in DELETE FROM: either `AS alias` or bare `alias`.
///
/// Variant ordering: WithAs (`AS ident`) has a longer first_pattern than
/// Bare (`ident`), so longest-match-wins picks it when AS is present.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TableAlias {
    WithAs(AsAlias),
    Bare(literal::Ident),
}

impl TableAlias {
    /// Returns the alias name regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            TableAlias::WithAs(a) => a.name.text(),
            TableAlias::Bare(ident) => ident.text(),
        }
    }
}

/// `USING table, ...` clause in DELETE statements.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeleteUsingClause {
    pub _using: PhantomData<keyword::Using>,
    pub tables: recursa::seq::Seq<crate::ast::select::TableRef, crate::tokens::punct::Comma>,
}

/// DELETE FROM statement: `DELETE FROM table [alias] [USING ...] [WHERE expr] [RETURNING ...]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[format_tokens(group(consistent))]
pub struct DeleteStmt {
    pub _delete: PhantomData<keyword::Delete>,
    pub _from: PhantomData<keyword::From>,
    pub table_name: QualifiedName,
    pub alias: Option<TableAlias>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub using_clause: Option<DeleteUsingClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub where_clause: Option<WhereClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub returning: Option<ReturningClause>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::delete::{DeleteStmt, TableAlias};
    use crate::rules::SqlRules;

    #[test]
    fn parse_delete_qualified_table() {
        let mut input = Input::new("DELETE FROM pg_catalog.pg_class");
        let stmt = DeleteStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "pg_class");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_delete_simple() {
        let mut input = Input::new("DELETE FROM delete_test WHERE a > 25");
        let stmt = DeleteStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "delete_test");
        assert!(stmt.alias.is_none());
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_delete_with_as_alias() {
        let mut input = Input::new("DELETE FROM delete_test AS dt WHERE dt.a > 75");
        let stmt = DeleteStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "delete_test");
        assert!(matches!(stmt.alias, Some(TableAlias::WithAs(_))));
        assert_eq!(stmt.alias.as_ref().unwrap().name(), "dt");
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_delete_with_bare_alias() {
        let mut input = Input::new("DELETE FROM delete_test dt WHERE delete_test.a > 25");
        let stmt = DeleteStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "delete_test");
        assert!(matches!(stmt.alias, Some(TableAlias::Bare(_))));
        assert_eq!(stmt.alias.as_ref().unwrap().name(), "dt");
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_delete_no_where() {
        let mut input = Input::new("DELETE FROM t");
        let stmt = DeleteStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "t");
        assert!(stmt.alias.is_none());
        assert!(stmt.where_clause.is_none());
        assert!(input.is_empty());
    }
}
