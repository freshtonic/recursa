//! The table-driven parser's runtime: the LALR(1) automaton loop over generated tables.
//!
//! Generated code supplies data and reduce actions only (`docs/table-driven-parsing.md`,
//! "Scope and grammar contract"): the packed [`crate::lr::LrTables`] of the whole grammar, one start state and accept rule per
//! grammar type, and one reduce function that builds a value for each rule. [`crate::lr::run`] is the loop
//! that joins them. It keeps two stacks: the state stack, whose frames remember the shifted token
//! so that a content-token rule can read it, and the [`crate::lr::ValueStack`], which holds only semantic
//! values, so fixed tokens cost the value stack nothing.
//!
//! [`crate::lr::run_flat`] is the same loop for flat construction: it hands every action a Flat
//! AST builder beside the reduction and returns the root handle the actions built. A parse that
//! tracks extents ([`crate::parsed::ParseContext::tracks_extents`], which only flat construction
//! asks for) also keeps a third stack of one source extent per state-stack frame, which gives
//! each flat Node its span. Nested parsing never allocates it.
//!
//! The token feed applies the grammar's lookahead filters (`docs/table-driven-parsing.md`,
//! "Lookahead filters") at the one place the loop reads a terminal: a record whose kind is a
//! filtered token, followed by a record whose kind is one of its triggers, is looked up as the
//! merged terminal. That substitution is absolute, as bison's `base_yylex` replacement is:
//! the merged terminal is the only terminal looked up, and a state with no action for it
//! reports a syntax error on that record rather than retrying the source terminal. Nothing
//! else changes: the record is shifted as lexed, so a reduce action and the error path see the
//! lexed kind and span, and the trigger record is read next as itself.
//!
//! Three behaviours are decisions of the design document and are pinned by this module's tests:
//! default reductions are taken before any lookahead is read, so a state that can only reduce
//! never asks for a token; a root parse accepts greedily, reducing its accept rule as soon as the
//! lookahead has no shift action while a shiftable lookahead always shifts; and a failed parse
//! restores the input cursor and drops every partially built value through the value stack's drop
//! stack.

use crate::input::TokenRecord;
use crate::parsed::{ContentText, Occurrence, ParseContext, ParsedNode};
use crate::rejection::Rejection;
use crate::{ParseError, Span};

pub mod lac;
pub mod provenance;
mod tables;
mod value_stack;

use provenance::OccurrenceStack;
pub use tables::{DecodedTables, LrTables, PACT_NINF, TABLE_NINF, TableAction};
pub use value_stack::ValueStack;

/// The token index of a state-stack frame entered by a goto rather than a shift.
const NO_TOKEN: u32 = u32::MAX;

/// Initial state-stack capacity, avoiding geometric growth for ordinary generated parses.
const INITIAL_STATE_CAPACITY: usize = 32;

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
pub struct Reduction<'reduction, 'context, 'input, 'arena, A> {
    /// The value stack; values are popped in reverse right-hand-side order.
    values: &'reduction mut ValueStack,
    /// The popped frames in right-hand-side order.
    frames: &'reduction [Frame],
    /// The extent of each popped frame, in the same order, when the parse keeps extents.
    ///
    /// Flat construction reads it to give a Node the extent of its own rule, and a grammar that
    /// declares a rejection rule reads it to position the rejection at one list item. A parse
    /// that asks for neither leaves the field `None` and runs no extent work at all.
    extents: Option<&'reduction [Span]>,
    /// The parse context, for token text and the builder.
    context: &'reduction mut ParseContext<'context, 'input, 'arena, A>,
    /// Whether this parse keeps an occurrence stack.
    capturing: bool,
}

impl<'input, 'arena, A> Reduction<'_, '_, 'input, 'arena, A> {
    /// Reports whether the parse folds provenance: capture was requested, the root's policy is
    /// mandatory, or the grammar has a mandatory site. When false no occurrence work runs.
    pub fn capturing(&self) -> bool {
        self.capturing
    }

    /// Returns the source extent of the whole rule, or `None` without extent tracking.
    ///
    /// The extent runs from the first source token the rule's symbols cover to the last. A rule
    /// that covers no token, such as an empty list alternative, has no extent. Flat reduce
    /// actions store it as the constructed Node's span.
    pub fn extent(&self) -> Option<Span> {
        merge_extents(self.extents?)
    }

    /// Returns the source extent of one right-hand-side symbol, counted from zero.
    ///
    /// A flat reduce action uses it for a child it allocates itself, such as a fixed-token unit
    /// Node that carries no value on the value stack. The result is `None` without extent
    /// tracking, out of range, or when the symbol covers no token.
    pub fn symbol_extent(&self, index: usize) -> Option<Span> {
        self.symbol_range_extent(index, 1)
    }

    /// Returns the source extent of `len` right-hand-side symbols from `start`.
    ///
    /// One grammar site may occupy several symbols, because the lowering inlines an attachment's
    /// own fixed tokens into its parent's rule. A flat reduce action gives an allocated child the
    /// extent of its whole site.
    pub fn symbol_range_extent(&self, start: usize, len: usize) -> Option<Span> {
        let extents = self.extents?;
        merge_extents(extents.get(start..start.checked_add(len)?)?)
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

    /// Moves the next value directly from the value stack into an arena box.
    ///
    /// This is the generated-code path for an arena-boxed field. It avoids materializing a large
    /// semantic value between [`ValueStack::pop`] and its final arena allocation.
    ///
    /// # Safety
    ///
    /// `V` must be the type the value was pushed with; see [`ValueStack::pop`].
    pub unsafe fn pop_boxed<V>(&mut self) -> bumpalo::boxed::Box<'arena, V> {
        let destination = self
            .context
            .arena()
            .alloc_layout(std::alloc::Layout::new::<V>())
            .cast::<V>()
            .as_ptr();
        // SAFETY: the arena allocation is aligned, writable, non-overlapping storage for one
        // `V`; the pushed-type invariant is forwarded to the caller. `from_raw` binds ownership
        // of the initialized allocation to the returned arena box.
        unsafe {
            self.values.pop_into(destination);
            bumpalo::boxed::Box::from_raw(destination)
        }
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

    /// Converts a shifted token into text whose borrow is shortened to the AST arena.
    pub fn matched_text_in_arena(
        &self,
        record: TokenRecord,
    ) -> Result<ContentText<'arena>, ParseError>
    where
        'input: 'arena,
    {
        self.context.matched_text_in_arena(record)
    }

    /// Returns the bump arena used by generated semantic containers.
    pub fn arena(&self) -> &'arena bumpalo::Bump {
        self.context.arena()
    }

    /// Returns the caller's construction state.
    pub fn builder(&mut self) -> &mut A {
        self.context.builder()
    }

    /// Turns one declared rejection into the parse failure that reports it.
    ///
    /// The generated list action of a repetition whose element type declares rejection rules
    /// calls this with whatever [`crate::rejection::first_rejection`] found. Both the nested and
    /// the flat action reach the same error through it, so all three storages report one
    /// rejection identically.
    pub fn reject(&self, rejection: Rejection) -> ParseError {
        self.context.reject(rejection.note(), rejection.span())
    }
}

/// The reduce dispatcher the automaton loop calls for one rule.
///
/// The loop needs two shapes of dispatcher: nested parsing's, which owns everything it builds on
/// the value stack, and flat construction's, which also appends to a Flat AST builder the parse
/// owns. A trait carries both without a wrapping closure, because a closure over a generic
/// dispatcher forces the input and arena lifetimes to unify on the workspace's minimum supported
/// Rust version.
trait Reducer<'input, 'arena, A> {
    /// Runs the generated action of `rule`.
    fn reduce(
        &mut self,
        rule: u32,
        reduction: &mut Reduction<'_, '_, 'input, 'arena, A>,
    ) -> Result<(), ParseError>;
}

impl<'input, 'arena, A, F> Reducer<'input, 'arena, A> for F
where
    F: FnMut(u32, &mut Reduction<'_, '_, 'input, 'arena, A>) -> Result<(), ParseError>,
{
    fn reduce(
        &mut self,
        rule: u32,
        reduction: &mut Reduction<'_, '_, 'input, 'arena, A>,
    ) -> Result<(), ParseError> {
        self(rule, reduction)
    }
}

/// One flat construction dispatcher: the generated action and the builder it appends to.
struct FlatReducer<'builder, B, F> {
    /// The Flat AST under construction, borrowed for the whole parse.
    builder: &'builder mut B,
    /// The generated dispatcher, which takes the builder as a third argument.
    reduce: F,
}

impl<'input, 'arena, A, B, F> Reducer<'input, 'arena, A> for FlatReducer<'_, B, F>
where
    F: FnMut(u32, &mut Reduction<'_, '_, 'input, 'arena, A>, &mut B) -> Result<(), ParseError>,
{
    fn reduce(
        &mut self,
        rule: u32,
        reduction: &mut Reduction<'_, '_, 'input, 'arena, A>,
    ) -> Result<(), ParseError> {
        (self.reduce)(rule, reduction, self.builder)
    }
}

/// Merges the extents of a run of frames into the extent that covers all of them.
///
/// [`Span::SYNTHETIC`] marks a frame that covers no source token: the start state, a goto frame
/// of an empty rule, and any frame whose own rule covered nothing. Such frames are skipped, so a
/// run of them merges to `None`. The result is `None` as well when the merged offsets cannot
/// form a checked [`Span`], which keeps the overflow behaviour of [`Span::new`] rather than
/// inventing a range.
fn merge_extents(extents: &[Span]) -> Option<Span> {
    let concrete = || extents.iter().copied().filter(|span| !span.is_synthetic());
    let start = concrete().next()?.start();
    let end = concrete().next_back()?.end();
    Span::new(start as usize, end as usize).ok()
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
pub fn run<'input, 'arena, T, A, F>(
    context: &mut ParseContext<'_, 'input, 'arena, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    reduce: F,
) -> Result<ParsedNode<'arena, T>, ParseError>
where
    F: FnMut(u32, &mut Reduction<'_, '_, 'input, 'arena, A>) -> Result<(), ParseError>,
{
    let (value, occurrence) =
        run_transactional::<T, A, _>(context, tables, start_state, accept_rule, reduce)?;
    Ok(ParsedNode::from_parts(value, occurrence))
}

/// Runs the automaton for one flat construction and returns the root handle its reduce actions
/// built.
///
/// This is `run` for a Flat AST: `builder` is the construction state every flat reduce action
/// appends to, and it reaches the action as a third argument instead of through the value stack.
/// The loop, the tables, the diagnostics and the transactional restore are the ones nested
/// parsing uses, so both representations accept the same inputs and consume the same tokens. A
/// flat parse retains no occurrence provenance (`docs/adr/0008-keep-flat-asts-semantic-only.md`),
/// which the context guarantees, and the returned occurrence is therefore dropped.
pub fn run_flat<'input, 'arena, T, A, B, F>(
    context: &mut ParseContext<'_, 'input, 'arena, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    builder: &mut B,
    reduce: F,
) -> Result<T, ParseError>
where
    F: FnMut(u32, &mut Reduction<'_, '_, 'input, 'arena, A>, &mut B) -> Result<(), ParseError>,
{
    let (value, _) = run_transactional::<T, A, _>(
        context,
        tables,
        start_state,
        accept_rule,
        FlatReducer { builder, reduce },
    )?;
    Ok(value)
}

/// Drives the automaton under the transactional guarantee both entry points promise: on failure
/// the input cursor is restored and every partial value is dropped.
fn run_transactional<'input, 'arena, T, A, R>(
    context: &mut ParseContext<'_, 'input, 'arena, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    mut reduce: R,
) -> Result<(T, Occurrence<'arena>), ParseError>
where
    R: Reducer<'input, 'arena, A>,
{
    let checkpoint = context.cursor();
    let mut values = ValueStack::new();
    let mut occurrences = context
        .captures_provenance(tables.mandatory_provenance())
        .then(|| OccurrenceStack::new(context.provenance_arena()));
    match drive::<T, A, _>(
        context,
        tables,
        start_state,
        accept_rule,
        &mut reduce,
        &mut values,
        &mut occurrences,
    ) {
        Ok(parsed) => {
            debug_assert_eq!(values.depth(), 0, "accept leaves exactly one value");
            Ok(parsed)
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
    context: &ParseContext<'_, '_, '_, A>,
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

/// The automaton loop proper; `run_transactional` wraps it with the transactional restore.
///
/// A parse that keeps extents — a flat parse, and a parse of a grammar that declares a rejection
/// rule — keeps one more stack beside the state stack: the source extent of each frame. A shift
/// pushes the shifted token's span and a reduce replaces its popped frames' extents with the
/// extent that covers them, so the goto frame carries the extent of the whole rule. A reduce
/// action reads it through [`Reduction::extent`]. Every other parse never allocates that stack.
///
/// `tail` tracks whether the current reduction spine requires a following token to belong to the
/// root FOLLOW. After the last shift, reductions up to acceptance are exactly the spine of last
/// symbols, so one flag set by the rules' `tail` table decides whether an outside token is an
/// error or the parsed prefix may be accepted. A one-token enum alternative and a rule whose
/// final site is a repetition set it; a left-recursive continuation of its own enum Node — a
/// rule whose first symbol is its own left-hand side, which is what an operator variant of an
/// expression Node lowers to — clears it, because such a reduce completes an operand of an
/// enclosing expression and what follows belongs to whatever encloses it.
fn drive<'input, 'arena, T, A, R>(
    context: &mut ParseContext<'_, 'input, 'arena, A>,
    tables: &LrTables,
    start_state: u32,
    accept_rule: u32,
    reduce: &mut R,
    values: &mut ValueStack,
    occurrences: &mut Option<OccurrenceStack<'arena>>,
) -> Result<(T, Occurrence<'arena>), ParseError>
where
    R: Reducer<'input, 'arena, A>,
{
    let decoded = tables.decoded();
    let end_marker = tables.end_marker();
    // The one thing the error path needs from the hot path: where this parse began, so that
    // lookahead correction can replay the state stack up to the failing token.
    let origin = context.cursor();
    let mut states = Vec::with_capacity(INITIAL_STATE_CAPACITY);
    states.push(Frame {
        state: start_state,
        token: NO_TOKEN,
    });
    // One extent per state-stack frame, or nothing at all when the parse does not track them.
    // The start frame covers no token, so it starts synthetic.
    let mut extents = context.keeps_extents().then(|| {
        let mut extents = Vec::with_capacity(INITIAL_STATE_CAPACITY);
        extents.push(Span::SYNTHETIC);
        extents
    });
    // Keep bison's `yystate` shape: the current state lives in a local as well as at the top of
    // the stack, so the action loop does not reload the frame it just pushed.
    let mut state = start_state;
    let mut tail = false;
    loop {
        let action = if decoded.acts_by_default(state) {
            decoded.default_action(state)
        } else {
            match context.peek(0) {
                None => decoded.action(state, end_marker),
                Some(record) if u32::from(record.kind) >= end_marker => TableAction::Error,
                Some(record) => {
                    let kind = u32::from(record.kind);
                    if decoded.requires_lookahead(kind) {
                        let next = context.peek(1).map(|next| u32::from(next.kind));
                        decoded.filtered_action(state, kind, next)
                    } else {
                        decoded.action(state, kind)
                    }
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
                if let Some(stack) = extents.as_mut() {
                    let record = shifted.expect("a shift consumes the lookahead it read");
                    stack.push(record.span);
                }
                states.push(Frame {
                    state: target,
                    token,
                });
                state = target;
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
                    Some(stack) => stack.into_root(context),
                    None => Occurrence::absent(),
                };
                return Ok((value, occurrence));
            }
            TableAction::Reduce(rule) => {
                let rule_info = &decoded.rules[rule as usize];
                let pops = rule_info.pops as usize;
                let value_pops = rule_info.value_pops as usize;
                let depth = values.depth();
                debug_assert!(
                    value_pops <= depth,
                    "value_pops never exceeds the values on the stack"
                );
                let kept = states.len() - pops;
                let mut reduction = Reduction {
                    values,
                    frames: &states[kept..],
                    extents: extents.as_ref().map(|stack| &stack[kept..]),
                    context,
                    capturing: occurrences.is_some(),
                };
                if rule_info.has_reduce_action {
                    reduce.reduce(rule, &mut reduction)?;
                }
                if let Some(stack) = extents.as_mut() {
                    let merged = merge_extents(&stack[kept..]).unwrap_or(Span::SYNTHETIC);
                    stack.truncate(kept);
                    stack.push(merged);
                }
                debug_assert!(
                    values.depth() == depth - value_pops
                        || values.depth() == depth - value_pops + 1,
                    "a reduce action pops `value_pops` values and pushes at most one"
                );
                if let Some(stack) = occurrences.as_mut() {
                    stack.reduce(context, tables.folds(), rule, pops);
                }
                match rule_info.tail {
                    1 => tail = true,
                    2 => tail = false,
                    _ => {}
                }
                states.truncate(kept);
                let top = states
                    .last()
                    .expect("the state stack keeps the start state")
                    .state;
                state = decoded.goto(top, rule_info.lhs);
                states.push(Frame {
                    state,
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
    use super::tables::tests::{ABSENT_MERGED_TEXTBOOK, FILTERED_TEXTBOOK, TEXTBOOK};
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
        reduction: &mut Reduction<'_, '_, '_, '_, Rc<Cell<usize>>>,
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
        reduction: &mut Reduction<'_, '_, '_, '_, Rc<Cell<usize>>>,
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
    fn a_filtered_token_is_rejected_where_the_merged_terminal_has_no_action() {
        // The filter maps `)` before `+` to `id`, and no state that shifts `)` shifts the
        // merged `id`. The lookahead is absolute, so `(2)+3` is a syntax error on the `)`
        // instead of falling back to the source terminal and closing the group.
        let live = Rc::new(Cell::new(0));
        let lexed = lex("(2)+3");
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &ABSENT_MERGED_TEXTBOOK, 0, 0, evaluate)
                .map(|node| node.into_parts().0.number)
        });
        let error = result.unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::UnexpectedToken);
        assert_eq!(error.span(), Span::new(2, 3).unwrap());
        assert_eq!(error.found(), Some(")"), "the error names the lexed token");
        // The expected set is what could continue at that position with `+` still following:
        // the group's own `)` is excluded, because the feed would merge it again.
        assert_eq!(error.expected().collect::<Vec<_>>(), ["fixed:+", "fixed:*"]);
        drop(error);
        assert_eq!(input.cursor(), 0, "a failed parse restores the cursor");
        assert_eq!(live.get(), 0, "a failed parse drops every partial value");

        // `)` before a token that triggers no filter stays `)`, so the same tables parse
        // `(2)*3`.
        let lexed = lex("(2)*3");
        let mut input = lexed.input_with_builder(&GRAMMAR, live.clone());
        let result = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &ABSENT_MERGED_TEXTBOOK, 0, 0, evaluate)
                .map(|node| node.into_parts().0.number)
        });
        assert_eq!(result.unwrap(), 6);
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
            reduction: &mut Reduction<'_, '_, '_, '_, Rc<Cell<usize>>>,
        ) -> Result<(), ParseError> {
            assert!(!reduction.capturing(), "capture is off");
            evaluate(rule, reduction)
        }
        let live = Rc::new(Cell::new(0));
        let lexed = lex("(2+3)*4");
        let mut input = lexed.input_with_builder(&GRAMMAR, live);
        let captured = crate::parsed::test_context(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, assert_off)
                .map(|node| node.into_parts().1.is_captured())
        })
        .unwrap();
        assert!(!captured);
    }

    #[test]
    #[cfg(feature = "spans")]
    fn a_capturing_parse_folds_the_generated_occurrence_tree() {
        // With capture requested every reduce sees the stack and the textbook folds build the
        // tree: each rule a node over the rule's extent whose children are its sites, a
        // nonterminal site grouped around the child's node, a terminal site a leaf, and
        // `F -> id` passing its token through to the group that consumes it.
        fn assert_on(
            rule: u32,
            reduction: &mut Reduction<'_, '_, '_, '_, Rc<Cell<usize>>>,
        ) -> Result<(), ParseError> {
            assert!(reduction.capturing(), "capture is on");
            evaluate(rule, reduction)
        }
        let live = Rc::new(Cell::new(0));
        let lexed = lex("2+3*4");
        let mut input = lexed.input_with_builder(&GRAMMAR, live);
        let occurrence = crate::parsed::test_context_spanned(&mut input, |context| {
            run::<Value, _, _>(context, &TEXTBOOK, 0, 0, assert_on)
                .map(|node| node.into_parts().1.render())
        })
        .unwrap();
        assert_eq!(
            occurrence,
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

    /// Records every reduced rule with its whole-rule extent and its first symbol's extent, and
    /// keeps a value-free stack so the log is the only output.
    fn record_extents(
        rule: u32,
        reduction: &mut Reduction<'_, '_, '_, '_, ExtentLog>,
    ) -> Result<(), ParseError> {
        let show = |extent: Option<Span>| match extent {
            Some(span) => format!("{}..{}", span.start(), span.end()),
            None => "-".to_owned(),
        };
        let line = format!(
            "{rule}:{} first={}",
            show(reduction.extent()),
            show(reduction.symbol_extent(0)),
        );
        reduction.builder().borrow_mut().push(line);
        // SAFETY: every rule of the textbook grammar pushes and pops the same unit value.
        unsafe {
            match rule {
                1 | 3 => {
                    reduction.pop::<()>();
                    reduction.pop::<()>();
                    reduction.push(());
                }
                6 => reduction.push(()),
                _ => {}
            }
        }
        Ok(())
    }

    /// The reduce log one extent-tracking parse writes.
    type ExtentLog = Rc<std::cell::RefCell<Vec<String>>>;

    /// Runs the textbook automaton over `source` with the extent stack on, and returns the log.
    #[cfg(feature = "spans")]
    fn extents_of(source: &str) -> Vec<String> {
        let log: ExtentLog = Rc::new(std::cell::RefCell::new(Vec::new()));
        let lexed = lex(source);
        let mut input = lexed.input_with_builder(&GRAMMAR, log.clone());
        crate::parsed::test_context_flat(&mut input, |context| {
            assert!(context.tracks_extents(), "a flat request tracks extents");
            assert!(
                !context.captures_provenance(true),
                "a flat parse keeps no occurrence stack even with a mandatory site"
            );
            run_flat::<(), _, (), _>(context, &TEXTBOOK, 0, 0, &mut (), |rule, reduction, ()| {
                record_extents(rule, reduction)
            })
        })
        .expect("the textbook input parses");
        log.borrow().clone()
    }

    #[test]
    #[cfg(feature = "spans")]
    fn an_extent_tracking_parse_gives_every_rule_the_source_range_of_its_symbols() {
        // `(2+3)*4`: a token rule takes its own token and a parent covers its first child's
        // start to its last child's end. The textbook tables mark `E -> T`, `T -> F` and
        // `F -> ( E )` as rules without a reduce action, so they never reach the log; rule 3's
        // first symbol proves the extent still travelled through them, because the `T` of
        // `T -> T * F` covers the whole parenthesized group.
        assert_eq!(
            extents_of("(2+3)*4"),
            [
                "6:1..2 first=1..2",
                "6:3..4 first=3..4",
                "1:1..4 first=1..2",
                "6:6..7 first=6..7",
                "3:0..7 first=0..5",
            ],
        );
    }

    #[test]
    fn a_parse_without_extent_tracking_reports_no_extent_at_all() {
        // The ordinary nested requests never allocate the extent stack, so every reduce sees
        // `None` and pays nothing.
        let log: ExtentLog = Rc::new(std::cell::RefCell::new(Vec::new()));
        let lexed = lex("2+3");
        let mut input = lexed.input_with_builder(&GRAMMAR, log.clone());
        crate::parsed::test_context(&mut input, |context| {
            assert!(!context.tracks_extents());
            run_flat::<(), _, (), _>(context, &TEXTBOOK, 0, 0, &mut (), |rule, reduction, ()| {
                record_extents(rule, reduction)
            })
        })
        .expect("the textbook input parses");
        assert_eq!(
            log.borrow().as_slice(),
            ["6:- first=-", "6:- first=-", "1:- first=-"],
        );
    }
}
