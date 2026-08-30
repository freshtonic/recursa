use regex_automata::meta::Regex;
use regex_automata::{Anchored, Input as RegexInput};

use crate::{LexResult, Span, TokenKind, TokenRecord};

/// One matching-version generated lexical rule.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct LexRule {
    /// Validated matcher executed at the current source offset.
    pub matcher: LexMatcher,
    /// Grammar-local kind emitted by the default action.
    pub kind: TokenKind,
    /// Whether the match is ignored as trivia.
    pub skip: bool,
    /// Whether this rule originated from exact fixed syntax.
    pub fixed: bool,
    /// Deterministic tie-break priority after match length and fixed status.
    pub priority: u32,
}

/// One matching-version generated lexical matcher.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum LexMatcher {
    /// Anchored regular expression.
    Regex(
        /// Validated regular-expression source.
        &'static str,
    ),
    /// Region closed by a byte-identical copy of its matched opener.
    SameDelimiter {
        /// Validated regular expression matching the complete opening delimiter.
        opener: &'static str,
    },
    /// Region whose fixed opening and closing delimiters may nest.
    Nested {
        /// Nonempty fixed opening delimiter.
        opener: &'static str,
        /// Nonempty fixed closing delimiter distinct from `opener`.
        closer: &'static str,
    },
    /// Regular expression whose match is rejected when the following scalar matches another expression.
    NextExclusion {
        /// Validated regular expression matching the candidate token.
        pattern: &'static str,
        /// Validated regular expression matching one excluded following scalar.
        excluded: &'static str,
    },
    /// Maximal run over a configured ASCII alphabet with generic fence and trailing rules.
    OperatorRun {
        /// ASCII characters admitted to the run.
        characters: &'static str,
        /// Nonempty byte strings that stop the run before their first occurrence.
        fences: &'static [&'static str],
        /// Run characters stripped from the end when no qualifying character occurs earlier.
        trailing: &'static str,
        /// Characters whose presence preserves configured trailing characters.
        qualifying: &'static str,
    },
    /// Source region extending from a matched prefix through one physical line.
    PhysicalLine {
        /// Validated regular expression matching the region's nonempty prefix.
        prefix: &'static str,
    },
}

/// Stable classification attached to one generated lexical failure.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexDiagnosticCode {
    /// No generated matcher accepted the current source scalar.
    Unrecognized,
    /// A same-delimiter region reached end of input without its matching closer.
    UnterminatedSameDelimiter,
    /// A nested region reached end of input with nonzero depth.
    UnterminatedNested,
    /// A checked matcher invariant failed before a supported lexical result could be formed.
    MatcherInvariant,
    /// A token candidate was followed immediately by an excluded scalar.
    ExcludedFollowingCharacter,
}

impl LexDiagnosticCode {
    /// Returns the append-only Recursa diagnostic-code spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unrecognized => "RCA4000",
            Self::MatcherInvariant => "RCA4000",
            Self::UnterminatedSameDelimiter => "RCA4001",
            Self::UnterminatedNested => "RCA4002",
            Self::ExcludedFollowingCharacter => "RCA4004",
        }
    }
}

/// Structured metadata for one rejected lexical source range.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexDiagnostic {
    /// Stable mechanism-specific failure code.
    pub code: LexDiagnosticCode,
    /// Primary source range identifying the construct that began the failure.
    pub anchor: Span,
}

/// Relative diagnostic metadata produced before an absolute checked span exists.
#[derive(Clone, Copy)]
struct RelativeLexDiagnostic {
    /// Stable mechanism-specific failure code.
    code: LexDiagnosticCode,
    /// Inclusive anchor start relative to the current source offset.
    anchor_start: usize,
    /// Exclusive anchor end relative to the current source offset.
    anchor_end: usize,
}

/// One matcher decision with its final byte extent already known.
#[derive(Clone, Copy)]
struct LexMatch {
    /// Exclusive byte extent relative to the current source offset.
    end: usize,
    /// Failure metadata when the matched extent must become an error record.
    diagnostic: Option<RelativeLexDiagnostic>,
}

/// One operator-run family's decision at the current source offset.
struct OperatorRunDecision {
    /// ASCII alphabet that relates fixed candidates to this operator family.
    characters: &'static str,
    /// Final matcher extent after the family's fence and trailing rules.
    final_extent: Option<usize>,
    /// Interior fence offset whose complete spelling is consumed by valid trivia.
    consumed_fence: Option<usize>,
}

impl OperatorRunDecision {
    /// Reports whether this family admits every byte of a fixed candidate.
    fn admits(&self, candidate: &[u8]) -> bool {
        candidate
            .iter()
            .all(|byte| self.characters.as_bytes().contains(byte))
    }
}

/// Compiled state reused while scanning one source.
enum CompiledLexMatcher {
    /// Compiled anchored regular expression.
    Regex(Regex),
    /// Compiled opening-delimiter expression.
    SameDelimiter {
        /// Anchored expression that returns the exact delimiter bytes to repeat.
        opener: Regex,
    },
    /// Validated fixed delimiters for nested scanning.
    Nested {
        /// Nonempty opening delimiter.
        opener: &'static str,
        /// Nonempty closing delimiter.
        closer: &'static str,
    },
    /// Compiled candidate and following-scalar exclusion expressions.
    NextExclusion {
        /// Candidate token expression.
        pattern: Regex,
        /// Excluded following-scalar expression.
        excluded: Regex,
    },
    /// Validated ASCII operator-run configuration.
    OperatorRun {
        /// ASCII characters admitted to the run.
        characters: &'static str,
        /// Nonempty byte strings that stop the run.
        fences: &'static [&'static str],
        /// Conditionally stripped trailing characters.
        trailing: &'static str,
        /// Characters that preserve configured trailing characters.
        qualifying: &'static str,
    },
    /// Compiled physical-line prefix expression.
    PhysicalLine {
        /// Prefix expression anchored at the current source offset.
        prefix: Regex,
    },
}

impl CompiledLexMatcher {
    /// Compiles one generated matcher whose authored inputs were already validated.
    fn new(matcher: LexMatcher) -> Self {
        match matcher {
            LexMatcher::Regex(pattern) => {
                Self::Regex(Regex::new(pattern).expect("generated lexical patterns were validated"))
            }
            LexMatcher::SameDelimiter { opener } => Self::SameDelimiter {
                opener: Regex::new(opener)
                    .expect("generated same-delimiter openers were validated"),
            },
            LexMatcher::Nested { opener, closer } => Self::Nested { opener, closer },
            LexMatcher::NextExclusion { pattern, excluded } => Self::NextExclusion {
                pattern: Regex::new(pattern)
                    .expect("generated next-exclusion patterns were validated"),
                excluded: Regex::new(excluded)
                    .expect("generated next-exclusion predicates were validated"),
            },
            LexMatcher::OperatorRun {
                characters,
                fences,
                trailing,
                qualifying,
            } => Self::OperatorRun {
                characters,
                fences,
                trailing,
                qualifying,
            },
            LexMatcher::PhysicalLine { prefix } => Self::PhysicalLine {
                prefix: Regex::new(prefix)
                    .expect("generated physical-line prefixes were validated"),
            },
        }
    }

    /// Returns a decision carrying the final byte extent from `remaining`'s start.
    fn matched(&self, remaining: &str) -> Option<LexMatch> {
        let anchored = |matcher: &Regex| {
            matcher
                .find(RegexInput::new(remaining).anchored(Anchored::Yes))
                .filter(|found| found.start() == 0 && found.end() > 0)
        };
        match self {
            Self::Regex(matcher) => anchored(matcher).map(|found| LexMatch {
                end: found.end(),
                diagnostic: None,
            }),
            Self::SameDelimiter { opener } => {
                let opened = anchored(opener)?;
                let delimiter = &remaining[..opened.end()];
                let body = &remaining[opened.end()..];
                let Some(close) = body.find(delimiter) else {
                    return Some(LexMatch {
                        end: remaining.len(),
                        diagnostic: Some(RelativeLexDiagnostic {
                            code: LexDiagnosticCode::UnterminatedSameDelimiter,
                            anchor_start: 0,
                            anchor_end: opened.end(),
                        }),
                    });
                };
                Some(LexMatch {
                    end: opened.end() + close + delimiter.len(),
                    diagnostic: None,
                })
            }
            Self::Nested { opener, closer } => {
                remaining.strip_prefix(opener)?;
                let mut depth = 1_u32;
                let mut cursor = opener.len();
                while cursor < remaining.len() {
                    let suffix = &remaining[cursor..];
                    if suffix.starts_with(closer) {
                        cursor += closer.len();
                        depth -= 1;
                        if depth == 0 {
                            return Some(LexMatch {
                                end: cursor,
                                diagnostic: None,
                            });
                        }
                    } else if suffix.starts_with(opener) {
                        cursor += opener.len();
                        let Some(incremented) = increment_nested_depth(depth) else {
                            return Some(LexMatch {
                                end: remaining.len(),
                                diagnostic: Some(RelativeLexDiagnostic {
                                    code: LexDiagnosticCode::MatcherInvariant,
                                    anchor_start: 0,
                                    anchor_end: opener.len(),
                                }),
                            });
                        };
                        depth = incremented;
                    } else {
                        cursor += suffix
                            .chars()
                            .next()
                            .expect("a nonempty source suffix has one scalar")
                            .len_utf8();
                    }
                }
                Some(LexMatch {
                    end: remaining.len(),
                    diagnostic: Some(RelativeLexDiagnostic {
                        code: LexDiagnosticCode::UnterminatedNested,
                        anchor_start: 0,
                        anchor_end: opener.len(),
                    }),
                })
            }
            Self::NextExclusion { pattern, excluded } => {
                let found = anchored(pattern)?;
                let following = remaining[found.end()..]
                    .chars()
                    .next()
                    .map(|character| &remaining[found.end()..found.end() + character.len_utf8()]);
                let rejected = following.is_some_and(|scalar| {
                    excluded
                        .find(RegexInput::new(scalar).anchored(Anchored::Yes))
                        .is_some_and(|next| next.start() == 0 && next.end() == scalar.len())
                });
                Some(LexMatch {
                    end: found.end(),
                    diagnostic: rejected.then_some(RelativeLexDiagnostic {
                        code: LexDiagnosticCode::ExcludedFollowingCharacter,
                        anchor_start: 0,
                        anchor_end: found.end(),
                    }),
                })
            }
            Self::OperatorRun {
                characters,
                fences,
                trailing,
                qualifying,
            } => {
                let admitted = characters.as_bytes();
                let mut end = 0;
                while let Some(&byte) = remaining.as_bytes().get(end) {
                    if fences
                        .iter()
                        .any(|fence| remaining[end..].starts_with(fence))
                        || !admitted.contains(&byte)
                    {
                        break;
                    }
                    end += 1;
                }
                if end == 0 {
                    return None;
                }
                let trailing = trailing.as_bytes();
                let qualifying = qualifying.as_bytes();
                if end > 1
                    && trailing.contains(&remaining.as_bytes()[end - 1])
                    && !remaining.as_bytes()[..end - 1]
                        .iter()
                        .any(|byte| qualifying.contains(byte))
                {
                    while end > 1 && trailing.contains(&remaining.as_bytes()[end - 1]) {
                        end -= 1;
                    }
                }
                Some(LexMatch {
                    end,
                    diagnostic: None,
                })
            }
            Self::PhysicalLine { prefix } => {
                let opened = anchored(prefix)?;
                let suffix = &remaining.as_bytes()[opened.end()..];
                let end = suffix
                    .iter()
                    .position(|byte| matches!(byte, b'\r' | b'\n'))
                    .map_or(remaining.len(), |line_end| {
                        let line_end = opened.end() + line_end;
                        if remaining.as_bytes()[line_end] == b'\r'
                            && remaining.as_bytes().get(line_end + 1) == Some(&b'\n')
                        {
                            line_end + 2
                        } else {
                            line_end + 1
                        }
                    });
                Some(LexMatch {
                    end,
                    diagnostic: None,
                })
            }
        }
    }

    /// Computes this operator family's decision and any trivia-consumed interior fence.
    fn operator_run_decision(
        &self,
        remaining: &str,
        skip_matchers: &[&Self],
    ) -> Option<OperatorRunDecision> {
        let Self::OperatorRun {
            characters, fences, ..
        } = self
        else {
            return None;
        };

        let admitted = characters.as_bytes();
        let mut fence = None;
        let mut end = 0;
        while let Some(byte) = remaining.as_bytes().get(end) {
            let fence_length = fences
                .iter()
                .filter(|fence| remaining.as_bytes()[end..].starts_with(fence.as_bytes()))
                .map(|fence| fence.len())
                .min();
            if let Some(fence_length) = fence_length {
                fence = (end > 0).then_some((end, fence_length));
                break;
            }
            if !admitted.contains(byte) {
                break;
            }
            end += 1;
        }

        let consumed_fence = fence.and_then(|(start, fence_length)| {
            skip_matchers
                .iter()
                .any(|matcher| {
                    matcher.matched(&remaining[start..]).is_some_and(|found| {
                        found.diagnostic.is_none() && found.end >= fence_length
                    })
                })
                .then_some(start)
        });
        Some(OperatorRunDecision {
            characters,
            final_extent: self.matched(remaining).map(|found| found.end),
            consumed_fence,
        })
    }
}

/// Reports whether valid trailing trivia makes an operator-shaped fixed candidate ineligible.
fn fixed_candidate_crosses_consumed_fence(
    candidate: &[u8],
    operator_runs: &[OperatorRunDecision],
) -> bool {
    let mut related = operator_runs
        .iter()
        .filter(|operator_run| operator_run.admits(candidate));
    let reaches_candidate = related.clone().any(|operator_run| {
        operator_run
            .final_extent
            .is_some_and(|end| end >= candidate.len())
    });
    !reaches_candidate
        && related.any(|operator_run| {
            operator_run
                .consumed_fence
                .is_some_and(|start| start < candidate.len())
        })
}

/// Advances the source-domain nesting counter without allowing an impossible wraparound.
fn increment_nested_depth(depth: u32) -> Option<u32> {
    depth.checked_add(1)
}

/// Executes a small generated lexical rule table.
///
/// Generation's complete lexer emitter may replace this private implementation without
/// changing grammar APIs.
#[doc(hidden)]
pub fn lex<'input>(
    source: &'input str,
    rules: &'static [LexRule],
    lexical_error_kind: TokenKind,
) -> LexResult<'input> {
    let compiled = rules
        .iter()
        .map(|rule| (CompiledLexMatcher::new(rule.matcher), *rule))
        .collect::<Vec<_>>();
    let skip_matchers = compiled
        .iter()
        .filter(|(_, rule)| rule.skip)
        .map(|(matcher, _)| matcher)
        .collect::<Vec<_>>();
    let operator_matchers = compiled
        .iter()
        .filter_map(|(matcher, _)| {
            matches!(matcher, CompiledLexMatcher::OperatorRun { .. }).then_some(matcher)
        })
        .collect::<Vec<_>>();
    let mut operator_decisions = Vec::with_capacity(operator_matchers.len());
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let remaining = &source[offset..];
        operator_decisions.clear();
        operator_decisions.extend(operator_matchers.iter().map(|matcher| {
            matcher
                .operator_run_decision(remaining, &skip_matchers)
                .expect("operator matcher produces an operator decision")
        }));
        let selected = compiled
            .iter()
            .filter_map(|(matcher, rule)| matcher.matched(remaining).map(|found| (found, *rule)))
            .filter(|(found, rule)| {
                !rule.fixed
                    || !fixed_candidate_crosses_consumed_fence(
                        &remaining.as_bytes()[..found.end],
                        &operator_decisions,
                    )
            })
            .max_by_key(|(found, rule)| (found.end, rule.fixed, rule.priority));

        let (end, emitted_kind, diagnostic) = selected.map_or_else(
            || {
                let length = remaining
                    .chars()
                    .next()
                    .expect("offset is before source end")
                    .len_utf8();
                let end = offset + length;
                (
                    end,
                    Some(lexical_error_kind),
                    Some(LexDiagnostic {
                        code: LexDiagnosticCode::Unrecognized,
                        anchor: Span::new(offset, end)
                            .expect("one source scalar is a checked UTF-8 span"),
                    }),
                )
            },
            |(found, rule)| {
                if let Some(rejected) = found.diagnostic {
                    let end = offset + found.end;
                    return (
                        end,
                        Some(lexical_error_kind),
                        Some(LexDiagnostic {
                            code: rejected.code,
                            anchor: Span::new(
                                offset + rejected.anchor_start,
                                offset + rejected.anchor_end,
                            )
                            .expect("generated matcher anchors are checked UTF-8 spans"),
                        }),
                    );
                }
                (offset + found.end, (!rule.skip).then_some(rule.kind), None)
            },
        );
        let span = Span::new(offset, end).expect("source accepted by LexResult fits checked spans");
        if let Some(kind) = emitted_kind {
            records.push(TokenRecord {
                kind,
                span,
                diagnostic,
            });
        }
        offset = end;
    }
    LexResult::new(source, records)
}
