//! Frozen, source-independent parse decisions owned by semantic analysis.
//!
//! These products deliberately retain semantic IDs. Source analysis joins them to source
//! identities; generation renders them without recomputing FIRST, FOLLOW, container,
//! Pratt, or failure behavior.
//!
//! Validation here is the trust boundary between semantic analysis and
//! generation. A plan is useful only while its dense IDs, dispatch tries,
//! container machines, Pratt roles, and failure edges agree with the same
//! normalized grammar snapshot.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::analysis::{FirstFacts, first_sequences_for_decision_sites};
use crate::ir::RepeatSeparator;
use crate::text_partition::{TextPartition, partition_by_residual};
use crate::{
    DeclId, FirstPosition, FirstSequence, GrammarDescription, KindId, KindSet, PrattPosition,
    SiteId, TextPredicate, TypeId, TypeShape,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Supplies the grammar construct controlled by a dispatch plan consumed when generation replays this decision.
pub enum DispatchKind {
    /// Select one enum alternative.
    Enum,
    /// Select a Pratt null denotation (atom or prefix).
    PrattNud,
    /// Select a Pratt left denotation (infix or postfix).
    PrattLed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One bounded token prefix capable of selecting a dispatch branch.
pub struct DispatchSequence {
    /// Supplies the positions consumed when generation replays this decision.
    positions: Vec<FirstPosition>,
    /// Supplies the on caller follow consumed when generation replays this decision.
    on_caller_follow: bool,
}

impl DispatchSequence {
    /// Returns the ordered token positions in this prefix.
    pub fn positions(&self) -> &[FirstPosition] {
        &self.positions
    }

    /// Returns whether exhaustion selects through the caller's FOLLOW set.
    pub const fn on_caller_follow(&self) -> bool {
        self.on_caller_follow
    }
}

/// One terminal choice in a frozen dispatch trie.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchTerminal {
    /// Declaration selected at this terminal.
    declaration: DeclId,
    /// Whether this terminal represents caller-FOLLOW fallback.
    on_caller_follow: bool,
}

impl DispatchTerminal {
    /// Identifies the declaration selected when this trie terminal is reached.
    pub const fn declaration(self) -> DeclId {
        self.declaration
    }

    /// Returns whether selection depends on caller FOLLOW.
    pub const fn on_caller_follow(self) -> bool {
        self.on_caller_follow
    }
}

/// One already-discriminated edge in a frozen dispatch trie.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchTrieEdge {
    /// Token predicate labeling the edge.
    position: DispatchPosition,
    /// Trie node reached after matching the position.
    next: DispatchTrieNode,
}

impl DispatchTrieEdge {
    /// Returns the token predicate labeling this edge.
    pub const fn position(&self) -> &DispatchPosition {
        &self.position
    }

    /// Returns the node reached after matching the edge.
    pub const fn next(&self) -> &DispatchTrieNode {
        &self.next
    }
}

/// One disjoint token and text partition labeling a frozen dispatch edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchPosition {
    /// Token kinds belonging to this edge partition.
    kinds: KindSet,
    /// Optional owned text refinement derived from the complete outgoing language.
    text_predicate: Option<TextPredicate>,
}

impl DispatchPosition {
    /// Returns the token kinds belonging to this disjoint edge partition.
    pub const fn kinds(&self) -> &KindSet {
        &self.kinds
    }

    /// Returns the text refinement belonging to this disjoint edge partition.
    pub const fn text_predicate(&self) -> Option<&TextPredicate> {
        self.text_predicate.as_ref()
    }
}

/// A frozen dispatch-trie node produced by semantic analysis.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DispatchTrieNode {
    /// Optional decision when the consumed prefix ends here.
    terminal: Option<DispatchTerminal>,
    /// Already-discriminated outgoing token predicates.
    edges: Vec<DispatchTrieEdge>,
}

impl DispatchTrieNode {
    /// Returns the terminal decision at this prefix, when any.
    pub const fn terminal(&self) -> Option<DispatchTerminal> {
        self.terminal
    }

    /// Returns the outgoing discriminated edges.
    pub fn edges(&self) -> &[DispatchTrieEdge] {
        &self.edges
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One declaration selectable by a frozen dispatch plan.
pub struct DispatchBranch {
    /// Supplies the declaration consumed when generation replays this decision.
    declaration: DeclId,
    /// Supplies the position consumed when generation replays this decision.
    position: Option<PrattPosition>,
    /// Supplies the sequences consumed when generation replays this decision.
    sequences: Vec<DispatchSequence>,
}

impl DispatchBranch {
    /// Identifies the declaration represented by this dispatch branch.
    pub const fn declaration(&self) -> DeclId {
        self.declaration
    }

    /// Returns the bounded prefixes that select this branch.
    pub fn sequences(&self) -> &[DispatchSequence] {
        &self.sequences
    }

    /// Pratt position and exact binding powers. Enum branches return `None`.
    pub const fn position(&self) -> Option<PrattPosition> {
        self.position
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete source-independent branch selection for one enum or Pratt role.
pub struct DispatchPlan {
    /// Supplies the owner consumed when generation replays this decision.
    owner: TypeId,
    /// Supplies the kind consumed when generation replays this decision.
    kind: DispatchKind,
    /// Supplies the branches consumed when generation replays this decision.
    branches: Vec<DispatchBranch>,
    /// Supplies the required lookahead consumed when generation replays this decision.
    required_lookahead: usize,
    /// Supplies the on caller follow consumed when generation replays this decision.
    on_caller_follow: Option<DeclId>,
    /// Supplies the trie consumed when generation replays this decision.
    trie: DispatchTrieNode,
}

impl DispatchPlan {
    /// Identifies the grammar type whose alternatives this plan dispatches.
    pub const fn owner(&self) -> TypeId {
        self.owner
    }

    /// Exposes whether dispatch selects a node or one of its variants.
    pub const fn kind(&self) -> DispatchKind {
        self.kind
    }

    /// Returns the selectable branches in declaration order.
    pub fn branches(&self) -> &[DispatchBranch] {
        &self.branches
    }

    /// Reports the bounded lookahead depth proven sufficient for selection.
    pub const fn required_lookahead(&self) -> usize {
        self.required_lookahead
    }

    /// Identifies the declaration selected when lookahead belongs to caller FOLLOW.
    pub const fn on_caller_follow(&self) -> Option<DeclId> {
        self.on_caller_follow
    }

    /// Exposes the immutable trie that performs bounded dispatch.
    pub const fn trie(&self) -> &DispatchTrieNode {
        &self.trie
    }

    /// Renders facts in canonical order.
    pub fn canonical_facts(&self, grammar: &GrammarDescription) -> String {
        let mut output = format!(
            "dispatch {} {} depth={} caller-fallback={}",
            grammar.types[self.owner.index()].name,
            dispatch_name(self.kind),
            self.required_lookahead,
            self.on_caller_follow.map_or("-", |declaration| {
                grammar.declarations[declaration.index()].name.as_str()
            }),
        );
        for branch in &self.branches {
            let declaration = &grammar.declarations[branch.declaration.index()].name;
            write!(
                output,
                "\n  {declaration}{}:",
                branch
                    .position
                    .map_or_else(String::new, |position| format!(" {position:?}")),
            )
            .expect("writing to a String cannot fail");
            for (index, sequence) in branch.sequences.iter().enumerate() {
                write!(
                    output,
                    " {}{}",
                    if index == 0 { "" } else { "| " },
                    render_sequence(sequence, grammar),
                )
                .expect("writing to a String cannot fail");
            }
        }
        output
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// How a parse site obtains the token kinds that may follow it.
pub enum FollowRecipe {
    /// Use only the owning parser's dynamic caller FOLLOW.
    Caller,
    /// Use only a statically known FIRST suffix.
    Static {
        #[doc = "Static FIRST positions that terminate the owning sequence."]
        first: Vec<FirstPosition>,
    },
    /// Use a static FIRST suffix plus caller FOLLOW when that suffix is nullable.
    StaticAndCaller {
        #[doc = "Static FIRST positions checked before caller FOLLOW."]
        first: Vec<FirstPosition>,
    },
}

impl FollowRecipe {
    /// Derives the canonical static first recorded in the immutable decision plan.
    pub fn static_first(&self) -> &[FirstPosition] {
        match self {
            Self::Caller => &[],
            Self::Static { first } | Self::StaticAndCaller { first } => first,
        }
    }

    /// Reports whether this recipe incorporates the caller's FOLLOW set.
    pub const fn uses_caller(&self) -> bool {
        matches!(self, Self::Caller | Self::StaticAndCaller { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Cardinality operation implemented by a container state machine.
pub enum ContainerOperation {
    /// Selects the direct decision for generation.
    Direct,
    /// Selects the optional decision for generation.
    Optional,
    /// Selects the zero or more decision for generation.
    ZeroOrMore,
    /// Selects the one or more decision for generation.
    OneOrMore,
    /// Selects the optional one or more decision for generation.
    OptionalOneOrMore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Frozen repetition/optional parsing behavior for one parse site.
pub struct ContainerMachine {
    /// Supplies the site consumed when generation replays this decision.
    site: SiteId,
    /// Supplies the operation consumed when generation replays this decision.
    operation: ContainerOperation,
    /// Supplies the separator consumed when generation replays this decision.
    separator: Option<RepeatSeparator>,
    /// Supplies the element first consumed when generation replays this decision.
    element_first: Vec<FirstPosition>,
    /// Supplies the separator first consumed when generation replays this decision.
    separator_first: Vec<FirstPosition>,
    /// Supplies the stop consumed when generation replays this decision.
    stop: FollowRecipe,
    /// Supplies the transitions consumed when generation replays this decision.
    transitions: Vec<ContainerTransition>,
}

impl ContainerMachine {
    /// Identifies the grammar position controlled by this container machine.
    pub const fn site(&self) -> SiteId {
        self.site
    }

    /// Exposes the container operation whose states and transitions are planned.
    pub const fn operation(&self) -> ContainerOperation {
        self.operation
    }

    /// Exposes the repetition separator, when the container syntax has one.
    pub const fn separator(&self) -> Option<RepeatSeparator> {
        self.separator
    }

    /// Reports whether the planned separator may appear after the final element.
    pub const fn optional_trailing(&self) -> bool {
        match self.separator {
            Some(separator) => separator.optional_trailing,
            None => false,
        }
    }

    /// Derives the canonical element first recorded in the immutable decision plan.
    pub fn element_first(&self) -> &[FirstPosition] {
        &self.element_first
    }

    /// Derives the canonical separator first recorded in the immutable decision plan.
    pub fn separator_first(&self) -> &[FirstPosition] {
        &self.separator_first
    }

    /// Exposes the FOLLOW recipe that terminates container parsing.
    pub const fn stop(&self) -> &FollowRecipe {
        &self.stop
    }

    /// Derives the canonical transitions recorded in the immutable decision plan.
    pub fn transitions(&self) -> &[ContainerTransition] {
        &self.transitions
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Runtime state of a frozen container machine.
pub enum ContainerState {
    /// Selects the start decision for generation.
    Start,
    /// Selects the after element decision for generation.
    AfterElement,
    /// Selects the after separator decision for generation.
    AfterSeparator,
    /// Selects the done decision for generation.
    Done,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Parser action performed by a container transition.
pub enum ContainerAction {
    /// Consumes and appends the next semantic element.
    ParseElement,
    /// Consumes the separator before another semantic element.
    ParseSeparator,
    /// Terminates the container when its FOLLOW recipe matches.
    Stop,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One guarded state transition in a frozen container machine.
pub struct ContainerTransition {
    /// State required before the transition may be applied.
    from: ContainerState,
    /// State entered after the transition succeeds.
    to: ContainerState,
    /// Parser action performed while traversing the transition.
    action: ContainerAction,
    /// Expected-token recipe that enables the transition.
    selector: ExpectedRecipe,
}

impl ContainerTransition {
    /// Exposes the transition's required starting state.
    pub const fn from(&self) -> ContainerState {
        self.from
    }

    /// Exposes the state entered after applying this transition.
    pub const fn to(&self) -> ContainerState {
        self.to
    }

    /// Exposes the parser action performed by this transition.
    pub const fn action(&self) -> ContainerAction {
        self.action
    }

    /// Exposes the expected-token recipe that enables this transition.
    pub const fn selector(&self) -> &ExpectedRecipe {
        &self.selector
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Semantic reason a parser path can fail and require an expectation.
pub enum FailureEdgeKind {
    /// Selects the required site decision for generation.
    RequiredSite,
    /// Selects the enum dispatch decision for generation.
    EnumDispatch,
    /// Selects the pratt nud dispatch decision for generation.
    PrattNudDispatch,
    /// Selects the pratt led dispatch decision for generation.
    PrattLedDispatch,
    /// Selects the pratt caller follow guard decision for generation.
    PrattCallerFollowGuard,
    /// Selects the container minimum decision for generation.
    ContainerMinimum,
    /// Selects the container continuation decision for generation.
    ContainerContinuation,
    /// Selects the container after separator decision for generation.
    ContainerAfterSeparator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Recipe for computing the token expectation at a failure edge.
pub enum ExpectedRecipe {
    /// Selects the static decision for generation.
    Static(Vec<FirstPosition>),
    /// Selects the follow decision for generation.
    Follow(FollowRecipe),
    /// Selects the continue or stop decision for generation.
    ContinueOrStop {
        /// Supplies the continuation consumed when generation replays this decision.
        continuation: Vec<FirstPosition>,
        /// Supplies the stop consumed when generation replays this decision.
        stop: FollowRecipe,
    },
    /// Selects the pratt extenders at or above min bp decision for generation.
    PrattExtendersAtOrAboveMinBp {
        /// Supplies the branches consumed when generation replays this decision.
        branches: Vec<DispatchBranch>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One source-independent parse failure and its expected-token recipe.
pub struct FailureEdge {
    /// Supplies the owner consumed when generation replays this decision.
    owner: TypeId,
    /// Supplies the site consumed when generation replays this decision.
    site: Option<SiteId>,
    /// Supplies the kind consumed when generation replays this decision.
    kind: FailureEdgeKind,
    /// Supplies the expected consumed when generation replays this decision.
    expected: ExpectedRecipe,
}

impl FailureEdge {
    /// Identifies the grammar type whose parse can traverse this failure edge.
    pub const fn owner(&self) -> TypeId {
        self.owner
    }

    /// Identifies the grammar position responsible for the failure, when localized.
    pub const fn site(&self) -> Option<SiteId> {
        self.site
    }

    /// Classifies the parser operation that can fail along this edge.
    pub const fn kind(&self) -> FailureEdgeKind {
        self.kind
    }

    /// Exposes the expected-token recipe used to diagnose the failure.
    pub const fn expected(&self) -> &ExpectedRecipe {
        &self.expected
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Validated executable decisions derived from one normalized grammar.
///
/// Generation consumes these tables as authority and must not recompute
/// dispatch, caller FOLLOW, container, Pratt, or failure behavior.
pub struct DecisionProducts {
    /// Supplies the dispatches consumed when generation replays this decision.
    dispatches: Vec<DispatchPlan>,
    /// Supplies the follow recipes by site consumed when generation replays this decision.
    follow_recipes_by_site: Vec<FollowRecipe>,
    /// Supplies the container machines consumed when generation replays this decision.
    container_machines: Vec<ContainerMachine>,
    /// Supplies the failure edges consumed when generation replays this decision.
    failure_edges: Vec<FailureEdge>,
}

impl DecisionProducts {
    /// Returns enum and Pratt dispatch plans.
    pub fn dispatches(&self) -> &[DispatchPlan] {
        &self.dispatches
    }

    /// Returns one FOLLOW recipe per dense parse-site ID.
    pub fn follow_recipes_by_site(&self) -> &[FollowRecipe] {
        &self.follow_recipes_by_site
    }

    /// Returns frozen machines for optional and repeated sites.
    pub fn container_machines(&self) -> &[ContainerMachine] {
        &self.container_machines
    }

    /// Returns all parser failure edges and expectation recipes.
    pub fn failure_edges(&self) -> &[FailureEdge] {
        &self.failure_edges
    }

    #[cfg(test)]
    pub(crate) fn corrupt_site_product_for_test(&mut self) {
        self.follow_recipes_by_site.push(FollowRecipe::Caller);
    }

    /// Renders facts in canonical order.
    pub fn canonical_facts(&self, grammar: &GrammarDescription) -> String {
        let mut output = String::new();
        for dispatch in &self.dispatches {
            writeln!(output, "{}", dispatch.canonical_facts(grammar))
                .expect("writing to a String cannot fail");
        }
        for (index, recipe) in self.follow_recipes_by_site.iter().enumerate() {
            let site = &grammar.sites[index];
            let declaration = &grammar.declarations[site.declaration.index()].name;
            writeln!(
                output,
                "follow {declaration} {}{}",
                follow_name(recipe),
                render_positions_suffix(recipe.static_first(), grammar),
            )
            .expect("writing to a String cannot fail");
        }
        output
    }

    /// Validates every decision against the grammar and computed FIRST facts.
    pub(crate) fn validate(
        &self,
        grammar: &GrammarDescription,
        semantic: &[FirstFacts],
    ) -> Result<(), DecisionInvariant> {
        // Validate every cross-table identity before generation can trust this frozen product as executable authority.
        if self.follow_recipes_by_site.len() != grammar.sites.len()
            || self.container_machines.len() != grammar.sites.len()
        {
            return Err(DecisionInvariant::MissingSiteProduct);
        }
        let mut machine_sites = self
            .container_machines
            .iter()
            .map(ContainerMachine::site)
            .collect::<Vec<_>>();
        machine_sites.sort_unstable();
        machine_sites.dedup();
        if machine_sites.len() != grammar.sites.len()
            || machine_sites
                .iter()
                .enumerate()
                .any(|(index, site)| site.index() != index)
        {
            return Err(DecisionInvariant::DuplicateSiteProduct);
        }
        let mut referenced_sites = Vec::new();
        for (index, description) in grammar.types.iter().enumerate() {
            let owner = TypeId::new(index as u32);
            let expected_dispatches = match &description.shape {
                TypeShape::Enum { alternatives } => {
                    referenced_sites
                        .extend(alternatives.iter().flat_map(|item| &item.sites).copied());
                    vec![(DispatchKind::Enum, alternatives.len())]
                }
                TypeShape::Pratt { variants } => {
                    referenced_sites.extend(variants.iter().flat_map(|item| &item.sites).copied());
                    let mut expected = vec![(
                        DispatchKind::PrattNud,
                        variants
                            .iter()
                            .filter(|variant| is_nud(variant.position))
                            .count(),
                    )];
                    let led = variants
                        .iter()
                        .filter(|variant| is_led(variant.position))
                        .count();
                    if led > 0 {
                        expected.push((DispatchKind::PrattLed, led));
                    }
                    expected
                }
                TypeShape::Alias { site } => {
                    referenced_sites.push(*site);
                    Vec::new()
                }
                TypeShape::Sequence { sites } => {
                    referenced_sites.extend(sites.iter().copied());
                    Vec::new()
                }
                TypeShape::Token { .. } => Vec::new(),
            };
            let actual = self
                .dispatches
                .iter()
                .filter(|dispatch| dispatch.owner == owner)
                .collect::<Vec<_>>();
            if actual.len() != expected_dispatches.len()
                || expected_dispatches.iter().any(|(kind, branch_count)| {
                    actual
                        .iter()
                        .filter(|dispatch| dispatch.kind == *kind)
                        .count()
                        != 1
                        || actual
                            .iter()
                            .find(|dispatch| dispatch.kind == *kind)
                            .is_none_or(|dispatch| dispatch.branches.len() != *branch_count)
                })
            {
                return Err(DecisionInvariant::MissingDispatch);
            }
            let has_led = matches!(&description.shape, TypeShape::Pratt { variants } if variants.iter().any(|variant| is_led(variant.position)));
            if has_led
                && !self.failure_edges.iter().any(|edge| {
                    edge.owner == owner && edge.kind == FailureEdgeKind::PrattCallerFollowGuard
                })
            {
                return Err(DecisionInvariant::MissingPrattLedGuard);
            }
        }
        referenced_sites.sort_unstable();
        if referenced_sites
            .windows(2)
            .any(|sites| sites[0] == sites[1])
        {
            return Err(DecisionInvariant::DuplicateSiteUse);
        }
        for edge in &self.failure_edges {
            if expected_is_empty(&edge.expected) {
                return Err(DecisionInvariant::EmptyExpectedRecipe);
            }
        }
        for dispatch in &self.dispatches {
            if dispatch
                .branches
                .iter()
                .filter(|branch| {
                    branch
                        .sequences
                        .iter()
                        .any(|sequence| sequence.positions.is_empty() && sequence.on_caller_follow)
                })
                .count()
                > 1
            {
                return Err(DecisionInvariant::MultipleCallerFallbacks);
            }
            for (left_index, left) in dispatch.branches.iter().enumerate() {
                for right in &dispatch.branches[left_index + 1..] {
                    if left.sequences.iter().any(|left_sequence| {
                        right.sequences.iter().any(|right_sequence| {
                            sequences_overlap(left_sequence, right_sequence, grammar)
                        })
                    }) {
                        return Err(DecisionInvariant::OverlappingDispatch);
                    }
                }
            }
        }
        let mut failure_keys = self
            .failure_edges
            .iter()
            .map(|edge| (edge.owner, edge.site, edge.kind))
            .collect::<Vec<_>>();
        let failure_count = failure_keys.len();
        failure_keys.sort_unstable();
        failure_keys.dedup();
        if failure_keys.len() != failure_count {
            return Err(DecisionInvariant::DuplicateFailureEdge);
        }
        let expected = plan_decisions(grammar, semantic);
        if self.follow_recipes_by_site != expected.follow_recipes_by_site {
            return Err(DecisionInvariant::FollowRecipeMismatch);
        }
        if self.dispatches != expected.dispatches {
            return Err(DecisionInvariant::DispatchProductMismatch);
        }
        if self.container_machines != expected.container_machines {
            return Err(DecisionInvariant::ContainerProductMismatch);
        }
        if self.failure_edges != expected.failure_edges {
            return Err(DecisionInvariant::FailureProductMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the decision invariant alternatives frozen into the decision plan.
pub(crate) enum DecisionInvariant {
    /// Selects the missing site product decision for generation.
    MissingSiteProduct,
    /// Selects the missing dispatch decision for generation.
    MissingDispatch,
    /// Selects the overlapping dispatch decision for generation.
    OverlappingDispatch,
    /// Selects the empty expected recipe decision for generation.
    EmptyExpectedRecipe,
    /// Selects the missing pratt led guard decision for generation.
    MissingPrattLedGuard,
    /// Selects the duplicate site product decision for generation.
    DuplicateSiteProduct,
    /// Selects the duplicate site use decision for generation.
    DuplicateSiteUse,
    /// Selects the multiple caller fallbacks decision for generation.
    MultipleCallerFallbacks,
    /// Selects the duplicate failure edge decision for generation.
    DuplicateFailureEdge,
    /// Selects the follow recipe mismatch decision for generation.
    FollowRecipeMismatch,
    /// Selects the dispatch product mismatch decision for generation.
    DispatchProductMismatch,
    /// Selects the container product mismatch decision for generation.
    ContainerProductMismatch,
    /// Selects the failure product mismatch decision for generation.
    FailureProductMismatch,
}

impl std::fmt::Display for DecisionInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

/// Freezes dispatch and container decisions after validating their semantic inputs.
pub(crate) fn plan_decisions(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
) -> DecisionProducts {
    // Build dispatch, container, Pratt, and failure products from the same semantic snapshot to prevent phase drift.
    let mut follow_recipes_by_site = vec![FollowRecipe::Caller; grammar.sites.len()];
    let mut dispatches = Vec::new();
    for (index, description) in grammar.types.iter().enumerate() {
        let owner = TypeId::new(index as u32);
        match &description.shape {
            TypeShape::Alias { site } => {
                plan_follow_for_sites(&[*site], grammar, semantic, &mut follow_recipes_by_site);
            }
            TypeShape::Sequence { sites } => {
                plan_follow_for_sites(sites, grammar, semantic, &mut follow_recipes_by_site);
            }
            TypeShape::Enum { alternatives } => {
                for alternative in alternatives {
                    plan_follow_for_sites(
                        &alternative.sites,
                        grammar,
                        semantic,
                        &mut follow_recipes_by_site,
                    );
                }
                dispatches.push(dispatch_plan(
                    owner,
                    DispatchKind::Enum,
                    alternatives.iter().map(|alternative| {
                        (alternative.declaration, None, alternative.sites.as_slice())
                    }),
                    grammar,
                    semantic,
                ));
            }
            TypeShape::Pratt { variants } => {
                for variant in variants {
                    plan_follow_for_sites(
                        &variant.sites,
                        grammar,
                        semantic,
                        &mut follow_recipes_by_site,
                    );
                }
                dispatches.push(dispatch_plan(
                    owner,
                    DispatchKind::PrattNud,
                    variants
                        .iter()
                        .filter(|variant| is_nud(variant.position))
                        .map(|variant| {
                            (
                                variant.declaration,
                                Some(variant.position),
                                variant.sites.as_slice(),
                            )
                        }),
                    grammar,
                    semantic,
                ));
                if variants.iter().any(|variant| is_led(variant.position)) {
                    dispatches.push(dispatch_plan(
                        owner,
                        DispatchKind::PrattLed,
                        variants
                            .iter()
                            .filter(|variant| is_led(variant.position))
                            .map(|variant| {
                                (
                                    variant.declaration,
                                    Some(variant.position),
                                    variant.sites.as_slice(),
                                )
                            }),
                        grammar,
                        semantic,
                    ));
                }
            }
            TypeShape::Token { .. } => {}
        }
    }
    dispatches.sort_by(|left, right| {
        grammar.types[left.owner.index()]
            .name
            .cmp(&grammar.types[right.owner.index()].name)
            .then_with(|| left.kind.cmp(&right.kind))
    });

    let container_machines = grammar
        .sites
        .iter()
        .enumerate()
        .map(|(index, site)| {
            let semantics = site
                .container
                .semantics()
                .expect("validated containers have normalized semantics");
            let operation = match semantics.cardinality {
                crate::ir::ContainerCardinality::One => ContainerOperation::Direct,
                crate::ir::ContainerCardinality::Optional => ContainerOperation::Optional,
                crate::ir::ContainerCardinality::ZeroOrMore => ContainerOperation::ZeroOrMore,
                crate::ir::ContainerCardinality::OneOrMore => ContainerOperation::OneOrMore,
                crate::ir::ContainerCardinality::OptionalOneOrMore => {
                    ContainerOperation::OptionalOneOrMore
                }
            };
            let element_first = semantic[site.target.index()].positions.clone();
            let separator_first = semantics.separator.map_or_else(Vec::new, |separator| {
                semantic[separator.type_id.index()].positions.clone()
            });
            let optional_trailing = semantics
                .separator
                .is_some_and(|separator| separator.optional_trailing);
            let stop = follow_recipes_by_site[index].clone();
            let transitions = container_transitions(
                operation,
                &element_first,
                &separator_first,
                optional_trailing,
                &stop,
            );
            ContainerMachine {
                site: SiteId::new(index as u32),
                operation,
                separator: semantics.separator,
                element_first,
                separator_first,
                stop,
                transitions,
            }
        })
        .collect::<Vec<_>>();
    let failure_edges = plan_failure_edges(grammar, semantic, &dispatches, &follow_recipes_by_site);
    DecisionProducts {
        dispatches,
        follow_recipes_by_site,
        container_machines,
        failure_edges,
    }
}

/// Derives the canonical container transitions recorded in the immutable decision plan.
fn container_transitions(
    operation: ContainerOperation,
    element_first: &[FirstPosition],
    separator_first: &[FirstPosition],
    optional_trailing: bool,
    stop: &FollowRecipe,
) -> Vec<ContainerTransition> {
    let element = |from, to| ContainerTransition {
        from,
        to,
        action: ContainerAction::ParseElement,
        selector: ExpectedRecipe::Static(element_first.to_vec()),
    };
    let stop_transition = |from| ContainerTransition {
        from,
        to: ContainerState::Done,
        action: ContainerAction::Stop,
        selector: ExpectedRecipe::Follow(stop.clone()),
    };
    if operation == ContainerOperation::Direct {
        return vec![element(ContainerState::Start, ContainerState::Done)];
    }
    if operation == ContainerOperation::Optional {
        return vec![
            element(ContainerState::Start, ContainerState::Done),
            stop_transition(ContainerState::Start),
        ];
    }
    let mut transitions = vec![element(ContainerState::Start, ContainerState::AfterElement)];
    if matches!(
        operation,
        ContainerOperation::ZeroOrMore | ContainerOperation::OptionalOneOrMore
    ) {
        transitions.push(stop_transition(ContainerState::Start));
    }
    if separator_first.is_empty() {
        transitions.push(element(
            ContainerState::AfterElement,
            ContainerState::AfterElement,
        ));
    } else {
        transitions.push(ContainerTransition {
            from: ContainerState::AfterElement,
            to: ContainerState::AfterSeparator,
            action: ContainerAction::ParseSeparator,
            selector: ExpectedRecipe::Static(separator_first.to_vec()),
        });
        transitions.push(element(
            ContainerState::AfterSeparator,
            ContainerState::AfterElement,
        ));
        if optional_trailing {
            transitions.push(stop_transition(ContainerState::AfterSeparator));
        }
    }
    transitions.push(stop_transition(ContainerState::AfterElement));
    transitions
}

/// Plans follow for sites.
fn plan_follow_for_sites(
    sites: &[SiteId],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    recipes: &mut [FollowRecipe],
) {
    for (index, site) in sites.iter().enumerate() {
        let suffix = first_sequences_for_decision_sites(&sites[index + 1..], grammar, semantic);
        recipes[site.index()] = follow_recipe(&suffix);
    }
}

/// Derives the canonical follow recipe recorded in the immutable decision plan.
fn follow_recipe(sequences: &[FirstSequence]) -> FollowRecipe {
    let mut first = sequences
        .iter()
        .filter_map(|sequence| sequence.positions.first().cloned())
        .collect::<Vec<_>>();
    sort_dedup_positions(&mut first);
    let caller = sequences
        .iter()
        .any(|sequence| sequence.positions.is_empty() && sequence.ends());
    match (first.is_empty(), caller) {
        (true, _) => FollowRecipe::Caller,
        (false, false) => FollowRecipe::Static { first },
        (false, true) => FollowRecipe::StaticAndCaller { first },
    }
}

/// Derives the canonical dispatch plan recorded in the immutable decision plan.
fn dispatch_plan<'a>(
    owner: TypeId,
    kind: DispatchKind,
    alternatives: impl IntoIterator<Item = (DeclId, Option<PrattPosition>, &'a [SiteId])>,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
) -> DispatchPlan {
    let mut branches = alternatives
        .into_iter()
        .map(|(declaration, position, sites)| DispatchBranch {
            declaration,
            position,
            sequences: first_sequences_for_decision_sites(sites, grammar, semantic)
                .into_iter()
                .map(|sequence| {
                    let on_caller_follow = sequence.ends();
                    DispatchSequence {
                        positions: sequence.positions,
                        on_caller_follow,
                    }
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    branches.sort_by(|left, right| {
        grammar.declarations[left.declaration.index()]
            .name
            .cmp(&grammar.declarations[right.declaration.index()].name)
    });
    let required_lookahead = required_dispatch_lookahead(&branches, grammar);
    let on_caller_follow = branches.iter().find_map(|branch| {
        branch
            .sequences
            .iter()
            .any(|sequence| sequence.positions.is_empty() && sequence.on_caller_follow)
            .then_some(branch.declaration)
    });
    let trie = freeze_dispatch_trie(&branches, grammar);
    DispatchPlan {
        owner,
        kind,
        branches,
        required_lookahead,
        on_caller_follow,
        trie,
    }
}

/// One immutable complete sequence used while partitioning a dispatch trie.
#[derive(Clone, Debug)]
struct DispatchPath {
    /// Terminal selected after all positions have matched.
    terminal: DispatchTerminal,
    /// Complete symbolic positions in this path.
    positions: Vec<FirstPosition>,
}

/// One residual path position in a symbolic dispatch state.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DispatchCursor {
    /// Index into the immutable complete path table.
    path: usize,
    /// Next position to match in that path.
    offset: usize,
}

/// Derives the canonical frozen dispatch trie from exact symbolic language partitions.
fn freeze_dispatch_trie(
    branches: &[DispatchBranch],
    grammar: &GrammarDescription,
) -> DispatchTrieNode {
    let paths = branches
        .iter()
        .flat_map(|branch| {
            branch.sequences.iter().map(|sequence| DispatchPath {
                terminal: DispatchTerminal {
                    declaration: branch.declaration,
                    on_caller_follow: sequence.on_caller_follow,
                },
                positions: sequence.positions.clone(),
            })
        })
        .collect::<Vec<_>>();
    let state = (0..paths.len())
        .map(|path| DispatchCursor { path, offset: 0 })
        .collect::<Vec<_>>();
    let mut memo = BTreeMap::new();
    let root = freeze_dispatch_state(&paths, &state, grammar, &mut memo);
    assert!(dispatch_edges_are_disjoint(&root, grammar));
    root
}

/// Freezes one residual-language state and memoizes equivalent suffix states.
fn freeze_dispatch_state(
    paths: &[DispatchPath],
    state: &[DispatchCursor],
    grammar: &GrammarDescription,
    memo: &mut BTreeMap<Vec<DispatchCursor>, DispatchTrieNode>,
) -> DispatchTrieNode {
    if let Some(node) = memo.get(state) {
        return node.clone();
    }

    let terminals = state
        .iter()
        .filter_map(|cursor| {
            (cursor.offset == paths[cursor.path].positions.len())
                .then_some(paths[cursor.path].terminal)
        })
        .collect::<Vec<_>>();
    let terminal = terminals.first().copied();
    if let Some(first) = terminal {
        assert!(
            terminals
                .iter()
                .all(|candidate| candidate.declaration == first.declaration),
            "validated dispatch cannot map one prefix to two terminals"
        );
        if terminals
            .iter()
            .any(|candidate| !candidate.on_caller_follow)
        {
            assert!(
                state
                    .iter()
                    .all(|cursor| { paths[cursor.path].terminal.declaration == first.declaration }),
                "validated dispatch cannot shadow another branch"
            );
            let node = DispatchTrieNode {
                terminal: Some(DispatchTerminal {
                    declaration: first.declaration,
                    on_caller_follow: false,
                }),
                edges: Vec::new(),
            };
            memo.insert(state.to_vec(), node.clone());
            return node;
        }
    }

    let mut partitions = BTreeMap::<(TextPartition, Vec<DispatchCursor>), KindSet>::new();
    for kind_index in 0..grammar.kinds.len() {
        let kind = KindId::new(kind_index.try_into().expect("validated kind width"));
        let candidates = state
            .iter()
            .copied()
            .filter(|cursor| {
                paths[cursor.path]
                    .positions
                    .get(cursor.offset)
                    .is_some_and(|position| position.kinds.contains(kind))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        for (text, residual) in partition_by_residual(
            &candidates,
            grammar,
            |cursor| paths[cursor.path].positions[cursor.offset].text_predicate,
            |cursor| DispatchCursor {
                path: cursor.path,
                offset: cursor.offset + 1,
            },
        ) {
            let kinds = partitions
                .entry((text, residual))
                .or_insert_with(|| KindSet::empty(grammar.kinds.len()));
            kinds.insert(kind).expect("kind belongs to grammar width");
        }
    }

    let mut edges = partitions
        .into_iter()
        .map(|((text, residual), kinds)| DispatchTrieEdge {
            position: DispatchPosition {
                kinds,
                text_predicate: text.into_predicate(),
            },
            next: freeze_dispatch_state(paths, &residual, grammar, memo),
        })
        .collect::<Vec<_>>();
    edges.sort_by(compare_dispatch_edges);
    let node = DispatchTrieNode { terminal, edges };
    memo.insert(state.to_vec(), node.clone());
    node
}

/// Orders disjoint outgoing edges independently of declaration construction order.
fn compare_dispatch_edges(left: &DispatchTrieEdge, right: &DispatchTrieEdge) -> Ordering {
    left.position
        .kinds
        .words()
        .cmp(right.position.kinds.words())
        .then_with(|| {
            TextPartition::from_predicate(left.position.text_predicate.as_ref()).cmp(
                &TextPartition::from_predicate(right.position.text_predicate.as_ref()),
            )
        })
}

/// Verifies every frozen trie node uses pairwise-disjoint edge predicates.
fn dispatch_edges_are_disjoint(node: &DispatchTrieNode, grammar: &GrammarDescription) -> bool {
    for (index, left) in node.edges.iter().enumerate() {
        for right in &node.edges[index + 1..] {
            let kinds_overlap = !left
                .position
                .kinds
                .intersection(&right.position.kinds)
                .expect("dispatch edges share the grammar width")
                .is_empty();
            if kinds_overlap
                && TextPartition::from_predicate(left.position.text_predicate.as_ref()).overlaps(
                    &TextPartition::from_predicate(right.position.text_predicate.as_ref()),
                    grammar.text_equivalence,
                )
            {
                return false;
            }
        }
        if !dispatch_edges_are_disjoint(&left.next, grammar) {
            return false;
        }
    }
    true
}

/// Derives the canonical required dispatch lookahead recorded in the immutable decision plan.
fn required_dispatch_lookahead(branches: &[DispatchBranch], grammar: &GrammarDescription) -> usize {
    let mut required = usize::from(!branches.is_empty());
    for (left_index, left) in branches.iter().enumerate() {
        for right in &branches[left_index + 1..] {
            for left_sequence in &left.sequences {
                for right_sequence in &right.sequences {
                    let depth =
                        sequence_discrimination_depth(left_sequence, right_sequence, grammar)
                            .unwrap_or(grammar.max_lookahead);
                    required = required.max(depth);
                }
            }
        }
    }
    required.min(grammar.max_lookahead)
}

/// Derives the canonical sequence discrimination depth recorded in the immutable decision plan.
fn sequence_discrimination_depth(
    left: &DispatchSequence,
    right: &DispatchSequence,
    grammar: &GrammarDescription,
) -> Option<usize> {
    for (index, (left, right)) in left.positions.iter().zip(&right.positions).enumerate() {
        let kinds_overlap = !left
            .kinds
            .intersection(&right.kinds)
            .expect("decision positions share the grammar kind width")
            .is_empty();
        if !kinds_overlap
            || !crate::analysis::predicates_overlap(
                left.text_predicate,
                right.text_predicate,
                &grammar.text_predicates,
                grammar.text_equivalence,
            )
        {
            return Some(index + 1);
        }
    }
    (left.positions.len() != right.positions.len())
        .then_some(left.positions.len().min(right.positions.len()) + 1)
}

/// Derives parser failure edges and expected-token recipes in canonical order.
fn plan_failure_edges(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    dispatches: &[DispatchPlan],
    follow_recipes: &[FollowRecipe],
) -> Vec<FailureEdge> {
    // Derive and sort failure edges after successful decisions so diagnostics cannot influence parse selection.
    let mut edges = Vec::new();
    for dispatch in dispatches {
        if dispatch.kind == DispatchKind::PrattLed {
            // A led dispatch miss is the ordinary Pratt-loop stop. Only a
            // matched, active extender can reach the D3 caller-FOLLOW guard.
            continue;
        }
        let mut expected = dispatch
            .branches
            .iter()
            .flat_map(|branch| branch.sequences.iter())
            .filter_map(|sequence| sequence.positions.first().cloned())
            .collect::<Vec<_>>();
        sort_dedup_positions(&mut expected);
        let uses_caller = dispatch.on_caller_follow.is_some();
        let expected = match (expected.is_empty(), uses_caller) {
            (true, _) => ExpectedRecipe::Follow(FollowRecipe::Caller),
            (false, false) => ExpectedRecipe::Static(expected),
            (false, true) => {
                ExpectedRecipe::Follow(FollowRecipe::StaticAndCaller { first: expected })
            }
        };
        edges.push(FailureEdge {
            owner: dispatch.owner,
            site: None,
            kind: match dispatch.kind {
                DispatchKind::Enum => FailureEdgeKind::EnumDispatch,
                DispatchKind::PrattNud => FailureEdgeKind::PrattNudDispatch,
                DispatchKind::PrattLed => FailureEdgeKind::PrattLedDispatch,
            },
            expected,
        });
    }
    for (index, site) in grammar.sites.iter().enumerate() {
        if !site.container.semantics().is_some_and(|semantics| {
            matches!(semantics.cardinality, crate::ir::ContainerCardinality::One)
        }) {
            continue;
        }
        let first = semantic[site.target.index()].positions.clone();
        edges.push(FailureEdge {
            owner: site.owner,
            site: Some(SiteId::new(index as u32)),
            kind: FailureEdgeKind::RequiredSite,
            expected: if first.is_empty() {
                ExpectedRecipe::Follow(follow_recipes[index].clone())
            } else {
                ExpectedRecipe::Static(first)
            },
        });
    }
    for (index, description) in grammar.types.iter().enumerate() {
        if matches!(&description.shape, TypeShape::Pratt { variants } if variants.iter().any(|variant| is_led(variant.position)))
        {
            let owner = TypeId::new(index as u32);
            let branches = dispatches
                .iter()
                .find(|dispatch| dispatch.owner == owner && dispatch.kind == DispatchKind::PrattLed)
                .map_or_else(Vec::new, |dispatch| dispatch.branches.clone());
            edges.push(FailureEdge {
                owner,
                site: None,
                kind: FailureEdgeKind::PrattCallerFollowGuard,
                expected: ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches },
            });
        }
    }
    for (index, site) in grammar.sites.iter().enumerate() {
        let site_id = SiteId::new(index as u32);
        let semantics = site
            .container
            .semantics()
            .expect("validated containers have normalized semantics");
        if matches!(
            semantics.cardinality,
            crate::ir::ContainerCardinality::OneOrMore
                | crate::ir::ContainerCardinality::OptionalOneOrMore
        ) {
            edges.push(FailureEdge {
                owner: site.owner,
                site: Some(site_id),
                kind: FailureEdgeKind::ContainerMinimum,
                expected: ExpectedRecipe::Static(semantic[site.target.index()].positions.clone()),
            });
        }
        if semantics.cardinality.is_repeated() {
            let continuation = semantics.separator.map_or_else(
                || semantic[site.target.index()].positions.clone(),
                |separator| semantic[separator.type_id.index()].positions.clone(),
            );
            edges.push(FailureEdge {
                owner: site.owner,
                site: Some(site_id),
                kind: FailureEdgeKind::ContainerContinuation,
                expected: ExpectedRecipe::ContinueOrStop {
                    continuation,
                    stop: follow_recipes[index].clone(),
                },
            });
            if let Some(separator) = semantics.separator {
                edges.push(FailureEdge {
                    owner: site.owner,
                    site: Some(site_id),
                    kind: FailureEdgeKind::ContainerAfterSeparator,
                    expected: if separator.optional_trailing {
                        ExpectedRecipe::ContinueOrStop {
                            continuation: semantic[site.target.index()].positions.clone(),
                            stop: follow_recipes[index].clone(),
                        }
                    } else {
                        ExpectedRecipe::Static(semantic[site.target.index()].positions.clone())
                    },
                });
            }
        }
    }
    edges.sort_by_key(|edge| (edge.owner, edge.site, edge.kind));
    edges
}

/// Derives the canonical expected is empty recorded in the immutable decision plan.
fn expected_is_empty(expected: &ExpectedRecipe) -> bool {
    match expected {
        ExpectedRecipe::Static(first) => first.is_empty(),
        ExpectedRecipe::Follow(FollowRecipe::Caller) => false,
        ExpectedRecipe::Follow(FollowRecipe::Static { first })
        | ExpectedRecipe::Follow(FollowRecipe::StaticAndCaller { first }) => first.is_empty(),
        ExpectedRecipe::ContinueOrStop { continuation, .. } => continuation.is_empty(),
        ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches } => branches.is_empty(),
    }
}

/// Derives the canonical sequences overlap recorded in the immutable decision plan.
fn sequences_overlap(
    left: &DispatchSequence,
    right: &DispatchSequence,
    grammar: &GrammarDescription,
) -> bool {
    if left.positions.len() != right.positions.len()
        || left.on_caller_follow != right.on_caller_follow
    {
        return false;
    }
    left.positions
        .iter()
        .zip(&right.positions)
        .all(|(left, right)| {
            !left
                .kinds
                .intersection(&right.kinds)
                .expect("decision positions share the grammar kind width")
                .is_empty()
                && crate::analysis::predicates_overlap(
                    left.text_predicate,
                    right.text_predicate,
                    &grammar.text_predicates,
                    grammar.text_equivalence,
                )
        })
}

/// Sorts FIRST positions canonically and removes equivalent predicates.
fn sort_dedup_positions(positions: &mut Vec<FirstPosition>) {
    positions.sort_by(compare_positions);
    positions.dedup();
}

/// Orders FIRST positions by kind and exact-text predicate.
fn compare_positions(left: &FirstPosition, right: &FirstPosition) -> Ordering {
    left.kinds
        .kind_count()
        .cmp(&right.kinds.kind_count())
        .then_with(|| left.kinds.words().cmp(right.kinds.words()))
        .then_with(|| left.text_predicate.cmp(&right.text_predicate))
}

/// Renders one bounded lookahead sequence for diagnostics and projections.
fn render_sequence(sequence: &DispatchSequence, grammar: &GrammarDescription) -> String {
    let mut output = sequence
        .positions
        .iter()
        .map(|position| render_position(position, grammar))
        .collect::<Vec<_>>()
        .join(" ");
    if sequence.on_caller_follow {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str("<caller-follow>");
    }
    if output.is_empty() {
        output.push_str("<empty>");
    }
    output
}

/// Renders the remaining FIRST positions after a shared prefix.
fn render_positions_suffix(positions: &[FirstPosition], grammar: &GrammarDescription) -> String {
    if positions.is_empty() {
        String::new()
    } else {
        format!(
            " [{}]",
            positions
                .iter()
                .map(|position| render_position(position, grammar))
                .collect::<Vec<_>>()
                .join(" | ")
        )
    }
}

/// Renders one token-kind and exact-text predicate.
fn render_position(position: &FirstPosition, grammar: &GrammarDescription) -> String {
    let kinds = position
        .kinds
        .iter()
        .map(|kind| grammar.kinds[kind.index()].name.as_str())
        .collect::<Vec<_>>()
        .join("|");
    position.text_predicate.map_or(kinds.clone(), |predicate| {
        format!("{kinds}@{:?}", grammar.text_predicates[predicate.index()])
    })
}

/// Derives the canonical dispatch name recorded in the immutable decision plan.
const fn dispatch_name(kind: DispatchKind) -> &'static str {
    match kind {
        DispatchKind::Enum => "enum",
        DispatchKind::PrattNud => "pratt-nud",
        DispatchKind::PrattLed => "pratt-led",
    }
}

/// Derives the canonical follow name recorded in the immutable decision plan.
const fn follow_name(recipe: &FollowRecipe) -> &'static str {
    match recipe {
        FollowRecipe::Caller => "caller",
        FollowRecipe::Static { .. } => "static",
        FollowRecipe::StaticAndCaller { .. } => "static+caller",
    }
}

/// Reports whether this Pratt role begins an expression without a left operand.
const fn is_nud(position: PrattPosition) -> bool {
    matches!(position, PrattPosition::Atom | PrattPosition::Prefix { .. })
}

/// Reports whether this Pratt role extends an already parsed left operand.
const fn is_led(position: PrattPosition) -> bool {
    matches!(
        position,
        PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AtomDescription, AtomExpression, ContainerShape, Declaration, KindDescription, Location,
        ParseSite, PrattVariant, RepeatSeparator, RepetitionCardinality, SourceId, TextEquivalence,
        TypeDescription,
    };

    fn declaration(grammar: &mut GrammarDescription, name: &str, byte: u32) -> DeclId {
        let location = grammar.push_location(Location::new(SourceId::new(0), byte..byte + 1));
        grammar.push_declaration(Declaration::new(name, location))
    }

    fn token(grammar: &mut GrammarDescription, name: &str, byte: u32) -> TypeId {
        let declaration = declaration(grammar, name, byte);
        let kind = grammar.push_kind(KindDescription::new(name, declaration));
        let atom = grammar.push_atom(AtomDescription::new(
            name,
            declaration,
            AtomExpression::Kind(kind),
        ));
        grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom,
                text_predicate: None,
            },
        ))
    }

    fn pratt_and_repeat_grammar() -> GrammarDescription {
        let mut grammar =
            GrammarDescription::new("decision-invariants", 4, 2, TextEquivalence::Sensitive);
        let number = token(&mut grammar, "NUMBER", 0);
        let minus = token(&mut grammar, "MINUS", 2);
        let plus = token(&mut grammar, "PLUS", 4);
        let comma = token(&mut grammar, "COMMA", 6);

        let expr_declaration = declaration(&mut grammar, "Expr", 10);
        let expr = grammar.next_type_id();
        let atom_declaration = declaration(&mut grammar, "Expr::Atom", 11);
        let atom_site = grammar.push_site(ParseSite::new(
            expr,
            number,
            atom_declaration,
            ContainerShape::Direct,
        ));
        let prefix_declaration = declaration(&mut grammar, "Expr::Neg", 12);
        let prefix_operator = grammar.push_site(ParseSite::new(
            expr,
            minus,
            prefix_declaration,
            ContainerShape::Direct,
        ));
        let prefix_operand_declaration = declaration(&mut grammar, "Expr::Neg::operand", 13);
        let prefix_operand = grammar.push_site(
            ParseSite::new(
                expr,
                expr,
                prefix_operand_declaration,
                ContainerShape::boxed(ContainerShape::Direct),
            )
            .with_min_bp(30),
        );
        let infix_declaration = declaration(&mut grammar, "Expr::Add", 14);
        let infix_operator = grammar.push_site(ParseSite::new(
            expr,
            plus,
            infix_declaration,
            ContainerShape::Direct,
        ));
        let infix_rhs_declaration = declaration(&mut grammar, "Expr::Add::rhs", 15);
        let infix_rhs = grammar.push_site(
            ParseSite::new(
                expr,
                expr,
                infix_rhs_declaration,
                ContainerShape::boxed(ContainerShape::Direct),
            )
            .with_min_bp(11),
        );
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Expr",
                expr_declaration,
                TypeShape::Pratt {
                    variants: vec![
                        PrattVariant {
                            declaration: atom_declaration,
                            position: PrattPosition::Atom,
                            sites: vec![atom_site],
                        },
                        PrattVariant {
                            declaration: prefix_declaration,
                            position: PrattPosition::Prefix { bp: 30 },
                            sites: vec![prefix_operator, prefix_operand],
                        },
                        PrattVariant {
                            declaration: infix_declaration,
                            position: PrattPosition::Infix { lbp: 10, rbp: 11 },
                            sites: vec![infix_operator, infix_rhs],
                        },
                    ],
                },
            )),
            expr,
        );

        let list_declaration = declaration(&mut grammar, "List", 20);
        let list = grammar.next_type_id();
        let items_declaration = declaration(&mut grammar, "List::items", 21);
        let items = grammar.push_site(ParseSite::new(
            list,
            number,
            items_declaration,
            ContainerShape::repeated_with_separator(
                ContainerShape::Direct,
                RepetitionCardinality::OneOrMore,
                Some(RepeatSeparator::new(comma, true)),
            ),
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "List",
                list_declaration,
                TypeShape::Sequence { sites: vec![items] },
            )),
            list,
        );
        grammar.entry_types = vec![expr, list];
        grammar
    }

    #[test]
    fn products_retain_pratt_and_container_execution_and_reject_mutations() {
        let grammar = pratt_and_repeat_grammar();
        let report = crate::analyze(&grammar);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let output = report.output.unwrap();
        let products = output.decisions().clone();
        assert_eq!(products.validate(&grammar, output.first_by_type()), Ok(()));

        let nud = products
            .dispatches
            .iter()
            .find(|dispatch| dispatch.kind == DispatchKind::PrattNud)
            .unwrap();
        assert_eq!(
            nud.branches
                .iter()
                .map(|branch| branch.position)
                .collect::<Vec<_>>(),
            [
                Some(PrattPosition::Atom),
                Some(PrattPosition::Prefix { bp: 30 }),
            ],
        );
        let led = products
            .dispatches
            .iter()
            .find(|dispatch| dispatch.kind == DispatchKind::PrattLed)
            .unwrap();
        assert_eq!(
            led.branches[0].position,
            Some(PrattPosition::Infix { lbp: 10, rbp: 11 }),
        );
        assert!(
            !products
                .failure_edges
                .iter()
                .any(|edge| edge.kind == FailureEdgeKind::PrattLedDispatch)
        );
        assert!(products.failure_edges.iter().any(|edge| {
            edge.kind == FailureEdgeKind::PrattCallerFollowGuard
                && matches!(
                    edge.expected,
                    ExpectedRecipe::PrattExtendersAtOrAboveMinBp { .. }
                )
        }));
        let repeat = products
            .container_machines
            .iter()
            .find(|machine| machine.operation == ContainerOperation::OneOrMore)
            .unwrap();
        assert!(repeat.optional_trailing());
        assert!(products.failure_edges.iter().any(|edge| {
            edge.kind == FailureEdgeKind::ContainerAfterSeparator
                && matches!(edge.expected, ExpectedRecipe::ContinueOrStop { .. })
        }));

        let mut missing_dispatch = products.clone();
        missing_dispatch.dispatches.remove(0);
        assert_eq!(
            missing_dispatch.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::MissingDispatch),
        );

        let mut overlap = products.clone();
        let nud = overlap
            .dispatches
            .iter_mut()
            .find(|dispatch| dispatch.kind == DispatchKind::PrattNud)
            .unwrap();
        nud.branches[1].sequences = nud.branches[0].sequences.clone();
        assert_eq!(
            overlap.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::OverlappingDispatch),
        );

        let mut empty_expected = products.clone();
        let edge = empty_expected
            .failure_edges
            .iter_mut()
            .find(|edge| matches!(edge.expected, ExpectedRecipe::Static(_)))
            .unwrap();
        edge.expected = ExpectedRecipe::Static(Vec::new());
        assert_eq!(
            empty_expected.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::EmptyExpectedRecipe),
        );

        let mut missing_guard = products.clone();
        missing_guard
            .failure_edges
            .retain(|edge| edge.kind != FailureEdgeKind::PrattCallerFollowGuard);
        assert_eq!(
            missing_guard.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::MissingPrattLedGuard),
        );

        let mut wrong_follow = products.clone();
        wrong_follow.follow_recipes_by_site[0] = FollowRecipe::Static {
            first: products.container_machines[0].element_first.clone(),
        };
        if wrong_follow.follow_recipes_by_site[0] == products.follow_recipes_by_site[0] {
            wrong_follow.follow_recipes_by_site[0] = FollowRecipe::Caller;
        }
        assert_eq!(
            wrong_follow.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::FollowRecipeMismatch),
        );

        let mut wrong_depth = products.clone();
        wrong_depth.dispatches[0].required_lookahead += 1;
        assert_eq!(
            wrong_depth.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::DispatchProductMismatch),
        );

        let mut wrong_trie = products.clone();
        wrong_trie.dispatches[0].trie.edges.pop();
        assert_eq!(
            wrong_trie.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::DispatchProductMismatch),
        );

        let mut wrong_pratt_position = products.clone();
        wrong_pratt_position
            .dispatches
            .iter_mut()
            .find(|dispatch| dispatch.kind == DispatchKind::PrattLed)
            .unwrap()
            .branches[0]
            .position = Some(PrattPosition::Postfix { bp: 10 });
        assert_eq!(
            wrong_pratt_position.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::DispatchProductMismatch),
        );

        let mut wrong_container = products.clone();
        let repeat = wrong_container
            .container_machines
            .iter_mut()
            .find(|machine| machine.operation == ContainerOperation::OneOrMore)
            .unwrap();
        repeat.separator = None;
        assert_eq!(
            wrong_container.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::ContainerProductMismatch),
        );

        let mut wrong_transitions = products.clone();
        wrong_transitions
            .container_machines
            .iter_mut()
            .find(|machine| machine.operation == ContainerOperation::OneOrMore)
            .unwrap()
            .transitions
            .pop();
        assert_eq!(
            wrong_transitions.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::ContainerProductMismatch),
        );

        let mut wrong_failure = products;
        let required = wrong_failure
            .failure_edges
            .iter_mut()
            .find(|edge| edge.kind == FailureEdgeKind::RequiredSite)
            .unwrap();
        required.expected = ExpectedRecipe::Follow(FollowRecipe::Caller);
        assert_eq!(
            wrong_failure.validate(&grammar, output.first_by_type()),
            Err(DecisionInvariant::FailureProductMismatch),
        );
    }
}
