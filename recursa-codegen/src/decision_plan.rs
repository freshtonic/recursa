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
use std::sync::Arc;

use crate::analysis::{FirstFacts, FirstSequenceAnalysis};
use crate::ir::RepeatSeparator;
use crate::text_partition::{TextPartition, overlay_by_kind_residual, partition_by_kind_residual};
use crate::{
    DeclId, FirstPosition, FirstSequence, GrammarDescription, KindId, KindSet, PrattPosition,
    SiteId, TextPredicate, TextPredicateId, TypeId, TypeShape,
};

#[cfg(test)]
std::thread_local! {
    /// Counts independent decision validations that reconstruct FIRST-language support.
    static INDEPENDENT_DECISION_VALIDATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Counts complete decision-plan constructions on the current test thread.
    static DECISION_PLAN_CONSTRUCTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Counts optional sites whose static FIRST overlap requires symbolic viability traversal.
    static OVERLAPPING_OPTIONAL_VIABILITY_CONSTRUCTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
/// Resets the per-test count of independent decision validations.
pub(crate) fn reset_independent_decision_validations_for_test() {
    INDEPENDENT_DECISION_VALIDATIONS.set(0);
}

#[cfg(test)]
/// Returns the per-test count of independent decision validations.
pub(crate) fn independent_decision_validations_for_test() -> usize {
    INDEPENDENT_DECISION_VALIDATIONS.get()
}

#[cfg(test)]
/// Resets the per-test count of complete decision-plan constructions.
pub(crate) fn reset_decision_plan_constructions_for_test() {
    DECISION_PLAN_CONSTRUCTIONS.set(0);
}

#[cfg(test)]
/// Returns the per-test count of complete decision-plan constructions.
pub(crate) fn decision_plan_constructions_for_test() -> usize {
    DECISION_PLAN_CONSTRUCTIONS.get()
}

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
    /// Exact owned positions consumed when generation replays this decision.
    positions: Vec<DispatchPosition>,
    /// Supplies the on caller follow consumed when generation replays this decision.
    on_caller_follow: bool,
}

impl DispatchSequence {
    /// Returns the ordered token positions in this prefix.
    pub fn positions(&self) -> &[DispatchPosition] {
        &self.positions
    }

    /// Returns whether exhaustion selects through the caller's FOLLOW set.
    pub const fn on_caller_follow(&self) -> bool {
        self.on_caller_follow
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Canonical all-but-one-coordinate key for exact owned dispatch rectangles.
struct DispatchRectangleMergeKey {
    /// Whether the represented prefix ends on caller FOLLOW.
    on_caller_follow: bool,
    /// Exact owned predicate retained at the coordinate whose kinds are merged.
    merged_text: TextPartition,
    /// Every non-merged position as packed kinds and an exact owned predicate.
    positions: Vec<(Vec<u64>, TextPartition)>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
/// Closed selection policy for a frozen dispatch terminal.
pub enum DispatchTerminalMode {
    /// Select immediately without considering outgoing edges.
    Immediate,
    /// Select only when lookahead belongs to the caller FOLLOW set.
    CallerFollow,
    /// Try every exact outgoing edge first, then select when none matches.
    FallbackAfterEdges,
}

/// One terminal choice in a frozen dispatch trie.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DispatchTerminal {
    /// Declaration selected at this terminal.
    declaration: DeclId,
    /// Closed selection policy for this terminal.
    mode: DispatchTerminalMode,
}

impl DispatchTerminal {
    /// Identifies the declaration selected when this trie terminal is reached.
    pub const fn declaration(self) -> DeclId {
        self.declaration
    }

    /// Returns the closed selection policy for this terminal.
    pub const fn mode(self) -> DispatchTerminalMode {
        self.mode
    }

    /// Returns whether selection depends on caller FOLLOW.
    pub const fn on_caller_follow(self) -> bool {
        matches!(self.mode, DispatchTerminalMode::CallerFollow)
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
///
/// Ordinary enum paths retain complete alternative and caller-FOLLOW
/// validation before exposing a terminal. An enum path that has actually
/// crossed a Pratt NUD may commit after a non-empty exact prefix leaves one
/// outer declaration; that declaration's parser validates the remaining
/// syntax. Pratt dispatches apply the same declaration-selection cutoff.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DispatchTrieNode {
    /// Optional decision when the consumed prefix ends here.
    terminal: Option<DispatchTerminal>,
    /// Already-discriminated outgoing token predicates.
    edges: Arc<[DispatchTrieEdge]>,
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

    /// Returns exact bounded prefixes that select this branch.
    ///
    /// Structural-only balanced-opener routes are represented by
    /// [`DispatchPlan::balanced`] and therefore do not appear as ordinary
    /// branch sequences. A [`DispatchTerminalMode::FallbackAfterEdges`]
    /// terminal selects the complement of its outgoing predicates, which is
    /// not an exact positive prefix and is therefore omitted here; consult
    /// [`DispatchPlan::trie`] for that authoritative decision.
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
///
/// Ordinary enum paths preserve complete-route and caller-FOLLOW selection.
/// Pratt dispatches, and enum derivations that have actually crossed a Pratt
/// NUD, may commit after a non-empty exact prefix uniquely identifies one
/// declaration. The selected declaration remains responsible for validating
/// residual syntax and producing any deeper parse error.
pub struct DispatchPlan {
    /// Supplies the owner consumed when generation replays this decision.
    owner: TypeId,
    /// Supplies the kind consumed when generation replays this decision.
    kind: DispatchKind,
    /// Supplies the branches consumed when generation replays this decision.
    branches: Vec<DispatchBranch>,
    /// Supplies the lookahead needed to select a declaration, not to validate its complete syntax.
    required_lookahead: usize,
    /// Supplies the on caller follow consumed when generation replays this decision.
    on_caller_follow: Option<DeclId>,
    /// Supplies the trie consumed when generation replays this decision.
    trie: DispatchTrieNode,
    /// Optional balanced-delimiter cut used before the bounded trie.
    balanced: Option<BalancedDispatch>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One structurally matched delimiter region followed by an ordinary residual dispatch.
pub struct BalancedDispatch {
    /// Fixed opening kind that starts the nesting scan.
    open: KindId,
    /// Fixed closing kind that ends the scan at nesting depth zero.
    close: KindId,
    /// Frozen branch selection beginning immediately after the matching close.
    after_close: DispatchTrieNode,
}

impl BalancedDispatch {
    /// Returns the fixed opening token kind.
    pub const fn open(&self) -> KindId {
        self.open
    }

    /// Returns the fixed closing token kind.
    pub const fn close(&self) -> KindId {
        self.close
    }

    /// Returns the frozen residual dispatch evaluated after the matching close.
    pub const fn after_close(&self) -> &DispatchTrieNode {
        &self.after_close
    }
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

    /// Reports the bounded lookahead depth proven sufficient for declaration selection.
    ///
    /// Ordinary enum paths include the complete route and caller-FOLLOW
    /// decision. Pratt dispatches and Pratt-crossed enum paths stop once a
    /// non-empty exact prefix leaves one declaration, even when its parser
    /// must inspect additional syntax.
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

    /// Returns the structural delimiter strategy used for an unresolved opener edge.
    pub const fn balanced(&self) -> Option<&BalancedDispatch> {
        self.balanced.as_ref()
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
        if dispatch_trie_has_fallback_after_edges(&self.trie) {
            output.push_str("\n  fallback-trie=");
            render_dispatch_trie(&self.trie, grammar, &mut output);
        }
        if let Some(balanced) = &self.balanced {
            write!(
                output,
                "\n  balanced open={} close={} after-close=",
                grammar.kinds[balanced.open.index()].name,
                grammar.kinds[balanced.close.index()].name,
            )
            .expect("writing to a String cannot fail");
            render_dispatch_trie(&balanced.after_close, grammar, &mut output);
        }
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

/// Reports whether a matched dispatch prefix keeps walking although one declaration decides it.
///
/// Every terminal below `node` naming one declaration means the prefix that reached `node`
/// has selected that declaration; an edge there re-validates the declaration's own language
/// with a frontier that omits its open continuations, which is how `f(a + b)` failed at `+`
/// (#127). The root is exempt because it has matched nothing yet.
///
/// The reachable declarations are recollected at every node rather than memoized, which is
/// quadratic in trie size; the tries are small and this runs once per grammar at build time.
fn dispatch_trie_walks_past_selection(node: &DispatchTrieNode) -> bool {
    fn declarations(node: &DispatchTrieNode, found: &mut Vec<DeclId>) {
        if let Some(terminal) = node.terminal {
            found.push(terminal.declaration);
        }
        for edge in node.edges.iter() {
            declarations(&edge.next, found);
        }
    }
    if node.edges.is_empty() {
        return false;
    }
    let mut found = Vec::new();
    declarations(node, &mut found);
    found.sort_unstable();
    found.dedup();
    if found.len() == 1 {
        return true;
    }
    node.edges
        .iter()
        .any(|edge| dispatch_trie_walks_past_selection(&edge.next))
}

/// Reports whether a trie contains a complement terminal that cannot be projected as a row.
fn dispatch_trie_has_fallback_after_edges(node: &DispatchTrieNode) -> bool {
    node.terminal
        .is_some_and(|terminal| terminal.mode == DispatchTerminalMode::FallbackAfterEdges)
        || node
            .edges
            .iter()
            .any(|edge| dispatch_trie_has_fallback_after_edges(&edge.next))
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
/// Action selected by frozen optional consume-vs-skip viability.
pub enum OptionalAction {
    /// Parse and store one occurrence of the optional site.
    Consume,
    /// Leave the optional site absent and continue with its suffix.
    Skip,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One terminal action in a frozen optional viability decision trie.
pub struct OptionalViabilityDecisionTerminal {
    /// Consume or skip action selected at this terminal.
    action: OptionalAction,
    /// Whether the action is available only through the owning parser's caller FOLLOW.
    on_caller_follow: bool,
}

impl OptionalViabilityDecisionTerminal {
    /// Returns the container action selected by this terminal.
    pub const fn action(self) -> OptionalAction {
        self.action
    }

    /// Returns whether selection depends on caller FOLLOW at the current lookahead position.
    pub const fn on_caller_follow(self) -> bool {
        self.on_caller_follow
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One exact token/text partition in a frozen optional viability decision trie.
pub struct OptionalViabilityDecisionEdge {
    /// Token predicate labeling this edge.
    position: DispatchPosition,
    /// Trie node reached after matching this position.
    next: OptionalViabilityDecisionNode,
}

impl OptionalViabilityDecisionEdge {
    /// Returns the token predicate labeling this edge.
    pub const fn position(&self) -> &DispatchPosition {
        &self.position
    }

    /// Returns the node reached after matching the edge.
    pub const fn next(&self) -> &OptionalViabilityDecisionNode {
        &self.next
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// One sparse state in a frozen optional consume-vs-skip viability trie.
///
/// Retained terminals and edges override or extend the root kind selector. An absent action
/// delegates to that selector rather than rejecting the token, so disjoint paths need not be
/// repeated in every state.
pub struct OptionalViabilityDecisionNode {
    /// Ordered unconditional and caller-FOLLOW actions available at this prefix.
    terminals: Arc<[OptionalViabilityDecisionTerminal]>,
    /// Pairwise-disjoint token/text partitions extending this prefix.
    edges: Arc<[OptionalViabilityDecisionEdge]>,
}

impl OptionalViabilityDecisionNode {
    /// Returns the terminal actions available when traversal stops at this prefix.
    pub fn terminals(&self) -> &[OptionalViabilityDecisionTerminal] {
        &self.terminals
    }

    /// Returns the outgoing exact token/text partitions.
    pub fn edges(&self) -> &[OptionalViabilityDecisionEdge] {
        &self.edges
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete source-independent viability decision for one optional parse site.
///
/// The consuming language expands one occurrence followed by the owning sequence tail. The
/// skipping language starts at that tail. A terminal may depend on caller FOLLOW when the tail
/// can end. The trie stores only paths that differ from, or must defer beyond, the root kind
/// selector; a missing action delegates to that selector. Unresolved invalid input consequently
/// preserves the established parser-path failure diagnostic.
pub struct OptionalViabilityDecision {
    /// Root of the immutable consume-vs-skip trie.
    trie: OptionalViabilityDecisionNode,
    /// Bounded lookahead depth needed to reach every unambiguous action.
    required_lookahead: usize,
    /// Caller-FOLLOW kinds this decision provably separates by element suffix.
    ///
    /// Non-empty only for suffix-proving decisions: the planner proved that
    /// every one of these overlapping kinds is decided by matching the
    /// element's own closed prefix language rather than by first-token
    /// commitment. Diagnostics consume this proof to retire the corresponding
    /// nullable-FOLLOW warnings; an empty set claims nothing.
    separated_caller_kinds: KindSet,
}

impl OptionalViabilityDecision {
    /// Returns the immutable consume-vs-skip trie.
    pub const fn trie(&self) -> &OptionalViabilityDecisionNode {
        &self.trie
    }

    /// Returns the bounded lookahead depth required by this optional viability decision.
    pub const fn required_lookahead(&self) -> usize {
        self.required_lookahead
    }

    /// Returns the caller-FOLLOW kinds provably separated by element suffix.
    pub const fn separated_caller_kinds(&self) -> &KindSet {
        &self.separated_caller_kinds
    }
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
    /// Supplies sparse consume-vs-skip overrides when the legacy selector is insufficient.
    ///
    /// `None` is valid only when exact static FOLLOW is disjoint from an unrefined element FIRST,
    /// so the existing kind selector completely determines consume versus skip.
    optional_viability_decision: Option<OptionalViabilityDecision>,
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

    /// Returns sparse consume-vs-skip overrides when this optional cannot use its kind selector.
    ///
    /// `None` means analysis proved the element selector complete for this site; non-optional
    /// machines also have no optional viability decision.
    pub const fn optional_viability_decision(&self) -> Option<&OptionalViabilityDecision> {
        self.optional_viability_decision.as_ref()
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
    /// Boundary kinds a matching `#[greedy(...)]` acceptance lets a Pratt loop extend on
    /// at each dense parse-site ID, empty at every site without an acceptance.
    greedy_pratt_boundary_by_site: Vec<KindSet>,
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

    /// Returns the accepted Pratt extender kinds indexed by dense parse-site ID.
    ///
    /// A site whose declaration carries no matching `#[greedy(...)]` acceptance has an
    /// empty set, and the whole table is empty for a grammar that declares no acceptance.
    pub fn greedy_pratt_boundary_by_site(&self) -> &[KindSet] {
        &self.greedy_pratt_boundary_by_site
    }

    /// Records the accepted Pratt extender kinds derived from the advisory FOLLOW overlaps.
    pub(crate) fn set_greedy_pratt_boundary_by_site(&mut self, accepted: Vec<KindSet>) {
        self.greedy_pratt_boundary_by_site = if accepted.iter().all(KindSet::is_empty) {
            Vec::new()
        } else {
            accepted
        };
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
        for machine in &self.container_machines {
            let Some(decision) = &machine.optional_viability_decision else {
                continue;
            };
            let declaration =
                &grammar.declarations[grammar.sites[machine.site.index()].declaration.index()].name;
            write!(
                output,
                "optional-viability {declaration} depth={} trie=",
                decision.required_lookahead,
            )
            .expect("writing to a String cannot fail");
            render_optional_viability_decision_trie(&decision.trie, grammar, &mut output);
            output.push('\n');
        }
        output
    }

    #[cfg(test)]
    /// Independently validates every decision against the grammar and computed FIRST facts.
    pub(crate) fn validate(
        &self,
        grammar: &GrammarDescription,
        semantic: &[FirstFacts],
    ) -> Result<(), DecisionInvariant> {
        INDEPENDENT_DECISION_VALIDATIONS
            .set(INDEPENDENT_DECISION_VALIDATIONS.get().saturating_add(1));
        let first_sequences = FirstSequenceAnalysis::for_grammar(grammar);
        let caller_first_by_site = crate::analysis::caller_first_by_site_for_replay(grammar);
        self.validate_with_first_sequences(
            grammar,
            semantic,
            &first_sequences,
            &caller_first_by_site,
        )
    }

    /// Validates cross-table identities that are independent of decision reconstruction.
    fn validate_structure(&self, grammar: &GrammarDescription) -> Result<(), DecisionInvariant> {
        // Validate every cross-table identity before generation can trust this frozen product as executable authority.
        if self.follow_recipes_by_site.len() != grammar.sites.len()
            || self.container_machines.len() != grammar.sites.len()
        {
            return Err(DecisionInvariant::MissingSiteProduct);
        }
        if !self.greedy_pratt_boundary_by_site.is_empty()
            && self.greedy_pratt_boundary_by_site.len() != grammar.sites.len()
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
        for machine in &self.container_machines {
            match (
                machine.operation,
                machine.optional_viability_decision.as_ref(),
            ) {
                (ContainerOperation::Optional, Some(decision)) => {
                    if decision.required_lookahead > grammar.max_lookahead
                        || !optional_edges_are_disjoint(&decision.trie, grammar)
                        || !optional_viability_decision_terminals_are_valid(&decision.trie)
                    {
                        return Err(DecisionInvariant::InvalidOptionalViabilityDecision);
                    }
                }
                (_, Some(_)) => {
                    return Err(DecisionInvariant::InvalidOptionalViabilityDecision);
                }
                (ContainerOperation::Optional, None)
                    if optional_legacy_selector_is_complete(machine, grammar) => {}
                (ContainerOperation::Optional, None) => {
                    return Err(DecisionInvariant::InvalidOptionalViabilityDecision);
                }
                (_, None) => {}
            }
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
        for dispatch in &self.dispatches {
            if dispatch.kind != DispatchKind::Enum {
                continue;
            }
            let after_close = dispatch
                .balanced
                .as_ref()
                .map(|balanced| &balanced.after_close);
            if dispatch
                .trie
                .edges
                .iter()
                .chain(after_close.iter().flat_map(|node| node.edges.iter()))
                .any(|edge| dispatch_trie_walks_past_selection(&edge.next))
            {
                return Err(DecisionInvariant::DispatchWalksPastSelection);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    /// Independently validates decisions while reusing supplied FIRST-language support.
    fn validate_with_first_sequences(
        &self,
        grammar: &GrammarDescription,
        semantic: &[FirstFacts],
        first_sequences: &FirstSequenceAnalysis<'_>,
        caller_first_by_site: &[Vec<FirstPosition>],
    ) -> Result<(), DecisionInvariant> {
        self.validate_structure(grammar)?;
        let expected = plan_decisions(grammar, semantic, first_sequences, caller_first_by_site).0;
        if self.follow_recipes_by_site != expected.follow_recipes_by_site {
            return Err(DecisionInvariant::FollowRecipeMismatch);
        }
        if self.dispatches != expected.dispatches {
            for dispatch in &self.dispatches {
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

#[derive(Clone, Debug, Eq, PartialEq)]
/// Decision products returned directly by the private canonical constructor.
pub(crate) struct ConstructedDecisionProducts(DecisionProducts);

impl ConstructedDecisionProducts {
    /// Checks construction-independent identities and yields publishable products.
    pub(crate) fn validate_for_publication(
        self,
        grammar: &GrammarDescription,
    ) -> Result<DecisionProducts, DecisionInvariant> {
        self.0.validate_structure(grammar)?;
        Ok(self.0)
    }

    #[cfg(test)]
    /// Corrupts one site product before the publication-boundary regression check.
    pub(crate) fn corrupt_site_product_for_test(&mut self) {
        self.0.corrupt_site_product_for_test();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the decision invariant alternatives frozen into the decision plan.
pub(crate) enum DecisionInvariant {
    /// Selects the missing site product decision for generation.
    MissingSiteProduct,
    /// Selects the missing dispatch decision for generation.
    MissingDispatch,
    #[cfg(test)]
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
    /// Selects malformed or missing optional consume-vs-skip viability.
    InvalidOptionalViabilityDecision,
    /// Selects the multiple caller fallbacks decision for generation.
    MultipleCallerFallbacks,
    /// Selects the duplicate failure edge decision for generation.
    DuplicateFailureEdge,
    /// Selects an enum dispatch that keeps walking after one declaration is selected.
    DispatchWalksPastSelection,
    #[cfg(test)]
    /// Selects the follow recipe mismatch decision for generation.
    FollowRecipeMismatch,
    #[cfg(test)]
    /// Selects the dispatch product mismatch decision for generation.
    DispatchProductMismatch,
    #[cfg(test)]
    /// Selects the container product mismatch decision for generation.
    ContainerProductMismatch,
    #[cfg(test)]
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
    first_sequences: &FirstSequenceAnalysis<'_>,
    caller_first_by_site: &[Vec<FirstPosition>],
) -> ConstructedDecisionProducts {
    #[cfg(test)]
    DECISION_PLAN_CONSTRUCTIONS.set(DECISION_PLAN_CONSTRUCTIONS.get().saturating_add(1));
    // An empty slice deliberately disables caller-overlap planning (test replays);
    // any other length mismatch would silently disable it site-by-site instead.
    debug_assert!(
        caller_first_by_site.is_empty() || caller_first_by_site.len() == grammar.sites.len(),
        "caller FOLLOW positions must align with grammar sites",
    );
    // Build dispatch, container, Pratt, and failure products from the same semantic snapshot to prevent phase drift.
    let mut follow_recipes_by_site = vec![FollowRecipe::Caller; grammar.sites.len()];
    let mut dispatches = Vec::new();
    for (index, description) in grammar.types.iter().enumerate() {
        let owner = TypeId::new(index as u32);
        match &description.shape {
            TypeShape::Alias { site } => {
                plan_follow_for_sites(&[*site], first_sequences, &mut follow_recipes_by_site);
            }
            TypeShape::Sequence { sites } => {
                plan_follow_for_sites(sites, first_sequences, &mut follow_recipes_by_site);
            }
            TypeShape::Enum { alternatives } => {
                for alternative in alternatives {
                    plan_follow_for_sites(
                        &alternative.sites,
                        first_sequences,
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
                    first_sequences,
                ));
            }
            TypeShape::Pratt { variants } => {
                for variant in variants {
                    plan_follow_for_sites(
                        &variant.sites,
                        first_sequences,
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
                    first_sequences,
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
                        first_sequences,
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

    let mut optional_viability_decisions_by_site = vec![None; grammar.sites.len()];
    let mut overlapping_optional_sites = Vec::new();
    for index in 0..grammar.sites.len() {
        let site_id = SiteId::new(index as u32);
        if grammar.sites[index]
            .container
            .semantics()
            .is_some_and(|semantics| {
                semantics.cardinality == crate::ir::ContainerCardinality::Optional
            })
        {
            plan_nonoverlapping_optional_viability_for_site(
                site_id,
                grammar,
                semantic,
                first_sequences,
                caller_first_by_site.get(index).map_or(&[], Vec::as_slice),
                &follow_recipes_by_site,
                &mut optional_viability_decisions_by_site,
                &mut overlapping_optional_sites,
            );
        }
    }
    for (site_id, resolution) in
        first_sequences.compact_optional_viabilities(&overlapping_optional_sites)
    {
        optional_viability_decisions_by_site[site_id.index()] = Some(OptionalViabilityDecision {
            trie: optional_viability_decision_from_compact(&resolution.root),
            required_lookahead: resolution.required_lookahead,
            separated_caller_kinds: KindSet::empty(grammar.kind_count),
        });
    }

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
                optional_viability_decision: optional_viability_decisions_by_site[index].clone(),
                transitions,
            }
        })
        .collect::<Vec<_>>();
    let failure_edges = plan_failure_edges(grammar, semantic, &dispatches, &follow_recipes_by_site);
    ConstructedDecisionProducts(DecisionProducts {
        dispatches,
        follow_recipes_by_site,
        container_machines,
        failure_edges,
        greedy_pratt_boundary_by_site: Vec::new(),
    })
}

/// Elides selector-complete optionals, freezes refined selectors, and queues exact static overlaps.
///
/// Elision is valid only for unrefined element FIRST positions disjoint from exact static FOLLOW
/// and from caller FOLLOW. A caller-FOLLOW-only overlap freezes a suffix-proving decision:
/// the caller FOLLOW carries no continuation past its first token, so viability is proven by
/// matching the input against the element's own bounded prefix language instead.
/// Generated parsing then uses the existing root kind selector for both consume and skip.
#[allow(clippy::too_many_arguments)]
fn plan_nonoverlapping_optional_viability_for_site(
    site_id: SiteId,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_sequences: &FirstSequenceAnalysis<'_>,
    caller_first: &[FirstPosition],
    follow_recipes: &[FollowRecipe],
    decisions: &mut [Option<OptionalViabilityDecision>],
    overlapping_sites: &mut Vec<(SiteId, KindSet)>,
) {
    let site = &grammar.sites[site_id.index()];
    let element_first = &semantic[site.target.index()].positions;
    let stop = &follow_recipes[site_id.index()];
    let static_overlap = first_positions_overlap(element_first, stop.static_first(), grammar);
    if static_overlap {
        #[cfg(test)]
        OVERLAPPING_OPTIONAL_VIABILITY_CONSTRUCTIONS.set(
            OVERLAPPING_OPTIONAL_VIABILITY_CONSTRUCTIONS
                .get()
                .saturating_add(1),
        );
        let mut selector = KindSet::empty(grammar.kind_count);
        for position in element_first {
            selector.union_assign(&position.kinds);
        }
        overlapping_sites.push((site_id, selector));
        return;
    }
    if stop.uses_caller() && first_positions_overlap(element_first, caller_first, grammar) {
        // Divergence-skip is sound only for a closed element language: every bounded
        // prefix must provably end, so a non-matching continuation cannot belong to an
        // expansion the bounded sequences approximate away (Pratt extenders, cut-off
        // depth). Open element languages keep the greedy legacy selector below; the
        // reachability pre-gate proves openness on the shared symbolic node graph, so
        // only provably closed element languages pay for materialized sequence rows.
        if first_sequences.element_language_ends_within_depth(site_id, grammar.max_lookahead) {
            let consuming =
                first_sequences.element_sequences_at_depth(site_id, grammar.max_lookahead);
            debug_assert!(!consuming.is_empty() && consuming.iter().all(FirstSequence::ends));
            let mut skipping = stop
                .static_first()
                .iter()
                .cloned()
                .map(FirstSequence::from_position)
                .collect::<Vec<_>>();
            skipping.push(FirstSequence::caller_follow());
            let trie = freeze_optional_viability_decision(&consuming, &skipping, grammar, true);
            let required_lookahead =
                optional_viability_decision_lookahead(&trie).min(grammar.max_lookahead);
            // A kind is provably separated only when the frozen trie defers past
            // it: every root edge covering the kind leads to a node without a
            // depth-one Consume terminal, so commitment requires the element's
            // own deeper suffix. An element that can fully end on the shared
            // token keeps the greedy inherent-ambiguity commitment and keeps
            // its warning.
            let mut separated_caller_kinds = KindSet::empty(grammar.kind_count);
            for element in element_first {
                for caller in caller_first {
                    if crate::analysis::predicates_overlap(
                        element.text_predicate,
                        caller.text_predicate,
                        &grammar.text_predicates,
                        grammar.text_equivalence,
                    ) && let Ok(shared) = element.kinds.intersection(&caller.kinds)
                    {
                        separated_caller_kinds.union_assign(&shared);
                    }
                }
            }
            for edge in trie.edges() {
                let concedes_at_depth_one = edge
                    .next()
                    .terminals()
                    .iter()
                    .any(|terminal| terminal.action() == OptionalAction::Consume);
                if concedes_at_depth_one
                    && let Ok(unproven) = separated_caller_kinds.difference(&edge.position().kinds)
                {
                    separated_caller_kinds = unproven;
                }
            }
            decisions[site_id.index()] = Some(OptionalViabilityDecision {
                trie,
                required_lookahead,
                separated_caller_kinds,
            });
            return;
        }
    }
    let element_is_kind_only = element_first.iter().all(|position| {
        matches!(
            TextPartition::from_predicate(
                position
                    .text_predicate
                    .map(|predicate| &grammar.text_predicates[predicate.index()]),
            ),
            TextPartition::Any,
        )
    });
    if element_is_kind_only {
        return;
    }

    let consuming = element_first
        .iter()
        .cloned()
        .map(FirstSequence::from_position)
        .collect::<Vec<_>>();
    let mut skipping = stop
        .static_first()
        .iter()
        .cloned()
        .map(FirstSequence::from_position)
        .collect::<Vec<_>>();
    if stop.uses_caller() {
        skipping.push(FirstSequence::caller_follow());
    }
    let trie = freeze_optional_viability_decision(&consuming, &skipping, grammar, false);
    let required_lookahead =
        optional_viability_decision_lookahead(&trie).min(grammar.max_lookahead);
    decisions[site_id.index()] = Some(OptionalViabilityDecision {
        trie,
        required_lookahead,
        separated_caller_kinds: KindSet::empty(grammar.kind_count),
    });
}

/// Projects one symbolic action-labelled analysis node into the public viability plan.
fn optional_viability_decision_from_compact(
    node: &crate::analysis::CompactOptionalNode,
) -> OptionalViabilityDecisionNode {
    OptionalViabilityDecisionNode {
        terminals: node
            .terminals
            .iter()
            .map(|terminal| OptionalViabilityDecisionTerminal {
                action: match terminal.action {
                    crate::analysis::CompactOptionalAction::Consume => OptionalAction::Consume,
                    crate::analysis::CompactOptionalAction::Skip => OptionalAction::Skip,
                },
                on_caller_follow: terminal.on_caller_follow,
            })
            .collect(),
        edges: node
            .edges
            .iter()
            .map(|edge| OptionalViabilityDecisionEdge {
                position: DispatchPosition {
                    kinds: edge.kinds.clone(),
                    text_predicate: edge.text.clone().into_predicate(),
                },
                next: optional_viability_decision_from_compact(&edge.next),
            })
            .collect(),
    }
}

/// Reports whether two one-position FIRST languages share an exact kind/text cell.
fn first_positions_overlap(
    left: &[FirstPosition],
    right: &[FirstPosition],
    grammar: &GrammarDescription,
) -> bool {
    left.iter().any(|left| {
        right.iter().any(|right| {
            left.kinds
                .intersects(&right.kinds)
                .expect("optional FIRST positions share the grammar kind width")
                && crate::analysis::predicates_overlap(
                    left.text_predicate,
                    right.text_predicate,
                    &grammar.text_predicates,
                    grammar.text_equivalence,
                )
        })
    })
}

/// Proves that a missing viability trie can delegate completely to the legacy kind selector.
fn optional_legacy_selector_is_complete(
    machine: &ContainerMachine,
    grammar: &GrammarDescription,
) -> bool {
    !first_positions_overlap(&machine.element_first, machine.stop.static_first(), grammar)
        && machine.element_first.iter().all(|position| {
            matches!(
                TextPartition::from_predicate(
                    position
                        .text_predicate
                        .map(|predicate| &grammar.text_predicates[predicate.index()]),
                ),
                TextPartition::Any,
            )
        })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Dense identity of one optional viability decision language suffix.
struct OptionalSuffixId(usize);

impl OptionalSuffixId {
    /// Returns the arena index for this suffix.
    const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug)]
/// One canonical suffix belonging to a consuming or skipping optional path.
struct OptionalSuffix {
    /// Action selected after every remaining position matches.
    terminal: OptionalViabilityDecisionTerminal,
    /// Current symbolic position, absent only at the suffix terminal.
    position: Option<FirstPosition>,
    /// Canonical residual after consuming the current position.
    next: Option<OptionalSuffixId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Structural identity used to hash-cons optional viability decision suffixes.
struct OptionalSuffixKey {
    /// Action and caller-FOLLOW condition selected by this suffix.
    terminal: OptionalViabilityDecisionTerminal,
    /// Packed current token kinds and optional text predicate.
    position: Option<(Vec<u64>, Option<TextPredicateId>)>,
    /// Canonical successor suffix, absent at the terminal.
    next: Option<OptionalSuffixId>,
}

/// Freezes one optional viability decision from exact bounded consuming and skipping languages.
///
/// With `suffix_proving`, the consuming language is the element's own bounded prefix set and the
/// decision proves viability by suffix: a fully matched or depth-exhausted consuming prefix
/// consumes unconditionally, while input that diverges from every consuming prefix skips through
/// a sticky unconditional fallback so the caller keeps tokens the element cannot claim. At a
/// node where one consuming prefix can already end while another continues, Consume outranks
/// the sticky Skip — an early-endable element keeps the legacy greedy commitment rather than
/// skipping input that its longer prefixes no longer match.
fn freeze_optional_viability_decision(
    consuming: &[FirstSequence],
    skipping: &[FirstSequence],
    grammar: &GrammarDescription,
    suffix_proving: bool,
) -> OptionalViabilityDecisionNode {
    let mut suffixes = Vec::<OptionalSuffix>::new();
    let mut suffix_ids = BTreeMap::<OptionalSuffixKey, OptionalSuffixId>::new();
    let mut state = Vec::new();
    for (action, language) in [
        (OptionalAction::Consume, consuming),
        (OptionalAction::Skip, skipping),
    ] {
        for sequence in language {
            let terminal = OptionalViabilityDecisionTerminal {
                action,
                on_caller_follow: sequence.ends()
                    && !(suffix_proving && action == OptionalAction::Consume),
            };
            let terminal_key = OptionalSuffixKey {
                terminal,
                position: None,
                next: None,
            };
            let mut suffix = *suffix_ids.entry(terminal_key).or_insert_with(|| {
                let id = OptionalSuffixId(suffixes.len());
                suffixes.push(OptionalSuffix {
                    terminal,
                    position: None,
                    next: None,
                });
                id
            });
            for position in sequence.positions.iter().rev() {
                let key = OptionalSuffixKey {
                    terminal,
                    position: Some((position.kinds.words().to_vec(), position.text_predicate)),
                    next: Some(suffix),
                };
                suffix = *suffix_ids.entry(key).or_insert_with(|| {
                    let id = OptionalSuffixId(suffixes.len());
                    suffixes.push(OptionalSuffix {
                        terminal,
                        position: Some(position.clone()),
                        next: Some(suffix),
                    });
                    id
                });
            }
            state.push(suffix);
        }
    }
    let sticky = suffix_proving.then(|| {
        let terminal = OptionalViabilityDecisionTerminal {
            action: OptionalAction::Skip,
            on_caller_follow: false,
        };
        let key = OptionalSuffixKey {
            terminal,
            position: None,
            next: None,
        };
        let id = *suffix_ids.entry(key).or_insert_with(|| {
            let id = OptionalSuffixId(suffixes.len());
            suffixes.push(OptionalSuffix {
                terminal,
                position: None,
                next: None,
            });
            id
        });
        state.push(id);
        id
    });
    state.sort_unstable();
    state.dedup();
    let trie = freeze_optional_state(
        &suffixes,
        &state,
        false,
        sticky,
        grammar,
        &mut BTreeMap::new(),
    );
    assert!(optional_edges_are_disjoint(&trie, grammar));
    trie
}

/// Freezes and hash-conses one residual optional viability decision state.
///
/// A `sticky` divergence-fallback suffix propagates into every matched residual so input that
/// leaves the consuming prefix language resolves to its unconditional action instead of
/// delegating back to the greedy root kind selector.
fn freeze_optional_state(
    suffixes: &[OptionalSuffix],
    state: &[OptionalSuffixId],
    matched_prefix: bool,
    sticky: Option<OptionalSuffixId>,
    grammar: &GrammarDescription,
    memo: &mut BTreeMap<
        (bool, Option<OptionalSuffixId>, Vec<OptionalSuffixId>),
        OptionalViabilityDecisionNode,
    >,
) -> OptionalViabilityDecisionNode {
    // `sticky` is constant within one builder invocation, but it joins the memo
    // key so a future shared or hoisted memo cannot conflate tries built under
    // different fallback regimes.
    let memo_key = (matched_prefix, sticky, state.to_vec());
    if let Some(frozen) = memo.get(&memo_key) {
        return frozen.clone();
    }
    let first_action = state
        .first()
        .map(|suffix| suffixes[suffix.index()].terminal.action);
    let has_multiple_actions = first_action.is_some_and(|first| {
        state
            .iter()
            .skip(1)
            .any(|suffix| suffixes[suffix.index()].terminal.action != first)
    });
    if matched_prefix && !has_multiple_actions {
        let frozen = OptionalViabilityDecisionNode {
            terminals: first_action
                .map(|action| {
                    Arc::from([OptionalViabilityDecisionTerminal {
                        action,
                        on_caller_follow: false,
                    }])
                })
                .unwrap_or_default(),
            edges: Arc::default(),
        };
        memo.insert(memo_key, frozen.clone());
        return frozen;
    }

    let raw_terminals = state
        .iter()
        .filter_map(|suffix| {
            let suffix = &suffixes[suffix.index()];
            suffix.position.is_none().then_some(suffix.terminal)
        })
        .collect::<Vec<_>>();
    let terminals = normalize_optional_terminals(&raw_terminals, matched_prefix);
    let candidates = state
        .iter()
        .copied()
        .filter(|suffix| suffixes[suffix.index()].position.is_some())
        .collect::<Vec<_>>();
    let partitions = partition_by_kind_residual(
        &candidates,
        grammar,
        |suffix| {
            suffixes[suffix.index()]
                .position
                .as_ref()
                .expect("optional candidate suffix has a position")
                .text_predicate
        },
        |suffix| {
            &suffixes[suffix.index()]
                .position
                .as_ref()
                .expect("optional candidate suffix has a position")
                .kinds
        },
        |suffix| {
            suffixes[suffix.index()]
                .next
                .expect("optional candidate suffix has a residual")
        },
    );
    let mut edges = partitions
        .into_iter()
        .map(|(text, residual, kinds)| {
            let mut residual = residual;
            if let Some(sticky) = sticky {
                residual.push(sticky);
                residual.sort_unstable();
                residual.dedup();
            }
            OptionalViabilityDecisionEdge {
                position: DispatchPosition {
                    kinds,
                    text_predicate: text.into_predicate(),
                },
                next: freeze_optional_state(suffixes, &residual, true, sticky, grammar, memo),
            }
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.position
            .kinds
            .words()
            .cmp(right.position.kinds.words())
            .then_with(|| {
                TextPartition::from_predicate(left.position.text_predicate.as_ref()).cmp(
                    &TextPartition::from_predicate(right.position.text_predicate.as_ref()),
                )
            })
    });
    let frozen = OptionalViabilityDecisionNode {
        terminals: terminals.into(),
        edges: edges.into(),
    };
    memo.insert(memo_key, frozen.clone());
    frozen
}

/// Normalizes optional terminals while retaining explicit caller-FOLLOW fallback.
fn normalize_optional_terminals(
    terminals: &[OptionalViabilityDecisionTerminal],
    matched_prefix: bool,
) -> Vec<OptionalViabilityDecisionTerminal> {
    let preferred_action = if matched_prefix {
        OptionalAction::Consume
    } else {
        OptionalAction::Skip
    };
    let preferred = |terminals: &[OptionalViabilityDecisionTerminal], caller: bool| {
        let actions = terminals
            .iter()
            .filter(|terminal| caller || !terminal.on_caller_follow)
            .map(|terminal| terminal.action)
            .collect::<Vec<_>>();
        actions
            .contains(&preferred_action)
            .then_some(preferred_action)
            .or_else(|| actions.first().copied())
    };
    let unconditional = preferred(terminals, false);
    let caller = preferred(terminals, true);
    let mut normalized = Vec::new();
    if let Some(action) = unconditional {
        if caller != unconditional
            && let Some(caller_action) = caller
        {
            normalized.push(OptionalViabilityDecisionTerminal {
                action: caller_action,
                on_caller_follow: true,
            });
        }
        normalized.push(OptionalViabilityDecisionTerminal {
            action,
            on_caller_follow: false,
        });
    } else if let Some(action) = caller {
        normalized.push(OptionalViabilityDecisionTerminal {
            action,
            on_caller_follow: true,
        });
    }
    normalized
}

/// Returns the deepest token prefix inspected by one optional viability decision trie.
fn optional_viability_decision_lookahead(root: &OptionalViabilityDecisionNode) -> usize {
    root.edges
        .iter()
        .map(|edge| 1 + optional_viability_decision_lookahead(&edge.next))
        .max()
        .unwrap_or(0)
}

/// Verifies every optional viability decision state uses pairwise-disjoint edge predicates.
fn optional_edges_are_disjoint(
    node: &OptionalViabilityDecisionNode,
    grammar: &GrammarDescription,
) -> bool {
    for (index, left) in node.edges.iter().enumerate() {
        for right in &node.edges[index + 1..] {
            let kinds_overlap = left
                .position
                .kinds
                .intersects(&right.position.kinds)
                .expect("optional decision edges share the grammar width");
            if kinds_overlap
                && TextPartition::from_predicate(left.position.text_predicate.as_ref()).overlaps(
                    &TextPartition::from_predicate(right.position.text_predicate.as_ref()),
                    grammar.text_equivalence,
                )
            {
                return false;
            }
        }
        if !optional_edges_are_disjoint(&left.next, grammar) {
            return false;
        }
    }
    true
}

/// Verifies normalized terminal ordering and uniqueness throughout an optional viability decision.
fn optional_viability_decision_terminals_are_valid(node: &OptionalViabilityDecisionNode) -> bool {
    if node.terminals.len() > 2
        || node.terminals.windows(2).any(|pair| pair[0] == pair[1])
        || node
            .terminals
            .iter()
            .filter(|terminal| terminal.on_caller_follow)
            .count()
            > 1
        || node
            .terminals
            .iter()
            .filter(|terminal| !terminal.on_caller_follow)
            .count()
            > 1
        || node.terminals.len() == 2
            && (!node.terminals[0].on_caller_follow || node.terminals[1].on_caller_follow)
    {
        return false;
    }
    node.edges
        .iter()
        .all(|edge| optional_viability_decision_terminals_are_valid(&edge.next))
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
    first_sequences: &FirstSequenceAnalysis<'_>,
    recipes: &mut [FollowRecipe],
) {
    let suffixes = first_sequences.first_batch((0..sites.len()).map(|index| &sites[index + 1..]));
    for (site, suffix) in sites.iter().zip(suffixes) {
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

/// Copies one grammar-local FIRST position into an exact owned dispatch position.
fn owned_dispatch_position(
    position: FirstPosition,
    grammar: &GrammarDescription,
) -> DispatchPosition {
    DispatchPosition {
        kinds: position.kinds,
        text_predicate: position
            .text_predicate
            .map(|predicate| grammar.text_predicates[predicate.index()].clone()),
    }
}

/// Copies one grammar-local FIRST row into an exact owned dispatch row.
fn owned_dispatch_sequence(
    sequence: FirstSequence,
    grammar: &GrammarDescription,
) -> DispatchSequence {
    DispatchSequence {
        on_caller_follow: sequence.ends(),
        positions: sequence
            .positions
            .into_iter()
            .map(|position| owned_dispatch_position(position, grammar))
            .collect(),
    }
}

/// Orders exact owned dispatch positions canonically.
fn compare_dispatch_positions(left: &DispatchPosition, right: &DispatchPosition) -> Ordering {
    left.kinds
        .kind_count()
        .cmp(&right.kinds.kind_count())
        .then_with(|| left.kinds.words().cmp(right.kinds.words()))
        .then_with(|| {
            TextPartition::from_predicate(left.text_predicate.as_ref()).cmp(
                &TextPartition::from_predicate(right.text_predicate.as_ref()),
            )
        })
}

/// Orders exact owned dispatch sequences canonically.
fn compare_dispatch_sequences(left: &DispatchSequence, right: &DispatchSequence) -> Ordering {
    left.positions
        .iter()
        .zip(&right.positions)
        .find_map(|(left, right)| {
            let ordering = compare_dispatch_positions(left, right);
            (!ordering.is_eq()).then_some(ordering)
        })
        .unwrap_or_else(|| {
            left.positions
                .len()
                .cmp(&right.positions.len())
                .then_with(|| left.on_caller_follow.cmp(&right.on_caller_follow))
        })
}

/// Compacts exact owned dispatch rows into canonical kind-set rectangles.
fn sort_dedup_dispatch_sequences(sequences: &mut Vec<DispatchSequence>) {
    sequences.sort_by(compare_dispatch_sequences);
    sequences.dedup();
    let maximum_length = sequences
        .iter()
        .map(|sequence| sequence.positions.len())
        .max()
        .unwrap_or(0);
    for coordinate in 0..maximum_length {
        let mut groups = BTreeMap::<DispatchRectangleMergeKey, DispatchSequence>::new();
        let mut shorter = Vec::new();
        for sequence in std::mem::take(sequences) {
            if sequence.positions.len() <= coordinate {
                shorter.push(sequence);
                continue;
            }
            let key = DispatchRectangleMergeKey {
                on_caller_follow: sequence.on_caller_follow,
                merged_text: TextPartition::from_predicate(
                    sequence.positions[coordinate].text_predicate.as_ref(),
                ),
                positions: sequence
                    .positions
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != coordinate)
                    .map(|(_, position)| {
                        (
                            position.kinds.words().to_vec(),
                            TextPartition::from_predicate(position.text_predicate.as_ref()),
                        )
                    })
                    .collect(),
            };
            groups
                .entry(key)
                .and_modify(|merged| {
                    merged.positions[coordinate]
                        .kinds
                        .union_assign(&sequence.positions[coordinate].kinds);
                })
                .or_insert(sequence);
        }
        sequences.extend(shorter);
        sequences.extend(groups.into_values());
    }
    sequences.sort_by(compare_dispatch_sequences);
    sequences.dedup();
}

/// Reconstructs exact selecting prefixes from one compact semantic trie.
fn compact_sequences_by_declaration(
    resolution: &crate::analysis::CompactDispatchResolution,
) -> BTreeMap<DeclId, Vec<DispatchSequence>> {
    fn visit(
        node: &crate::analysis::CompactDispatchNode,
        prefix: &mut Vec<DispatchPosition>,
        sequences: &mut BTreeMap<DeclId, Vec<DispatchSequence>>,
    ) {
        if let Some(terminal) = node.terminal.filter(|terminal| {
            terminal.mode != crate::analysis::CompactDispatchTerminalMode::FallbackAfterEdges
        }) {
            sequences
                .entry(terminal.declaration)
                .or_default()
                .push(DispatchSequence {
                    positions: prefix.clone(),
                    on_caller_follow: matches!(
                        terminal.mode,
                        crate::analysis::CompactDispatchTerminalMode::CallerFollow
                    ),
                });
        }
        for edge in node.edges.iter() {
            prefix.push(DispatchPosition {
                kinds: edge.kinds.clone(),
                text_predicate: edge.text.clone().into_predicate(),
            });
            visit(&edge.next, prefix, sequences);
            prefix.pop();
        }
    }

    let mut sequences = BTreeMap::new();
    visit(&resolution.root, &mut Vec::new(), &mut sequences);
    for rows in sequences.values_mut() {
        sort_dedup_dispatch_sequences(rows);
    }
    sequences
}

/// Derives the canonical dispatch plan recorded in the immutable decision plan.
fn dispatch_plan<'a>(
    owner: TypeId,
    kind: DispatchKind,
    alternatives: impl IntoIterator<Item = (DeclId, Option<PrattPosition>, &'a [SiteId])>,
    grammar: &GrammarDescription,
    first_sequences: &FirstSequenceAnalysis<'_>,
) -> DispatchPlan {
    let alternatives = alternatives.into_iter().collect::<Vec<_>>();
    if kind == DispatchKind::Enum {
        return compact_enum_dispatch_plan(owner, &alternatives, grammar, first_sequences);
    }
    let maximum_depth = grammar.max_lookahead;
    let depths = if kind == DispatchKind::PrattLed || maximum_depth == 0 {
        vec![maximum_depth]
    } else {
        (1..=maximum_depth).collect()
    };
    let bounded = depths.into_iter().find_map(|depth| {
        let sequences = first_sequences
            .sequences_batch_at_depth(alternatives.iter().map(|(_, _, sites)| *sites), depth);
        let mut branches = alternatives
            .iter()
            .zip(sequences)
            .map(|((declaration, position, _), sequences)| DispatchBranch {
                declaration: *declaration,
                position: *position,
                sequences: sequences
                    .into_iter()
                    .map(|sequence| owned_dispatch_sequence(sequence, grammar))
                    .collect(),
            })
            .collect::<Vec<_>>();
        branches.sort_by(|left, right| {
            grammar.declarations[left.declaration.index()]
                .name
                .cmp(&grammar.declarations[right.declaration.index()].name)
        });
        try_freeze_dispatch_trie(&branches, grammar, kind != DispatchKind::Enum)
            .map(|(trie, required)| (branches, trie, required))
    });
    let (branches, trie, required_lookahead, balanced) =
        if let Some((branches, trie, required_lookahead)) = bounded {
            (branches, trie, required_lookahead, None)
        } else {
            let sequences = first_sequences.sequences_batch_at_depth(
                alternatives.iter().map(|(_, _, sites)| *sites),
                maximum_depth,
            );
            let mut branches = alternatives
                .iter()
                .zip(sequences)
                .map(|((declaration, position, _), sequences)| DispatchBranch {
                    declaration: *declaration,
                    position: *position,
                    sequences: sequences
                        .into_iter()
                        .map(|sequence| owned_dispatch_sequence(sequence, grammar))
                        .collect(),
                })
                .collect::<Vec<_>>();
            branches.sort_by(|left, right| {
                grammar.declarations[left.declaration.index()]
                    .name
                    .cmp(&grammar.declarations[right.declaration.index()].name)
            });
            let balanced =
                try_balanced_dispatch(&alternatives, &branches, grammar, first_sequences)
                    .expect("validated structural dispatch resolves the unbounded shared prefix");
            let bounded_branches =
                ordinary_branches_without_balanced_opener(&branches, balanced.open, grammar)
                    .expect("validated structural dispatch partitions every opener route exactly");
            let (trie, required_lookahead) =
                try_freeze_dispatch_trie(&bounded_branches, grammar, kind != DispatchKind::Enum)
                    .expect("validated structural dispatch resolves every non-opener route");
            (branches, trie, required_lookahead, Some(balanced))
        };
    let on_caller_follow = branches.iter().find_map(|branch| {
        branch
            .sequences
            .iter()
            .any(|sequence| sequence.positions.is_empty() && sequence.on_caller_follow)
            .then_some(branch.declaration)
    });
    DispatchPlan {
        owner,
        kind,
        branches,
        required_lookahead,
        on_caller_follow,
        trie,
        balanced,
    }
}

/// Freezes one enum through the labelled traversal before materializing bounded branch languages.
fn compact_enum_dispatch_plan(
    owner: TypeId,
    alternatives: &[(DeclId, Option<PrattPosition>, &[SiteId])],
    grammar: &GrammarDescription,
    first_sequences: &FirstSequenceAnalysis<'_>,
) -> DispatchPlan {
    let semantic_alternatives = alternatives
        .iter()
        .map(|(declaration, _, sites)| (*declaration, *sites))
        .collect::<Vec<_>>();
    let (sequences, trie, required_lookahead, balanced) =
        if let Some(resolution) = first_sequences.compact_enum_dispatch(&semantic_alternatives) {
            let mut by_declaration = compact_sequences_by_declaration(&resolution);
            let sequences: Vec<_> = alternatives
                .iter()
                .map(|(declaration, _, _)| by_declaration.remove(declaration).unwrap_or_default())
                .collect();
            (
                sequences,
                dispatch_trie_from_compact(resolution.root),
                resolution.required_lookahead,
                None,
            )
        } else {
            let resolution = first_sequences
                .compact_balanced_enum_dispatch(&semantic_alternatives)
                .expect("validated structural dispatch resolves the unbounded shared prefix");
            // Structural routes are represented by `BalancedDispatch`; branch
            // rows describe only truthful bounded routes in the ordinary trie.
            let mut by_declaration = compact_sequences_by_declaration(&resolution.ordinary);
            let sequences: Vec<_> = alternatives
                .iter()
                .map(|(declaration, _, _)| by_declaration.remove(declaration).unwrap_or_default())
                .collect();
            (
                sequences,
                dispatch_trie_from_compact(resolution.ordinary.root),
                resolution.ordinary.required_lookahead,
                Some(BalancedDispatch {
                    open: resolution.open,
                    close: resolution.close,
                    after_close: dispatch_trie_from_compact(resolution.after_close.root),
                }),
            )
        };
    let mut branches = alternatives
        .iter()
        .zip(sequences)
        .map(|((declaration, position, _), sequences)| DispatchBranch {
            declaration: *declaration,
            position: *position,
            sequences,
        })
        .collect::<Vec<_>>();
    branches.sort_by(|left, right| {
        grammar.declarations[left.declaration.index()]
            .name
            .cmp(&grammar.declarations[right.declaration.index()].name)
    });
    let on_caller_follow = trie
        .terminal
        .filter(|terminal| terminal.mode == DispatchTerminalMode::CallerFollow)
        .map(|terminal| terminal.declaration);
    DispatchPlan {
        owner,
        kind: DispatchKind::Enum,
        branches,
        required_lookahead,
        on_caller_follow,
        trie,
        balanced,
    }
}

/// Converts one compact semantic trie into the published dispatch representation.
fn dispatch_trie_from_compact(node: crate::analysis::CompactDispatchNode) -> DispatchTrieNode {
    fn convert(
        node: &crate::analysis::CompactDispatchNode,
        memo: &mut BTreeMap<
            (
                Option<crate::analysis::CompactDispatchTerminal>,
                usize,
                usize,
            ),
            DispatchTrieNode,
        >,
    ) -> DispatchTrieNode {
        let key = (
            node.terminal,
            node.edges.as_ptr() as usize,
            node.edges.len(),
        );
        if let Some(converted) = memo.get(&key) {
            return converted.clone();
        }
        let converted = DispatchTrieNode {
            terminal: node.terminal.map(|terminal| DispatchTerminal {
                declaration: terminal.declaration,
                mode: match terminal.mode {
                    crate::analysis::CompactDispatchTerminalMode::Immediate => {
                        DispatchTerminalMode::Immediate
                    }
                    crate::analysis::CompactDispatchTerminalMode::CallerFollow => {
                        DispatchTerminalMode::CallerFollow
                    }
                    crate::analysis::CompactDispatchTerminalMode::FallbackAfterEdges => {
                        DispatchTerminalMode::FallbackAfterEdges
                    }
                },
            }),
            edges: node
                .edges
                .iter()
                .map(|edge| DispatchTrieEdge {
                    position: DispatchPosition {
                        kinds: edge.kinds.clone(),
                        text_predicate: edge.text.clone().into_predicate(),
                    },
                    next: convert(&edge.next, memo),
                })
                .collect::<Vec<_>>()
                .into(),
        };
        memo.insert(key, converted.clone());
        converted
    }
    convert(&node, &mut BTreeMap::new())
}

/// Builds one balanced-delimiter cut and freezes its post-close residual languages.
fn try_balanced_dispatch(
    alternatives: &[(DeclId, Option<PrattPosition>, &[SiteId])],
    branches: &[DispatchBranch],
    grammar: &GrammarDescription,
    first_sequences: &FirstSequenceAnalysis<'_>,
) -> Option<BalancedDispatch> {
    if alternatives
        .iter()
        .any(|(_, position, _)| position.is_some())
    {
        return None;
    }
    let sites = alternatives
        .iter()
        .map(|(declaration, _, sites)| (*declaration, *sites))
        .collect::<Vec<_>>();
    let resolution = crate::analysis::balanced_dispatch_resolution(&sites, first_sequences)?;
    let non_open_branches =
        ordinary_branches_without_balanced_opener(branches, resolution.open, grammar)?;
    // The balanced plan replaces only routes that begin with the exact opener.
    // Every other route still uses the ordinary trie and must independently
    // freeze without ambiguity. An empty non-opener language is valid.
    try_freeze_dispatch_trie(&non_open_branches, grammar, false)?;
    let residual_branches = resolution
        .branches
        .into_iter()
        .map(|branch| DispatchBranch {
            declaration: branch.declaration,
            position: None,
            sequences: branch
                .sequences
                .into_iter()
                .map(|sequence| owned_dispatch_sequence(sequence, grammar))
                .collect(),
        })
        .collect::<Vec<_>>();
    let (after_close, _) = try_freeze_dispatch_trie(&residual_branches, grammar, false)?;
    Some(BalancedDispatch {
        open: resolution.open,
        close: resolution.close,
        after_close,
    })
}

/// Removes the exact balanced-opener route while preserving every ordinary kind in mixed cells.
///
/// FIRST-language rectangle compaction may combine the opener with other
/// kinds when their bounded suffix is identical. The structural scanner owns
/// only the opener kind; the ordinary trie retains the set difference. A text
/// predicate over a mixed opener cell cannot be split soundly by kind alone,
/// so that shape remains unsupported.
fn ordinary_branches_without_balanced_opener(
    branches: &[DispatchBranch],
    open: KindId,
    grammar: &GrammarDescription,
) -> Option<Vec<DispatchBranch>> {
    let opener = KindSet::singleton(grammar.kinds.len(), open).ok()?;
    let mut ordinary = branches.to_vec();
    for branch in &mut ordinary {
        let mut sequences = Vec::with_capacity(branch.sequences.len());
        for source in &branch.sequences {
            let mut sequence = source.clone();
            if let Some(position) = sequence.positions.first_mut()
                && position.kinds.contains(open)
            {
                if position.text_predicate.is_some() {
                    return None;
                }
                position.kinds = position.kinds.difference(&opener).ok()?;
                if position.kinds.is_empty() {
                    continue;
                }
            }
            sequences.push(sequence);
        }
        branch.sequences = sequences;
    }
    Some(ordinary)
}

/// Reports whether the exact structural strategy can replace one enum conflict diagnostic.
pub(crate) fn balanced_dispatch_is_supported(
    alternatives: &[&crate::Alternative],
    _grammar: &GrammarDescription,
    first_sequences: &FirstSequenceAnalysis<'_>,
) -> bool {
    let alternatives = alternatives
        .iter()
        .map(|alternative| (alternative.declaration, alternative.sites.as_slice()))
        .collect::<Vec<_>>();
    first_sequences
        .compact_balanced_enum_dispatch(&alternatives)
        .is_some()
}

/// Dense identity of one canonical dispatch-language suffix.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DispatchSuffixId(usize);

impl DispatchSuffixId {
    /// Returns the arena index for this suffix.
    const fn index(self) -> usize {
        self.0
    }
}

/// One immutable canonical suffix used while partitioning a dispatch trie.
#[derive(Clone, Debug)]
struct DispatchSuffix {
    /// Terminal reached after every remaining position has matched.
    terminal: DispatchTerminal,
    /// Current symbolic position, absent only at the terminal suffix.
    position: Option<DispatchSuffixPosition>,
    /// Canonical residual after consuming the current position.
    next: Option<DispatchSuffixId>,
}

#[derive(Clone, Debug)]
/// One exact owned position retained in the private suffix arena.
struct DispatchSuffixPosition {
    /// Token kinds accepted at this position.
    kinds: KindSet,
    /// Exact canonical text cell accepted at this position.
    text: TextPartition,
}

/// Structural identity used to hash-cons equivalent dispatch suffixes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DispatchSuffixKey {
    /// Terminal reached by the suffix.
    terminal: DispatchTerminal,
    /// Packed current token kinds and exact canonical text cell.
    position: Option<(Vec<u64>, TextPartition)>,
    /// Canonical successor suffix, absent at the terminal.
    next: Option<DispatchSuffixId>,
}

/// One memoized trie state and the relative depth needed to distinguish its declarations.
#[derive(Clone, Debug)]
struct FrozenDispatchState {
    /// Immutable dispatch node for this residual language.
    node: DispatchTrieNode,
    /// Additional token positions needed while more than one declaration remains.
    relative_required_lookahead: usize,
}

/// Attempts to freeze a dispatch at the supplied bounded-language depth.
///
/// `cut_off_unique_pratt_branch` turns every non-root residual state belonging
/// to one declaration into an unconditional leaf, including states that also
/// contain same-declaration caller-FOLLOW terminals. Disabling it preserves
/// complete-route and caller-FOLLOW behavior for ordinary enum dispatch.
fn try_freeze_dispatch_trie(
    branches: &[DispatchBranch],
    grammar: &GrammarDescription,
    cut_off_unique_pratt_branch: bool,
) -> Option<(DispatchTrieNode, usize)> {
    try_freeze_dispatch_trie_with_observer(branches, grammar, cut_off_unique_pratt_branch, |_| {})
}

/// Freezes dispatch while exposing each unique residual state to performance regressions.
#[cfg(test)]
fn freeze_dispatch_trie_with_observer(
    branches: &[DispatchBranch],
    grammar: &GrammarDescription,
    mut observe_state: impl FnMut(&[DispatchSuffixId]),
) -> (DispatchTrieNode, usize) {
    try_freeze_dispatch_trie_with_observer(branches, grammar, true, &mut observe_state)
        .expect("validated dispatch resolves within supplied lookahead")
}

/// Attempts to freeze dispatch while exposing each unique residual state.
///
/// The Pratt cutoff selects declaration identity after a non-empty unique
/// prefix; without it, residual syntax and caller FOLLOW remain part of the
/// frozen selection language.
fn try_freeze_dispatch_trie_with_observer(
    branches: &[DispatchBranch],
    grammar: &GrammarDescription,
    cut_off_unique_pratt_branch: bool,
    mut observe_state: impl FnMut(&[DispatchSuffixId]),
) -> Option<(DispatchTrieNode, usize)> {
    let mut suffixes = Vec::<DispatchSuffix>::new();
    let mut suffix_ids = BTreeMap::<DispatchSuffixKey, DispatchSuffixId>::new();
    let mut state = Vec::new();
    for branch in branches {
        for sequence in &branch.sequences {
            let terminal = DispatchTerminal {
                declaration: branch.declaration,
                mode: if sequence.on_caller_follow {
                    DispatchTerminalMode::CallerFollow
                } else {
                    DispatchTerminalMode::Immediate
                },
            };
            let terminal_key = DispatchSuffixKey {
                terminal,
                position: None,
                next: None,
            };
            let mut suffix = *suffix_ids.entry(terminal_key).or_insert_with(|| {
                let id = DispatchSuffixId(suffixes.len());
                suffixes.push(DispatchSuffix {
                    terminal,
                    position: None,
                    next: None,
                });
                id
            });
            for position in sequence.positions.iter().rev() {
                let text = TextPartition::from_predicate(position.text_predicate.as_ref());
                let key = DispatchSuffixKey {
                    terminal,
                    position: Some((position.kinds.words().to_vec(), text.clone())),
                    next: Some(suffix),
                };
                suffix = *suffix_ids.entry(key).or_insert_with(|| {
                    let id = DispatchSuffixId(suffixes.len());
                    suffixes.push(DispatchSuffix {
                        terminal,
                        position: Some(DispatchSuffixPosition {
                            kinds: position.kinds.clone(),
                            text,
                        }),
                        next: Some(suffix),
                    });
                    id
                });
            }
            state.push(suffix);
        }
    }
    state.sort_unstable();
    state.dedup();
    let mut memo = BTreeMap::new();
    let frozen = freeze_dispatch_state(
        &suffixes,
        &state,
        false,
        cut_off_unique_pratt_branch,
        &mut memo,
        &mut observe_state,
    )?;
    assert!(dispatch_edges_are_disjoint(&frozen.node, grammar));
    let required_lookahead = usize::from(!branches.is_empty())
        .max(frozen.relative_required_lookahead)
        .min(grammar.max_lookahead);
    Some((frozen.node, required_lookahead))
}

/// Freezes one residual-language state and memoizes equivalent suffix states.
///
/// When the Pratt cutoff is enabled, a matched state whose suffixes all name
/// one declaration becomes an unconditional leaf before terminal or edge
/// processing. This deliberately delegates residual validation and error
/// provenance to the selected declaration parser.
fn freeze_dispatch_state(
    suffixes: &[DispatchSuffix],
    state: &[DispatchSuffixId],
    matched_prefix: bool,
    cut_off_unique_pratt_branch: bool,
    memo: &mut BTreeMap<(bool, Vec<DispatchSuffixId>), Option<FrozenDispatchState>>,
    observe_state: &mut impl FnMut(&[DispatchSuffixId]),
) -> Option<FrozenDispatchState> {
    let memo_key = (matched_prefix, state.to_vec());
    if let Some(frozen) = memo.get(&memo_key) {
        return frozen.clone();
    }
    observe_state(state);

    let has_multiple_declarations = state.first().is_some_and(|first| {
        let declaration = suffixes[first.index()].terminal.declaration;
        state
            .iter()
            .skip(1)
            .any(|suffix| suffixes[suffix.index()].terminal.declaration != declaration)
    });

    let terminals = state
        .iter()
        .filter_map(|suffix| {
            suffixes[suffix.index()]
                .position
                .is_none()
                .then_some(suffixes[suffix.index()].terminal)
        })
        .collect::<Vec<_>>();
    if cut_off_unique_pratt_branch && matched_prefix && !has_multiple_declarations {
        let terminal = suffixes[state.first()?.index()].terminal;
        let frozen = FrozenDispatchState {
            node: DispatchTrieNode {
                terminal: Some(DispatchTerminal {
                    declaration: terminal.declaration,
                    mode: DispatchTerminalMode::Immediate,
                }),
                edges: Arc::default(),
            },
            relative_required_lookahead: 0,
        };
        memo.insert(memo_key, Some(frozen.clone()));
        return Some(frozen);
    }
    let terminal = terminals.first().copied();
    if let Some(first) = terminal {
        if terminals
            .iter()
            .any(|candidate| candidate.declaration != first.declaration)
        {
            memo.insert(memo_key, None);
            return None;
        }
        if terminals
            .iter()
            .any(|candidate| candidate.mode != DispatchTerminalMode::CallerFollow)
        {
            if state
                .iter()
                .any(|suffix| suffixes[suffix.index()].terminal.declaration != first.declaration)
            {
                memo.insert(memo_key, None);
                return None;
            }
            let frozen = FrozenDispatchState {
                node: DispatchTrieNode {
                    terminal: Some(DispatchTerminal {
                        declaration: first.declaration,
                        mode: DispatchTerminalMode::Immediate,
                    }),
                    edges: Arc::default(),
                },
                relative_required_lookahead: 0,
            };
            memo.insert(memo_key, Some(frozen.clone()));
            return Some(frozen);
        }
    }

    let candidates = state
        .iter()
        .copied()
        .filter(|suffix| suffixes[suffix.index()].position.is_some())
        .collect::<Vec<_>>();
    let partitions = overlay_by_kind_residual(
        &candidates,
        |suffix| {
            &suffixes[suffix.index()]
                .position
                .as_ref()
                .expect("candidate suffix has a position")
                .text
        },
        |suffix| {
            &suffixes[suffix.index()]
                .position
                .as_ref()
                .expect("candidate suffix has a position")
                .kinds
        },
        |suffix| {
            suffixes[suffix.index()]
                .next
                .expect("candidate suffix has a residual")
        },
    );

    let mut edges = Vec::with_capacity(partitions.len());
    let mut maximum_child_lookahead = 0;
    for (text, residual, kinds) in partitions {
        let next = freeze_dispatch_state(
            suffixes,
            &residual,
            true,
            cut_off_unique_pratt_branch,
            memo,
            observe_state,
        )?;
        maximum_child_lookahead = maximum_child_lookahead.max(next.relative_required_lookahead);
        edges.push(DispatchTrieEdge {
            position: DispatchPosition {
                kinds,
                text_predicate: text.into_predicate(),
            },
            next: next.node,
        });
    }
    edges.sort_by(compare_dispatch_edges);
    let relative_required_lookahead = if has_multiple_declarations {
        maximum_child_lookahead.saturating_add(1)
    } else {
        0
    };
    let frozen = FrozenDispatchState {
        node: DispatchTrieNode {
            terminal,
            edges: edges.into(),
        },
        relative_required_lookahead,
    };
    memo.insert(memo_key, Some(frozen.clone()));
    Some(frozen)
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
            let kinds_overlap = left
                .position
                .kinds
                .intersects(&right.position.kinds)
                .expect("dispatch edges share the grammar width");
            if kinds_overlap
                && TextPartition::from_predicate(left.position.text_predicate.as_ref()).overlaps(
                    &TextPartition::from_predicate(right.position.text_predicate.as_ref()),
                    grammar.text_equivalence,
                )
            {
                return false;
            }
        }
        if !dispatch_edges_are_disjoint(left.next(), grammar) {
            return false;
        }
    }
    true
}

/// Derives required dispatch lookahead using the supplied pairwise discriminator.
#[cfg(test)]
fn required_dispatch_lookahead_with(
    branches: &[DispatchBranch],
    max_lookahead: usize,
    mut discrimination_depth: impl FnMut(&DispatchSequence, &DispatchSequence) -> Option<usize>,
) -> usize {
    let mut required = usize::from(!branches.is_empty());
    if required >= max_lookahead {
        return max_lookahead;
    }
    for (left_index, left) in branches.iter().enumerate() {
        for right in &branches[left_index + 1..] {
            for left_sequence in &left.sequences {
                for right_sequence in &right.sequences {
                    let depth = discrimination_depth(left_sequence, right_sequence)
                        .unwrap_or(max_lookahead);
                    required = required.max(depth);
                    if required >= max_lookahead {
                        return max_lookahead;
                    }
                }
            }
        }
    }
    required.min(max_lookahead)
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
        // Failure recipes still use grammar-local FIRST positions, so exact
        // owned/synthetic dispatch predicates are conservatively projected to
        // their token kinds rather than assigned fabricated predicate IDs.
        let mut expected = if dispatch.kind == DispatchKind::Enum {
            let mut expected = dispatch
                .trie
                .edges
                .iter()
                .map(|edge| FirstPosition {
                    kinds: edge.position.kinds.clone(),
                    text_predicate: None,
                })
                .collect::<Vec<_>>();
            if let Some(balanced) = &dispatch.balanced {
                expected.push(FirstPosition {
                    kinds: KindSet::singleton(grammar.kind_count, balanced.open)
                        .expect("balanced opener belongs to the grammar"),
                    text_predicate: None,
                });
            }
            expected
        } else {
            dispatch
                .branches
                .iter()
                .flat_map(|branch| branch.sequences.iter())
                .filter_map(|sequence| sequence.positions.first())
                .map(|position| FirstPosition {
                    kinds: position.kinds.clone(),
                    text_predicate: None,
                })
                .collect::<Vec<_>>()
        };
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
#[cfg(test)]
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
            left.kinds
                .intersects(&right.kinds)
                .expect("decision positions share the grammar kind width")
                && TextPartition::from_predicate(left.text_predicate.as_ref()).overlaps(
                    &TextPartition::from_predicate(right.text_predicate.as_ref()),
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
        .map(|position| render_dispatch_position(position, grammar))
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

/// Renders every terminal and edge in one frozen dispatch trie without assigning incidental IDs.
fn render_dispatch_trie(
    node: &DispatchTrieNode,
    grammar: &GrammarDescription,
    output: &mut String,
) {
    output.push_str("(terminal=");
    if let Some(terminal) = node.terminal {
        output.push_str(&grammar.declarations[terminal.declaration.index()].name);
        match terminal.mode {
            DispatchTerminalMode::Immediate => {}
            DispatchTerminalMode::CallerFollow => output.push_str("@caller-follow"),
            DispatchTerminalMode::FallbackAfterEdges => {
                output.push_str("@fallback-after-edges");
            }
        }
    } else {
        output.push('-');
    }
    output.push_str(" edges=[");
    for (index, edge) in node.edges.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&render_dispatch_position(&edge.position, grammar));
        output.push_str("=>");
        render_dispatch_trie(&edge.next, grammar, output);
    }
    output.push_str("])");
}

/// Renders one optional viability decision trie without incidental node identities.
fn render_optional_viability_decision_trie(
    node: &OptionalViabilityDecisionNode,
    grammar: &GrammarDescription,
    output: &mut String,
) {
    output.push_str("(terminals=[");
    for (index, terminal) in node.terminals.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "{:?}", terminal.action).expect("writing to a String cannot fail");
        if terminal.on_caller_follow {
            output.push_str("@caller-follow");
        }
    }
    output.push_str("] edges=[");
    for (index, edge) in node.edges.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&render_dispatch_position(&edge.position, grammar));
        output.push_str("=>");
        render_optional_viability_decision_trie(&edge.next, grammar, output);
    }
    output.push_str("])");
}

/// Renders one owned frozen-trie position using grammar-local names and text semantics.
fn render_dispatch_position(position: &DispatchPosition, grammar: &GrammarDescription) -> String {
    let kinds = position
        .kinds
        .iter()
        .map(|kind| grammar.kinds[kind.index()].name.as_str())
        .collect::<Vec<_>>()
        .join("|");
    position
        .text_predicate
        .as_ref()
        .map_or(kinds.clone(), |predicate| format!("{kinds}@{predicate:?}"))
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
        AtomDescription, AtomExpression, AtomId, ContainerShape, Declaration, KindDescription,
        Location, ParseSite, PrattVariant, RepeatSeparator, RepetitionCardinality, SourceId,
        TextEquivalence, TypeDescription,
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

    fn disjoint_optional_grammar() -> GrammarDescription {
        let mut grammar =
            GrammarDescription::new("disjoint-optional", 2, 2, TextEquivalence::Sensitive);
        let optional_token = token(&mut grammar, "OPTIONAL", 0);
        let required_token = token(&mut grammar, "REQUIRED", 2);
        let owner = grammar.next_type_id();
        let owner_declaration = declaration(&mut grammar, "Container", 4);
        let optional_declaration = declaration(&mut grammar, "Container::optional", 5);
        let optional = grammar.push_site(ParseSite::new(
            owner,
            optional_token,
            optional_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        let required_declaration = declaration(&mut grammar, "Container::required", 6);
        let required = grammar.push_site(ParseSite::new(
            owner,
            required_token,
            required_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Container",
                owner_declaration,
                TypeShape::Sequence {
                    sites: vec![optional, required],
                },
            )),
            owner,
        );
        grammar.entry_types = vec![owner];
        grammar
    }

    fn same_kind_disjoint_text_optional_grammar() -> GrammarDescription {
        let mut grammar = GrammarDescription::new(
            "same-kind-disjoint-text-optional",
            1,
            2,
            TextEquivalence::Sensitive,
        );
        let word_declaration = declaration(&mut grammar, "WORD", 0);
        let word = grammar.push_kind(KindDescription::new("WORD", word_declaration));
        let atom_declaration = declaration(&mut grammar, "WordAtom", 2);
        let atom = grammar.push_atom(AtomDescription::new(
            "WordAtom",
            atom_declaration,
            AtomExpression::Kind(word),
        ));
        let predicated_token =
            |grammar: &mut GrammarDescription, name: &str, spelling: &str, byte: u32| {
                let declaration = declaration(grammar, name, byte);
                let text_predicate =
                    grammar.push_text_predicate(TextPredicate::Exact(spelling.to_owned()));
                grammar.push_type(TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Token {
                        atom,
                        text_predicate: Some(text_predicate),
                    },
                ))
            };
        let optional_token = predicated_token(&mut grammar, "OPEN", "OPEN", 4);
        let required_token = predicated_token(&mut grammar, "CLOSE", "CLOSE", 6);
        let owner = grammar.next_type_id();
        let owner_declaration = declaration(&mut grammar, "Container", 8);
        let optional_declaration = declaration(&mut grammar, "Container::optional", 10);
        let optional = grammar.push_site(ParseSite::new(
            owner,
            optional_token,
            optional_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        let required_declaration = declaration(&mut grammar, "Container::required", 12);
        let required = grammar.push_site(ParseSite::new(
            owner,
            required_token,
            required_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Container",
                owner_declaration,
                TypeShape::Sequence {
                    sites: vec![optional, required],
                },
            )),
            owner,
        );
        grammar.entry_types = vec![owner];
        grammar
    }

    fn static_and_caller_optional_grammar() -> GrammarDescription {
        let mut grammar = GrammarDescription::new(
            "static-and-caller-optional",
            2,
            2,
            TextEquivalence::Sensitive,
        );
        let first_token = token(&mut grammar, "FIRST", 0);
        let second_token = token(&mut grammar, "SECOND", 2);
        let first_predicate = grammar.push_text_predicate(TextPredicate::Exact("FIRST".to_owned()));
        let TypeShape::Token { text_predicate, .. } = &mut grammar.types[first_token.index()].shape
        else {
            unreachable!("the fixture first token has token shape");
        };
        *text_predicate = Some(first_predicate);
        let owner = grammar.next_type_id();
        let owner_declaration = declaration(&mut grammar, "Container", 4);
        let first_declaration = declaration(&mut grammar, "Container::first", 5);
        let first = grammar.push_site(ParseSite::new(
            owner,
            first_token,
            first_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        let second_declaration = declaration(&mut grammar, "Container::second", 6);
        let second = grammar.push_site(ParseSite::new(
            owner,
            second_token,
            second_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Container",
                owner_declaration,
                TypeShape::Sequence {
                    sites: vec![first, second],
                },
            )),
            owner,
        );
        grammar.entry_types = vec![owner];
        grammar
    }

    fn branching_optional_overlap_grammar_with_owners(owner_count: usize) -> GrammarDescription {
        let mut grammar =
            GrammarDescription::new("branching-optional", 5, 19, TextEquivalence::Sensitive);
        let a = token(&mut grammar, "A", 0);
        let b = token(&mut grammar, "B", 2);
        let c = token(&mut grammar, "C", 4);
        let left = token(&mut grammar, "LEFT", 6);
        let right = token(&mut grammar, "RIGHT", 8);
        let mut next_byte = 10;

        let mut branching = left;
        for depth in 0..8 {
            let branching_state = grammar.next_type_id();
            let branching_declaration =
                declaration(&mut grammar, &format!("Branching{depth}"), next_byte);
            next_byte += 1;
            let left_declaration =
                declaration(&mut grammar, &format!("Branching{depth}::Left"), next_byte);
            next_byte += 1;
            let left_token_declaration = declaration(
                &mut grammar,
                &format!("Branching{depth}::Left::token"),
                next_byte,
            );
            next_byte += 1;
            let left_token = grammar.push_site(ParseSite::new(
                branching_state,
                left,
                left_token_declaration,
                ContainerShape::Direct,
            ));
            let left_tail_declaration = declaration(
                &mut grammar,
                &format!("Branching{depth}::Left::tail"),
                next_byte,
            );
            next_byte += 1;
            let left_tail = grammar.push_site(ParseSite::new(
                branching_state,
                branching,
                left_tail_declaration,
                ContainerShape::Direct,
            ));
            let left_close_declaration = declaration(
                &mut grammar,
                &format!("Branching{depth}::Left::close"),
                next_byte,
            );
            next_byte += 1;
            let left_close = grammar.push_site(ParseSite::new(
                branching_state,
                left,
                left_close_declaration,
                ContainerShape::Direct,
            ));
            let right_declaration =
                declaration(&mut grammar, &format!("Branching{depth}::Right"), next_byte);
            next_byte += 1;
            let right_token_declaration = declaration(
                &mut grammar,
                &format!("Branching{depth}::Right::token"),
                next_byte,
            );
            next_byte += 1;
            let right_token = grammar.push_site(ParseSite::new(
                branching_state,
                right,
                right_token_declaration,
                ContainerShape::Direct,
            ));
            let right_tail_declaration = declaration(
                &mut grammar,
                &format!("Branching{depth}::Right::tail"),
                next_byte,
            );
            next_byte += 1;
            let right_tail = grammar.push_site(ParseSite::new(
                branching_state,
                branching,
                right_tail_declaration,
                ContainerShape::Direct,
            ));
            let right_close_declaration = declaration(
                &mut grammar,
                &format!("Branching{depth}::Right::close"),
                next_byte,
            );
            next_byte += 1;
            let right_close = grammar.push_site(ParseSite::new(
                branching_state,
                right,
                right_close_declaration,
                ContainerShape::Direct,
            ));
            assert_eq!(
                grammar.push_type(TypeDescription::new(
                    format!("Branching{depth}"),
                    branching_declaration,
                    TypeShape::Enum {
                        alternatives: vec![
                            crate::Alternative {
                                declaration: left_declaration,
                                sites: vec![left_token, left_tail, left_close],
                            },
                            crate::Alternative {
                                declaration: right_declaration,
                                sites: vec![right_token, right_tail, right_close],
                            },
                        ],
                    },
                )),
                branching_state,
            );
            branching = branching_state;
        }

        let path =
            |grammar: &mut GrammarDescription, name: &str, second: TypeId, next_byte: &mut u32| {
                let path = grammar.next_type_id();
                let path_declaration = declaration(grammar, name, *next_byte);
                *next_byte += 1;
                let first_declaration = declaration(grammar, &format!("{name}::first"), *next_byte);
                *next_byte += 1;
                let first = grammar.push_site(ParseSite::new(
                    path,
                    a,
                    first_declaration,
                    ContainerShape::Direct,
                ));
                let second_declaration =
                    declaration(grammar, &format!("{name}::second"), *next_byte);
                *next_byte += 1;
                let second = grammar.push_site(ParseSite::new(
                    path,
                    second,
                    second_declaration,
                    ContainerShape::Direct,
                ));
                let branching_declaration =
                    declaration(grammar, &format!("{name}::branching"), *next_byte);
                *next_byte += 1;
                let branching_site = grammar.push_site(ParseSite::new(
                    path,
                    branching,
                    branching_declaration,
                    ContainerShape::Direct,
                ));
                assert_eq!(
                    grammar.push_type(TypeDescription::new(
                        name,
                        path_declaration,
                        TypeShape::Sequence {
                            sites: vec![first, second, branching_site],
                        },
                    )),
                    path,
                );
                path
            };
        let consuming = path(&mut grammar, "Consuming", b, &mut next_byte);
        let skipping = path(&mut grammar, "Skipping", c, &mut next_byte);

        for index in 0..owner_count {
            let owner = grammar.next_type_id();
            let owner_name = format!("Owner{index}");
            let owner_declaration = declaration(&mut grammar, &owner_name, next_byte);
            next_byte += 1;
            let optional_declaration =
                declaration(&mut grammar, &format!("{owner_name}::optional"), next_byte);
            next_byte += 1;
            let optional = grammar.push_site(ParseSite::new(
                owner,
                consuming,
                optional_declaration,
                ContainerShape::optional(ContainerShape::Direct),
            ));
            let required_declaration =
                declaration(&mut grammar, &format!("{owner_name}::required"), next_byte);
            next_byte += 1;
            let required = grammar.push_site(ParseSite::new(
                owner,
                skipping,
                required_declaration,
                ContainerShape::Direct,
            ));
            assert_eq!(
                grammar.push_type(TypeDescription::new(
                    owner_name,
                    owner_declaration,
                    TypeShape::Sequence {
                        sites: vec![optional, required],
                    },
                )),
                owner,
            );
            grammar.entry_types.push(owner);
        }
        grammar
    }

    fn branching_optional_overlap_grammar() -> GrammarDescription {
        branching_optional_overlap_grammar_with_owners(1)
    }

    fn wide_disjoint_tail_optional_overlap_grammar(width: usize) -> GrammarDescription {
        let mut grammar = GrammarDescription::new(
            "wide-disjoint-tail-optional",
            width + 3,
            2,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0u32;
        let mut next_token = |grammar: &mut GrammarDescription, name: &str| {
            let token = token(grammar, name, next_byte);
            next_byte += 2;
            token
        };
        let a = next_token(&mut grammar, "A");
        let b = next_token(&mut grammar, "B");
        let c = next_token(&mut grammar, "C");
        let disjoint = (0..width)
            .map(|index| next_token(&mut grammar, &format!("D{index}")))
            .collect::<Vec<_>>();

        let consuming = grammar.next_type_id();
        let consuming_declaration = declaration(&mut grammar, "Consuming", next_byte);
        next_byte += 1;
        let consuming_a_declaration = declaration(&mut grammar, "Consuming::a", next_byte);
        next_byte += 1;
        let consuming_a = grammar.push_site(ParseSite::new(
            consuming,
            a,
            consuming_a_declaration,
            ContainerShape::Direct,
        ));
        let consuming_b_declaration = declaration(&mut grammar, "Consuming::b", next_byte);
        next_byte += 1;
        let consuming_b = grammar.push_site(ParseSite::new(
            consuming,
            b,
            consuming_b_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Consuming",
                consuming_declaration,
                TypeShape::Sequence {
                    sites: vec![consuming_a, consuming_b],
                },
            )),
            consuming,
        );

        let skipping = grammar.next_type_id();
        let skipping_declaration = declaration(&mut grammar, "Skipping", next_byte);
        next_byte += 1;
        let shared_declaration = declaration(&mut grammar, "Skipping::Shared", next_byte);
        next_byte += 1;
        let shared_a_declaration = declaration(&mut grammar, "Skipping::Shared::a", next_byte);
        next_byte += 1;
        let shared_a = grammar.push_site(ParseSite::new(
            skipping,
            a,
            shared_a_declaration,
            ContainerShape::Direct,
        ));
        let shared_c_declaration = declaration(&mut grammar, "Skipping::Shared::c", next_byte);
        next_byte += 1;
        let shared_c = grammar.push_site(ParseSite::new(
            skipping,
            c,
            shared_c_declaration,
            ContainerShape::Direct,
        ));
        let mut alternatives = vec![crate::Alternative {
            declaration: shared_declaration,
            sites: vec![shared_a, shared_c],
        }];
        for (index, token) in disjoint.into_iter().enumerate() {
            let alternative_declaration =
                declaration(&mut grammar, &format!("Skipping::D{index}"), next_byte);
            next_byte += 1;
            let site_declaration = declaration(
                &mut grammar,
                &format!("Skipping::D{index}::token"),
                next_byte,
            );
            next_byte += 1;
            let site = grammar.push_site(ParseSite::new(
                skipping,
                token,
                site_declaration,
                ContainerShape::Direct,
            ));
            alternatives.push(crate::Alternative {
                declaration: alternative_declaration,
                sites: vec![site],
            });
        }
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Skipping",
                skipping_declaration,
                TypeShape::Enum { alternatives },
            )),
            skipping,
        );

        let owner = grammar.next_type_id();
        let owner_declaration = declaration(&mut grammar, "Owner", next_byte);
        next_byte += 1;
        let optional_declaration = declaration(&mut grammar, "Owner::optional", next_byte);
        next_byte += 1;
        let optional = grammar.push_site(ParseSite::new(
            owner,
            consuming,
            optional_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        let required_declaration = declaration(&mut grammar, "Owner::required", next_byte);
        let required = grammar.push_site(ParseSite::new(
            owner,
            skipping,
            required_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Owner",
                owner_declaration,
                TypeShape::Sequence {
                    sites: vec![optional, required],
                },
            )),
            owner,
        );
        grammar.entry_types = vec![owner];
        grammar
    }

    fn mixed_kind_optional_overlap_grammar() -> (GrammarDescription, SiteId, KindId, KindId) {
        let mut grammar =
            GrammarDescription::new("mixed-kind-optional", 4, 2, TextEquivalence::Sensitive);
        let a = token(&mut grammar, "A", 0);
        let b = token(&mut grammar, "B", 2);
        let c = token(&mut grammar, "C", 4);
        let _tail_only = token(&mut grammar, "TAIL_ONLY", 6);
        let element_kind = KindId::new(0);
        let tail_only_kind = KindId::new(3);
        let mixed_declaration = declaration(&mut grammar, "MIXED", 8);
        let mixed_atom = grammar.push_atom(AtomDescription::new(
            "MIXED",
            mixed_declaration,
            AtomExpression::Category(vec![AtomId::new(0), AtomId::new(3)]),
        ));
        let mixed = grammar.push_type(TypeDescription::new(
            "MIXED",
            mixed_declaration,
            TypeShape::Token {
                atom: mixed_atom,
                text_predicate: None,
            },
        ));

        let path = |grammar: &mut GrammarDescription,
                    name: &str,
                    first: TypeId,
                    second: TypeId,
                    byte: u32| {
            let path = grammar.next_type_id();
            let path_declaration = declaration(grammar, name, byte);
            let first_declaration = declaration(grammar, &format!("{name}::first"), byte + 1);
            let first = grammar.push_site(ParseSite::new(
                path,
                first,
                first_declaration,
                ContainerShape::Direct,
            ));
            let second_declaration = declaration(grammar, &format!("{name}::second"), byte + 2);
            let second = grammar.push_site(ParseSite::new(
                path,
                second,
                second_declaration,
                ContainerShape::Direct,
            ));
            assert_eq!(
                grammar.push_type(TypeDescription::new(
                    name,
                    path_declaration,
                    TypeShape::Sequence {
                        sites: vec![first, second],
                    },
                )),
                path,
            );
            path
        };
        let consuming = path(&mut grammar, "Consuming", a, b, 10);
        let skipping = path(&mut grammar, "Skipping", mixed, c, 14);
        let owner = grammar.next_type_id();
        let owner_declaration = declaration(&mut grammar, "Owner", 18);
        let optional_declaration = declaration(&mut grammar, "Owner::optional", 19);
        let optional = grammar.push_site(ParseSite::new(
            owner,
            consuming,
            optional_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        let required_declaration = declaration(&mut grammar, "Owner::required", 20);
        let required = grammar.push_site(ParseSite::new(
            owner,
            skipping,
            required_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Owner",
                owner_declaration,
                TypeShape::Sequence {
                    sites: vec![optional, required],
                },
            )),
            owner,
        );
        grammar.entry_types = vec![owner];
        (grammar, optional, element_kind, tail_only_kind)
    }

    fn nullable_tail_and_caller_optional_grammar() -> (GrammarDescription, SiteId) {
        let mut grammar =
            GrammarDescription::new("nullable-tail-caller", 5, 2, TextEquivalence::Sensitive);
        let a = token(&mut grammar, "A", 0);
        let b = token(&mut grammar, "B", 2);
        let c = token(&mut grammar, "C", 4);
        let d = token(&mut grammar, "D", 6);
        let x = token(&mut grammar, "X", 8);
        let path = |grammar: &mut GrammarDescription,
                    name: &str,
                    first: TypeId,
                    second: TypeId,
                    byte: u32| {
            let path = grammar.next_type_id();
            let path_declaration = declaration(grammar, name, byte);
            let first_declaration = declaration(grammar, &format!("{name}::a"), byte + 1);
            let first = grammar.push_site(ParseSite::new(
                path,
                first,
                first_declaration,
                ContainerShape::Direct,
            ));
            let second_declaration = declaration(grammar, &format!("{name}::tail"), byte + 2);
            let second = grammar.push_site(ParseSite::new(
                path,
                second,
                second_declaration,
                ContainerShape::Direct,
            ));
            assert_eq!(
                grammar.push_type(TypeDescription::new(
                    name,
                    path_declaration,
                    TypeShape::Sequence {
                        sites: vec![first, second],
                    },
                )),
                path,
            );
            path
        };
        let consuming_a = path(&mut grammar, "ConsumingA", a, b, 10);
        let consuming_x = path(&mut grammar, "ConsumingX", x, b, 14);
        let consuming = grammar.next_type_id();
        let consuming_declaration = declaration(&mut grammar, "Consuming", 18);
        let consuming_a_declaration = declaration(&mut grammar, "Consuming::A", 19);
        let consuming_a_site = grammar.push_site(ParseSite::new(
            consuming,
            consuming_a,
            consuming_a_declaration,
            ContainerShape::Direct,
        ));
        let consuming_x_declaration = declaration(&mut grammar, "Consuming::X", 20);
        let consuming_x_site = grammar.push_site(ParseSite::new(
            consuming,
            consuming_x,
            consuming_x_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Consuming",
                consuming_declaration,
                TypeShape::Enum {
                    alternatives: vec![
                        crate::Alternative {
                            declaration: consuming_a_declaration,
                            sites: vec![consuming_a_site],
                        },
                        crate::Alternative {
                            declaration: consuming_x_declaration,
                            sites: vec![consuming_x_site],
                        },
                    ],
                },
            )),
            consuming,
        );
        let skipping = path(&mut grammar, "Skipping", a, c, 22);
        let caller_tail = path(&mut grammar, "CallerTail", x, d, 26);

        let inner = grammar.next_type_id();
        let inner_declaration = declaration(&mut grammar, "Inner", 30);
        let first_declaration = declaration(&mut grammar, "Inner::first", 31);
        let first = grammar.push_site(ParseSite::new(
            inner,
            consuming,
            first_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        let second_declaration = declaration(&mut grammar, "Inner::second", 32);
        let second = grammar.push_site(ParseSite::new(
            inner,
            skipping,
            second_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Inner",
                inner_declaration,
                TypeShape::Sequence {
                    sites: vec![first, second],
                },
            )),
            inner,
        );

        let prefixed = grammar.next_type_id();
        let prefixed_declaration = declaration(&mut grammar, "Prefixed", 34);
        let prefix_declaration = declaration(&mut grammar, "Prefixed::prefix", 35);
        let prefix = grammar.push_site(ParseSite::new(
            prefixed,
            d,
            prefix_declaration,
            ContainerShape::Direct,
        ));
        let inner_site_declaration = declaration(&mut grammar, "Prefixed::inner", 36);
        let inner_site = grammar.push_site(ParseSite::new(
            prefixed,
            inner,
            inner_site_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Prefixed",
                prefixed_declaration,
                TypeShape::Sequence {
                    sites: vec![prefix, inner_site],
                },
            )),
            prefixed,
        );

        let outer = grammar.next_type_id();
        let outer_declaration = declaration(&mut grammar, "Outer", 38);
        let prefixed_site_declaration = declaration(&mut grammar, "Outer::prefixed", 39);
        let prefixed_site = grammar.push_site(ParseSite::new(
            outer,
            prefixed,
            prefixed_site_declaration,
            ContainerShape::Direct,
        ));
        let tail_declaration = declaration(&mut grammar, "Outer::tail", 40);
        let tail = grammar.push_site(ParseSite::new(
            outer,
            caller_tail,
            tail_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Outer",
                outer_declaration,
                TypeShape::Sequence {
                    sites: vec![prefixed_site, tail],
                },
            )),
            outer,
        );
        grammar.entry_types = vec![outer];
        (grammar, first)
    }

    /// `Cast = TIMESTAMP [Optional(Qualifier)]` directly followed by
    /// `UniqueKeys`, where `Qualifier = WITH TIME ZONE` and
    /// `UniqueKeys = WITH UNIQUE KEYS`.
    ///
    /// The optional element FIRST and the caller FOLLOW share exactly one kind
    /// (`WITH`) and no static tail exists, so only the second lookahead token
    /// (`TIME` versus `UNIQUE`) proves whether the optional is viable.
    fn suffix_proving_caller_overlap_optional_grammar() -> (GrammarDescription, SiteId) {
        caller_overlap_optional_grammar_at_lookahead(5)
    }

    /// The caller-overlap fixture at one configurable bounded depth.
    ///
    /// At depth 5 the element `WITH TIME ZONE` is a closed prefix language;
    /// at depth 2 its sequences truncate, so the element is open and the
    /// suffix-proving path must decline.
    fn caller_overlap_optional_grammar_at_lookahead(
        max_lookahead: usize,
    ) -> (GrammarDescription, SiteId) {
        let mut grammar = GrammarDescription::new(
            "caller-suffix-overlap",
            6,
            max_lookahead,
            TextEquivalence::Sensitive,
        );
        let with_tok = token(&mut grammar, "WITH", 0);
        let time_tok = token(&mut grammar, "TIME", 2);
        let zone_tok = token(&mut grammar, "ZONE", 4);
        let unique_tok = token(&mut grammar, "UNIQUE", 6);
        let keys_tok = token(&mut grammar, "KEYS", 8);
        let timestamp_tok = token(&mut grammar, "TIMESTAMP", 40);
        let triple = |grammar: &mut GrammarDescription,
                      name: &str,
                      first: TypeId,
                      second: TypeId,
                      third: TypeId,
                      byte: u32| {
            let sequence = grammar.next_type_id();
            let sequence_declaration = declaration(grammar, name, byte);
            let sites = [(first, "::a", 1), (second, "::b", 2), (third, "::c", 3)]
                .into_iter()
                .map(|(target, suffix, offset)| {
                    let site_declaration =
                        declaration(grammar, &format!("{name}{suffix}"), byte + offset);
                    grammar.push_site(ParseSite::new(
                        sequence,
                        target,
                        site_declaration,
                        ContainerShape::Direct,
                    ))
                })
                .collect::<Vec<_>>();
            assert_eq!(
                grammar.push_type(TypeDescription::new(
                    name,
                    sequence_declaration,
                    TypeShape::Sequence { sites },
                )),
                sequence,
            );
            sequence
        };
        let qualifier = triple(&mut grammar, "Qualifier", with_tok, time_tok, zone_tok, 10);
        let unique_keys = triple(
            &mut grammar,
            "UniqueKeys",
            with_tok,
            unique_tok,
            keys_tok,
            20,
        );
        let cast = grammar.next_type_id();
        let cast_declaration = declaration(&mut grammar, "Cast", 30);
        let base_declaration = declaration(&mut grammar, "Cast::base", 32);
        let base = grammar.push_site(ParseSite::new(
            cast,
            timestamp_tok,
            base_declaration,
            ContainerShape::Direct,
        ));
        let tz_declaration = declaration(&mut grammar, "Cast::tz", 31);
        let tz = grammar.push_site(ParseSite::new(
            cast,
            qualifier,
            tz_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Cast",
                cast_declaration,
                TypeShape::Sequence {
                    sites: vec![base, tz],
                },
            )),
            cast,
        );
        let outer = grammar.next_type_id();
        let outer_declaration = declaration(&mut grammar, "Outer", 34);
        let cast_site_declaration = declaration(&mut grammar, "Outer::cast", 35);
        let cast_site = grammar.push_site(ParseSite::new(
            outer,
            cast,
            cast_site_declaration,
            ContainerShape::Direct,
        ));
        let unique_site_declaration = declaration(&mut grammar, "Outer::unique", 36);
        let unique_site = grammar.push_site(ParseSite::new(
            outer,
            unique_keys,
            unique_site_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Outer",
                outer_declaration,
                TypeShape::Sequence {
                    sites: vec![cast_site, unique_site],
                },
            )),
            outer,
        );
        grammar.entry_types = vec![outer];
        (grammar, tz)
    }

    #[test]
    fn caller_follow_overlap_builds_a_suffix_proving_viability_decision() {
        let (grammar, optional) = suffix_proving_caller_overlap_optional_grammar();
        let report = crate::analyze(&grammar);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == crate::DiagnosticCode::NullableFollowOverlap),
            "a suffix-proving decision suppresses RCA0300 by consuming the frozen proof: {:?}",
            report.diagnostics,
        );
        let output = report.output.unwrap_or_else(|| {
            panic!(
                "caller overlap remains a warning rather than blocking generation: {:?}",
                report.diagnostics,
            )
        });
        let decision = output
            .decisions()
            .container_machines()
            .iter()
            .find(|machine| machine.site() == optional)
            .and_then(ContainerMachine::optional_viability_decision)
            .expect("a caller-FOLLOW-only overlap must freeze a suffix-proving decision");
        assert_eq!(
            decision.required_lookahead(),
            3,
            "the full element prefix WITH TIME ZONE proves viability",
        );
        let with_edge = decision
            .trie()
            .edges()
            .iter()
            .find(|edge| edge.position().kinds().contains(KindId::new(0)))
            .expect("the root branches on the shared WITH kind");
        let after_with = with_edge.next();
        assert_eq!(
            after_with.terminals(),
            [OptionalViabilityDecisionTerminal {
                action: OptionalAction::Skip,
                on_caller_follow: false,
            }],
            "input that diverges after WITH belongs to the caller",
        );
        let time_edge = after_with
            .edges()
            .iter()
            .find(|edge| edge.position().kinds().contains(KindId::new(1)))
            .expect("the WITH node extends only through the element's own TIME");
        let after_time = time_edge.next();
        assert_eq!(
            after_time.terminals(),
            [OptionalViabilityDecisionTerminal {
                action: OptionalAction::Skip,
                on_caller_follow: false,
            }],
            "input that diverges after WITH TIME belongs to the caller",
        );
        let zone_edge = after_time
            .edges()
            .iter()
            .find(|edge| edge.position().kinds().contains(KindId::new(2)))
            .expect("the WITH TIME node extends only through the element's own ZONE");
        let proven = zone_edge.next();
        assert_eq!(
            proven.terminals(),
            [OptionalViabilityDecisionTerminal {
                action: OptionalAction::Consume,
                on_caller_follow: false,
            }],
            "a fully matched element prefix consumes unconditionally",
        );
        assert!(
            proven.edges().is_empty(),
            "the proven prefix needs no further lookahead",
        );
    }

    #[test]
    fn open_element_languages_keep_the_greedy_legacy_selector() {
        let (grammar, optional) = caller_overlap_optional_grammar_at_lookahead(2);
        let report = crate::analyze(&grammar);
        let warning = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == crate::DiagnosticCode::NullableFollowOverlap)
            .expect("the caller overlap still warns when no decision can prove the suffix");
        let evidence = warning
            .lookahead
            .as_ref()
            .expect("a retained warning states that no proof exists within bounds");
        assert_eq!(evidence.configured_depth, grammar.max_lookahead);
        assert_eq!(
            evidence.sufficient_depth, None,
            "an open element admits no separating depth within the configured bound",
        );
        let output = report.output.unwrap_or_else(|| {
            panic!(
                "caller overlap remains a warning rather than blocking generation: {:?}",
                report.diagnostics,
            )
        });
        let machine = output
            .decisions()
            .container_machines()
            .iter()
            .find(|machine| machine.site() == optional)
            .expect("the fixture optional site has a container machine");
        assert!(
            machine.optional_viability_decision().is_none(),
            "an element whose bounded prefixes truncate must not divergence-skip",
        );
    }

    #[test]
    fn disjoint_optional_paths_use_the_existing_selector_without_a_viability_decision() {
        let grammar = disjoint_optional_grammar();
        OVERLAPPING_OPTIONAL_VIABILITY_CONSTRUCTIONS.set(0);
        let report = crate::analyze(&grammar);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let output = report
            .output
            .expect("the disjoint optional grammar is valid");
        let machine = output
            .decisions()
            .container_machines()
            .iter()
            .find(|machine| machine.operation() == ContainerOperation::Optional)
            .expect("the fixture contains one optional machine");
        assert!(
            machine.optional_viability_decision().is_none(),
            "disjoint FIRST and FOLLOW use the existing one-position selector",
        );
        assert_eq!(
            OVERLAPPING_OPTIONAL_VIABILITY_CONSTRUCTIONS.get(),
            0,
            "disjoint optionals must not materialize max-depth FIRST languages",
        );
    }

    #[test]
    fn publication_accepts_only_optional_omissions_covered_by_the_legacy_selector() {
        let grammar = branching_optional_overlap_grammar();
        let output = crate::analysis::analyze_for_generation(&grammar)
            .output
            .expect("the A B versus A C optional is decidable");
        let mut missing_required = ConstructedDecisionProducts(output.decisions().clone());
        let optional = missing_required
            .0
            .container_machines
            .iter_mut()
            .find(|machine| machine.operation == ContainerOperation::Optional)
            .expect("the fixture contains one optional machine");
        assert!(optional.optional_viability_decision.take().is_some());
        assert_eq!(
            missing_required.validate_for_publication(&grammar),
            Err(DecisionInvariant::InvalidOptionalViabilityDecision),
            "the kind-only selector cannot distinguish A B from A C",
        );

        let grammar = same_kind_disjoint_text_optional_grammar();
        let output = crate::analysis::analyze_for_generation(&grammar)
            .output
            .expect("the exact-text optional is decidable");
        let mut missing_refined = ConstructedDecisionProducts(output.decisions().clone());
        let optional = missing_refined
            .0
            .container_machines
            .iter_mut()
            .find(|machine| machine.operation == ContainerOperation::Optional)
            .expect("the fixture contains one optional machine");
        assert!(optional.optional_viability_decision.take().is_some());
        assert_eq!(
            missing_refined.validate_for_publication(&grammar),
            Err(DecisionInvariant::InvalidOptionalViabilityDecision),
            "the kind-only selector cannot preserve an exact-text partition",
        );

        let grammar = disjoint_optional_grammar();
        let output = crate::analysis::analyze_for_generation(&grammar)
            .output
            .expect("the disjoint optional is valid");
        assert!(
            output
                .decisions()
                .container_machines()
                .iter()
                .find(|machine| machine.operation() == ContainerOperation::Optional)
                .expect("the fixture contains one optional machine")
                .optional_viability_decision()
                .is_none(),
        );
        assert!(
            ConstructedDecisionProducts(output.decisions().clone())
                .validate_for_publication(&grammar)
                .is_ok(),
            "disjoint kind-only optionals are complete without a viability trie",
        );
    }

    #[test]
    fn same_kind_disjoint_text_paths_retain_an_exact_viability_decision() {
        let grammar = same_kind_disjoint_text_optional_grammar();
        OVERLAPPING_OPTIONAL_VIABILITY_CONSTRUCTIONS.set(0);
        let report = crate::analyze(&grammar);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let output = report
            .output
            .expect("the same-kind disjoint-text grammar is valid");
        let decision = output
            .decisions()
            .container_machines()
            .iter()
            .find_map(ContainerMachine::optional_viability_decision)
            .expect("the kind-only runtime selector needs an exact-text decision");

        assert_eq!(decision.required_lookahead(), 1);
        assert_eq!(decision.trie().edges().len(), 2);
        assert_eq!(
            OVERLAPPING_OPTIONAL_VIABILITY_CONSTRUCTIONS.get(),
            0,
            "disjoint text cells do not require symbolic suffix traversal",
        );
    }

    #[test]
    fn optional_viability_decisions_retain_static_and_caller_follow_skip_paths() {
        let grammar = static_and_caller_optional_grammar();
        let report = crate::analyze(&grammar);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let output = report
            .output
            .expect("the static-and-caller optional grammar is valid");
        let first = output
            .decisions()
            .container_machines()
            .first()
            .expect("the fixture has a first optional machine");
        assert!(matches!(first.stop(), FollowRecipe::StaticAndCaller { .. }));
        let decision = first
            .optional_viability_decision()
            .expect("the optional machine retains exact viability");
        assert_eq!(decision.required_lookahead(), 1);
        assert_eq!(
            decision.trie().terminals(),
            [OptionalViabilityDecisionTerminal {
                action: OptionalAction::Skip,
                on_caller_follow: true,
            }],
        );
        let actions = decision
            .trie()
            .edges()
            .iter()
            .map(|edge| {
                edge.next()
                    .terminals()
                    .first()
                    .expect("one matched token uniquely selects an action")
                    .action()
            })
            .collect::<Vec<_>>();
        assert_eq!(actions, [OptionalAction::Consume, OptionalAction::Skip]);
    }

    #[test]
    fn overlapping_optional_paths_cut_off_before_exponential_suffixes() {
        let grammar = branching_optional_overlap_grammar();
        crate::analysis::reset_first_sequence_peak_rows_for_test();
        crate::analysis::reset_optional_partition_candidates_for_test();
        let output = crate::analysis::analyze_for_generation(&grammar)
            .output
            .expect("the depth-two optional overlap is decidable");
        let decision = output
            .decisions()
            .container_machines()
            .iter()
            .find_map(ContainerMachine::optional_viability_decision)
            .expect("the fixture contains one optional decision");

        assert_eq!(decision.required_lookahead(), 2);
        let [shared_prefix] = decision.trie().edges() else {
            panic!("the overlap fixture has one shared first-position cell");
        };
        let [override_edge] = shared_prefix.next().edges() else {
            panic!("the unique consume branch must delegate to the legacy selector");
        };
        assert_eq!(
            override_edge
                .next()
                .terminals()
                .first()
                .expect("the retained branch is an explicit skip override")
                .action(),
            OptionalAction::Skip,
        );
        let (partition_candidates_before, partition_candidates_after) =
            crate::analysis::optional_partition_candidates_for_test();
        assert!(
            partition_candidates_after < partition_candidates_before,
            "legacy-equivalent branches must be discarded before exact partitioning: \
             before={partition_candidates_before}, after={partition_candidates_after}",
        );
        assert!(
            crate::analysis::first_sequence_peak_rows_for_test() <= 8,
            "optional viability must not eagerly enumerate the correlated suffix language",
        );
    }

    #[test]
    fn optional_override_partition_discards_wide_disjoint_tail_frontiers() {
        const DISJOINT_WIDTH: usize = 32;
        let grammar = wide_disjoint_tail_optional_overlap_grammar(DISJOINT_WIDTH);
        crate::analysis::reset_optional_partition_candidates_for_test();
        let output = crate::analysis::analyze_for_generation(&grammar)
            .output
            .expect("the depth-two overlap beside a wide disjoint tail is decidable");
        let decision = output
            .decisions()
            .container_machines()
            .iter()
            .find_map(ContainerMachine::optional_viability_decision)
            .expect("the exact A-prefix overlap retains one sparse override");
        let (before, after) = crate::analysis::optional_partition_candidates_for_test();

        assert!(
            before.saturating_sub(after) >= DISJOINT_WIDTH,
            "wide disjoint FIRST branches must be removed before exact partitioning: \
             before={before}, after={after}",
        );
        assert_eq!(decision.required_lookahead(), 2);
        assert_eq!(decision.trie().edges().len(), 1);
        assert_eq!(decision.trie().edges()[0].next().edges().len(), 1);
    }

    #[test]
    fn optional_override_partition_splits_mixed_root_kind_sets_by_selector() {
        let (grammar, optional, element_kind, tail_only_kind) =
            mixed_kind_optional_overlap_grammar();
        let report = crate::analyze(&grammar);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let output = report.output.expect("the mixed-kind overlap is decidable");
        let decision = output
            .decisions()
            .container_machines()
            .iter()
            .find(|machine| machine.site() == optional)
            .and_then(ContainerMachine::optional_viability_decision)
            .expect("the shared element kind retains one sparse override");
        let [root] = decision.trie().edges() else {
            panic!("only the selector-accepted root partition remains");
        };

        assert!(root.position().kinds().contains(element_kind));
        assert!(
            !root.position().kinds().contains(tail_only_kind),
            "the selector-rejected half of a mixed tail position delegates to fallback Skip",
        );
    }

    #[test]
    fn optional_override_edges_shield_caller_terminals_before_fallback() {
        let (grammar, optional) = nullable_tail_and_caller_optional_grammar();
        let report = crate::analyze(&grammar);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == crate::DiagnosticCode::NullableFollowOverlap),
            "same-cell caller provenance must retain RCA0300",
        );
        let output = report.output.unwrap_or_else(|| {
            panic!(
                "caller overlap remains a warning rather than blocking generation: {:?}",
                report.diagnostics,
            )
        });
        let decision = output
            .decisions()
            .container_machines()
            .iter()
            .find(|machine| machine.site() == optional)
            .and_then(ContainerMachine::optional_viability_decision)
            .expect("the static A-prefix overlap retains exact viability");
        assert_eq!(
            decision.trie().terminals(),
            [OptionalViabilityDecisionTerminal {
                action: OptionalAction::Skip,
                on_caller_follow: true,
            }],
        );
        let shared = decision
            .trie()
            .edges()
            .iter()
            .find(|edge| edge.position().kinds().contains(KindId::new(0)))
            .expect("the optional paths share one A prefix");
        let consume_delegate = decision
            .trie()
            .edges()
            .iter()
            .find(|edge| edge.position().kinds().contains(KindId::new(4)))
            .expect("the root X branch shields the caller terminal");
        assert!(consume_delegate.next().terminals().is_empty());
        assert!(consume_delegate.next().edges().is_empty());
        let skip_override = shared
            .next()
            .edges()
            .iter()
            .find(|edge| edge.position().kinds().contains(KindId::new(2)))
            .expect("the C branch remains an explicit Skip override");
        assert_eq!(
            skip_override.next().terminals(),
            [OptionalViabilityDecisionTerminal {
                action: OptionalAction::Skip,
                on_caller_follow: false,
            }],
        );
    }

    #[test]
    fn equivalent_overlapping_optional_sites_share_one_symbolic_freeze() {
        let grammar = branching_optional_overlap_grammar_with_owners(1);
        crate::analysis::reset_decision_frontier_computations_for_test();
        crate::analysis::analyze_for_generation(&grammar)
            .output
            .expect("the single-owner overlap is decidable");
        let one_owner_builds = crate::analysis::decision_frontier_computations_for_test();

        let grammar = branching_optional_overlap_grammar_with_owners(8);
        crate::analysis::reset_decision_frontier_computations_for_test();
        let output = crate::analysis::analyze_for_generation(&grammar)
            .output
            .expect("the equivalent multi-owner overlaps are decidable");
        let eight_owner_builds = crate::analysis::decision_frontier_computations_for_test();

        assert_eq!(
            output
                .decisions()
                .container_machines()
                .iter()
                .filter_map(ContainerMachine::optional_viability_decision)
                .count(),
            8,
        );
        assert!(
            one_owner_builds > 0,
            "the control must observe real decision-frontier computation",
        );
        assert_eq!(
            eight_owner_builds, one_owner_builds,
            "equivalent optional roots must reuse the symbolic semantic frontier",
        );
    }

    #[test]
    fn canonical_facts_expose_balanced_dispatch_and_post_close_mutations() {
        let grammar = pratt_and_repeat_grammar();
        let post_close = DispatchTrieNode {
            terminal: None,
            edges: vec![DispatchTrieEdge {
                position: DispatchPosition {
                    kinds: KindSet::singleton(grammar.kind_count, KindId::new(2)).unwrap(),
                    text_predicate: None,
                },
                next: DispatchTrieNode {
                    terminal: Some(DispatchTerminal {
                        declaration: DeclId::new(0),
                        mode: DispatchTerminalMode::Immediate,
                    }),
                    edges: Arc::default(),
                },
            }]
            .into(),
        };
        let mut products = DecisionProducts {
            dispatches: vec![DispatchPlan {
                owner: TypeId::new(0),
                kind: DispatchKind::Enum,
                branches: Vec::new(),
                required_lookahead: 0,
                on_caller_follow: None,
                trie: DispatchTrieNode::default(),
                balanced: Some(BalancedDispatch {
                    open: KindId::new(0),
                    close: KindId::new(1),
                    after_close: post_close,
                }),
            }],
            follow_recipes_by_site: Vec::new(),
            container_machines: Vec::new(),
            failure_edges: Vec::new(),
            greedy_pratt_boundary_by_site: Vec::new(),
        };

        let original = products.canonical_facts(&grammar);
        assert!(
            original.contains(
                "balanced open=NUMBER close=MINUS after-close=(terminal=- edges=[PLUS=>(terminal=NUMBER edges=[])])"
            ),
            "canonical facts must expose the complete balanced decision:\n{original}",
        );

        let balanced = products.dispatches[0]
            .balanced
            .as_mut()
            .expect("fixture has balanced dispatch");
        let mut edges = balanced.after_close.edges.to_vec();
        edges[0].next.terminal = Some(DispatchTerminal {
            declaration: DeclId::new(1),
            mode: DispatchTerminalMode::CallerFollow,
        });
        balanced.after_close.edges = edges.into();

        let mutated = products.canonical_facts(&grammar);
        assert_ne!(
            mutated, original,
            "post-close trie changes must remain visible to semantic projections",
        );
        assert!(mutated.contains("terminal=MINUS@caller-follow"));
    }

    #[test]
    fn required_lookahead_stops_after_reaching_the_configured_maximum() {
        let sequence = DispatchSequence {
            positions: Vec::new(),
            on_caller_follow: false,
        };
        let branches = [
            DispatchBranch {
                declaration: DeclId::new(0),
                position: None,
                sequences: vec![sequence.clone(), sequence.clone()],
            },
            DispatchBranch {
                declaration: DeclId::new(1),
                position: None,
                sequences: vec![sequence.clone(), sequence],
            },
        ];
        let mut comparisons = 0;

        let required = required_dispatch_lookahead_with(&branches, 4, |_, _| {
            comparisons += 1;
            assert_eq!(comparisons, 1, "saturated comparisons must stop");
            Some(4)
        });

        assert_eq!(required, 4);
        assert_eq!(comparisons, 1);
    }

    #[test]
    fn predicated_mixed_opener_cell_remains_unsupported() {
        let grammar = pratt_and_repeat_grammar();
        let open = KindId::new(0);
        let mixed = DispatchPosition {
            kinds: KindSet::from_kinds(grammar.kinds.len(), [open, KindId::new(1)]).unwrap(),
            text_predicate: Some(TextPredicate::Exact("OPEN".to_owned())),
        };
        let branches = [DispatchBranch {
            declaration: DeclId::new(0),
            position: None,
            sequences: vec![DispatchSequence {
                positions: vec![mixed],
                on_caller_follow: false,
            }],
        }];

        assert!(
            ordinary_branches_without_balanced_opener(&branches, open, &grammar).is_none(),
            "a text predicate over a mixed opener cell cannot be split by kind alone",
        );
    }

    #[test]
    fn required_lookahead_follows_partitioned_states_instead_of_sequence_pairs() {
        let grammar = pratt_and_repeat_grammar();
        let position = |kind| DispatchPosition {
            kinds: KindSet::singleton(grammar.kinds.len(), KindId::new(kind)).unwrap(),
            text_predicate: None,
        };
        let left = DispatchSequence {
            positions: vec![position(0), position(1)],
            on_caller_follow: false,
        };
        let right = DispatchSequence {
            positions: vec![position(0), position(2)],
            on_caller_follow: false,
        };
        let branches = [
            DispatchBranch {
                declaration: DeclId::new(0),
                position: None,
                sequences: vec![left; 64],
            },
            DispatchBranch {
                declaration: DeclId::new(1),
                position: None,
                sequences: vec![right; 64],
            },
        ];
        let mut states = 0;
        let mut maximum_state_width = 0;

        let (_, required) = freeze_dispatch_trie_with_observer(&branches, &grammar, |state| {
            states += 1;
            maximum_state_width = maximum_state_width.max(state.len());
        });

        assert_eq!(required, 2);
        assert!(states <= 4, "partitioning visited {states} residual states");
        assert!(
            maximum_state_width <= 2,
            "equivalent dispatch suffixes must share one residual identity; width={maximum_state_width}",
        );
    }

    #[test]
    fn compact_rows_keep_ascii_insensitive_shared_heads_and_two_token_selection_exact() {
        fn predicated_token(
            grammar: &mut GrammarDescription,
            atom: crate::AtomId,
            name: &str,
            spelling: &str,
            byte: u32,
        ) -> TypeId {
            let declaration = declaration(grammar, name, byte);
            let text_predicate =
                grammar.push_text_predicate(TextPredicate::Exact(spelling.to_owned()));
            grammar.push_type(TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: Some(text_predicate),
                },
            ))
        }

        let mut grammar = GrammarDescription::new(
            "exact-owned-dispatch-rows",
            1,
            2,
            TextEquivalence::AsciiInsensitive,
        );
        let word_declaration = declaration(&mut grammar, "WORD", 0);
        let word = grammar.push_kind(KindDescription::new("WORD", word_declaration));
        let atom_declaration = declaration(&mut grammar, "WordAtom", 2);
        let atom = grammar.push_atom(AtomDescription::new(
            "WordAtom",
            atom_declaration,
            AtomExpression::Kind(word),
        ));
        let upper_head = predicated_token(&mut grammar, atom, "UpperHead", "HEAD", 4);
        let lower_head = predicated_token(&mut grammar, atom, "LowerHead", "head", 6);
        let left_tail = predicated_token(&mut grammar, atom, "LeftTail", "LEFT", 8);
        let right_tail = predicated_token(&mut grammar, atom, "RightTail", "RIGHT", 10);

        let choice = grammar.next_type_id();
        let choice_declaration = declaration(&mut grammar, "Choice", 12);
        let left_declaration = declaration(&mut grammar, "Choice::Left", 14);
        let right_declaration = declaration(&mut grammar, "Choice::Right", 16);
        let mut next_byte = 18;
        let mut alternative =
            |branch_declaration, name: &str, targets: [TypeId; 2]| crate::Alternative {
                declaration: branch_declaration,
                sites: targets
                    .into_iter()
                    .map(|target| {
                        let site_declaration = declaration(&mut grammar, name, next_byte);
                        next_byte += 2;
                        grammar.push_site(ParseSite::new(
                            choice,
                            target,
                            site_declaration,
                            ContainerShape::Direct,
                        ))
                    })
                    .collect(),
            };
        let alternatives = vec![
            alternative(
                left_declaration,
                "Choice::Left::site",
                [upper_head, left_tail],
            ),
            alternative(
                right_declaration,
                "Choice::Right::site",
                [lower_head, right_tail],
            ),
        ];
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum {
                    alternatives: alternatives.clone(),
                },
            )),
            choice,
        );

        let first_sequences = FirstSequenceAnalysis::for_grammar(&grammar);
        let plan = dispatch_plan(
            choice,
            DispatchKind::Enum,
            alternatives
                .iter()
                .map(|alternative| (alternative.declaration, None, alternative.sites.as_slice())),
            &grammar,
            &first_sequences,
        );
        assert_eq!(plan.required_lookahead(), 2);
        let rows = plan
            .branches()
            .iter()
            .map(|branch| {
                assert_eq!(
                    branch.sequences().len(),
                    1,
                    "each branch has one exact selecting rectangle",
                );
                let sequence = &branch.sequences()[0];
                assert_eq!(
                    sequence.positions().len(),
                    2,
                    "the shared exact-text head alone must never be published as selecting",
                );
                assert_eq!(
                    sequence.positions()[0].text_predicate(),
                    Some(&TextPredicate::Exact("head".to_owned())),
                    "ASCII-equivalent authored heads share one canonical exact cell",
                );
                sequence
            })
            .collect::<Vec<_>>();
        assert!(
            !sequences_overlap(rows[0], rows[1], &grammar),
            "the owned two-token rows must be truthful disjoint selecting prefixes",
        );
    }

    #[test]
    fn pratt_led_dispatch_commits_after_its_operator_prefix_is_unique() {
        let grammar = pratt_and_repeat_grammar();
        let report = crate::analyze(&grammar);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let output = report.output.expect("the fixture grammar is valid");
        let led = output
            .decisions()
            .dispatches()
            .iter()
            .find(|dispatch| dispatch.kind() == DispatchKind::PrattLed)
            .expect("the Pratt fixture has an LED dispatch");
        let [operator] = led.trie().edges() else {
            panic!("the sole LED has one operator route");
        };
        let terminal = operator
            .next()
            .terminal()
            .expect("the unique operator prefix commits its LED");
        assert_eq!(
            grammar.declarations[terminal.declaration().index()].name,
            "Expr::Add",
        );
        assert_eq!(terminal.mode(), DispatchTerminalMode::Immediate);
        assert!(
            operator.next().edges().is_empty(),
            "recursive RHS lookahead must not participate in LED selection",
        );
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
        let mut edges = wrong_trie.dispatches[0]
            .trie
            .edges
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        edges.pop();
        wrong_trie.dispatches[0].trie.edges = edges.into();
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
