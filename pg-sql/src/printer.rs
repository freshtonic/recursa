//! SQL printer -- converts AST back to valid SQL text.
//!
//! Does not attempt to reproduce original formatting. Produces semantically
//! equivalent SQL with consistent casing (uppercase keywords, lowercase for
//! identifiers as-is from the AST).

use crate::ast::analyze::AnalyzeStmt;
use crate::ast::create_function::{CreateFunctionStmt, DropFunctionStmt, ReturnType};
use crate::ast::create_index::{CreateIndexStmt, DropIndexStmt};
use crate::ast::create_table::{CreateTableBody, CreateTableStmt};
use crate::ast::delete::{DeleteStmt, TableAlias};
use crate::ast::drop_table::DropTableStmt;
use crate::ast::explain::ExplainStmt;
use crate::ast::expr::{
    BoolTestKind, Expr, FuncCall, ParenExpr, QualifiedRef, QualifiedWildcard, TypeCastFunc,
    TypeName,
};
use crate::ast::insert::InsertStmt;
use crate::ast::select::{
    LateralRef, NullsOrder, OrderByClause, OrderByItem, SelectItem, SelectStmt, SortDir,
    SubqueryRef, TableRef, UsingClause, ValuesBody,
};
use crate::ast::set_reset::{ResetStmt, SetSep, SetStmt, SetValue};
use crate::ast::values::{CompoundBody, CompoundQuery, TableStmt};
use crate::ast::{PsqlCommand, PsqlDirective, Statement, TerminatedStatement};

/// Print a sequence of psql commands back to SQL text.
pub fn print_commands(commands: &[PsqlCommand]) -> String {
    let mut output = String::new();
    for cmd in commands {
        match cmd {
            PsqlCommand::Directive(PsqlDirective { rest, .. }) => {
                output.push('\\');
                output.push_str(&rest.0);
                output.push('\n');
            }
            PsqlCommand::Statement(TerminatedStatement { stmt, .. }) => {
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
        Statement::Delete(s) => print_delete(output, s),
        Statement::DropTable(s) => print_drop_table(output, s),
        Statement::Set(s) => print_set(output, s),
        Statement::Reset(s) => print_reset(output, s),
        Statement::Analyze(s) => print_analyze(output, s),
        Statement::Explain(s) => print_explain(output, s),
        Statement::CreateIndex(s) => print_create_index(output, s),
        Statement::DropIndex(s) => print_drop_index(output, s),
        Statement::CreateFunction(s) => print_create_function(output, s),
        Statement::DropFunction(s) => print_drop_function(output, s),
        Statement::Values(s) => print_compound_query(output, s),
        Statement::Table(s) => print_table_stmt(output, s),
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
    if let Some(limit) = &stmt.limit {
        output.push_str(" LIMIT ");
        print_expr(output, &limit.count);
    }
    if let Some(offset) = &stmt.offset {
        output.push_str(" OFFSET ");
        print_expr(output, &offset.count);
    }
    if stmt.for_update.is_some() {
        output.push_str(" FOR UPDATE");
    }
}

fn print_select_item(output: &mut String, item: &SelectItem) {
    print_expr(output, &item.expr);
    if let Some(alias) = &item.alias {
        output.push_str(" AS ");
        output.push_str(&alias.name.0);
    }
}

fn print_table_ref(output: &mut String, table_ref: &TableRef) {
    match table_ref {
        TableRef::Table(plain) => {
            output.push_str(&plain.name.0);
            if let Some(alias) = &plain.alias {
                output.push(' ');
                output.push_str(&alias.0);
            }
        }
        TableRef::Func(func_call) => print_func_call(output, func_call),
        TableRef::Inherited(inh) => {
            output.push_str(&inh.name.0);
            output.push('*');
            if let Some(alias) = &inh.alias {
                output.push(' ');
                output.push_str(&alias.0);
            }
        }
        TableRef::Subquery(sub) => print_subquery_ref(output, sub),
        TableRef::Lateral(lat) => print_lateral_ref(output, lat),
    }
}

fn print_subquery_ref(output: &mut String, sub: &SubqueryRef) {
    output.push('(');
    print_select_body(output, &sub.query);
    output.push(')');
    print_table_alias(output, &sub.alias);
}

fn print_lateral_ref(output: &mut String, lat: &LateralRef) {
    output.push_str("LATERAL (");
    print_select_body(output, &lat.query);
    output.push(')');
    if let Some(alias) = &lat.alias {
        output.push(' ');
        output.push_str(&alias.0);
    }
}

fn print_table_alias(output: &mut String, alias: &crate::ast::select::TableAlias) {
    if alias._as.is_some() {
        output.push_str(" AS ");
    } else {
        output.push(' ');
    }
    output.push_str(&alias.name.0);
    if let Some(cols) = &alias.columns {
        output.push_str(" (");
        for (i, col) in cols.inner.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&col.0);
        }
        output.push(')');
    }
}

fn print_select_body(output: &mut String, body: &crate::ast::select::SelectBody) {
    match body {
        crate::ast::select::SelectBody::Select(s) => print_select(output, s),
        crate::ast::select::SelectBody::Values(v) => print_values_body(output, v),
    }
}

fn print_values_body(output: &mut String, v: &ValuesBody) {
    output.push_str("VALUES ");
    for (i, row) in v.rows.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        output.push('(');
        for (j, expr) in row.inner.iter().enumerate() {
            if j > 0 {
                output.push_str(", ");
            }
            print_expr(output, expr);
        }
        output.push(')');
    }
}

fn print_order_by(output: &mut String, order_by: &OrderByClause) {
    output.push_str(" ORDER BY ");
    for (i, item) in order_by.items.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        print_order_by_item(output, item);
    }
}

fn print_order_by_item(output: &mut String, item: &OrderByItem) {
    print_expr(output, &item.expr);
    if let Some(dir) = &item.dir {
        match dir {
            SortDir::Asc(_) => output.push_str(" ASC"),
            SortDir::Desc(_) => output.push_str(" DESC"),
        }
    }
    if let Some(using) = &item.using {
        print_using_clause(output, using);
    }
    if let Some(nulls) = &item.nulls {
        match nulls {
            NullsOrder::First(_) => output.push_str(" NULLS FIRST"),
            NullsOrder::Last(_) => output.push_str(" NULLS LAST"),
        }
    }
}

fn print_using_clause(output: &mut String, using: &UsingClause) {
    output.push_str(" USING ");
    match &using.op {
        crate::ast::select::UsingOp::Gt(_) => output.push('>'),
        crate::ast::select::UsingOp::Lt(_) => output.push('<'),
    }
}

// --- CREATE TABLE ---

fn print_create_table(output: &mut String, stmt: &CreateTableStmt) {
    output.push_str("CREATE ");
    if stmt.temp.is_some() {
        output.push_str("TEMP ");
    }
    output.push_str("TABLE ");
    output.push_str(&stmt.name.0);
    match &stmt.body {
        CreateTableBody::Columns {
            columns,
            partition_by,
        } => {
            output.push_str(" (");
            for (i, col) in columns.inner.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                output.push_str(&col.name.0);
                output.push(' ');
                print_type_name(output, &col.type_name);
                if col.primary_key.is_some() {
                    output.push_str(" PRIMARY KEY");
                }
            }
            output.push(')');
            if let Some(pb) = partition_by {
                print_partition_by(output, pb);
            }
        }
        CreateTableBody::PartitionOf {
            parent,
            for_values,
            partition_by,
        } => {
            output.push_str(" PARTITION OF ");
            output.push_str(&parent.0);
            output.push_str(" FOR VALUES IN (");
            for (i, val) in for_values.values.inner.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                print_expr(output, val);
            }
            output.push(')');
            if let Some(pb) = partition_by {
                print_partition_by(output, pb);
            }
        }
    }
}

fn print_partition_by(output: &mut String, pb: &crate::ast::partition::PartitionByClause) {
    output.push_str(" PARTITION BY ");
    output.push_str(&pb.strategy.0);
    output.push_str(" (");
    for (i, col) in pb.columns.inner.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        output.push_str(&col.0);
    }
    output.push(')');
}

// --- INSERT ---

fn print_insert(output: &mut String, stmt: &InsertStmt) {
    output.push_str("INSERT INTO ");
    output.push_str(&stmt.table_name.0);
    if let Some(col_list) = &stmt.columns {
        output.push_str(" (");
        for (i, col) in col_list.inner.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&col.0);
        }
        output.push(')');
    }
    match &stmt.source {
        crate::ast::insert::InsertSource::Default(_) => {
            output.push_str(" DEFAULT VALUES");
        }
        crate::ast::insert::InsertSource::Rows(rows) => {
            output.push_str(" VALUES ");
            for (i, row) in rows.rows.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                output.push('(');
                for (j, val) in row.inner.iter().enumerate() {
                    if j > 0 {
                        output.push_str(", ");
                    }
                    print_expr(output, val);
                }
                output.push(')');
            }
        }
    }
}

// --- DELETE ---

fn print_delete(output: &mut String, stmt: &DeleteStmt) {
    output.push_str("DELETE FROM ");
    output.push_str(&stmt.table_name.0);
    if let Some(alias) = &stmt.alias {
        match alias {
            TableAlias::WithAs(_) => {
                output.push_str(" AS ");
                output.push_str(alias.name());
            }
            TableAlias::Bare(_) => {
                output.push(' ');
                output.push_str(alias.name());
            }
        }
    }
    if let Some(where_clause) = &stmt.where_clause {
        output.push_str(" WHERE ");
        print_expr(output, &where_clause.condition);
    }
}

// --- DROP TABLE ---

fn print_drop_table(output: &mut String, stmt: &DropTableStmt) {
    output.push_str("DROP TABLE ");
    output.push_str(&stmt.name.0);
}

// --- SET / RESET ---

fn print_set(output: &mut String, stmt: &SetStmt) {
    output.push_str("SET ");
    output.push_str(&stmt.param.0);
    match &stmt.sep {
        SetSep::To(_) => output.push_str(" TO "),
        SetSep::Eq(_) => output.push_str(" = "),
    }
    match &stmt.value {
        SetValue::On(_) => output.push_str("on"),
        SetValue::Off(_) => output.push_str("off"),
        SetValue::True(_) => output.push_str("true"),
        SetValue::False(_) => output.push_str("false"),
        SetValue::StringLit(s) => output.push_str(&s.0),
        SetValue::Ident(i) => output.push_str(&i.0),
    }
}

fn print_reset(output: &mut String, stmt: &ResetStmt) {
    output.push_str("RESET ");
    output.push_str(&stmt.param.0);
}

// --- ANALYZE ---

fn print_analyze(output: &mut String, stmt: &AnalyzeStmt) {
    output.push_str("ANALYZE ");
    output.push_str(&stmt.table_name.0);
}

// --- EXPLAIN ---

fn print_explain(output: &mut String, stmt: &ExplainStmt) {
    output.push_str("EXPLAIN ");
    if let Some(opts) = &stmt.options {
        output.push('(');
        for (i, opt) in opts.inner.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&opt.name.0);
            if let Some(val) = &opt.value {
                output.push(' ');
                match val {
                    crate::ast::explain::ExplainOptValue::On(_) => output.push_str("on"),
                    crate::ast::explain::ExplainOptValue::Off(_) => output.push_str("off"),
                    crate::ast::explain::ExplainOptValue::Ident(i) => output.push_str(&i.0),
                }
            }
        }
        output.push_str(") ");
    }
    print_select(output, &stmt.stmt);
}

// --- CREATE INDEX / DROP INDEX ---

fn print_create_index(output: &mut String, stmt: &CreateIndexStmt) {
    output.push_str("CREATE INDEX ");
    output.push_str(&stmt.name.0);
    output.push_str(" ON ");
    output.push_str(&stmt.table_name.0);
    output.push_str(" (");
    for (i, elem) in stmt.columns.inner.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        output.push_str(&elem.column.0);
        if let Some(dir) = &elem.dir {
            match dir {
                SortDir::Asc(_) => output.push_str(" ASC"),
                SortDir::Desc(_) => output.push_str(" DESC"),
            }
        }
        if let Some(nulls) = &elem.nulls {
            match nulls {
                NullsOrder::First(_) => output.push_str(" NULLS FIRST"),
                NullsOrder::Last(_) => output.push_str(" NULLS LAST"),
            }
        }
    }
    output.push(')');
}

fn print_drop_index(output: &mut String, stmt: &DropIndexStmt) {
    output.push_str("DROP INDEX ");
    output.push_str(&stmt.name.0);
}

// --- CREATE FUNCTION / DROP FUNCTION ---

fn print_create_function(output: &mut String, stmt: &CreateFunctionStmt) {
    output.push_str("CREATE FUNCTION ");
    output.push_str(&stmt.name.0);
    output.push('(');
    for (i, arg) in stmt.args.inner.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        print_type_name(output, arg);
    }
    output.push_str(") RETURNS ");
    match &stmt.returns.return_type {
        ReturnType::Setof(s) => {
            output.push_str("SETOF ");
            print_type_name(output, &s.type_name);
        }
        ReturnType::Plain(t) => print_type_name(output, t),
    }
    output.push_str(" AS ");
    output.push_str(&stmt.body.0);
    output.push_str(" LANGUAGE ");
    output.push_str(&stmt.language.name.0);
    if stmt.immutable.is_some() {
        output.push_str(" IMMUTABLE");
    }
}

fn print_drop_function(output: &mut String, stmt: &DropFunctionStmt) {
    output.push_str("DROP FUNCTION ");
    output.push_str(&stmt.name.0);
    output.push('(');
    for (i, arg) in stmt.args.inner.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        print_type_name(output, arg);
    }
    output.push(')');
}

// --- VALUES / TABLE / UNION ALL ---

fn print_compound_query(output: &mut String, query: &CompoundQuery) {
    match query {
        CompoundQuery::Table(t) => print_table_stmt(output, t),
        CompoundQuery::Body(b) => print_compound_body(output, b),
    }
}

fn print_compound_body(output: &mut String, body: &CompoundBody) {
    print_select_body(output, &body.body);
    if let Some(union_all) = &body.union_all {
        output.push_str(" UNION ALL ");
        print_compound_query(output, &union_all.right);
    }
}

fn print_table_stmt(output: &mut String, stmt: &TableStmt) {
    output.push_str("TABLE ");
    output.push_str(&stmt.table_name.0);
}

// --- Expressions ---

fn print_expr(output: &mut String, expr: &Expr) {
    match expr {
        Expr::IntegerLit(lit) => output.push_str(&lit.0),
        Expr::NumericLit(lit) => output.push_str(&lit.0),
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
        Expr::Func(func_call) => print_func_call(output, func_call),
        Expr::Paren(ParenExpr { inner, .. }) => {
            output.push('(');
            match inner {
                crate::ast::expr::ParenContent::Subquery(body) => {
                    print_select_body(output, body);
                }
                crate::ast::expr::ParenContent::Exprs(exprs) => {
                    for (i, e) in exprs.iter().enumerate() {
                        if i > 0 {
                            output.push_str(", ");
                        }
                        print_expr(output, e);
                    }
                }
            }
            output.push(')');
        }
        Expr::CastFunc(TypeCastFunc { type_name, value }) => {
            print_type_name(output, type_name);
            output.push(' ');
            output.push_str(&value.0);
        }
        Expr::Not(_, operand) => {
            output.push_str("NOT ");
            if is_infix(operand) {
                output.push('(');
                print_expr(output, operand);
                output.push(')');
            } else {
                print_expr(output, operand);
            }
        }
        Expr::Neg(_, operand) => {
            output.push('-');
            print_expr(output, operand);
        }
        Expr::Or(left, _, right)
        | Expr::And(left, _, right)
        | Expr::Eq(left, _, right)
        | Expr::Neq(left, _, right)
        | Expr::Lt(left, _, right)
        | Expr::Gt(left, _, right)
        | Expr::Lte(left, _, right)
        | Expr::Gte(left, _, right)
        | Expr::Add(left, _, right)
        | Expr::Sub(left, _, right) => {
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
        Expr::InExpr(inner, _, list) => {
            print_expr(output, inner);
            output.push_str(" IN (");
            match &list.inner {
                crate::ast::expr::InContent::Subquery(body) => {
                    print_select_body(output, body);
                }
                crate::ast::expr::InContent::Exprs(exprs) => {
                    for (i, val) in exprs.iter().enumerate() {
                        if i > 0 {
                            output.push_str(", ");
                        }
                        print_expr(output, val);
                    }
                }
            }
            output.push(')');
        }
    }
}

fn print_func_call(output: &mut String, func_call: &FuncCall) {
    output.push_str(&func_call.name.0);
    output.push('(');
    for (i, arg) in func_call.args.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        print_expr(output, arg);
    }
    output.push(')');
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
            | Expr::Add(..)
            | Expr::Sub(..)
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
        Expr::Add(..) => output.push('+'),
        Expr::Sub(..) => output.push('-'),
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
        TypeName::Serial(_) => output.push_str("SERIAL"),
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

    // --- CREATE TABLE with PRIMARY KEY ---

    #[test]
    fn print_create_table_with_primary_key() {
        let result = round_trip_stmt("CREATE TABLE t (id SERIAL PRIMARY KEY, a INT, b text)");
        assert_eq!(
            result,
            "CREATE TABLE t (id SERIAL PRIMARY KEY, a int, b text)"
        );
    }

    // --- DELETE ---

    #[test]
    fn print_delete_simple() {
        let result = round_trip_stmt("DELETE FROM delete_test WHERE a > 25");
        assert_eq!(result, "DELETE FROM delete_test WHERE a > 25");
    }

    #[test]
    fn print_delete_with_as_alias() {
        let result = round_trip_stmt("DELETE FROM delete_test AS dt WHERE dt.a > 75");
        assert_eq!(result, "DELETE FROM delete_test AS dt WHERE dt.a > 75");
    }

    #[test]
    fn print_delete_with_bare_alias() {
        let result = round_trip_stmt("DELETE FROM delete_test dt WHERE delete_test.a > 25");
        assert_eq!(
            result,
            "DELETE FROM delete_test dt WHERE delete_test.a > 25"
        );
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
