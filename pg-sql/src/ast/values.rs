/// VALUES statement, TABLE statement, and set operation support (UNION, EXCEPT, INTERSECT).
use std::marker::PhantomData;

use recursa::{Input, Parse, ParseError, ParseRules, Visit};

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

/// Set operation type: UNION ALL, UNION DISTINCT, UNION, EXCEPT ALL, EXCEPT, INTERSECT ALL, INTERSECT
#[derive(Debug, Clone)]
pub enum SetOp {
    UnionAll,
    UnionDistinct,
    Union,
    ExceptAll,
    Except,
    IntersectAll,
    Intersect,
}

impl recursa::visitor::AsNodeKey for SetOp {}

impl Visit for SetOp {
    fn visit<V: recursa::visitor::Visitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

/// A set operation combiner: `UNION [ALL|DISTINCT] | EXCEPT [ALL] | INTERSECT [ALL]`
///
/// Manual Parse impl because the operator keyword must be consumed first,
/// then the optional ALL/DISTINCT modifier, before we know which variant we have.
/// To eliminate this, recursa would need multi-keyword compound tokens.
#[derive(Debug, Clone, Visit)]
pub struct SetOpCombiner {
    pub op: SetOp,
    pub right: Box<CompoundQuery>,
}

impl<'input> Parse<'input> for SetOpCombiner {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // Union starts with UNION, but EXCEPT and INTERSECT also valid
        keyword::Union::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        keyword::Union::peek(input, rules)
            || keyword::Except::peek(input, rules)
            || keyword::Intersect::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let op = if keyword::Union::peek(input, rules) {
            PhantomData::<keyword::Union>::parse(input, rules)?;
            R::consume_ignored(input);
            if keyword::All::peek(input, rules) {
                PhantomData::<keyword::All>::parse(input, rules)?;
                SetOp::UnionAll
            } else if keyword::Distinct::peek(input, rules) {
                PhantomData::<keyword::Distinct>::parse(input, rules)?;
                SetOp::UnionDistinct
            } else {
                SetOp::Union
            }
        } else if keyword::Except::peek(input, rules) {
            PhantomData::<keyword::Except>::parse(input, rules)?;
            R::consume_ignored(input);
            if keyword::All::peek(input, rules) {
                PhantomData::<keyword::All>::parse(input, rules)?;
                SetOp::ExceptAll
            } else {
                SetOp::Except
            }
        } else {
            PhantomData::<keyword::Intersect>::parse(input, rules)?;
            R::consume_ignored(input);
            if keyword::All::peek(input, rules) {
                PhantomData::<keyword::All>::parse(input, rules)?;
                SetOp::IntersectAll
            } else {
                SetOp::Intersect
            }
        };
        R::consume_ignored(input);
        let right = Box::new(CompoundQuery::parse(input, rules)?);
        Ok(SetOpCombiner { op, right })
    }
}

/// A compound query: a query body optionally followed by a set operation.
/// This allows chaining: `VALUES ... UNION ALL SELECT ... EXCEPT TABLE ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CompoundQuery {
    Table(TableStmt),
    Body(CompoundBody),
}

/// A SELECT or VALUES body with optional set operation continuation.
#[derive(Debug, Clone, Parse, Visit)]
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
