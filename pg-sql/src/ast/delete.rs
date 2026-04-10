/// DELETE FROM statement AST.
use std::marker::PhantomData;

use std::ops::ControlFlow;

use recursa::visitor::{AsNodeKey, Break, Visitor};
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::select::WhereClause;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal};

/// Table alias with explicit AS keyword: `AS alias`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsAlias {
    pub _as: PhantomData<keyword::As>,
    pub name: literal::Ident,
}

/// Table alias in DELETE FROM: either `AS alias` or bare `alias`.
#[derive(Debug, Clone)]
pub enum TableAlias {
    WithAs(AsAlias),
    Bare(literal::Ident),
}

impl AsNodeKey for TableAlias {}

impl Visit for TableAlias {
    fn visit<V: Visitor>(&self, _visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        ControlFlow::Continue(())
    }
}

impl TableAlias {
    /// Returns the alias name regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            TableAlias::WithAs(a) => &a.name.0,
            TableAlias::Bare(ident) => &ident.0,
        }
    }
}

/// DELETE FROM statement: `DELETE FROM table [alias] [WHERE expr]`.
///
/// Manual Parse impl required because `Option<TableAlias>` doesn't work with
/// derive: the bare alias variant uses `Ident` whose regex pattern matches
/// SQL keywords, but whose postcondition rejects them. Since `Option<T>`
/// propagates parse errors when peek succeeds but parse fails (postcondition
/// rejection), `DELETE FROM t WHERE ...` would error instead of treating
/// `WHERE` as the start of the WHERE clause.
///
/// To eliminate this manual impl, recursa would need either:
/// 1. `Option<T>` to return `None` on postcondition failure (try-parse), or
/// 2. Negative lookahead support in the regex crate for scan patterns.
#[derive(Debug, Visit)]
pub struct DeleteStmt {
    pub _delete: PhantomData<keyword::Delete>,
    pub _from: PhantomData<keyword::From>,
    pub table_name: literal::Ident,
    pub alias: Option<TableAlias>,
    pub where_clause: Option<WhereClause>,
}

impl<'input> Parse<'input> for DeleteStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::Delete::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        keyword::Delete::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let _delete = PhantomData::<keyword::Delete>::parse(input, rules)?;
        R::consume_ignored(input);
        let _from = PhantomData::<keyword::From>::parse(input, rules)?;
        R::consume_ignored(input);
        let table_name = literal::Ident::parse(input, rules)?;
        R::consume_ignored(input);

        // Try AS alias first, then bare alias (Ident), falling back to None.
        // For the bare alias case, we fork the input and attempt Ident::parse.
        // If the postcondition rejects a keyword (e.g., WHERE), we discard the
        // fork and return None, leaving the input cursor unchanged.
        let alias = if AsAlias::peek(input, rules) {
            let a = AsAlias::parse(input, rules)?;
            R::consume_ignored(input);
            Some(TableAlias::WithAs(a))
        } else {
            let mut fork = input.fork();
            if literal::Ident::peek(&fork, rules) {
                match literal::Ident::parse(&mut fork, rules) {
                    Ok(ident) => {
                        // Commit the fork's position back to input
                        input.advance(fork.cursor() - input.cursor());
                        R::consume_ignored(input);
                        Some(TableAlias::Bare(ident))
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        let where_clause = Option::<WhereClause>::parse(input, rules)?;

        Ok(DeleteStmt {
            _delete,
            _from,
            table_name,
            alias,
            where_clause,
        })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::delete::{DeleteStmt, TableAlias};
    use crate::rules::SqlRules;

    #[test]
    fn parse_delete_simple() {
        let mut input = Input::new("DELETE FROM delete_test WHERE a > 25");
        let stmt = DeleteStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "delete_test");
        assert!(stmt.alias.is_none());
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_delete_with_as_alias() {
        let mut input = Input::new("DELETE FROM delete_test AS dt WHERE dt.a > 75");
        let stmt = DeleteStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "delete_test");
        assert!(matches!(stmt.alias, Some(TableAlias::WithAs(_))));
        assert_eq!(stmt.alias.as_ref().unwrap().name(), "dt");
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_delete_with_bare_alias() {
        let mut input = Input::new("DELETE FROM delete_test dt WHERE delete_test.a > 25");
        let stmt = DeleteStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "delete_test");
        assert!(matches!(stmt.alias, Some(TableAlias::Bare(_))));
        assert_eq!(stmt.alias.as_ref().unwrap().name(), "dt");
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_delete_no_where() {
        let mut input = Input::new("DELETE FROM t");
        let stmt = DeleteStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "t");
        assert!(stmt.alias.is_none());
        assert!(stmt.where_clause.is_none());
        assert!(input.is_empty());
    }
}
