/// INSERT INTO statement AST.
use recursa::seq::Seq;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens;

/// INSERT INTO statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertStmt {
    pub insert_kw: tokens::Insert,
    pub into_kw: tokens::Into,
    pub table_name: tokens::Ident,
    pub columns: Option<ColumnList>,
    pub values_kw: tokens::Values,
    pub values_lparen: tokens::LParen,
    pub values: Seq<Expr, tokens::Comma>,
    pub values_rparen: tokens::RParen,
}

/// Optional column list: `(col1, col2, ...)`.
///
/// Manual Parse (recursa gap): Option<ColumnList> needs ColumnList::peek to
/// return false when the parens contain expressions (VALUES clause), not
/// identifiers. A derived ColumnList's first_pattern starts with `\(` which
/// also matches the VALUES clause. Option<T> propagates parse errors when
/// peek succeeds, so it can't recover from the misidentification.
#[derive(Debug, Visit)]
pub struct ColumnList {
    pub lparen: tokens::LParen,
    pub columns: Seq<tokens::Ident, tokens::Comma>,
    pub rparen: tokens::RParen,
}

// --- Parse implementations ---

impl<'input> Parse<'input> for ColumnList {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        tokens::LParen::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        // Column list starts with '(' and the first token inside should be an identifier
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        if !tokens::LParen::peek(&fork, &SqlRules) {
            return false;
        }
        // Peek further: after '(' we need an identifier (not an expression like VALUES has)
        let Ok(_) = tokens::LParen::parse(&mut fork, &SqlRules) else {
            return false;
        };
        SqlRules::consume_ignored(&mut fork);
        tokens::Ident::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let lparen = tokens::LParen::parse(input, &SqlRules)?;
        let columns = Seq::<tokens::Ident, tokens::Comma>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let rparen = tokens::RParen::parse(input, &SqlRules)?;
        Ok(ColumnList {
            lparen,
            columns,
            rparen,
        })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::insert::InsertStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_insert_with_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't')");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "BOOLTBL1");
        assert!(stmt.columns.is_some());
        assert_eq!(stmt.columns.as_ref().unwrap().columns.len(), 1);
        assert_eq!(stmt.values.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_multiple_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL3 (d, b, o) VALUES ('true', true, 1)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns.as_ref().unwrap().columns.len(), 3);
        assert_eq!(stmt.values.len(), 3);
    }

    #[test]
    fn parse_insert_without_columns() {
        let mut input = Input::new("INSERT INTO booltbl4 VALUES (false, true, null)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.columns.is_none());
        assert_eq!(stmt.values.len(), 3);
    }
}
