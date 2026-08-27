//! Frozen, source-independent parse decisions owned by semantic analysis.
//!
//! These products deliberately retain semantic IDs. source analysis joins them to source
//! identities; generation renders them without recomputing FIRST, FOLLOW, container,
//! Pratt, or failure behavior.

use std::cmp::Ordering;
use std::fmt::Write as _;

use crate::analysis::{FirstFacts, first_sequences_for_decision_sites};
use crate::ir::RepeatSeparator;
use crate::{
    DeclId, FirstPosition, FirstSequence, GrammarDescription, PrattPosition, SiteId, TypeId,
    TypeShape,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DispatchKind {
    Enum,
    PrattNud,
    PrattLed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchSequence {
    positions: Vec<FirstPosition>,
    on_caller_follow: bool,
}

impl DispatchSequence {
    pub fn positions(&self) -> &[FirstPosition] {
        &self.positions
    }

    pub const fn on_caller_follow(&self) -> bool {
        self.on_caller_follow
    }
}

/// One terminal choice in a frozen dispatch trie.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchTerminal {
    declaration: DeclId,
    on_caller_follow: bool,
}

impl DispatchTerminal {
    pub const fn declaration(self) -> DeclId {
        self.declaration
    }

    pub const fn on_caller_follow(self) -> bool {
        self.on_caller_follow
    }
}

/// One already-discriminated edge in a frozen dispatch trie.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchTrieEdge {
    position: FirstPosition,
    next: DispatchTrieNode,
}

impl DispatchTrieEdge {
    pub const fn position(&self) -> &FirstPosition {
        &self.position
    }

    pub const fn next(&self) -> &DispatchTrieNode {
        &self.next
    }
}

/// A frozen dispatch-trie node produced by semantic analysis.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DispatchTrieNode {
    terminal: Option<DispatchTerminal>,
    edges: Vec<DispatchTrieEdge>,
}

impl DispatchTrieNode {
    pub const fn terminal(&self) -> Option<DispatchTerminal> {
        self.terminal
    }

    pub fn edges(&self) -> &[DispatchTrieEdge] {
        &self.edges
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchBranch {
    declaration: DeclId,
    position: Option<PrattPosition>,
    sequences: Vec<DispatchSequence>,
}

impl DispatchBranch {
    pub const fn declaration(&self) -> DeclId {
        self.declaration
    }

    pub fn sequences(&self) -> &[DispatchSequence] {
        &self.sequences
    }

    /// Pratt position and exact binding powers. Enum branches return `None`.
    pub const fn position(&self) -> Option<PrattPosition> {
        self.position
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchPlan {
    owner: TypeId,
    kind: DispatchKind,
    branches: Vec<DispatchBranch>,
    required_lookahead: usize,
    on_caller_follow: Option<DeclId>,
    trie: DispatchTrieNode,
}

impl DispatchPlan {
    pub const fn owner(&self) -> TypeId {
        self.owner
    }

    pub const fn kind(&self) -> DispatchKind {
        self.kind
    }

    pub fn branches(&self) -> &[DispatchBranch] {
        &self.branches
    }

    pub const fn required_lookahead(&self) -> usize {
        self.required_lookahead
    }

    pub const fn on_caller_follow(&self) -> Option<DeclId> {
        self.on_caller_follow
    }

    pub const fn trie(&self) -> &DispatchTrieNode {
        &self.trie
    }

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
pub enum FollowRecipe {
    Caller,
    Static { first: Vec<FirstPosition> },
    StaticAndCaller { first: Vec<FirstPosition> },
}

impl FollowRecipe {
    pub fn static_first(&self) -> &[FirstPosition] {
        match self {
            Self::Caller => &[],
            Self::Static { first } | Self::StaticAndCaller { first } => first,
        }
    }

    pub const fn uses_caller(&self) -> bool {
        matches!(self, Self::Caller | Self::StaticAndCaller { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContainerOperation {
    Direct,
    Optional,
    ZeroOrMore,
    OneOrMore,
    OptionalOneOrMore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerMachine {
    site: SiteId,
    operation: ContainerOperation,
    separator: Option<RepeatSeparator>,
    element_first: Vec<FirstPosition>,
    separator_first: Vec<FirstPosition>,
    stop: FollowRecipe,
    transitions: Vec<ContainerTransition>,
}

impl ContainerMachine {
    pub const fn site(&self) -> SiteId {
        self.site
    }

    pub const fn operation(&self) -> ContainerOperation {
        self.operation
    }

    pub const fn separator(&self) -> Option<RepeatSeparator> {
        self.separator
    }

    pub const fn optional_trailing(&self) -> bool {
        match self.separator {
            Some(separator) => separator.optional_trailing,
            None => false,
        }
    }

    pub fn element_first(&self) -> &[FirstPosition] {
        &self.element_first
    }

    pub fn separator_first(&self) -> &[FirstPosition] {
        &self.separator_first
    }

    pub const fn stop(&self) -> &FollowRecipe {
        &self.stop
    }

    pub fn transitions(&self) -> &[ContainerTransition] {
        &self.transitions
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContainerState {
    Start,
    AfterElement,
    AfterSeparator,
    Done,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContainerAction {
    ParseElement,
    ParseSeparator,
    Stop,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerTransition {
    from: ContainerState,
    to: ContainerState,
    action: ContainerAction,
    selector: ExpectedRecipe,
}

impl ContainerTransition {
    pub const fn from(&self) -> ContainerState {
        self.from
    }

    pub const fn to(&self) -> ContainerState {
        self.to
    }

    pub const fn action(&self) -> ContainerAction {
        self.action
    }

    pub const fn selector(&self) -> &ExpectedRecipe {
        &self.selector
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FailureEdgeKind {
    RequiredSite,
    EnumDispatch,
    PrattNudDispatch,
    PrattLedDispatch,
    PrattCallerFollowGuard,
    ContainerMinimum,
    ContainerContinuation,
    ContainerAfterSeparator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpectedRecipe {
    Static(Vec<FirstPosition>),
    Follow(FollowRecipe),
    ContinueOrStop {
        continuation: Vec<FirstPosition>,
        stop: FollowRecipe,
    },
    PrattExtendersAtOrAboveMinBp {
        branches: Vec<DispatchBranch>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureEdge {
    owner: TypeId,
    site: Option<SiteId>,
    kind: FailureEdgeKind,
    expected: ExpectedRecipe,
}

impl FailureEdge {
    pub const fn owner(&self) -> TypeId {
        self.owner
    }

    pub const fn site(&self) -> Option<SiteId> {
        self.site
    }

    pub const fn kind(&self) -> FailureEdgeKind {
        self.kind
    }

    pub const fn expected(&self) -> &ExpectedRecipe {
        &self.expected
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionProducts {
    dispatches: Vec<DispatchPlan>,
    follow_recipes_by_site: Vec<FollowRecipe>,
    container_machines: Vec<ContainerMachine>,
    failure_edges: Vec<FailureEdge>,
}

impl DecisionProducts {
    pub fn dispatches(&self) -> &[DispatchPlan] {
        &self.dispatches
    }

    pub fn follow_recipes_by_site(&self) -> &[FollowRecipe] {
        &self.follow_recipes_by_site
    }

    pub fn container_machines(&self) -> &[ContainerMachine] {
        &self.container_machines
    }

    pub fn failure_edges(&self) -> &[FailureEdge] {
        &self.failure_edges
    }

    #[cfg(test)]
    pub(crate) fn corrupt_site_product_for_test(&mut self) {
        self.follow_recipes_by_site.push(FollowRecipe::Caller);
    }

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

    pub(crate) fn validate(
        &self,
        grammar: &GrammarDescription,
        semantic: &[FirstFacts],
    ) -> Result<(), DecisionInvariant> {
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
pub(crate) enum DecisionInvariant {
    MissingSiteProduct,
    MissingDispatch,
    OverlappingDispatch,
    EmptyExpectedRecipe,
    MissingPrattLedGuard,
    DuplicateSiteProduct,
    DuplicateSiteUse,
    MultipleCallerFallbacks,
    DuplicateFailureEdge,
    FollowRecipeMismatch,
    DispatchProductMismatch,
    ContainerProductMismatch,
    FailureProductMismatch,
}

impl std::fmt::Display for DecisionInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

pub(crate) fn plan_decisions(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
) -> DecisionProducts {
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
    let trie = freeze_dispatch_trie(&branches);
    DispatchPlan {
        owner,
        kind,
        branches,
        required_lookahead,
        on_caller_follow,
        trie,
    }
}

fn freeze_dispatch_trie(branches: &[DispatchBranch]) -> DispatchTrieNode {
    let mut root = DispatchTrieNode::default();
    for branch in branches {
        'sequence: for sequence in &branch.sequences {
            let mut node = &mut root;
            for position in &sequence.positions {
                if let Some(terminal) = node.terminal.filter(|terminal| !terminal.on_caller_follow)
                {
                    assert_eq!(
                        terminal.declaration, branch.declaration,
                        "validated dispatch cannot extend another branch's terminal"
                    );
                    continue 'sequence;
                }
                let edge_index = node
                    .edges
                    .iter()
                    .position(|edge| edge.position == *position);
                let edge = if let Some(index) = edge_index {
                    &mut node.edges[index]
                } else {
                    node.edges.push(DispatchTrieEdge {
                        position: position.clone(),
                        next: DispatchTrieNode::default(),
                    });
                    node.edges.last_mut().expect("the inserted edge exists")
                };
                node = &mut edge.next;
            }
            let terminal = DispatchTerminal {
                declaration: branch.declaration,
                on_caller_follow: sequence.on_caller_follow,
            };
            match &mut node.terminal {
                Some(existing) => {
                    assert_eq!(
                        existing.declaration, terminal.declaration,
                        "validated dispatch cannot map one prefix to two terminals"
                    );
                    if !terminal.on_caller_follow {
                        assert!(
                            node.edges.iter().all(|edge| trie_targets_declaration(
                                &edge.next,
                                terminal.declaration
                            )),
                            "validated dispatch cannot shadow another branch"
                        );
                        existing.on_caller_follow = false;
                        node.edges.clear();
                    }
                }
                None => {
                    if !terminal.on_caller_follow {
                        assert!(
                            node.edges.iter().all(|edge| trie_targets_declaration(
                                &edge.next,
                                terminal.declaration
                            )),
                            "validated dispatch cannot shadow another branch"
                        );
                        node.edges.clear();
                    }
                    node.terminal = Some(terminal);
                }
            }
        }
    }
    root
}

fn trie_targets_declaration(node: &DispatchTrieNode, declaration: DeclId) -> bool {
    node.terminal
        .is_none_or(|terminal| terminal.declaration == declaration)
        && node
            .edges
            .iter()
            .all(|edge| trie_targets_declaration(&edge.next, declaration))
}

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

fn plan_failure_edges(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    dispatches: &[DispatchPlan],
    follow_recipes: &[FollowRecipe],
) -> Vec<FailureEdge> {
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

fn sort_dedup_positions(positions: &mut Vec<FirstPosition>) {
    positions.sort_by(compare_positions);
    positions.dedup();
}

fn compare_positions(left: &FirstPosition, right: &FirstPosition) -> Ordering {
    left.kinds
        .kind_count()
        .cmp(&right.kinds.kind_count())
        .then_with(|| left.kinds.words().cmp(right.kinds.words()))
        .then_with(|| left.text_predicate.cmp(&right.text_predicate))
}

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

const fn dispatch_name(kind: DispatchKind) -> &'static str {
    match kind {
        DispatchKind::Enum => "enum",
        DispatchKind::PrattNud => "pratt-nud",
        DispatchKind::PrattLed => "pratt-led",
    }
}

const fn follow_name(recipe: &FollowRecipe) -> &'static str {
    match recipe {
        FollowRecipe::Caller => "caller",
        FollowRecipe::Static { .. } => "static",
        FollowRecipe::StaticAndCaller { .. } => "static+caller",
    }
}

const fn is_nud(position: PrattPosition) -> bool {
    matches!(position, PrattPosition::Atom | PrattPosition::Prefix { .. })
}

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
