/// CREATE TABLE statement AST.
use recursa::seq::Seq;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::expr::TypeName;
use crate::rules::SqlRules;
use crate::tokens;

/// A column definition: `name type`.
///
/// Manual Parse: type name parsing uses keyword-to-variant dispatch
/// that the derive macro cannot express.
#[derive(Debug, Clone, Visit)]
pub struct ColumnDef {
    pub name: tokens::Ident,
    pub type_name: TypeName,
}

/// CREATE TABLE statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTableStmt {
    pub create_kw: tokens::Create,
    pub table_kw: tokens::Table,
    pub name: tokens::Ident,
    pub lparen: tokens::LParen,
    pub columns: Seq<ColumnDef, tokens::Comma>,
    pub rparen: tokens::RParen,
}

// --- Parse implementations ---

/// Parse a type name for column definitions.
fn parse_column_type(input: &mut Input<'_>) -> Result<TypeName, ParseError> {
    SqlRules::consume_ignored(input);
    if tokens::Boolean::peek(input, &SqlRules) {
        tokens::Boolean::parse(input, &SqlRules)?;
        return Ok(TypeName::Boolean);
    }
    if tokens::Bool::peek(input, &SqlRules) {
        tokens::Bool::parse(input, &SqlRules)?;
        return Ok(TypeName::Bool);
    }
    if tokens::Text::peek(input, &SqlRules) {
        tokens::Text::parse(input, &SqlRules)?;
        return Ok(TypeName::Text);
    }
    if tokens::Int::peek(input, &SqlRules) {
        tokens::Int::parse(input, &SqlRules)?;
        return Ok(TypeName::Int);
    }
    if tokens::Ident::peek(input, &SqlRules) {
        let ident = tokens::Ident::parse(input, &SqlRules)?;
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
        tokens::Ident::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        tokens::Ident::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let name = tokens::Ident::parse(input, &SqlRules)?;
        let type_name = parse_column_type(input)?;
        Ok(ColumnDef { name, type_name })
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
