/// DROP TABLE statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::common::{DropBehavior, QualifiedName};
use crate::ast::create_view::IfExistsKw;
use crate::rules::SqlRules;
use crate::tokens::{keyword, punct};

/// ```sql
/// DROP TABLE [IF EXISTS] name [, name ...] [CASCADE | RESTRICT]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTableStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _table: PhantomData<keyword::Table>,
    pub if_exists: Option<IfExistsKw>,
    pub names: Seq<QualifiedName<'input>, punct::Comma>,
    pub behavior: Option<DropBehavior>,
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
        assert_eq!(stmt.names.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_table_lowercase() {
        let mut input = Input::new("drop table my_table");
        let stmt = DropTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.names.len(), 1);
    }

    #[test]
    fn parse_drop_table_if_exists() {
        let mut input = Input::new("DROP TABLE IF EXISTS foo");
        let stmt = DropTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.if_exists.is_some());
    }

    #[test]
    fn parse_drop_table_multi_cascade() {
        let mut input = Input::new("DROP TABLE IF EXISTS a, b, c CASCADE");
        let stmt = DropTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.if_exists.is_some());
        assert_eq!(stmt.names.len(), 3);
        assert!(stmt.behavior.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_table_qualified() {
        let mut input = Input::new("DROP TABLE schema1.foo RESTRICT");
        let stmt = DropTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.behavior.is_some());
        assert!(input.is_empty());
    }
}
