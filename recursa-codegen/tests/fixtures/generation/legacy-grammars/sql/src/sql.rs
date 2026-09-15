//! Runtime form of the frozen M3 SQL grammar from recursa-spike b410d2f.

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

recursa::ast_node! {

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
}

recursa::ast_node! {

    #[tok(CREATE, TABLE, this)]
    pub struct CreateTable {
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(CREATE, VIEW, this)]
    pub struct CreateView {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(CREATE, INDEX, this)]
    pub struct CreateIndex {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(CREATE, SCHEMA, this)]
    pub struct CreateSchema {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(CREATE, FUNCTION, this)]
    pub struct CreateFunction {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(CREATE, PROCEDURE, this)]
    pub struct CreateProcedure {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(DROP, TABLE, this)]
    pub struct DropTable {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(DROP, VIEW, this)]
    pub struct DropView {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(DROP, INDEX, this)]
    pub struct DropIndex {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(DROP, SCHEMA, this)]
    pub struct DropSchema {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(DROP, FUNCTION, this)]
    pub struct DropFunction {
        pub rest: Rest,
    }
}

recursa::ast_node! {

    #[tok(DROP, PROCEDURE, this)]
    pub struct DropProcedure {
        pub rest: Rest,
    }
}
