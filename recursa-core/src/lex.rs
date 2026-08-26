use regex_automata::meta::Regex;
use regex_automata::{Anchored, Input as RegexInput};

use crate::{LexResult, Span, TokenKind, TokenRecord};

/// One matching-version generated lexical rule.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct LexRule {
    pub pattern: &'static str,
    pub kind: TokenKind,
    pub skip: bool,
}

/// Executes a small generated lexical rule table.
///
/// M4's complete lexer emitter may replace this private implementation without
/// changing grammar APIs.
#[doc(hidden)]
pub fn lex<'input>(
    source: &'input str,
    rules: &'static [LexRule],
    lexical_error_kind: TokenKind,
) -> LexResult<'input> {
    let compiled = rules
        .iter()
        .map(|rule| {
            (
                Regex::new(rule.pattern).expect("generated lexical patterns were validated"),
                *rule,
            )
        })
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let remaining = &source[offset..];
        let selected = compiled
            .iter()
            .enumerate()
            .filter_map(|(priority, (matcher, rule))| {
                matcher
                    .find(RegexInput::new(remaining).anchored(Anchored::Yes))
                    .filter(|found| found.start() == 0 && found.end() > 0)
                    .map(|found| (found.end(), priority, *rule))
            })
            .max_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));

        let (length, kind, skip) = selected.map_or_else(
            || {
                let length = remaining
                    .chars()
                    .next()
                    .expect("offset is before source end")
                    .len_utf8();
                (length, lexical_error_kind, false)
            },
            |(length, _, rule)| (length, rule.kind, rule.skip),
        );
        let end = offset + length;
        let span = Span::new(offset, end).expect("source accepted by LexResult fits checked spans");
        if !skip {
            records.push(TokenRecord { kind, span });
        }
        offset = end;
    }
    LexResult::new(source, records)
}
