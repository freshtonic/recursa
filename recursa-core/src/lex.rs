use regex_automata::meta::Regex;
use regex_automata::{Anchored, Input as RegexInput};
use std::fmt;

use crate::{LexResult, Span, TokenKind, TokenRecord};

/// One matching-version generated lexical rule.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct LexRule {
    pub pattern: &'static str,
    pub kind: TokenKind,
    pub skip: bool,
    pub callback: Option<for<'source> fn(&mut LexContext<'source>) -> LexAction>,
    pub fixed: bool,
    pub priority: u32,
}

/// Backend-independent state exposed to one generated lexer-callback shim.
#[doc(hidden)]
pub struct LexContext<'source> {
    source: &'source str,
    start: usize,
    end: usize,
}

impl<'source> LexContext<'source> {
    pub fn source(&self) -> &'source str {
        self.source
    }

    pub const fn start(&self) -> usize {
        self.start
    }

    pub const fn end(&self) -> usize {
        self.end
    }

    pub fn slice(&self) -> &'source str {
        &self.source[self.start..self.end]
    }

    pub fn remainder(&self) -> &'source str {
        &self.source[self.end..]
    }

    pub fn bump(&mut self, bytes: usize) -> Result<(), LexBumpError> {
        let Some(end) = self.end.checked_add(bytes) else {
            return Err(LexBumpError);
        };
        if end > self.source.len() || !self.source.is_char_boundary(end) {
            return Err(LexBumpError);
        }
        self.end = end;
        Ok(())
    }
}

/// One erased callback decision after a generated typed shim has run.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexAction {
    Emit,
    EmitAs(TokenKind),
    Skip,
    Error,
}

/// A callback attempted to advance outside the remaining source or between
/// UTF-8 code units.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexBumpError;

impl fmt::Display for LexBumpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("lexer callback bump is outside the remaining UTF-8 source")
    }
}

impl std::error::Error for LexBumpError {}

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
            .filter_map(|(matcher, rule)| {
                matcher
                    .find(RegexInput::new(remaining).anchored(Anchored::Yes))
                    .filter(|found| found.start() == 0 && found.end() > 0)
                    .map(|found| (found.end(), *rule))
            })
            .max_by_key(|(length, rule)| (*length, rule.fixed, rule.priority));

        let (end, emitted_kind) = selected.map_or_else(
            || {
                let length = remaining
                    .chars()
                    .next()
                    .expect("offset is before source end")
                    .len_utf8();
                (offset + length, Some(lexical_error_kind))
            },
            |(length, rule)| {
                let mut context = LexContext {
                    source,
                    start: offset,
                    end: offset + length,
                };
                let action = rule.callback.map_or_else(
                    || {
                        if rule.skip {
                            LexAction::Skip
                        } else {
                            LexAction::Emit
                        }
                    },
                    |callback| callback(&mut context),
                );
                let kind = match action {
                    LexAction::Emit => Some(rule.kind),
                    LexAction::EmitAs(kind) => Some(kind),
                    LexAction::Skip => None,
                    LexAction::Error => Some(lexical_error_kind),
                };
                (context.end, kind)
            },
        );
        let span = Span::new(offset, end).expect("source accepted by LexResult fits checked spans");
        if let Some(kind) = emitted_kind {
            records.push(TokenRecord { kind, span });
        }
        offset = end;
    }
    LexResult::new(source, records)
}
