/// UPDATE statement AST.
///
/// `UPDATE table SET col = expr [, ...] [FROM ...] [WHERE ...] [RETURNING ...]`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::common::QualifiedName;
use crate::ast::expr::Expr;
use crate::ast::select::{FromClause, WhereClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// `[idx]` subscript suffix for a target column in UPDATE SET.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubscriptSuffix {
    pub _lbracket: punct::LBracket,
    pub index: Expr,
    pub _rbracket: punct::RBracket,
}

/// Single SET assignment: `col[idx] = expr` or `col = expr`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SingleAssignment {
    pub column: literal::AliasName,
    pub subscript: Option<Box<SubscriptSuffix>>,
    pub _eq: punct::Eq,
    pub value: Expr,
}

/// Tuple SET assignment: `(col, ...) = expr`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TupleAssignment {
    pub columns: recursa::surrounded::Surrounded<
        punct::LParen,
        Seq<literal::AliasName, punct::Comma>,
        punct::RParen,
    >,
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
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[format_tokens(group(consistent))]
pub struct UpdateStmt {
    pub _update: PhantomData<keyword::Update>,
    pub table_name: QualifiedName,
    pub alias: Option<literal::Ident>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub _set: PhantomData<keyword::Set>,
    #[format_tokens(indent)]
    pub assignments: Seq<SetAssignment, punct::Comma>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub from_clause: Option<FromClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub where_clause: Option<WhereClause>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub returning: Option<ReturningClause>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_update_qualified_table() {
        let mut input = Input::new("UPDATE pg_catalog.pg_class SET relname = '123'");
        let stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "pg_class");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_simple() {
        let mut input = Input::new("UPDATE y SET a = a + 1");
        let stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.object(), "y");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_with_returning() {
        let mut input = Input::new("UPDATE y SET a = a + 1 RETURNING *");
        let stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.returning.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_with_from_where() {
        let mut input = Input::new(
            "UPDATE y SET a = y.a - 10 FROM t WHERE y.a > 20 AND t.a = y.a RETURNING y.a",
        );
        let stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(stmt.where_clause.is_some());
        assert!(stmt.returning.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_subscript_assignment() {
        let mut input = Input::new("UPDATE t SET e[0] = '1.1'");
        let _stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_subscript_assignment_one() {
        let mut input = Input::new("UPDATE t SET e[1] = '2.2'");
        let _stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_plain_assignment_still_parses() {
        let mut input = Input::new("UPDATE t SET col = 'x'");
        let _stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_update_tuple_set() {
        let mut input = Input::new(
            "UPDATE parent SET (k, v) = (SELECT k, v FROM simpletup WHERE simpletup.k = parent.k)",
        );
        let _stmt = UpdateStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
