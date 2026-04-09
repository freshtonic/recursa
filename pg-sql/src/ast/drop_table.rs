/// DROP TABLE statement AST.
use std::ops::ControlFlow;

use recursa::{AsNodeKey, Break, Input, Parse, ParseError, ParseRules, Visit, Visitor};

use crate::rules::SqlRules;
use crate::tokens;

/// DROP TABLE statement.
#[derive(Debug, Clone)]
pub struct DropTableStmt {
    pub drop_kw: tokens::Drop,
    pub table_kw: tokens::Table,
    pub name: tokens::Ident,
}

impl AsNodeKey for DropTableStmt {}
impl Visit for DropTableStmt {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.drop_kw.visit(visitor)?;
        self.table_kw.visit(visitor)?;
        self.name.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for DropTableStmt {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Drop as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Drop as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let drop_kw = <tokens::Drop as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let table_kw = <tokens::Table as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let name = <tokens::Ident as Parse>::parse(input, &SqlRules)?;
        Ok(DropTableStmt {
            drop_kw,
            table_kw,
            name,
        })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::drop_table::DropTableStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_drop_table() {
        let mut input = Input::new("DROP TABLE BOOLTBL1");
        let stmt = DropTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL1");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_table_lowercase() {
        let mut input = Input::new("drop table my_table");
        let stmt = DropTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "my_table");
    }
}
