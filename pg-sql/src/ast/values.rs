/// VALUES statement, TABLE statement, and set operation support (UNION, EXCEPT, INTERSECT).
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::select::SelectBody;
use crate::rules::SqlRules;
use crate::tokens::{punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// TABLE statement: `TABLE tablename`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableStmt<'input> {
    pub table: TABLE,
    pub table_name: crate::tokens::literal::Ident<'input>,
}

/// Set operation type.
///
/// Variant ordering: longer keyword sequences first within each group
/// so longest-match-wins picks UNION ALL over bare UNION, etc.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetOp {
    UnionAll((UNION, ALL)),
    UnionDistinct((UNION, DISTINCT)),
    ExceptAll((EXCEPT, ALL)),
    IntersectAll((INTERSECT, ALL)),
    Union(UNION),
    Except(EXCEPT),
    Intersect(INTERSECT),
}

/// A set operation combiner: `UNION [ALL|DISTINCT] | EXCEPT [ALL] | INTERSECT [ALL]`
/// followed by the right-hand query.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetOpCombiner<'input> {
    pub op: SetOp,
    pub right: Box<CompoundQuery<'input>>,
}

/// A compound query: a query body optionally followed by a set operation.
/// This allows chaining: `VALUES ... UNION ALL SELECT ... EXCEPT TABLE ...`
/// Paren variant handles `(WITH ... SELECT ... UNION ...)` grouping.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CompoundQuery<'input> {
    Paren(CompoundParen<'input>),
    Table(TableStmt<'input>),
    Body(CompoundBody<'input>),
}

/// Parenthesized compound query with optional set operation continuation.
/// e.g., `(SELECT ... UNION ALL ...) EXCEPT ...`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CompoundParen<'input> {
    pub inner: Surrounded<punct::LParen, Box<CompoundQuery<'input>>, punct::RParen>,
    pub set_op: Option<SetOpCombiner<'input>>,
    /// Optional trailing `ORDER BY ...` applied to the parenthesized query.
    pub order_by: Option<Box<crate::ast::select::OrderByClause<'input>>>,
    /// Optional trailing `LIMIT ...`.
    pub limit: Option<Box<crate::ast::select::LimitClause<'input>>>,
    /// Optional trailing `OFFSET ...`.
    pub offset: Option<Box<crate::ast::select::OffsetClause<'input>>>,
}

/// A SELECT or VALUES body with optional set operation continuation.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CompoundBody<'input> {
    pub body: SelectBody<'input>,
    pub set_op: Option<SetOpCombiner<'input>>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::values::{CompoundBody, TableStmt};
    use crate::rules::SqlRules;

    #[test]
    fn parse_table_stmt() {
        let mut input = Input::new("TABLE int8_tbl");
        let stmt = TableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.table_name.text(), "int8_tbl");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_values_standalone() {
        let mut input = Input::new("VALUES (1,2), (3,4), (7,8)");
        let body = CompoundBody::parse::<SqlRules>(&mut input).unwrap();
        assert!(body.set_op.is_none());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_values_union_all_select() {
        let mut input = Input::new("VALUES (1,2) UNION ALL SELECT 3, 4");
        let body = CompoundBody::parse::<SqlRules>(&mut input).unwrap();
        assert!(body.set_op.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_values_union_all_table() {
        let mut input = Input::new("VALUES (1,2) UNION ALL TABLE t");
        let body = CompoundBody::parse::<SqlRules>(&mut input).unwrap();
        assert!(body.set_op.is_some());
        assert!(input.is_empty());
    }
}
