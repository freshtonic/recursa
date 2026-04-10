/// UPDATE statement AST.
///
/// `UPDATE table SET col = expr [, ...] [FROM ...] [WHERE ...] [RETURNING ...]`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::expr::Expr;
use crate::ast::select::{FromClause, WhereClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// A single SET assignment: `col = expr` or `(col, ...) = (expr, ...)`
///
/// Manual Parse impl needed because the tuple form `(col, ...) = (expr, ...)`
/// vs single form `col = expr` requires lookahead.
/// To eliminate this, recursa would need look-ahead enum disambiguation for
/// non-keyword-led variants.
#[derive(Debug, Clone, Visit)]
pub enum SetAssignment {
    Single {
        column: literal::AliasName,
        value: Expr,
    },
    Tuple {
        columns: Vec<literal::AliasName>,
        values: Expr,
    },
}

impl<'input> Parse<'input> for SetAssignment {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        literal::AliasName::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        literal::AliasName::peek(input, rules) || punct::LParen::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        if punct::LParen::peek(input, rules) {
            // Tuple form: (col, ...) = expr
            punct::LParen::parse(input, rules)?;
            R::consume_ignored(input);
            let mut columns = Vec::new();
            loop {
                columns.push(literal::AliasName::parse(input, rules)?);
                R::consume_ignored(input);
                if punct::Comma::peek(input, rules) {
                    punct::Comma::parse(input, rules)?;
                    R::consume_ignored(input);
                } else {
                    break;
                }
            }
            punct::RParen::parse(input, rules)?;
            R::consume_ignored(input);
            punct::Eq::parse(input, rules)?;
            R::consume_ignored(input);
            let values = Expr::parse(input, rules)?;
            Ok(SetAssignment::Tuple { columns, values })
        } else {
            let column = literal::AliasName::parse(input, rules)?;
            R::consume_ignored(input);
            punct::Eq::parse(input, rules)?;
            R::consume_ignored(input);
            let value = Expr::parse(input, rules)?;
            Ok(SetAssignment::Single { column, value })
        }
    }
}

/// RETURNING clause: `RETURNING expr, ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReturningClause {
    pub _returning: PhantomData<keyword::Returning>,
    pub items: Seq<crate::ast::select::SelectItem, punct::Comma>,
}

/// UPDATE statement: `UPDATE table [alias] SET assignments [FROM ...] [WHERE ...] [RETURNING ...]`
///
/// Manual Parse impl needed because the optional alias after the table name
/// has the same keyword-rejection issue as DELETE -- keywords like WHERE/SET
/// would be rejected as identifiers but peek would succeed.
/// To eliminate this, recursa would need Option try-parse semantics.
#[derive(Debug, Clone, Visit)]
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

impl<'input> Parse<'input> for UpdateStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::Update::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        keyword::Update::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let _update = PhantomData::<keyword::Update>::parse(input, rules)?;
        R::consume_ignored(input);
        let table_name = literal::Ident::parse(input, rules)?;
        R::consume_ignored(input);

        // Try alias (not SET keyword)
        let alias = if keyword::Set::peek(input, rules) {
            None
        } else {
            let mut fork = input.fork();
            match literal::Ident::parse(&mut fork, rules) {
                Ok(ident) => {
                    input.advance(fork.cursor() - input.cursor());
                    R::consume_ignored(input);
                    Some(ident)
                }
                Err(_) => None,
            }
        };

        let _set = PhantomData::<keyword::Set>::parse(input, rules)?;
        R::consume_ignored(input);
        let assignments = Seq::<SetAssignment, punct::Comma>::parse(input, rules)?;
        R::consume_ignored(input);
        let from_clause = Option::<FromClause>::parse(input, rules)?;
        R::consume_ignored(input);
        let where_clause = Option::<WhereClause>::parse(input, rules)?;
        R::consume_ignored(input);
        let returning = Option::<ReturningClause>::parse(input, rules)?;

        Ok(UpdateStmt {
            _update,
            table_name,
            alias,
            _set,
            assignments,
            from_clause,
            where_clause,
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
