//! Source-analysis fixture for the optional-qualifier LL(2) grammar.
//!
//! Rust 2024 source derived from `recursa-spike` revision
//! `b410d2f123cd7510a8a40e9e5b971936a2fd8463`, specifically
//! `src/descriptions/optq.rs` and `src/grammars/optq.rs`.

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
    /// Presence records whether the optional qualifier occurred.

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
