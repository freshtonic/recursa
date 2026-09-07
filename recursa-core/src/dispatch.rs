//! Table-driven interpretation of frozen predictive dispatch graphs.
//!
//! Generated code used to carry one copy of the graph walker, the position matcher,
//! and the packed-cell decoder per dispatch table. The tables differ only in their
//! packed cells, so one runtime walker interprets every table. Predictive, Pratt,
//! and optional-viability graphs share the cell encoding and the edge traversal; only
//! their terminal rules differ, and those live on [`crate::parsed::ParseContext`].
//!
//! # Packed literals, decoded once
//!
//! The frozen graph reaches the runtime as fixed-width URL-safe base64 byte strings.
//! That encoding is chosen for the *generated source*: a byte-string literal costs one
//! token to the compiler where a typed `&[u32]` array costs one token per cell, and the
//! grammar emits hundreds of tables. Decoding it on every lookup, though, is pure
//! overhead. [`Decoded`] therefore decodes each table exactly once, on first use, into
//! `u32` arrays and a per-state token-kind index, and every later lookup reads those.
//! The packed literal stays the on-disk form; the decoded form is the runtime form.

use std::sync::OnceLock;

use crate::grammar::FollowSet;
use crate::input::TokenRecord;

/// Minimum edge count before a state earns a dense token-kind row.
///
/// Below it a decoded linear scan is a handful of array reads and one bitset test per
/// edge, which beats paying `kind_count` cells of memory for the state.
const DENSE_MIN_EDGES: usize = 8;

/// Cap on the dense index of one table, in cells.
///
/// Rows are granted to the widest states first, so the states that dominate lookup
/// time get O(1) selection and a pathological graph cannot allocate without bound.
const DENSE_BUDGET_CELLS: usize = 1 << 20;

/// Sentinel for "this state has no dense row".
const NO_ROW: u32 = u32::MAX;

/// One frozen dispatch graph in fixed-width URL-safe base64 cells.
///
/// `states` holds one row per graph state, `edges` two cells per edge (position, next
/// state), and `positions` four cells per position (expected-set index, predicate mode,
/// spelling start, spelling count). Every cell uses the same digit `width`.
///
/// `state_cells` and `edge_field` describe the state row shape: how many cells one row
/// holds, and at which cell the `(edge start, edge count)` pair begins. The three graph
/// kinds carry different terminal fields around that pair, and the runtime needs the
/// pair alone to index a state's edges.
#[derive(Debug)]
pub struct DispatchTable {
    /// Packed state rows.
    states: &'static [u8],
    /// Packed `(position, next)` edge pairs.
    edges: &'static [u8],
    /// Packed position rows.
    positions: &'static [u8],
    /// Canonical spellings referenced by position rows.
    spellings: &'static [&'static str],
    /// Digits per packed cell.
    width: usize,
    /// Cells in one state row.
    state_cells: usize,
    /// Cell offset of `(edge start, edge count)` within a state row.
    edge_field: usize,
    /// Grammar-wide expectation pool indexed by position and state rows.
    expected: &'static [FollowSet],
    /// Decoded cells and per-state kind index, built on first use.
    decoded: OnceLock<Decoded>,
}

/// Constructs one frozen dispatch table from generated packed literals.
#[allow(clippy::too_many_arguments)]
pub const fn dispatch_table(
    states: &'static [u8],
    edges: &'static [u8],
    positions: &'static [u8],
    spellings: &'static [&'static str],
    width: usize,
    state_cells: usize,
    edge_field: usize,
    expected: &'static [FollowSet],
) -> DispatchTable {
    DispatchTable {
        states,
        edges,
        positions,
        spellings,
        width,
        state_cells,
        edge_field,
        expected,
        decoded: OnceLock::new(),
    }
}

/// One dispatch table's packed cells, decoded, plus its per-state kind index.
#[derive(Debug)]
struct Decoded {
    /// Decoded state cells, `state_cells` per state.
    states: Box<[u32]>,
    /// Decoded edge cells, two per edge.
    edges: Box<[u32]>,
    /// Decoded position cells, four per position.
    positions: Box<[u32]>,
    /// Offset into `dense` of each state's kind row, or [`NO_ROW`].
    rows: Box<[u32]>,
    /// Concatenated dense kind rows.
    ///
    /// A cell is `0` when no edge of the state admits the kind. Otherwise it is
    /// `next state + 1` for a table whose positions are all unconditional, and
    /// `edge index + 1` — the earliest edge admitting the kind — for a table that
    /// carries spelling predicates.
    dense: Box<[u32]>,
    /// Grammar token-kind width the dense rows are cut to.
    kind_count: usize,
    /// Whether every position row of this table uses predicate mode 0.
    unconditional: bool,
}

impl DispatchTable {
    /// Returns the grammar-wide expectation pool this table indexes into.
    pub(crate) fn expected(&self) -> &'static [FollowSet] {
        self.expected
    }

    /// Decodes one packed cell.
    fn cell(&self, table: &[u8], slot: usize) -> u32 {
        table[slot * self.width..(slot + 1) * self.width]
            .iter()
            .copied()
            .fold(0u32, |value, byte| {
                let digit = match byte {
                    b'A'..=b'Z' => u32::from(byte - b'A'),
                    b'a'..=b'z' => u32::from(byte - b'a') + 26,
                    b'0'..=b'9' => u32::from(byte - b'0') + 52,
                    b'-' => 62,
                    b'_' => 63,
                    _ => unreachable!("generated packed dispatch table is invalid"),
                };
                (value << 6) | digit
            })
    }

    /// Decodes one whole packed table into cells.
    fn decode_all(&self, table: &'static [u8]) -> Box<[u32]> {
        (0..table.len() / self.width)
            .map(|slot| self.cell(table, slot))
            .collect()
    }

    /// Returns this table's decoded cells and kind index, decoding on first use.
    fn decoded(&self) -> &Decoded {
        self.decoded.get_or_init(|| self.decode())
    }

    /// Decodes every packed cell and builds the per-state kind index.
    fn decode(&self) -> Decoded {
        let states = self.decode_all(self.states);
        let edges = self.decode_all(self.edges);
        let positions = self.decode_all(self.positions);
        let unconditional = positions.chunks_exact(4).all(|position| position[1] == 0);
        let kind_count = self
            .expected
            .first()
            .map_or(0, |expected| expected.kind_count() as usize);
        let state_count = states.len() / self.state_cells;

        // Grant dense rows to the widest states first, under a fixed cell budget.
        let mut candidates = (0..state_count)
            .filter_map(|state| {
                let count = states[state * self.state_cells + self.edge_field + 1] as usize;
                (count >= DENSE_MIN_EDGES).then_some((count, state))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by(|left, right| right.cmp(left));

        let mut rows = vec![NO_ROW; state_count].into_boxed_slice();
        let mut granted = Vec::new();
        let mut used = 0usize;
        if kind_count != 0 {
            for (_, state) in candidates {
                if used + kind_count > DENSE_BUDGET_CELLS {
                    break;
                }
                rows[state] = u32::try_from(used).expect("dense dispatch index fits u32");
                used += kind_count;
                granted.push(state);
            }
        }

        let mut dense = vec![0u32; used].into_boxed_slice();
        for state in granted {
            let offset = rows[state] as usize;
            let row = &mut dense[offset..offset + kind_count];
            let start = states[state * self.state_cells + self.edge_field] as usize;
            let count = states[state * self.state_cells + self.edge_field + 1] as usize;
            // Fill from the last edge back so the earliest edge admitting a kind wins,
            // exactly as the frozen-order linear scan does.
            for edge in (start..start + count).rev() {
                let position = edges[edge * 2] as usize;
                let next = edges[edge * 2 + 1];
                let expected = &self.expected[positions[position * 4] as usize];
                let value = if unconditional {
                    next + 1
                } else {
                    edge as u32 + 1
                };
                for (word_index, word) in expected.kind_words().iter().copied().enumerate() {
                    let mut word = word;
                    while word != 0 {
                        let kind = word_index * 64 + word.trailing_zeros() as usize;
                        word &= word - 1;
                        if kind < kind_count {
                            row[kind] = value;
                        }
                    }
                }
            }
        }

        Decoded {
            states,
            edges,
            positions,
            rows,
            dense,
            kind_count,
            unconditional,
        }
    }

    /// Decodes one fixed-width state row.
    pub(crate) fn state<const CELLS: usize>(&self, state: usize) -> [usize; CELLS] {
        let decoded = self.decoded();
        debug_assert_eq!(CELLS, self.state_cells, "state row shape matches the table");
        let slot = state
            .checked_mul(CELLS)
            .filter(|slot| slot + CELLS <= decoded.states.len())
            .unwrap_or_else(|| unreachable!("frozen dispatch reached an unknown state"));
        std::array::from_fn(|cell| decoded.states[slot + cell] as usize)
    }

    /// Follows the first matching edge out of `state`, if any.
    ///
    /// The result is the first edge of the state, in frozen order, whose position
    /// admits the lookahead. States wide enough to earn a dense row answer that in one
    /// array read when the table has no spelling predicates, and start their predicate
    /// walk at the earliest admitting edge when it does.
    pub(crate) fn follow_edge(
        &self,
        state: usize,
        record: Option<TokenRecord>,
        source: &str,
    ) -> Option<usize> {
        let record = record?;
        let decoded = self.decoded();
        let base = state * self.state_cells + self.edge_field;
        let edge_start = decoded.states[base] as usize;
        let edge_count = decoded.states[base + 1] as usize;
        if edge_count == 0 {
            return None;
        }
        let edge_end = edge_start + edge_count;
        let kind = usize::from(record.kind);
        let row = decoded.rows[state];
        if row != NO_ROW {
            if kind >= decoded.kind_count {
                return None;
            }
            let cell = decoded.dense[row as usize + kind];
            if cell == 0 {
                return None;
            }
            if decoded.unconditional {
                return Some((cell - 1) as usize);
            }
            return self.scan(decoded, (cell - 1) as usize, edge_end, record, source);
        }
        self.scan(decoded, edge_start, edge_end, record, source)
    }

    /// Scans a half-open edge range in frozen order for the first matching position.
    fn scan(
        &self,
        decoded: &Decoded,
        from: usize,
        end: usize,
        record: TokenRecord,
        source: &str,
    ) -> Option<usize> {
        (from..end).find_map(|edge| {
            let position = decoded.edges[edge * 2] as usize;
            let next = decoded.edges[edge * 2 + 1] as usize;
            self.position_matches(decoded, record, source, position)
                .then_some(next)
        })
    }

    /// Tests one lookahead record against a decoded position row.
    fn position_matches(
        &self,
        decoded: &Decoded,
        record: TokenRecord,
        source: &str,
        position: usize,
    ) -> bool {
        let slot = position
            .checked_mul(4)
            .filter(|slot| slot + 4 <= decoded.positions.len())
            .unwrap_or_else(|| unreachable!("frozen dispatch reached an unknown position"));
        let expected_index = decoded.positions[slot] as usize;
        let predicate_mode = decoded.positions[slot + 1];
        let spelling_start = decoded.positions[slot + 2] as usize;
        let spelling_count = decoded.positions[slot + 3] as usize;
        let spelling_end = spelling_start
            .checked_add(spelling_count)
            .filter(|end| *end <= self.spellings.len())
            .unwrap_or_else(|| unreachable!("frozen dispatch reached an unknown position"));
        let expected = &self.expected[expected_index];
        if !expected.contains(record.kind) {
            return false;
        }
        if predicate_mode == 0 {
            return true;
        }
        let options = &self.spellings[spelling_start..spelling_end];
        match predicate_mode {
            1 | 2 => source
                .get(record.span.range())
                .is_some_and(|text| options.contains(&text)),
            3 => source
                .get(record.span.range())
                .is_some_and(|text| !options.contains(&text)),
            4 | 5 => source.get(record.span.range()).is_some_and(|text| {
                options
                    .iter()
                    .any(|option| text.eq_ignore_ascii_case(option))
            }),
            6 => source.get(record.span.range()).is_some_and(|text| {
                !options
                    .iter()
                    .any(|option| text.eq_ignore_ascii_case(option))
            }),
            7 => source.get(record.span.range()).is_some(),
            _ => unreachable!("frozen dispatch reached an unknown position"),
        }
    }
}

/// Decodes one packed predictive target cell into `(branch, terminal mode)`.
pub(crate) fn decode_target(encoded: usize) -> Option<(usize, usize)> {
    (encoded != 0).then(|| {
        let target = encoded - 1;
        (target / 3, target % 3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;
    use crate::grammar::ExpectedAtomId;

    /// Packs cells the way `recursa-codegen` does, so tests exercise the real encoding.
    fn pack(values: &[usize], width: usize) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut encoded = Vec::with_capacity(values.len() * width);
        for value in values {
            for digit in (0..width).rev() {
                encoded.push(ALPHABET[(value >> (digit * 6)) & 0x3f]);
            }
        }
        encoded
    }

    /// The pre-index walker, kept verbatim as the oracle the new lookup must match.
    fn oracle(
        table: &DispatchTable,
        edge_start: usize,
        edge_count: usize,
        record: Option<TokenRecord>,
        source: &str,
    ) -> Option<usize> {
        let raw_cell = |bytes: &[u8], slot: usize| -> usize {
            bytes[slot * table.width..(slot + 1) * table.width]
                .iter()
                .copied()
                .fold(0usize, |value, byte| {
                    let digit = match byte {
                        b'A'..=b'Z' => usize::from(byte - b'A'),
                        b'a'..=b'z' => usize::from(byte - b'a') + 26,
                        b'0'..=b'9' => usize::from(byte - b'0') + 52,
                        b'-' => 62,
                        b'_' => 63,
                        _ => unreachable!("invalid packed cell"),
                    };
                    (value << 6) | digit
                })
        };
        (edge_start..edge_start + edge_count).find_map(|edge| {
            let position = raw_cell(table.edges, edge * 2);
            let next = raw_cell(table.edges, edge * 2 + 1);
            let slot = position * 4;
            let expected_index = raw_cell(table.positions, slot);
            let predicate_mode = raw_cell(table.positions, slot + 1);
            let spelling_start = raw_cell(table.positions, slot + 2);
            let spelling_count = raw_cell(table.positions, slot + 3);
            let expected = &table.expected[expected_index];
            let options = &table.spellings[spelling_start..spelling_start + spelling_count];
            let matched = record.is_some_and(|record| {
                expected.contains(record.kind)
                    && match predicate_mode {
                        0 => true,
                        1 | 2 => source
                            .get(record.span.range())
                            .is_some_and(|text| options.contains(&text)),
                        3 => source
                            .get(record.span.range())
                            .is_some_and(|text| !options.contains(&text)),
                        4 | 5 => source.get(record.span.range()).is_some_and(|text| {
                            options
                                .iter()
                                .any(|option| text.eq_ignore_ascii_case(option))
                        }),
                        6 => source.get(record.span.range()).is_some_and(|text| {
                            !options
                                .iter()
                                .any(|option| text.eq_ignore_ascii_case(option))
                        }),
                        7 => source.get(record.span.range()).is_some(),
                        _ => unreachable!("invalid predicate mode"),
                    }
            });
            matched.then_some(next)
        })
    }

    const KIND_COUNT: u32 = 40;

    /// Builds a leaked expectation pool whose set `i` holds the kinds in `sets[i]`.
    fn expectation_pool(sets: &[&[u16]]) -> &'static [FollowSet] {
        let pool = sets
            .iter()
            .map(|kinds| {
                let mut words = vec![0u64; (KIND_COUNT as usize).div_ceil(64)];
                for kind in kinds.iter().copied() {
                    words[usize::from(kind) / 64] |= 1u64 << (usize::from(kind) % 64);
                }
                FollowSet::from_static(
                    KIND_COUNT,
                    Box::leak(words.into_boxed_slice()),
                    &[] as &[ExpectedAtomId],
                )
            })
            .collect::<Vec<_>>();
        Box::leak(pool.into_boxed_slice())
    }

    struct Fixture {
        table: DispatchTable,
        /// `(edge start, edge count)` of each state, mirroring the state rows.
        edge_ranges: Vec<(usize, usize)>,
    }

    /// Builds a two-state graph: a wide root that earns a dense row, plus a narrow
    /// state that stays on the scan path. `positions` are `(expected, mode, spellings)`.
    fn fixture(
        positions: &[(usize, usize, &'static [&'static str])],
        wide_edges: &[(usize, usize)],
        narrow_edges: &[(usize, usize)],
        expected_sets: &[&[u16]],
    ) -> Fixture {
        let mut spellings = Vec::new();
        let mut position_values = Vec::new();
        for (expected, mode, options) in positions {
            let start = spellings.len();
            spellings.extend_from_slice(options);
            position_values.extend([*expected, *mode, start, options.len()]);
        }
        let mut edge_values = Vec::new();
        for (position, next) in wide_edges.iter().chain(narrow_edges) {
            edge_values.extend([*position, *next]);
        }
        let edge_ranges = vec![
            (0, wide_edges.len()),
            (wide_edges.len(), narrow_edges.len()),
        ];
        // State rows use the predictive shape: target, edge start, edge count, expected.
        let mut state_values = Vec::new();
        for (start, count) in &edge_ranges {
            state_values.extend([0usize, *start, *count, 0usize]);
        }
        let width = 2;
        let table = dispatch_table(
            Box::leak(pack(&state_values, width).into_boxed_slice()),
            Box::leak(pack(&edge_values, width).into_boxed_slice()),
            Box::leak(pack(&position_values, width).into_boxed_slice()),
            Box::leak(spellings.into_boxed_slice()),
            width,
            4,
            1,
            expectation_pool(expected_sets),
        );
        Fixture { table, edge_ranges }
    }

    /// Asserts the indexed lookup matches the oracle for every kind and every spelling.
    fn assert_matches_oracle(
        fixture: &Fixture,
        source: &str,
        spans: &[(u16, std::ops::Range<usize>)],
    ) {
        for (state, (start, count)) in fixture.edge_ranges.iter().copied().enumerate() {
            assert_eq!(
                fixture.table.follow_edge(state, None, source),
                oracle(&fixture.table, start, count, None, source),
                "state {state} at end of input"
            );
            for kind in 0..=u16::try_from(KIND_COUNT + 4).unwrap() {
                for (_, range) in spans {
                    let record = TokenRecord {
                        kind,
                        span: Span::new(range.start, range.end).expect("test span"),
                        diagnostic: None,
                    };
                    assert_eq!(
                        fixture.table.follow_edge(state, Some(record), source),
                        oracle(&fixture.table, start, count, Some(record), source),
                        "state {state}, kind {kind}, span {range:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn indexed_lookup_matches_the_linear_walk_without_predicates() {
        // Ten overlapping edges on the root so it earns a dense row.
        let sets: &[&[u16]] = &[
            &[1, 2, 3],
            &[3, 4, 5],
            &[5, 6, 7],
            &[1, 7, 9],
            &[10, 11],
            &[2, 4, 6, 8],
            &[0],
            &[39],
            &[12, 13, 14],
            &[3, 39],
            &[],
        ];
        let positions = (0..sets.len())
            .map(|index| (index, 0usize, &[] as &'static [&'static str]))
            .collect::<Vec<_>>();
        let wide: Vec<(usize, usize)> = (0..sets.len()).map(|index| (index, index + 1)).collect();
        let narrow = vec![(0usize, 7usize), (3usize, 8usize)];
        let fixture = fixture(&positions, &wide, &narrow, sets);
        assert_matches_oracle(&fixture, "alpha", &[(0, 0..5)]);
    }

    #[test]
    fn indexed_lookup_matches_the_linear_walk_across_predicate_modes() {
        let sets: &[&[u16]] = &[&[1, 2, 3, 4, 5, 6, 7, 8, 9], &[3, 4], &[9, 10, 11]];
        let options: &'static [&'static str] = &["alpha", "BETA"];
        let positions: Vec<(usize, usize, &'static [&'static str])> =
            (0..8).map(|mode| (mode % 3, mode, options)).collect();
        // Nine edges on the root: every predicate mode plus a repeat, so the root is
        // dense and the dense cell must only start, never finish, the predicate walk.
        let wide: Vec<(usize, usize)> = (0..8)
            .map(|position| (position, position + 1))
            .chain([(0usize, 9usize)])
            .collect();
        let narrow = vec![(4usize, 3usize), (6usize, 4usize), (0usize, 5usize)];
        let fixture = fixture(&positions, &wide, &narrow, sets);
        assert_matches_oracle(
            &fixture,
            "alpha beta BETA gamma",
            &[(0, 0..5), (0, 6..10), (0, 11..15), (0, 16..21), (0, 3..3)],
        );
    }

    #[test]
    fn narrow_states_keep_frozen_order_when_edges_overlap() {
        let sets: &[&[u16]] = &[&[1, 2], &[2, 3]];
        let positions: Vec<(usize, usize, &'static [&'static str])> =
            vec![(0, 0, &[]), (1, 0, &[])];
        let fixture = fixture(&positions, &[(0, 5), (1, 6)], &[(1, 7), (0, 8)], sets);
        let record = |kind| TokenRecord {
            kind,
            span: Span::new(0, 1).expect("test span"),
            diagnostic: None,
        };
        // Kind 2 is admitted by both positions; the earlier edge wins in each state.
        assert_eq!(fixture.table.follow_edge(0, Some(record(2)), "x"), Some(5));
        assert_eq!(fixture.table.follow_edge(1, Some(record(2)), "x"), Some(7));
        assert_matches_oracle(&fixture, "x", &[(0, 0..1)]);
    }

    #[test]
    fn decoded_state_rows_match_the_packed_cells() {
        let sets: &[&[u16]] = &[&[1]];
        let positions: Vec<(usize, usize, &'static [&'static str])> = vec![(0, 0, &[])];
        let fixture = fixture(&positions, &[(0, 1)], &[(0, 0)], sets);
        assert_eq!(fixture.table.state::<4>(0), [0, 0, 1, 0]);
        assert_eq!(fixture.table.state::<4>(1), [0, 1, 1, 0]);
    }
}
