//! Runtime form of the frozen M3 optional-query grammar from recursa-spike b410d2f.

recursa::grammar! {
    module = crate::optq,
    keyword_matching = sensitive,
    max_lookahead = 2,
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

#[derive(recursa::Node)]
#[tok(DEBUG)]
pub struct Debug;

#[derive(recursa::Node)]
pub enum Stmt<'input> {
    Print(PrintStmt<'input>),
    Read(ReadStmt<'input>),
    Set(SetStmt<'input>),
}

#[derive(recursa::Node)]
pub struct PrintStmt<'input> {
    pub debug: Option<Debug>,
    #[tok(PRINT)]
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
pub struct ReadStmt<'input> {
    pub debug: Option<Debug>,
    #[tok(READ)]
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
pub struct SetStmt<'input> {
    pub debug: Option<Debug>,
    #[tok(SET)]
    pub rest: Rest<'input>,
}
