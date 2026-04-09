pub mod create_table;
pub mod drop_table;
pub mod expr;
pub mod insert;
pub mod select;

use std::ops::ControlFlow;

use recursa::{AsNodeKey, Break, Input, Parse, ParseError, ParseRules, Visit, Visitor};

use crate::rules::SqlRules;
use crate::tokens;

use self::create_table::CreateTableStmt;
use self::drop_table::DropTableStmt;
use self::insert::InsertStmt;
use self::select::SelectStmt;

/// Top-level SQL statement.
#[derive(Debug)]
pub enum Statement {
    Select(SelectStmt),
    CreateTable(CreateTableStmt),
    Insert(InsertStmt),
    DropTable(DropTableStmt),
}

/// A command in a psql input file: either a SQL statement or a psql directive.
#[derive(Debug)]
pub enum PsqlCommand {
    /// A SQL statement followed by a semicolon.
    Statement(Statement, tokens::Semi),
    /// A psql directive (e.g., `\pset null '(null)'`).
    Directive(String),
}

// --- Parse implementations ---

impl AsNodeKey for Statement {}
impl Visit for Statement {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        match self {
            Statement::Select(s) => s.visit(visitor)?,
            Statement::CreateTable(s) => s.visit(visitor)?,
            Statement::Insert(s) => s.visit(visitor)?,
            Statement::DropTable(s) => s.visit(visitor)?,
        }
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for Statement {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        // Statements start with SELECT, CREATE, INSERT, DROP
        r"(?i:SELECT|CREATE|INSERT|DROP)\b"
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        SelectStmt::peek(&fork, &SqlRules)
            || CreateTableStmt::peek(&fork, &SqlRules)
            || InsertStmt::peek(&fork, &SqlRules)
            || DropTableStmt::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);

        // Disambiguate by leading keyword.
        // CREATE and DROP need two-token lookahead (CREATE TABLE, DROP TABLE)
        // but since we only support TABLE variants, just peek for the keyword.
        if SelectStmt::peek(input, &SqlRules) {
            return Ok(Statement::Select(SelectStmt::parse(input, &SqlRules)?));
        }
        if CreateTableStmt::peek(input, &SqlRules) {
            return Ok(Statement::CreateTable(CreateTableStmt::parse(
                input, &SqlRules,
            )?));
        }
        if InsertStmt::peek(input, &SqlRules) {
            return Ok(Statement::Insert(InsertStmt::parse(input, &SqlRules)?));
        }
        if DropTableStmt::peek(input, &SqlRules) {
            return Ok(Statement::DropTable(DropTableStmt::parse(
                input, &SqlRules,
            )?));
        }

        Err(ParseError::new(
            input.source().to_string(),
            input.cursor()..input.cursor(),
            "SQL statement (SELECT, CREATE TABLE, INSERT, DROP TABLE)",
        ))
    }
}

impl AsNodeKey for PsqlCommand {}
impl Visit for PsqlCommand {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        match self {
            PsqlCommand::Statement(stmt, semi) => {
                stmt.visit(visitor)?;
                semi.visit(visitor)?;
            }
            PsqlCommand::Directive(_) => {}
        }
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for PsqlCommand {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        // Can start with '\' for directives or a statement keyword
        r"[\\a-zA-Z]"
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        fork.remaining().starts_with('\\') || Statement::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);

        // Psql directives start with '\'
        if input.remaining().starts_with('\\') {
            let remaining = input.remaining();
            let end = remaining.find('\n').unwrap_or(remaining.len());
            let directive = remaining[..end].to_string();
            input.advance(end);
            // Also consume the newline if present
            if input.remaining().starts_with('\n') {
                input.advance(1);
            }
            return Ok(PsqlCommand::Directive(directive));
        }

        let stmt = Statement::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let semi = <tokens::Semi as Parse>::parse(input, &SqlRules)?;
        Ok(PsqlCommand::Statement(stmt, semi))
    }
}

/// Parse a complete SQL file into a list of commands.
pub fn parse_sql_file(input: &mut Input<'_>) -> Result<Vec<PsqlCommand>, ParseError> {
    let mut commands = Vec::new();
    loop {
        SqlRules::consume_ignored(input);
        if input.is_empty() {
            break;
        }
        if !PsqlCommand::peek(input, &SqlRules) {
            break;
        }
        commands.push(PsqlCommand::parse(input, &SqlRules)?);
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use super::*;

    #[test]
    fn parse_statement_select() {
        let mut input = Input::new("SELECT 1 AS one");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::Select(_)));
    }

    #[test]
    fn parse_statement_create_table() {
        let mut input = Input::new("CREATE TABLE t (f1 bool)");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::CreateTable(_)));
    }

    #[test]
    fn parse_statement_insert() {
        let mut input = Input::new("INSERT INTO t (f1) VALUES (true)");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::Insert(_)));
    }

    #[test]
    fn parse_statement_drop_table() {
        let mut input = Input::new("DROP TABLE t");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::DropTable(_)));
    }

    #[test]
    fn parse_psql_command_statement() {
        let mut input = Input::new("SELECT 1;");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_, _)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_psql_command_directive() {
        let mut input = Input::new("\\pset null '(null)'\n");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        match cmd {
            PsqlCommand::Directive(d) => assert_eq!(d, "\\pset null '(null)'"),
            _ => panic!("expected directive"),
        }
    }

    #[test]
    fn parse_multiple_commands() {
        let sql = "SELECT 1;\n\\pset null '(null)'\nSELECT 2;\n";
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert_eq!(commands.len(), 3);
        assert!(matches!(commands[0], PsqlCommand::Statement(_, _)));
        assert!(matches!(commands[1], PsqlCommand::Directive(_)));
        assert!(matches!(commands[2], PsqlCommand::Statement(_, _)));
    }

    #[test]
    fn parse_select_with_where_and_bool_test() {
        let mut input = Input::new("SELECT f1 FROM BOOLTBL1 WHERE f1 IS TRUE;");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_, _)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_full_insert_with_type_cast() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't');");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_, _)));
    }

    #[test]
    fn parse_create_drop_sequence() {
        let sql = "CREATE TABLE t (f1 bool);\nDROP TABLE t;\n";
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn parse_boolean_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/boolean.sql")
            .expect("boolean.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();

        // boolean.sql has many statements -- verify we parsed a reasonable number
        assert!(
            commands.len() > 50,
            "expected >50 commands, got {}",
            commands.len()
        );

        // Verify we consumed all input
        assert!(
            input.is_empty(),
            "leftover input at offset {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }
}
