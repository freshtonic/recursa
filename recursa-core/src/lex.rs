//! Generated lexing driven by one `logos` state machine per grammar.
//!
//! Generation emits a private `#[derive(Logos)]` enum with one variant per
//! lexical rule and a matching `LexPlan`. `lex` drives that machine and
//! converts every decision into the records, trivia, and diagnostics the
//! grammar's public lexing API exposes. The `logos` dependency is an internal
//! implementation detail: grammar authors never name it.
//!
//! Two lexical mechanisms cannot be expressed by a single longest-match state
//! machine and stay in this driver:
//!
//! * **Operator runs.** A run's extent depends on the first interior fence and
//!   on a trailing-character rule that inspects the characters already
//!   consumed. Both shorten the match after the fact, which a `logos` callback
//!   cannot do, so runs are scanned here and arbitrated against the state
//!   machine's winner.
//! * **Operator-fence suppression.** PostgreSQL rejects a fixed operator
//!   candidate that would swallow a comment introducer when the comment is
//!   valid. Deciding that requires lexing at the fence, which is a second
//!   decision at a later offset.
//! * **Closed regions.** A same-delimiter, nested, or physical-line region
//!   arbitrates on the extent its callback reaches, not on the extent of its
//!   opener. A single machine can only arbitrate on the opener, so closed
//!   regions get their own machine, consulted at every offset whose first byte
//!   can open one.

use logos::Logos;

use crate::input::{LexResult, TokenRecord, TriviaRecord};
use crate::{Span, TokenKind};

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

/// Failure metadata a generated callback attaches to the token it just matched.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexFailure {
    /// Stable mechanism-specific failure code.
    pub code: LexDiagnosticCode,
    /// Inclusive anchor start relative to the matched token's start.
    pub anchor_start: usize,
    /// Exclusive anchor end relative to the matched token's start.
    pub anchor_end: usize,
}

/// Per-lexer scratch shared between generated callbacks and this driver.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LexExtras {
    /// Failure recorded while scanning the token currently being matched.
    pub failure: Option<LexFailure>,
}

/// Maps one generated token variant to its rule index.
///
/// Generation implements this on the private `#[derive(Logos)]` enum; the
/// implementation is a dense match over unit variants.
#[doc(hidden)]
pub trait LexTokenIndex {
    /// Returns this variant's index into [`LexPlan::rules`].
    fn rule_index(&self) -> usize;
}

/// A 256-bit ASCII-and-byte membership set emitted by generation.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteSet(
    /// Little-endian membership words covering bytes `0..=255`.
    pub [u64; 4],
);

impl ByteSet {
    /// Reports whether `byte` belongs to this set.
    #[inline]
    pub const fn contains(self, byte: u8) -> bool {
        self.0[(byte >> 6) as usize] & (1_u64 << (byte & 63)) != 0
    }

    /// Reports whether every byte of `candidate` belongs to this set.
    fn admits(self, candidate: &[u8]) -> bool {
        let mut index = 0;
        while index < candidate.len() {
            if !self.contains(candidate[index]) {
                return false;
            }
            index += 1;
        }
        true
    }
}

/// The outcome one generated lexical rule assigns to its matched extent.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexRule {
    /// Grammar-local kind emitted by the default action.
    pub kind: TokenKind,
    /// Whether the match is excluded from the significant-token stream.
    pub skip: bool,
    /// Classified trivia kind retained outside the significant-token stream.
    pub trivia_kind: Option<u16>,
    /// Whether this rule originated from exact fixed syntax.
    pub fixed: bool,
    /// Total arbitration order after match length; higher wins and is unique.
    pub rank: u32,
}

/// One fixed spelling recoverable after operator-fence suppression.
///
/// Suppression can only reject fixed candidates, so recovering the next-best
/// candidate needs the complete fixed alphabet rather than the single winner
/// the state machine reports. Generation emits this table only for grammars
/// that declare an operator run.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct FixedLexLiteral {
    /// Exact authored spelling.
    pub spelling: &'static str,
    /// Whether the spelling matches under ASCII-case equivalence.
    pub ascii_insensitive: bool,
    /// Outcome assigned when this spelling wins arbitration.
    pub rule: LexRule,
}

/// A maximal run over a configured ASCII alphabet with fence and trailing rules.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct OperatorRunRule {
    /// Bytes admitted to the run.
    pub characters: ByteSet,
    /// First bytes of every configured fence.
    pub fence_starts: ByteSet,
    /// Nonempty byte strings that stop the run before their first occurrence.
    pub fences: &'static [&'static str],
    /// Run characters stripped from the end when no qualifying character occurs earlier.
    pub trailing: ByteSet,
    /// Characters whose presence preserves configured trailing characters.
    pub qualifying: ByteSet,
    /// Outcome assigned when this run wins arbitration.
    pub rule: LexRule,
}

/// The complete static lexical description of one generated grammar.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct LexPlan {
    /// Rule outcomes indexed by generated token variant.
    pub rules: &'static [LexRule],
    /// Fixed spellings consulted only by operator-fence recovery.
    pub fixed_literals: &'static [FixedLexLiteral],
    /// Operator-run families arbitrated outside the state machine.
    pub operator_runs: &'static [OperatorRunRule],
    /// First bytes that can open a closed region.
    pub closed_first_bytes: ByteSet,
    /// Closed-region machine, absent when the grammar declares no closed region.
    pub closed_scan: Option<for<'source> fn(&'source str) -> Option<LexClosedMatch>>,
    /// Kind emitted for every rejected source range.
    pub lexical_error_kind: TokenKind,
    /// Whether any rule classifies retained trivia.
    pub retains_trivia: bool,
}

/// One closed region's decision at the current source offset.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct LexClosedMatch {
    /// Exclusive byte extent relative to the region's start.
    pub end: usize,
    /// Outcome the winning region assigns.
    pub rule: LexRule,
    /// Failure metadata when the matched extent must become an error record.
    pub failure: Option<LexFailure>,
}

/// Scans the best closed region opening at the start of `source`.
#[doc(hidden)]
pub fn scan_closed<'source, Token>(
    source: &'source str,
    rules: &'static [LexRule],
) -> Option<LexClosedMatch>
where
    Token: Logos<'source, Source = str, Extras = LexExtras> + LexTokenIndex,
{
    let mut lexer = Token::lexer(source);
    match lexer.next() {
        Some(Ok(token)) => Some(LexClosedMatch {
            end: lexer.span().end,
            rule: rules[token.rule_index()],
            failure: lexer.extras.failure,
        }),
        Some(Err(_)) | None => None,
    }
}

/// Extends a same-delimiter region through a byte-identical copy of its opener.
#[doc(hidden)]
pub fn lex_same_delimiter<'source, Token>(lexer: &mut logos::Lexer<'source, Token>)
where
    Token: Logos<'source, Source = str, Extras = LexExtras>,
{
    let delimiter = lexer.slice();
    let body = lexer.remainder();
    match body.find(delimiter) {
        Some(close) => lexer.bump(close + delimiter.len()),
        None => {
            lexer.extras.failure = Some(LexFailure {
                code: LexDiagnosticCode::UnterminatedSameDelimiter,
                anchor_start: 0,
                anchor_end: delimiter.len(),
            });
            lexer.bump(body.len());
        }
    }
}

/// Extends a region whose fixed opening and closing delimiters may nest.
#[doc(hidden)]
pub fn lex_nested<'source, Token>(
    lexer: &mut logos::Lexer<'source, Token>,
    opener: &str,
    closer: &str,
) where
    Token: Logos<'source, Source = str, Extras = LexExtras>,
{
    let body = lexer.remainder();
    let mut depth = 1_u32;
    let mut cursor = 0_usize;
    while cursor < body.len() {
        let suffix = &body[cursor..];
        if suffix.starts_with(closer) {
            cursor += closer.len();
            depth -= 1;
            if depth == 0 {
                lexer.bump(cursor);
                return;
            }
        } else if suffix.starts_with(opener) {
            cursor += opener.len();
            let Some(incremented) = depth.checked_add(1) else {
                lexer.extras.failure = Some(LexFailure {
                    code: LexDiagnosticCode::MatcherInvariant,
                    anchor_start: 0,
                    anchor_end: opener.len(),
                });
                lexer.bump(body.len());
                return;
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
    lexer.extras.failure = Some(LexFailure {
        code: LexDiagnosticCode::UnterminatedNested,
        anchor_start: 0,
        anchor_end: opener.len(),
    });
    lexer.bump(body.len());
}

/// Extends a matched prefix through the end of its physical line.
#[doc(hidden)]
pub fn lex_physical_line<'source, Token>(lexer: &mut logos::Lexer<'source, Token>)
where
    Token: Logos<'source, Source = str, Extras = LexExtras>,
{
    let suffix = lexer.remainder().as_bytes();
    let length = suffix
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))
        .map_or(suffix.len(), |line_end| {
            if suffix[line_end] == b'\r' && suffix.get(line_end + 1) == Some(&b'\n') {
                line_end + 2
            } else {
                line_end + 1
            }
        });
    lexer.bump(length);
}

/// Rejects a matched candidate whose following scalar falls in `excluded`.
#[doc(hidden)]
pub fn lex_next_exclusion<'source, Token>(
    lexer: &mut logos::Lexer<'source, Token>,
    excluded: &[(char, char)],
) where
    Token: Logos<'source, Source = str, Extras = LexExtras>,
{
    let Some(following) = lexer.remainder().chars().next() else {
        return;
    };
    if excluded
        .iter()
        .any(|&(low, high)| following >= low && following <= high)
    {
        let span = lexer.span();
        lexer.extras.failure = Some(LexFailure {
            code: LexDiagnosticCode::ExcludedFollowingCharacter,
            anchor_start: 0,
            anchor_end: span.end - span.start,
        });
    }
}

/// One operator-run family's decision at the current source offset.
#[derive(Clone, Copy)]
struct OperatorRunDecision {
    /// Index of the decided family in [`LexPlan::operator_runs`].
    index: usize,
    /// Final matcher extent after the family's fence and trailing rules.
    final_extent: usize,
    /// Interior fence offset and byte length before which this operator run stops.
    interior_fence: Option<(usize, usize)>,
    /// Whether ordinary local arbitration selects valid trivia over this fence.
    fence_consumed: Option<bool>,
}

/// One arbitrated candidate with its final byte extent already known.
#[derive(Clone, Copy)]
struct LexChoice {
    /// Exclusive byte extent relative to the current source offset.
    end: usize,
    /// Outcome the winning rule assigns.
    rule: LexRule,
    /// Failure metadata when the matched extent must become an error record.
    failure: Option<LexFailure>,
}

/// Replaces `best` when `challenger` wins the final-extent and rank ordering.
#[inline]
fn consider(best: &mut Option<LexChoice>, challenger: LexChoice) {
    if best.is_none_or(|held| (challenger.end, challenger.rule.rank) > (held.end, held.rule.rank)) {
        *best = Some(challenger);
    }
}

/// Scans one operator-run family at the start of `remaining`.
///
/// Returns the final extent after the fence and trailing rules together with
/// the first interior fence encountered, or `None` when the family does not
/// admit the first byte.
fn scan_operator_run(
    remaining: &str,
    run: &OperatorRunRule,
) -> Option<(usize, Option<(usize, usize)>)> {
    let bytes = remaining.as_bytes();
    let mut interior_fence = None;
    let mut end = 0;
    while let Some(&byte) = bytes.get(end) {
        if run.fence_starts.contains(byte) {
            let fence_length = run
                .fences
                .iter()
                .filter(|fence| bytes[end..].starts_with(fence.as_bytes()))
                .map(|fence| fence.len())
                .min();
            if let Some(fence_length) = fence_length {
                interior_fence = (end > 0).then_some((end, fence_length));
                break;
            }
        }
        if !run.characters.contains(byte) {
            break;
        }
        end += 1;
    }
    if end == 0 {
        return None;
    }
    if end > 1
        && run.trailing.contains(bytes[end - 1])
        && !bytes[..end - 1]
            .iter()
            .any(|&byte| run.qualifying.contains(byte))
    {
        while end > 1 && run.trailing.contains(bytes[end - 1]) {
            end -= 1;
        }
    }
    Some((end, interior_fence))
}

/// Collects every operator-run decision applicable at the start of `remaining`.
fn operator_run_decisions(
    remaining: &str,
    plan: &LexPlan,
    decisions: &mut Vec<OperatorRunDecision>,
) {
    decisions.clear();
    for (index, run) in plan.operator_runs.iter().enumerate() {
        let Some((final_extent, interior_fence)) = scan_operator_run(remaining, run) else {
            continue;
        };
        decisions.push(OperatorRunDecision {
            index,
            final_extent,
            interior_fence,
            fence_consumed: None,
        });
    }
}

/// Folds the closed-region machine's decision into `best`.
fn consider_closed_region(best: &mut Option<LexChoice>, remaining: &str, plan: &LexPlan) {
    let Some(scan) = plan.closed_scan else {
        return;
    };
    let Some(&first) = remaining.as_bytes().first() else {
        return;
    };
    if !plan.closed_first_bytes.contains(first) {
        return;
    }
    let Some(found) = scan(remaining) else {
        return;
    };
    consider(
        best,
        LexChoice {
            end: found.end,
            rule: found.rule,
            failure: found.failure,
        },
    );
}

/// Selects one raw lexical winner without applying operator-fence suppression.
fn select_raw<'source, Token>(source: &'source str, plan: &LexPlan) -> Option<LexChoice>
where
    Token: Logos<'source, Source = str, Extras = LexExtras> + LexTokenIndex,
{
    let mut lexer = Token::lexer(source);
    let mut best = match lexer.next() {
        Some(Ok(token)) => Some(LexChoice {
            end: lexer.span().end,
            rule: plan.rules[token.rule_index()],
            failure: lexer.extras.failure,
        }),
        Some(Err(_)) | None => None,
    };
    consider_closed_region(&mut best, source, plan);
    for run in plan.operator_runs {
        let Some((end, _)) = scan_operator_run(source, run) else {
            continue;
        };
        consider(
            &mut best,
            LexChoice {
                end,
                rule: run.rule,
                failure: None,
            },
        );
    }
    best
}

/// Reports whether valid trailing trivia consumes this family's interior fence.
fn fence_is_consumed<'source, Token>(
    source: &'source str,
    offset: usize,
    decision: &mut OperatorRunDecision,
    plan: &LexPlan,
) -> bool
where
    Token: Logos<'source, Source = str, Extras = LexExtras> + LexTokenIndex,
{
    let Some((boundary, fence_length)) = decision.interior_fence else {
        return false;
    };
    if let Some(consumed) = decision.fence_consumed {
        return consumed;
    }
    let local = select_raw::<Token>(&source[offset + boundary..], plan);
    let consumed = local.is_some_and(|found| {
        found.rule.skip && found.failure.is_none() && found.end >= fence_length
    });
    decision.fence_consumed = Some(consumed);
    consumed
}

/// Reports whether valid trailing trivia makes a fixed candidate ineligible.
///
/// This reproduces PostgreSQL's operator-versus-comment rule: a fixed operator
/// spelling may not swallow a comment introducer that ordinary arbitration
/// would recognize as valid trivia.
fn crosses_consumed_fence<'source, Token>(
    source: &'source str,
    offset: usize,
    candidate: &[u8],
    decisions: &mut [OperatorRunDecision],
    plan: &LexPlan,
) -> bool
where
    Token: Logos<'source, Source = str, Extras = LexExtras> + LexTokenIndex,
{
    let related = |decision: &OperatorRunDecision| {
        plan.operator_runs[decision.index]
            .characters
            .admits(candidate)
    };
    if decisions
        .iter()
        .any(|decision| related(decision) && decision.final_extent >= candidate.len())
    {
        return false;
    }
    for decision in decisions.iter_mut() {
        if !related(decision) {
            continue;
        }
        if decision
            .interior_fence
            .is_none_or(|(start, _)| start >= candidate.len())
        {
            continue;
        }
        if fence_is_consumed::<Token>(source, offset, decision, plan) {
            return true;
        }
    }
    false
}

/// Reports the matched length of one fixed spelling at the start of `remaining`.
fn fixed_literal_length(remaining: &str, literal: &FixedLexLiteral) -> Option<usize> {
    let length = literal.spelling.len();
    let candidate = remaining.as_bytes().get(..length)?;
    let matched = if literal.ascii_insensitive {
        candidate.eq_ignore_ascii_case(literal.spelling.as_bytes())
    } else {
        candidate == literal.spelling.as_bytes()
    };
    matched.then_some(length)
}

/// Selects the best candidate that survives operator-fence suppression.
///
/// Suppression only ever rejects fixed candidates, so the eligible set is the
/// complete fixed alphabet filtered by the suppression rule, together with the
/// operator runs themselves.
fn recover_after_suppression<'source, Token>(
    source: &'source str,
    offset: usize,
    decisions: &mut [OperatorRunDecision],
    plan: &LexPlan,
) -> Option<LexChoice>
where
    Token: Logos<'source, Source = str, Extras = LexExtras> + LexTokenIndex,
{
    let remaining = &source[offset..];
    let mut best = None;
    for literal in plan.fixed_literals {
        let Some(length) = fixed_literal_length(remaining, literal) else {
            continue;
        };
        if crosses_consumed_fence::<Token>(
            source,
            offset,
            &remaining.as_bytes()[..length],
            decisions,
            plan,
        ) {
            continue;
        }
        consider(
            &mut best,
            LexChoice {
                end: length,
                rule: literal.rule,
                failure: None,
            },
        );
    }
    for decision in decisions.iter() {
        consider(
            &mut best,
            LexChoice {
                end: decision.final_extent,
                rule: plan.operator_runs[decision.index].rule,
                failure: None,
            },
        );
    }
    consider_closed_region(&mut best, remaining, plan);
    best
}

/// Executes one grammar's generated lexical state machine over `source`.
///
/// Generation's complete lexer emitter may replace this private implementation
/// without changing grammar APIs.
#[doc(hidden)]
pub fn lex<'input, Token>(source: &'input str, plan: &LexPlan) -> LexResult<'input>
where
    Token: Logos<'input, Source = str, Extras = LexExtras> + LexTokenIndex,
{
    let mut records = Vec::new();
    let mut trivia = plan.retains_trivia.then(Vec::new);
    let mut decisions: Vec<OperatorRunDecision> = Vec::new();
    let mut base = 0_usize;
    let mut lexer = Token::lexer(source);
    let mut offset = 0_usize;
    while offset < source.len() {
        let remaining = &source[offset..];
        lexer.extras.failure = None;
        let mut selected = match lexer.next() {
            Some(Ok(token)) => Some(LexChoice {
                end: lexer.span().end + base - offset,
                rule: plan.rules[token.rule_index()],
                failure: lexer.extras.failure,
            }),
            Some(Err(_)) | None => None,
        };
        let machine_end = base + lexer.span().end;
        consider_closed_region(&mut selected, remaining, plan);
        if !plan.operator_runs.is_empty() {
            operator_run_decisions(remaining, plan, &mut decisions);
            for decision in &decisions {
                consider(
                    &mut selected,
                    LexChoice {
                        end: decision.final_extent,
                        rule: plan.operator_runs[decision.index].rule,
                        failure: None,
                    },
                );
            }
            if let Some(found) = selected
                && found.rule.fixed
                && crosses_consumed_fence::<Token>(
                    source,
                    offset,
                    &remaining.as_bytes()[..found.end],
                    &mut decisions,
                    plan,
                )
            {
                selected = recover_after_suppression::<Token>(source, offset, &mut decisions, plan);
            }
        }

        let (end, emitted_kind, retained_kind, diagnostic) = selected.map_or_else(
            || {
                let length = remaining
                    .chars()
                    .next()
                    .expect("offset is before source end")
                    .len_utf8();
                let end = offset + length;
                (
                    end,
                    Some(plan.lexical_error_kind),
                    None,
                    Some(LexDiagnostic {
                        code: LexDiagnosticCode::Unrecognized,
                        anchor: Span::new(offset, end)
                            .expect("one source scalar is a checked UTF-8 span"),
                    }),
                )
            },
            |found| {
                if let Some(rejected) = found.failure {
                    let end = offset + found.end;
                    return (
                        end,
                        Some(plan.lexical_error_kind),
                        None,
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
                (
                    offset + found.end,
                    (!found.rule.skip).then_some(found.rule.kind),
                    found.rule.trivia_kind,
                    None,
                )
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
        if let Some(kind) = retained_kind {
            trivia
                .as_mut()
                .expect("a retained rule enables the trivia sidecar")
                .push(TriviaRecord { kind, span });
        }
        if machine_end != end {
            base = end;
            lexer = Token::lexer(&source[base..]);
        }
        offset = end;
    }
    LexResult::new(source, records, trivia)
}
