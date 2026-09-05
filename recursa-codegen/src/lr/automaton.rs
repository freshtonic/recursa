#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! LR(0) item sets, LALR(1) lookaheads and conflict detection.
//!
//! The construction is the one bison uses: LR(0) states from every start rule, then lookaheads
//! by DeRemer and Pennello's `reads`, `includes` and `lookback` relations over the digraph
//! algorithm. Shift/reduce conflicts go through a [`ConflictResolver`] seam, which binding-power
//! precedence implements; a resolved conflict is recorded, not reported. Without a resolution a
//! shift/reduce conflict keeps the shift and a reduce/reduce conflict keeps the lowest rule, as
//! bison does, and both are reported.
//!
//! Prefix acceptance shapes two details. The synthetic start rule `S'(T) -> T` has the end marker
//! as its only lookahead, and the end marker means "no other action applies" at runtime; the
//! state's default reduction therefore prefers the reduce whose lookahead holds the end marker,
//! so a root ends when the next token has no shift action, as the design's prefix acceptance
//! requires.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;

use super::grammar::{LrGrammar, NonterminalId, RuleId, Symbol, TerminalId};
use crate::TypeId;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense automaton state identity in construction order; start states come first.
pub struct StateId(u32);

impl StateId {
    /// Creates a state identity from its dense index.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Returns the identity as a collection index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One LR(0) item: a rule with a dot position in its right-hand side.
pub struct Item {
    /// The rule.
    pub rule: RuleId,
    /// How many right-hand-side symbols precede the dot.
    pub dot: u32,
}

impl Item {
    /// Returns the symbol after the dot, or `None` for a complete item.
    pub fn next_symbol(self, grammar: &LrGrammar) -> Option<Symbol> {
        grammar.rule(self.rule).rhs.get(self.dot as usize).copied()
    }

    /// Reports whether the dot is at the end of the rule.
    pub fn is_complete(self, grammar: &LrGrammar) -> bool {
        self.next_symbol(grammar).is_none()
    }

    /// Renders the item bison-style, for example `S -> i S . e S`.
    pub fn render(self, grammar: &LrGrammar) -> String {
        let rule = grammar.rule(self.rule);
        let mut text = grammar.nonterminal(rule.lhs).name.clone();
        text.push_str(" ->");
        for (index, symbol) in rule.rhs.iter().enumerate() {
            if index == self.dot as usize {
                text.push_str(" .");
            }
            text.push(' ');
            text.push_str(grammar.symbol_name(*symbol));
        }
        if self.dot as usize == rule.rhs.len() {
            text.push_str(" .");
        }
        text
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
/// A set of terminals as a fixed-width bitset over the grammar's terminals.
pub struct TerminalSet {
    /// Bit words, least significant bit first.
    words: Vec<u64>,
    /// Number of terminals the set ranges over.
    width: usize,
}

impl TerminalSet {
    /// Creates an empty set over `width` terminals.
    pub fn empty(width: usize) -> Self {
        Self {
            words: vec![0; width.div_ceil(64)],
            width,
        }
    }

    /// Adds one terminal.
    pub fn insert(&mut self, terminal: TerminalId) {
        debug_assert!(terminal.index() < self.width);
        self.words[terminal.index() / 64] |= 1 << (terminal.index() % 64);
    }

    /// Reports whether the set holds a terminal.
    pub fn contains(&self, terminal: TerminalId) -> bool {
        self.words[terminal.index() / 64] & (1 << (terminal.index() % 64)) != 0
    }

    /// Reports whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    /// Adds every terminal of another set of the same width; returns whether anything changed.
    pub fn union_assign(&mut self, other: &Self) -> bool {
        debug_assert_eq!(self.width, other.width);
        let mut changed = false;
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            let merged = *left | *right;
            changed |= merged != *left;
            *left = merged;
        }
        changed
    }

    /// Reports whether the two sets share a terminal.
    pub fn intersects(&self, other: &Self) -> bool {
        self.words
            .iter()
            .zip(&other.words)
            .any(|(left, right)| left & right != 0)
    }

    /// Iterates member terminals in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = TerminalId> + '_ {
        (0..self.width)
            .map(|index| TerminalId::new(index as u32))
            .filter(move |terminal| self.contains(*terminal))
    }

    /// Counts the members.
    pub fn len(&self) -> usize {
        self.words.iter().map(|word| word.count_ones() as usize).sum()
    }
}

impl fmt::Debug for TerminalSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.iter()).finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A resolved parser action on one lookahead.
pub enum Action {
    /// Shift the lookahead and go to the state.
    Shift(StateId),
    /// Reduce by the rule; reducing a start rule accepts the parse.
    Reduce(RuleId),
    /// An explicit syntax error, as a non-associative precedence produces.
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One complete item of a state together with its LALR(1) lookahead set.
pub struct Reduction {
    /// The complete rule.
    pub rule: RuleId,
    /// The terminals on which the rule reduces.
    pub lookaheads: TerminalSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One automaton state after lookahead computation and conflict resolution.
pub struct State {
    /// The state's identity.
    id: StateId,
    /// Kernel items in ascending order; the closure is recomputed on demand.
    kernel: Vec<Item>,
    /// Kernel plus closure items in ascending order.
    items: Vec<Item>,
    /// Transitions in ascending symbol order: terminals first, then nonterminals.
    transitions: Vec<(Symbol, StateId)>,
    /// Complete items with their lookaheads, in ascending rule order.
    reductions: Vec<Reduction>,
    /// The resolved action on every terminal that has one, in ascending terminal order.
    actions: BTreeMap<TerminalId, Action>,
    /// The reduce taken when the lookahead has no explicit action; see the module notes.
    default_reduction: Option<RuleId>,
}

impl State {
    /// Returns the state's identity.
    pub fn id(&self) -> StateId {
        self.id
    }

    /// Returns the kernel items.
    pub fn kernel(&self) -> &[Item] {
        &self.kernel
    }

    /// Returns kernel plus closure items.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Returns the transitions in ascending symbol order.
    pub fn transitions(&self) -> &[(Symbol, StateId)] {
        &self.transitions
    }

    /// Returns the terminal transitions.
    pub fn shifts(&self) -> impl Iterator<Item = (TerminalId, StateId)> + '_ {
        self.transitions.iter().filter_map(|(symbol, target)| match symbol {
            Symbol::Terminal(terminal) => Some((*terminal, *target)),
            Symbol::Nonterminal(_) => None,
        })
    }

    /// Returns the nonterminal transitions.
    pub fn gotos(&self) -> impl Iterator<Item = (NonterminalId, StateId)> + '_ {
        self.transitions.iter().filter_map(|(symbol, target)| match symbol {
            Symbol::Nonterminal(nonterminal) => Some((*nonterminal, *target)),
            Symbol::Terminal(_) => None,
        })
    }

    /// Returns the complete items with their lookaheads.
    pub fn reductions(&self) -> &[Reduction] {
        &self.reductions
    }

    /// Returns the resolved actions.
    pub fn actions(&self) -> &BTreeMap<TerminalId, Action> {
        &self.actions
    }

    /// Returns the default reduction.
    pub fn default_reduction(&self) -> Option<RuleId> {
        self.default_reduction
    }

    /// Returns the target of the transition on a symbol.
    pub fn transition(&self, symbol: Symbol) -> Option<StateId> {
        self.transitions
            .binary_search_by_key(&symbol, |(symbol, _)| *symbol)
            .ok()
            .map(|index| self.transitions[index].1)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// The competing actions of one conflict.
pub enum ConflictKind {
    /// A shift and a reduce compete on the lookahead.
    ShiftReduce {
        /// The shift target.
        shift: StateId,
        /// The competing rule.
        reduce: RuleId,
    },
    /// Two or more reduces compete on the lookahead.
    ReduceReduce {
        /// The competing rules in ascending order.
        rules: Vec<RuleId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One unresolved conflict; every one is a hard build error for the table-driven style.
pub struct Conflict {
    /// The state in which the conflict occurs.
    pub state: StateId,
    /// The lookahead on which the actions compete.
    pub lookahead: TerminalId,
    /// The competing actions.
    pub kind: ConflictKind,
    /// The action the tables carry, chosen by bison's default policy.
    pub resolution: Action,
    /// Whether the conflict is an LALR merge artefact: absent in every canonical LR(1) state
    /// that merged into this state. Only a reduce/reduce conflict can be one.
    pub merge_artefact: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A resolver's verdict on a shift/reduce conflict.
pub enum Resolution {
    /// Keep the shift.
    Shift,
    /// Keep the reduce.
    Reduce,
    /// Neither: the lookahead is a syntax error, as for non-associative operators.
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One shift/reduce conflict a [`ConflictResolver`] settled. It is not a conflict: the tables
/// carry the resolution and the dump records it the way bison's `.output` does.
pub struct ResolvedConflict {
    /// The state in which the actions competed.
    pub state: StateId,
    /// The lookahead on which they competed.
    pub lookahead: TerminalId,
    /// The shift target that competed.
    pub shift: StateId,
    /// The rule that competed.
    pub reduce: RuleId,
    /// The resolver's verdict.
    pub resolution: Resolution,
}

/// The precedence seam: [`BindingPowerPrecedence`](super::precedence::BindingPowerPrecedence)
/// implements it over Pratt binding powers; [`NoPrecedence`] reports every conflict.
pub trait ConflictResolver {
    /// Resolves a shift/reduce conflict between `reduce` and the shift items on `lookahead`,
    /// or returns `None` to report the conflict.
    fn resolve(
        &self,
        grammar: &LrGrammar,
        reduce: RuleId,
        lookahead: TerminalId,
        shift_items: &[Item],
    ) -> Option<Resolution>;
}

/// The resolver with no precedence table: every conflict is reported.
pub struct NoPrecedence;

impl ConflictResolver for NoPrecedence {
    fn resolve(
        &self,
        _grammar: &LrGrammar,
        _reduce: RuleId,
        _lookahead: TerminalId,
        _shift_items: &[Item],
    ) -> Option<Resolution> {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Construction options.
pub struct AutomatonOptions {
    /// Whether to build the canonical LR(1) automaton to classify reduce/reduce conflicts as
    /// merge artefacts. It runs only when such a conflict exists.
    pub check_merge_artefacts: bool,
    /// Upper bound on canonical LR(1) states before the check gives up.
    pub lr1_state_budget: usize,
}

impl Default for AutomatonOptions {
    fn default() -> Self {
        Self {
            check_merge_artefacts: true,
            lr1_state_budget: 2_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// What the merge-artefact check did.
pub enum MergeArtefactCheck {
    /// No reduce/reduce conflict exists, so nothing needed classifying.
    NotNeeded,
    /// The check was disabled by [`AutomatonOptions`].
    Disabled,
    /// The canonical LR(1) automaton was built and every reduce/reduce conflict classified.
    Verified {
        /// How many canonical LR(1) states the check built.
        lr1_states: usize,
    },
    /// The canonical LR(1) automaton exceeded its budget; no conflict is marked an artefact.
    BudgetExceeded {
        /// The budget that was exceeded.
        budget: usize,
    },
}

#[derive(Clone, Debug)]
/// The LALR(1) automaton of one grammar.
pub struct Automaton {
    /// States in construction order.
    states: Vec<State>,
    /// Start state per start rule, in the grammar's start-rule order.
    start_states: Vec<(TypeId, StateId)>,
    /// Conflicts in ascending (state, lookahead) order.
    conflicts: Vec<Conflict>,
    /// Shift/reduce conflicts the resolver settled, in ascending (state, lookahead, rule) order.
    resolved: Vec<ResolvedConflict>,
    /// What the merge-artefact check did.
    merge_check: MergeArtefactCheck,
}

impl Automaton {
    /// Builds the LALR(1) automaton, resolving conflicts through `resolver`.
    ///
    /// The phases stay together because each consumes the previous one's dense indices: LR(0)
    /// states, nonterminal transitions, the DeRemer–Pennello relations, per-reduction lookaheads,
    /// actions with conflict resolution, and finally the merge-artefact classification.
    pub fn build(
        grammar: &LrGrammar,
        resolver: &dyn ConflictResolver,
        options: AutomatonOptions,
    ) -> Self {
        let nullable = grammar.nullable_nonterminals();
        let lr0 = Lr0Automaton::build(grammar);
        let lookaheads = compute_lookaheads(grammar, &lr0, &nullable);

        let mut states = Vec::with_capacity(lr0.states.len());
        let mut conflicts = Vec::new();
        let mut resolved = Vec::new();
        for (index, built) in lr0.states.iter().enumerate() {
            let id = StateId::new(index as u32);
            let reductions: Vec<Reduction> = built
                .items
                .iter()
                .filter(|item| item.is_complete(grammar))
                .map(|item| Reduction {
                    rule: item.rule,
                    lookaheads: lookaheads[&(id, item.rule)].clone(),
                })
                .collect();
            let (actions, state_conflicts, state_resolved) =
                resolve_actions(grammar, resolver, id, built, &reductions);
            conflicts.extend(state_conflicts);
            resolved.extend(state_resolved);
            let default_reduction = choose_default_reduction(grammar, &actions);
            states.push(State {
                id,
                kernel: built.kernel.clone(),
                items: built.items.clone(),
                transitions: built.transitions.iter().map(|(s, t)| (*s, *t)).collect(),
                reductions,
                actions,
                default_reduction,
            });
        }

        let start_states = grammar
            .starts()
            .iter()
            .zip(&lr0.start_states)
            .map(|(start, state)| (start.type_id, *state))
            .collect();

        let merge_check = classify_merge_artefacts(grammar, &states, &mut conflicts, options);

        Self {
            states,
            start_states,
            conflicts,
            resolved,
            merge_check,
        }
    }

    /// Returns the states in construction order.
    pub fn states(&self) -> &[State] {
        &self.states
    }

    /// Returns the start state of a grammar type, when the type has a start rule.
    pub fn start_state(&self, type_id: TypeId) -> Option<StateId> {
        self.start_states
            .iter()
            .find(|(candidate, _)| *candidate == type_id)
            .map(|(_, state)| *state)
    }

    /// Returns every start state with its grammar type, in start-rule order.
    pub fn start_states(&self) -> &[(TypeId, StateId)] {
        &self.start_states
    }

    /// Returns the shift/reduce conflicts the resolver settled, in ascending (state, lookahead,
    /// rule) order; none of them counts as a conflict.
    pub fn resolved_conflicts(&self) -> &[ResolvedConflict] {
        &self.resolved
    }

    /// Returns the conflicts in ascending (state, lookahead) order.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Returns what the merge-artefact check did.
    pub fn merge_check(&self) -> &MergeArtefactCheck {
        &self.merge_check
    }
}

/// One LR(0) state under construction.
struct Lr0State {
    /// Kernel items in ascending order.
    kernel: Vec<Item>,
    /// Kernel plus closure items in ascending order.
    items: Vec<Item>,
    /// Transitions in ascending symbol order.
    transitions: BTreeMap<Symbol, StateId>,
}

/// The LR(0) automaton: states, transitions and the start state per start rule.
struct Lr0Automaton {
    /// States in construction order.
    states: Vec<Lr0State>,
    /// Start state per start rule.
    start_states: Vec<StateId>,
}

impl Lr0Automaton {
    /// Builds the canonical LR(0) collection from every start rule, breadth first, so that state
    /// numbering depends only on the grammar.
    fn build(grammar: &LrGrammar) -> Self {
        let mut states: Vec<Lr0State> = Vec::new();
        let mut index: HashMap<Vec<Item>, StateId> = HashMap::new();
        let mut queue = VecDeque::new();
        let mut start_states = Vec::new();

        let mut intern = |kernel: Vec<Item>,
                          states: &mut Vec<Lr0State>,
                          queue: &mut VecDeque<StateId>|
         -> StateId {
            if let Some(id) = index.get(&kernel) {
                return *id;
            }
            let id = StateId::new(states.len() as u32);
            index.insert(kernel.clone(), id);
            let items = closure(grammar, &kernel);
            states.push(Lr0State {
                kernel,
                items,
                transitions: BTreeMap::new(),
            });
            queue.push_back(id);
            id
        };

        for start in grammar.starts() {
            let kernel = vec![Item {
                rule: start.rule,
                dot: 0,
            }];
            start_states.push(intern(kernel, &mut states, &mut queue));
        }

        while let Some(id) = queue.pop_front() {
            let mut successors: BTreeMap<Symbol, Vec<Item>> = BTreeMap::new();
            for item in &states[id.index()].items {
                if let Some(symbol) = item.next_symbol(grammar) {
                    successors.entry(symbol).or_default().push(Item {
                        rule: item.rule,
                        dot: item.dot + 1,
                    });
                }
            }
            for (symbol, mut kernel) in successors {
                kernel.sort_unstable();
                let target = intern(kernel, &mut states, &mut queue);
                states[id.index()].transitions.insert(symbol, target);
            }
        }

        Self {
            states,
            start_states,
        }
    }
}

/// Computes the closure of a kernel: kernel items plus a dot-zero item for every rule of every
/// nonterminal that follows a dot, in ascending item order.
fn closure(grammar: &LrGrammar, kernel: &[Item]) -> Vec<Item> {
    let mut items = kernel.to_vec();
    let mut added = vec![false; grammar.nonterminal_count()];
    let mut cursor = 0;
    while cursor < items.len() {
        if let Some(Symbol::Nonterminal(nonterminal)) = items[cursor].next_symbol(grammar) {
            if !added[nonterminal.index()] {
                added[nonterminal.index()] = true;
                for rule in grammar.rules_of(nonterminal) {
                    items.push(Item {
                        rule: *rule,
                        dot: 0,
                    });
                }
            }
        }
        cursor += 1;
    }
    items.sort_unstable();
    items.dedup();
    items
}

/// Computes the LALR(1) lookahead set of every complete item by DeRemer and Pennello.
///
/// The nonterminal transitions are the nodes of both digraphs; `reads` seeds `Read` from the
/// direct-read sets, `includes` seeds `Follow` from `Read`, and `lookback` distributes `Follow`
/// to the complete items. Start rules take the end marker directly.
fn compute_lookaheads(
    grammar: &LrGrammar,
    lr0: &Lr0Automaton,
    nullable: &[bool],
) -> BTreeMap<(StateId, RuleId), TerminalSet> {
    let width = grammar.terminal_count();
    let end = grammar.end_marker();

    // Nonterminal transitions in (state, nonterminal) order.
    let mut transitions: Vec<(StateId, NonterminalId, StateId)> = Vec::new();
    let mut transition_index: HashMap<(StateId, NonterminalId), usize> = HashMap::new();
    for (index, state) in lr0.states.iter().enumerate() {
        let id = StateId::new(index as u32);
        for (symbol, target) in &state.transitions {
            if let Symbol::Nonterminal(nonterminal) = symbol {
                transition_index.insert((id, *nonterminal), transitions.len());
                transitions.push((id, *nonterminal, *target));
            }
        }
    }

    // Direct reads, and the `reads` relation.
    let mut direct_reads = Vec::with_capacity(transitions.len());
    let mut reads: Vec<Vec<u32>> = vec![Vec::new(); transitions.len()];
    for (index, (_, _, target)) in transitions.iter().enumerate() {
        let target_state = &lr0.states[target.index()];
        let mut set = TerminalSet::empty(width);
        for (symbol, _) in &target_state.transitions {
            match symbol {
                Symbol::Terminal(terminal) => set.insert(*terminal),
                Symbol::Nonterminal(nonterminal) => {
                    if nullable[nonterminal.index()] {
                        reads[index].push(transition_index[&(*target, *nonterminal)] as u32);
                    }
                }
            }
        }
        if target_state
            .items
            .iter()
            .any(|item| grammar.is_start_rule(item.rule) && item.is_complete(grammar))
        {
            set.insert(end);
        }
        direct_reads.push(set);
    }
    let read = digraph(&reads, &direct_reads);

    // The `includes` relation and the `lookback` relation, both found by walking each rule's
    // right-hand side from every state that has a transition on its left-hand side.
    let mut includes: Vec<Vec<u32>> = vec![Vec::new(); transitions.len()];
    let mut lookback: BTreeMap<(StateId, RuleId), Vec<usize>> = BTreeMap::new();
    for (index, (origin, nonterminal, _)) in transitions.iter().enumerate() {
        for rule_id in grammar.rules_of(*nonterminal) {
            let rule = grammar.rule(*rule_id);
            let mut nullable_suffix = vec![true; rule.rhs.len() + 1];
            for position in (0..rule.rhs.len()).rev() {
                nullable_suffix[position] = nullable_suffix[position + 1]
                    && matches!(rule.rhs[position], Symbol::Nonterminal(id) if nullable[id.index()]);
            }
            let mut state = *origin;
            for (position, symbol) in rule.rhs.iter().enumerate() {
                if let Symbol::Nonterminal(inner) = symbol {
                    if nullable_suffix[position + 1] {
                        let inner_index = transition_index[&(state, *inner)];
                        includes[inner_index].push(index as u32);
                    }
                }
                state = lr0.states[state.index()].transitions[symbol];
            }
            lookback.entry((state, *rule_id)).or_default().push(index);
        }
    }
    for edges in &mut includes {
        edges.sort_unstable();
        edges.dedup();
    }
    let follow = digraph(&includes, &read);

    let mut lookaheads = BTreeMap::new();
    for (index, state) in lr0.states.iter().enumerate() {
        let id = StateId::new(index as u32);
        for item in &state.items {
            if !item.is_complete(grammar) {
                continue;
            }
            let mut set = TerminalSet::empty(width);
            if grammar.is_start_rule(item.rule) {
                set.insert(end);
            } else if let Some(sources) = lookback.get(&(id, item.rule)) {
                for source in sources {
                    set.union_assign(&follow[*source]);
                }
            }
            lookaheads.insert((id, item.rule), set);
        }
    }
    lookaheads
}

/// DeRemer and Pennello's digraph algorithm: the least solution of
/// `F(x) = init(x) ∪ ⋃ { F(y) | x R y }`, computed iteratively so deep relations cannot overflow
/// the thread stack.
fn digraph(relation: &[Vec<u32>], init: &[TerminalSet]) -> Vec<TerminalSet> {
    /// One in-progress traversal frame.
    struct Frame {
        /// The node being traversed.
        node: usize,
        /// The node's stack depth when it was entered.
        depth: usize,
        /// The next successor to visit.
        next: usize,
    }

    let count = relation.len();
    let mut result: Vec<TerminalSet> = init.to_vec();
    let mut number = vec![0usize; count];
    let mut stack: Vec<usize> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();

    for root in 0..count {
        if number[root] != 0 {
            continue;
        }
        stack.push(root);
        number[root] = stack.len();
        frames.push(Frame {
            node: root,
            depth: stack.len(),
            next: 0,
        });
        while let Some(frame) = frames.last_mut() {
            let node = frame.node;
            if frame.next < relation[node].len() {
                let successor = relation[node][frame.next] as usize;
                frame.next += 1;
                if number[successor] == 0 {
                    stack.push(successor);
                    number[successor] = stack.len();
                    frames.push(Frame {
                        node: successor,
                        depth: stack.len(),
                        next: 0,
                    });
                } else {
                    number[node] = number[node].min(number[successor]);
                    let merged = result[successor].clone();
                    result[node].union_assign(&merged);
                }
            } else {
                let depth = frame.depth;
                frames.pop();
                if let Some(parent) = frames.last() {
                    let parent = parent.node;
                    number[parent] = number[parent].min(number[node]);
                    let merged = result[node].clone();
                    result[parent].union_assign(&merged);
                }
                if number[node] == depth {
                    loop {
                        let top = stack.pop().expect("node is on the stack");
                        number[top] = usize::MAX;
                        if top == node {
                            break;
                        }
                        result[top] = result[node].clone();
                    }
                }
            }
        }
    }
    result
}

/// Resolves the actions of one state and records its conflicts and its resolved conflicts.
fn resolve_actions(
    grammar: &LrGrammar,
    resolver: &dyn ConflictResolver,
    id: StateId,
    state: &Lr0State,
    reductions: &[Reduction],
) -> (BTreeMap<TerminalId, Action>, Vec<Conflict>, Vec<ResolvedConflict>) {
    let mut actions: BTreeMap<TerminalId, Action> = BTreeMap::new();
    for (symbol, target) in &state.transitions {
        if let Symbol::Terminal(terminal) = symbol {
            actions.insert(*terminal, Action::Shift(*target));
        }
    }
    let mut conflicts: BTreeMap<TerminalId, Conflict> = BTreeMap::new();
    let mut resolved: Vec<ResolvedConflict> = Vec::new();
    for reduction in reductions {
        for lookahead in reduction.lookaheads.iter() {
            match actions.get(&lookahead).copied() {
                None => {
                    actions.insert(lookahead, Action::Reduce(reduction.rule));
                }
                Some(Action::Shift(shift)) => {
                    let shift_items: Vec<Item> = state
                        .items
                        .iter()
                        .filter(|item| {
                            item.next_symbol(grammar) == Some(Symbol::Terminal(lookahead))
                        })
                        .copied()
                        .collect();
                    match resolver.resolve(grammar, reduction.rule, lookahead, &shift_items) {
                        Some(resolution) => {
                            match resolution {
                                Resolution::Shift => {}
                                Resolution::Reduce => {
                                    actions.insert(lookahead, Action::Reduce(reduction.rule));
                                }
                                Resolution::Error => {
                                    actions.insert(lookahead, Action::Error);
                                }
                            }
                            resolved.push(ResolvedConflict {
                                state: id,
                                lookahead,
                                shift,
                                reduce: reduction.rule,
                                resolution,
                            });
                        }
                        None => {
                            conflicts.entry(lookahead).or_insert(Conflict {
                                state: id,
                                lookahead,
                                kind: ConflictKind::ShiftReduce {
                                    shift,
                                    reduce: reduction.rule,
                                },
                                resolution: Action::Shift(shift),
                                merge_artefact: false,
                            });
                        }
                    }
                }
                Some(Action::Reduce(existing)) => {
                    let conflict = conflicts.entry(lookahead).or_insert(Conflict {
                        state: id,
                        lookahead,
                        kind: ConflictKind::ReduceReduce {
                            rules: vec![existing],
                        },
                        resolution: Action::Reduce(existing),
                        merge_artefact: false,
                    });
                    match &mut conflict.kind {
                        ConflictKind::ReduceReduce { rules } => rules.push(reduction.rule),
                        ConflictKind::ShiftReduce { .. } => {
                            // A precedence-resolved reduce meeting another reduce is still a
                            // reduce/reduce conflict; the earlier shift/reduce record stands.
                        }
                    }
                }
                Some(Action::Error) => {}
            }
        }
    }
    resolved.sort_by_key(|conflict| (conflict.lookahead, conflict.reduce));
    (actions, conflicts.into_values().collect(), resolved)
}

/// Chooses the default reduction: the reduce whose lookahead holds the end marker if any, else
/// the most frequent reduce, ties broken by the lowest rule.
fn choose_default_reduction(
    grammar: &LrGrammar,
    actions: &BTreeMap<TerminalId, Action>,
) -> Option<RuleId> {
    let end = grammar.end_marker();
    if let Some(Action::Reduce(rule)) = actions.get(&end) {
        return Some(*rule);
    }
    let mut counts: BTreeMap<RuleId, usize> = BTreeMap::new();
    for action in actions.values() {
        if let Action::Reduce(rule) = action {
            *counts.entry(*rule).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|(left_rule, left), (right_rule, right)| {
            left.cmp(right).then(right_rule.cmp(left_rule))
        })
        .map(|(rule, _)| rule)
}

/// Marks the reduce/reduce conflicts that no canonical LR(1) state exhibits.
fn classify_merge_artefacts(
    grammar: &LrGrammar,
    states: &[State],
    conflicts: &mut [Conflict],
    options: AutomatonOptions,
) -> MergeArtefactCheck {
    let has_reduce_reduce = conflicts
        .iter()
        .any(|conflict| matches!(conflict.kind, ConflictKind::ReduceReduce { .. }));
    if !has_reduce_reduce {
        return MergeArtefactCheck::NotNeeded;
    }
    if !options.check_merge_artefacts {
        return MergeArtefactCheck::Disabled;
    }
    let Some(canonical) = CanonicalLr1::build(grammar, options.lr1_state_budget) else {
        return MergeArtefactCheck::BudgetExceeded {
            budget: options.lr1_state_budget,
        };
    };
    for conflict in conflicts.iter_mut() {
        let ConflictKind::ReduceReduce { rules } = &conflict.kind else {
            continue;
        };
        let core = states[conflict.state.index()].kernel();
        conflict.merge_artefact = !canonical.exhibits(core, rules, conflict.lookahead);
    }
    MergeArtefactCheck::Verified {
        lr1_states: canonical.states.len(),
    }
}

/// One canonical LR(1) state: its complete items with their lookaheads, keyed by LR(0) core.
struct Lr1State {
    /// Lookaheads of every complete item, in ascending rule order.
    reductions: Vec<(RuleId, TerminalSet)>,
}

/// The canonical LR(1) collection, built only to classify merge artefacts.
struct CanonicalLr1 {
    /// States in construction order.
    states: Vec<Lr1State>,
    /// LR(1) states grouped by their LR(0) kernel core.
    by_core: HashMap<Vec<Item>, Vec<usize>>,
}

impl CanonicalLr1 {
    /// Builds the collection, or returns `None` once the state budget is exceeded.
    fn build(grammar: &LrGrammar, budget: usize) -> Option<Self> {
        let width = grammar.terminal_count();
        let end = grammar.end_marker();
        let nullable = grammar.nullable_nonterminals();
        let first = first_sets(grammar, &nullable);

        let mut states = Vec::new();
        let mut by_core: HashMap<Vec<Item>, Vec<usize>> = HashMap::new();
        let mut index: HashMap<Vec<(Item, Vec<u64>)>, usize> = HashMap::new();
        let mut queue: VecDeque<(usize, Vec<(Item, TerminalSet)>)> = VecDeque::new();

        let mut intern = |kernel: Vec<(Item, TerminalSet)>,
                          states: &mut Vec<Lr1State>,
                          queue: &mut VecDeque<(usize, Vec<(Item, TerminalSet)>)>|
         -> usize {
            let key: Vec<(Item, Vec<u64>)> = kernel
                .iter()
                .map(|(item, set)| (*item, set.words.clone()))
                .collect();
            if let Some(id) = index.get(&key) {
                return *id;
            }
            let id = states.len();
            index.insert(key, id);
            let core: Vec<Item> = kernel.iter().map(|(item, _)| *item).collect();
            by_core.entry(core).or_default().push(id);
            states.push(Lr1State {
                reductions: Vec::new(),
            });
            queue.push_back((id, kernel));
            id
        };

        for start in grammar.starts() {
            let mut set = TerminalSet::empty(width);
            set.insert(end);
            let kernel = vec![(
                Item {
                    rule: start.rule,
                    dot: 0,
                },
                set,
            )];
            intern(kernel, &mut states, &mut queue);
        }

        while let Some((id, kernel)) = queue.pop_front() {
            if states.len() > budget {
                return None;
            }
            let items = lr1_closure(grammar, &nullable, &first, kernel);
            let mut reductions = Vec::new();
            let mut successors: BTreeMap<Symbol, Vec<(Item, TerminalSet)>> = BTreeMap::new();
            for (item, set) in items {
                match item.next_symbol(grammar) {
                    None => reductions.push((item.rule, set)),
                    Some(symbol) => successors.entry(symbol).or_default().push((
                        Item {
                            rule: item.rule,
                            dot: item.dot + 1,
                        },
                        set,
                    )),
                }
            }
            states[id].reductions = reductions;
            for (_, kernel) in successors {
                intern(kernel, &mut states, &mut queue);
            }
        }

        Some(Self { states, by_core })
    }

    /// Reports whether some LR(1) state with the given core has every listed rule reducing on
    /// the lookahead, which is what makes the LALR conflict genuine.
    fn exhibits(&self, core: &[Item], rules: &[RuleId], lookahead: TerminalId) -> bool {
        let Some(states) = self.by_core.get(core) else {
            return false;
        };
        states.iter().any(|id| {
            let state = &self.states[*id];
            rules.iter().all(|rule| {
                state
                    .reductions
                    .iter()
                    .any(|(candidate, set)| candidate == rule && set.contains(lookahead))
            })
        })
    }
}

/// Computes FIRST of every nonterminal.
pub(super) fn first_sets(grammar: &LrGrammar, nullable: &[bool]) -> Vec<TerminalSet> {
    let width = grammar.terminal_count();
    let mut first = vec![TerminalSet::empty(width); grammar.nonterminal_count()];
    let mut changed = true;
    while changed {
        changed = false;
        for rule in grammar.rules() {
            for symbol in &rule.rhs {
                match symbol {
                    Symbol::Terminal(terminal) => {
                        if !first[rule.lhs.index()].contains(*terminal) {
                            first[rule.lhs.index()].insert(*terminal);
                            changed = true;
                        }
                        break;
                    }
                    Symbol::Nonterminal(inner) => {
                        let inner_first = first[inner.index()].clone();
                        changed |= first[rule.lhs.index()].union_assign(&inner_first);
                        if !nullable[inner.index()] {
                            break;
                        }
                    }
                }
            }
        }
    }
    first
}

/// Computes the LR(1) closure of a kernel: every item with the union of the lookaheads that
/// reach it, in ascending item order.
fn lr1_closure(
    grammar: &LrGrammar,
    nullable: &[bool],
    first: &[TerminalSet],
    kernel: Vec<(Item, TerminalSet)>,
) -> Vec<(Item, TerminalSet)> {
    let width = grammar.terminal_count();
    let mut items: BTreeMap<Item, TerminalSet> = kernel.into_iter().collect();
    let mut queue: VecDeque<Item> = items.keys().copied().collect();
    let mut queued: BTreeSet<Item> = queue.iter().copied().collect();
    while let Some(item) = queue.pop_front() {
        queued.remove(&item);
        let Some(Symbol::Nonterminal(nonterminal)) = item.next_symbol(grammar) else {
            continue;
        };
        let rule = grammar.rule(item.rule);
        let mut lookahead = TerminalSet::empty(width);
        let mut suffix_nullable = true;
        for symbol in &rule.rhs[item.dot as usize + 1..] {
            match symbol {
                Symbol::Terminal(terminal) => {
                    lookahead.insert(*terminal);
                    suffix_nullable = false;
                    break;
                }
                Symbol::Nonterminal(inner) => {
                    lookahead.union_assign(&first[inner.index()]);
                    if !nullable[inner.index()] {
                        suffix_nullable = false;
                        break;
                    }
                }
            }
        }
        if suffix_nullable {
            let own = items[&item].clone();
            lookahead.union_assign(&own);
        }
        for rule in grammar.rules_of(nonterminal) {
            let successor = Item {
                rule: *rule,
                dot: 0,
            };
            let entry = items
                .entry(successor)
                .or_insert_with(|| TerminalSet::empty(width));
            if entry.union_assign(&lookahead) && queued.insert(successor) {
                queue.push_back(successor);
            }
        }
    }
    items.into_iter().collect()
}
