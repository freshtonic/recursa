//! The container grammar of the table-driven parser (#114): every container shape the grammar
//! model accepts, each in the form that lowers to inlined alternatives and in the form that
//! lowers to a shared helper nonterminal, plus admission sets and fixed-token attachments.
//!
//! The keywords `AS`, `ONLY` and `TEMP` are admitted as names and `IN` and `OUT` as callees. An
//! optional whose first token can be one of them (`Option<Alias>`, `Option<Mode>`, lists of
//! `Value`) is inlined into its parent's alternatives; an optional that starts elsewhere
//! (`Option<Guard>`, `Vec<Bang>`) lowers to a nullable helper. `Modifiers`, `Erased`, `Options` and `Trail` carry
//! more optional choices than the inlining budget admits, so their rightmost choices fall back
//! to helpers: two presences, one erased group, one valued optional and one trailing separator.
//!
//! The grammar root selects `flat` and `derives(Pretty)`, so generation emits both renderers for
//! every declaration: the nested `Pretty` implementation and the flat one. `Value` takes a typed
//! pool with a declared byte bound and `Name` takes a bare typed pool; every other declaration
//! reaches the shared pool, so rendering crosses both tiers on every input.
//!
//! The grammar is the `flat-containers` sibling's, unchanged, so the two fixtures cover the same
//! container shapes: this one renders each shape twice and compares, and that one proves how the
//! stores were built.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    flat,
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
        VTAIL => "vtail",
        VARIADIC => "variadic",
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

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Statement {
        Let(Let),
        Print(Print),
        Call(Call),
        Define(Define),
        With(With),
        Block(Block),
        Modifiers(Modifiers),
        Erased(Erased),
        Options(Options),
        Trail(Trail),
        Boxes(Boxes),
        Marks(Marks),
        Filter(Filter),
        FunctionTail(FunctionTail),
    }
}

recursa::ast_node! {
    /// An erased optional group, a presence with an envelope, `Box<T>` and an inlined
    /// `Option<Box<T>>`.
    #[derive(Debug, Clone)]
    #[tok(LET, optional(ONE), this, SEMI)]
    pub struct Let {
        #[tok(IF, this, EXISTS)]
        #[presence(NOT)]
        pub unless: bool,
        pub name: Name,
        #[tok(EQ)]
        pub value: boxed!(Value),
        pub alias: Option<boxed!(Alias)>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(AS, this)]
    pub struct Alias {
        pub name: Name,
    }
}

recursa::ast_node! {
    /// A bare presence and an `Option<T>` that lowers to a helper.
    #[derive(Debug, Clone)]
    #[tok(PRINT, this, SEMI)]
    pub struct Print {
        #[presence(NOT)]
        pub negated: bool,
        pub value: Value,
        pub guard: Option<Guard>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(IF, this)]
    pub struct Guard {
        pub condition: Value,
    }
}

recursa::ast_node! {
    /// An admitted keyword as content and an inlined `Vec<T>` with a trailing separator.
    #[derive(Debug, Clone)]
    #[tok(this, SEMI)]
    pub struct Call {
        pub callee: Callee,
        pub args: Args,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(LPAREN, this, RPAREN)]
    pub struct Args {
        #[sep(COMMA, trailing)]
        pub values: zero_or_many!(Value),
    }
}

recursa::ast_node! {
    /// A list whose following optional tail owns the same comma. `VARIADIC` cannot begin `Value`,
    /// so the LR parser can leave that comma for the tail without backtracking.
    #[derive(Debug, Clone)]
    #[tok(VTAIL, this, SEMI)]
    pub struct FunctionTail {
        #[sep(COMMA)]
        pub values: one_or_many!(Value),
        pub trailing_variadic: Option<TrailingVariadic>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(COMMA, VARIADIC, this)]
    pub struct TrailingVariadic {
        pub value: Value,
    }
}

recursa::ast_node! {
    /// A `Vec1<T>` with a separator, the gram.y `arg_class` shape and an `Option<Box<T>>` behind
    /// an attachment that lowers to a helper.
    #[derive(Debug, Clone)]
    #[tok(FN, this, SEMI)]
    pub struct Define {
        pub name: Name,
        pub params: Params,
        #[tok(RETURNS, this)]
        pub returns: Option<boxed!(Value)>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(LPAREN, this, RPAREN)]
    pub struct Params {
        #[sep(COMMA)]
        pub params: one_or_many!(Param),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Param {
        pub mode: Option<Mode>,
        pub name: Name,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub enum Mode {
        #[tok(IN)]
        In,
        #[tok(OUT)]
        Out,
    }
}

recursa::ast_node! {
    /// An inlined presence and an inlined `Vec1<T>` with a trailing separator.
    #[derive(Debug, Clone)]
    #[tok(WITH, this, SEMI)]
    pub struct With {
        #[presence(RECURSIVE)]
        pub recursive: bool,
        #[sep(PIPE, trailing)]
        pub names: one_or_many!(Name),
    }
}

recursa::ast_node! {
    /// An inlined `Vec<T>` without a separator, recursive through `Statement`.
    #[derive(Debug, Clone)]
    #[tok(LBRACE, this, RBRACE)]
    pub struct Block {
        pub statements: zero_or_many!(Statement),
    }
}

recursa::ast_node! {
    /// Six fixed-only choices: the two rightmost presences fall back to helpers.
    #[derive(Debug, Clone)]
    #[tok(MODS, optional(ONE), optional(TWO), optional(THREE), this, SEMI)]
    pub struct Modifiers {
        #[presence(FOUR)]
        pub four: bool,
        #[presence(FIVE)]
        pub five: bool,
        #[presence(SIX)]
        pub six: bool,
        pub name: Name,
    }
}

recursa::ast_node! {
    /// Five erased groups: the rightmost falls back to a helper.
    #[derive(Debug, Clone)]
    #[tok(
        ERASED,
        optional(ONE),
        optional(TWO),
        optional(THREE),
        optional(FOUR),
        optional(FIVE),
        this,
        SEMI
    )]
    pub struct Erased {
        pub name: Name,
    }
}

recursa::ast_node! {
    /// Five valued optionals of unit fixed-token nodes: the rightmost falls back to a helper.
    #[derive(Debug, Clone)]
    #[tok(OPTIONS, this, SEMI)]
    pub struct Options {
        pub a: Option<KwAs>,
        pub b: Option<KwIn>,
        pub c: Option<KwOut>,
        pub d: Option<KwOnly>,
        pub e: Option<KwTemp>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(AS)]
    pub struct KwAs;
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(IN)]
    pub struct KwIn;
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(OUT)]
    pub struct KwOut;
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(ONLY)]
    pub struct KwOnly;
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(TEMP)]
    pub struct KwTemp;
}

recursa::ast_node! {
    /// Four erased groups and a trailing separator: the trailing choice falls back to a helper.
    #[derive(Debug, Clone)]
    #[tok(
        TRAIL,
        optional(ONE),
        optional(TWO),
        optional(THREE),
        optional(FOUR),
        this,
        SEMI
    )]
    pub struct Trail {
        #[sep(COMMA, trailing)]
        pub names: one_or_many!(Name),
    }
}

recursa::ast_node! {
    /// An inlined `Vec<Box<T>>` with a separator.
    #[derive(Debug, Clone)]
    #[tok(BOXES, this, SEMI)]
    pub struct Boxes {
        #[sep(COMMA)]
        pub values: zero_or_many!(boxed!(Value)),
    }
}

recursa::ast_node! {
    /// A `Vec<T>` and an `Option<Vec1<T>>` of unit fixed-token nodes, both helpers.
    #[derive(Debug, Clone)]
    #[tok(MARKS, this, SEMI)]
    pub struct Marks {
        pub marks: zero_or_many!(Bang),
        #[tok(COLON)]
        pub tail: Option<one_or_many!(Bang)>,
    }
}

recursa::ast_node! {
    /// A `Vec1<T>` without a separator and a repeated attachment with an erased group inside.
    #[derive(Debug, Clone)]
    #[tok(FILTER, this, SEMI)]
    pub struct Filter {
        pub bangs: one_or_many!(Bang),
        #[tok(COLON, optional(NOT), this_each)]
        pub filters: zero_or_many!(Value),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[flat(pool, max_size = 16)]
    pub enum Value {
        Name { name: Name },
        Number(Number),
        Negated { mark: Bang, inner: boxed!(Value) },
        List(List),
        Tuple(Tuple),
    }
}

recursa::ast_node! {
    /// An inlined `Option<Vec1<T>>` with a separator.
    #[derive(Debug, Clone)]
    #[tok(LBRACKET, this, RBRACKET)]
    pub struct List {
        #[sep(COMMA)]
        pub items: Option<one_or_many!(Value)>,
    }
}

recursa::ast_node! {
    /// A repeated attachment around every element of a `Vec1<T>`.
    #[derive(Debug, Clone)]
    pub struct Tuple {
        #[tok(LPAREN, this_each, RPAREN)]
        #[sep(PIPE)]
        pub cells: one_or_many!(Value),
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(BANG)]
    pub struct Bang;
}

recursa::ast_node! {
    /// A content token whose admission set reclaims three keywords as names.
    #[derive(Debug, Clone)]
    #[flat(pool)]
    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(AS, ONLY, TEMP))]
        pub text: NameText,
    }
}

recursa::ast_node! {
    /// A content token whose admission set reclaims the two parameter modes as callees.
    #[derive(Debug, Clone)]
    pub struct Callee {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(IN, OUT))]
        pub text: CalleeText,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}
