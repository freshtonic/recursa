/// CREATE TABLE statement AST.
use std::ops::ControlFlow;

use recursa::seq::Seq;
use recursa::{AsNodeKey, Break, Input, Parse, ParseError, ParseRules, Visit, Visitor};

use crate::ast::expr::TypeName;
use crate::rules::SqlRules;
use crate::tokens;

/// A column definition: `name type`.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: tokens::Ident,
    pub type_name: TypeName,
}

/// CREATE TABLE statement.
#[derive(Debug)]
pub struct CreateTableStmt {
    pub create_kw: tokens::Create,
    pub table_kw: tokens::Table,
    pub name: tokens::Ident,
    pub lparen: tokens::LParen,
    pub columns: Seq<ColumnDef, tokens::Comma>,
    pub rparen: tokens::RParen,
}

// --- Parse implementations ---

impl AsNodeKey for ColumnDef {}
impl Visit for ColumnDef {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.name.visit(visitor)?;
        self.type_name.visit(visitor)?;
        visitor.exit(self)
    }
}

/// Parse a type name for column definitions.
fn parse_column_type(input: &mut Input<'_>) -> Result<TypeName, ParseError> {
    SqlRules::consume_ignored(input);
    if <tokens::Boolean as Parse>::peek(input, &SqlRules) {
        <tokens::Boolean as Parse>::parse(input, &SqlRules)?;
        return Ok(TypeName::Boolean);
    }
    if <tokens::Bool as Parse>::peek(input, &SqlRules) {
        <tokens::Bool as Parse>::parse(input, &SqlRules)?;
        return Ok(TypeName::Bool);
    }
    if <tokens::Text as Parse>::peek(input, &SqlRules) {
        <tokens::Text as Parse>::parse(input, &SqlRules)?;
        return Ok(TypeName::Text);
    }
    if <tokens::Int as Parse>::peek(input, &SqlRules) {
        <tokens::Int as Parse>::parse(input, &SqlRules)?;
        return Ok(TypeName::Int);
    }
    if <tokens::Ident as Parse>::peek(input, &SqlRules) {
        let ident = <tokens::Ident as Parse>::parse(input, &SqlRules)?;
        return Ok(TypeName::Ident(ident.0));
    }
    Err(ParseError::new(
        input.source().to_string(),
        input.cursor()..input.cursor(),
        "column type name",
    ))
}

impl<'input> Parse<'input> for ColumnDef {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Ident as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Ident as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let name = <tokens::Ident as Parse>::parse(input, &SqlRules)?;
        let type_name = parse_column_type(input)?;
        Ok(ColumnDef { name, type_name })
    }
}

impl AsNodeKey for CreateTableStmt {}
impl Visit for CreateTableStmt {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.create_kw.visit(visitor)?;
        self.table_kw.visit(visitor)?;
        self.name.visit(visitor)?;
        self.columns.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for CreateTableStmt {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Create as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Create as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let create_kw = <tokens::Create as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let table_kw = <tokens::Table as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let name = <tokens::Ident as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let lparen = <tokens::LParen as Parse>::parse(input, &SqlRules)?;
        let columns = Seq::<ColumnDef, tokens::Comma>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let rparen = <tokens::RParen as Parse>::parse(input, &SqlRules)?;
        Ok(CreateTableStmt {
            create_kw,
            table_kw,
            name,
            lparen,
            columns,
            rparen,
        })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_table::CreateTableStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_table_single_column() {
        let mut input = Input::new("CREATE TABLE BOOLTBL1 (f1 bool)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL1");
        assert_eq!(stmt.columns.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_multiple_columns() {
        let mut input = Input::new("CREATE TABLE BOOLTBL3 (d text, b bool, o int)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL3");
        assert_eq!(stmt.columns.len(), 3);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_boolean_type() {
        let mut input = Input::new("CREATE TABLE t (f1 boolean)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns.len(), 1);
    }
}
