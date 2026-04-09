/// INSERT INTO statement AST.
use std::ops::ControlFlow;

use recursa::seq::Seq;
use recursa::{AsNodeKey, Break, Input, Parse, ParseError, ParseRules, Visit, Visitor};

use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens;

/// INSERT INTO statement.
#[derive(Debug)]
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
#[derive(Debug)]
pub struct ColumnList {
    pub lparen: tokens::LParen,
    pub columns: Seq<tokens::Ident, tokens::Comma>,
    pub rparen: tokens::RParen,
}

// --- Parse implementations ---

impl AsNodeKey for ColumnList {}
impl Visit for ColumnList {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.columns.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for ColumnList {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::LParen as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        // Column list starts with '(' and the first token inside should be an identifier
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        if !<tokens::LParen as Parse>::peek(&fork, &SqlRules) {
            return false;
        }
        // Peek further: after '(' we need an identifier (not an expression like VALUES has)
        let Ok(_) = <tokens::LParen as Parse>::parse(&mut fork, &SqlRules) else {
            return false;
        };
        SqlRules::consume_ignored(&mut fork);
        <tokens::Ident as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let lparen = <tokens::LParen as Parse>::parse(input, &SqlRules)?;
        let columns = Seq::<tokens::Ident, tokens::Comma>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let rparen = <tokens::RParen as Parse>::parse(input, &SqlRules)?;
        Ok(ColumnList {
            lparen,
            columns,
            rparen,
        })
    }
}

impl AsNodeKey for InsertStmt {}
impl Visit for InsertStmt {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.insert_kw.visit(visitor)?;
        self.into_kw.visit(visitor)?;
        self.table_name.visit(visitor)?;
        self.columns.visit(visitor)?;
        self.values_kw.visit(visitor)?;
        self.values.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for InsertStmt {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Insert as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Insert as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let insert_kw = <tokens::Insert as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let into_kw = <tokens::Into as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let table_name = <tokens::Ident as Parse>::parse(input, &SqlRules)?;

        // Optional column list -- must check carefully to distinguish from VALUES
        let columns = if ColumnList::peek(input, &SqlRules) {
            Some(ColumnList::parse(input, &SqlRules)?)
        } else {
            None
        };

        SqlRules::consume_ignored(input);
        let values_kw = <tokens::Values as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let values_lparen = <tokens::LParen as Parse>::parse(input, &SqlRules)?;
        let values = Seq::<Expr, tokens::Comma>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let values_rparen = <tokens::RParen as Parse>::parse(input, &SqlRules)?;

        Ok(InsertStmt {
            insert_kw,
            into_kw,
            table_name,
            columns,
            values_kw,
            values_lparen,
            values,
            values_rparen,
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
