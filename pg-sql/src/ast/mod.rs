pub mod create_table;
pub mod drop_table;
pub mod expr;
pub mod insert;
pub mod select;

use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::rules::SqlRules;
use crate::tokens;

use self::create_table::CreateTableStmt;
use self::drop_table::DropTableStmt;
use self::insert::InsertStmt;
use self::select::SelectStmt;

/// Top-level SQL statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum Statement {
    Select(SelectStmt),
    CreateTable(CreateTableStmt),
    Insert(InsertStmt),
    DropTable(DropTableStmt),
}

/// A SQL statement followed by a semicolon.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TerminatedStatement {
    pub stmt: Statement,
    pub semi: tokens::Semi,
}

/// A psql directive: backslash followed by the rest of the line.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PsqlDirective {
    pub backslash: tokens::BackSlash,
    pub rest: tokens::RestOfLine,
}

/// A command in a psql input file: either a SQL statement or a psql directive.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum PsqlCommand {
    /// A psql directive (e.g., `\pset null '(null)'`).
    /// Listed first so `\` is checked before statement keywords.
    Directive(PsqlDirective),
    /// A SQL statement followed by a semicolon.
    Statement(TerminatedStatement),
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
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_psql_command_directive() {
        let mut input = Input::new("\\pset null '(null)'\n");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        match cmd {
            PsqlCommand::Directive(d) => assert_eq!(d.rest.0, "pset null '(null)'"),
            _ => panic!("expected directive"),
        }
    }

    #[test]
    fn parse_multiple_commands() {
        let sql = "SELECT 1;\n\\pset null '(null)'\nSELECT 2;\n";
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert_eq!(commands.len(), 3);
        assert!(matches!(commands[0], PsqlCommand::Statement(_)));
        assert!(matches!(commands[1], PsqlCommand::Directive(_)));
        assert!(matches!(commands[2], PsqlCommand::Statement(_)));
    }

    #[test]
    fn parse_select_with_where_and_bool_test() {
        let mut input = Input::new("SELECT f1 FROM BOOLTBL1 WHERE f1 IS TRUE;");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_full_insert_with_type_cast() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't');");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
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

        assert!(
            commands.len() > 50,
            "expected >50 commands, got {}",
            commands.len()
        );

        assert!(
            input.is_empty(),
            "leftover input at offset {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }
}
