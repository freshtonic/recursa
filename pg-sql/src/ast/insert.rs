/// INSERT INTO statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Parse, Visit};

use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens;

/// INSERT INTO statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertStmt {
    pub _insert: PhantomData<tokens::Insert>,
    pub _into: PhantomData<tokens::Into>,
    pub table_name: tokens::Ident,
    pub columns: Option<ColumnList>,
    pub _values: PhantomData<tokens::Values>,
    pub values: ValueList,
}

/// Column list: `(col1, col2, ...)`.
pub type ColumnList = Surrounded<tokens::LParen, Seq<tokens::Ident, tokens::Comma>, tokens::RParen>;

/// Value list: `(col1, col2, ...)`.
pub type ValueList = Surrounded<tokens::LParen, Seq<Expr, tokens::Comma>, tokens::RParen>;

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::insert::InsertStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_insert_with_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't')");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "BOOLTBL1");
        assert!(stmt.columns.is_some());
        assert_eq!(stmt.columns.as_ref().unwrap().inner.len(), 1);
        assert_eq!(stmt.values.inner.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_multiple_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL3 (d, b, o) VALUES ('true', true, 1)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns.as_ref().unwrap().inner.len(), 3);
        assert_eq!(stmt.values.inner.len(), 3);
    }

    #[test]
    fn parse_insert_without_columns() {
        let mut input = Input::new("INSERT INTO booltbl4 VALUES (false, true, null)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.columns.is_none());
        assert_eq!(stmt.values.inner.len(), 3);
    }
}
