//! Declared rejections: the static rule table and the one walk every parse form runs.
//!
//! A rejection rule is a type-level declaration on an enum that names forbidden unordered pairs
//! of its variants together with the code, message, hint and citation of the source that rejects
//! them. Every repetition whose element type is that enum runs the rule set as the list is built,
//! and the first item that completes a forbidden pair ends the parse with a declared rejection.
//!
//! The message data is the data a configuration gate carries
//! ([`crate::RequirementNote`]), because both answer the same consumer question: which source
//! rule refused this construct, and what does that source say about it.
//!
//! Three storages run the rule set, and this module holds the one walk all three use. A value
//! parse and an arena parse share one generated list action; a flat parse has a parallel one.
//! Both hand `first_rejection` the same thing: the variant tag of every item in source order,
//! with the extent of the item that the step just added. Nothing below that differs.

use crate::span::Span;

/// Static description of one declared rejection rule's message data.
///
/// The three message fields are opaque to Recursa, exactly as a gate's are: a consumer supplies
/// whatever its own diagnostics need. `message` and `citation` are not optional, because a rule
/// that rejects a parse must say why and must name the source that rejects it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectionNote {
    /// Authored name of the enum that declares the rule, such as `TableCheckAttr`.
    site: &'static str,
    /// Opaque code supplied by the rule.
    code: Option<&'static str>,
    /// Opaque message supplied by the rule.
    message: &'static str,
    /// Opaque hint supplied by the rule.
    hint: Option<&'static str>,
    /// Citation of the source that rejects the pair.
    citation: &'static str,
}

impl RejectionNote {
    /// Builds one note. Generated code is the only caller.
    #[doc(hidden)]
    pub const fn new(
        site: &'static str,
        code: Option<&'static str>,
        message: &'static str,
        hint: Option<&'static str>,
        citation: &'static str,
    ) -> Self {
        Self {
            site,
            code,
            message,
            hint,
            citation,
        }
    }

    /// The authored name of the enum that declares the rule.
    pub const fn site(&self) -> &'static str {
        self.site
    }

    /// The opaque code the rule carries.
    pub const fn code(&self) -> Option<&'static str> {
        self.code
    }

    /// The opaque message the rule carries.
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The opaque hint the rule carries.
    pub const fn hint(&self) -> Option<&'static str> {
        self.hint
    }

    /// The citation of the source that rejects the pair.
    pub const fn citation(&self) -> &'static str {
        self.citation
    }
}

/// One forbidden pair of variants, as the two generated variant tags.
///
/// The pair is unordered: a list rejects whether the left variant or the right one comes first.
/// A variant matches by name only, so a payload never takes part in the test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectionPair {
    /// Tag of the variant written first.
    left: u32,
    /// Tag of the variant written second.
    right: u32,
}

impl RejectionPair {
    /// Builds one forbidden pair. Generated code is the only caller.
    #[doc(hidden)]
    pub const fn new(left: u32, right: u32) -> Self {
        Self { left, right }
    }
}

/// One declared rejection rule: the forbidden pairs of one authored attribute and their note.
///
/// One attribute carries one message and any number of pairs, so the pairs of a rule share a
/// note. A Node's rules keep their authored order, and the pairs of a rule keep theirs, because
/// the first pair that a list completes decides the message the parse reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectionRule {
    /// Forbidden pairs in written order.
    pairs: &'static [RejectionPair],
    /// Message data every pair of this rule carries.
    note: RejectionNote,
}

impl RejectionRule {
    /// Builds one rule. Generated code is the only caller.
    #[doc(hidden)]
    pub const fn new(pairs: &'static [RejectionPair], note: RejectionNote) -> Self {
        Self { pairs, note }
    }

    /// The message data of this rule.
    pub const fn note(&self) -> &RejectionNote {
        &self.note
    }
}

/// One declared rejection found in one list of items.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rejection {
    /// Message data of the rule that rejected the list.
    note: &'static RejectionNote,
    /// Extent of the item that completed the forbidden pair, when the parse retained spans.
    span: Option<Span>,
}

impl Rejection {
    /// The message data of the rule that rejected the list.
    pub const fn note(&self) -> &'static RejectionNote {
        self.note
    }

    /// The extent of the item that completed the forbidden pair.
    pub const fn span(&self) -> Option<Span> {
        self.span
    }
}

/// The variant tags one walk has already passed.
///
/// A tag is a variant's position in its authored enum, so the common enum fits one word and the
/// overflow vector stays empty and unallocated.
struct SeenTags {
    /// One bit per tag below 64.
    low: u64,
    /// Every tag from 64 up, in first-seen order.
    high: Vec<u32>,
}

impl SeenTags {
    /// Starts an empty set, which allocates nothing.
    const fn new() -> Self {
        Self {
            low: 0,
            high: Vec::new(),
        }
    }

    /// Reports whether one tag has already been passed.
    fn contains(&self, tag: u32) -> bool {
        if tag < 64 {
            self.low & (1 << tag) != 0
        } else {
            self.high.contains(&tag)
        }
    }

    /// Adds one tag to the set.
    fn insert(&mut self, tag: u32) {
        if tag < 64 {
            self.low |= 1 << tag;
        } else if !self.high.contains(&tag) {
            self.high.push(tag);
        }
    }
}

/// One list's rule set and the variants it has already passed.
///
/// This is the state of the walk, and it is the whole semantics of a rejection rule: it
/// reproduces the incremental check a `gram.y` action makes over a bit set. At each item the
/// filter tests the rules in authored order and, inside each rule, the pairs in written order,
/// against the variants of the items before it. The first pair a rule completes answers, so a
/// specific message beats a general one when both complete at the same item and the specific rule
/// is written first.
///
/// A parse runs it through `first_rejection` and stops at the answer. `Arbitrary` sampling runs
/// it through [`RejectionFilter::admits`] and drops the item instead, so a sampled value always
/// parses back.
#[doc(hidden)]
pub struct RejectionFilter {
    /// Rules of the element type, in authored order.
    rules: &'static [RejectionRule],
    /// Variant tags the filter has already passed.
    seen: SeenTags,
}

impl RejectionFilter {
    /// Starts an empty filter over one element type's rules.
    #[doc(hidden)]
    pub const fn new(rules: &'static [RejectionRule]) -> Self {
        Self {
            rules,
            seen: SeenTags::new(),
        }
    }

    /// Returns the note of the first rule one item completes, and records the item otherwise.
    #[doc(hidden)]
    pub fn refused(&mut self, tag: u32) -> Option<&'static RejectionNote> {
        for rule in self.rules {
            for pair in rule.pairs {
                let partner = if pair.left == tag {
                    pair.right
                } else if pair.right == tag {
                    pair.left
                } else {
                    continue;
                };
                if self.seen.contains(partner) {
                    return Some(&rule.note);
                }
            }
        }
        self.seen.insert(tag);
        None
    }

    /// Reports whether one item may join the list, and records it when it may.
    ///
    /// This is [`RejectionFilter::refused`] read the other way round. Generated `Arbitrary`
    /// sampling keeps an item exactly while this answers `true`.
    #[doc(hidden)]
    pub fn admits(&mut self, tag: u32) -> bool {
        self.refused(tag).is_none()
    }
}

/// Walks `items` in source order and returns the first declared rejection, if any.
///
/// The span of the answer is the span the caller supplied for that item, so the rejection is
/// positioned at the item that completes the pair and not at the list.
#[doc(hidden)]
pub fn first_rejection<I>(items: I, rules: &'static [RejectionRule]) -> Option<Rejection>
where
    I: IntoIterator<Item = (u32, Option<Span>)>,
{
    let mut filter = RejectionFilter::new(rules);
    for (tag, span) in items {
        if let Some(note) = filter.refused(tag) {
            return Some(Rejection { note, span });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOT_DEFERRABLE: u32 = 0;
    const DEFERRABLE: u32 = 1;
    const INITIALLY_IMMEDIATE: u32 = 2;
    const INITIALLY_DEFERRED: u32 = 3;

    static SPECIFIC_PAIRS: [RejectionPair; 1] =
        [RejectionPair::new(NOT_DEFERRABLE, INITIALLY_DEFERRED)];
    static GENERAL_PAIRS: [RejectionPair; 2] = [
        RejectionPair::new(NOT_DEFERRABLE, DEFERRABLE),
        RejectionPair::new(INITIALLY_IMMEDIATE, INITIALLY_DEFERRED),
    ];

    static RULES: [RejectionRule; 2] = [
        RejectionRule::new(
            &SPECIFIC_PAIRS,
            RejectionNote::new(
                "TableCheckAttr",
                Some("42601"),
                "constraint declared INITIALLY DEFERRED must be DEFERRABLE",
                None,
                "gram.y REL_17_11 6114",
            ),
        ),
        RejectionRule::new(
            &GENERAL_PAIRS,
            RejectionNote::new(
                "TableCheckAttr",
                Some("42601"),
                "conflicting constraint properties",
                None,
                "gram.y REL_17_11 6121",
            ),
        ),
    ];

    /// Builds one item sequence whose last item carries the only span.
    fn items(tags: &[u32]) -> Vec<(u32, Option<Span>)> {
        let last = tags.len().saturating_sub(1);
        tags.iter()
            .enumerate()
            .map(|(index, tag)| {
                let span = (index == last).then(|| Span::new(index, index + 1).unwrap());
                (*tag, span)
            })
            .collect()
    }

    #[test]
    fn a_conflict_free_list_and_a_repeated_variant_are_accepted() {
        assert!(first_rejection(items(&[DEFERRABLE, INITIALLY_DEFERRED]), &RULES).is_none());
        assert!(first_rejection(items(&[DEFERRABLE, DEFERRABLE, DEFERRABLE]), &RULES).is_none());
        assert!(first_rejection(items(&[]), &RULES).is_none());
    }

    #[test]
    fn a_pair_is_unordered_and_reports_the_completing_item() {
        let forward = first_rejection(items(&[NOT_DEFERRABLE, INITIALLY_DEFERRED]), &RULES)
            .expect("the second item completes the pair");
        let backward = first_rejection(items(&[INITIALLY_DEFERRED, NOT_DEFERRABLE]), &RULES)
            .expect("the second item completes the pair");
        assert_eq!(
            forward.note().message(),
            "constraint declared INITIALLY DEFERRED must be DEFERRABLE"
        );
        assert_eq!(forward.note().message(), backward.note().message());
        assert_eq!(forward.span(), Span::new(1, 2).ok());
        assert_eq!(backward.span(), Span::new(1, 2).ok());
    }

    #[test]
    fn attribute_order_decides_which_message_a_shared_item_reports() {
        // `NOT DEFERRABLE` completes the specific pair with `INITIALLY DEFERRED` and the general
        // pair with `DEFERRABLE` at the same item. The first rule written wins.
        let rejection = first_rejection(
            items(&[DEFERRABLE, INITIALLY_DEFERRED, NOT_DEFERRABLE]),
            &RULES,
        )
        .expect("the third item completes both pairs");
        assert_eq!(
            rejection.note().message(),
            "constraint declared INITIALLY DEFERRED must be DEFERRABLE"
        );
    }

    #[test]
    fn a_later_rule_reports_its_own_message() {
        let rejection = first_rejection(items(&[INITIALLY_IMMEDIATE, INITIALLY_DEFERRED]), &RULES)
            .expect("the second item completes the general pair");
        assert_eq!(
            rejection.note().message(),
            "conflicting constraint properties"
        );
        assert_eq!(rejection.note().citation(), "gram.y REL_17_11 6121");
    }

    #[test]
    fn the_walk_stops_at_the_first_item_that_completes_a_pair() {
        // Three conflicts are present; the answer is the earliest one in source order.
        let rejection = first_rejection(
            [
                (NOT_DEFERRABLE, Span::new(0, 1).ok()),
                (DEFERRABLE, Span::new(2, 3).ok()),
                (INITIALLY_DEFERRED, Span::new(4, 5).ok()),
            ],
            &RULES,
        )
        .expect("the second item already conflicts");
        assert_eq!(rejection.span(), Span::new(2, 3).ok());
        assert_eq!(
            rejection.note().message(),
            "conflicting constraint properties"
        );
    }

    #[test]
    fn the_filter_drops_every_item_that_would_complete_a_pair() {
        let mut filter = RejectionFilter::new(&RULES);
        let kept = [
            DEFERRABLE,
            NOT_DEFERRABLE,
            INITIALLY_DEFERRED,
            INITIALLY_IMMEDIATE,
        ]
        .into_iter()
        .filter(|tag| filter.admits(*tag))
        .collect::<Vec<_>>();
        // `NOT DEFERRABLE` conflicts with the `DEFERRABLE` already kept, so it goes; the two
        // later items then conflict with nothing that survived.
        assert_eq!(kept, vec![DEFERRABLE, INITIALLY_DEFERRED]);
        assert!(
            first_rejection(items(&kept), &RULES).is_none(),
            "what the filter keeps always parses"
        );
    }

    #[test]
    fn a_tag_above_the_first_word_still_takes_part() {
        static WIDE_PAIRS: [RejectionPair; 1] = [RejectionPair::new(7, 70)];
        static WIDE: [RejectionRule; 1] = [RejectionRule::new(
            &WIDE_PAIRS,
            RejectionNote::new("Wide", None, "wide conflict", None, "test"),
        )];
        assert!(first_rejection(items(&[70, 7]), &WIDE).is_some());
        assert!(first_rejection(items(&[7, 70]), &WIDE).is_some());
        assert!(first_rejection(items(&[70, 70]), &WIDE).is_none());
    }
}
