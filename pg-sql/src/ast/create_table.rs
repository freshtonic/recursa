/// CREATE TABLE statement AST.
use recursa::seq::Seq;
use recursa::{Parse, Visit};

use crate::ast::expr::TypeName;
use crate::rules::SqlRules;
use crate::tokens;

/// A column definition: `name type`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDef {
    pub name: tokens::Ident,
    pub type_name: TypeName,
}

/// CREATE TABLE statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTableStmt {
    pub create_kw: tokens::Create,
    pub table_kw: tokens::Table,
    pub name: tokens::Ident,
    pub lparen: tokens::LParen,
    pub columns: Seq<ColumnDef, tokens::Comma>,
    pub rparen: tokens::RParen,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_table::CreateTableStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_table_single_column() {
        let mut input = Input::new("CREATE TABLE BOOLTBL1 (f1 bool)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL1");
        assert_eq!(stmt.columns.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_multiple_columns() {
        let mut input = Input::new("CREATE TABLE BOOLTBL3 (d text, b bool, o int)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL3");
        assert_eq!(stmt.columns.len(), 3);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_boolean_type() {
        let mut input = Input::new("CREATE TABLE t (f1 boolean)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns.len(), 1);
    }
}
