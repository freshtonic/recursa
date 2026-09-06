//! Enum variants that share a long call tail and part on their second token (#127).
//!
//! `max_lookahead = 4` lets a predictive walk reach the token after `f ( a`, which is
//! where the defect lived: the walk continued past the dot-or-paren decision into the
//! call tail, whose arguments are open Pratt expressions, and rejected `f(a + b)` at `+`.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 4,
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
}

#[derive(recursa::Node)]
pub struct Ident<'input> {
    #[lex(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
    pub text: IdentText<'input>,
}

#[derive(recursa::Node)]
pub struct Number<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: NumberText<'input>,
}

/// A call whose variants share the whole call tail: a dot after the first identifier
/// qualifies the name, a paren opens the call, so the second token decides.
#[derive(recursa::Node)]
pub enum FuncCall<'input> {
    Qualified(QualifiedFuncCall<'input>),
    Named(NamedFuncCall<'input>),
}

#[derive(recursa::Node)]
pub struct QualifiedFuncCall<'input> {
    pub name: QualifiedFuncName<'input>,
    pub tail: FunctionCallTail<'input>,
}

#[derive(recursa::Node)]
pub struct QualifiedFuncName<'input> {
    pub first: Ident<'input>,
    pub parts: recursa::Vec1<FuncNamePart<'input>>,
}

#[derive(recursa::Node)]
pub struct FuncNamePart<'input> {
    #[tok(DOT, this)]
    pub name: Ident<'input>,
}

#[derive(recursa::Node)]
pub struct NamedFuncCall<'input> {
    pub name: Ident<'input>,
    pub tail: FunctionCallTail<'input>,
}

/// The parens are nodes of their own: a fixed-token envelope on a repeated field
/// attaches to each element, not to the list.
#[derive(recursa::Node)]
pub struct FunctionCallTail<'input> {
    pub open: CallOpen,
    #[sep(COMMA)]
    pub args: Vec<FuncArg<'input>>,
    pub close: CallClose,
}

#[derive(recursa::Node)]
#[tok(LPAREN)]
pub struct CallOpen;

#[derive(recursa::Node)]
#[tok(RPAREN)]
pub struct CallClose;

/// An argument is a named `name => expr` or a bare expression, so the state after
/// `f ( name` mixes a derivation inside the expression with one that is not, exactly
/// as pg-sql's `FuncArg` does.
#[derive(recursa::Node)]
pub enum FuncArg<'input> {
    Named(NamedArg<'input>),
    Positional(Expr<'input>),
}

#[derive(recursa::Node)]
pub struct NamedArg<'input> {
    pub name: Ident<'input>,
    #[tok(ARROW, this)]
    pub value: Expr<'input>,
}

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'input> {
    Name(Ident<'input>),
    Number(Number<'input>),
    #[tok(NULL)]
    Null,

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),
}

/// pg-sql's `IndexTarget` shape: a parenthesized expression, a call, or a bare column.
/// `f` alone is a column and `f(` opens a call, so the second token decides here too.
#[derive(recursa::Node)]
pub enum Target<'input> {
    Paren(#[tok(LPAREN, this, RPAREN)] Box<Expr<'input>>),
    Func(Box<FuncCall<'input>>),
    Col(Ident<'input>),
}
