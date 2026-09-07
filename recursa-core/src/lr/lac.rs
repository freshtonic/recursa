//! Lookahead correction: the expected set of a table-driven parse error.
//!
//! LALR(1) with default reductions may detect an error only after reductions triggered by the
//! failing token, so the final state's row can name fewer
//! continuations than were possible: after `( id + id` the end of input reduces `T -> F` and
//! `E -> E + T` before failing in `F -> ( E . )`, whose row is `+` and `)`, although `*` could
//! have continued `T -> T . * F`. Bison's lookahead correction (`parse.lac`) answers this by
//! exploring from the stack as it was before those reductions. Recursa does the same on the
//! error path only.
//!
//! The pre-reduction stack is not kept on the hot path. The state stack at any cursor is a
//! function of the tokens consumed since the parse began, so [`expected_set`] replays the
//! automaton, states only, from the parse's origin to the moment the failing token was first
//! read (default reductions taken without a lookahead do not depend on the token, so replaying
//! them is exact), then simulates every terminal of the grammar on a scratch copy of that stack
//! until it is shifted, accepted or rejected. The terminals that are shifted or accepted form
//! the expected set; each is named by the canonical atom of its token kind, so
//! [`ParseError::expected`](crate::ParseError::expected) renders stable names. The end marker is not explored: it has no atom
//! and no kind bit, so nothing could report it. A terminal whose exploration reaches the accept
//! rule counts as a continuation, since the parse would have ended without an error on it.

use super::tables::{DecodedTables, LrTables, TableAction};
use crate::parsed::ParseContext;
use crate::{ExpectedAtomId, FollowSet};

/// The most reductions one terminal's exploration may take before it counts as rejected.
///
/// A reduction never consumes input, so an exploration ends only by shifting, accepting or
/// erroring; the bound keeps a grammar with a reduce cycle (`A -> B`, `B -> A`) from looping
/// here, where the real parse would loop as well.
const EXPLORATION_BUDGET: usize = 1 << 16;

#[derive(Clone, Copy, Debug)]
/// Where a parse failed, in the terms the replay needs.
pub struct ErrorSite {
    /// The start state the parse ran from.
    pub start_state: u32,
    /// The rule whose reduction accepts this parse.
    pub accept_rule: u32,
    /// The cursor the parse began at.
    pub origin: usize,
    /// The cursor of the token the automaton could not act on.
    pub failing: usize,
    /// Whether reaching the accept rule counts as continuing the parse. It does for an error
    /// the automaton raised, where the accepted prefix would have ended the parse before the
    /// token; it does not for a token rejected after acceptance, whose expected set is what
    /// could have been shifted instead.
    pub accepts: bool,
}

/// Computes the expected set of a failed parse by lookahead correction.
///
/// Replays the state stack to the point the failing token was first read, then explores every
/// terminal from it; see the module documentation.
pub fn expected_set<A>(
    context: &ParseContext<'_, '_, A>,
    tables: &LrTables,
    site: ErrorSite,
) -> FollowSet {
    let decoded = tables.decoded();
    let stack = replay(context, decoded, site);
    let atoms = tables.expected_atoms();
    let end_marker = tables.end_marker();
    let next = context
        .record(site.failing + 1)
        .map(|record| u32::from(record.kind));
    let kind_count = context.grammar().kind_count();
    let mut words = vec![0_u64; kind_count.div_ceil(64)];
    let mut expected = Vec::new();
    let mut scratch = Scratch::over(&stack);
    for terminal in 0..=end_marker {
        if terminal == end_marker || !scratch.continues(decoded, terminal, next, &site) {
            continue;
        }
        let index = terminal as usize;
        if index < kind_count {
            words[index / 64] |= 1 << (index % 64);
        }
        if let Some(atom) = atoms.get(index).copied().filter(|atom| *atom != 0) {
            expected.push(ExpectedAtomId(atom - 1));
        }
    }
    expected.sort_unstable();
    expected.dedup();
    FollowSet::owned(kind_count as u32, words, expected)
}

/// Rebuilds the state stack as it was when the failing token was first read.
///
/// Every token before `site.failing` was shifted by the real parse, so the replay never meets
/// an error or the accept rule before the failing cursor; both arms return the stack anyway
/// rather than panic, since the caller is already reporting an error.
fn replay<A>(
    context: &ParseContext<'_, '_, A>,
    decoded: &DecodedTables,
    site: ErrorSite,
) -> Vec<u32> {
    let mut stack = vec![site.start_state];
    let mut cursor = site.origin;
    loop {
        let state = *stack
            .last()
            .expect("the replayed stack keeps the start state");
        let action = if decoded.acts_by_default(state) {
            decoded.default_action(state)
        } else {
            if cursor == site.failing {
                return stack;
            }
            let record = context
                .record(cursor)
                .expect("every token before the failing one was shifted, so it is a lexed record");
            let next = context.record(cursor + 1).map(|next| u32::from(next.kind));
            decoded.filtered_action(state, u32::from(record.kind), next)
        };
        match action {
            TableAction::Shift(target) => {
                stack.push(target);
                cursor += 1;
            }
            TableAction::Reduce(rule) if rule == site.accept_rule => return stack,
            TableAction::Reduce(rule) => reduce(decoded, &mut stack, rule),
            TableAction::Error => return stack,
        }
    }
}

/// Pops a rule's right-hand side and pushes its goto.
fn reduce(decoded: &DecodedTables, stack: &mut Vec<u32>, rule: u32) {
    let kept = stack.len() - decoded.r2[rule as usize] as usize;
    stack.truncate(kept);
    let top = *stack
        .last()
        .expect("a reduction never pops the start state");
    stack.push(decoded.goto(top, decoded.r1[rule as usize]));
}

/// A scratch state stack over an immutable base: pops shorten the visible base, pushes go to
/// the overlay, so exploring a terminal never copies the base and resetting is free.
struct Scratch<'base> {
    /// The pre-reduction stack.
    base: &'base [u32],
    /// How much of `base` is still visible.
    base_len: usize,
    /// The frames pushed above the visible base.
    top: Vec<u32>,
}

impl<'base> Scratch<'base> {
    /// Starts a scratch stack showing the whole base.
    fn over(base: &'base [u32]) -> Self {
        Self {
            base,
            base_len: base.len(),
            top: Vec::new(),
        }
    }

    /// Reports whether `terminal` can continue the parse from the base stack: it is shifted, or
    /// accepted when the site counts acceptance, after some reductions, rather than rejected.
    fn continues(
        &mut self,
        decoded: &DecodedTables,
        terminal: u32,
        next: Option<u32>,
        site: &ErrorSite,
    ) -> bool {
        self.base_len = self.base.len();
        self.top.clear();
        for _ in 0..EXPLORATION_BUDGET {
            match decoded.filtered_action(self.state(), terminal, next) {
                TableAction::Shift(_) => return true,
                TableAction::Reduce(rule) if rule == site.accept_rule => return site.accepts,
                TableAction::Reduce(rule) => {
                    self.pop(decoded.r2[rule as usize] as usize);
                    let target = decoded.goto(self.state(), decoded.r1[rule as usize]);
                    self.top.push(target);
                }
                TableAction::Error => return false,
            }
        }
        false
    }

    /// The state on top of the scratch stack.
    fn state(&self) -> u32 {
        match self.top.last() {
            Some(state) => *state,
            None => self.base[self.base_len - 1],
        }
    }

    /// Pops `count` frames, cutting into the base once the overlay is exhausted.
    fn pop(&mut self, count: usize) {
        let from_top = count.min(self.top.len());
        self.top.truncate(self.top.len() - from_top);
        let from_base = count - from_top;
        debug_assert!(
            from_base < self.base_len,
            "a reduction never pops the start state"
        );
        self.base_len -= from_base;
    }
}
