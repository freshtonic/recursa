/// MERGE statement AST.
///
/// `MERGE INTO table USING source ON condition
///   WHEN MATCHED THEN UPDATE SET ...
///   WHEN NOT MATCHED THEN INSERT VALUES (...)`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::expr::Expr;
use crate::ast::select::{SelectItem, TableRef};
use crate::ast::update::{ReturningClause, SetAssignment};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// WHEN MATCHED THEN UPDATE SET ...
#[derive(Debug, Clone, Visit)]
pub struct WhenMatchedUpdate {
    pub assignments: Seq<SetAssignment, punct::Comma>,
}

/// WHEN MATCHED THEN DELETE
#[derive(Debug, Clone, Visit)]
pub struct WhenMatchedDelete;

impl recursa::visitor::AsNodeKey for WhenMatchedDelete {}
impl recursa::visitor::AsNodeKey for WhenMatchedUpdate {}

/// WHEN NOT MATCHED THEN INSERT VALUES (...)
#[derive(Debug, Clone, Visit)]
pub struct WhenNotMatchedInsert {
    pub columns: Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub values: Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>,
}

impl recursa::visitor::AsNodeKey for WhenNotMatchedInsert {}

/// A WHEN clause in MERGE
#[derive(Debug, Clone)]
pub enum WhenClause {
    MatchedUpdate(WhenMatchedUpdate),
    MatchedDelete(WhenMatchedDelete),
    NotMatchedInsert(WhenNotMatchedInsert),
}

impl recursa::visitor::AsNodeKey for WhenClause {}

impl Visit for WhenClause {
    fn visit<V: recursa::visitor::Visitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for WhenClause {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::When::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        keyword::When::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        PhantomData::<keyword::When>::parse(input, rules)?;
        R::consume_ignored(input);

        if keyword::Not::peek(input, rules) {
            // WHEN NOT MATCHED THEN INSERT ...
            PhantomData::<keyword::Not>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Matched>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Then>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Insert>::parse(input, rules)?;
            R::consume_ignored(input);

            // Optional column list
            let columns = if punct::LParen::peek(input, rules) {
                // Check if this is a column list or VALUES
                let mut fork = input.fork();
                match Surrounded::<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>::parse(&mut fork, rules) {
                    Ok(cols) => {
                        // Check if VALUES follows
                        R::consume_ignored(&mut fork);
                        if keyword::Values::peek(&fork, rules) {
                            input.advance(fork.cursor() - input.cursor());
                            R::consume_ignored(input);
                            Some(cols)
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

            PhantomData::<keyword::Values>::parse(input, rules)?;
            R::consume_ignored(input);
            let values = Surrounded::parse(input, rules)?;

            Ok(WhenClause::NotMatchedInsert(WhenNotMatchedInsert {
                columns,
                values,
            }))
        } else {
            // WHEN MATCHED THEN UPDATE SET ... | DELETE
            PhantomData::<keyword::Matched>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Then>::parse(input, rules)?;
            R::consume_ignored(input);

            if keyword::Delete::peek(input, rules) {
                PhantomData::<keyword::Delete>::parse(input, rules)?;
                Ok(WhenClause::MatchedDelete(WhenMatchedDelete))
            } else {
                PhantomData::<keyword::Update>::parse(input, rules)?;
                R::consume_ignored(input);
                PhantomData::<keyword::Set>::parse(input, rules)?;
                R::consume_ignored(input);
                let assignments = Seq::parse(input, rules)?;
                Ok(WhenClause::MatchedUpdate(WhenMatchedUpdate { assignments }))
            }
        }
    }
}

/// MERGE statement.
///
/// Manual Parse impl needed because the USING source can be a subquery or
/// a table reference, and the WHEN clauses are a variable-length sequence.
/// To eliminate this, recursa would need multi-variant sequence parsing.
#[derive(Debug, Visit)]
pub struct MergeStmt {
    pub _merge: PhantomData<keyword::Merge>,
    pub _into: PhantomData<keyword::Into>,
    pub table_name: literal::Ident,
    pub _using: PhantomData<keyword::Using>,
    pub source: TableRef,
    pub _on: PhantomData<keyword::On>,
    pub condition: Expr,
    pub when_clauses: Vec<WhenClause>,
    pub returning: Option<ReturningClause>,
}

impl<'input> Parse<'input> for MergeStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::Merge::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        keyword::Merge::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let _merge = PhantomData::<keyword::Merge>::parse(input, rules)?;
        R::consume_ignored(input);
        let _into = PhantomData::<keyword::Into>::parse(input, rules)?;
        R::consume_ignored(input);
        let table_name = literal::Ident::parse(input, rules)?;
        R::consume_ignored(input);
        let _using = PhantomData::<keyword::Using>::parse(input, rules)?;
        R::consume_ignored(input);
        let source = TableRef::parse(input, rules)?;
        R::consume_ignored(input);
        let _on = PhantomData::<keyword::On>::parse(input, rules)?;
        R::consume_ignored(input);
        let condition = Expr::parse(input, rules)?;
        R::consume_ignored(input);

        let mut when_clauses = Vec::new();
        while keyword::When::peek(input, rules) {
            when_clauses.push(WhenClause::parse(input, rules)?);
            R::consume_ignored(input);
        }

        let returning = Option::<ReturningClause>::parse(input, rules)?;

        Ok(MergeStmt {
            _merge,
            _into,
            table_name,
            _using,
            source,
            _on,
            condition,
            when_clauses,
            returning,
        })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_merge_basic() {
        let sql = "MERGE INTO m USING (select 0 k, 'v' v) o ON m.k = o.k WHEN MATCHED THEN UPDATE SET v = 'updated' WHEN NOT MATCHED THEN INSERT VALUES(o.k, o.v)";
        let mut input = Input::new(sql);
        let stmt = MergeStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "m");
        assert_eq!(stmt.when_clauses.len(), 2);
        assert!(input.is_empty());
    }
}
