//! The occurrence stack of the table-driven parser: provenance folded on reduce (#118).
//!
//! When a parse captures provenance, every state-stack frame has one entry here, pushed on
//! shift with the token's span and folded on reduce into the occurrence described by the rule's
//! generated provenance data. When capture is off the stack does not exist, and the loop in
//! [`super`] never touches it.
//!
//! The fold of a rule is data, not code: a [`RuleFold`] decoded from the packed `folds` table
//! codegen renders from the same site forms its reduce actions come from. A construction rule
//! folds to one node whose children are its sites in grammar-element order; each site kind names
//! the leaf, group, presence, repetition, or pass-through shape for that element. Helper rules
//! (`opt(...)`, `presence(...)`, `list(...)`) fold to the occurrence their inlined form denotes,
//! so a site that delegates to a helper passes the helper's entry through. Every occurrence goes
//! through the same collector, so compaction and span validation stay centralized.
//!
//! A site's explicit capture policy stands, while an inherited policy uses the root entry type's
//! `CAPTURE_POLICY`. A grammar whose nodes override the policy below the root is rejected at
//! generation time (`RCA0406`), because a child rule is reduced before the consuming site is
//! known.

use std::ops::Range;

use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;

use crate::parsed::{
    CapturePolicy, Occurrence, Occurrences, ParseContext, RepetitionSeparatorOccurrence,
};
use crate::{ParseError, Span};

/// The capture policy a fold applies to one occurrence, as the packed table encodes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FoldPolicy {
    /// The parse context's root policy.
    Inherited,
    /// [`CapturePolicy::Off`].
    Off,
    /// [`CapturePolicy::Optional`].
    Optional,
    /// [`CapturePolicy::Mandatory`], which erased optional fixed-token groups always carry.
    Mandatory,
}

impl FoldPolicy {
    /// Decodes the packed value.
    fn decode(value: u32) -> Self {
        match value {
            0 => Self::Inherited,
            1 => Self::Off,
            2 => Self::Optional,
            3 => Self::Mandatory,
            other => panic!("fold policy {other} is not a packed policy"),
        }
    }

    /// Resolves the policy against the parse context.
    fn resolve<A>(self, context: &ParseContext<'_, '_, '_, A>) -> CapturePolicy {
        match self {
            Self::Inherited => context.capture_policy(),
            Self::Off => CapturePolicy::Off,
            Self::Optional => CapturePolicy::Optional,
            Self::Mandatory => CapturePolicy::Mandatory,
        }
    }
}

/// What the reduce of one rule folds its popped entries into.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FoldKind {
    /// The one popped entry stays as it is: a content-token or fixed-token rule, whose leaf the
    /// consuming site finishes under its own policy.
    Through,
    /// An absent optional helper: no occurrence, at the cursor.
    Absent,
    /// A grammar type, attachment unit, fixed-only sequence or presence envelope: one node
    /// spanning the rule, with one child per site.
    Node,
    /// A present optional helper: one group over every popped entry.
    Group,
    /// A present or absent one-or-more list inside an optional helper: one repetition group
    /// over every popped entry.
    Repetition,
    /// `list1 -> e`: a list of one item.
    ListStart,
    /// `list1 -> list1 [sep] e`: the list extended by a separator and an item.
    ListExtend,
    /// `list1t -> list1 [sep]` and `list -> list1 [sep]`: the list with a trailing separator;
    /// `list -> %empty`: an empty list.
    ListThrough,
}

impl FoldKind {
    /// Decodes the packed value.
    fn decode(value: u32) -> Self {
        match value {
            0 => Self::Through,
            1 => Self::Absent,
            2 => Self::Node,
            3 => Self::Group,
            4 => Self::Repetition,
            5 => Self::ListStart,
            6 => Self::ListExtend,
            7 => Self::ListThrough,
            other => panic!("fold kind {other} is not a packed kind"),
        }
    }
}

/// The occurrence one site of a node fold builds from the entries it occupies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SiteKind {
    /// A fixed token: the leaf of its one entry under the site's policy.
    Leaf,
    /// A semantic element, or an inlined present optional or erased group: a group over the
    /// entries, whose leaves are finished under the site's policy.
    Group,
    /// A helper's or attachment operand's entry: its occurrence unchanged.
    Through,
    /// An absent inlined optional or erased group: no occurrence.
    Absent,
    /// An inlined presence: the outer group over the inner group of the sequence tokens, or
    /// over an absent inner occurrence when the sequence did not occur.
    Presence,
    /// An inlined repetition, present or absent: the repetition group over the list entry and
    /// any trailing separator.
    Repetition,
}

impl SiteKind {
    /// Decodes the packed value.
    fn decode(value: u32) -> Self {
        match value {
            0 => Self::Leaf,
            1 => Self::Group,
            2 => Self::Through,
            3 => Self::Absent,
            4 => Self::Presence,
            5 => Self::Repetition,
            other => panic!("site kind {other} is not a packed kind"),
        }
    }
}

/// One site of a node fold: the entries it occupies and the shape it builds from them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SiteFold {
    /// Index of the site's first entry among the rule's popped entries.
    pub start: u32,
    /// Number of entries the site occupies; zero for an absent choice.
    pub len: u32,
    /// The shape built.
    pub kind: SiteKind,
    /// The policy applied to the site's group and its leaves.
    pub policy: FoldPolicy,
}

/// The fold of one rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleFold {
    /// What the popped entries fold into.
    pub kind: FoldKind,
    /// The policy of the node, group or leaf the fold builds.
    pub policy: FoldPolicy,
    /// The sites of a [`FoldKind::Node`], as a range of [`FoldTable::sites`].
    pub sites: Range<usize>,
}

/// The decoded `folds` table: one [`RuleFold`] per rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoldTable {
    /// The fold of each rule, indexed by rule.
    pub rules: Vec<RuleFold>,
    /// The sites of every node fold, contiguous per rule.
    pub sites: Vec<SiteFold>,
    /// Whether any fold carries a mandatory policy, in which case the stack retains provenance
    /// even when the root's policy does not capture.
    pub mandatory: bool,
}

impl FoldTable {
    /// Decodes the flat packed values: per rule `kind, policy, site count`, then per site
    /// `start, len, kind, policy`.
    pub fn decode(values: &[u32]) -> Self {
        let mut rules = Vec::new();
        let mut sites = Vec::new();
        let mut mandatory = false;
        let mut values = values.iter().copied();
        while let Some(kind) = values.next() {
            let mut next = || values.next().expect("a packed fold is complete");
            let policy = FoldPolicy::decode(next());
            let count = next() as usize;
            let first = sites.len();
            for _ in 0..count {
                let site = SiteFold {
                    start: next(),
                    len: next(),
                    kind: SiteKind::decode(next()),
                    policy: FoldPolicy::decode(next()),
                };
                mandatory |= site.policy == FoldPolicy::Mandatory;
                sites.push(site);
            }
            mandatory |= policy == FoldPolicy::Mandatory;
            rules.push(RuleFold {
                kind: FoldKind::decode(kind),
                policy,
                sites: first..sites.len(),
            });
        }
        Self {
            rules,
            sites,
            mandatory,
        }
    }
}

/// What one entry's symbol contributed.
enum Payload<'arena> {
    /// A shifted token, whose span was validated on shift. The site that consumes it applies
    /// its policy, the way `parse_fixed` and a content token's `finish` take theirs.
    Token(Span),
    /// A folded nonterminal: the occurrence its rule built, final.
    Folded(Occurrence<'arena>),
    /// A list helper: the items and separators gathered so far, not yet a group.
    List {
        /// Item occurrences accumulated in source order.
        items: Occurrences<'arena>,
        /// The separators shifted between and after the items.
        separators: BumpVec<'arena, RepetitionSeparatorOccurrence>,
    },
}

/// One entry, parallel to a state-stack frame entered by a shift or a goto.
struct Entry<'arena> {
    /// The absolute index of the first token the entry's symbol covers.
    first: u32,
    /// The absolute index after the last token it covers; equal to `first` for an empty symbol.
    end: u32,
    /// What the symbol contributed.
    payload: Payload<'arena>,
}

/// The occurrence stack; see the module documentation.
pub(super) struct OccurrenceStack<'arena> {
    /// Arena backing list accumulators and every folded occurrence node.
    arena: &'arena Bump,
    /// One entry per state-stack frame above the start frame.
    entries: Vec<Entry<'arena>>,
}

impl<'arena> OccurrenceStack<'arena> {
    /// Creates the stack of one capturing parse.
    pub(super) fn new(arena: &'arena Bump) -> Self {
        Self {
            arena,
            entries: Vec::new(),
        }
    }

    /// Records a shifted token, validating its span against the source as `parse_fixed` does.
    pub(super) fn shift<A>(
        &mut self,
        context: &ParseContext<'_, '_, 'arena, A>,
        token: u32,
        span: Span,
    ) -> Result<(), ParseError> {
        if context.source().get(span.range()).is_none() {
            return Err(context.invalid_token_span(span));
        }
        self.entries.push(Entry {
            first: token,
            end: token + 1,
            payload: Payload::Token(span),
        });
        Ok(())
    }

    /// Folds the top `pops` entries by the rule's fold and pushes the result.
    pub(super) fn reduce<A>(
        &mut self,
        context: &ParseContext<'_, '_, 'arena, A>,
        folds: &FoldTable,
        rule: u32,
        pops: usize,
    ) {
        let fold = &folds.rules[rule as usize];
        let kept = self.entries.len() - pops;
        let cursor = context.cursor() as u32;
        let (first, end) = match &self.entries[kept..] {
            [] => (cursor, cursor),
            [only] => (only.first, only.end),
            [head, .., last] => (head.first, last.end),
        };
        let popped = &mut self.entries[kept..];
        let payload = match fold.kind {
            FoldKind::Through => {
                debug_assert_eq!(pops, 1, "a pass-through rule has exactly one symbol");
                return;
            }
            FoldKind::Absent => Payload::Folded(Occurrence::absent()),
            FoldKind::Node => {
                let sites = &folds.sites[fold.sites.clone()];
                let mut children = BumpVec::with_capacity_in(sites.len(), self.arena);
                for site in sites {
                    children.push(fold_site(context, popped, end, site));
                }
                let span = consumed_span(context, first, end);
                let policy = fold.policy.resolve(context);
                Payload::Folded(context.collect_occurrence(
                    policy,
                    span,
                    children,
                    BumpVec::new_in(self.arena),
                ))
            }
            FoldKind::Group | FoldKind::Repetition => {
                let site = SiteFold {
                    start: 0,
                    len: pops as u32,
                    kind: if fold.kind == FoldKind::Group {
                        SiteKind::Group
                    } else {
                        SiteKind::Repetition
                    },
                    policy: fold.policy,
                };
                Payload::Folded(fold_site(context, popped, end, &site))
            }
            FoldKind::ListStart => {
                let [item] = popped else {
                    unreachable!("a list starts with exactly one item");
                };
                let mut items = Occurrences::new_in(self.arena);
                items.push(item_occurrence(context, fold.policy, take(item)));
                Payload::List {
                    items,
                    separators: BumpVec::new_in(self.arena),
                }
            }
            FoldKind::ListExtend => {
                let (list, rest) = popped
                    .split_first_mut()
                    .expect("a list extension pops its list");
                let Payload::List {
                    mut items,
                    mut separators,
                } = take(list)
                else {
                    unreachable!("a list extension pops a list first");
                };
                let (item, between) = rest
                    .split_last_mut()
                    .expect("a list extension pops the item last");
                for separator in between {
                    separators.push(separator_occurrence(take(separator)));
                }
                items.push(item_occurrence(context, fold.policy, take(item)));
                Payload::List { items, separators }
            }
            FoldKind::ListThrough => match popped.split_first_mut() {
                None => Payload::List {
                    items: Occurrences::new_in(self.arena),
                    separators: BumpVec::new_in(self.arena),
                },
                Some((list, rest)) => {
                    let Payload::List {
                        items,
                        mut separators,
                    } = take(list)
                    else {
                        unreachable!("a trailing list pops a list first");
                    };
                    for separator in rest {
                        separators.push(separator_occurrence(take(separator)));
                    }
                    Payload::List { items, separators }
                }
            },
        };
        self.entries.truncate(kept);
        self.entries.push(Entry {
            first,
            end,
            payload,
        });
    }

    /// Returns the root's occurrence once the accept rule has popped everything else: the
    /// folded entry, or a content-token root's leaf under the root's policy.
    pub(super) fn into_root<A>(
        mut self,
        context: &ParseContext<'_, '_, 'arena, A>,
    ) -> Occurrence<'arena> {
        let entry = self.entries.pop().expect("the accepted root has an entry");
        debug_assert!(self.entries.is_empty(), "acceptance pops every other entry");
        match entry.payload {
            Payload::Folded(occurrence) => occurrence,
            Payload::Token(span) => context.collect_occurrence(
                context.capture_policy(),
                Some(span),
                BumpVec::new_in(self.arena),
                BumpVec::new_in(self.arena),
            ),
            Payload::List { .. } => unreachable!("a list helper is never a grammar type"),
        }
    }
}

/// Takes an entry's payload, leaving an absent one behind; every payload is consumed once.
fn take<'arena>(entry: &mut Entry<'arena>) -> Payload<'arena> {
    std::mem::replace(&mut entry.payload, Payload::Folded(Occurrence::absent()))
}

/// The occurrence of a list item or a direct child: a folded nonterminal's occurrence, or a
/// content token's leaf finished under the consuming site's policy.
fn item_occurrence<'arena, A>(
    context: &ParseContext<'_, '_, 'arena, A>,
    policy: FoldPolicy,
    payload: Payload<'arena>,
) -> Occurrence<'arena> {
    match payload {
        Payload::Folded(occurrence) => occurrence,
        Payload::Token(span) => context.collect_occurrence(
            policy.resolve(context),
            Some(span),
            BumpVec::new_in(context.provenance_arena()),
            BumpVec::new_in(context.provenance_arena()),
        ),
        Payload::List { .. } => unreachable!("a list helper never stands for an item"),
    }
}

/// The separator provenance of a shifted separator token.
fn separator_occurrence(payload: Payload<'_>) -> RepetitionSeparatorOccurrence {
    let Payload::Token(span) = payload else {
        unreachable!("a repetition separator is a fixed token")
    };
    RepetitionSeparatorOccurrence::from_span(span)
}

/// The source extent between two token indexes, exactly as `ParseContext::consumed_span_from`
/// reports it for a parse that began at `first` and stands at `end`: `None` past the end of
/// input, and for an empty range the extent from the next token's start to the previous
/// token's end, which is a zero-width span when the two are adjacent.
fn consumed_span<A>(context: &ParseContext<'_, '_, '_, A>, first: u32, end: u32) -> Option<Span> {
    let first = context.record(first as usize)?;
    let last = context.record((end as usize).checked_sub(1)?)?;
    Span::new(first.span.start() as usize, last.span.end() as usize).ok()
}

/// Builds one site's occurrence from the entries it occupies; `rule_end` locates an empty site
/// after the rule's last entry.
fn fold_site<'arena, A>(
    context: &ParseContext<'_, '_, 'arena, A>,
    popped: &mut [Entry<'arena>],
    rule_end: u32,
    site: &SiteFold,
) -> Occurrence<'arena> {
    let start = site.start as usize;
    let range = start..start + site.len as usize;
    let policy = site.policy.resolve(context);
    let extent = |popped: &[Entry<'_>]| -> (u32, u32) {
        if range.is_empty() {
            let position = popped.get(start).map_or(rule_end, |entry| entry.first);
            (position, position)
        } else {
            (popped[range.start].first, popped[range.end - 1].end)
        }
    };
    match site.kind {
        SiteKind::Absent => Occurrence::absent(),
        SiteKind::Leaf | SiteKind::Through => {
            debug_assert_eq!(site.len, 1, "a leaf or pass-through site has one entry");
            item_occurrence(context, site.policy, take(&mut popped[start]))
        }
        SiteKind::Group => {
            let (first, end) = extent(popped);
            let mut children = BumpVec::with_capacity_in(range.len(), context.provenance_arena());
            for entry in &mut popped[range] {
                children.push(item_occurrence(context, site.policy, take(entry)));
            }
            let span = consumed_span(context, first, end);
            context.collect_occurrence(
                policy,
                span,
                children,
                BumpVec::new_in(context.provenance_arena()),
            )
        }
        SiteKind::Presence => {
            let (first, end) = extent(popped);
            let span = consumed_span(context, first, end);
            let inner = if range.is_empty() {
                Occurrence::absent()
            } else {
                let mut leaves = BumpVec::with_capacity_in(range.len(), context.provenance_arena());
                for entry in &mut popped[range] {
                    leaves.push(item_occurrence(context, site.policy, take(entry)));
                }
                context.collect_occurrence(
                    policy,
                    span,
                    leaves,
                    BumpVec::new_in(context.provenance_arena()),
                )
            };
            let mut children = BumpVec::with_capacity_in(1, context.provenance_arena());
            children.push(inner);
            context.collect_occurrence(
                policy,
                span,
                children,
                BumpVec::new_in(context.provenance_arena()),
            )
        }
        SiteKind::Repetition => {
            let (first, end) = extent(popped);
            let mut items = Occurrences::new_in(context.provenance_arena());
            let mut separators = BumpVec::new_in(context.provenance_arena());
            for entry in &mut popped[range] {
                match take(entry) {
                    Payload::List {
                        items: list_items,
                        separators: list_separators,
                    } => {
                        items = list_items;
                        separators = list_separators;
                    }
                    Payload::Token(span) => {
                        separators.push(RepetitionSeparatorOccurrence::from_span(span));
                    }
                    Payload::Folded(_) => {
                        unreachable!("a repetition site holds lists and separators")
                    }
                }
            }
            let span = consumed_span(context, first, end);
            context.collect_occurrence(policy, span, items.into_retained(), separators)
        }
    }
}
