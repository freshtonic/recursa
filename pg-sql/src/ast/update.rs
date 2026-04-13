/// UPDATE statement AST.
///
/// `UPDATE table SET col = expr [, ...] [FROM ...] [WHERE ...] [RETURNING ...]`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::expr::Expr;
use crate::ast::select::{FromClause, WhereClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// Single SET assignment: `col = expr`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SingleAssignment {
    pub column: literal::AliasName,
    pub _eq: punct::Eq,
    pub value: Expr,
}

/// Tuple SET assignment: `(col, ...) = expr`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TupleAssignment {
    pub columns:
        recursa::surrounded::Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>,
    pub _eq: punct::Eq,
    pub values: Expr,
}

/// A single SET assignment: `col = expr` or `(col, ...) = (expr, ...)`
///
/// Variant ordering: Tuple starts with `(` which is longer than a bare
/// identifier, so longest-match-wins picks it when parens are present.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetAssignment {
    Tuple(TupleAssignment),
    Single(SingleAssignment),
}

/// RETURNING clause: `RETURNING expr, ...`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReturningClause {
    pub _returning: PhantomData<keyword::Returning>,
    pub items: Seq<crate::ast::select::SelectItem, punct::Comma>,
}

/// UPDATE statement: `UPDATE table [alias] SET assignments [FROM ...] [WHERE ...] [RETURNING ...]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UpdateStmt {
    pub _update: PhantomData<keyword::Update>,
    pub table_name: literal::Ident,
    pub alias: Option<literal::Ident>,
    pub _set: PhantomData<keyword::Set>,
    pub assignments: Seq<SetAssignment, punct::Comma>,
    pub from_clause: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub returning: Option<ReturningClause>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_update_simple() {
        let mut input = Input::new("UPDATE y SET a = a + 1");
        let stmt = UpdateStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "y");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_with_returning() {
        let mut input = Input::new("UPDATE y SET a = a + 1 RETURNING *");
        let stmt = UpdateStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.returning.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_with_from_where() {
        let mut input = Input::new(
            "UPDATE y SET a = y.a - 10 FROM t WHERE y.a > 20 AND t.a = y.a RETURNING y.a",
        );
        let stmt = UpdateStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(stmt.where_clause.is_some());
        assert!(stmt.returning.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_tuple_set() {
        let mut input = Input::new(
            "UPDATE parent SET (k, v) = (SELECT k, v FROM simpletup WHERE simpletup.k = parent.k)",
        );
        let stmt = UpdateStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(input.is_empty());
    }
}
