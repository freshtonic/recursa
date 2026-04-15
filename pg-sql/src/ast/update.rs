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
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SubscriptSuffix<'input> {
    pub _lbracket: punct::LBracket,
    pub index: Expr<'input>,
    pub _rbracket: punct::RBracket,
}

/// Single SET assignment: `col[idx] = expr` or `col = expr`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SingleAssignment<'input> {
    pub column: literal::AliasName<'input>,
    pub subscript: Option<Box<SubscriptSuffix<'input>>>,
    pub _eq: punct::Eq,
    pub value: Expr<'input>,
}

/// Tuple SET assignment: `(col, ...) = expr`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TupleAssignment<'input> {
    pub columns: recursa::surrounded::Surrounded<
        punct::LParen,
        Seq<literal::AliasName<'input>, punct::Comma>,
        punct::RParen,
    >,
    pub _eq: punct::Eq,
    pub values: Expr<'input>,
}

/// A single SET assignment: `col = expr` or `(col, ...) = (expr, ...)`
///
/// Variant ordering: Tuple starts with `(` which is longer than a bare
/// identifier, so longest-match-wins picks it when parens are present.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetAssignment<'input> {
    Tuple(TupleAssignment<'input>),
    Single(SingleAssignment<'input>),
}

/// RETURNING clause: `RETURNING expr, ...`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReturningClause<'input> {
    pub _returning: PhantomData<keyword::Returning>,
    pub items: Seq<crate::ast::select::SelectItem<'input>, punct::Comma>,
}

/// UPDATE statement: `UPDATE table [alias] SET assignments [FROM ...] [WHERE ...] [RETURNING ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[format_tokens(group(consistent))]
pub struct UpdateStmt<'input> {
    pub _update: PhantomData<keyword::Update>,
    pub table_name: QualifiedName<'input>,
    pub alias: Option<literal::Ident<'input>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub _set: PhantomData<keyword::Set>,
    #[format_tokens(indent)]
    pub assignments: Seq<SetAssignment<'input>, punct::Comma>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub from_clause: Option<Box<FromClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub where_clause: Option<Box<WhereClause<'input>>>,
    #[format_tokens(break(flat = " ", broken = "\n"))]
    pub returning: Option<Box<ReturningClause<'input>>>,
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
