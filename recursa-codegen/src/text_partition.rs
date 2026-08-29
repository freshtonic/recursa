//! Canonical text cells shared by symbolic FIRST and frozen dispatch construction.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

use std::collections::{BTreeMap, BTreeSet};

use crate::{GrammarDescription, TextEquivalence, TextPredicate, TextPredicateId};

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
    let mut explicit = BTreeSet::new();
    for candidate in candidates {
        if let Some(predicate) = predicate_of(candidate) {
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

    let mut explicit_states = BTreeMap::<Vec<R>, Vec<String>>::new();
    for text in &explicit {
        explicit_states
            .entry(matching_residual(
                candidates,
                grammar,
                &predicate_of,
                &residual_of,
                Some(text),
            ))
            .or_default()
            .push(text.clone());
    }
    let other = matching_residual(candidates, grammar, &predicate_of, &residual_of, None);
    let mut output = Vec::new();
    if !other.is_empty() {
        let excluded = explicit_states
            .iter()
            .filter(|(state, _)| **state != other)
            .flat_map(|(_, values)| values.iter().cloned())
            .collect::<Vec<_>>();
        output.push((
            if excluded.is_empty() {
                TextPartition::Any
            } else {
                TextPartition::NotIn(excluded)
            },
            other.clone(),
        ));
    }
    for (state, values) in explicit_states {
        if state.is_empty() || state == other {
            continue;
        }
        output.push((
            if values.len() == 1 {
                TextPartition::Exact(values.into_iter().next().expect("one spelling"))
            } else {
                TextPartition::OneOf(values)
            },
            state,
        ));
    }
    output
}

/// Collects the canonical residual reached by one explicit or other-spelling class.
fn matching_residual<C, R, P, N>(
    candidates: &[C],
    grammar: &GrammarDescription,
    predicate_of: &P,
    residual_of: &N,
    text: Option<&str>,
) -> Vec<R>
where
    R: Clone + Ord,
    P: Fn(&C) -> Option<TextPredicateId>,
    N: Fn(&C) -> R,
{
    let mut residuals = candidates
        .iter()
        .filter(|candidate| predicate_matches_class(predicate_of(candidate), text, grammar))
        .map(residual_of)
        .collect::<Vec<_>>();
    residuals.sort();
    residuals.dedup();
    residuals
}

/// Tests one authored predicate against an explicit spelling or the other-spelling class.
fn predicate_matches_class(
    predicate: Option<TextPredicateId>,
    text: Option<&str>,
    grammar: &GrammarDescription,
) -> bool {
    let Some(predicate) = predicate else {
        return true;
    };
    match (&grammar.text_predicates[predicate.index()], text) {
        (TextPredicate::Any, _) => true,
        (TextPredicate::Exact(_), None) | (TextPredicate::OneOf(_), None) => false,
        (TextPredicate::NotIn(_), None) => true,
        (TextPredicate::Exact(expected), Some(actual)) => {
            grammar.text_equivalence.equivalent(expected, actual)
        }
        (TextPredicate::OneOf(expected), Some(actual)) => expected
            .iter()
            .any(|candidate| grammar.text_equivalence.equivalent(candidate, actual)),
        (TextPredicate::NotIn(excluded), Some(actual)) => !excluded
            .iter()
            .any(|candidate| grammar.text_equivalence.equivalent(candidate, actual)),
    }
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

    #[derive(Clone)]
    /// One predicate-to-residual candidate used by partition algebra tests.
    struct Candidate {
        /// Authored predicate identity.
        predicate: TextPredicateId,
        /// Residual selected when the predicate matches.
        residual: char,
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
