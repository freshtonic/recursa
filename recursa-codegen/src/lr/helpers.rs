#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Per-site copies of shared helper nonterminals (#125).
//!
//! A helper (`opt(...)`, `list(...)`, `presence(...)`) is shared between every use site, so an
//! LR(0) state whose kernel holds one of its partially or fully parsed rules (`opt(X) -> X .`,
//! `list1 -> list1 . sep e`) has one core across all of those sites, and LALR(1) unions the
//! lookaheads of every site into it. That union can manufacture only reduce/reduce conflicts:
//! the shift set of a state is a property of its LR(0) core, so a shift/reduce conflict that
//! LALR(1) reports also exists in some canonical LR(1) state, and no renaming of nonterminals
//! removes it. The pass therefore looks only at reduce/reduce conflicts in states whose kernel
//! holds a helper item with the dot past the start.
//!
//! Such a helper gets one copy per co-occurrence class of its use sites. Two sites must share a
//! copy when some LR(0) state holds both of their items with the dot before the helper: separate
//! copies there would make the parser choose between the sites before it has seen what tells
//! them apart, a conflict the shared form did not have. Under that rule every state of the
//! copied grammar maps, with copy names erased, onto a state of the shared grammar with the same
//! items, so lookaheads can only shrink and no new conflict can appear. The first class keeps the
//! original helper; every other class gets a copy named `opt(X)@site` after its first site. A
//! copy's rules are the original's with self-references redirected, and its reduce actions,
//! folds and tail decisions are the original's. The automaton is rebuilt after each round until
//! a round copies nothing or [`MAX_ROUNDS`] is reached, whichever comes first; a copy owns one
//! class, so a helper is split again only when a copy of an enclosing helper gave it new sites.

use std::collections::{BTreeMap, BTreeSet};

use super::automaton::{Automaton, AutomatonOptions, ConflictKind, ConflictResolver};
use super::grammar::{LrGrammar, NonterminalId, NonterminalOrigin, Rule, RuleId, Symbol};
use super::report::LrDiagnostic;

/// Upper bound on split-and-rebuild rounds. Each round copies only helpers with a kernel item
/// in a reduce/reduce state, and a copy owns one class, so the rounds are bounded by the helper
/// nesting depth of the grammar; the cap guards the loop against a lowering that breaks that.
pub const MAX_ROUNDS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One use site of a helper: a position on the right-hand side of a rule of another nonterminal.
pub struct HelperSite {
    /// The rule that uses the helper.
    pub rule: RuleId,
    /// The position of the helper symbol on the rule's right-hand side.
    pub position: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One copy of a helper and the co-occurrence class of use sites it now owns.
pub struct HelperCopy {
    /// The copy.
    pub copy: NonterminalId,
    /// The sites rewritten to the copy, in rule order.
    pub sites: Vec<HelperSite>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One shared helper split into per-class copies in one round.
pub struct HelperSplit {
    /// The helper that was shared; it keeps the class of its first site.
    pub original: NonterminalId,
    /// The copies for the other classes, in class order.
    pub copies: Vec<HelperCopy>,
}

/// The outcome of [`build_with_helper_copies`].
pub struct CopiedBuild {
    /// The grammar with every copy inserted.
    pub grammar: LrGrammar,
    /// The automaton of that grammar, classified per the caller's options.
    pub automaton: Automaton,
    /// Every split made, in round order.
    pub splits: Vec<HelperSplit>,
    /// How many automata were built: one more than the rounds that copied something, unless
    /// the cap stopped the loop.
    pub rounds: usize,
}

/// Reports whether a nonterminal is a helper or a copy of one.
pub fn is_helper(grammar: &LrGrammar, nonterminal: NonterminalId) -> bool {
    matches!(
        grammar.nonterminal(nonterminal).origin,
        NonterminalOrigin::Helper | NonterminalOrigin::HelperCopy { .. }
    )
}

/// The helpers with a kernel item, dot past the start, in a state that has a reduce/reduce
/// conflict, in ascending order.
pub fn polluted_helpers(grammar: &LrGrammar, automaton: &Automaton) -> BTreeSet<NonterminalId> {
    let mut helpers = BTreeSet::new();
    for conflict in automaton.conflicts() {
        if !matches!(conflict.kind, ConflictKind::ReduceReduce { .. }) {
            continue;
        }
        for item in automaton.states()[conflict.state.index()].kernel() {
            let lhs = grammar.rule(item.rule).lhs;
            if item.dot > 0 && is_helper(grammar, lhs) {
                helpers.insert(lhs);
            }
        }
    }
    helpers
}

/// The use sites of a helper outside its own rules, in rule order.
pub fn sites_of(grammar: &LrGrammar, helper: NonterminalId) -> Vec<HelperSite> {
    let mut sites = Vec::new();
    for (index, rule) in grammar.rules().iter().enumerate() {
        if rule.lhs == helper {
            continue;
        }
        for (position, symbol) in rule.rhs.iter().enumerate() {
            if *symbol == Symbol::Nonterminal(helper) {
                sites.push(HelperSite {
                    rule: RuleId::new(index as u32),
                    position: position as u32,
                });
            }
        }
    }
    sites
}

/// Groups the sites of a helper into co-occurrence classes: sites whose items with the dot
/// before the helper share an LR(0) state, transitively. Classes come in the order of their
/// first site, each with its sites in rule order.
fn site_classes(
    grammar: &LrGrammar,
    automaton: &Automaton,
    helper: NonterminalId,
    sites: &[HelperSite],
) -> Vec<Vec<HelperSite>> {
    let index: BTreeMap<HelperSite, usize> = sites
        .iter()
        .enumerate()
        .map(|(index, site)| (*site, index))
        .collect();
    let mut parent: Vec<usize> = (0..sites.len()).collect();
    fn root(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }
    for state in automaton.states() {
        let mut first: Option<usize> = None;
        for item in state.items() {
            if item.next_symbol(grammar) != Some(Symbol::Nonterminal(helper))
                || grammar.rule(item.rule).lhs == helper
            {
                continue;
            }
            let site = index[&HelperSite {
                rule: item.rule,
                position: item.dot,
            }];
            match first {
                None => first = Some(site),
                Some(other) => {
                    let (left, right) = (root(&mut parent, other), root(&mut parent, site));
                    parent[left.max(right)] = left.min(right);
                }
            }
        }
    }
    let mut classes: BTreeMap<usize, Vec<HelperSite>> = BTreeMap::new();
    for (index, site) in sites.iter().enumerate() {
        classes.entry(root(&mut parent, index)).or_default().push(*site);
    }
    classes.into_values().collect()
}

/// Names a copy after the first site of its class: `helper@lhs`, or `helper@lhs#k` when the
/// left-hand side uses the helper at several sites (`k` counts them in rule order), made unique
/// with a trailing `@n` if the grammar already has the name.
fn copy_name(grammar: &LrGrammar, helper: NonterminalId, sites: &[HelperSite], first: HelperSite) -> String {
    let lhs = grammar.rule(first.rule).lhs;
    let under_lhs: Vec<&HelperSite> = sites
        .iter()
        .filter(|site| grammar.rule(site.rule).lhs == lhs)
        .collect();
    let mut name = format!(
        "{}@{}",
        grammar.nonterminal(helper).name,
        grammar.nonterminal(lhs).name
    );
    if under_lhs.len() > 1 {
        let ordinal = under_lhs
            .iter()
            .position(|site| **site == first)
            .expect("the first site of a class is a site")
            + 1;
        name.push_str(&format!("#{ordinal}"));
    }
    if grammar.nonterminal_by_name(&name).is_none() {
        return name;
    }
    let mut counter = 2;
    loop {
        let candidate = format!("{name}@{counter}");
        if grammar.nonterminal_by_name(&candidate).is_none() {
            return candidate;
        }
        counter += 1;
    }
}

/// Copies one helper per class after the first: rewrites the class's sites to the copy and
/// appends the copy's rules to `rules` for the caller to add, with the split record.
fn copy_helper(
    grammar: &mut LrGrammar,
    helper: NonterminalId,
    sites: &[HelperSite],
    classes: &[Vec<HelperSite>],
    rules: &mut Vec<Rule>,
) -> Option<HelperSplit> {
    if classes.len() < 2 {
        return None;
    }
    let original = match grammar.nonterminal(helper).origin {
        NonterminalOrigin::HelperCopy { original } => original,
        _ => helper,
    };
    let mut copies = Vec::new();
    for class in &classes[1..] {
        let name = copy_name(grammar, helper, sites, class[0]);
        let carries_value = grammar.nonterminal(helper).carries_value;
        let copy = grammar.add_nonterminal(
            name,
            NonterminalOrigin::HelperCopy { original },
            carries_value,
        );
        for rule in grammar.rules_of(helper) {
            let mut cloned = grammar.rule(*rule).clone();
            cloned.lhs = copy;
            for symbol in &mut cloned.rhs {
                if *symbol == Symbol::Nonterminal(helper) {
                    *symbol = Symbol::Nonterminal(copy);
                }
            }
            rules.push(cloned);
        }
        for site in class {
            grammar.rules_mut()[site.rule.index()].rhs[site.position as usize] =
                Symbol::Nonterminal(copy);
        }
        copies.push(HelperCopy {
            copy,
            sites: class.clone(),
        });
    }
    Some(HelperSplit {
        original: helper,
        copies,
    })
}

/// One round: copies every polluted helper per co-occurrence class and returns the splits made,
/// in ascending helper order. Existing rule identities are unchanged; the copies' rules are
/// appended, each copy's rules contiguous.
pub fn copy_polluted_helpers(grammar: &mut LrGrammar, automaton: &Automaton) -> Vec<HelperSplit> {
    let candidates = polluted_helpers(grammar, automaton);
    let planned: Vec<(NonterminalId, Vec<HelperSite>, Vec<Vec<HelperSite>>)> = candidates
        .into_iter()
        .map(|helper| {
            let sites = sites_of(grammar, helper);
            let classes = site_classes(grammar, automaton, helper, &sites);
            (helper, sites, classes)
        })
        .collect();
    let mut rules = Vec::new();
    let mut splits = Vec::new();
    for (helper, sites, classes) in &planned {
        if let Some(split) = copy_helper(grammar, *helper, sites, classes, &mut rules) {
            splits.push(split);
        }
    }
    for rule in rules {
        grammar.add_rule(rule);
    }
    splits
}

/// Builds the automaton, copying polluted helpers and rebuilding until a round copies nothing.
///
/// `resolver` makes the conflict resolver for each grammar the loop builds, because a resolver
/// such as binding-power precedence is derived from the rules. Intermediate automata skip the
/// merge-artefact check; the last one is classified per `options`.
pub fn build_with_helper_copies<R: ConflictResolver>(
    mut grammar: LrGrammar,
    resolver: impl Fn(&LrGrammar) -> Result<R, Vec<LrDiagnostic>>,
    options: AutomatonOptions,
) -> Result<CopiedBuild, Vec<LrDiagnostic>> {
    let unclassified = AutomatonOptions {
        check_merge_artefacts: false,
        ..options
    };
    let mut splits = Vec::new();
    let mut rounds = 0;
    loop {
        let mut automaton = Automaton::build(&grammar, &resolver(&grammar)?, unclassified);
        rounds += 1;
        let round = if rounds <= MAX_ROUNDS {
            copy_polluted_helpers(&mut grammar, &automaton)
        } else {
            Vec::new()
        };
        if round.is_empty() {
            automaton.classify_merge_artefacts(&grammar, options);
            return Ok(CopiedBuild {
                grammar,
                automaton,
                splits,
                rounds,
            });
        }
        splits.extend(round);
    }
}
