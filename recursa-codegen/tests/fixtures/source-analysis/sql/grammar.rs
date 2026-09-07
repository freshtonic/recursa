//! Source-analysis fixture for the flat SQL-shaped LL(2) grammar.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/sql.rs` and `src/grammars/sql.rs`. The authoritative
//! shape is the spike's flat 12-variant `Statement`, not its earlier nested
//! shared-prefix explanation.

recursa::grammar! {
    module = crate::sql,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        CREATE => "CREATE",
        TABLE => "TABLE",
        VIEW => "VIEW",
        INDEX => "INDEX",
        SCHEMA => "SCHEMA",
        FUNCTION => "FUNCTION",
        PROCEDURE => "PROCEDURE",
        DROP => "DROP",
    }
}

#[derive(recursa::Node)]
pub enum Statement {
    CreateTable(CreateTable),
    CreateView(CreateView),
    CreateIndex(CreateIndex),
    CreateSchema(CreateSchema),
    CreateFunction(CreateFunction),
    CreateProcedure(CreateProcedure),
    DropTable(DropTable),
    DropView(DropView),
    DropIndex(DropIndex),
    DropSchema(DropSchema),
    DropFunction(DropFunction),
    DropProcedure(DropProcedure),
}

#[derive(recursa::Node)]
#[tok(CREATE, TABLE, this)]
pub struct CreateTable {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(CREATE, VIEW, this)]
pub struct CreateView {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(CREATE, INDEX, this)]
pub struct CreateIndex {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(CREATE, SCHEMA, this)]
pub struct CreateSchema {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(CREATE, FUNCTION, this)]
pub struct CreateFunction {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(CREATE, PROCEDURE, this)]
pub struct CreateProcedure {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(DROP, TABLE, this)]
pub struct DropTable {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(DROP, VIEW, this)]
pub struct DropView {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(DROP, INDEX, this)]
pub struct DropIndex {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(DROP, SCHEMA, this)]
pub struct DropSchema {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(DROP, FUNCTION, this)]
pub struct DropFunction {
    pub rest: Rest,
}

#[derive(recursa::Node)]
#[tok(DROP, PROCEDURE, this)]
pub struct DropProcedure {
    pub rest: Rest,
}
