/// DROP TABLE statement AST.
use std::marker::PhantomData;

use recursa::{Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens;

/// DROP TABLE statement.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTableStmt {
    pub _drop: PhantomData<tokens::Drop>,
    pub _table: PhantomData<tokens::Table>,
    pub name: tokens::Ident,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::drop_table::DropTableStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_drop_table() {
        let mut input = Input::new("DROP TABLE BOOLTBL1");
        let stmt = DropTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL1");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_table_lowercase() {
        let mut input = Input::new("drop table my_table");
        let stmt = DropTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "my_table");
    }
}
