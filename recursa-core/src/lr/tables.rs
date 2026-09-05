//! The packed LALR(1) tables and their decoder.
//!
//! The tables mirror bison's `yypact`, `yytable`, `yycheck`, `yydefact`, `yypgoto`, `yydefgoto`,
//! `yyr1` and `yyr2`, plus Recursa's `value_pops` per rule, `expected_atoms` per terminal
//! (the symbolic expectation the error path names a terminal by) and its `lookahead` filter
//! table, the flat `(token, merged, trigger)` triples of the grammar's `lookahead`
//! declarations, which [`DecodedTables::merged_terminal`] applies to the token feed. Codegen
//! packs them
//! (`recursa_codegen::lr::tables`) and renders each as one byte-string literal so a table costs
//! the compiler one token; this module is the runtime half of that format and must agree with
//! the codegen encoder cell for cell.
//!
//! # Wire format
//!
//! A literal is one base64 digit giving the cell width, then `width` digits per cell, in the
//! URL-safe alphabet `DispatchTable` uses. Unsigned tables store the cell value. Signed tables
//! store `0` for the sentinel [`PACT_NINF`] and otherwise the zigzag form of the value plus one,
//! so small magnitudes keep small widths.
//!
//! # Lookup contract
//!
//! - action on terminal `t` in state `s`: if `pact[s] == PACT_NINF` take `defact[s]`; else let
//!   `i = pact[s] + t`; if `0 <= i < table.len()` and `check[i] == t` the entry is `table[i]`,
//!   otherwise take `defact[s]`.
//! - `table[i] > 0` shifts to state `table[i]`; `table[i] < 0` reduces by rule `-table[i] - 1`;
//!   `table[i] == TABLE_NINF` is an explicit error.
//! - `defact[s] == 0` is an error; otherwise reduce by rule `defact[s] - 1`.
//! - goto on nonterminal `n` from state `s`: let `i = pgoto[n] + s`; if in range and
//!   `check[i] == s` go to `table[i]`, otherwise `defgoto[n]`.

use std::sync::OnceLock;

/// The `pact` or `pgoto` value of a row that has no explicit entries.
pub const PACT_NINF: i32 = i32::MIN;

/// The `table` value of an explicit syntax error.
pub const TABLE_NINF: i32 = i32::MIN;

/// The URL-safe base64 alphabet shared with `DispatchTable` and the codegen encoder.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The packed tables of one generation's automaton, decoded once on first use.
#[derive(Debug)]
pub struct LrTables {
    /// Dense index of the end-marker terminal, which stands for end of input.
    end_marker: u32,
    /// Packed `pact`.
    pact: &'static [u8],
    /// Packed `table`.
    table: &'static [u8],
    /// Packed `check`.
    check: &'static [u8],
    /// Packed `defact`.
    defact: &'static [u8],
    /// Packed `pgoto`.
    pgoto: &'static [u8],
    /// Packed `defgoto`.
    defgoto: &'static [u8],
    /// Packed `r1`.
    r1: &'static [u8],
    /// Packed `r2`.
    r2: &'static [u8],
    /// Packed `value_pops`.
    value_pops: &'static [u8],
    /// Packed `expected_atoms`: per terminal, its canonical atom plus one, or `0` for none.
    expected_atoms: &'static [u8],
    /// Packed `lookahead`.
    lookahead: &'static [u8],
    /// The decoded form, filled on first lookup.
    decoded: OnceLock<DecodedTables>,
    /// The decoded `expected_atoms`, filled by the first error, never by a successful parse.
    atoms: OnceLock<Vec<u32>>,
}

/// The decoded tables, the form every lookup reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedTables {
    /// Base of each state's action row in `table`, or [`PACT_NINF`].
    pub pact: Vec<i32>,
    /// Packed action and goto entries.
    pub table: Vec<i32>,
    /// The key each entry of `table` belongs to: a terminal for action rows, a state for goto
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
    /// Lookahead filters as flat `(token, merged, trigger)` triples.
    pub lookahead: Vec<u32>,
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

impl LrTables {
    /// Binds the packed literals of one generation; the argument order is the literal order
    /// codegen renders.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        end_marker: u32,
        pact: &'static [u8],
        table: &'static [u8],
        check: &'static [u8],
        defact: &'static [u8],
        pgoto: &'static [u8],
        defgoto: &'static [u8],
        r1: &'static [u8],
        r2: &'static [u8],
        value_pops: &'static [u8],
        expected_atoms: &'static [u8],
        lookahead: &'static [u8],
    ) -> Self {
        Self {
            end_marker,
            pact,
            table,
            check,
            defact,
            pgoto,
            defgoto,
            r1,
            r2,
            value_pops,
            expected_atoms,
            lookahead,
            decoded: OnceLock::new(),
            atoms: OnceLock::new(),
        }
    }

    /// Returns the terminal that stands for end of input.
    pub fn end_marker(&self) -> u32 {
        self.end_marker
    }

    /// Returns the decoded tables, decoding the literals on first use.
    ///
    /// The literals are generated, so a malformed one is a generation bug and panics.
    pub fn decoded(&self) -> &DecodedTables {
        self.decoded.get_or_init(|| DecodedTables {
            pact: decode_signed(self.pact),
            table: decode_signed(self.table),
            check: decode_signed(self.check),
            defact: decode_unsigned(self.defact),
            pgoto: decode_signed(self.pgoto),
            defgoto: decode_unsigned(self.defgoto),
            r1: decode_unsigned(self.r1),
            r2: decode_unsigned(self.r2),
            value_pops: decode_unsigned(self.value_pops),
            lookahead: decode_unsigned(self.lookahead),
        })
    }

    /// Returns the canonical atom of each terminal, as the atom index plus one or `0` when the
    /// terminal has no atom; the end marker has no entry.
    ///
    /// This is the vocabulary [`ParseError::expected`](crate::ParseError::expected) renders,
    /// the same canonical atom per token kind that recursive descent's expected sets carry.
    /// Only the error path reads it, so it is decoded apart from the action tables.
    pub fn expected_atoms(&self) -> &[u32] {
        self.atoms.get_or_init(|| decode_unsigned(self.expected_atoms))
    }
}

impl DecodedTables {
    /// The terminal the token feed presents for a record of `kind` followed by a record of
    /// `next`: the merged terminal of the lookahead filter whose token is `kind` and whose
    /// triggers hold `next`, otherwise `kind` itself. Only the table lookup sees the merged
    /// terminal; the records themselves keep their lexed kinds.
    pub fn merged_terminal(&self, kind: u32, next: Option<u32>) -> u32 {
        let Some(next) = next else {
            return kind;
        };
        self.lookahead
            .chunks_exact(3)
            .find(|triple| triple[0] == kind && triple[2] == next)
            .map_or(kind, |triple| triple[1])
    }

    /// Reports whether a state acts without reading a lookahead.
    pub fn acts_by_default(&self, state: u32) -> bool {
        self.pact[state as usize] == PACT_NINF
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
        self.default_action(state)
    }

    /// Returns a state's default action: its default reduction, or an error when it has none.
    pub fn default_action(&self, state: u32) -> TableAction {
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

    /// Returns the terminals with an explicit shift or reduce in a state, in ascending order.
    ///
    /// This is the state's own row, not the expected set of an error raised in it: default
    /// reductions taken on the way to the state hide continuations, which
    /// [`lac`](crate::lr::lac) recovers from the pre-reduction stack.
    pub fn explicit_terminals(&self, state: u32, terminal_count: u32) -> Vec<u32> {
        let base = self.pact[state as usize];
        if base == PACT_NINF {
            return Vec::new();
        }
        (0..terminal_count)
            .filter(|terminal| {
                let slot = i64::from(base) + i64::from(*terminal);
                slot >= 0
                    && (slot as usize) < self.table.len()
                    && self.check[slot as usize] == *terminal as i32
                    && self.table[slot as usize] != TABLE_NINF
            })
            .collect()
    }
}

/// Reads one base64 digit.
fn digit(byte: u8) -> u32 {
    ALPHABET
        .iter()
        .position(|candidate| *candidate == byte)
        .map(|value| value as u32)
        .unwrap_or_else(|| panic!("byte {byte:#04x} is outside the LR table alphabet"))
}

/// Decodes an unsigned table literal: a width digit, then `width` digits per cell.
fn decode_unsigned(literal: &[u8]) -> Vec<u32> {
    let Some((width, cells)) = literal.split_first() else {
        panic!("an LR table literal carries at least its width digit");
    };
    let width = digit(*width) as usize;
    assert!(
        width > 0 && cells.len() % width == 0,
        "LR table literal is ragged: {} digits at width {width}",
        cells.len()
    );
    cells
        .chunks(width)
        .map(|cell| cell.iter().fold(0_u32, |value, byte| (value << 6) | digit(*byte)))
        .collect()
}

/// Decodes a signed table literal, inverting codegen's zigzag-plus-one mapping.
fn decode_signed(literal: &[u8]) -> Vec<i32> {
    decode_unsigned(literal)
        .into_iter()
        .map(|value| {
            if value == 0 {
                PACT_NINF
            } else {
                let zigzag = value - 1;
                ((zigzag >> 1) as i32) ^ -((zigzag & 1) as i32)
            }
        })
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// The dragon-book expression grammar `E -> E + T | T`, `T -> T * F | F`,
    /// `F -> ( E ) | id`, as codegen's `lr::textbook::expression_grammar` packs and encodes it.
    /// Terminals are `+ * ( ) id $end` (0..=5); rules are `0: $accept -> E`, `1: E -> E + T`,
    /// `2: E -> T`, `3: T -> T * F`, `4: T -> F`, `5: F -> ( E )`, `6: F -> id`; the start state
    /// of `E` is 0. The literals are copied from that encoder's output; the last one maps each
    /// terminal to its atom (`+ * ( ) id` are atoms 0 to 4), which the tests' `GrammarInfo` names.
    pub(in crate::lr) static TEXTBOOK: LrTables = LrTables::new(
        5,
        b"BFFADJABFFAJA",
        b"BPPVTDRFNBBBX",
        b"BBBPHFDJDCCCR",
        b"BAAHBDFAAAGCE",
        b"BNAKH",
        b"BDAEF",
        b"BBAACCDD",
        b"BBDBDBDB",
        b"BBCBCBBA",
        b"BBCDEF",
        b"B",
    );

    /// The textbook tables with a synthetic lookahead filter: `(` before `)` is read as `id`
    /// (terminals 2, 4 and 3), which is what `lr::tests` uses to prove the feed substitutes the
    /// lookup terminal only.
    pub(in crate::lr) static FILTERED_TEXTBOOK: LrTables = LrTables::new(
        5,
        b"BFFADJABFFAJA",
        b"BPPVTDRFNBBBX",
        b"BBBPHFDJDCCCR",
        b"BAAHBDFAAAGCE",
        b"BNAKH",
        b"BDAEF",
        b"BBAACCDD",
        b"BBDBDBDB",
        b"BBCBCBBA",
        b"BBCDEF",
        b"BCED",
    );

    /// The decoded form codegen's `LrTables::pack` produced for the textbook grammar.
    pub(in crate::lr) fn textbook_decoded() -> DecodedTables {
        DecodedTables {
            pact: vec![2, 2, PACT_NINF, 1, 4, PACT_NINF, 0, 2, 2, PACT_NINF, 4, PACT_NINF],
            table: vec![7, 7, 10, 9, 1, 8, 2, 6, 0, 0, 0, 11],
            check: vec![0, 0, 7, 3, 2, 1, 4, 1, -1, -1, -1, 8],
            defact: vec![0, 0, 7, 1, 3, 5, 0, 0, 0, 6, 2, 4],
            pgoto: vec![6, PACT_NINF, -5, 3],
            defgoto: vec![3, 0, 4, 5],
            r1: vec![1, 0, 0, 2, 2, 3, 3],
            r2: vec![1, 3, 1, 3, 1, 3, 1],
            value_pops: vec![1, 2, 1, 2, 1, 1, 0],
            lookahead: Vec::new(),
        }
    }

    #[test]
    fn the_lookahead_table_maps_a_token_before_a_trigger_to_its_merged_terminal() {
        let decoded = FILTERED_TEXTBOOK.decoded();
        assert_eq!(decoded.lookahead, [2, 4, 3]);
        assert_eq!(decoded.merged_terminal(2, Some(3)), 4, "`(` before `)` is `id`");
        assert_eq!(decoded.merged_terminal(2, Some(0)), 2, "`(` before `+` stays `(`");
        assert_eq!(decoded.merged_terminal(2, None), 2, "`(` at the end stays `(`");
        assert_eq!(decoded.merged_terminal(4, Some(3)), 4, "an unfiltered token stays");
        assert!(TEXTBOOK.decoded().lookahead.is_empty());
        assert_eq!(TEXTBOOK.decoded().merged_terminal(2, Some(3)), 2);
    }

    #[test]
    fn the_decoder_reproduces_the_tables_codegen_packed() {
        assert_eq!(*TEXTBOOK.decoded(), textbook_decoded());
        assert_eq!(TEXTBOOK.end_marker(), 5);
    }

    #[test]
    fn lookups_agree_with_the_codegen_simulation_of_the_textbook_grammar() {
        // `id + id * id` reduces `F -> id`, `T -> F`, `E -> T`, `F -> id`, `T -> F`, `F -> id`,
        // `T -> T * F`, `E -> E + T` and then accepts on `$end`, exactly as
        // `expression_grammar_tables_drive_a_prefix_parse` asserts in codegen.
        let tables = TEXTBOOK.decoded();
        let (plus, star, id, end) = (0, 1, 4, 5);
        let mut stack = vec![0_u32];
        let mut reductions = Vec::new();
        let input = [id, plus, id, star, id];
        let mut position = 0;
        loop {
            let state = *stack.last().unwrap();
            let lookahead = input.get(position).copied().unwrap_or(end);
            match tables.action(state, lookahead) {
                TableAction::Shift(target) => {
                    stack.push(target);
                    position += 1;
                }
                TableAction::Reduce(0) => break,
                TableAction::Reduce(rule) => {
                    let pops = tables.r2[rule as usize] as usize;
                    stack.truncate(stack.len() - pops);
                    reductions.push(rule);
                    let top = *stack.last().unwrap();
                    stack.push(tables.goto(top, tables.r1[rule as usize]));
                }
                TableAction::Error => panic!("unexpected error at token {position}"),
            }
        }
        assert_eq!(reductions, [6, 4, 2, 6, 4, 6, 3, 1]);
        assert_eq!(position, 5);
    }

    #[test]
    fn reduce_only_states_act_by_default_and_the_explicit_terminals_are_the_row() {
        let tables = TEXTBOOK.decoded();
        assert!(tables.acts_by_default(2));
        assert!(!tables.acts_by_default(0));
        // State 0 shifts `(` and `id` only.
        assert_eq!(tables.explicit_terminals(0, 6), [2, 4]);
        assert_eq!(tables.action(0, 0), TableAction::Error);
        assert!(tables.explicit_terminals(2, 6).is_empty());
    }

    #[test]
    #[should_panic(expected = "outside the LR table alphabet")]
    fn a_byte_outside_the_alphabet_is_a_generation_bug() {
        decode_unsigned(b"B!");
    }

    #[test]
    #[should_panic(expected = "ragged")]
    fn a_ragged_literal_is_a_generation_bug() {
        decode_unsigned(b"CABC");
    }
}
