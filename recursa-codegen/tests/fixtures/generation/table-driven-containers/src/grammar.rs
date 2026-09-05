//! The container grammar of the table-driven parser (#114): every container shape the grammar
//! model accepts, each in the form that lowers to inlined alternatives and in the form that
//! lowers to a shared helper nonterminal, plus admission sets and fixed-token attachments.
//!
//! The integration test generates this file twice, once as written and once with the parser
//! style flipped to `recursive_descent`, and compares what the two parsers produce.
//!
//! The keywords `AS`, `ONLY` and `TEMP` are admitted as names and `IN` and `OUT` as callees. An
//! optional whose first token can be one of them (`Option<Alias>`, `Option<Mode>`, lists of
//! `Value`) is inlined into its parent's alternatives; an optional that starts elsewhere
//! (`Option<Guard>`, `Vec<Bang>`) lowers to a nullable helper. `Modifiers`, `Erased`, `Options` and `Trail` carry
//! more optional choices than the inlining budget admits, so their rightmost choices fall back
//! to helpers: two presences, one erased group, one valued optional and one trailing separator.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
    parsers(rd = recursive_descent, lr = table_driven),
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        LET => "let",
        PRINT => "print",
        FN => "fn",
        WITH => "with",
        RECURSIVE => "recursive",
        IF => "if",
        NOT => "not",
        EXISTS => "exists",
        RETURNS => "returns",
        AS => "as",
        IN => "in",
        OUT => "out",
        ONLY => "only",
        TEMP => "temp",
        ONE => "one",
        TWO => "two",
        THREE => "three",
        FOUR => "four",
        FIVE => "five",
        SIX => "six",
        MODS => "mods",
        ERASED => "erased",
        OPTIONS => "options",
        TRAIL => "trail",
        BOXES => "boxes",
        MARKS => "marks",
        FILTER => "filter",
    }
    punctuation {
        EQ => "=",
        SEMI => ";",
        COMMA => ",",
        LPAREN => "(",
        RPAREN => ")",
        LBRACKET => "[",
        RBRACKET => "]",
        LBRACE => "{",
        RBRACE => "}",
        BANG => "!",
        COLON => ":",
        PIPE => "|",
    }
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Statement<'input> {
    Let(Let<'input>),
    Print(Print<'input>),
    Call(Call<'input>),
    Define(Define<'input>),
    With(With<'input>),
    Block(Block<'input>),
    Modifiers(Modifiers<'input>),
    Erased(Erased<'input>),
    Options(Options),
    Trail(Trail<'input>),
    Boxes(Boxes<'input>),
    Marks(Marks),
    Filter(Filter<'input>),
}

/// An erased optional group, a presence with an envelope, `Box<T>` and an inlined
/// `Option<Box<T>>`.
#[derive(recursa::Node, Debug, Clone)]
#[tok(LET, optional(ONE), this, SEMI)]
pub struct Let<'input> {
    #[tok(IF, this, EXISTS)]
    #[presence(NOT)]
    pub unless: bool,
    pub name: Name<'input>,
    #[tok(EQ)]
    pub value: Box<Value<'input>>,
    pub alias: Option<Box<Alias<'input>>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(AS, this)]
pub struct Alias<'input> {
    pub name: Name<'input>,
}

/// A bare presence and an `Option<T>` that lowers to a helper.
#[derive(recursa::Node, Debug, Clone)]
#[tok(PRINT, this, SEMI)]
pub struct Print<'input> {
    #[presence(NOT)]
    pub negated: bool,
    pub value: Value<'input>,
    pub guard: Option<Guard<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(IF, this)]
pub struct Guard<'input> {
    pub condition: Value<'input>,
}

/// An admitted keyword as content and an inlined `Vec<T>` with a trailing separator.
#[derive(recursa::Node, Debug, Clone)]
#[tok(this, SEMI)]
pub struct Call<'input> {
    pub callee: Callee<'input>,
    pub args: Args<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(LPAREN, this, RPAREN)]
pub struct Args<'input> {
    #[sep(COMMA, trailing)]
    pub values: Vec<Value<'input>>,
}

/// A `Vec1<T>` with a separator, the gram.y `arg_class` shape and an `Option<Box<T>>` behind
/// an attachment that lowers to a helper.
#[derive(recursa::Node, Debug, Clone)]
#[tok(FN, this, SEMI)]
pub struct Define<'input> {
    pub name: Name<'input>,
    pub params: Params<'input>,
    #[tok(RETURNS, this)]
    pub returns: Option<Box<Value<'input>>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(LPAREN, this, RPAREN)]
pub struct Params<'input> {
    #[sep(COMMA)]
    pub params: recursa::Vec1<Param<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Param<'input> {
    pub mode: Option<Mode>,
    pub name: Name<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Mode {
    #[tok(IN)]
    In,
    #[tok(OUT)]
    Out,
}

/// An inlined presence and an inlined `Vec1<T>` with a trailing separator.
#[derive(recursa::Node, Debug, Clone)]
#[tok(WITH, this, SEMI)]
pub struct With<'input> {
    #[presence(RECURSIVE)]
    pub recursive: bool,
    #[sep(PIPE, trailing)]
    pub names: recursa::Vec1<Name<'input>>,
}

/// An inlined `Vec<T>` without a separator, recursive through `Statement`.
#[derive(recursa::Node, Debug, Clone)]
#[tok(LBRACE, this, RBRACE)]
pub struct Block<'input> {
    pub statements: Vec<Statement<'input>>,
}

/// Six fixed-only choices: the two rightmost presences fall back to helpers.
#[derive(recursa::Node, Debug, Clone)]
#[tok(MODS, optional(ONE), optional(TWO), optional(THREE), this, SEMI)]
pub struct Modifiers<'input> {
    #[presence(FOUR)]
    pub four: bool,
    #[presence(FIVE)]
    pub five: bool,
    #[presence(SIX)]
    pub six: bool,
    pub name: Name<'input>,
}

/// Five erased groups: the rightmost falls back to a helper.
#[derive(recursa::Node, Debug, Clone)]
#[tok(ERASED, optional(ONE), optional(TWO), optional(THREE), optional(FOUR), optional(FIVE), this, SEMI)]
pub struct Erased<'input> {
    pub name: Name<'input>,
}

/// Five valued optionals of unit fixed-token nodes: the rightmost falls back to a helper.
#[derive(recursa::Node, Debug, Clone)]
#[tok(OPTIONS, this, SEMI)]
pub struct Options {
    pub a: Option<KwAs>,
    pub b: Option<KwIn>,
    pub c: Option<KwOut>,
    pub d: Option<KwOnly>,
    pub e: Option<KwTemp>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(AS)]
pub struct KwAs;

#[derive(recursa::Node, Debug, Clone)]
#[tok(IN)]
pub struct KwIn;

#[derive(recursa::Node, Debug, Clone)]
#[tok(OUT)]
pub struct KwOut;

#[derive(recursa::Node, Debug, Clone)]
#[tok(ONLY)]
pub struct KwOnly;

#[derive(recursa::Node, Debug, Clone)]
#[tok(TEMP)]
pub struct KwTemp;

/// Four erased groups and a trailing separator: the trailing choice falls back to a helper.
#[derive(recursa::Node, Debug, Clone)]
#[tok(TRAIL, optional(ONE), optional(TWO), optional(THREE), optional(FOUR), this, SEMI)]
pub struct Trail<'input> {
    #[sep(COMMA, trailing)]
    pub names: recursa::Vec1<Name<'input>>,
}

/// An inlined `Vec<Box<T>>` with a separator.
#[derive(recursa::Node, Debug, Clone)]
#[tok(BOXES, this, SEMI)]
pub struct Boxes<'input> {
    #[sep(COMMA)]
    pub values: Vec<Box<Value<'input>>>,
}

/// A `Vec<T>` and an `Option<Vec1<T>>` of unit fixed-token nodes, both helpers.
#[derive(recursa::Node, Debug, Clone)]
#[tok(MARKS, this, SEMI)]
pub struct Marks {
    pub marks: Vec<Bang>,
    #[tok(COLON)]
    pub tail: Option<recursa::Vec1<Bang>>,
}

/// A `Vec1<T>` without a separator and a repeated attachment with an erased group inside.
#[derive(recursa::Node, Debug, Clone)]
#[tok(FILTER, this, SEMI)]
pub struct Filter<'input> {
    pub bangs: recursa::Vec1<Bang>,
    #[tok(COLON, optional(NOT), this)]
    pub filters: Vec<Value<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
pub enum Value<'input> {
    Name { name: Name<'input> },
    Number(Number<'input>),
    Negated {
        mark: Bang,
        inner: Box<Value<'input>>,
    },
    List(List<'input>),
    Tuple(Tuple<'input>),
}

/// An inlined `Option<Vec1<T>>` with a separator.
#[derive(recursa::Node, Debug, Clone)]
#[tok(LBRACKET, this, RBRACKET)]
pub struct List<'input> {
    #[sep(COMMA)]
    pub items: Option<recursa::Vec1<Value<'input>>>,
}

/// A repeated attachment around every element of a `Vec1<T>`.
#[derive(recursa::Node, Debug, Clone)]
pub struct Tuple<'input> {
    #[tok(LPAREN, this, RPAREN)]
    #[sep(PIPE)]
    pub cells: recursa::Vec1<Value<'input>>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(BANG)]
pub struct Bang;

/// A content token whose admission set reclaims three keywords as names.
#[derive(recursa::Node, Debug, Clone)]
pub struct Name<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(AS, ONLY, TEMP))]
    pub text: NameText<'input>,
}

/// A content token whose admission set reclaims the two parameter modes as callees.
#[derive(recursa::Node, Debug, Clone)]
pub struct Callee<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(IN, OUT))]
    pub text: CalleeText<'input>,
}

#[derive(recursa::Node, Debug, Clone)]
pub struct Number<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: NumberText<'input>,
}
