//! Runtime form of the frozen M3 optional-query grammar from recursa-spike b410d2f.

recursa::grammar! {
    module = crate::optq,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        DEBUG => "DEBUG",
        PRINT => "PRINT",
        READ => "READ",
        SET => "SET",
    }
}

recursa::ast_node! {

    #[tok(DEBUG)]
    pub struct Debug;
}

recursa::ast_node! {

    pub enum Stmt {
        Print(PrintStmt),
        Read(ReadStmt),
        Set(SetStmt),
    }
}

recursa::ast_node! {

    pub struct PrintStmt {
        pub debug: Option<Debug>,
        #[tok(PRINT)]
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
        pub rest: Rest,
    }
}

recursa::ast_node! {

    pub struct ReadStmt {
        pub debug: Option<Debug>,
        #[tok(READ)]
        pub rest: Rest,
    }
}

recursa::ast_node! {

    pub struct SetStmt {
        pub debug: Option<Debug>,
        #[tok(SET)]
        pub rest: Rest,
    }
}
