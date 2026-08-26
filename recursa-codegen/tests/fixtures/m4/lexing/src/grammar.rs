recursa::grammar! {
    module = crate::grammar,
    keyword_matching = ascii_insensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+|--[^\n]*",
    post_lex = crate::grammar::repair,
    keywords {
        SELECT => "select",
        NULL => "null",
        TRUE => "true",
        FALSE => "false",
        CALLBACK_KEYWORD => "kk",
    }
    punctuation {
        COLON => ":",
        NOT_EQUAL => "!=",
        LEGIT_NOT_EQUAL => "!=-",
    }
}

static POST_LEX_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub fn reset_post_lex_calls() {
    POST_LEX_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
}

pub fn post_lex_calls() -> usize {
    POST_LEX_CALLS.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn repair(editor: &mut LexEditor<'_>) {
    POST_LEX_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(before) = editor.record(0) {
        let outside = recursa::Span::new(0, editor.source().len() + 1).unwrap();
        assert!(
            editor
                .replace(0, [(before.kind(), outside)])
                .is_err()
        );
        assert_eq!(editor.record(0), Some(before));
        assert!(
            editor
                .replace(
                    0,
                    [(before.kind(), before.span()), (before.kind(), before.span())],
                )
                .is_err()
        );
        assert_eq!(editor.record(0), Some(before));
    }
    assert!(editor.remove(editor.len()..editor.len() + 1).is_err());
    assert!(
        editor
            .reclassify(editor.len(), TokenKind::LexicalError)
            .is_err()
    );
    let mut index = 0;
    while let Some(record) = editor.record(index) {
        match record.kind() {
            TokenKind::PseudoText
                if record
                    .text()
                    .get(1..)
                    .is_some_and(|body| {
                        body.eq_ignore_ascii_case("NULL")
                            || body.eq_ignore_ascii_case("TRUE")
                            || body.eq_ignore_ascii_case("FALSE")
                    }) =>
            {
                let span = record.span();
                editor
                    .replace(
                        index,
                        [
                            (
                                TokenKind::COLON,
                                recursa::Span::new(span.start() as usize, span.start() as usize + 1)
                                    .unwrap(),
                            ),
                            (
                                TokenKind::WordText,
                                recursa::Span::new(span.start() as usize + 1, span.end() as usize)
                                    .unwrap(),
                            ),
                        ],
                    )
                    .expect("a pseudo-variable can be split into adjacent source records");
                index += 2;
            }
            TokenKind::PseudoText if record.text().eq_ignore_ascii_case(":ERROR") => {
                editor
                    .reclassify(index, TokenKind::LexicalError)
                    .expect("reclassification updates the authoritative error view");
                index += 1;
            }
            TokenKind::BrokenOperator => {
                let span = record.span();
                let comment_start = span.start() as usize + 2;
                let comment_end = editor.source()[comment_start..]
                    .find('\n')
                    .map_or(editor.source().len(), |offset| comment_start + offset);
                editor
                    .replace(
                        index,
                        [(
                            TokenKind::NOT_EQUAL,
                            recursa::Span::new(span.start() as usize, span.start() as usize + 2)
                                .unwrap(),
                        )],
                    )
                    .expect("the broken operator record can be shortened before the comment");
                let mut comment_records_end = index + 1;
                while editor
                    .record(comment_records_end)
                    .is_some_and(|record| (record.span().start() as usize) < comment_end)
                {
                    comment_records_end += 1;
                }
                editor
                    .remove(index + 1..comment_records_end)
                    .expect("records emitted inside the line comment are contiguous");
                index += 1;
            }
            _ => index += 1,
        }
    }
}

pub fn scan_word(context: &mut LexContext<'_>) -> LexAction {
    if let Some(character) = context.remainder().chars().next()
        && character.len_utf8() > 1
    {
        assert!(context.bump(1).is_err());
        context
            .bump(character.len_utf8())
            .expect("a callback may bump over one complete UTF-8 scalar");
        return LexAction::Emit;
    }
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
    #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(SELECT, NULL))]
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
        pattern = r"[~kK]",
        callback = crate::grammar::scan_word,
        priority = 2
    )]
    pub name: CallbackText<'input>,
}

#[derive(recursa::Node)]
pub struct PseudoSelection<'input> {
    #[lex(pattern = r#":(?:[A-Za-z_][A-Za-z0-9_]*|'[^']*'|"[^"]*")"#)]
    pub name: PseudoText<'input>,
}

#[derive(recursa::Node)]
pub struct QuotedSelection<'input> {
    #[lex(pattern = r"'[^']*'")]
    pub name: QuotedText<'input>,
}

#[derive(recursa::Node)]
pub struct BrokenOperatorSelection<'input> {
    #[lex(pattern = r"!=--")]
    pub name: BrokenOperator<'input>,
}
