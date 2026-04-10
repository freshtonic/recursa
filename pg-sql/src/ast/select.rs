/// SELECT statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::{Parse, Visit};

use crate::ast::expr::{Expr, FuncCall};
use crate::rules::SqlRules;
use crate::tokens;

/// A single item in the SELECT list: `expr [AS alias]`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<Alias>,
}

/// AS alias clause.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct Alias {
    pub _as: PhantomData<tokens::As>,
    pub name: tokens::AliasName,
}

/// FROM clause: `FROM table [, table ...]`.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FromClause {
    pub _from: PhantomData<tokens::From>,
    pub tables: Seq<TableRef, tokens::Comma>,
}

/// A table reference: either a plain table name or a function call.
/// FuncCall listed first — its longer first_pattern (ident + lparen) wins
/// over plain Ident via longest-match dispatch.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TableRef {
    Func(FuncCall),
    Table(tokens::Ident),
}

/// WHERE clause: `WHERE expr`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhereClause {
    pub _where: PhantomData<tokens::Where>,
    pub condition: Expr,
}

/// ORDER BY clause: `ORDER BY expr [, expr ...]`.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByClause {
    pub _order: PhantomData<tokens::Order>,
    pub _by: PhantomData<tokens::By>,
    pub items: Seq<Expr, tokens::Comma>,
}

/// SELECT statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectStmt {
    pub _select: PhantomData<tokens::Select>,
    pub items: Seq<SelectItem, tokens::Comma>,
    pub from_clause: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub order_by: Option<OrderByClause>,
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
        assert_eq!(alias.name.0, "true");
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
}
