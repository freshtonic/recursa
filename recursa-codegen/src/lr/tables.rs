#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Comb-vector table packing in bison's shape, and its deterministic encoding.
//!
//! The in-memory [`LrTables`] mirror bison's `yypact`, `yytable`, `yycheck`, `yydefact`,
//! `yypgoto`, `yydefgoto`, `yyr1` and `yyr2`, plus Recursa's `value_pops` per rule and
//! `start_states` per grammar type. Action rows and goto columns share one `table`/`check` pair.
//! The encoding is the fixed-width URL-safe base64 that `DispatchTable` uses, so each table
//! renders as one byte-string literal (`EncodedTable::literal`); `recursa_core::lr` carries the
//! runtime decoder, the decoder here proves the round trip.
//!
//! Lookup contract, which the runtime must follow exactly:
//! - action on terminal `t` in state `s`: if `pact[s] == PACT_NINF` take `defact[s]`; else let
//!   `i = pact[s] + t`; if `0 <= i < table.len()` and `check[i] == t` the entry is `table[i]`,
//!   otherwise take `defact[s]`.
//! - `table[i] > 0` shifts to state `table[i]`; `table[i] < 0` reduces by rule `-table[i] - 1`;
//!   `table[i] == TABLE_NINF` is an explicit error.
//! - `defact[s] == 0` is an error; otherwise reduce by rule `defact[s] - 1`. Reducing a start rule
//!   accepts the parse.
//! - goto on nonterminal `n` from state `s`: let `i = pgoto[n] + s`; if in range and
//!   `check[i] == s` go to `table[i]`, otherwise `defgoto[n]`.

use std::collections::HashMap;

use super::automaton::{Action, Automaton, State, StateId};
use super::grammar::{LrGrammar, NonterminalId, RuleId, Symbol, TerminalId};

/// The `pact` value of a state whose only action is its default reduction.
pub const PACT_NINF: i32 = i32::MIN;

/// The `table` value of an explicit syntax error.
pub const TABLE_NINF: i32 = i32::MIN;

/// The `start_states` value of a grammar type without a start rule.
pub const NO_START_STATE: u32 = u32::MAX;

#[derive(Clone, Debug, Eq, PartialEq)]
/// The packed parser tables of one automaton.
pub struct LrTables {
    /// Base of each state's action row in `table`, or [`PACT_NINF`].
    pub pact: Vec<i32>,
    /// Packed action and goto entries.
    pub table: Vec<i32>,
    /// The key an entry of `table` belongs to: a terminal for action rows, a state for goto
    /// columns, `-1` for unused slots.
    pub check: Vec<i32>,
    /// Default reduction of each state as `rule + 1`, or `0` for none.
    pub defact: Vec<u32>,
    /// Base of each nonterminal's goto column in `table`, or [`PACT_NINF`].
    pub pgoto: Vec<i32>,
    /// Default goto target of each nonterminal.
    pub defgoto: Vec<u32>,
    /// Left-hand-side nonterminal of each rule.
    pub r1: Vec<u32>,
    /// Right-hand-side length of each rule.
    pub r2: Vec<u32>,
    /// Value-carrying right-hand-side symbols of each rule.
    pub value_pops: Vec<u32>,
    /// Start state of each grammar type indexed by [`TypeId`](crate::TypeId), or
    /// [`NO_START_STATE`].
    pub start_states: Vec<u32>,
    /// The lookahead filters as flat `(token, merged, trigger)` triples, one per trigger, in
    /// filter then trigger order; the token feed reads `token` before `trigger` as `merged`.
    pub lookahead: Vec<u32>,
    /// The provenance fold of each rule, packed as [`super::grammar::RuleFold::pack`] packs it, in rule order.
    pub folds: Vec<u32>,
    /// The tail decision of each rule ([`super::grammar::TailDecision::code`]).
    pub tail: Vec<u32>,
}

/// One sparse vector to pack: `(key, value)` pairs in ascending key order.
type SparseVector = Vec<(i32, i32)>;

impl LrTables {
    /// Packs an automaton's actions and gotos.
    pub fn pack(grammar: &LrGrammar, automaton: &Automaton) -> Self {
        let states = automaton.states();
        let mut vectors: Vec<SparseVector> = Vec::new();
        let mut defact = Vec::with_capacity(states.len());
        for state in states {
            vectors.push(action_row(state));
            defact.push(
                state
                    .default_reduction()
                    .map_or(0, |rule| rule.index() as u32 + 1),
            );
        }
        let mut defgoto = Vec::with_capacity(grammar.nonterminal_count());
        for index in 0..grammar.nonterminal_count() {
            let nonterminal = NonterminalId::new(index as u32);
            let (column, default) = goto_column(states, nonterminal);
            vectors.push(column);
            defgoto.push(default);
        }

        let (bases, table, check) = pack_vectors(&vectors);
        let (pact, pgoto) = bases.split_at(states.len());

        let mut start_states = Vec::new();
        for (type_id, state) in automaton.start_states() {
            if start_states.len() <= type_id.index() {
                start_states.resize(type_id.index() + 1, NO_START_STATE);
            }
            start_states[type_id.index()] = state.index() as u32;
        }

        Self {
            pact: pact.to_vec(),
            table,
            check,
            defact,
            pgoto: pgoto.to_vec(),
            defgoto,
            r1: grammar
                .rules()
                .iter()
                .map(|rule| rule.lhs.index() as u32)
                .collect(),
            r2: grammar
                .rules()
                .iter()
                .map(|rule| rule.rhs.len() as u32)
                .collect(),
            value_pops: grammar.rules().iter().map(|rule| rule.value_pops).collect(),
            start_states,
            lookahead: grammar
                .filters()
                .iter()
                .flat_map(|filter| {
                    filter.triggers.iter().flat_map(move |trigger| {
                        [
                            filter.token.index() as u32,
                            filter.merged.index() as u32,
                            trigger.index() as u32,
                        ]
                    })
                })
                .collect(),
            folds: {
                let mut folds = Vec::new();
                for rule in grammar.rules() {
                    rule.fold.pack(&mut folds);
                }
                folds
            },
            tail: grammar
                .rules()
                .iter()
                .map(|rule| rule.tail.code())
                .collect(),
        }
    }

    /// Encodes every table as fixed-width base64 text.
    pub fn encode(&self) -> EncodedLrTables {
        EncodedLrTables {
            pact: encode_signed(&self.pact),
            table: encode_signed(&self.table),
            check: encode_signed(&self.check),
            defact: encode_unsigned(&self.defact),
            pgoto: encode_signed(&self.pgoto),
            defgoto: encode_unsigned(&self.defgoto),
            r1: encode_unsigned(&self.r1),
            r2: encode_unsigned(&self.r2),
            value_pops: encode_unsigned(&self.value_pops),
            start_states: encode_unsigned(&self.start_states),
            lookahead: encode_unsigned(&self.lookahead),
            folds: encode_unsigned(&self.folds),
            tail: encode_unsigned(&self.tail),
        }
    }

    /// Decodes tables encoded by [`LrTables::encode`].
    pub fn decode(encoded: &EncodedLrTables) -> Result<Self, DecodeError> {
        Ok(Self {
            pact: decode_signed(&encoded.pact)?,
            table: decode_signed(&encoded.table)?,
            check: decode_signed(&encoded.check)?,
            defact: decode_unsigned(&encoded.defact)?,
            pgoto: decode_signed(&encoded.pgoto)?,
            defgoto: decode_unsigned(&encoded.defgoto)?,
            r1: decode_unsigned(&encoded.r1)?,
            r2: decode_unsigned(&encoded.r2)?,
            value_pops: decode_unsigned(&encoded.value_pops)?,
            start_states: decode_unsigned(&encoded.start_states)?,
            lookahead: decode_unsigned(&encoded.lookahead)?,
            folds: decode_unsigned(&encoded.folds)?,
            tail: decode_unsigned(&encoded.tail)?,
        })
    }

    /// Looks the action on a terminal up in a state, following the module's lookup contract.
    pub fn action(&self, state: u32, terminal: u32) -> TableAction {
        let base = self.pact[state as usize];
        if base != PACT_NINF {
            let slot = i64::from(base) + i64::from(terminal);
            if slot >= 0 && (slot as usize) < self.table.len() {
                let slot = slot as usize;
                if self.check[slot] == terminal as i32 {
                    let entry = self.table[slot];
                    return if entry == TABLE_NINF {
                        TableAction::Error
                    } else if entry > 0 {
                        TableAction::Shift(entry as u32)
                    } else {
                        TableAction::Reduce((-entry - 1) as u32)
                    };
                }
            }
        }
        match self.defact[state as usize] {
            0 => TableAction::Error,
            rule => TableAction::Reduce(rule - 1),
        }
    }

    /// Looks the goto on a nonterminal up from a state.
    pub fn goto(&self, state: u32, nonterminal: u32) -> u32 {
        let base = self.pgoto[nonterminal as usize];
        if base != PACT_NINF {
            let slot = i64::from(base) + i64::from(state);
            if slot >= 0 && (slot as usize) < self.table.len() {
                let slot = slot as usize;
                if self.check[slot] == state as i32 {
                    return self.table[slot] as u32;
                }
            }
        }
        self.defgoto[nonterminal as usize]
    }

    /// Total number of table cells, the size figure reports print.
    pub fn cell_count(&self) -> usize {
        self.pact.len()
            + self.table.len()
            + self.check.len()
            + self.defact.len()
            + self.pgoto.len()
            + self.defgoto.len()
            + self.r1.len()
            + self.r2.len()
            + self.value_pops.len()
            + self.start_states.len()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One decoded table action.
pub enum TableAction {
    /// Shift and go to the state.
    Shift(u32),
    /// Reduce by the rule.
    Reduce(u32),
    /// Syntax error.
    Error,
}

/// The explicit action row of a state: every action that is not the default reduction.
fn action_row(state: &State) -> SparseVector {
    state
        .actions()
        .iter()
        .filter_map(|(terminal, action)| {
            let value = match action {
                Action::Shift(target) => target.index() as i32,
                Action::Reduce(rule) => {
                    if state.default_reduction() == Some(*rule) {
                        return None;
                    }
                    -(rule.index() as i32) - 1
                }
                Action::Error => TABLE_NINF,
            };
            Some((terminal.index() as i32, value))
        })
        .collect()
}

/// The goto column of a nonterminal without its most frequent target, and that target.
fn goto_column(states: &[State], nonterminal: NonterminalId) -> (SparseVector, u32) {
    let mut entries: Vec<(StateId, StateId)> = Vec::new();
    for state in states {
        if let Some(target) = state.transition(Symbol::Nonterminal(nonterminal)) {
            entries.push((state.id(), target));
        }
    }
    let mut counts: HashMap<StateId, usize> = HashMap::new();
    for (_, target) in &entries {
        *counts.entry(*target).or_default() += 1;
    }
    let default = entries
        .iter()
        .map(|(_, target)| *target)
        .max_by_key(|target| (counts[target], std::cmp::Reverse(*target)))
        .map_or(0, |target| target.index() as u32);
    let column = entries
        .into_iter()
        .filter(|(_, target)| target.index() as u32 != default)
        .map(|(state, target)| (state.index() as i32, target.index() as i32))
        .collect();
    (column, default)
}

/// Packs sparse vectors into one comb: widest first, identical vectors share a base, distinct
/// vectors get distinct bases, each at the lowest base whose slots are all free.
fn pack_vectors(vectors: &[SparseVector]) -> (Vec<i32>, Vec<i32>, Vec<i32>) {
    let mut order: Vec<usize> = (0..vectors.len()).collect();
    order.sort_by_key(|index| {
        let vector = &vectors[*index];
        let width = vector.last().map_or(0, |(last, _)| last - vector[0].0 + 1);
        (
            std::cmp::Reverse(vector.len()),
            std::cmp::Reverse(width),
            *index,
        )
    });

    let mut bases = vec![PACT_NINF; vectors.len()];
    let mut table: Vec<i32> = Vec::new();
    let mut check: Vec<i32> = Vec::new();
    let mut occupied: Vec<bool> = Vec::new();
    let mut used_bases: HashMap<i32, ()> = HashMap::new();
    let mut identical: HashMap<&SparseVector, i32> = HashMap::new();

    for index in order {
        let vector = &vectors[index];
        if vector.is_empty() {
            continue;
        }
        if let Some(base) = identical.get(vector) {
            bases[index] = *base;
            continue;
        }
        let min_key = vector[0].0;
        let mut base = -min_key;
        loop {
            let fits = !used_bases.contains_key(&base)
                && vector.iter().all(|(key, _)| {
                    let slot = (base + key) as usize;
                    slot >= occupied.len() || !occupied[slot]
                });
            if fits {
                break;
            }
            base += 1;
        }
        for (key, value) in vector {
            let slot = (base + key) as usize;
            if slot >= occupied.len() {
                occupied.resize(slot + 1, false);
                table.resize(slot + 1, 0);
                check.resize(slot + 1, -1);
            }
            occupied[slot] = true;
            table[slot] = *value;
            check[slot] = *key;
        }
        used_bases.insert(base, ());
        identical.insert(vector, base);
        bases[index] = base;
    }
    (bases, table, check)
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One table as fixed-width base64 text.
pub struct EncodedTable {
    /// Base64 digits per cell.
    pub width: usize,
    /// The digits, `width` per cell, no padding.
    pub text: String,
}

impl EncodedTable {
    /// Encodes one unsigned table that is not part of [`LrTables`], such as the per-terminal
    /// expected atoms the renderer adds beside the action tables.
    pub fn unsigned(values: &[u32]) -> Self {
        encode_unsigned(values)
    }

    /// Renders the table as the byte-string literal the runtime decoder reads: one base64 digit
    /// for the width, then the cells.
    pub fn literal(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.text.len() + 1);
        bytes.push(ALPHABET[self.width]);
        bytes.extend_from_slice(self.text.as_bytes());
        bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Every table of [`LrTables`] encoded; the field names are the literal names ticket #113 renders.
pub struct EncodedLrTables {
    /// Encoded `pact`.
    pub pact: EncodedTable,
    /// Encoded `table`.
    pub table: EncodedTable,
    /// Encoded `check`.
    pub check: EncodedTable,
    /// Encoded `defact`.
    pub defact: EncodedTable,
    /// Encoded `pgoto`.
    pub pgoto: EncodedTable,
    /// Encoded `defgoto`.
    pub defgoto: EncodedTable,
    /// Encoded `r1`.
    pub r1: EncodedTable,
    /// Encoded `r2`.
    pub r2: EncodedTable,
    /// Encoded `value_pops`.
    pub value_pops: EncodedTable,
    /// Encoded `start_states`.
    pub start_states: EncodedTable,
    /// Encoded `lookahead`.
    pub lookahead: EncodedTable,
    /// Encoded `folds`.
    pub folds: EncodedTable,
    /// Encoded `tail`.
    pub tail: EncodedTable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Why an encoded table failed to decode.
pub enum DecodeError {
    /// A byte outside the base64 alphabet.
    InvalidDigit(u8),
    /// The text length is not a multiple of the width.
    RaggedText,
}

/// The URL-safe base64 alphabet `DispatchTable` uses.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encodes unsigned cells at the smallest width that holds the largest value.
fn encode_unsigned(values: &[u32]) -> EncodedTable {
    let max = values.iter().copied().max().unwrap_or(0);
    let bits = 32 - max.leading_zeros() as usize;
    let width = bits.div_ceil(6).max(1);
    let mut text = String::with_capacity(values.len() * width);
    for value in values {
        for digit in (0..width).rev() {
            text.push(ALPHABET[((value >> (digit * 6)) & 0x3f) as usize] as char);
        }
    }
    EncodedTable { width, text }
}

/// Encodes signed cells: the sentinel [`PACT_NINF`] becomes `0`, every other value its zigzag
/// form plus one, so small magnitudes keep small widths.
fn encode_signed(values: &[i32]) -> EncodedTable {
    let mapped: Vec<u32> = values
        .iter()
        .map(|value| {
            if *value == PACT_NINF {
                0
            } else {
                ((*value << 1) ^ (*value >> 31)) as u32 + 1
            }
        })
        .collect();
    encode_unsigned(&mapped)
}

/// Decodes unsigned cells.
fn decode_unsigned(encoded: &EncodedTable) -> Result<Vec<u32>, DecodeError> {
    let bytes = encoded.text.as_bytes();
    if encoded.width == 0 || bytes.len() % encoded.width != 0 {
        return Err(DecodeError::RaggedText);
    }
    bytes
        .chunks(encoded.width)
        .map(|cell| {
            cell.iter().try_fold(0u32, |value, byte| {
                let digit = ALPHABET
                    .iter()
                    .position(|candidate| candidate == byte)
                    .ok_or(DecodeError::InvalidDigit(*byte))?;
                Ok((value << 6) | digit as u32)
            })
        })
        .collect()
}

/// Decodes signed cells, inverting [`encode_signed`].
fn decode_signed(encoded: &EncodedTable) -> Result<Vec<i32>, DecodeError> {
    Ok(decode_unsigned(encoded)?
        .into_iter()
        .map(|value| {
            if value == 0 {
                PACT_NINF
            } else {
                let zigzag = value - 1;
                ((zigzag >> 1) as i32) ^ -((zigzag & 1) as i32)
            }
        })
        .collect())
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// What a simulated parse did.
pub struct SimulationTrace {
    /// Rules reduced, rendered, in order.
    pub reductions: Vec<String>,
    /// Tokens consumed before acceptance.
    pub consumed: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Why a simulated parse failed.
pub struct SimulationError {
    /// Index of the token with no action.
    pub position: usize,
    /// State in which the error occurred.
    pub state: u32,
}

/// Runs the tables over a token sequence from a start state with prefix acceptance, checking the
/// `value_pops` bookkeeping along the way. This is the reference interpretation of the tables;
/// ticket #113's runtime loop must agree with it.
pub fn simulate(
    grammar: &LrGrammar,
    tables: &LrTables,
    start_state: u32,
    input: &[TerminalId],
) -> Result<SimulationTrace, SimulationError> {
    let end = grammar.end_marker().index() as u32;
    let mut stack: Vec<u32> = vec![start_state];
    let mut value_depth: usize = 0;
    let mut position = 0;
    let mut reductions = Vec::new();
    loop {
        let state = *stack.last().expect("state stack is never empty");
        let lookahead = input
            .get(position)
            .map_or(end, |terminal| terminal.index() as u32);
        match tables.action(state, lookahead) {
            TableAction::Shift(target) => {
                stack.push(target);
                position += 1;
            }
            TableAction::Reduce(rule) => {
                let id = RuleId::new(rule);
                let pops = tables.r2[rule as usize] as usize;
                stack.truncate(stack.len() - pops);
                value_depth = value_depth
                    .checked_sub(tables.value_pops[rule as usize] as usize)
                    .expect("value_pops never exceeds the values on the stack");
                if grammar.is_start_rule(id) {
                    assert_eq!(value_depth, 0, "accept leaves exactly one value");
                    return Ok(SimulationTrace {
                        reductions,
                        consumed: position,
                    });
                }
                reductions.push(grammar.render_rule(id));
                if grammar.nonterminal(grammar.rule(id).lhs).carries_value {
                    value_depth += 1;
                }
                let top = *stack.last().expect("state stack keeps the start state");
                stack.push(tables.goto(top, tables.r1[rule as usize]));
            }
            TableAction::Error => {
                return Err(SimulationError { position, state });
            }
        }
    }
}
