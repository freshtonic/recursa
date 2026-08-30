recursa::grammar! {
    module = crate::grammar,
    derives(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Pretty, Visit, VisitMut, Arbitrary, serde::Serialize, serde::Deserialize),
    spans = optional,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        IS => "IS",
        KW_NOT => "NOT",
        NEVER => "NEVER",
        EXISTS => "EXISTS",
        HAS => "HAS",
        LIKE => "LIKE",
        ESCAPE => "ESCAPE",
        NULL => "NULL",
        BEGIN => "BEGIN",
        GROUP => "GROUP",
        END => "END",
    }
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
        CARET => "^",
        BANG => "!",
        DOUBLE_COLON => "::", spacing = tight,
        GROUP_OPEN => "[",
        GROUP_CLOSE => "]",
    }
}

#[derive(recursa::Node)]
pub struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub value: IntegerText<'input>,
}

#[derive(recursa::Node)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub value: NameText<'input>,
}

#[derive(recursa::Node)]
pub struct EscapeClause<'input> {
    #[tok(ESCAPE, this)]
    pub value: Name<'input>,
}

#[derive(recursa::Node)]
#[pratt]
#[pretty(group = inconsistent)]
pub enum Expr<'input> {
    Integer(Integer<'input>),
    Name(Name<'input>),
    MultiGroup(#[tok(BEGIN, GROUP, this, END, GROUP)] Box<Self>),
    Paren(#[tok(GROUP_OPEN, this, GROUP_CLOSE)] Box<Self>),

    #[parse(prefix, bp = 35)]
    #[tok(MINUS)]
    Neg(Box<Self>),

    #[parse(postfix, bp = 40)]
    #[tok(BANG)]
    Factorial(Box<Self>),

    #[parse(infix, lbp = 30, rbp = 30)]
    #[tok(CARET)]
    Power(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(STAR)]
    Multiply(Box<Self>, Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),

    #[parse(prefix, bp = 35)]
    #[tok(NEVER, EXISTS)]
    NotExists(Box<Self>),

    #[parse(infix, lbp = 5, rbp = 6)]
    #[tok(IS, KW_NOT)]
    #[pretty(group = inconsistent)]
    IsNot(Box<Self>, #[pretty(break_before = soft)] Box<Self>),

    #[parse(postfix, bp = 5)]
    Like(
        Box<Self>,
        #[tok(LIKE, this)] Box<Self>,
        Option<EscapeClause<'input>>,
    ),

    #[parse(infix, lbp = 25, rbp = 26)]
    #[tok(DOUBLE_COLON)]
    Cast(Box<Self>, Box<Self>),

    #[parse(postfix, bp = 40)]
    #[tok(HAS, NULL)]
    IsNull(Box<Self>),
}

#[derive(recursa::Node)]
pub struct Followed<'input> {
    #[tok(this, PLUS)]
    pub expression: Expr<'input>,
    pub tail: Integer<'input>,
}

#[derive(recursa::Node)]
pub struct NestedFollowed<'input> {
    #[tok(this, STAR)]
    pub expression: Expr<'input>,
    pub tail: Integer<'input>,
}
