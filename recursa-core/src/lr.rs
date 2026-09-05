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
//! terminal. Nothing else changes: the record is shifted as lexed, so a reduce action and the
//! error path see the lexed kind and span, and the trigger record is read next as itself.
//!
//! Three behaviours are decisions of the design document and are pinned by this module's tests:
//! default reductions are taken before any lookahead is read, so a state that can only reduce
//! never asks for a token; a root parse accepts greedily, reducing its accept rule as soon as the
//! lookahead has no shift action while a shiftable lookahead always shifts, which is the boundary
//! recursive descent produces; and a failed parse restores the input cursor and drops every
//! partially built value through the value stack's drop stack.

use crate::input::TokenRecord;
use crate::parsed::{ContentText, ParseContext, ParsedNode};
use crate::{FollowSet, ParseError};

mod tables;
mod value_stack;

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

/// The reduce dispatcher of one generation: builds the value of `rule` from the reduction
/// context and pushes it, or returns the error a content token's text raised.
pub type ReduceFn<'input, A> =
    fn(rule: u32, reduction: &mut Reduction<'_, '_, 'input, A>) -> Result<(), ParseError>;

/// What one reduce action sees: the value stack, the frames its rule popped, and the parse
/// context for token text and the caller's builder.
///
/// The frames are the rule's `r2` state-stack entries in right-hand-side order, so a token rule
/// (`Ident -> IDENT`) reads its terminal through [`Reduction::token`]. Provenance (#118) will
/// add its occurrence stack here without touching the reduce signature.
pub struct Reduction<'reduction, 'context, 'input, A> {
    /// The value stack; values are popped in reverse right-hand-side order.
    values: &'reduction mut ValueStack,
    /// The popped frames in right-hand-side order.
    frames: &'reduction [Frame],
    /// The parse context, for token text and the builder.
    context: &'reduction mut ParseContext<'context, 'input, A>,
}

impl<'input, A> Reduction<'_, '_, 'input, A> {
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
        assert!(frame.token != NO_TOKEN, "the popped frame was entered by a goto, not a shift");
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

/// Runs the automaton from a grammar type's start state and returns its value.
///
/// `accept_rule` is the type's start rule `S'(T) -> T`; reducing it ends the parse, with the
/// single value on the value stack as the root. `reduce` is the generation's dispatcher for
/// every other rule. On failure the input cursor is restored and every partial value dropped,
/// and the error names the token the automaton could not act on, which is the token recursive
/// descent fails on; its expected set is the state's explicit terminals until #117.
pub fn run<'input, T, A>(
    context: &mut ParseContext<'_, 'input, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    reduce: ReduceFn<'input, A>,
) -> Result<ParsedNode<T>, ParseError> {
    let checkpoint = context.cursor();
    let mut values = ValueStack::new();
    match drive::<T, A>(context, tables, start_state, accept_rule, reduce, &mut values) {
        Ok(value) => {
            debug_assert_eq!(values.depth(), 0, "accept leaves exactly one value");
            context.finish_with_children(value, None, std::iter::empty())
        }
        Err(error) => {
            values.unwind();
            context.restore(checkpoint);
            Err(error)
        }
    }
}

/// The automaton loop proper; `run` wraps it with the transactional restore.
fn drive<'input, T, A>(
    context: &mut ParseContext<'_, 'input, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    reduce: ReduceFn<'input, A>,
    values: &mut ValueStack,
) -> Result<T, ParseError> {
    let decoded = tables.decoded();
    let end_marker = tables.end_marker();
    let mut states = vec![Frame {
        state: start_state,
        token: NO_TOKEN,
    }];
    loop {
        let state = states.last().expect("the state stack keeps the start state").state;
        let action = if decoded.acts_by_default(state) {
            decoded.default_action(state)
        } else {
            match context.peek(0) {
                None => decoded.action(state, end_marker),
                Some(record) if u32::from(record.kind) >= end_marker => TableAction::Error,
                Some(record) => {
                    let next = context.peek(1).map(|next| u32::from(next.kind));
                    decoded.action(state, decoded.merged_terminal(u32::from(record.kind), next))
                }
            }
        };
        match action {
            TableAction::Shift(target) => {
                let token = context.cursor() as u32;
                context.advance();
                states.push(Frame {
                    state: target,
                    token,
                });
            }
            TableAction::Reduce(rule) if rule == accept_rule => {
                // SAFETY: the accept rule `S'(T) -> T` pops the root value, which the reduce
                // action of a `T` rule pushed as a `T`.
                return Ok(unsafe { values.pop::<T>() });
            }
            TableAction::Reduce(rule) => {
                let pops = decoded.r2[rule as usize] as usize;
                let value_pops = decoded.value_pops[rule as usize] as usize;
                let depth = values.depth();
                debug_assert!(value_pops <= depth, "value_pops never exceeds the values on the stack");
                let kept = states.len() - pops;
                let mut reduction = Reduction {
                    values,
                    frames: &states[kept..],
                    context,
                };
                reduce(rule, &mut reduction)?;
                debug_assert!(
                    values.depth() == depth - value_pops || values.depth() == depth - value_pops + 1,
                    "a reduce action pops `value_pops` values and pushes at most one"
                );
                states.truncate(kept);
                let top = states.last().expect("the state stack keeps the start state").state;
                states.push(Frame {
                    state: decoded.goto(top, decoded.r1[rule as usize]),
                    token: NO_TOKEN,
                });
            }
            TableAction::Error => {
                let expected = expected_set(context, decoded, state, end_marker);
                return Err(context.unexpected(&expected));
            }
        }
    }
}

/// The crude expected set of an error state: the terminals with an explicit action.
fn expected_set<A>(
    context: &ParseContext<'_, '_, A>,
    decoded: &DecodedTables,
    state: u32,
    end_marker: u32,
) -> FollowSet {
    let kind_count = context.grammar().kind_count();
    let mut words = vec![0_u64; kind_count.div_ceil(64)];
    for terminal in decoded.explicit_terminals(state, end_marker) {
        let terminal = terminal as usize;
        if terminal < kind_count {
            words[terminal / 64] |= 1 << (terminal % 64);
        }
    }
    FollowSet::owned(kind_count as u32, words)
}

#[cfg(test)]
mod tests {
    use super::tables::tests::{FILTERED_TEXTBOOK, TEXTBOOK};
    use super::*;
    use crate::input::LexResult;
    use crate::{GrammarInfo, ParseErrorKind, Span};
    use std::cell::Cell;
    use std::rc::Rc;

    /// Kinds of the textbook grammar's terminals `+ * ( ) id`, plus the lexical-error kind.
    static GRAMMAR: GrammarInfo = GrammarInfo {
        name: "lr-tests",
        kind_names: &["+", "*", "(", ")", "id", "lexical error"],
        atom_names: &[],
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
    fn evaluate(rule: u32, reduction: &mut Reduction<'_, '_, '_, Rc<Cell<usize>>>) -> Result<(), ParseError> {
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
                2 | 4 | 5 => {}
                6 => {
                    let record = reduction.token();
                    let text = reduction.matched_text(record)?;
                    let number = text.as_str().parse().expect("id tokens are numbers");
                    let live = reduction.builder().clone();
                    reduction.push(Value::new(number, &live));
                }
                other => panic!("rule {other} is not reduced by the dispatcher"),
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
            run::<Value, _>(context, &TEXTBOOK, 0, 0, evaluate)
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
        assert_eq!(live, 0, "the root value was returned and dropped by the test");
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
    fn record_kinds(rule: u32, reduction: &mut Reduction<'_, '_, '_, Rc<Cell<usize>>>) -> Result<(), ParseError> {
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
            run::<Value, _>(context, &FILTERED_TEXTBOOK, 0, 0, record_kinds)
                .map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap(), 2);
        assert_eq!(input.cursor(), 1);
        // `(+` is not filtered (`+` is no trigger), so `(` is looked up as itself and the parse
        // fails on the `+` that cannot open an operand.
        let lexed = lex("(+");
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _>(context, &FILTERED_TEXTBOOK, 0, 0, record_kinds)
                .map(|node| node.into_parts().0.number)
        });
        let error = result.unwrap_err();
        assert_eq!(error.span(), Span::new(1, 2).unwrap());
        assert_eq!(error.found(), Some("+"));
        // Without a filter the same tables reject `()` at the `)`.
        let lexed = lex("()");
        let mut input = lexed.input_with_builder(&GRAMMAR, live);
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _>(context, &TEXTBOOK, 0, 0, record_kinds)
                .map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap_err().found(), Some(")"));
    }

    #[test]
    fn a_reduce_error_is_transactional_too() {
        // A record whose span leaves the source makes `matched_text` fail inside the reduce.
        let live = Rc::new(Cell::new(0));
        let lexed = LexResult::new(
            "2+3",
            vec![
                TokenRecord { kind: 4, span: Span::new(0, 1).unwrap(), diagnostic: None },
                TokenRecord { kind: 0, span: Span::new(1, 2).unwrap(), diagnostic: None },
                TokenRecord { kind: 4, span: Span::new(2, 9).unwrap(), diagnostic: None },
            ],
            None,
        );
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _>(context, &TEXTBOOK, 0, 0, evaluate).map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap_err().kind(), ParseErrorKind::InvalidTokenSpan);
        assert_eq!(input.cursor(), 0);
        assert_eq!(live.get(), 0);
    }
}
