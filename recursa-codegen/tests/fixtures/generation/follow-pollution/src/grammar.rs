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

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Statement {
        A(boxed!(A)),
        A2(boxed!(A2)),
        B(boxed!(B)),
        B2(boxed!(B2)),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(A_KW, this, C_KW)]
    pub struct A {
        pub x: Option<X>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(A_KW, this, D_KW)]
    pub struct A2 {
        pub y: Y,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(B_KW, this, D_KW)]
    pub struct B {
        pub x: Option<X>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(B_KW, this, C_KW)]
    pub struct B2 {
        pub y: Y,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Y {
        pub x: X,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct X {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Expr {
        Column(ColumnRef),
        Call(FuncCall),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct ColumnRef {
        pub name: ColId,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(this, LPAREN, RPAREN)]
    pub struct FuncCall {
        pub name: TypeFunctionName,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum FuncName {
        Qualified(QualifiedName),
        Plain(TypeFunctionName),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct QualifiedName {
        pub first: ColId,
        #[tok(DOT)]
        pub second: ColId,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct ColId {
        #[lex(pattern = r"[a-z_][a-z0-9_]*")]
        pub text: ColIdText,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct TypeFunctionName {
        #[lex(pattern = r"[a-z_][a-z0-9_]*")]
        pub text: TypeFunctionNameText,
    }
}
