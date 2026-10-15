//! Declared rejections: the static rule table and the one walk every parse form runs.
//!
//! A rejection rule is a type-level declaration on an enum that names the items a repetition of
//! that enum may not hold together, together with the code, message, hint and citation of the
//! source that rejects them. Every repetition whose element type is that enum runs the rule set as
//! the list is built, and the first item that a rule refuses ends the parse with a declared
//! rejection.
//!
//! A rule refuses an item in one of three ways. A forbidden pair refuses the second of two
//! classes; a `once` set refuses a second item of the set; a text rule refuses one item of one
//! variant by the folded text of its identifier field. An item's class is its variant, unless a
//! classification maps its folded text to another variant or to a text label.
//!
//! The message data is the data a configuration gate carries
//! ([`crate::RequirementNote`]), because both answer the same consumer question: which source
//! rule refused this construct, and what does that source say about it.
//!
//! Three storages run the rule set, and this module holds the one walk all three use. A value
//! parse and an arena parse share one generated list action; a flat parse has a parallel one.
//! Both hand `first_rejection` the same thing: the key of every item in source order, with the
//! extent of the item that the step just added. Nothing below that differs.

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
    /// Citation of the source that rejects the item.
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

    /// The citation of the source that rejects the item.
    pub const fn citation(&self) -> &'static str {
        self.citation
    }
}

/// The text index of an item that has no folded text a rule names.
///
/// A key whose text is this value takes part in no `one_of` rule and in every `not_in` rule of its
/// variant: an item with no folded text, or with one that no rule names, is outside every set.
pub(crate) const NO_TEXT: u32 = u32::MAX;

/// What the walk knows about one item: its variant, its class and its folded text.
///
/// Every number is generated. `variant` is the item's position in its authored enum. `class` is
/// the label the rules see: the variant itself, or the label a classification maps the item's
/// folded text to. `text` is the position of the folded text in the enum's text table, or
/// `NO_TEXT`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectionKey {
    /// Tag of the item's own variant.
    variant: u32,
    /// Tag of the label the rules see for the item.
    class: u32,
    /// Index of the item's folded text in the enum's text table, or `NO_TEXT`.
    text: u32,
}

impl RejectionKey {
    /// Builds the key of one item. Generated code is the only caller.
    #[doc(hidden)]
    pub const fn new(variant: u32, class: u32, text: u32) -> Self {
        Self {
            variant,
            class,
            text,
        }
    }

    /// Builds the key of an item that no classification reads: its class is its variant.
    #[doc(hidden)]
    pub const fn plain(variant: u32) -> Self {
        Self::new(variant, variant, NO_TEXT)
    }
}

/// One forbidden pair of classes, as the two generated tags.
///
/// The pair is unordered: a list rejects whether the left class or the right one comes first.
/// A class matches by tag only, so a payload never takes part in the test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectionPair {
    /// Tag of the class written first.
    left: u32,
    /// Tag of the class written second.
    right: u32,
}

impl RejectionPair {
    /// Builds one forbidden pair. Generated code is the only caller.
    #[doc(hidden)]
    pub const fn new(left: u32, right: u32) -> Self {
        Self { left, right }
    }
}

/// The test one rule makes at one item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RejectionCheck {
    /// The item completes one of these unordered pairs with a class the walk has passed.
    Pairs(&'static [RejectionPair]),
    /// The item's class is in this set, and the walk has already passed a class of the set.
    Once(&'static [u32]),
    /// The item is of this variant, and its folded text is one of these texts.
    TextIn {
        /// Tag of the variant whose items the rule reads.
        variant: u32,
        /// Indexes of the refused texts in the enum's text table.
        texts: &'static [u32],
    },
    /// The item is of this variant, and its folded text is none of these texts.
    TextNotIn {
        /// Tag of the variant whose items the rule reads.
        variant: u32,
        /// Indexes of the admitted texts in the enum's text table.
        texts: &'static [u32],
    },
}

/// One declared rejection rule: the test of one authored attribute and its note.
///
/// One attribute carries one message, so every pair of a rule shares a note. A Node's rules keep
/// their authored order, and the pairs of a rule keep theirs, because the first rule that refuses
/// an item decides the message the parse reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectionRule {
    /// The test this rule makes.
    check: RejectionCheck,
    /// Message data the rule carries.
    note: RejectionNote,
}

impl RejectionRule {
    /// Builds a rule of forbidden pairs. Generated code is the only caller.
    #[doc(hidden)]
    pub const fn new(pairs: &'static [RejectionPair], note: RejectionNote) -> Self {
        Self {
            check: RejectionCheck::Pairs(pairs),
            note,
        }
    }

    /// Builds a rule that admits at most one item of a set of classes. Generated code is the only
    /// caller.
    #[doc(hidden)]
    pub const fn once(classes: &'static [u32], note: RejectionNote) -> Self {
        Self {
            check: RejectionCheck::Once(classes),
            note,
        }
    }

    /// Builds a rule that refuses an item of one variant whose folded text is one of `texts`.
    /// Generated code is the only caller.
    #[doc(hidden)]
    pub const fn text_in(variant: u32, texts: &'static [u32], note: RejectionNote) -> Self {
        Self {
            check: RejectionCheck::TextIn { variant, texts },
            note,
        }
    }

    /// Builds a rule that refuses an item of one variant whose folded text is none of `texts`.
    /// Generated code is the only caller.
    #[doc(hidden)]
    pub const fn text_not_in(variant: u32, texts: &'static [u32], note: RejectionNote) -> Self {
        Self {
            check: RejectionCheck::TextNotIn { variant, texts },
            note,
        }
    }

    /// The message data of this rule.
    pub const fn note(&self) -> &RejectionNote {
        &self.note
    }

    /// Reports whether this rule refuses `key`, given the classes the walk has passed.
    fn refuses(&self, key: RejectionKey, seen: &SeenTags) -> bool {
        match self.check {
            RejectionCheck::Pairs(pairs) => pairs.iter().any(|pair| {
                let partner = if pair.left == key.class {
                    pair.right
                } else if pair.right == key.class {
                    pair.left
                } else {
                    return false;
                };
                seen.contains(partner)
            }),
            RejectionCheck::Once(classes) => {
                classes.contains(&key.class) && classes.iter().any(|class| seen.contains(*class))
            }
            RejectionCheck::TextIn { variant, texts } => {
                key.variant == variant && texts.contains(&key.text)
            }
            RejectionCheck::TextNotIn { variant, texts } => {
                key.variant == variant && !texts.contains(&key.text)
            }
        }
    }
}

/// One declared rejection found in one list of items.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rejection {
    /// Message data of the rule that rejected the list.
    note: &'static RejectionNote,
    /// Extent of the item that a rule refused, when the parse retained spans.
    span: Option<Span>,
}

impl Rejection {
    /// The message data of the rule that rejected the list.
    pub const fn note(&self) -> &'static RejectionNote {
        self.note
    }

    /// The extent of the item that a rule refused.
    pub const fn span(&self) -> Option<Span> {
        self.span
    }
}

/// The class tags one walk has already passed.
///
/// A class tag is a variant's position in its authored enum, or a text label numbered after the
/// variants, so the common enum fits one word and the overflow vector stays empty and
/// unallocated.
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

/// One list's rule set and the classes it has already passed.
///
/// This is the state of the walk, and it is the whole semantics of a rejection rule: it
/// reproduces the incremental check a `gram.y` action makes over a bit set. At each item the
/// filter tests the rules in authored order against the item's key and the classes of the items
/// before it. The first rule that refuses the item answers, so a specific message beats a general
/// one when both refuse the same item and the specific rule is written first.
///
/// A parse runs it through `first_rejection` and stops at the answer. `Arbitrary` sampling runs
/// it through [`RejectionFilter::admits`] and drops the item instead, so a sampled value always
/// parses back.
#[doc(hidden)]
pub struct RejectionFilter {
    /// Rules of the element type, in authored order.
    rules: &'static [RejectionRule],
    /// Class tags the filter has already passed.
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

    /// Returns the note of the first rule that refuses one item, and records the item otherwise.
    #[doc(hidden)]
    pub fn refused(&mut self, key: RejectionKey) -> Option<&'static RejectionNote> {
        if let Some(rule) = self.rules.iter().find(|rule| rule.refuses(key, &self.seen)) {
            return Some(&rule.note);
        }
        self.seen.insert(key.class);
        None
    }

    /// Reports whether one item may join the list, and records it when it may.
    ///
    /// This is [`RejectionFilter::refused`] read the other way round. Generated `Arbitrary`
    /// sampling keeps an item exactly while this answers `true`.
    #[doc(hidden)]
    pub fn admits(&mut self, key: RejectionKey) -> bool {
        self.refused(key).is_none()
    }
}

/// Walks `items` in source order and returns the first declared rejection, if any.
///
/// The span of the answer is the span the caller supplied for that item, so the rejection is
/// positioned at the item that completes the pair and not at the list.
#[doc(hidden)]
pub fn first_rejection<I>(items: I, rules: &'static [RejectionRule]) -> Option<Rejection>
where
    I: IntoIterator<Item = (RejectionKey, Option<Span>)>,
{
    let mut filter = RejectionFilter::new(rules);
    for (key, span) in items {
        if let Some(note) = filter.refused(key) {
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
    fn items(tags: &[u32]) -> Vec<(RejectionKey, Option<Span>)> {
        keyed(
            &tags
                .iter()
                .map(|tag| RejectionKey::plain(*tag))
                .collect::<Vec<_>>(),
        )
    }

    /// Builds one item sequence of explicit keys whose last item carries the only span.
    fn keyed(keys: &[RejectionKey]) -> Vec<(RejectionKey, Option<Span>)> {
        let last = keys.len().saturating_sub(1);
        keys.iter()
            .enumerate()
            .map(|(index, key)| {
                let span = (index == last).then(|| Span::new(index, index + 1).unwrap());
                (*key, span)
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
                (RejectionKey::plain(NOT_DEFERRABLE), Span::new(0, 1).ok()),
                (RejectionKey::plain(DEFERRABLE), Span::new(2, 3).ok()),
                (
                    RejectionKey::plain(INITIALLY_DEFERRED),
                    Span::new(4, 5).ok(),
                ),
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
        .filter(|tag| filter.admits(RejectionKey::plain(*tag)))
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

    /// The option list of `xmltable_column_el` (gram.y REL_17_11 14000-14090): variants `Path`,
    /// `Default`, `NotNull`, `Null` and `Named`, where `Named` is `IDENT b_expr` and its folded
    /// text `path` or `default` makes it count as the keyword arm of the same name.
    mod options {
        use super::*;

        pub(super) const PATH: u32 = 0;
        pub(super) const DEFAULT: u32 = 1;
        pub(super) const NOT_NULL: u32 = 2;
        pub(super) const NULL: u32 = 3;
        pub(super) const NAMED: u32 = 4;

        /// Text table: `path`, `default`, `__pg__is_not_null`.
        pub(super) const TEXT_PATH: u32 = 0;
        pub(super) const TEXT_DEFAULT: u32 = 1;
        pub(super) const TEXT_RESERVED: u32 = 2;

        static ONCE_PATH: [u32; 1] = [PATH];
        static ONCE_DEFAULT: [u32; 1] = [DEFAULT];
        static ONCE_NULLABILITY: [u32; 2] = [NOT_NULL, NULL];
        static RESERVED: [u32; 1] = [TEXT_RESERVED];
        static KNOWN: [u32; 2] = [TEXT_PATH, TEXT_DEFAULT];

        pub(super) static RULES: [RejectionRule; 5] = [
            RejectionRule::text_in(
                NAMED,
                &RESERVED,
                RejectionNote::new(
                    "XmlColumnOption",
                    Some("42601"),
                    "option name cannot be used in XMLTABLE",
                    None,
                    "test",
                ),
            ),
            RejectionRule::once(
                &ONCE_PATH,
                RejectionNote::new(
                    "XmlColumnOption",
                    Some("42601"),
                    "only one PATH value per column is allowed",
                    None,
                    "test",
                ),
            ),
            RejectionRule::once(
                &ONCE_DEFAULT,
                RejectionNote::new(
                    "XmlColumnOption",
                    Some("42601"),
                    "only one DEFAULT value is allowed",
                    None,
                    "test",
                ),
            ),
            RejectionRule::once(
                &ONCE_NULLABILITY,
                RejectionNote::new(
                    "XmlColumnOption",
                    Some("42601"),
                    "conflicting or redundant NULL / NOT NULL declarations",
                    None,
                    "test",
                ),
            ),
            RejectionRule::text_not_in(
                NAMED,
                &KNOWN,
                RejectionNote::new(
                    "XmlColumnOption",
                    Some("42601"),
                    "unrecognized column option",
                    None,
                    "test",
                ),
            ),
        ];

        /// `IDENT b_expr` whose folded text the classification maps to `class`.
        pub(super) fn named(class: u32, text: u32) -> RejectionKey {
            RejectionKey::new(NAMED, class, text)
        }
    }

    fn message(keys: &[RejectionKey]) -> Option<&'static str> {
        first_rejection(keyed(keys), &options::RULES).map(|rejection| rejection.note().message())
    }

    #[test]
    fn once_admits_one_item_of_its_set_in_any_order() {
        use options::*;
        let plain = RejectionKey::plain;
        assert_eq!(message(&[plain(NULL), plain(DEFAULT), plain(PATH)]), None);
        assert_eq!(message(&[plain(NOT_NULL), plain(PATH)]), None);
        assert_eq!(
            message(&[plain(PATH), plain(PATH)]),
            Some("only one PATH value per column is allowed")
        );
        // A set of two classes refuses a repeat of either member and the second member alike.
        assert_eq!(
            message(&[plain(NOT_NULL), plain(NOT_NULL)]),
            Some("conflicting or redundant NULL / NOT NULL declarations")
        );
        assert_eq!(
            message(&[plain(NULL), plain(NOT_NULL)]),
            Some("conflicting or redundant NULL / NOT NULL declarations")
        );
    }

    #[test]
    fn a_classified_item_counts_as_the_class_its_text_maps_to() {
        use options::*;
        let plain = RejectionKey::plain;
        assert_eq!(message(&[named(PATH, TEXT_PATH)]), None);
        assert_eq!(
            message(&[plain(PATH), named(PATH, TEXT_PATH)]),
            Some("only one PATH value per column is allowed")
        );
        assert_eq!(
            message(&[named(DEFAULT, TEXT_DEFAULT), plain(DEFAULT)]),
            Some("only one DEFAULT value is allowed")
        );
    }

    #[test]
    fn a_text_rule_refuses_one_item_even_when_it_is_the_first() {
        use options::*;
        assert_eq!(
            message(&[named(NAMED, NO_TEXT)]),
            Some("unrecognized column option")
        );
        // The specific text rule is written first, so it answers before the general one.
        assert_eq!(
            message(&[named(NAMED, TEXT_RESERVED)]),
            Some("option name cannot be used in XMLTABLE")
        );
        // A text rule reads one variant only.
        assert_eq!(message(&[RejectionKey::new(PATH, PATH, NO_TEXT)]), None);
    }

    #[test]
    fn the_filter_drops_an_item_a_text_rule_refuses() {
        use options::*;
        let mut filter = RejectionFilter::new(&options::RULES);
        assert!(!filter.admits(named(NAMED, NO_TEXT)));
        assert!(filter.admits(named(PATH, TEXT_PATH)));
        assert!(!filter.admits(RejectionKey::plain(PATH)));
        assert!(filter.admits(RejectionKey::plain(NULL)));
    }
}
