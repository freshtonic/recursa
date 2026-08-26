recursa::grammar! {
    module = crate::grammar,
    keyword_matching = ascii_insensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        SELECT => "select",
    }
}

#[derive(recursa::Node)]
#[tok(SELECT, this)]
pub struct Selection<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub name: WordText<'input>,
}
