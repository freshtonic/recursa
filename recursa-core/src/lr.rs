//! The table-driven parser's runtime: the LALR(1) automaton loop over generated tables.
//!
//! Generated code supplies data and reduce actions only (`docs/table-driven-parsing.md`,
//! section 3): the packed [`crate::lr::LrTables`] of the whole grammar, one start state and accept rule per
//! grammar type, and one reduce function that builds a value for each rule. [`crate::lr::run`] is the loop
//! that joins them. It keeps two stacks: the state stack, whose frames remember the shifted token
//! so that a content-token rule can read it, and the [`crate::lr::ValueStack`], which holds only semantic
//! values, so fixed tokens cost the value stack nothing.
//!
//! The token feed applies the grammar's lookahead filters (`docs/table-driven-parsing.md`,
//! section 5) at the one place the loop reads a terminal: a record whose kind is a filtered
//! token, followed by a record whose kind is one of its triggers, is looked up as the merged
//! terminal while that lookup can shift or reduce. A row whose merged lookup has no explicit
//! entry or default reduction retains the source terminal, which lets one filtered token serve
//! grammar contexts with different continuations. Nothing else changes: the record is shifted
//! as lexed, so a reduce action and the error path see the lexed kind and span, and the trigger
//! record is read next as itself.
//!
//! Three behaviours are decisions of the design document and are pinned by this module's tests:
//! default reductions are taken before any lookahead is read, so a state that can only reduce
//! never asks for a token; a root parse accepts greedily, reducing its accept rule as soon as the
//! lookahead has no shift action while a shiftable lookahead always shifts; and a failed parse
//! restores the input cursor and drops every partially built value through the value stack's drop
//! stack.

use crate::ParseError;
use crate::input::TokenRecord;
use crate::parsed::{ContentText, Occurrence, ParseContext, ParsedNode};

pub mod lac;
pub mod provenance;
mod tables;
mod value_stack;

use provenance::OccurrenceStack;
pub use tables::{DecodedTables, LrTables, PACT_NINF, TABLE_NINF, TableAction};
pub use value_stack::ValueStack;

/// The token index of a state-stack frame entered by a goto rather than a shift.
const NO_TOKEN: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
/// One state-stack frame: the automaton state and, for a frame entered by a shift, the index of
/// the token shifted into it.
struct Frame {
    /// The automaton state.
    state: u32,
    /// The absolute index of the shifted token, or [`NO_TOKEN`].
    token: u32,
}

/// What one reduce action sees: the value stack, the frames its rule popped, and the parse
/// context for token text and the caller's builder.
///
/// The frames are the rule's `r2` state-stack entries in right-hand-side order, so a token rule
/// (`Ident -> IDENT`) reads its terminal through [`Reduction::token`]. Provenance is not the
/// reduce action's concern: the loop folds the occurrence stack from the rule's static fold
/// after the action ran (see [`provenance`]), so the action only learns through
/// [`Reduction::capturing`] whether a stack exists at all.
pub struct Reduction<'reduction, 'context, 'input, A> {
    /// The value stack; values are popped in reverse right-hand-side order.
    values: &'reduction mut ValueStack,
    /// The popped frames in right-hand-side order.
    frames: &'reduction [Frame],
    /// The parse context, for token text and the builder.
    context: &'reduction mut ParseContext<'context, 'input, A>,
    /// Whether this parse keeps an occurrence stack.
    capturing: bool,
}

impl<'input, A> Reduction<'_, '_, 'input, A> {
    /// Reports whether the parse folds provenance: capture was requested, the root's policy is
    /// mandatory, or the grammar has a mandatory site. When false no occurrence work runs.
    pub fn capturing(&self) -> bool {
        self.capturing
    }

    /// Pushes the value the rule built.
    pub fn push<V>(&mut self, value: V) {
        self.values.push(value);
    }

    /// Pops the next value of the rule, last right-hand-side value first.
    ///
    /// # Safety
    ///
    /// `V` must be the type the value was pushed with; see [`ValueStack::pop`].
    pub unsafe fn pop<V>(&mut self) -> V {
        // SAFETY: forwarded to the caller, whose reduce action names the pushed type.
        unsafe { self.values.pop::<V>() }
    }

    /// Returns the token a rule of one terminal shifted (`r2 == 1`).
    pub fn token(&self) -> TokenRecord {
        let [frame] = self.frames else {
            panic!("`Reduction::token` is for rules of exactly one terminal");
        };
        assert!(
            frame.token != NO_TOKEN,
            "the popped frame was entered by a goto, not a shift"
        );
        self.context
            .record(frame.token as usize)
            .expect("a shifted token index is always a lexed record")
    }

    /// Converts a shifted token into borrowed content-token text.
    pub fn matched_text(&self, record: TokenRecord) -> Result<ContentText<'input>, ParseError> {
        self.context.matched_text(record)
    }

    /// Returns the caller's construction state.
    pub fn builder(&mut self) -> &mut A {
        self.context.builder()
    }
}

/// Runs the automaton from a grammar type's start state and returns its value with the
/// provenance the parse retained.
///
/// `accept_rule` is the type's start rule `S'(T) -> T`; reducing it ends the parse, with the
/// single value on the value stack as the root. `reduce` is the generation's dispatcher for
/// every other rule. On failure the input cursor is restored and every partial value dropped,
/// and the error names the token the automaton could not act on. Its expected set is computed by
/// lookahead correction ([`lac`]) from the state stack as it was before that token triggered any
/// reduction.
///
/// The occurrence stack exists only when the context captures (the request or the root's
/// policy asks for spans) or the grammar has a mandatory site; otherwise no occurrence work runs
/// and the root is returned without provenance.
pub fn run<'input, T, A, F>(
    context: &mut ParseContext<'_, 'input, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    mut reduce: F,
) -> Result<ParsedNode<T>, ParseError>
where
    F: FnMut(u32, &mut Reduction<'_, '_, 'input, A>) -> Result<(), ParseError>,
{
    let checkpoint = context.cursor();
    let mut values = ValueStack::new();
    let mut occurrences =
        (context.captures() || tables.mandatory_provenance()).then(OccurrenceStack::new);
    match drive::<T, A, _>(
        context,
        tables,
        start_state,
        accept_rule,
        &mut reduce,
        &mut values,
        &mut occurrences,
    ) {
        Ok((value, occurrence)) => {
            debug_assert_eq!(values.depth(), 0, "accept leaves exactly one value");
            Ok(ParsedNode::from_parts(value, occurrence))
        }
        Err(error) => {
            values.unwind();
            context.restore(checkpoint);
            Err(error)
        }
    }
}

/// Builds the error for the token at the cursor, with its expected set corrected from the
/// state stack as it was before that token triggered any reduction; `accepts` says whether
/// acceptance counts as a continuation (see [`lac::ErrorSite::accepts`]).
fn unexpected_here<A>(
    context: &ParseContext<'_, '_, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    origin: usize,
    accepts: bool,
) -> ParseError {
    let site = lac::ErrorSite {
        start_state,
        accept_rule,
        origin,
        failing: context.cursor(),
        accepts,
    };
    let expected = lac::expected_set(context, tables, site);
    context.unexpected(&expected)
}

/// The automaton loop proper; `run` wraps it with the transactional restore.
///
/// `tail` tracks whether the current reduction spine requires a following token to belong to the
/// root FOLLOW. After the last shift, reductions up to acceptance are exactly the spine of last
/// symbols, so one flag set by the rules' `tail` table (a one-token enum alternative or repetition
/// tail sets it, while a Pratt rule clears it) decides whether an outside token is an error or the
/// parsed prefix may be accepted.
fn drive<'input, T, A, F>(
    context: &mut ParseContext<'_, 'input, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    reduce: &mut F,
    values: &mut ValueStack,
    occurrences: &mut Option<OccurrenceStack>,
) -> Result<(T, Occurrence), ParseError>
where
    F: FnMut(u32, &mut Reduction<'_, '_, 'input, A>) -> Result<(), ParseError>,
{
    let decoded = tables.decoded();
    let end_marker = tables.end_marker();
    // The one thing the error path needs from the hot path: where this parse began, so that
    // lookahead correction can replay the state stack up to the failing token.
    let origin = context.cursor();
    let mut states = vec![Frame {
        state: start_state,
        token: NO_TOKEN,
    }];
    let mut tail = false;
    loop {
        let state = states
            .last()
            .expect("the state stack keeps the start state")
            .state;
        let action = if decoded.acts_by_default(state) {
            decoded.default_action(state)
        } else {
            match context.peek(0) {
                None => decoded.action(state, end_marker),
                Some(record) if u32::from(record.kind) >= end_marker => TableAction::Error,
                Some(record) => {
                    let next = context.peek(1).map(|next| u32::from(next.kind));
                    decoded.filtered_action(state, u32::from(record.kind), next)
                }
            }
        };
        match action {
            TableAction::Shift(target) => {
                let token = context.cursor() as u32;
                let shifted = context.advance();
                if let Some(stack) = occurrences.as_mut() {
                    let record = shifted.expect("a shift consumes the lookahead it read");
                    stack.shift(context, token, record.span)?;
                }
                states.push(Frame {
                    state: target,
                    token,
                });
                tail = false;
            }
            TableAction::Reduce(rule) if rule == accept_rule => {
                if tail
                    && context
                        .peek(0)
                        .is_some_and(|record| !context.follow().contains(record.kind))
                {
                    return Err(unexpected_here(
                        context,
                        tables,
                        start_state,
                        accept_rule,
                        origin,
                        false,
                    ));
                }
                // SAFETY: the accept rule `S'(T) -> T` pops the root value, which the reduce
                // action of a `T` rule pushed as a `T`.
                let value = unsafe { values.pop::<T>() };
                let occurrence = match occurrences.take() {
                    Some(stack) => stack.into_root(context)?,
                    None => Occurrence::absent(),
                };
                return Ok((value, occurrence));
            }
            TableAction::Reduce(rule) => {
                let pops = decoded.r2[rule as usize] as usize;
                let value_pops = decoded.value_pops[rule as usize] as usize;
                let depth = values.depth();
                debug_assert!(
                    value_pops <= depth,
                    "value_pops never exceeds the values on the stack"
                );
                let kept = states.len() - pops;
                let mut reduction = Reduction {
                    values,
                    frames: &states[kept..],
                    context,
                    capturing: occurrences.is_some(),
                };
                if decoded.reduce_actions[rule as usize] != 0 {
                    reduce(rule, &mut reduction)?;
                }
                debug_assert!(
                    values.depth() == depth - value_pops
                        || values.depth() == depth - value_pops + 1,
                    "a reduce action pops `value_pops` values and pushes at most one"
                );
                if let Some(stack) = occurrences.as_mut() {
                    stack.reduce(context, tables.folds(), rule, pops)?;
                }
                match decoded.tail[rule as usize] {
                    1 => tail = true,
                    2 => tail = false,
                    _ => {}
                }
                states.truncate(kept);
                let top = states
                    .last()
                    .expect("the state stack keeps the start state")
                    .state;
                states.push(Frame {
                    state: decoded.goto(top, decoded.r1[rule as usize]),
                    token: NO_TOKEN,
                });
            }
            TableAction::Error => {
                return Err(unexpected_here(
                    context,
                    tables,
                    start_state,
                    accept_rule,
                    origin,
                    true,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tables::tests::{FALLBACK_FILTERED_TEXTBOOK, FILTERED_TEXTBOOK, TEXTBOOK};
    use super::*;
    use crate::input::LexResult;
    use crate::{GrammarInfo, ParseErrorKind, Span};
    use std::cell::Cell;
    use std::rc::Rc;

    /// Kinds of the textbook grammar's terminals `+ * ( ) id`, plus the lexical-error kind.
    static GRAMMAR: GrammarInfo = GrammarInfo {
        name: "lr-tests",
        kind_names: &["+", "*", "(", ")", "id", "lexical error"],
        atom_names: &["fixed:+", "fixed:*", "fixed:(", "fixed:)", "content:id"],
        lexical_error_kind: 5,
    };

    /// An evaluated expression that counts live values, so a failed parse can prove it dropped
    /// every partial one.
    struct Value {
        number: i64,
        live: Rc<Cell<usize>>,
    }

    impl Value {
        fn new(number: i64, live: &Rc<Cell<usize>>) -> Self {
            live.set(live.get() + 1);
            Self {
                number,
                live: live.clone(),
            }
        }
    }

    impl Drop for Value {
        fn drop(&mut self) {
            self.live.set(self.live.get() - 1);
        }
    }

    /// Evaluates the textbook grammar: `+` adds, `*` multiplies, `id` is a decimal number.
    fn evaluate(
        rule: u32,
        reduction: &mut Reduction<'_, '_, '_, Rc<Cell<usize>>>,
    ) -> Result<(), ParseError> {
        // SAFETY: every pop names the type the child rule pushed, `Value`.
        unsafe {
            match rule {
                1 | 3 => {
                    let right = reduction.pop::<Value>();
                    let left = reduction.pop::<Value>();
                    let number = if rule == 1 {
                        left.number + right.number
                    } else {
                        left.number * right.number
                    };
                    let live = reduction.builder().clone();
                    reduction.push(Value::new(number, &live));
                }
                6 => {
                    let record = reduction.token();
                    let text = reduction.matched_text(record)?;
                    let number = text.as_str().parse().expect("id tokens are numbers");
                    let live = reduction.builder().clone();
                    reduction.push(Value::new(number, &live));
                }
                other => panic!("rule {other} has no semantic reduce action"),
            }
        }
        Ok(())
    }

    /// Lexes one byte per token: digits are `id`, `!` is a lexical error, spaces are skipped.
    fn lex(source: &str) -> LexResult<'_> {
        let records = source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte != b' ')
            .map(|(offset, byte)| TokenRecord {
                kind: match byte {
                    b'+' => 0,
                    b'*' => 1,
                    b'(' => 2,
                    b')' => 3,
                    b'0'..=b'9' => 4,
                    _ => 5,
                },
                span: Span::new(offset, offset + 1).unwrap(),
                diagnostic: None,
            })
            .collect();
        LexResult::new(source, records, None)
    }

    /// Runs the textbook automaton over `source` and reports the value, the cursor after the
    /// parse and the live-value count.
    fn parse(source: &str) -> (Result<i64, ParseError>, usize, usize) {
        let live = Rc::new(Cell::new(0));
        let lexed = lex(source);
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, evaluate)
                .map(|node| node.into_parts().0.number)
        });
        let cursor = input.cursor();
        (result, cursor, live.get())
    }

    #[test]
    fn the_loop_evaluates_the_textbook_expression_with_a_hand_written_reduce() {
        let (result, cursor, live) = parse("2+3*4");
        assert_eq!(result.unwrap(), 14);
        assert_eq!(cursor, 5);
        assert_eq!(
            live, 0,
            "the root value was returned and dropped by the test"
        );
        let (result, cursor, _) = parse("(2+3)*4");
        assert_eq!(result.unwrap(), 20);
        assert_eq!(cursor, 7);
    }

    #[test]
    fn a_root_parse_accepts_the_greedy_prefix_and_leaves_the_rest() {
        // `)` has no shift action once `2+3` is an `E`, so the parse accepts without consuming
        // it; `+` always shifts, so `2+3+4` is consumed whole.
        let (result, cursor, _) = parse("2+3)*4");
        assert_eq!(result.unwrap(), 5);
        assert_eq!(cursor, 3);
        let (result, cursor, _) = parse("2+3+4");
        assert_eq!(result.unwrap(), 9);
        assert_eq!(cursor, 5);
    }

    #[test]
    fn a_failed_parse_names_the_failing_token_restores_the_cursor_and_drops_partial_values() {
        let (result, cursor, live) = parse("2+3*+");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::UnexpectedToken);
        assert_eq!(error.span(), Span::new(4, 5).unwrap());
        assert_eq!(error.found(), Some("+"));
        assert_eq!(cursor, 0);
        assert_eq!(live, 0, "the partial `2`, `3` values were dropped");
    }

    #[test]
    fn a_missing_operand_at_the_end_is_an_unexpected_eof() {
        let (result, cursor, live) = parse("2+");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::UnexpectedEof);
        assert_eq!(error.span(), Span::new(2, 2).unwrap());
        assert_eq!(cursor, 0);
        assert_eq!(live, 0);
        let (result, _, _) = parse("");
        assert_eq!(result.unwrap_err().kind(), ParseErrorKind::UnexpectedEof);
    }

    #[test]
    fn a_lexical_error_token_is_reported_as_one_and_never_shifted() {
        let (result, cursor, live) = parse("2+!");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::LexicalError);
        assert_eq!(error.span(), Span::new(2, 3).unwrap());
        assert_eq!(cursor, 0);
        assert_eq!(live, 0);
    }

    /// Reduces the textbook grammar by recording, for every `F -> id` rule, the lexed kind of
    /// the shifted record; sums record the kinds seen so far.
    fn record_kinds(
        rule: u32,
        reduction: &mut Reduction<'_, '_, '_, Rc<Cell<usize>>>,
    ) -> Result<(), ParseError> {
        // SAFETY: every pop names the type the child rule pushed, `Value`.
        unsafe {
            match rule {
                1 | 3 => {
                    let right = reduction.pop::<Value>();
                    let left = reduction.pop::<Value>();
                    let live = reduction.builder().clone();
                    reduction.push(Value::new(left.number * 10 + right.number, &live));
                }
                2 | 4 | 5 => {}
                6 => {
                    let record = reduction.token();
                    let live = reduction.builder().clone();
                    reduction.push(Value::new(i64::from(record.kind), &live));
                }
                other => panic!("rule {other} is not reduced by the dispatcher"),
            }
        }
        Ok(())
    }

    #[test]
    fn the_feed_substitutes_the_lookup_terminal_and_keeps_the_lexed_record() {
        // Under the synthetic filter `(` before `)` is looked up as `id`, so `()` parses as one
        // `id` whose reduce action still sees the lexed `(` (kind 2); the `)` is then read as
        // itself and, having no action, ends the greedy root parse without being consumed.
        let live = Rc::new(Cell::new(0));
        let lexed = lex("()");
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &FILTERED_TEXTBOOK, 0, 0, record_kinds)
                .map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap(), 2);
        assert_eq!(input.cursor(), 1);
        // `(+` is not filtered (`+` is no trigger), so `(` is looked up as itself and the parse
        // fails on the `+` that cannot open an operand.
        let lexed = lex("(+");
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &FILTERED_TEXTBOOK, 0, 0, record_kinds)
                .map(|node| node.into_parts().0.number)
        });
        let error = result.unwrap_err();
        assert_eq!(error.span(), Span::new(1, 2).unwrap());
        assert_eq!(error.found(), Some("+"));
        // Without a filter the same tables reject `()` at the `)`.
        let lexed = lex("()");
        let mut input = lexed.input_with_builder(&GRAMMAR, live);
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, record_kinds)
                .map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap_err().found(), Some(")"));
    }

    #[test]
    fn a_filter_falls_back_to_its_source_terminal_outside_merged_states() {
        // The filter maps `)` before `+` to `id`, but the state after `(2`
        // has an explicit action only for `)`. The source-token fallback
        // closes the group, after which `+3` continues normally.
        let live = Rc::new(Cell::new(0));
        let lexed = lex("(2)+3");
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &FALLBACK_FILTERED_TEXTBOOK, 0, 0, evaluate)
                .map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap(), 5);
        assert_eq!(input.cursor(), 5);
        assert_eq!(live.get(), 0);
    }

    #[test]
    fn the_expected_set_names_the_terminals_the_pre_reduction_stack_could_shift() {
        // `+` after `*` errors in a state with no reductions, so the row is the whole answer.
        let (result, _, _) = parse("2+3*+");
        let error = result.unwrap_err();
        assert_eq!(
            error.expected().collect::<Vec<_>>(),
            ["fixed:(", "content:id"]
        );
        // An empty input fails in the start state, which shifts `(` and `id`.
        let (result, _, _) = parse("");
        let error = result.unwrap_err();
        assert_eq!(
            error.expected().collect::<Vec<_>>(),
            ["fixed:(", "content:id"]
        );
        // A lexical error is never shifted, so the expected set is still the pre-reduction one.
        let (result, _, _) = parse("2+!");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::LexicalError);
        assert_eq!(
            error.expected().collect::<Vec<_>>(),
            ["fixed:(", "content:id"]
        );
    }

    #[test]
    fn lookahead_correction_recovers_the_continuations_a_default_reduction_hides() {
        // After `(2+3` the end of input takes the default reductions `F -> id`, `T -> F` and
        // `E -> E + T`, and the error is raised in state 6 (`F -> ( E . )`, `E -> E . + T`),
        // whose explicit row is `+` and `)`. But `*` could have continued `T -> T . * F` before
        // the reductions ran, so the expected set must name it too.
        let tables = TEXTBOOK.decoded();
        assert_eq!(
            tables.explicit_terminals(6, 6),
            [0, 3],
            "the post-reduction row hides `*`"
        );
        let (result, cursor, live) = parse("(2+3");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::UnexpectedEof);
        assert_eq!(error.span(), Span::new(4, 4).unwrap());
        assert_eq!(
            error.expected().collect::<Vec<_>>(),
            ["fixed:+", "fixed:*", "fixed:)"]
        );
        assert_eq!(cursor, 0);
        assert_eq!(live, 0);
        // The same state, reached with a token instead of the end: the token is the failing one
        // and the set is the same.
        let (result, _, _) = parse("(2+3 4");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::UnexpectedToken);
        assert_eq!(error.span(), Span::new(5, 6).unwrap());
        assert_eq!(error.found(), Some("id"));
        assert_eq!(
            error.expected().collect::<Vec<_>>(),
            ["fixed:+", "fixed:*", "fixed:)"]
        );
    }

    #[test]
    fn capture_off_keeps_no_occurrence_stack_and_returns_no_provenance() {
        // Every reduce sees `capturing() == false` under the default request with an optional
        // policy, so no occurrence stack exists to be pushed or folded, and the root carries
        // no occurrence.
        fn assert_off(
            rule: u32,
            reduction: &mut Reduction<'_, '_, '_, Rc<Cell<usize>>>,
        ) -> Result<(), ParseError> {
            assert!(!reduction.capturing(), "capture is off");
            evaluate(rule, reduction)
        }
        let live = Rc::new(Cell::new(0));
        let lexed = lex("(2+3)*4");
        let mut input = lexed.input_with_builder(&GRAMMAR, live);
        let occurrence = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, assert_off).map(|node| node.into_parts().1)
        })
        .unwrap();
        assert!(!occurrence.is_captured());
    }

    #[test]
    fn a_capturing_parse_folds_the_generated_occurrence_tree() {
        // With capture requested every reduce sees the stack and the textbook folds build the
        // tree: each rule a node over the rule's extent whose children are its sites, a
        // nonterminal site grouped around the child's node, a terminal site a leaf, and
        // `F -> id` passing its token through to the group that consumes it.
        fn assert_on(
            rule: u32,
            reduction: &mut Reduction<'_, '_, '_, Rc<Cell<usize>>>,
        ) -> Result<(), ParseError> {
            assert!(reduction.capturing(), "capture is on");
            evaluate(rule, reduction)
        }
        let live = Rc::new(Cell::new(0));
        let lexed = lex("2+3*4");
        let mut input = lexed.input_with_builder(&GRAMMAR, live);
        let occurrence = crate::parsed::test_context_spanned(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, assert_on).map(|node| node.into_parts().1)
        })
        .unwrap();
        assert_eq!(
            occurrence.render(),
            "(0..5: (0..1: (0..1: (0..1: (0..1: (0..1: (0..1)))))) (1..2) (2..5: (2..5: (2..3: (2..3: (2..3: (2..3)))) (3..4) (4..5: (4..5)))))"
        );
        // A failed capturing parse drops the stack with everything else.
        let lexed = lex("2+3*+");
        let mut input = lexed.input_with_builder(&GRAMMAR, Rc::new(Cell::new(0)));
        let error = crate::parsed::test_context_spanned(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, assert_on).map(|_| ())
        })
        .unwrap_err();
        assert_eq!(error.span(), Span::new(4, 5).unwrap());
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn a_reduce_error_is_transactional_too() {
        // A record whose span leaves the source makes `matched_text` fail inside the reduce.
        let live = Rc::new(Cell::new(0));
        let lexed = LexResult::new(
            "2+3",
            vec![
                TokenRecord {
                    kind: 4,
                    span: Span::new(0, 1).unwrap(),
                    diagnostic: None,
                },
                TokenRecord {
                    kind: 0,
                    span: Span::new(1, 2).unwrap(),
                    diagnostic: None,
                },
                TokenRecord {
                    kind: 4,
                    span: Span::new(2, 9).unwrap(),
                    diagnostic: None,
                },
            ],
            None,
        );
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, evaluate)
                .map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap_err().kind(), ParseErrorKind::InvalidTokenSpan);
        assert_eq!(input.cursor(), 0);
        assert_eq!(live.get(), 0);
    }
}
