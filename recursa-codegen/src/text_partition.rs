//! Canonical text cells shared by symbolic FIRST and frozen dispatch construction.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

use std::collections::{BTreeMap, BTreeSet};

use crate::{GrammarDescription, KindSet, TextEquivalence, TextPredicate, TextPredicateId};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One canonical spelling partition within a token kind.
pub(crate) enum TextPartition {
    /// Every spelling in the token kind.
    Any,
    /// Exactly one canonical spelling.
    Exact(String),
    /// One of the canonical spellings.
    OneOf(Vec<String>),
    /// Every spelling except the canonical exclusions.
    NotIn(Vec<String>),
}

impl TextPartition {
    /// Converts the canonical internal cell into the public predicate shape.
    pub(crate) fn into_predicate(self) -> Option<TextPredicate> {
        match self {
            Self::Any => None,
            Self::Exact(value) => Some(TextPredicate::Exact(value)),
            Self::OneOf(values) => Some(TextPredicate::OneOf(values)),
            Self::NotIn(values) => Some(TextPredicate::NotIn(values)),
        }
    }

    /// Copies an owned public predicate into the canonical internal value shape.
    pub(crate) fn from_predicate(predicate: Option<&TextPredicate>) -> Self {
        match predicate {
            None | Some(TextPredicate::Any) => Self::Any,
            Some(TextPredicate::Exact(value)) => Self::Exact(value.clone()),
            Some(TextPredicate::OneOf(values)) => Self::OneOf(values.clone()),
            Some(TextPredicate::NotIn(values)) => Self::NotIn(values.clone()),
        }
    }

    /// Reports whether two canonical cells accept a common spelling.
    pub(crate) fn overlaps(&self, other: &Self, equivalence: TextEquivalence) -> bool {
        let equivalent = |left: &str, right: &str| equivalence.equivalent(left, right);
        match (self, other) {
            (Self::Any, _) | (_, Self::Any) => true,
            (Self::Exact(left), Self::Exact(right)) => equivalent(left, right),
            (Self::Exact(exact), Self::OneOf(options))
            | (Self::OneOf(options), Self::Exact(exact)) => {
                options.iter().any(|option| equivalent(exact, option))
            }
            (Self::Exact(exact), Self::NotIn(excluded))
            | (Self::NotIn(excluded), Self::Exact(exact)) => {
                !excluded.iter().any(|value| equivalent(exact, value))
            }
            (Self::OneOf(left), Self::OneOf(right)) => left
                .iter()
                .any(|left| right.iter().any(|right| equivalent(left, right))),
            (Self::OneOf(options), Self::NotIn(excluded))
            | (Self::NotIn(excluded), Self::OneOf(options)) => options
                .iter()
                .any(|option| !excluded.iter().any(|value| equivalent(option, value))),
            (Self::NotIn(_), Self::NotIn(_)) => true,
        }
    }

    /// Adds every canonical spelling that can distinguish this cell from its
    /// complement to one shared partition boundary set.
    fn add_boundaries(&self, boundaries: &mut BTreeSet<String>) {
        match self {
            Self::Any => {}
            Self::Exact(value) => {
                boundaries.insert(value.clone());
            }
            Self::OneOf(values) | Self::NotIn(values) => {
                boundaries.extend(values.iter().cloned());
            }
        }
    }

    /// Reports whether this canonical cell accepts one explicit spelling, or
    /// the representative class outside every explicit boundary when absent.
    fn accepts_boundary(&self, boundary: Option<&str>) -> bool {
        match (self, boundary) {
            (Self::Any, _) => true,
            (Self::Exact(value), Some(boundary)) => value == boundary,
            (Self::OneOf(values), Some(boundary)) => values.iter().any(|value| value == boundary),
            (Self::NotIn(values), Some(boundary)) => values.iter().all(|value| value != boundary),
            (Self::NotIn(_), None) => true,
            (Self::Exact(_) | Self::OneOf(_), None) => false,
        }
    }
}

/// Partitions candidate predicates by the exact canonical residual each spelling reaches.
pub(crate) fn partition_by_residual<C, R, P, N>(
    candidates: &[C],
    grammar: &GrammarDescription,
    predicate_of: P,
    residual_of: N,
) -> Vec<(TextPartition, Vec<R>)>
where
    R: Clone + Ord,
    P: Fn(&C) -> Option<TextPredicateId>,
    N: Fn(&C) -> R,
{
    let partitioned =
        partition_residual_memberships(candidates, grammar, predicate_of, residual_of);
    partitioned
        .cells
        .into_iter()
        .map(|cell| {
            (
                cell.text,
                materialize_membership(&cell.membership, &partitioned.residuals),
            )
        })
        .collect()
}

/// Partitions token kinds and text together without expanding wide kind sets
/// into one candidate per concrete kind.
pub(crate) fn partition_by_kind_residual<'a, C, R, P, K, N>(
    candidates: &'a [C],
    grammar: &GrammarDescription,
    predicate_of: P,
    kinds_of: K,
    residual_of: N,
) -> Vec<(TextPartition, Vec<R>, KindSet)>
where
    R: Clone + Ord,
    P: Fn(&C) -> Option<TextPredicateId>,
    K: Fn(&'a C) -> &'a KindSet,
    N: Fn(&C) -> R,
{
    let mut factored = BTreeMap::<(Option<TextPredicateId>, R), KindSet>::new();
    for candidate in candidates {
        let kinds = kinds_of(candidate);
        factored
            .entry((predicate_of(candidate), residual_of(candidate)))
            .and_modify(|known| known.union_assign(kinds))
            .or_insert_with(|| kinds.clone());
    }
    let factored = factored
        .into_iter()
        .map(|((predicate, residual), kinds)| (predicate, residual, kinds))
        .collect::<Vec<_>>();

    let mut cells = BTreeMap::<(TextPartition, Vec<R>), KindSet>::new();
    for (membership, kinds) in exact_kind_regions(&factored, |candidate| &candidate.2) {
        for (text, residuals) in partition_by_residual(
            &membership,
            grammar,
            |candidate| factored[*candidate].0,
            |candidate| factored[*candidate].1.clone(),
        ) {
            cells
                .entry((text, residuals))
                .and_modify(|known| known.union_assign(&kinds))
                .or_insert_with(|| kinds.clone());
        }
    }
    cells
        .into_iter()
        .map(|((text, residuals), kinds)| (text, residuals, kinds))
        .collect()
}

/// Overlays already-canonical text cells and wide token-kind sets while
/// retaining the exact residual union selected by every spelling class.
pub(crate) fn overlay_by_kind_residual<'a, C, R, T, K, N>(
    candidates: &'a [C],
    text_of: T,
    kinds_of: K,
    residual_of: N,
) -> Vec<(TextPartition, Vec<R>, KindSet)>
where
    R: Clone + Ord,
    T: Fn(&'a C) -> &'a TextPartition,
    K: Fn(&'a C) -> &'a KindSet,
    N: Fn(&C) -> R,
{
    let mut factored = BTreeMap::<(TextPartition, R), KindSet>::new();
    for candidate in candidates {
        let kinds = kinds_of(candidate);
        factored
            .entry((text_of(candidate).clone(), residual_of(candidate)))
            .and_modify(|known| known.union_assign(kinds))
            .or_insert_with(|| kinds.clone());
    }
    let factored = factored
        .into_iter()
        .map(|((text, residual), kinds)| (text, residual, kinds))
        .collect::<Vec<_>>();

    let mut cells = BTreeMap::<(TextPartition, Vec<R>), KindSet>::new();
    for (membership, kinds) in exact_kind_regions(&factored, |candidate| &candidate.2) {
        let mut boundaries = BTreeSet::new();
        for candidate in &membership {
            factored[*candidate].0.add_boundaries(&mut boundaries);
        }
        let boundaries = boundaries.into_iter().collect::<Vec<_>>();
        let residuals = membership
            .iter()
            .map(|candidate| factored[*candidate].1.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let residual_index = residuals
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, residual)| (residual, index))
            .collect::<BTreeMap<_, _>>();
        let word_count = residuals.len().div_ceil(u64::BITS as usize);
        let membership_for = |boundary: Option<&str>| {
            let mut selected = vec![0_u64; word_count];
            for candidate in &membership {
                if factored[*candidate].0.accepts_boundary(boundary) {
                    membership_insert(&mut selected, residual_index[&factored[*candidate].1]);
                }
            }
            selected
        };
        let other = membership_for(None);
        let mut explicit = BTreeMap::<Vec<u64>, Vec<String>>::new();
        for boundary in boundaries {
            explicit
                .entry(membership_for(Some(&boundary)))
                .or_default()
                .push(boundary);
        }
        if membership_is_nonempty(&other) {
            let excluded = explicit
                .iter()
                .filter(|(selected, _)| **selected != other)
                .flat_map(|(_, values)| values.iter().cloned())
                .collect::<Vec<_>>();
            cells
                .entry((
                    if excluded.is_empty() {
                        TextPartition::Any
                    } else {
                        TextPartition::NotIn(excluded)
                    },
                    materialize_membership(&other, &residuals),
                ))
                .and_modify(|known| known.union_assign(&kinds))
                .or_insert_with(|| kinds.clone());
        }
        for (selected, values) in explicit {
            if !membership_is_nonempty(&selected) || selected == other {
                continue;
            }
            cells
                .entry((
                    if values.len() == 1 {
                        TextPartition::Exact(values.into_iter().next().expect("one spelling"))
                    } else {
                        TextPartition::OneOf(values)
                    },
                    materialize_membership(&selected, &residuals),
                ))
                .and_modify(|known| known.union_assign(&kinds))
                .or_insert_with(|| kinds.clone());
        }
    }
    cells
        .into_iter()
        .map(|((text, residuals), kinds)| (text, residuals, kinds))
        .collect()
}

/// Refines symbolic kind sets only at authored boundaries and returns one
/// canonical candidate-membership signature per disjoint region.
fn exact_kind_regions<'a, C, K>(candidates: &'a [C], kinds_of: K) -> BTreeMap<Vec<usize>, KindSet>
where
    K: Fn(&'a C) -> &'a KindSet,
{
    let mut regions = Vec::<(KindSet, Vec<usize>)>::new();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        let candidate_kinds = kinds_of(candidate);
        let mut uncovered = candidate_kinds.clone();
        let mut refined = Vec::new();
        for (region_kinds, region_membership) in regions {
            let overlap = region_kinds
                .intersection(candidate_kinds)
                .expect("kind regions share one width");
            let remainder = region_kinds
                .difference(candidate_kinds)
                .expect("kind regions share one width");
            uncovered = uncovered
                .difference(&region_kinds)
                .expect("kind regions share one width");
            if !remainder.is_empty() {
                refined.push((remainder, region_membership.clone()));
            }
            if !overlap.is_empty() {
                let mut membership = region_membership;
                membership.push(candidate_index);
                refined.push((overlap, membership));
            }
        }
        if !uncovered.is_empty() {
            refined.push((uncovered, vec![candidate_index]));
        }
        regions = refined;
    }

    let mut canonical = BTreeMap::<Vec<usize>, KindSet>::new();
    for (kinds, membership) in regions {
        canonical
            .entry(membership)
            .and_modify(|known| known.union_assign(&kinds))
            .or_insert(kinds);
    }
    canonical
}

#[derive(Clone)]
/// Token-kind languages reaching each observable cutoff outcome bit.
struct SummaryKindSets {
    /// Kinds whose successor can end at the bounded cutoff.
    ends: KindSet,
    /// Kinds whose successor can continue beyond the bounded cutoff.
    continues: KindSet,
}

impl SummaryKindSets {
    /// Creates an empty pair for one grammar-local token-kind width.
    fn empty(kind_count: usize) -> Self {
        Self {
            ends: KindSet::empty(kind_count),
            continues: KindSet::empty(kind_count),
        }
    }

    /// Adds one symbolic kind set to every selected idempotent outcome bit.
    fn add(&mut self, kinds: &KindSet, summary: u8) {
        if summary & 1 != 0 {
            self.ends.union_assign(kinds);
        }
        if summary & 2 != 0 {
            self.continues.union_assign(kinds);
        }
    }

    /// Adds both outcome languages from another factored contribution.
    fn union_assign(&mut self, other: &Self) {
        self.ends.union_assign(&other.ends);
        self.continues.union_assign(&other.continues);
    }

    /// Returns the canonical packed identity used to group equal text cells.
    fn key(&self) -> (Vec<u64>, Vec<u64>) {
        (self.ends.words().to_vec(), self.continues.words().to_vec())
    }

    /// Reports whether neither observable outcome accepts a token kind.
    fn is_empty(&self) -> bool {
        self.ends.is_empty() && self.continues.is_empty()
    }
}

/// Partitions cutoff predicates while preserving token-kind sets symbolically.
pub(crate) fn partition_by_kind_summary<'a, C, P, K, S>(
    candidates: &'a [C],
    grammar: &GrammarDescription,
    predicate_of: P,
    kinds_of: K,
    summary_of: S,
) -> Vec<(TextPartition, u8, KindSet)>
where
    P: Fn(&C) -> Option<TextPredicateId>,
    K: Fn(&'a C) -> &'a KindSet,
    S: Fn(&C) -> u8,
{
    let kind_count = grammar.kinds.len();
    let mut explicit = BTreeSet::new();
    let mut always = SummaryKindSets::empty(kind_count);
    let mut direct = BTreeMap::<String, SummaryKindSets>::new();
    let mut exclusions = Vec::<(BTreeSet<String>, &'a KindSet, u8)>::new();

    for candidate in candidates {
        let kinds = kinds_of(candidate);
        let summary = summary_of(candidate);
        match predicate_of(candidate).map(|predicate| &grammar.text_predicates[predicate.index()]) {
            None | Some(TextPredicate::Any) => always.add(kinds, summary),
            Some(TextPredicate::Exact(value)) => {
                let value = canonical_text(value, grammar.text_equivalence);
                explicit.insert(value.clone());
                direct
                    .entry(value)
                    .or_insert_with(|| SummaryKindSets::empty(kind_count))
                    .add(kinds, summary);
            }
            Some(TextPredicate::OneOf(values)) => {
                for value in values {
                    let value = canonical_text(value, grammar.text_equivalence);
                    explicit.insert(value.clone());
                    direct
                        .entry(value)
                        .or_insert_with(|| SummaryKindSets::empty(kind_count))
                        .add(kinds, summary);
                }
            }
            Some(TextPredicate::NotIn(values)) => {
                let values = values
                    .iter()
                    .map(|value| canonical_text(value, grammar.text_equivalence))
                    .collect::<BTreeSet<_>>();
                explicit.extend(values.iter().cloned());
                exclusions.push((values, kinds, summary));
            }
        }
    }

    let mut other = always.clone();
    for (_, kinds, summary) in &exclusions {
        other.add(kinds, *summary);
    }
    let other_key = other.key();
    let mut grouped = BTreeMap::<(Vec<u64>, Vec<u64>), (SummaryKindSets, Vec<String>)>::new();
    for value in explicit {
        let mut state = always.clone();
        if let Some(contribution) = direct.get(&value) {
            state.union_assign(contribution);
        }
        for (excluded, kinds, summary) in &exclusions {
            if !excluded.contains(&value) {
                state.add(kinds, *summary);
            }
        }
        let grouped = grouped
            .entry(state.key())
            .or_insert_with(|| (state, Vec::new()));
        grouped.1.push(value);
    }

    let mut cells = Vec::new();
    if !other.is_empty() {
        let excluded = grouped
            .iter()
            .filter(|(key, _)| **key != other_key)
            .flat_map(|(_, (_, values))| values.iter().cloned())
            .collect::<Vec<_>>();
        append_summary_kind_cells(
            &mut cells,
            if excluded.is_empty() {
                TextPartition::Any
            } else {
                TextPartition::NotIn(excluded)
            },
            other,
        );
    }
    for (key, (state, values)) in grouped {
        if state.is_empty() || key == other_key {
            continue;
        }
        append_summary_kind_cells(
            &mut cells,
            if values.len() == 1 {
                TextPartition::Exact(values.into_iter().next().expect("one spelling"))
            } else {
                TextPartition::OneOf(values)
            },
            state,
        );
    }
    cells.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.words().cmp(right.2.words()))
    });
    cells
}

/// Splits two overlapping outcome kind sets into pairwise-disjoint summary regions.
fn append_summary_kind_cells(
    cells: &mut Vec<(TextPartition, u8, KindSet)>,
    text: TextPartition,
    state: SummaryKindSets,
) {
    let both = state
        .ends
        .intersection(&state.continues)
        .expect("summary kind sets share one grammar width");
    let ends = state
        .ends
        .difference(&state.continues)
        .expect("summary kind sets share one grammar width");
    let continues = state
        .continues
        .difference(&state.ends)
        .expect("summary kind sets share one grammar width");
    if !ends.is_empty() {
        cells.push((text.clone(), 1, ends));
    }
    if !continues.is_empty() {
        cells.push((text.clone(), 2, continues));
    }
    if !both.is_empty() {
        cells.push((text, 3, both));
    }
}

/// Compact exact residual cells before optional full-residual projection.
struct ResidualPartitions<R> {
    /// Canonical unique residual values indexed by membership bits.
    residuals: Vec<R>,
    /// Canonical text cells with dense residual memberships.
    cells: Vec<ResidualPartitionCell>,
}

/// One compact exact text cell and its residual membership.
struct ResidualPartitionCell {
    /// Canonical spelling partition accepted by this cell.
    text: TextPartition,
    /// Dense membership of canonical residual identities.
    membership: Vec<u64>,
}

/// Builds the one shared canonical text-cell partition over compact residual memberships.
fn partition_residual_memberships<C, R, P, N>(
    candidates: &[C],
    grammar: &GrammarDescription,
    predicate_of: P,
    residual_of: N,
) -> ResidualPartitions<R>
where
    R: Ord,
    P: Fn(&C) -> Option<TextPredicateId>,
    N: Fn(&C) -> R,
{
    let mut candidates = candidates
        .iter()
        .map(|candidate| (residual_of(candidate), predicate_of(candidate)))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    let mut residuals = Vec::<R>::new();
    let mut indexed = Vec::with_capacity(candidates.len());
    for (residual, predicate) in candidates {
        let residual_index = if residuals.last().is_some_and(|known| known == &residual) {
            residuals.len() - 1
        } else {
            residuals.push(residual);
            residuals.len() - 1
        };
        indexed.push((predicate, residual_index));
    }

    let mut explicit = BTreeSet::new();
    for (predicate, _) in &indexed {
        if let Some(predicate) = predicate {
            match &grammar.text_predicates[predicate.index()] {
                TextPredicate::Any => {}
                TextPredicate::Exact(value) => {
                    explicit.insert(canonical_text(value, grammar.text_equivalence));
                }
                TextPredicate::OneOf(values) | TextPredicate::NotIn(values) => {
                    explicit.extend(
                        values
                            .iter()
                            .map(|value| canonical_text(value, grammar.text_equivalence)),
                    );
                }
            }
        }
    }
    let explicit = explicit.into_iter().collect::<Vec<_>>();
    let word_count = residuals.len().div_ceil(u64::BITS as usize);
    let mut always = vec![0_u64; word_count];
    let mut explicit_states = vec![vec![0_u64; word_count]; explicit.len()];
    let mut other = vec![0_u64; word_count];
    for (predicate, residual) in indexed {
        match predicate.map(|predicate| &grammar.text_predicates[predicate.index()]) {
            None | Some(TextPredicate::Any) => membership_insert(&mut always, residual),
            Some(TextPredicate::Exact(value)) => {
                let canonical = canonical_text(value, grammar.text_equivalence);
                let index = explicit
                    .binary_search(&canonical)
                    .expect("authored exact spelling belongs to the explicit partition");
                membership_insert(&mut explicit_states[index], residual);
            }
            Some(TextPredicate::OneOf(values)) => {
                for value in values {
                    let canonical = canonical_text(value, grammar.text_equivalence);
                    let index = explicit
                        .binary_search(&canonical)
                        .expect("authored spelling belongs to the explicit partition");
                    membership_insert(&mut explicit_states[index], residual);
                }
            }
            Some(TextPredicate::NotIn(values)) => {
                membership_insert(&mut other, residual);
                let excluded = values
                    .iter()
                    .map(|value| canonical_text(value, grammar.text_equivalence))
                    .collect::<BTreeSet<_>>();
                for (index, value) in explicit.iter().enumerate() {
                    if !excluded.contains(value) {
                        membership_insert(&mut explicit_states[index], residual);
                    }
                }
            }
        }
    }
    membership_union_assign(&mut other, &always);

    let mut grouped_explicit = BTreeMap::<Vec<u64>, Vec<String>>::new();
    for (text, mut state) in explicit.into_iter().zip(explicit_states) {
        membership_union_assign(&mut state, &always);
        grouped_explicit.entry(state).or_default().push(text);
    }
    let mut cells = Vec::new();
    if membership_is_nonempty(&other) {
        let excluded = grouped_explicit
            .iter()
            .filter(|(state, _)| **state != other)
            .flat_map(|(_, values)| values.iter().cloned())
            .collect::<Vec<_>>();
        cells.push(ResidualPartitionCell {
            text: if excluded.is_empty() {
                TextPartition::Any
            } else {
                TextPartition::NotIn(excluded)
            },
            membership: other.clone(),
        });
    }
    for (state, values) in grouped_explicit {
        if !membership_is_nonempty(&state) || state == other {
            continue;
        }
        cells.push(ResidualPartitionCell {
            text: if values.len() == 1 {
                TextPartition::Exact(values.into_iter().next().expect("one spelling"))
            } else {
                TextPartition::OneOf(values)
            },
            membership: state,
        });
    }
    ResidualPartitions { residuals, cells }
}

/// Adds one dense residual identity to a compact membership set.
fn membership_insert(membership: &mut [u64], residual: usize) {
    membership[residual / u64::BITS as usize] |= 1_u64 << (residual % u64::BITS as usize);
}

/// Unions one compact residual membership set into another.
fn membership_union_assign(into: &mut [u64], from: &[u64]) {
    for (into, from) in into.iter_mut().zip(from) {
        *into |= from;
    }
}

/// Reports whether a compact residual membership set contains any identity.
fn membership_is_nonempty(membership: &[u64]) -> bool {
    membership.iter().any(|word| *word != 0)
}

/// Projects one compact membership set back to canonical ordered residual values.
fn materialize_membership<R: Clone>(membership: &[u64], residuals: &[R]) -> Vec<R> {
    residuals
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            membership[index / u64::BITS as usize] & (1_u64 << (index % u64::BITS as usize)) != 0
        })
        .map(|(_, residual)| residual.clone())
        .collect()
}

/// Canonicalizes a spelling under the grammar's comparison policy.
fn canonical_text(text: &str, equivalence: TextEquivalence) -> String {
    match equivalence {
        TextEquivalence::Sensitive => text.to_owned(),
        TextEquivalence::AsciiInsensitive => text.to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::cmp::Ordering;

    thread_local! {
        /// Counts full residual comparisons made while partitioning a wide frontier.
        static RESIDUAL_COMPARISONS: Cell<usize> = const { Cell::new(0) };
    }

    #[derive(Clone)]
    /// One predicate-to-residual candidate used by partition algebra tests.
    struct Candidate {
        /// Authored predicate identity.
        predicate: TextPredicateId,
        /// Residual selected when the predicate matches.
        residual: char,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    /// One deliberately non-trivial residual whose ordering work is observable.
    struct MeasuredResidual(usize);

    impl Ord for MeasuredResidual {
        fn cmp(&self, other: &Self) -> Ordering {
            RESIDUAL_COMPARISONS.set(RESIDUAL_COMPARISONS.get().saturating_add(1));
            self.0.cmp(&other.0)
        }
    }

    impl PartialOrd for MeasuredResidual {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    #[derive(Clone)]
    /// One mixed text predicate and semantic residual in a wide frontier.
    struct WideCandidate {
        /// Optional authored text predicate.
        predicate: Option<TextPredicateId>,
        /// Residual language selected by the candidate.
        residual: MeasuredResidual,
    }

    #[test]
    fn wide_mixed_frontier_partitions_from_single_candidate_reads() {
        const EXPLICIT_COUNT: usize = 64;
        const CANDIDATE_COUNT: usize = 1_024;
        let mut grammar = GrammarDescription::new(
            "wide-text-partition",
            1,
            5,
            TextEquivalence::AsciiInsensitive,
        );
        let exact = (0..EXPLICIT_COUNT)
            .map(|index| grammar.push_text_predicate(TextPredicate::Exact(format!("KW{index}"))))
            .collect::<Vec<_>>();
        let one_of = (0..16)
            .map(|offset| {
                grammar.push_text_predicate(TextPredicate::OneOf(vec![
                    format!("kw{offset}"),
                    format!("KW{}", offset + 16),
                    format!("kw{}", offset + 32),
                ]))
            })
            .collect::<Vec<_>>();
        let not_in = (0..16)
            .map(|offset| {
                grammar.push_text_predicate(TextPredicate::NotIn(vec![
                    format!("KW{offset}"),
                    format!("kw{}", offset + 48),
                ]))
            })
            .collect::<Vec<_>>();
        let any = grammar.push_text_predicate(TextPredicate::Any);
        let candidates = (0..CANDIDATE_COUNT)
            .map(|index| WideCandidate {
                predicate: match index % 5 {
                    0 => None,
                    1 => Some(any),
                    2 => Some(exact[index % exact.len()]),
                    3 => Some(one_of[index % one_of.len()]),
                    _ => Some(not_in[index % not_in.len()]),
                },
                residual: MeasuredResidual(index),
            })
            .collect::<Vec<_>>();
        let predicate_reads = Cell::new(0_usize);
        let residual_reads = Cell::new(0_usize);
        RESIDUAL_COMPARISONS.set(0);

        let partitions = partition_by_residual(
            &candidates,
            &grammar,
            |candidate| {
                predicate_reads.set(predicate_reads.get().saturating_add(1));
                candidate.predicate
            },
            |candidate| {
                residual_reads.set(residual_reads.get().saturating_add(1));
                candidate.residual.clone()
            },
        );
        assert!(
            predicate_reads.get() <= CANDIDATE_COUNT,
            "predicate metadata must be read once per candidate; predicate_reads={} residual_reads={} residual_comparisons={} structural_cells={}",
            predicate_reads.get(),
            residual_reads.get(),
            RESIDUAL_COMPARISONS.get(),
            partitions.len(),
        );
        assert!(
            residual_reads.get() <= CANDIDATE_COUNT,
            "semantic residuals must be cloned once per candidate; read {} times",
            residual_reads.get(),
        );
        assert!(
            RESIDUAL_COMPARISONS.get() <= CANDIDATE_COUNT * 20,
            "wide cells must not repeatedly sort full residuals; compared {} times",
            RESIDUAL_COMPARISONS.get(),
        );
        assert!(
            partitions.len() <= EXPLICIT_COUNT + 1,
            "partitioning cannot retain more structural cells than spelling classes",
        );
    }

    #[test]
    fn overlapping_predicates_form_exact_residual_truth_cells() {
        let mut grammar =
            GrammarDescription::new("text-partitions", 1, 2, TextEquivalence::AsciiInsensitive);
        let one_of =
            grammar.push_text_predicate(TextPredicate::OneOf(vec!["A".into(), "b".into()]));
        let not_a = grammar.push_text_predicate(TextPredicate::NotIn(vec!["a".into()]));
        let candidates = vec![
            Candidate {
                predicate: one_of,
                residual: 'C',
            },
            Candidate {
                predicate: not_a,
                residual: 'D',
            },
        ];

        let partitions = partition_by_residual(
            &candidates,
            &grammar,
            |candidate| Some(candidate.predicate),
            |candidate| candidate.residual,
        )
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(
            partitions,
            BTreeSet::from([
                (TextPartition::Exact("a".into()), vec!['C']),
                (TextPartition::Exact("b".into()), vec!['C', 'D']),
                (
                    TextPartition::NotIn(vec!["a".into(), "b".into()]),
                    vec!['D']
                ),
            ]),
        );
    }

    #[test]
    fn overlap_uses_the_same_canonical_cell_algebra_as_partitioning() {
        let exact_a = TextPartition::Exact("a".into());
        let one_of = TextPartition::OneOf(vec!["a".into(), "b".into()]);
        let not_a = TextPartition::NotIn(vec!["a".into()]);

        assert!(!exact_a.overlaps(&not_a, TextEquivalence::Sensitive));
        assert!(one_of.overlaps(&not_a, TextEquivalence::Sensitive));
        assert!(TextPartition::Any.overlaps(&exact_a, TextEquivalence::Sensitive));
    }
}
