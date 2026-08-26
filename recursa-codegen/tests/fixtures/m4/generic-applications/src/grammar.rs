recursa::grammar! {
    module = crate::grammar,
    derives(Debug, Clone, PartialEq, Pretty),
    spans = optional,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        IS => "is",
        NOT => "not",
        EXISTS => "exists",
    }
    punctuation {
        MINUS => "-",
        COMMA => ",",
        LPAREN => "(",
        RPAREN => ")",
        LBRACKET => "[",
        RBRACKET => "]",
        LBRACE => "{",
        RBRACE => "}",
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
pub struct Atom<T> {
    pub value: T,
}

#[derive(recursa::Node)]
pub struct Wrap<T> {
    pub value: T,
}

#[derive(recursa::Node)]
pub enum Mixed<'input> {
    Direct(#[tok(LPAREN, this, RPAREN)] Wrap<Name<'input>>),
    Repeated(#[tok(LBRACKET, this, RBRACKET)] Wrap<Vec<Name<'input>>>),
}

pub type Maybe<T> = Option<Box<T>>;

#[derive(recursa::Node)]
pub struct OptionalOnly<T> {
    #[tok(LPAREN, this, RPAREN)]
    pub value: Option<Box<T>>,
}

#[derive(recursa::Node)]
pub struct Aliased<T> {
    #[tok(LBRACKET, this, RBRACKET)]
    pub value: Maybe<T>,
}

#[derive(recursa::Node)]
#[tok(LBRACE, this, RBRACE)]
pub struct Chain<T> {
    pub value: T,
    pub tail: Option<Box<Self>>,
}

#[derive(recursa::Node)]
#[tok(LPAREN, this, RPAREN)]
pub struct Pair<'left, 'right> {
    pub left: Integer<'left>,
    #[tok(COMMA, this)]
    pub right: Name<'right>,
}

#[derive(recursa::Node)]
#[tok(LBRACE, this, RBRACE)]
pub struct LifetimeNode<'input, T> {
    pub value: T,
    #[tok(COMMA, this)]
    pub marker: Name<'input>,
}

mod aliases {
    use super::LifetimeNode as DefinitionLocalNode;

    pub type Maybe<'alias, T> = Option<Box<DefinitionLocalNode<'alias, T>>>;
}

#[derive(recursa::Node)]
pub struct AliasLifetimeRoot<'input, T> {
    #[tok(LPAREN, this, RPAREN)]
    pub value: aliases::Maybe<'input, T>,
}

#[derive(recursa::Node)]
pub struct AliasLifetimeConcrete<'input> {
    pub value: AliasLifetimeRoot<'input, Integer<'input>>,
}

#[derive(recursa::Node)]
pub struct AuditRoot<'outer> {
    pub optional: OptionalOnly<Name<'outer>>,
    pub aliased: Aliased<Integer<'outer>>,
    pub integer_chain: Chain<Integer<'outer>>,
    pub name_chain: Chain<Name<'outer>>,
    pub pair: Pair<'outer, 'outer>,
}

#[derive(recursa::Node)]
#[pratt]
pub enum Expr<'input> {
    Integer(Atom<Integer<'input>>),
    Name(Atom<Name<'input>>),

    #[parse(prefix, bp = 20)]
    #[tok(MINUS)]
    Neg(Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(MINUS)]
    Sub(Box<Self>, Box<Self>),

    #[parse(prefix, bp = 20)]
    #[tok(NOT, EXISTS)]
    NotExists(Box<Self>),

    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(IS, NOT)]
    IsNot(Box<Self>, Box<Self>),
}
