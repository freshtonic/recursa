/// VALUES statement, TABLE statement, and set operation support (UNION, EXCEPT, INTERSECT).
use std::marker::PhantomData;

use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::select::SelectBody;
use crate::rules::SqlRules;
use crate::tokens::{keyword, punct};
use recursa_diagram::railroad;

/// TABLE statement: `TABLE tablename`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableStmt {
    pub _table: PhantomData<keyword::Table>,
    pub table_name: crate::tokens::literal::Ident,
}

/// UNION ALL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnionAllOp(PhantomData<keyword::Union>, PhantomData<keyword::All>);

/// UNION DISTINCT
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnionDistinctOp(PhantomData<keyword::Union>, PhantomData<keyword::Distinct>);

/// UNION (bare)
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnionOp(PhantomData<keyword::Union>);

/// EXCEPT ALL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExceptAllOp(PhantomData<keyword::Except>, PhantomData<keyword::All>);

/// EXCEPT (bare)
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExceptOp(PhantomData<keyword::Except>);

/// INTERSECT ALL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IntersectAllOp(PhantomData<keyword::Intersect>, PhantomData<keyword::All>);

/// INTERSECT (bare)
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IntersectOp(PhantomData<keyword::Intersect>);

/// Set operation type.
///
/// Variant ordering: longer keyword sequences first within each group
/// so longest-match-wins picks UNION ALL over bare UNION, etc.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetOp {
    UnionAll(UnionAllOp),
    UnionDistinct(UnionDistinctOp),
    ExceptAll(ExceptAllOp),
    IntersectAll(IntersectAllOp),
    Union(UnionOp),
    Except(ExceptOp),
    Intersect(IntersectOp),
}

/// A set operation combiner: `UNION [ALL|DISTINCT] | EXCEPT [ALL] | INTERSECT [ALL]`
/// followed by the right-hand query.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetOpCombiner {
    pub op: SetOp,
    pub right: Box<CompoundQuery>,
}

/// A compound query: a query body optionally followed by a set operation.
/// This allows chaining: `VALUES ... UNION ALL SELECT ... EXCEPT TABLE ...`
/// Paren variant handles `(WITH ... SELECT ... UNION ...)` grouping.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CompoundQuery {
    Paren(CompoundParen),
    Table(TableStmt),
    Body(CompoundBody),
}

/// Parenthesized compound query with optional set operation continuation.
/// e.g., `(SELECT ... UNION ALL ...) EXCEPT ...`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CompoundParen {
    pub inner: Surrounded<punct::LParen, Box<CompoundQuery>, punct::RParen>,
    pub set_op: Option<SetOpCombiner>,
}

/// A SELECT or VALUES body with optional set operation continuation.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CompoundBody {
    pub body: SelectBody,
    pub set_op: Option<SetOpCombiner>,
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
