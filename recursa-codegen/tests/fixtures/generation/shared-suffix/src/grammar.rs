//! Enum variants that share a long call tail and part on their second token (#127).
//!
//! The LR automaton separates the qualified and unqualified forms at their second
//! token without inspecting the open call tail.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        NULL => "NULL",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        DOT => ".",
        COMMA => ",",
        PLUS => "+",
        ARROW => "=>",
    }
    precedence {
        left(bp = 10) { PLUS },
    }
}

recursa::ast_node! {

    pub struct Ident {
        #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
        pub text: IdentText,
    }
}

recursa::ast_node! {

    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}

recursa::ast_node! {
    /// A call whose variants share the whole call tail: a dot after the first identifier
    /// qualifies the name, a paren opens the call, so the second token decides.

    pub enum FuncCall {
        Qualified(QualifiedFuncCall),
        Named(NamedFuncCall),
    }
}

recursa::ast_node! {

    pub struct QualifiedFuncCall {
        pub name: QualifiedFuncName,
        pub tail: FunctionCallTail,
    }
}

recursa::ast_node! {

    pub struct QualifiedFuncName {
        pub first: Ident,
        pub parts: one_or_many!(FuncNamePart),
    }
}

recursa::ast_node! {

    pub struct FuncNamePart {
        #[tok(DOT, this)]
        pub name: Ident,
    }
}

recursa::ast_node! {

    pub struct NamedFuncCall {
        pub name: Ident,
        pub tail: FunctionCallTail,
    }
}

recursa::ast_node! {
    /// The parens are nodes of their own: a fixed-token envelope on a repeated field
    /// attaches to each element, not to the list.

    pub struct FunctionCallTail {
        pub open: CallOpen,
        #[sep(COMMA)]
        pub args: zero_or_many!(FuncArg),
        pub close: CallClose,
    }
}

recursa::ast_node! {

    #[tok(LPAREN)]
    pub struct CallOpen;
}

recursa::ast_node! {

    #[tok(RPAREN)]
    pub struct CallClose;
}

recursa::ast_node! {
    /// An argument is a named `name => expr` or a bare expression, so the state after
    /// `f ( name` mixes a derivation inside the expression with one that is not, exactly
    /// as pg-sql's `FuncArg` does.

    pub enum FuncArg {
        Named(NamedArg),
        Positional(Expr),
    }
}

recursa::ast_node! {

    pub struct NamedArg {
        pub name: Ident,
        #[tok(ARROW, this)]
        pub value: Expr,
    }
}

recursa::ast_node! {

    pub enum Expr {
        Name(Ident),
        Number(Number),
        #[tok(NULL)]
        Null,

        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}

recursa::ast_node! {
    /// pg-sql's `IndexTarget` shape: a parenthesized expression, a call, or a bare column.
    /// `f` alone is a column and `f(` opens a call, so the second token decides here too.

    pub enum Target {
        Paren(#[tok(LPAREN, this, RPAREN)] boxed!(Expr)),
        Func(boxed!(FuncCall)),
        Col(Ident),
    }
}
