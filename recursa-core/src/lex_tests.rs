//! Driver-level lexical tests over hand-written state machines.
//!
//! Generation owns the machine a real grammar uses; these tests fix the
//! driver's own contracts — the trivia sidecar, the one-record-per-scalar
//! failure rule, and operator-fence suppression — against small machines that
//! exercise them directly.

use logos::Logos;

use crate::lex::{
    ByteSet, FixedLexLiteral, LexClosedMatch, LexExtras, LexPlan, LexRule, LexTokenIndex,
    OperatorRunRule, lex, lex_nested, lex_physical_line, scan_closed,
};

/// Packs one ASCII alphabet into the runtime's membership words.
const fn byte_set(characters: &str) -> ByteSet {
    let bytes = characters.as_bytes();
    let mut words = [0_u64; 4];
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        words[(byte >> 6) as usize] |= 1_u64 << (byte & 63);
        index += 1;
    }
    ByteSet(words)
}

/// Builds one emitting rule record.
const fn emitting(kind: u16, fixed: bool, rank: u32) -> LexRule {
    LexRule {
        kind,
        skip: false,
        trivia_kind: None,
        fixed,
        rank,
    }
}

/// Builds one skipped rule record.
const fn skipped(rank: u32) -> LexRule {
    LexRule {
        kind: 0,
        skip: true,
        trivia_kind: None,
        fixed: false,
        rank,
    }
}

/// Returns the emitted kind and extent of every record.
fn records(result: &crate::input::LexResult<'_>) -> Vec<(u16, u32, u32)> {
    result
        .records()
        .iter()
        .map(|record| (record.kind, record.span.start(), record.span.end()))
        .collect()
}

/// A machine whose only rule is ignored whitespace.
#[derive(Clone, Copy, Debug, Logos)]
#[logos(extras = LexExtras)]
enum WhitespaceToken {
    #[regex(r"\s+", allow_greedy = true, priority = 1)]
    Whitespace,
}

impl LexTokenIndex for WhitespaceToken {
    fn rule_index(&self) -> usize {
        match self {
            Self::Whitespace => 0,
        }
    }
}

/// Plan for [`WhitespaceToken`].
static WHITESPACE_PLAN: LexPlan = LexPlan {
    rules: &[skipped(1)],
    fixed_literals: &[],
    operator_runs: &[],
    closed_first_bytes: ByteSet([0; 4]),
    closed_scan: None,
    lexical_error_kind: 1,
    retains_trivia: false,
};

#[test]
fn non_retaining_grammar_keeps_no_trivia_sidecar_or_gap_projection() {
    let result = lex::<WhitespaceToken>(" \n", &WHITESPACE_PLAN);

    assert!(result.trivia().is_none());
    assert!(result.source_gaps().is_none());
    assert!(result.records().is_empty());
}

#[test]
fn every_unrecognized_scalar_becomes_its_own_error_record() {
    let result = lex::<WhitespaceToken>("é?", &WHITESPACE_PLAN);

    assert_eq!(records(&result), vec![(1, 0, 2), (1, 2, 3)]);
}

/// A machine covering the PostgreSQL operator-versus-comment arbitration.
#[derive(Clone, Copy, Debug, Logos)]
#[logos(extras = LexExtras)]
enum OperatorToken {
    #[token("||/", priority = 7)]
    PipePipeSlash,
    #[token("||", priority = 6)]
    PipePipe,
    #[token("/", priority = 5)]
    Slash,
    #[regex(r"[A-Za-z]+", allow_greedy = true, priority = 1)]
    Word,
}

impl LexTokenIndex for OperatorToken {
    fn rule_index(&self) -> usize {
        match self {
            Self::PipePipeSlash => 0,
            Self::PipePipe => 1,
            Self::Slash => 2,
            Self::Word => 3,
        }
    }
}

/// The closed-region machine holding the nested block comment.
#[derive(Clone, Copy, Debug, Logos)]
#[logos(extras = LexExtras)]
enum OperatorClosedToken {
    #[token("/*", callback = |lexer| lex_nested(lexer, "/*", "*/"), priority = 3)]
    BlockComment,
    #[regex("--", callback = lex_physical_line, allow_greedy = true, priority = 4)]
    LineComment,
}

impl LexTokenIndex for OperatorClosedToken {
    fn rule_index(&self) -> usize {
        match self {
            Self::BlockComment => 0,
            Self::LineComment => 1,
        }
    }
}

/// Rules for [`OperatorToken`], indexed by variant.
///
/// Ranks reproduce the runtime's total order: every fixed rule outranks every
/// pattern rule, and pattern rules order by their authored priority.
static OPERATOR_RULES: [LexRule; 4] = [
    emitting(10, true, 7),
    emitting(11, true, 6),
    emitting(12, true, 5),
    emitting(13, false, 1),
];

/// Rules for [`OperatorClosedToken`], indexed by variant.
static OPERATOR_CLOSED_RULES: [LexRule; 2] = [skipped(3), skipped(4)];

/// Scans the closed regions of the operator machine.
fn scan_operator_closed(source: &str) -> Option<LexClosedMatch> {
    scan_closed::<OperatorClosedToken>(source, &OPERATOR_CLOSED_RULES)
}

/// The custom-operator run fenced by both comment introducers.
static OPERATOR_RUNS: [OperatorRunRule; 1] = [OperatorRunRule {
    characters: byte_set("|/-*"),
    fence_starts: byte_set("/-"),
    fences: &["/*", "--"],
    trailing: byte_set("-"),
    qualifying: byte_set("|"),
    rule: emitting(14, false, 2),
}];

/// Plan for [`OperatorToken`].
static OPERATOR_PLAN: LexPlan = LexPlan {
    rules: &OPERATOR_RULES,
    fixed_literals: &[
        FixedLexLiteral {
            spelling: "||/",
            ascii_insensitive: false,
            rule: OPERATOR_RULES[0],
        },
        FixedLexLiteral {
            spelling: "||",
            ascii_insensitive: false,
            rule: OPERATOR_RULES[1],
        },
        FixedLexLiteral {
            spelling: "/",
            ascii_insensitive: false,
            rule: OPERATOR_RULES[2],
        },
    ],
    operator_runs: &OPERATOR_RUNS,
    closed_first_bytes: byte_set("/-"),
    closed_scan: Some(scan_operator_closed),
    lexical_error_kind: 99,
    retains_trivia: false,
};

#[test]
fn a_fixed_operator_may_not_swallow_a_valid_comment_introducer() {
    let terminated = lex::<OperatorToken>("x||/*c*/y", &OPERATOR_PLAN);

    assert_eq!(
        records(&terminated),
        vec![(13, 0, 1), (11, 1, 3), (13, 8, 9)],
    );
}

#[test]
fn an_unterminated_comment_introducer_leaves_the_longest_fixed_operator() {
    let unterminated = lex::<OperatorToken>("x||/*c", &OPERATOR_PLAN);

    assert_eq!(records(&unterminated)[..2], [(13, 0, 1), (10, 1, 4)]);
}

#[test]
fn a_closed_region_arbitrates_on_the_extent_its_callback_reaches() {
    let commented = lex::<OperatorToken>("a/*b*/c", &OPERATOR_PLAN);

    assert_eq!(records(&commented), vec![(13, 0, 1), (13, 6, 7)]);
}
