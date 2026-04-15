/// CREATE TABLESPACE / DROP TABLESPACE statement AST.
use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::create_index::{StorageParam, WithStorage};
use crate::ast::create_view::IfExistsKw;
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// `OWNER role` optional clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OwnerClause<'input> {
    pub _owner: OWNER,
    pub role: literal::Ident<'input>,
}

/// `LOCATION 'path'` clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LocationClause<'input> {
    pub _location: LOCATION,
    pub path: literal::StringLit<'input>,
}

/// `CREATE TABLESPACE name [OWNER role] LOCATION 'path' [WITH (params)]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTablespaceStmt<'input> {
    pub _create: CREATE,
    pub _tablespace: TABLESPACE,
    pub name: literal::Ident<'input>,
    pub owner: Option<OwnerClause<'input>>,
    pub location: LocationClause<'input>,
    pub with_options: Option<WithStorage<'input>>,
}

/// `RENAME TO new_name` action on ALTER TABLESPACE.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTablespaceRename<'input> {
    pub _rename: RENAME,
    pub _to: TO,
    pub new_name: literal::Ident<'input>,
}

/// `OWNER TO new_owner` action on ALTER TABLESPACE.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTablespaceOwner<'input> {
    pub _owner: OWNER,
    pub _to: TO,
    pub new_owner: literal::Ident<'input>,
}

/// `SET (param = value, ...)` action on ALTER TABLESPACE.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTablespaceSetAction<'input> {
    pub _set: SET,
    pub params:
        Surrounded<punct::LParen, Seq<StorageParam<'input>, punct::Comma>, punct::RParen>,
}

/// `RESET (param [, ...])` action on ALTER TABLESPACE.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTablespaceResetAction<'input> {
    pub _reset: RESET,
    pub params:
        Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>,
}

/// One of the supported ALTER TABLESPACE actions.
///
/// Variant ordering: all variants start with distinct keywords (SET, RESET,
/// RENAME, OWNER), so order is for clarity only.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum AlterTablespaceAction<'input> {
    Set(AlterTablespaceSetAction<'input>),
    Reset(AlterTablespaceResetAction<'input>),
    Rename(AlterTablespaceRename<'input>),
    Owner(AlterTablespaceOwner<'input>),
}

/// `ALTER TABLESPACE name { RENAME TO new_name | OWNER TO new_owner
///                         | SET (params) | RESET (params) }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTablespaceStmt<'input> {
    pub _alter: ALTER,
    pub _tablespace: TABLESPACE,
    pub name: literal::Ident<'input>,
    pub action: AlterTablespaceAction<'input>,
}

/// `DROP TABLESPACE [IF EXISTS] name`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTablespaceStmt<'input> {
    pub _drop: DROP,
    pub _tablespace: TABLESPACE,
    pub if_exists: Option<IfExistsKw>,
    pub name: literal::Ident<'input>,
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

    #[test]
    fn parse_alter_tablespace_set() {
        let mut input = Input::new("ALTER TABLESPACE ts SET (random_page_cost = 1.0)");
        let _stmt = AlterTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_alter_tablespace_reset() {
        let mut input =
            Input::new("ALTER TABLESPACE ts RESET (random_page_cost, effective_io_concurrency)");
        let _stmt = AlterTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_alter_tablespace_rename() {
        let mut input = Input::new("ALTER TABLESPACE ts RENAME TO ts2");
        let _stmt = AlterTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_alter_tablespace_owner() {
        let mut input = Input::new("ALTER TABLESPACE ts OWNER TO foo");
        let _stmt = AlterTablespaceStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
