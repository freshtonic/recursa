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

pub fn scan_word(context: &mut LexContext<'_>) -> LexAction {
    let extra = context
        .remainder()
        .bytes()
        .take_while(u8::is_ascii_alphabetic)
        .count();
    assert!(context.bump(context.remainder().len() + 1).is_err());
    context
        .bump(extra)
        .expect("the callback may extend through the remaining ASCII word");
    match context.slice()[1..].to_ascii_lowercase().as_str() {
        "as" => LexAction::EmitAs(TokenKind::SELECT),
        "skip" => LexAction::Skip,
        "error" => LexAction::Error,
        _ => LexAction::Emit,
    }
}

#[derive(recursa::Node)]
#[tok(SELECT, this)]
pub struct Selection<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub name: WordText<'input>,
}

#[derive(recursa::Node)]
pub struct LowSelection<'input> {
    #[lex(pattern = r"~", priority = 1)]
    pub name: LowText<'input>,
}

#[derive(recursa::Node)]
pub struct CallbackSelection<'input> {
    #[lex(
        pattern = r"~",
        callback = crate::grammar::scan_word,
        priority = 2
    )]
    pub name: CallbackText<'input>,
}
