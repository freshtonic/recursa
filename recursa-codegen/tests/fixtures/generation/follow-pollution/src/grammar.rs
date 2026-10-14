//! The two FOLLOW-pollution shapes of #125 through the sole LR parser.
//!
//! `Statement` is the shared-helper shape: `Option<X>` lowers to one `opt(X)` helper used by
//! `A` (followed by `c`) and by `B` (followed by `d`), while `A2` and `B2` read `Y -> X` with
//! the terminals swapped. Both contexts reach the one LR(0) state `{opt(X) -> X ., Y -> X .}`,
//! whose merged lookaheads are a reduce/reduce conflict that no canonical LR(1) state has; the
//! construction copies the helper per use-site class (`opt(X)@B`) and the conflict is gone.
//!
//! `ParenContent` is the unit-rule shape of the same end-marker rule (#143), modelled on
//! pg-sql's `Expr -> SelectWithParens` beside `SelectClause -> SelectWithParens`. Both are unit
//! rules of a grammar type over the one parenthesised-query Node, and `ParenContent` derives
//! either, so its start state predicts both and the goto on `SelectWithParens` reaches the one
//! LR(0) state `{Expr -> SelectWithParens ., SelectClause -> SelectWithParens .}` with the end
//! marker on both reductions. `,` continues only an `Expr` (`RowExpr`) and `)` only a
//! `SelectClause` (`SelectWithParens`), so the end marker is the single shared lookahead and
//! declaration order takes `Expr`, the earlier of the two.
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
        UNION_KW => "union",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        DOT => ".",
        COMMA => ",",
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
        Row(RowExpr),
        /// gram.y `c_expr: select_with_parens` (REL_17_11 gram.y 15299): the earlier of the two
        /// unit rules that complete `SelectWithParens`, so declaration order chooses it.
        Sub(SelectWithParens),
    }
}

recursa::ast_node! {
    /// gram.y `c_expr: '(' a_expr ')' opt_indirection`, reduced to a row of two so that the
    /// fixture needs none of gram.y's `%prec UMINUS` trick: a one-element parenthesis would be
    /// both an `Expr` and a `SelectClause`, which is the ambiguity gram.y resolves by
    /// precedence rather than the reduce/reduce this fixture is about.
    #[derive(Debug, Clone)]
    #[tok(LPAREN, this, RPAREN)]
    pub struct RowExpr {
        pub first: boxed!(Expr),
        #[tok(COMMA)]
        pub second: boxed!(Expr),
    }
}

recursa::ast_node! {
    /// gram.y `select_with_parens: '(' select_no_parens ')' | '(' select_with_parens ')'`
    /// (REL_17_11 gram.y 12683-12686): the one nonterminal the expression grammar and the query
    /// grammar share. The second production needs no arm of its own, because [`SelectClause`]
    /// admits a further `SelectWithParens`.
    #[derive(Debug, Clone)]
    #[tok(LPAREN, this, RPAREN)]
    pub struct SelectWithParens {
        pub inner: boxed!(SelectClause),
    }
}

recursa::ast_node! {
    /// gram.y `select_clause: simple_select | select_with_parens` (REL_17_11 gram.y 12757).
    /// The set operation is left-recursive with a bare right operand, which keeps the fixture
    /// free of a `precedence` declaration; gram.y writes `select_clause UNION set_quantifier
    /// select_clause` and settles the chain with `%left UNION`.
    #[derive(Debug, Clone)]
    pub enum SelectClause {
        /// gram.y `select_clause: select_with_parens`: the later of the two unit rules.
        Parens(SelectWithParens),
        Union(boxed!(UnionSelect)),
        Simple(X),
    }
}

recursa::ast_node! {
    /// gram.y `select_clause UNION set_quantifier select_clause` (REL_17_11 gram.y 12844).
    #[derive(Debug, Clone)]
    pub struct UnionSelect {
        pub left: boxed!(SelectClause),
        #[tok(UNION_KW)]
        pub right: X,
    }
}

recursa::ast_node! {
    /// The type that derives either side, the way pg-sql's parenthesised content does. Every
    /// grammar type is a parsing entry point, so this type's start state predicts `Expr` and
    /// `SelectClause` together: that is what puts the end marker on both unit rules of the one
    /// state that completes `SelectWithParens`.
    #[derive(Debug, Clone)]
    pub enum ParenContent {
        Query(SelectClause),
        Value(Expr),
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
