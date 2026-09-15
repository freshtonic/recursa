recursa::grammar! {
    module = crate::grammar,
    diagrams,
    derives(Debug, Clone, PartialEq, Pretty, serde::Serialize, serde::Deserialize),
    spans = optional,
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        IS => "is",
        NOT => "not",
        EXISTS => "exists",
        AND => "and",
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

recursa::ast_node! {

    pub struct Integer {
        #[lex(pattern = r"[0-9]+")]
        pub value: IntegerText,
    }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub value: NameText,
    }
}

recursa::ast_node! {

    pub struct Atom<T> {
        pub value: T,
    }
}

recursa::ast_node! {

    pub struct Wrap<T> {
        pub value: T,
    }
}

recursa::ast_node! {

    #[tok(IS, this)]
    pub struct KeywordName {
        pub value: Name,
    }
}

recursa::ast_node! {

    pub struct GenericKeywordRoot {
        pub value: Wrap<KeywordName>,
    }
}

recursa::ast_node! {

    pub struct AttachedWrap<T> {
        #[tok(LBRACE, this, RBRACE)]
        pub value: T,
    }
}

recursa::ast_node! {

    pub struct GenericAttachedRoot {
        pub value: AttachedWrap<KeywordName>,
    }
}

recursa::ast_node! {

    pub struct Separated<T> {
        #[tok(LBRACKET, this, RBRACKET)]
        #[sep(AND)]
        pub values: zero_or_many!(T),
    }
}

recursa::ast_node! {

    pub enum Mixed {
        Direct(#[tok(LPAREN, this, RPAREN)] Wrap<Name>),
        Repeated(#[tok(LBRACKET, this, RBRACKET)] Wrap<zero_or_many!(Name)>),
    }
}

pub type Maybe<T> = Option<T>;

recursa::ast_node! {

    pub struct OptionalOnly<T> {
        #[tok(LPAREN, this, RPAREN)]
        pub value: Option<boxed!(T)>,
    }
}

recursa::ast_node! {

    pub struct Aliased<T> {
        #[tok(LBRACKET, this, RBRACKET)]
        pub value: Maybe<boxed!(T)>,
    }
}

recursa::ast_node! {

    #[tok(LBRACE, this, RBRACE)]
    pub struct Chain<T> {
        pub value: T,
        pub tail: Option<boxed!(Self)>,
    }
}

recursa::ast_node! {

    #[tok(LPAREN, this, RPAREN)]
    pub struct Pair {
        pub left: Integer,
        #[tok(COMMA, this)]
        pub right: Name,
    }
}

recursa::ast_node! {

    #[tok(LBRACE, this, RBRACE)]
    pub struct LifetimeNode<T> {
        pub value: T,
        #[tok(COMMA, this)]
        pub marker: Name,
    }
}

mod aliases {
    use super::LifetimeNode as DefinitionLocalNode;

    pub type Marked<'alias, T> = DefinitionLocalNode<'alias, T>;
}

recursa::ast_node! {

    pub struct AliasLifetimeRoot<T> {
        #[tok(LPAREN, this, RPAREN)]
        pub value: Option<boxed!(aliases::Marked<T>)>,
    }
}

recursa::ast_node! {

    pub struct AliasLifetimeConcrete {
        pub value: AliasLifetimeRoot<Integer>,
    }
}

recursa::ast_node! {

    pub struct AuditRoot {
        pub optional: OptionalOnly<Name>,
        pub aliased: Aliased<Integer>,
        pub integer_chain: Chain<Integer>,
        pub name_chain: Chain<Name>,
        pub pair: Pair,
    }
}

recursa::ast_node! {

    pub struct SeparatorAudit {
        pub separated: Separated<Name>,
    }
}

recursa::ast_node! {

    #[pratt]
    pub enum Expr {
        Integer(Atom<Integer>),
        Name(Atom<Name>),
        Paren(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),

        #[parse(prefix, bp = 20)]
        #[tok(MINUS)]
        Neg(boxed!(Self)),

        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(MINUS)]
        Sub(boxed!(Self), boxed!(Self)),

        #[parse(prefix, bp = 20)]
        #[tok(NOT, EXISTS)]
        NotExists(boxed!(Self)),

        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(IS, NOT)]
        IsNot(boxed!(Self), boxed!(Self)),
    }
}
