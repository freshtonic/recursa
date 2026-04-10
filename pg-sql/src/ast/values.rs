/// VALUES statement, TABLE statement, and UNION ALL support.
use std::marker::PhantomData;

use recursa::{Parse, Visit};

use crate::ast::select::SelectBody;
use crate::rules::SqlRules;
use crate::tokens::keyword;

/// TABLE statement: `TABLE tablename`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableStmt {
    pub _table: PhantomData<keyword::Table>,
    pub table_name: crate::tokens::literal::Ident,
}

/// UNION ALL combiner between two query bodies.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnionAll {
    pub _union: PhantomData<keyword::Union>,
    pub _all: PhantomData<keyword::All>,
    pub right: Box<CompoundQuery>,
}

/// A compound query: a query body optionally followed by UNION ALL.
/// This allows chaining: `VALUES ... UNION ALL SELECT ... UNION ALL TABLE ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CompoundQuery {
    Table(TableStmt),
    Body(CompoundBody),
}

/// A SELECT or VALUES body with optional UNION ALL continuation.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CompoundBody {
    pub body: SelectBody,
    pub union_all: Option<UnionAll>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::values::{CompoundBody, TableStmt};
    use crate::rules::SqlRules;

    #[test]
    fn parse_table_stmt() {
        let mut input = Input::new("TABLE int8_tbl");
        let stmt = TableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "int8_tbl");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_values_standalone() {
        let mut input = Input::new("VALUES (1,2), (3,4), (7,8)");
        let body = CompoundBody::parse(&mut input, &SqlRules).unwrap();
        assert!(body.union_all.is_none());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_values_union_all_select() {
        let mut input = Input::new("VALUES (1,2) UNION ALL SELECT 3, 4");
        let body = CompoundBody::parse(&mut input, &SqlRules).unwrap();
        assert!(body.union_all.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_values_union_all_table() {
        let mut input = Input::new("VALUES (1,2) UNION ALL TABLE t");
        let body = CompoundBody::parse(&mut input, &SqlRules).unwrap();
        assert!(body.union_all.is_some());
        assert!(input.is_empty());
    }
}
