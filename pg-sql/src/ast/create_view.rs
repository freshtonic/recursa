/// CREATE VIEW and DROP VIEW statement AST.
///
/// `CREATE [OR REPLACE] [TEMP|TEMPORARY] [RECURSIVE] VIEW name [(cols)] AS query`
/// `DROP VIEW [IF EXISTS] name`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::create_table::TempKw;
use crate::ast::values::CompoundQuery;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// OR REPLACE keyword pair.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrReplaceKw {
    pub _or: PhantomData<keyword::Or>,
    pub _replace: PhantomData<keyword::Replace>,
}

/// IF EXISTS keyword pair.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IfExistsKw {
    pub _if: PhantomData<keyword::If>,
    pub _exists: PhantomData<keyword::Exists>,
}

/// CREATE VIEW statement.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateViewStmt {
    pub _create: PhantomData<keyword::Create>,
    pub or_replace: Option<OrReplaceKw>,
    pub temp: Option<TempKw>,
    pub recursive: Option<PhantomData<keyword::Recursive>>,
    pub _view: PhantomData<keyword::View>,
    pub name: literal::Ident,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub _as: PhantomData<keyword::As>,
    pub query: CompoundQuery,
}

/// DROP VIEW statement: `DROP VIEW [IF EXISTS] name`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropViewStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _view: PhantomData<keyword::View>,
    pub if_exists: Option<IfExistsKw>,
    pub name: literal::Ident,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_create_view() {
        let mut input = Input::new("CREATE VIEW v AS SELECT 1");
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "v");
        assert!(stmt.or_replace.is_none());
        assert!(stmt.temp.is_none());
        assert!(stmt.recursive.is_none());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_temp_view() {
        let mut input = Input::new("CREATE TEMPORARY VIEW v AS SELECT 1");
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.temp.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_recursive_view() {
        let mut input = Input::new(
            "CREATE RECURSIVE VIEW nums (n) AS VALUES (1) UNION ALL SELECT n+1 FROM nums WHERE n < 5",
        );
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.recursive.is_some());
        assert!(stmt.columns.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_or_replace_recursive_view() {
        let mut input = Input::new(
            "CREATE OR REPLACE RECURSIVE VIEW nums (n) AS VALUES (1) UNION ALL SELECT n+1 FROM nums WHERE n < 6",
        );
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.or_replace.is_some());
        assert!(stmt.recursive.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_view() {
        let mut input = Input::new("DROP VIEW v");
        let stmt = DropViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "v");
        assert!(stmt.if_exists.is_none());
        assert!(input.is_empty());
    }
}
