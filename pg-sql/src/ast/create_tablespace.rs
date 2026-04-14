/// CREATE TABLESPACE / DROP TABLESPACE statement AST.
use std::marker::PhantomData;

use recursa::{FormatTokens, Parse, Visit};

use crate::ast::create_index::WithStorage;
use crate::ast::create_view::IfExistsKw;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal};

/// `OWNER role` optional clause.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OwnerClause {
    pub _owner: PhantomData<keyword::Owner>,
    pub role: literal::Ident,
}

/// `LOCATION 'path'` clause.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LocationClause {
    pub _location: PhantomData<keyword::Location>,
    pub path: literal::StringLit,
}

/// `CREATE TABLESPACE name [OWNER role] LOCATION 'path' [WITH (params)]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTablespaceStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _tablespace: PhantomData<keyword::Tablespace>,
    pub name: literal::Ident,
    pub owner: Option<OwnerClause>,
    pub location: LocationClause,
    pub with_options: Option<WithStorage>,
}

/// `DROP TABLESPACE [IF EXISTS] name`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTablespaceStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _tablespace: PhantomData<keyword::Tablespace>,
    pub if_exists: Option<IfExistsKw>,
    pub name: literal::Ident,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use super::*;

    #[test]
    fn parse_create_tablespace_basic() {
        let mut input = Input::new("CREATE TABLESPACE ts1 LOCATION '/tmp'");
        let _stmt = CreateTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_tablespace_with_options() {
        let mut input =
            Input::new("CREATE TABLESPACE ts1 LOCATION '' WITH (random_page_cost = 3.0)");
        let _stmt = CreateTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_tablespace_owner() {
        let mut input = Input::new("CREATE TABLESPACE ts1 OWNER foo LOCATION '/tmp'");
        let _stmt = CreateTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_tablespace() {
        let mut input = Input::new("DROP TABLESPACE ts1");
        let _stmt = DropTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_tablespace_if_exists() {
        let mut input = Input::new("DROP TABLESPACE IF EXISTS ts1");
        let _stmt = DropTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
