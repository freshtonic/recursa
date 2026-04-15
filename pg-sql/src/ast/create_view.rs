/// CREATE VIEW and DROP VIEW statement AST.
///
/// `CREATE [OR REPLACE] [TEMP|TEMPORARY] [RECURSIVE] VIEW name [(cols)] AS query`
/// `DROP VIEW [IF EXISTS] name`
use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::common::{DropBehavior, QualifiedName};
use crate::ast::create_table::TempKw;
use crate::ast::values::Subquery;
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// CREATE VIEW statement.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateViewStmt<'input> {
    pub create: CREATE,
    pub or_replace: Option<(OR, REPLACE)>,
    pub temp: Option<TempKw>,
    pub recursive: Option<RECURSIVE>,
    pub view: VIEW,
    pub name: QualifiedName<'input>,
    pub columns: Option<
        Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>,
    >,
    /// Optional `USING access_method` (accepted by PG parser though rejected
    /// semantically for plain VIEW; tests include it).
    pub using: Option<ViewUsing<'input>>,
    /// Optional `WITH (option [= value], ...)` view options such as
    /// `security_invoker`, `security_barrier`, `check_option`.
    pub with_options: Option<crate::ast::create_index::WithStorage<'input>>,
    pub r#as: AS,
    pub query: Subquery<'input>,
    /// Optional `WITH [CASCADED|LOCAL] CHECK OPTION` trailer, used with
    /// updatable views to cascade predicate checks to underlying rows.
    pub check_option: Option<ViewCheckOption>,
}

/// `USING access_method` trailer on CREATE VIEW.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ViewUsing<'input> {
    pub using: USING,
    pub method: literal::AliasName<'input>,
}

/// `WITH [CASCADED | LOCAL] CHECK OPTION` trailer on CREATE VIEW.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ViewCheckOption {
    pub with: WITH,
    pub mode: Option<ViewCheckMode>,
    pub check: CHECK,
    pub option: OPTION,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ViewCheckMode {
    Cascaded(CASCADED),
    Local(LOCAL),
}

/// DROP VIEW statement:
///
/// ```sql
/// DROP VIEW [IF EXISTS] name [, name ...] [CASCADE | RESTRICT]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropViewStmt<'input> {
    pub drop: DROP,
    pub view: VIEW,
    pub if_exists: Option<(IF, EXISTS)>,
    pub names: Seq<QualifiedName<'input>, punct::Comma>,
    pub behavior: Option<DropBehavior>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_create_view() {
        let mut input = Input::new("CREATE VIEW v AS SELECT 1");
        let stmt = CreateViewStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.object(), "v");
        assert!(stmt.or_replace.is_none());
        assert!(stmt.temp.is_none());
        assert!(stmt.recursive.is_none());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_temp_view() {
        let mut input = Input::new("CREATE TEMPORARY VIEW v AS SELECT 1");
        let stmt = CreateViewStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.temp.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_recursive_view() {
        let mut input = Input::new(
            "CREATE RECURSIVE VIEW nums (n) AS VALUES (1) UNION ALL SELECT n+1 FROM nums WHERE n < 5",
        );
        let stmt = CreateViewStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.recursive.is_some());
        assert!(stmt.columns.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_or_replace_recursive_view() {
        let mut input = Input::new(
            "CREATE OR REPLACE RECURSIVE VIEW nums (n) AS VALUES (1) UNION ALL SELECT n+1 FROM nums WHERE n < 6",
        );
        let stmt = CreateViewStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.or_replace.is_some());
        assert!(stmt.recursive.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_view() {
        let mut input = Input::new("DROP VIEW v");
        let stmt = DropViewStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.names.len(), 1);
        assert!(stmt.if_exists.is_none());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_view_if_exists_multi_cascade() {
        let mut input = Input::new("DROP VIEW IF EXISTS a, b CASCADE");
        let stmt = DropViewStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.if_exists.is_some());
        assert_eq!(stmt.names.len(), 2);
        assert!(stmt.behavior.is_some());
        assert!(input.is_empty());
    }
}
