/// DROP TABLE statement AST.
use std::marker::PhantomData;

use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal};

/// DROP TABLE statement.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTableStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _table: PhantomData<keyword::Table>,
    pub name: literal::Ident,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::drop_table::DropTableStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_drop_table() {
        let mut input = Input::new("DROP TABLE BOOLTBL1");
        let stmt = DropTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "BOOLTBL1");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_table_lowercase() {
        let mut input = Input::new("drop table my_table");
        let stmt = DropTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "my_table");
    }
}
