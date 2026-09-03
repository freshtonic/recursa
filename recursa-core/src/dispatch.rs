//! Table-driven interpretation of frozen predictive dispatch graphs.
//!
//! Generated code used to carry one copy of the graph walker, the position matcher,
//! and the packed-cell decoder per dispatch table. The tables differ only in their
//! packed cells, so one runtime walker interprets every table. Predictive, Pratt,
//! and optional-viability graphs share the cell encoding and the edge traversal; only
//! their terminal rules differ, and those live on [`crate::parsed::ParseContext`].

use crate::grammar::FollowSet;
use crate::input::TokenRecord;

/// One frozen dispatch graph in fixed-width URL-safe base64 cells.
///
/// `states` holds one row per graph state, `edges` two cells per edge (position, next
/// state), and `positions` four cells per position (expected-set index, predicate mode,
/// spelling start, spelling count). Every cell uses the same digit `width`.
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
    /// Grammar-wide expectation pool indexed by position and state rows.
    expected: &'static [FollowSet],
}

/// Constructs one frozen dispatch table from generated packed literals.
pub const fn dispatch_table(
    states: &'static [u8],
    edges: &'static [u8],
    positions: &'static [u8],
    spellings: &'static [&'static str],
    width: usize,
    expected: &'static [FollowSet],
) -> DispatchTable {
    DispatchTable {
        states,
        edges,
        positions,
        spellings,
        width,
        expected,
    }
}

impl DispatchTable {
    /// Returns the grammar-wide expectation pool this table indexes into.
    pub(crate) fn expected(&self) -> &'static [FollowSet] {
        self.expected
    }

    /// Decodes one packed cell.
    fn cell(&self, table: &[u8], slot: usize) -> usize {
        table[slot * self.width..(slot + 1) * self.width]
            .iter()
            .copied()
            .fold(0usize, |value, byte| {
                let digit = match byte {
                    b'A'..=b'Z' => usize::from(byte - b'A'),
                    b'a'..=b'z' => usize::from(byte - b'a') + 26,
                    b'0'..=b'9' => usize::from(byte - b'0') + 52,
                    b'-' => 62,
                    b'_' => 63,
                    _ => unreachable!("generated packed dispatch table is invalid"),
                };
                (value << 6) | digit
            })
    }

    /// Decodes one fixed-width state row.
    pub(crate) fn state<const CELLS: usize>(&self, state: usize) -> [usize; CELLS] {
        let slot = state
            .checked_mul(CELLS)
            .filter(|slot| (slot + CELLS) * self.width <= self.states.len())
            .unwrap_or_else(|| unreachable!("frozen dispatch reached an unknown state"));
        std::array::from_fn(|cell| self.cell(self.states, slot + cell))
    }

    /// Follows the first matching edge out of a state, if any.
    pub(crate) fn follow_edge(
        &self,
        edge_start: usize,
        edge_count: usize,
        record: Option<TokenRecord>,
        source: &str,
    ) -> Option<usize> {
        (edge_start..edge_start + edge_count).find_map(|edge| {
            let position = self.cell(self.edges, edge * 2);
            let next = self.cell(self.edges, edge * 2 + 1);
            self.position_matches(record, source, position)
                .then_some(next)
        })
    }

    /// Tests one lookahead record against a frozen position row.
    fn position_matches(&self, record: Option<TokenRecord>, source: &str, position: usize) -> bool {
        let slot = position
            .checked_mul(4)
            .filter(|slot| (slot + 4) * self.width <= self.positions.len())
            .unwrap_or_else(|| unreachable!("frozen dispatch reached an unknown position"));
        let expected_index = self.cell(self.positions, slot);
        let predicate_mode = self.cell(self.positions, slot + 1);
        let spelling_start = self.cell(self.positions, slot + 2);
        let spelling_count = self.cell(self.positions, slot + 3);
        let spelling_end = spelling_start
            .checked_add(spelling_count)
            .filter(|end| *end <= self.spellings.len())
            .unwrap_or_else(|| unreachable!("frozen dispatch reached an unknown position"));
        let expected = &self.expected[expected_index];
        let options = &self.spellings[spelling_start..spelling_end];
        record.is_some_and(|record| {
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
                    _ => unreachable!("frozen dispatch reached an unknown position"),
                }
        })
    }
}

/// Decodes one packed predictive target cell into `(branch, terminal mode)`.
pub(crate) fn decode_target(encoded: usize) -> Option<(usize, usize)> {
    (encoded != 0).then(|| {
        let target = encoded - 1;
        (target / 3, target % 3)
    })
}
