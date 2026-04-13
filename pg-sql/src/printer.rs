//! SQL printer -- converts AST back to valid SQL text.
//!
//! Does not attempt to reproduce original formatting. Produces semantically
//! equivalent SQL with consistent casing (uppercase keywords, lowercase for
//! identifiers as-is from the AST).

use crate::ast::analyze::AnalyzeStmt;
use crate::ast::create_function::{
    CreateFunctionStmt, DropFunctionStmt, FuncBody, FuncReturnType, FuncReturnTypeName,
};
use crate::ast::create_index::{CreateIndexStmt, DropIndexStmt};
use crate::ast::create_table::{CreateTableBody, CreateTableStmt};
use crate::ast::create_view::{CreateViewStmt, DropViewStmt};
use crate::ast::delete::{DeleteStmt, TableAlias};
use crate::ast::drop_table::DropTableStmt;
use crate::ast::explain::ExplainStmt;
use crate::ast::expr::{
    ArrayExpr, BoolTestKind, CastType, Expr, FuncCall, ParenExpr, QualifiedRef, QualifiedWildcard,
    TypeCastFunc, TypeName,
};
use crate::ast::insert::InsertStmt;
use crate::ast::merge::MergeStmt;
use crate::ast::select::{
    LateralRef, NullsOrder, OrderByClause, OrderByItem, SelectItem, SelectStmt, SortDir,
    SubqueryRef, TableRef, UsingClause, ValuesBody,
};
use crate::ast::set_reset::{ResetStmt, SetSep, SetStmt, SetValue};
use crate::ast::update::UpdateStmt;
use crate::ast::values::{CompoundBody, CompoundQuery, TableStmt};
use crate::ast::with_clause::WithStatement;
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
        Statement::With(s) => print_with_statement(output, s),
        Statement::Select(s) => print_select(output, s),
        Statement::CreateTable(s) => print_create_table(output, s),
        Statement::CreateView(s) => print_create_view(output, s),
        Statement::Insert(s) => print_insert(output, s),
        Statement::Update(s) => print_update(output, s),
        Statement::Merge(s) => print_merge(output, s),
        Statement::Delete(s) => print_delete(output, s),
        Statement::DropTable(s) => print_drop_table(output, s),
        Statement::DropView(s) => print_drop_view(output, s),
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
        Statement::Raw(s) => output.push_str(&s.text),
    }
}

// --- SELECT ---

fn print_select(output: &mut String, stmt: &SelectStmt) {
    output.push_str("SELECT ");
    if stmt.distinct.is_some() {
        output.push_str("DISTINCT ");
    }
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
    if let Some(group_by) = &stmt.group_by {
        output.push_str(" GROUP BY ");
        for (i, e) in group_by.exprs.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            print_expr(output, e);
        }
    }
    if let Some(having) = &stmt.having {
        output.push_str(" HAVING ");
        print_expr(output, &having.condition);
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
        match alias {
            crate::ast::select::Alias::WithAs(a) => {
                output.push_str(" AS ");
                output.push_str(&a.name.0);
            }
            crate::ast::select::Alias::Bare(ident) => {
                output.push(' ');
                output.push_str(&ident.0);
            }
        }
    }
}

fn print_table_ref(output: &mut String, table_ref: &TableRef) {
    print_simple_table_ref(output, &table_ref.base);
    for join in table_ref.joins.iter() {
        if let Some(jt) = &join.join_type {
            match jt {
                crate::ast::select::JoinType::Left(_) => output.push_str(" LEFT"),
                crate::ast::select::JoinType::Right(_) => output.push_str(" RIGHT"),
                crate::ast::select::JoinType::Full(_) => output.push_str(" FULL"),
                crate::ast::select::JoinType::Inner(_) => output.push_str(" INNER"),
                crate::ast::select::JoinType::Cross(_) => output.push_str(" CROSS"),
            }
        }
        output.push_str(" JOIN ");
        print_simple_table_ref(output, &join.table);
        if let Some(cond) = &join.condition {
            match cond {
                crate::ast::select::JoinCondition::On(on) => {
                    output.push_str(" ON ");
                    print_expr(output, &on.condition);
                }
                crate::ast::select::JoinCondition::Using(u) => {
                    output.push_str(" USING (");
                    for (i, col) in u.columns.inner.iter().enumerate() {
                        if i > 0 {
                            output.push_str(", ");
                        }
                        output.push_str(&col.0);
                    }
                    output.push(')');
                }
            }
        }
    }
}

fn print_simple_table_ref(output: &mut String, table_ref: &crate::ast::select::SimpleTableRef) {
    use crate::ast::select::SimpleTableRef;
    match table_ref {
        SimpleTableRef::Table(plain) => {
            output.push_str(&plain.name.0);
            if let Some(alias) = &plain.alias {
                match alias {
                    crate::ast::delete::TableAlias::WithAs(a) => {
                        output.push_str(" AS ");
                        output.push_str(&a.name.0);
                    }
                    crate::ast::delete::TableAlias::Bare(ident) => {
                        output.push(' ');
                        output.push_str(&ident.0);
                    }
                }
            }
        }
        SimpleTableRef::Func(func_ref) => {
            print_func_call(output, &func_ref.func);
            if let Some(alias) = &func_ref.alias {
                print_table_alias(output, alias);
            }
        }
        SimpleTableRef::Inherited(inh) => {
            output.push_str(&inh.name.0);
            output.push('*');
            if let Some(alias) = &inh.alias {
                output.push(' ');
                output.push_str(&alias.0);
            }
        }
        SimpleTableRef::Subquery(sub) => print_subquery_ref(output, sub),
        SimpleTableRef::Lateral(lat) => print_lateral_ref(output, lat),
    }
}

fn print_subquery_ref(output: &mut String, sub: &SubqueryRef) {
    output.push('(');
    print_compound_query(output, &sub.query);
    output.push(')');
    print_table_alias(output, &sub.alias);
}

fn print_lateral_ref(output: &mut String, lat: &LateralRef) {
    output.push_str("LATERAL (");
    print_compound_query(output, &lat.query);
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
        crate::ast::select::SelectBody::WithBody(w) => print_with_statement(output, w),
        crate::ast::select::SelectBody::Select(s) => print_select(output, s.as_ref()),
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
            inherits,
            partition_by,
        } => {
            output.push_str(" (");
            for (i, col) in columns.inner.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                output.push_str(&col.name.0);
                output.push(' ');
                print_cast_type(output, &col.type_name);
                for constraint in &col.constraints {
                    match constraint {
                        crate::ast::create_table::ColumnConstraint::PrimaryKey => {
                            output.push_str(" PRIMARY KEY");
                        }
                        crate::ast::create_table::ColumnConstraint::NotNull => {
                            output.push_str(" NOT NULL");
                        }
                        crate::ast::create_table::ColumnConstraint::Unique => {
                            output.push_str(" UNIQUE");
                        }
                        crate::ast::create_table::ColumnConstraint::References(table, col_ref) => {
                            output.push_str(" REFERENCES ");
                            output.push_str(table);
                            if let Some(c) = col_ref {
                                output.push('(');
                                output.push_str(c);
                                output.push(')');
                            }
                        }
                        crate::ast::create_table::ColumnConstraint::GeneratedAlwaysAsIdentity => {
                            output.push_str(" GENERATED ALWAYS AS IDENTITY");
                        }
                        crate::ast::create_table::ColumnConstraint::Default(expr) => {
                            output.push_str(" DEFAULT ");
                            print_expr(output, expr);
                        }
                    }
                }
            }
            output.push(')');
            if let Some(inh) = inherits {
                output.push_str(" INHERITS (");
                for (i, parent) in inh.inner.iter().enumerate() {
                    if i > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&parent.0);
                }
                output.push(')');
            }
            if let Some(pb) = partition_by {
                print_partition_by(output, pb);
            }
        }
        CreateTableBody::AsQuery { query } => {
            output.push_str(" AS ");
            print_statement_to(output, query);
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
        crate::ast::insert::InsertSource::Select(query) => {
            output.push(' ');
            print_compound_query(output, query);
        }
    }
    if let Some(oc) = &stmt.on_conflict {
        output.push_str(" ON CONFLICT ");
        if let Some(target) = &oc.target {
            output.push('(');
            for (i, col) in target.inner.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                output.push_str(&col.0);
            }
            output.push_str(") ");
        }
        match &oc.action {
            crate::ast::insert::ConflictAction::DoNothing => {
                output.push_str("DO NOTHING");
            }
            crate::ast::insert::ConflictAction::DoUpdate(update) => {
                output.push_str("DO UPDATE SET ");
                for (i, asgn) in update.assignments.iter().enumerate() {
                    if i > 0 {
                        output.push_str(", ");
                    }
                    match asgn {
                        crate::ast::update::SetAssignment::Single { column, value } => {
                            output.push_str(&column.0);
                            output.push_str(" = ");
                            print_expr(output, value);
                        }
                        crate::ast::update::SetAssignment::Tuple { columns, values } => {
                            output.push('(');
                            for (j, col) in columns.iter().enumerate() {
                                if j > 0 {
                                    output.push_str(", ");
                                }
                                output.push_str(&col.0);
                            }
                            output.push_str(") = ");
                            print_expr(output, values);
                        }
                    }
                }
                if let Some(w) = &update.where_clause {
                    output.push_str(" WHERE ");
                    print_expr(output, &w.condition);
                }
            }
        }
    }
    if let Some(ret) = &stmt.returning {
        output.push_str(" RETURNING ");
        for (i, item) in ret.items.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            print_select_item(output, item);
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
    if let Some(using) = &stmt.using_clause {
        output.push_str(" USING ");
        for (i, table_ref) in using.tables.iter().enumerate() {
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
    if let Some(ret) = &stmt.returning {
        output.push_str(" RETURNING ");
        for (i, item) in ret.items.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            print_select_item(output, item);
        }
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
    print_statement_to(output, &stmt.body);
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
    output.push_str("CREATE ");
    if stmt.or_replace {
        output.push_str("OR REPLACE ");
    }
    output.push_str("FUNCTION ");
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
        FuncReturnType::Setof(s) => {
            output.push_str("SETOF ");
            print_func_return_type_name(output, &s.type_name);
        }
        FuncReturnType::Plain(t) => print_func_return_type_name(output, t),
    }
    output.push_str(" AS ");
    match &stmt.body {
        FuncBody::String(s) => output.push_str(&s.0),
        FuncBody::Dollar(d) => output.push_str(&d.0),
    }
    output.push_str(" LANGUAGE ");
    output.push_str(&stmt.language.name.0);
    if stmt.immutable.is_some() {
        output.push_str(" IMMUTABLE");
    }
}

fn print_func_return_type_name(output: &mut String, name: &FuncReturnTypeName) {
    match name {
        FuncReturnTypeName::Trigger(_) => output.push_str("trigger"),
        FuncReturnTypeName::Base(t) => print_type_name(output, t),
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
        CompoundQuery::Paren(p) => {
            output.push('(');
            print_compound_query(output, &p.inner.inner);
            output.push(')');
            if let Some(set_op) = &p.set_op {
                print_set_op(output, set_op);
            }
        }
        CompoundQuery::Table(t) => print_table_stmt(output, t),
        CompoundQuery::Body(b) => print_compound_body(output, b),
    }
}

fn print_compound_body(output: &mut String, body: &CompoundBody) {
    print_select_body(output, &body.body);
    if let Some(set_op) = &body.set_op {
        print_set_op(output, set_op);
    }
}

fn print_set_op(output: &mut String, set_op: &crate::ast::values::SetOpCombiner) {
    match &set_op.op {
        crate::ast::values::SetOp::UnionAll => output.push_str(" UNION ALL "),
        crate::ast::values::SetOp::UnionDistinct => output.push_str(" UNION DISTINCT "),
        crate::ast::values::SetOp::Union => output.push_str(" UNION "),
        crate::ast::values::SetOp::ExceptAll => output.push_str(" EXCEPT ALL "),
        crate::ast::values::SetOp::Except => output.push_str(" EXCEPT "),
        crate::ast::values::SetOp::IntersectAll => output.push_str(" INTERSECT ALL "),
        crate::ast::values::SetOp::Intersect => output.push_str(" INTERSECT "),
    }
    print_compound_query(output, &set_op.right);
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
                    print_compound_query(output, body);
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
        | Expr::BangEq(left, _, right)
        | Expr::Neq(left, _, right)
        | Expr::Lt(left, _, right)
        | Expr::Gt(left, _, right)
        | Expr::Lte(left, _, right)
        | Expr::Gte(left, _, right)
        | Expr::Add(left, _, right)
        | Expr::Sub(left, _, right)
        | Expr::Mul(left, _, right)
        | Expr::Div(left, _, right) => {
            print_binop_operand(output, left);
            output.push(' ');
            print_infix_op(output, expr);
            output.push(' ');
            print_binop_operand(output, right);
        }
        Expr::Cast(inner, _, cast_type) => {
            print_expr(output, inner);
            output.push_str("::");
            print_cast_type(output, cast_type);
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
                    print_compound_query(output, body);
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
        Expr::NotInExpr(inner, suffix) => {
            print_expr(output, inner);
            output.push_str(" NOT IN (");
            match &suffix.list.inner {
                crate::ast::expr::InContent::Subquery(body) => {
                    print_compound_query(output, body);
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
        Expr::Subscript(inner, _, idx, _) => {
            print_expr(output, inner);
            output.push('[');
            print_expr(output, idx);
            output.push(']');
        }
        Expr::Concat(left, _, right) => {
            print_binop_operand(output, left);
            output.push_str(" || ");
            print_binop_operand(output, right);
        }
        Expr::Mod(left, _, right) => {
            print_binop_operand(output, left);
            output.push_str(" % ");
            print_binop_operand(output, right);
        }
        Expr::Exists(e) => {
            output.push_str("EXISTS (");
            print_compound_query(output, &e.subquery.inner);
            output.push(')');
        }
        Expr::Array(arr) => {
            output.push_str("ARRAY");
            match arr {
                ArrayExpr::Bracket { elements, .. } => {
                    output.push('[');
                    for (i, elem) in elements.iter().enumerate() {
                        if i > 0 {
                            output.push_str(", ");
                        }
                        print_expr(output, elem);
                    }
                    output.push(']');
                }
                ArrayExpr::Subquery(sub) => {
                    output.push('(');
                    print_compound_query(output, &sub.inner);
                    output.push(')');
                }
            }
        }
        Expr::RowExpr(r) => {
            output.push_str("ROW(");
            for (i, val) in r.values.inner.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                print_expr(output, val);
            }
            output.push(')');
        }
    }
}

fn print_func_call(output: &mut String, func_call: &FuncCall) {
    output.push_str(&func_call.name.0);
    output.push('(');
    if func_call.star_arg {
        output.push('*');
    } else {
        if func_call.distinct.is_some() {
            output.push_str("DISTINCT ");
        }
        for (i, arg) in func_call.args.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            print_expr(output, arg);
        }
    }
    output.push(')');
    if let Some(window) = &func_call.window {
        output.push_str(" OVER (");
        if let Some(pb) = &window.partition_by {
            output.push_str("PARTITION BY ");
            for (i, e) in pb.exprs.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                print_expr(output, e);
            }
        }
        if let Some(ob) = &window.order_by {
            print_order_by(output, ob);
        }
        output.push(')');
    }
}

/// Returns true if the expression is an infix binary operation.
fn is_infix(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Or(..)
            | Expr::And(..)
            | Expr::Eq(..)
            | Expr::BangEq(..)
            | Expr::Neq(..)
            | Expr::Lt(..)
            | Expr::Gt(..)
            | Expr::Lte(..)
            | Expr::Gte(..)
            | Expr::Add(..)
            | Expr::Sub(..)
            | Expr::Mul(..)
            | Expr::Div(..)
            | Expr::Concat(..)
            | Expr::Mod(..)
    )
}

/// Print the operator keyword/symbol for an infix expression.
fn print_infix_op(output: &mut String, expr: &Expr) {
    match expr {
        Expr::And(..) => output.push_str("AND"),
        Expr::Or(..) => output.push_str("OR"),
        Expr::Eq(..) => output.push('='),
        Expr::BangEq(..) => output.push_str("!="),
        Expr::Neq(..) => output.push_str("<>"),
        Expr::Lt(..) => output.push('<'),
        Expr::Gt(..) => output.push('>'),
        Expr::Lte(..) => output.push_str("<="),
        Expr::Gte(..) => output.push_str(">="),
        Expr::Add(..) => output.push('+'),
        Expr::Sub(..) => output.push('-'),
        Expr::Mul(..) => output.push('*'),
        Expr::Div(..) => output.push('/'),
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
        TypeName::Integer(_) => output.push_str("integer"),
        TypeName::Int(_) => output.push_str("int"),
        TypeName::Serial(_) => output.push_str("SERIAL"),
        TypeName::Numeric(_) => output.push_str("numeric"),
        TypeName::Varchar(_) => output.push_str("varchar"),
        TypeName::Ident(ident) => output.push_str(&ident.0),
    }
}

fn print_cast_type(output: &mut String, ct: &CastType) {
    print_type_name(output, &ct.base);
    if let Some(prec) = &ct.precision {
        output.push('(');
        for (i, p) in prec.inner.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&p.0);
        }
        output.push(')');
    }
    if ct.array_suffix.is_some() {
        output.push_str("[]");
    }
}

// --- WITH ---

fn print_with_statement(output: &mut String, stmt: &WithStatement) {
    output.push_str("WITH ");
    if stmt.with_clause.recursive.is_some() {
        output.push_str("RECURSIVE ");
    }
    for (i, cte) in stmt.with_clause.ctes.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        output.push_str(&cte.name.0);
        if let Some(cols) = &cte.columns {
            output.push_str(" (");
            for (j, col) in cols.inner.iter().enumerate() {
                if j > 0 {
                    output.push_str(", ");
                }
                output.push_str(&col.0);
            }
            output.push(')');
        }
        output.push_str(" AS ");
        if let Some(mat) = &cte.materialized {
            match mat {
                crate::ast::with_clause::MaterializedOption::Materialized(_) => {
                    output.push_str("MATERIALIZED ");
                }
                crate::ast::with_clause::MaterializedOption::NotMaterialized(_) => {
                    output.push_str("NOT MATERIALIZED ");
                }
            }
        }
        output.push('(');
        print_statement_to(output, &cte.query.inner);
        output.push(')');
        // SEARCH clause
        if let Some(search) = &cte.search {
            output.push_str(" SEARCH ");
            match &search.direction {
                crate::ast::with_clause::SearchDirection::Depth(_) => {
                    output.push_str("DEPTH ");
                }
                crate::ast::with_clause::SearchDirection::Breadth(_) => {
                    output.push_str("BREADTH ");
                }
            }
            output.push_str("FIRST BY ");
            for (j, col) in search.columns.iter().enumerate() {
                if j > 0 {
                    output.push_str(", ");
                }
                output.push_str(&col.0);
            }
            output.push_str(" SET ");
            output.push_str(&search.set_column.0);
        }
        // CYCLE clause
        if let Some(cycle) = &cte.cycle {
            output.push_str(" CYCLE ");
            for (j, col) in cycle.columns.iter().enumerate() {
                if j > 0 {
                    output.push_str(", ");
                }
                output.push_str(&col.0);
            }
            output.push_str(" SET ");
            output.push_str(&cycle.set_column.name.0);
            if let Some(to_default) = &cycle.set_column.to_default {
                output.push_str(" TO ");
                print_expr(output, &to_default.to_value);
                output.push_str(" DEFAULT ");
                print_expr(output, &to_default.default_value);
            }
            output.push_str(" USING ");
            output.push_str(&cycle.using_column.0);
        }
    }
    output.push(' ');
    print_statement_to(output, &stmt.body);
}

// --- UPDATE ---

fn print_update(output: &mut String, stmt: &UpdateStmt) {
    output.push_str("UPDATE ");
    output.push_str(&stmt.table_name.0);
    if let Some(alias) = &stmt.alias {
        output.push(' ');
        output.push_str(&alias.0);
    }
    output.push_str(" SET ");
    for (i, asgn) in stmt.assignments.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        match asgn {
            crate::ast::update::SetAssignment::Single { column, value } => {
                output.push_str(&column.0);
                output.push_str(" = ");
                print_expr(output, value);
            }
            crate::ast::update::SetAssignment::Tuple { columns, values } => {
                output.push('(');
                for (j, col) in columns.iter().enumerate() {
                    if j > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&col.0);
                }
                output.push_str(") = ");
                print_expr(output, values);
            }
        }
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
    if let Some(w) = &stmt.where_clause {
        output.push_str(" WHERE ");
        print_expr(output, &w.condition);
    }
    if let Some(ret) = &stmt.returning {
        output.push_str(" RETURNING ");
        for (i, item) in ret.items.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            print_select_item(output, item);
        }
    }
}

// --- MERGE ---

fn print_merge(output: &mut String, stmt: &MergeStmt) {
    output.push_str("MERGE INTO ");
    output.push_str(&stmt.table_name.0);
    output.push_str(" USING ");
    print_table_ref(output, &stmt.source);
    output.push_str(" ON ");
    print_expr(output, &stmt.condition);
    for wc in stmt.when_clauses.iter() {
        match wc {
            crate::ast::merge::WhenClause::MatchedUpdate(u) => {
                output.push_str(" WHEN MATCHED THEN UPDATE SET ");
                for (i, asgn) in u.assignments.iter().enumerate() {
                    if i > 0 {
                        output.push_str(", ");
                    }
                    match asgn {
                        crate::ast::update::SetAssignment::Single { column, value } => {
                            output.push_str(&column.0);
                            output.push_str(" = ");
                            print_expr(output, value);
                        }
                        crate::ast::update::SetAssignment::Tuple { columns, values } => {
                            output.push('(');
                            for (j, col) in columns.iter().enumerate() {
                                if j > 0 {
                                    output.push_str(", ");
                                }
                                output.push_str(&col.0);
                            }
                            output.push_str(") = ");
                            print_expr(output, values);
                        }
                    }
                }
            }
            crate::ast::merge::WhenClause::MatchedDelete(_) => {
                output.push_str(" WHEN MATCHED THEN DELETE");
            }
            crate::ast::merge::WhenClause::NotMatchedInsert(ins) => {
                output.push_str(" WHEN NOT MATCHED THEN INSERT ");
                if let Some(cols) = &ins.columns {
                    output.push('(');
                    for (i, col) in cols.inner.iter().enumerate() {
                        if i > 0 {
                            output.push_str(", ");
                        }
                        output.push_str(&col.0);
                    }
                    output.push_str(") ");
                }
                output.push_str("VALUES (");
                for (i, val) in ins.values.inner.iter().enumerate() {
                    if i > 0 {
                        output.push_str(", ");
                    }
                    print_expr(output, val);
                }
                output.push(')');
            }
        }
    }
}

// --- CREATE VIEW ---

fn print_create_view(output: &mut String, stmt: &CreateViewStmt) {
    output.push_str("CREATE ");
    if stmt.or_replace {
        output.push_str("OR REPLACE ");
    }
    if stmt.temp {
        output.push_str("TEMPORARY ");
    }
    if stmt.recursive {
        output.push_str("RECURSIVE ");
    }
    output.push_str("VIEW ");
    output.push_str(&stmt.name.0);
    if let Some(cols) = &stmt.columns {
        output.push_str(" (");
        for (i, col) in cols.inner.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&col.0);
        }
        output.push(')');
    }
    output.push_str(" AS ");
    print_compound_query(output, &stmt.query);
}

// --- DROP VIEW ---

fn print_drop_view(output: &mut String, stmt: &DropViewStmt) {
    output.push_str("DROP VIEW ");
    if stmt.if_exists {
        output.push_str("IF EXISTS ");
    }
    output.push_str(&stmt.name.0);
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

    // --- ORDER BY USING ---

    #[test]
    fn print_order_by_using_gt() {
        let result = round_trip_stmt("SELECT f1 FROM t ORDER BY f1 USING >");
        assert_eq!(result, "SELECT f1 FROM t ORDER BY f1 USING >");
    }

    #[test]
    fn print_order_by_using_lt() {
        let result = round_trip_stmt("SELECT f1 FROM t ORDER BY f1 USING <");
        assert_eq!(result, "SELECT f1 FROM t ORDER BY f1 USING <");
    }

    #[test]
    fn print_order_by_asc() {
        let result = round_trip_stmt("SELECT * FROM t ORDER BY f1 ASC");
        assert_eq!(result, "SELECT * FROM t ORDER BY f1 ASC");
    }

    #[test]
    fn print_order_by_desc() {
        let result = round_trip_stmt("SELECT * FROM t ORDER BY f1 DESC");
        assert_eq!(result, "SELECT * FROM t ORDER BY f1 DESC");
    }

    #[test]
    fn print_order_by_nulls_first() {
        let result = round_trip_stmt("SELECT * FROM t ORDER BY f1 NULLS FIRST");
        assert_eq!(result, "SELECT * FROM t ORDER BY f1 NULLS FIRST");
    }

    #[test]
    fn print_order_by_desc_nulls_last() {
        let result = round_trip_stmt("SELECT * FROM t ORDER BY f1 DESC NULLS LAST");
        assert_eq!(result, "SELECT * FROM t ORDER BY f1 DESC NULLS LAST");
    }

    // --- OFFSET / LIMIT ---

    #[test]
    fn print_select_offset() {
        let result = round_trip_stmt("SELECT 1 OFFSET 0");
        assert_eq!(result, "SELECT 1 OFFSET 0");
    }

    #[test]
    fn print_select_limit() {
        let result = round_trip_stmt("SELECT 1 LIMIT 1");
        assert_eq!(result, "SELECT 1 LIMIT 1");
    }

    #[test]
    fn print_select_limit_offset() {
        let result = round_trip_stmt("SELECT * FROM t LIMIT 10 OFFSET 5");
        assert_eq!(result, "SELECT * FROM t LIMIT 10 OFFSET 5");
    }

    // --- FOR UPDATE ---

    #[test]
    fn print_select_for_update() {
        let result = round_trip_stmt("SELECT f1 FROM t FOR UPDATE");
        assert_eq!(result, "SELECT f1 FROM t FOR UPDATE");
    }

    // --- SET / RESET ---

    #[test]
    fn print_set_to() {
        let result = round_trip_stmt("SET enable_seqscan TO off");
        assert_eq!(result, "SET enable_seqscan TO off");
    }

    #[test]
    fn print_set_eq() {
        let result = round_trip_stmt("SET enable_sort = false");
        assert_eq!(result, "SET enable_sort = false");
    }

    #[test]
    fn print_reset() {
        let result = round_trip_stmt("RESET enable_seqscan");
        assert_eq!(result, "RESET enable_seqscan");
    }

    // --- ANALYZE ---

    #[test]
    fn print_analyze() {
        let result = round_trip_stmt("ANALYZE onek2");
        assert_eq!(result, "ANALYZE onek2");
    }

    // --- EXPLAIN ---

    #[test]
    fn print_explain_costs_off() {
        let result = round_trip_stmt("EXPLAIN (costs off) SELECT * FROM t");
        assert_eq!(result, "EXPLAIN (costs off) SELECT * FROM t");
    }

    // --- CREATE INDEX / DROP INDEX ---

    #[test]
    fn print_create_index() {
        let result = round_trip_stmt("CREATE INDEX fooi ON foo (f1)");
        assert_eq!(result, "CREATE INDEX fooi ON foo (f1)");
    }

    #[test]
    fn print_create_index_desc_nulls_last() {
        let result = round_trip_stmt("CREATE INDEX fooi ON foo (f1 DESC NULLS LAST)");
        assert_eq!(result, "CREATE INDEX fooi ON foo (f1 DESC NULLS LAST)");
    }

    #[test]
    fn print_drop_index() {
        let result = round_trip_stmt("DROP INDEX fooi");
        assert_eq!(result, "DROP INDEX fooi");
    }

    // --- CREATE FUNCTION / DROP FUNCTION ---

    #[test]
    fn print_create_function_immutable() {
        let result = round_trip_stmt(
            "CREATE FUNCTION sillysrf(int) RETURNS SETOF int AS 'values (1),(10),(2),($1)' LANGUAGE sql IMMUTABLE",
        );
        assert_eq!(
            result,
            "CREATE FUNCTION sillysrf(int) RETURNS SETOF int AS 'values (1),(10),(2),($1)' LANGUAGE sql IMMUTABLE"
        );
    }

    #[test]
    fn print_drop_function() {
        let result = round_trip_stmt("DROP FUNCTION sillysrf(int)");
        assert_eq!(result, "DROP FUNCTION sillysrf(int)");
    }

    // --- VALUES / TABLE / UNION ALL ---

    #[test]
    fn print_values() {
        let result = round_trip_stmt("VALUES (1, 2), (3, 4)");
        assert_eq!(result, "VALUES (1, 2), (3, 4)");
    }

    #[test]
    fn print_table_stmt() {
        let result = round_trip_stmt("TABLE int8_tbl");
        assert_eq!(result, "TABLE int8_tbl");
    }

    #[test]
    fn print_values_union_all_select() {
        let result =
            round_trip_stmt("VALUES (1, 2), (3, 4) UNION ALL SELECT 5, 6 UNION ALL TABLE t");
        assert_eq!(
            result,
            "VALUES (1, 2), (3, 4) UNION ALL SELECT 5, 6 UNION ALL TABLE t"
        );
    }

    // --- Arithmetic / negation / numeric ---

    #[test]
    fn print_addition() {
        let result = round_trip_stmt("SELECT 4 + 4");
        assert_eq!(result, "SELECT 4 + 4");
    }

    #[test]
    fn print_negation() {
        let result = round_trip_stmt("SELECT -1");
        assert_eq!(result, "SELECT -1");
    }

    #[test]
    fn print_numeric_literal() {
        let result = round_trip_stmt("SELECT 77.7");
        assert_eq!(result, "SELECT 77.7");
    }

    // --- IN expression ---

    #[test]
    fn print_in_list() {
        let result = round_trip_stmt("SELECT * FROM t WHERE f1 IN (1, 2, 3)");
        assert_eq!(result, "SELECT * FROM t WHERE f1 IN (1, 2, 3)");
    }

    // --- Subquery in parens ---

    #[test]
    fn print_subquery_in_parens() {
        let result = round_trip_stmt("SELECT (SELECT 1)");
        assert_eq!(result, "SELECT (SELECT 1)");
    }

    // --- Table refs: subquery, lateral, inherited ---

    #[test]
    fn print_subquery_ref() {
        let result = round_trip_stmt("SELECT foo FROM (SELECT 1 OFFSET 0) AS foo");
        assert_eq!(result, "SELECT foo FROM (SELECT 1 OFFSET 0) AS foo");
    }

    #[test]
    fn print_lateral_ref() {
        let result = round_trip_stmt("SELECT * FROM t, LATERAL (VALUES (1)) v");
        assert_eq!(result, "SELECT * FROM t, LATERAL (VALUES (1)) v");
    }

    #[test]
    fn print_inherited_table() {
        let result = round_trip_stmt("SELECT p.name FROM person* p");
        assert_eq!(result, "SELECT p.name FROM person* p");
    }

    // --- Subquery alias with column list ---

    #[test]
    fn print_subquery_alias_with_columns() {
        let result = round_trip_stmt("SELECT * FROM (VALUES (1, 2)) AS v (i, j)");
        assert_eq!(result, "SELECT * FROM (VALUES (1, 2)) AS v (i, j)");
    }

    // --- Row expression (tuple in parentheses) ---

    #[test]
    fn print_row_expression() {
        let result = round_trip_stmt("SELECT * FROM t WHERE (a, b) IN (VALUES (1, 1), (2, 2))");
        assert_eq!(
            result,
            "SELECT * FROM t WHERE (a, b) IN (VALUES (1, 1), (2, 2))"
        );
    }

    // --- Partition tables ---

    #[test]
    fn print_create_partitioned_table() {
        let result = round_trip_stmt("CREATE TABLE t (a int, b int) PARTITION BY list (a)");
        assert_eq!(
            result,
            "CREATE TABLE t (a int, b int) PARTITION BY list (a)"
        );
    }

    #[test]
    fn print_create_partition_of() {
        let result = round_trip_stmt(
            "CREATE TABLE t1 PARTITION OF t FOR VALUES IN (1) PARTITION BY list (b)",
        );
        assert_eq!(
            result,
            "CREATE TABLE t1 PARTITION OF t FOR VALUES IN (1) PARTITION BY list (b)"
        );
    }

    // --- Full select.sql round-trip parse+print ---

    #[test]
    fn round_trip_select_sql_parses_and_prints() {
        let sql = std::fs::read_to_string("fixtures/sql/select.sql")
            .expect("select.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        let printed = print_commands(&commands);

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
    fn round_trip_with_sql_parses_and_prints() {
        let sql = std::fs::read_to_string("fixtures/sql/with.sql")
            .expect("with.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        let printed = print_commands(&commands);

        let mut input2 = Input::new(&printed);
        let commands2 = parse_sql_file(&mut input2).unwrap();
        assert_eq!(
            commands.len(),
            commands2.len(),
            "re-parsed command count should match"
        );
    }
}
