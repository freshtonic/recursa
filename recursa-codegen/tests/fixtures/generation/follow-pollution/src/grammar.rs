//! The two FOLLOW-pollution shapes of #125 through the sole LR parser.
//!
//! `Statement` is the shared-helper shape: `Option<X>` lowers to one `opt(X)` helper used by
//! `A` (followed by `c`) and by `B` (followed by `d`), while `A2` and `B2` read `Y -> X` with
//! the terminals swapped. Both contexts reach the one LR(0) state `{opt(X) -> X ., Y -> X .}`,
//! whose merged lookaheads are a reduce/reduce conflict that no canonical LR(1) state has; the
//! construction copies the helper per use-site class (`opt(X)@B`) and the conflict is gone.
//!
//! `Expr` and `FuncName` are the token-wrapper shape: `ColIdText` and `TypeFunctionNameText`
//! are two content tokens over one pattern, and every root reaches the one LR(0) state
//! `{ColIdText -> IDENT ., TypeFunctionNameText -> IDENT .}` with the end marker on both
//! reductions. The construction takes the earliest-declared wrapper, `ColIdText`. `Expr`
//! continues past the wrapper (`( )` selects the call) or ends in `ColId`. `FuncName` can end
//! only in `TypeFunctionName`, so on a bare `foo` the parser reduces `ColIdText`, looks for `.`,
//! and reports an unexpected end: the documented limit of the declaration-order rule.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        A_KW => "a",
        B_KW => "b",
        C_KW => "c",
        D_KW => "d",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        DOT => ".",
    }
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Statement<'input> {
    A(Box<A<'input>>),
    A2(Box<A2<'input>>),
    B(Box<B<'input>>),
    B2(Box<B2<'input>>),
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(A_KW, this, C_KW)]
pub struct A<'input> {
    pub x: Option<X<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(A_KW, this, D_KW)]
pub struct A2<'input> {
    pub y: Y<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(B_KW, this, D_KW)]
pub struct B<'input> {
    pub x: Option<X<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(B_KW, this, C_KW)]
pub struct B2<'input> {
    pub y: Y<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Y<'input> {
    pub x: X<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct X<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: NumberText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Expr<'input> {
    Column(ColumnRef<'input>),
    Call(FuncCall<'input>),
}

#[derive(recursa::Node, Debug, Clone)]
pub struct ColumnRef<'input> {
    pub name: ColId<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(this, LPAREN, RPAREN)]
pub struct FuncCall<'input> {
    pub name: TypeFunctionName<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum FuncName<'input> {
    Qualified(QualifiedName<'input>),
    Plain(TypeFunctionName<'input>),
}

#[derive(recursa::Node, Debug, Clone)]
pub struct QualifiedName<'input> {
    pub first: ColId<'input>,
    #[tok(DOT)]
    pub second: ColId<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct ColId<'input> {
    #[lex(pattern = r"[a-z_][a-z0-9_]*")]
    pub text: ColIdText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct TypeFunctionName<'input> {
    #[lex(pattern = r"[a-z_][a-z0-9_]*")]
    pub text: TypeFunctionNameText<'input>,
}
