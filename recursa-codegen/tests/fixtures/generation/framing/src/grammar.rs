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

recursa::ast_node! {

    pub enum Entry {
        #[tok(PUT, this)]
        Put(Name),
        #[tok(COPY, this)]
        CopyFromStdin(Name),
        #[tok(PUT, LPAREN, this, RPAREN)]
        Nested(Pair),
        Tagged(TagTriple),
    }
}

// Three declared semantic targets share one content base kind.
recursa::ast_node! {

    pub struct TagTriple {
        #[lex(pattern = r"[A-Z]+")]
        pub tag: TagName,
        #[lex(pattern = r"[A-Z]+")]
        pub value: TagValue,
        #[lex(pattern = r"[A-Z]+")]
        pub unit: TagUnit,
    }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(matcher)]
        pub text: Quoted,
    }
}

recursa::ast_node! {

    pub struct StructEntry {
        #[tok(PUT, this)]
        pub name: Name,
    }
}

recursa::ast_node! {

    pub struct Pair {
        #[tok(this, SEMI)]
        pub first: Name,
        pub second: Name,
    }
}

// Materialize the framing-only atomic kinds without admitting them to Entry.
recursa::ast_node! {

    pub struct AtomicRegions {
        #[lex(matcher)]
        pub line: MetaLine,
        #[lex(matcher)]
        pub raw: RawBlock,
    }
}
