recursa::grammar! {
    module = crate::grammar,
    spans = mandatory,
    framing(
        island = Entry,
        boundary = SEMI,
        line = [MetaLine],
        delimited = [RawBlock],
        payload(after = Entry::CopyFromStdin, terminator = "\\.")
    ),
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\n\r]+",
        LineComment => r"--[^\n]*",
    }
    keywords {
        PUT => "put",
        COPY => "copy",
    }
    punctuation {
        SEMI => ";",
        LPAREN => "(",
        RPAREN => ")",
    }
    matchers {
        Quoted => same_delimiter(opener = r#"'"#),
        MetaLine => physical_line(prefix = r"\\[A-Za-z]+"),
        RawBlock => same_delimiter(opener = r"\$raw\$"),
    }
}

#[derive(recursa::Node)]
pub enum Entry<'input> {
    #[tok(PUT, this)]
    Put(Name<'input>),
    #[tok(COPY, this)]
    CopyFromStdin(Name<'input>),
    #[tok(PUT, LPAREN, this, RPAREN)]
    Nested(Pair<'input>),
    Tagged(TagTriple<'input>),
}

// Three declared semantic targets share one content base kind.
#[derive(recursa::Node)]
pub struct TagTriple<'input> {
    #[lex(pattern = r"[A-Z]+")]
    pub tag: TagName<'input>,
    #[lex(pattern = r"[A-Z]+")]
    pub value: TagValue<'input>,
    #[lex(pattern = r"[A-Z]+")]
    pub unit: TagUnit<'input>,
}

#[derive(recursa::Node)]
pub struct Name<'input> {
    #[lex(matcher)]
    pub text: Quoted<'input>,
}

#[derive(recursa::Node)]
pub struct StructEntry<'input> {
    #[tok(PUT, this)]
    pub name: Name<'input>,
}

#[derive(recursa::Node)]
pub struct Pair<'input> {
    #[tok(this, SEMI)]
    pub first: Name<'input>,
    pub second: Name<'input>,
}

// Materialize the framing-only atomic kinds without admitting them to Entry.
#[derive(recursa::Node)]
pub struct AtomicRegions<'input> {
    #[lex(matcher)]
    pub line: MetaLine<'input>,
    #[lex(matcher)]
    pub raw: RawBlock<'input>,
}
