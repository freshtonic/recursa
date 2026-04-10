//! SQL printer -- converts AST back to valid SQL text.
//!
//! Does not attempt to reproduce original formatting. Produces semantically
//! equivalent SQL with consistent casing (uppercase keywords, lowercase for
//! identifiers as-is from the AST).

use crate::ast::create_table::CreateTableStmt;
use crate::ast::drop_table::DropTableStmt;
use crate::ast::expr::{
    BoolTestKind, Expr, FuncCall, ParenExpr, QualifiedRef, QualifiedWildcard, TypeCastFunc,
    TypeName,
};
use crate::ast::insert::InsertStmt;
use crate::ast::select::{OrderByClause, SelectItem, SelectStmt, TableRef};
use crate::ast::{PsqlCommand, Statement};

/// Print a sequence of psql commands back to SQL text.
pub fn print_commands(commands: &[PsqlCommand]) -> String {
    let mut output = String::new();
    for cmd in commands {
        match cmd {
            PsqlCommand::Directive(d) => {
                output.push_str(d);
                output.push('\n');
            }
            PsqlCommand::Statement(stmt, _semi) => {
                print_statement_to(&mut output, stmt);
                output.push_str(";\n");
            }
        }
    }
    output
}

/// Print a single statement to a string (without trailing semicolon).
pub fn print_statement(stmt: &Statement) -> String {
    let mut output = String::new();
    print_statement_to(&mut output, stmt);
    output
}

fn print_statement_to(output: &mut String, stmt: &Statement) {
    match stmt {
        Statement::Select(s) => print_select(output, s),
        Statement::CreateTable(s) => print_create_table(output, s),
        Statement::Insert(s) => print_insert(output, s),
        Statement::DropTable(s) => print_drop_table(output, s),
    }
}

// --- SELECT ---

fn print_select(output: &mut String, stmt: &SelectStmt) {
    output.push_str("SELECT ");
    for (i, item) in stmt.items.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        print_select_item(output, item);
    }
    if let Some(from) = &stmt.from_clause {
        output.push_str(" FROM ");
        for (i, table_ref) in from.tables.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            print_table_ref(output, table_ref);
        }
    }
    if let Some(where_clause) = &stmt.where_clause {
        output.push_str(" WHERE ");
        print_expr(output, &where_clause.condition);
    }
    if let Some(order_by) = &stmt.order_by {
        print_order_by(output, order_by);
    }
}

fn print_select_item(output: &mut String, item: &SelectItem) {
    print_expr(output, &item.expr);
    if let Some(alias) = &item.alias {
        output.push_str(" AS ");
        output.push_str(&alias.name);
    }
}

fn print_table_ref(output: &mut String, table_ref: &TableRef) {
    match table_ref {
        TableRef::Table(ident) => output.push_str(&ident.0),
        TableRef::FuncCall { name, args } => {
            output.push_str(&name.0);
            output.push('(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                print_expr(output, arg);
            }
            output.push(')');
        }
    }
}

fn print_order_by(output: &mut String, order_by: &OrderByClause) {
    output.push_str(" ORDER BY ");
    for (i, item) in order_by.items.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        print_expr(output, item);
    }
}

// --- CREATE TABLE ---

fn print_create_table(output: &mut String, stmt: &CreateTableStmt) {
    output.push_str("CREATE TABLE ");
    output.push_str(&stmt.name.0);
    output.push_str(" (");
    for (i, col) in stmt.columns.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        output.push_str(&col.name.0);
        output.push(' ');
        print_type_name(output, &col.type_name);
    }
    output.push(')');
}

// --- INSERT ---

fn print_insert(output: &mut String, stmt: &InsertStmt) {
    output.push_str("INSERT INTO ");
    output.push_str(&stmt.table_name.0);
    if let Some(col_list) = &stmt.columns {
        output.push_str(" (");
        for (i, col) in col_list.columns.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&col.0);
        }
        output.push(')');
    }
    output.push_str(" VALUES (");
    for (i, val) in stmt.values.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        print_expr(output, val);
    }
    output.push(')');
}

// --- DROP TABLE ---

fn print_drop_table(output: &mut String, stmt: &DropTableStmt) {
    output.push_str("DROP TABLE ");
    output.push_str(&stmt.name.0);
}

// --- Expressions ---

fn print_expr(output: &mut String, expr: &Expr) {
    match expr {
        Expr::IntegerLit(lit) => output.push_str(&lit.0),
        Expr::StringLit(lit) => output.push_str(&lit.0),
        Expr::BoolTrue(_) => output.push_str("TRUE"),
        Expr::BoolFalse(_) => output.push_str("FALSE"),
        Expr::Null(_) => output.push_str("NULL"),
        Expr::ColumnRef(ident) => output.push_str(&ident.0),
        Expr::QualRef(QualifiedRef { table, column, .. }) => {
            output.push_str(&table.0);
            output.push('.');
            output.push_str(&column.0);
        }
        Expr::QualWild(QualifiedWildcard { table, .. }) => {
            output.push_str(&table.0);
            output.push_str(".*");
        }
        Expr::Star(_) => output.push('*'),
        Expr::Func(FuncCall { name, args, .. }) => {
            output.push_str(&name.0);
            output.push('(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                print_expr(output, arg);
            }
            output.push(')');
        }
        Expr::Paren(ParenExpr { inner, .. }) => {
            output.push('(');
            print_expr(output, inner);
            output.push(')');
        }
        Expr::CastFunc(TypeCastFunc { type_name, value }) => {
            print_type_name(output, type_name);
            output.push(' ');
            output.push_str(&value.0);
        }
        Expr::Not(_, operand) => {
            output.push_str("NOT ");
            // Parenthesize if the operand is an infix op to avoid ambiguity
            if is_infix(operand) {
                output.push('(');
                print_expr(output, operand);
                output.push(')');
            } else {
                print_expr(output, operand);
            }
        }
        Expr::Or(left, _, right)
        | Expr::And(left, _, right)
        | Expr::Eq(left, _, right)
        | Expr::Neq(left, _, right)
        | Expr::Lt(left, _, right)
        | Expr::Gt(left, _, right)
        | Expr::Lte(left, _, right)
        | Expr::Gte(left, _, right) => {
            print_binop_operand(output, left);
            output.push(' ');
            print_infix_op(output, expr);
            output.push(' ');
            print_binop_operand(output, right);
        }
        Expr::Cast(inner, _, type_name) => {
            print_expr(output, inner);
            output.push_str("::");
            print_type_name(output, type_name);
        }
        Expr::BoolTest(inner, _, kind) => {
            print_expr(output, inner);
            match kind {
                BoolTestKind::IsTrue(_) => output.push_str(" IS TRUE"),
                BoolTestKind::IsNotTrue(_) => output.push_str(" IS NOT TRUE"),
                BoolTestKind::IsFalse(_) => output.push_str(" IS FALSE"),
                BoolTestKind::IsNotFalse(_) => output.push_str(" IS NOT FALSE"),
                BoolTestKind::IsUnknown(_) => output.push_str(" IS UNKNOWN"),
                BoolTestKind::IsNotUnknown(_) => output.push_str(" IS NOT UNKNOWN"),
                BoolTestKind::IsNull(_) => output.push_str(" IS NULL"),
                BoolTestKind::IsNotNull(_) => output.push_str(" IS NOT NULL"),
            }
        }
    }
}

/// Returns true if the expression is an infix binary operation.
fn is_infix(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Or(..)
            | Expr::And(..)
            | Expr::Eq(..)
            | Expr::Neq(..)
            | Expr::Lt(..)
            | Expr::Gt(..)
            | Expr::Lte(..)
            | Expr::Gte(..)
    )
}

/// Print the operator keyword/symbol for an infix expression.
fn print_infix_op(output: &mut String, expr: &Expr) {
    match expr {
        Expr::And(..) => output.push_str("AND"),
        Expr::Or(..) => output.push_str("OR"),
        Expr::Eq(..) => output.push('='),
        Expr::Neq(..) => output.push_str("<>"),
        Expr::Lt(..) => output.push('<'),
        Expr::Gt(..) => output.push('>'),
        Expr::Lte(..) => output.push_str("<="),
        Expr::Gte(..) => output.push_str(">="),
        _ => {}
    }
}

/// Print a binary operator operand, parenthesizing if it is itself an infix op.
fn print_binop_operand(output: &mut String, expr: &Expr) {
    if is_infix(expr) {
        output.push('(');
        print_expr(output, expr);
        output.push(')');
    } else {
        print_expr(output, expr);
    }
}

fn print_type_name(output: &mut String, type_name: &TypeName) {
    match type_name {
        TypeName::Bool(_) => output.push_str("bool"),
        TypeName::Boolean(_) => output.push_str("boolean"),
        TypeName::Text(_) => output.push_str("text"),
        TypeName::Int(_) => output.push_str("int"),
        TypeName::Ident(ident) => output.push_str(&ident.0),
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::{Statement, parse_sql_file};
    use crate::printer::{print_commands, print_statement};
    use crate::rules::SqlRules;

    /// Helper: parse a statement and print it back.
    fn round_trip_stmt(sql: &str) -> String {
        let mut input = Input::new(sql);
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        print_statement(&stmt)
    }

    // --- Basic statement printing ---

    #[test]
    fn print_select_literal() {
        let result = round_trip_stmt("SELECT 1 AS one");
        assert_eq!(result, "SELECT 1 AS one");
    }

    #[test]
    fn print_select_star() {
        let result = round_trip_stmt("SELECT *");
        assert_eq!(result, "SELECT *");
    }

    #[test]
    fn print_select_from_where() {
        let result = round_trip_stmt("SELECT f1 FROM BOOLTBL1 WHERE f1 = true");
        assert_eq!(result, "SELECT f1 FROM BOOLTBL1 WHERE f1 = TRUE");
    }

    #[test]
    fn print_select_qualified_wildcard() {
        let result = round_trip_stmt("SELECT BOOLTBL1.* FROM BOOLTBL1");
        assert_eq!(result, "SELECT BOOLTBL1.* FROM BOOLTBL1");
    }

    #[test]
    fn print_select_multiple_tables() {
        let result = round_trip_stmt(
            "SELECT BOOLTBL1.*, BOOLTBL2.* FROM BOOLTBL1, BOOLTBL2 WHERE f1 = true",
        );
        assert_eq!(
            result,
            "SELECT BOOLTBL1.*, BOOLTBL2.* FROM BOOLTBL1, BOOLTBL2 WHERE f1 = TRUE"
        );
    }

    #[test]
    fn print_select_order_by() {
        let result = round_trip_stmt(
            "SELECT BOOLTBL1.*, BOOLTBL2.* FROM BOOLTBL1, BOOLTBL2 ORDER BY BOOLTBL1.f1, BOOLTBL2.f1",
        );
        assert!(result.contains("ORDER BY BOOLTBL1.f1, BOOLTBL2.f1"));
    }

    // --- Expression printing ---

    #[test]
    fn print_bool_true_false_null() {
        assert_eq!(round_trip_stmt("SELECT true"), "SELECT TRUE");
        assert_eq!(round_trip_stmt("SELECT false"), "SELECT FALSE");
        assert_eq!(round_trip_stmt("SELECT null"), "SELECT NULL");
    }

    #[test]
    fn print_string_literal() {
        let result = round_trip_stmt("SELECT 'hello'");
        assert_eq!(result, "SELECT 'hello'");
    }

    #[test]
    fn print_function_call() {
        let result = round_trip_stmt("SELECT pg_input_is_valid('true', 'bool')");
        assert_eq!(result, "SELECT pg_input_is_valid('true', 'bool')");
    }

    #[test]
    fn print_type_cast_func_style() {
        let result = round_trip_stmt("SELECT bool 't'");
        assert_eq!(result, "SELECT bool 't'");
    }

    #[test]
    fn print_type_cast_colon_colon() {
        let result = round_trip_stmt("SELECT 0::boolean");
        assert_eq!(result, "SELECT 0::boolean");
    }

    #[test]
    fn print_chained_cast() {
        let result = round_trip_stmt("SELECT 'TrUe'::text::boolean");
        assert_eq!(result, "SELECT 'TrUe'::text::boolean");
    }

    #[test]
    fn print_not_expr() {
        let result = round_trip_stmt("SELECT NOT true");
        assert_eq!(result, "SELECT NOT TRUE");
    }

    #[test]
    fn print_binary_and() {
        let result = round_trip_stmt("SELECT true AND false");
        assert_eq!(result, "SELECT TRUE AND FALSE");
    }

    #[test]
    fn print_binary_or_with_parens_for_subexprs() {
        // a AND b OR c -> (a AND b) OR c due to precedence
        // The printer parenthesizes binary sub-expressions
        let result = round_trip_stmt("SELECT true AND false OR true");
        assert_eq!(result, "SELECT (TRUE AND FALSE) OR TRUE");
    }

    #[test]
    fn print_is_true() {
        let result = round_trip_stmt("SELECT f1 FROM t WHERE f1 IS TRUE");
        assert!(result.contains("f1 IS TRUE"));
    }

    #[test]
    fn print_is_not_false() {
        let result = round_trip_stmt("SELECT f1 FROM t WHERE f1 IS NOT FALSE");
        assert!(result.contains("f1 IS NOT FALSE"));
    }

    #[test]
    fn print_bool_cast_or() {
        let result = round_trip_stmt("SELECT bool 't' or bool 'f' AS true");
        // Should produce valid SQL with OR
        assert!(result.contains("OR"));
        assert!(result.contains("AS true"));
    }

    #[test]
    fn print_parenthesized_expr() {
        let result = round_trip_stmt("SELECT (1)");
        assert_eq!(result, "SELECT (1)");
    }

    #[test]
    fn print_qualified_column_ref() {
        let result = round_trip_stmt("SELECT BOOLTBL1.f1 FROM BOOLTBL1");
        assert_eq!(result, "SELECT BOOLTBL1.f1 FROM BOOLTBL1");
    }

    // --- CREATE TABLE ---

    #[test]
    fn print_create_table() {
        let result = round_trip_stmt("CREATE TABLE BOOLTBL1 (f1 bool)");
        assert_eq!(result, "CREATE TABLE BOOLTBL1 (f1 bool)");
    }

    #[test]
    fn print_create_table_multiple_columns() {
        let result = round_trip_stmt("CREATE TABLE BOOLTBL3 (d text, b bool, o int)");
        assert_eq!(result, "CREATE TABLE BOOLTBL3 (d text, b bool, o int)");
    }

    // --- INSERT ---

    #[test]
    fn print_insert_with_columns() {
        let result = round_trip_stmt("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't')");
        assert_eq!(result, "INSERT INTO BOOLTBL1 (f1) VALUES (bool 't')");
    }

    #[test]
    fn print_insert_without_columns() {
        let result = round_trip_stmt("INSERT INTO booltbl4 VALUES (false, true, null)");
        assert_eq!(result, "INSERT INTO booltbl4 VALUES (FALSE, TRUE, NULL)");
    }

    // --- DROP TABLE ---

    #[test]
    fn print_drop_table() {
        let result = round_trip_stmt("DROP TABLE BOOLTBL1");
        assert_eq!(result, "DROP TABLE BOOLTBL1");
    }

    // --- Commands (directives + statements) ---

    #[test]
    fn print_directive() {
        let sql = "\\pset null '(null)'\nSELECT 1;\n";
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        let output = print_commands(&commands);
        assert!(output.starts_with("\\pset null '(null)'\n"));
        assert!(output.contains("SELECT 1;\n"));
    }

    #[test]
    fn print_multiple_commands() {
        let sql = "SELECT 1;\nSELECT 2;\n";
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        let output = print_commands(&commands);
        assert_eq!(output, "SELECT 1;\nSELECT 2;\n");
    }

    // --- Full fixture round-trip ---

    #[test]
    fn round_trip_boolean_sql_parses_and_prints() {
        let sql = std::fs::read_to_string("fixtures/sql/boolean.sql")
            .expect("boolean.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        let printed = print_commands(&commands);

        // Verify the printed output is non-empty and contains expected content
        assert!(!printed.is_empty());
        assert!(printed.contains("CREATE TABLE"));
        assert!(printed.contains("INSERT INTO"));
        assert!(printed.contains("SELECT"));
        assert!(printed.contains("DROP TABLE"));

        // Verify the printed output can be re-parsed
        let mut input2 = Input::new(&printed);
        let commands2 = parse_sql_file(&mut input2).unwrap();
        assert_eq!(
            commands.len(),
            commands2.len(),
            "re-parsed command count should match"
        );
    }

    #[test]
    fn print_select_star_from_function() {
        let result = round_trip_stmt("SELECT * FROM pg_input_error_info('junk', 'bool')");
        assert_eq!(result, "SELECT * FROM pg_input_error_info('junk', 'bool')");
    }

    #[test]
    fn print_comparison_operators() {
        assert_eq!(
            round_trip_stmt("SELECT f1 FROM t WHERE f1 <> true"),
            "SELECT f1 FROM t WHERE f1 <> TRUE"
        );
        assert_eq!(
            round_trip_stmt("SELECT f1 FROM t WHERE f1 > false"),
            "SELECT f1 FROM t WHERE f1 > FALSE"
        );
        assert_eq!(
            round_trip_stmt("SELECT f1 FROM t WHERE f1 >= false"),
            "SELECT f1 FROM t WHERE f1 >= FALSE"
        );
        assert_eq!(
            round_trip_stmt("SELECT f1 FROM t WHERE f1 < true"),
            "SELECT f1 FROM t WHERE f1 < TRUE"
        );
        assert_eq!(
            round_trip_stmt("SELECT f1 FROM t WHERE f1 <= true"),
            "SELECT f1 FROM t WHERE f1 <= TRUE"
        );
    }

    #[test]
    fn print_is_unknown() {
        let result = round_trip_stmt("SELECT b FROM t WHERE b IS UNKNOWN");
        assert!(result.contains("b IS UNKNOWN"));
    }

    #[test]
    fn print_is_not_unknown() {
        let result = round_trip_stmt("SELECT b FROM t WHERE b IS NOT UNKNOWN");
        assert!(result.contains("b IS NOT UNKNOWN"));
    }

    #[test]
    fn print_is_null() {
        let result = round_trip_stmt("SELECT b FROM t WHERE b IS NULL");
        assert!(result.contains("b IS NULL"));
    }

    #[test]
    fn print_is_not_null() {
        let result = round_trip_stmt("SELECT b FROM t WHERE b IS NOT NULL");
        assert!(result.contains("b IS NOT NULL"));
    }
}
