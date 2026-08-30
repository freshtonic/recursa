recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 2,
    spans = mandatory,
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
        DEBUG => "DEBUG",
        PRINT => "PRINT",
        READ => "READ",
        SET => "SET",
        AS => "AS",
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        SELECT => "SELECT",
        UNION => "UNION",
        END => "END",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
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
pub struct NestedStatement<'input> {
    pub statement: Statement<'input>,
}

#[derive(recursa::Node)]
#[tok(CREATE, TABLE, this)]
pub struct CreateTable<'input> {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", admits(AS))]
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

#[derive(recursa::Node)]
pub struct Column<'input> {
    pub name: Rest<'input>,
    pub as_alias: Option<AsAlias<'input>>,
}

#[derive(recursa::Node)]
pub struct AsAlias<'input> {
    #[tok(AS)]
    pub alias: Rest<'input>,
}

#[derive(recursa::Node)]
pub enum MaybeDebug {
    Present(Debug),
    Absent(MaybeOther),
}

#[derive(recursa::Node)]
pub struct MaybeOther {
    pub other: Option<Other>,
}

#[derive(recursa::Node)]
#[tok(SET)]
pub struct Other;

#[derive(recursa::Node)]
pub struct NullablePrefix<'input> {
    pub maybe_debug: MaybeDebug,
    pub tail: Option<PrintTail<'input>>,
}

#[derive(recursa::Node)]
pub struct PrintTail<'input> {
    #[tok(PRINT)]
    pub rest: Rest<'input>,
}

#[derive(recursa::Node)]
pub enum OverlappingRectangles {
    First(FirstRectangle),
    Second(SecondRectangle),
}

#[derive(recursa::Node)]
pub struct FirstRectangle {
    pub lead: FirstAb,
    pub tail: TokenC,
}

#[derive(recursa::Node)]
pub struct SecondRectangle {
    pub lead: FirstBc,
    pub tail: TokenD,
}

#[derive(recursa::Node)]
pub enum FirstAb {
    A(TokenA),
    B(TokenB),
}

#[derive(recursa::Node)]
pub enum FirstBc {
    B(TokenB),
    C(TokenC),
}

#[derive(recursa::Node)]
#[tok(A)]
pub struct TokenA;

#[derive(recursa::Node)]
#[tok(B)]
pub struct TokenB;

#[derive(recursa::Node)]
#[tok(C)]
pub struct TokenC;

#[derive(recursa::Node)]
#[tok(D)]
pub struct TokenD;

#[derive(recursa::Node)]
pub enum BalancedNested {
    #[tok(SELECT)]
    Select,
    Group(#[tok(LPAREN, this, RPAREN)] Box<Self>),
}

#[derive(recursa::Node)]
pub enum BalancedChoice {
    Query(BalancedQuery),
    Scalar(BalancedScalar),
}

#[derive(recursa::Node)]
pub struct BalancedQuery {
    #[tok(LPAREN, this, RPAREN)]
    pub left: BalancedNested,
    #[tok(UNION)]
    pub right: BalancedNested,
}

#[derive(recursa::Node)]
pub struct BalancedScalar(#[tok(LPAREN, this, RPAREN)] pub BalancedNested);

#[derive(recursa::Node)]
pub struct BalancedRoot {
    pub choice: BalancedChoice,
    pub end: BalancedEnd,
}

#[derive(recursa::Node)]
#[tok(END)]
pub struct BalancedEnd;

#[derive(recursa::Node)]
pub struct BalancedOuterSet {
    pub left: BalancedChoice,
    #[tok(UNION)]
    pub right: BalancedNested,
    pub end: BalancedEnd,
}

/// Choice whose FIRST-2 language coalesces LPAREN with an ordinary starter.
#[derive(recursa::Node)]
pub enum MixedBalancedChoice {
    Routed(MixedRouted),
    Query(MixedEnclosedC),
}

/// One branch with both an ordinary route and a balanced-enclosure route.
#[derive(recursa::Node)]
pub enum MixedRouted {
    Ordinary(MixedOrdinary),
    Enclosed(MixedEnclosedB),
}

#[derive(recursa::Node)]
#[tok(A, SELECT, D)]
pub struct MixedOrdinary;

#[derive(recursa::Node)]
#[tok(LPAREN, SELECT, RPAREN, B)]
pub struct MixedEnclosedB;

#[derive(recursa::Node)]
#[tok(LPAREN, SELECT, RPAREN, C)]
pub struct MixedEnclosedC;
