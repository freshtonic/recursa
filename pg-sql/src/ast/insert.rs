/// INSERT INTO statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Parse, Visit};

use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// DEFAULT VALUES variant.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DefaultValues {
    pub _default: PhantomData<keyword::Default>,
    pub _values: PhantomData<keyword::Values>,
}

/// Multiple value rows: `VALUES (row1), (row2), ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertValueRows {
    pub _values: PhantomData<keyword::Values>,
    pub rows: Seq<ValueList, punct::Comma>,
}

/// Insert source: DEFAULT VALUES or VALUES (row), (row), ...
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InsertSource {
    Default(DefaultValues),
    Rows(InsertValueRows),
}

/// INSERT INTO statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertStmt {
    pub _insert: PhantomData<keyword::Insert>,
    pub _into: PhantomData<keyword::Into>,
    pub table_name: literal::Ident,
    pub columns: Option<ColumnList>,
    pub source: InsertSource,
}

/// Column list: `(col1, col2, ...)`.
pub type ColumnList = Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>;

/// Value list: `(col1, col2, ...)`.
pub type ValueList = Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>;

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
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_multiple_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL3 (d, b, o) VALUES ('true', true, 1)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns.as_ref().unwrap().inner.len(), 3);
    }

    #[test]
    fn parse_insert_without_columns() {
        let mut input = Input::new("INSERT INTO booltbl4 VALUES (false, true, null)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.columns.is_none());
    }
}
