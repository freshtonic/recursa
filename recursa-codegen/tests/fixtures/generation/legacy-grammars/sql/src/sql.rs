//! Runtime form of the frozen M3 SQL grammar from recursa-spike b410d2f.

recursa::grammar! {
    module = crate::sql,
    keyword_matching = sensitive,
    max_lookahead = 2,
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
pub enum Statement<'input> {
    CreateTable(CreateTable<'input>),
    CreateView(CreateView<'input>),
    CreateIndex(CreateIndex<'input>),
    CreateSchema(CreateSchema<'input>),
    CreateFunction(CreateFunction<'input>),
    CreateProcedure(CreateProcedure<'input>),
    DropTable(DropTable<'input>),
    DropView(DropView<'input>),
    DropIndex(DropIndex<'input>),
    DropSchema(DropSchema<'input>),
    DropFunction(DropFunction<'input>),
    DropProcedure(DropProcedure<'input>),
}

#[derive(recursa::Node)]
#[tok(CREATE, TABLE, this)]
pub struct CreateTable<'input> {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(CREATE, VIEW, this)]
pub struct CreateView<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(CREATE, INDEX, this)]
pub struct CreateIndex<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(CREATE, SCHEMA, this)]
pub struct CreateSchema<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(CREATE, FUNCTION, this)]
pub struct CreateFunction<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(CREATE, PROCEDURE, this)]
pub struct CreateProcedure<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(DROP, TABLE, this)]
pub struct DropTable<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(DROP, VIEW, this)]
pub struct DropView<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(DROP, INDEX, this)]
pub struct DropIndex<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(DROP, SCHEMA, this)]
pub struct DropSchema<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(DROP, FUNCTION, this)]
pub struct DropFunction<'input> {
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
#[tok(DROP, PROCEDURE, this)]
pub struct DropProcedure<'input> {
    pub rest: Rest<'input>,
}
