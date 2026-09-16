#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Bison-style automaton dump and conflict diagnostics.
//!
//! The dump is the human-readable half of the generated-code debug bundle: the rules, then every
//! state with its shortest path, items, the conflicts precedence or declaration order resolved,
//! actions, rejected actions in brackets, and gotos, then a conflict summary and the helper
//! copies the construction made. The diagnostics carry the same facts in the shape the build
//! report needs: a code, a message, the shortest path to the state, the state's items, the
//! competing actions, and notes such as `RCA0405`.

use std::fmt::Write as _;

use super::automaton::{
    Action, Automaton, Conflict, ConflictKind, MergeArtefactCheck, Resolution, State,
};
use super::grammar::{LrGrammar, NonterminalOrigin, RuleId, Symbol};
use super::paths::{SymbolPath, render_path, shortest_paths};

/// Registered code of a shift/reduce conflict: a state where one lookahead can both be shifted
/// and end a rule, which precedence did not resolve.
pub const SHIFT_REDUCE: &str = "RCA0400";
/// Registered code of a reduce/reduce conflict: a state where two rules end on one lookahead.
pub const REDUCE_REDUCE: &str = "RCA0401";
/// Registered code of a token type refined by a text predicate, which table mode cannot lower.
pub const TEXT_PREDICATE: &str = "RCA0403";
/// Registered code of the note attached to a reduce/reduce conflict that no canonical LR(1)
/// state exhibits, so a move to IELR(1) would remove it.
pub const MERGE_ARTEFACT: &str = "RCA0405";

/// A node, variant or field overrides the capture policy (`#[spanned(...)]`), which the
/// table-driven parser cannot resolve below the entry type (#118).
pub const CAPTURE_OVERRIDE: &str = "RCA0406";
/// Registered code of a stale, missing, ambiguous, incompatible, or over-broad scoped LR
/// conflict declaration.
pub const INVALID_SCOPED_RESOLUTION: &str = "RCA0407";

#[derive(Clone, Debug, Eq, PartialEq)]
/// One table-driven analysis diagnostic, before the build report adapts it.
pub struct LrDiagnostic {
    /// The registered code.
    pub code: &'static str,
    /// The one-line message.
    pub message: String,
    /// The shortest symbol path from a start state to the state the diagnostic concerns, with
    /// the lookahead after the dot (`S -> i S . e`); empty for a diagnostic without a state.
    pub path: String,
    /// The items of the state the diagnostic concerns, rendered.
    pub items: Vec<String>,
    /// The competing actions, rendered, for a conflict.
    pub actions: Vec<String>,
    /// Attached notes, each starting with its code.
    pub notes: Vec<String>,
}

impl LrDiagnostic {
    /// Renders the diagnostic as indented text.
    pub fn render(&self) -> String {
        let mut text = format!("error[{}]: {}\n", self.code, self.message);
        if !self.path.is_empty() {
            let _ = writeln!(text, "  path: {}", self.path);
        }
        if !self.items.is_empty() {
            text.push_str("  items:\n");
            for item in &self.items {
                let _ = writeln!(text, "    {item}");
            }
        }
        if !self.actions.is_empty() {
            text.push_str("  actions:\n");
            for action in &self.actions {
                let _ = writeln!(text, "    {action}");
            }
        }
        for note in &self.notes {
            let _ = writeln!(text, "  note: {note}");
        }
        text
    }
}

/// Renders an action the way the dump does.
fn render_action(grammar: &LrGrammar, action: Action) -> String {
    match action {
        Action::Shift(target) => format!("shift, and go to state {}", target.index()),
        Action::Reduce(rule) if grammar.is_start_rule(rule) => "accept".to_owned(),
        Action::Reduce(rule) => format!(
            "reduce using rule {} ({})",
            rule.index(),
            grammar.render_rule(rule)
        ),
        Action::Error => "error (nonassociative)".to_owned(),
    }
}

/// Renders the competing actions of a conflict.
fn competing_actions(grammar: &LrGrammar, conflict: &Conflict) -> Vec<String> {
    match &conflict.kind {
        ConflictKind::ShiftReduce { shift, reduce } => vec![
            render_action(grammar, Action::Shift(*shift)),
            render_action(grammar, Action::Reduce(*reduce)),
        ],
        ConflictKind::ReduceReduce { rules } => rules
            .iter()
            .map(|rule| render_action(grammar, Action::Reduce(*rule)))
            .collect(),
    }
}

/// Converts every conflict into a diagnostic, in (state, lookahead) order.
pub fn conflict_diagnostics(grammar: &LrGrammar, automaton: &Automaton) -> Vec<LrDiagnostic> {
    let paths = shortest_paths(automaton);
    automaton
        .conflicts()
        .iter()
        .map(|conflict| {
            let state = &automaton.states()[conflict.state.index()];
            let lookahead = grammar.terminal_name(conflict.lookahead);
            let path = paths[conflict.state.index()]
                .as_ref()
                .map_or_else(String::new, |path| {
                    render_path(grammar, path, Some(conflict.lookahead))
                });
            let (code, message) = match &conflict.kind {
                ConflictKind::ShiftReduce { .. } => (
                    SHIFT_REDUCE,
                    format!(
                        "shift/reduce conflict in state {} on lookahead `{lookahead}`",
                        conflict.state.index()
                    ),
                ),
                ConflictKind::ReduceReduce { .. } => (
                    REDUCE_REDUCE,
                    format!(
                        "reduce/reduce conflict in state {} on lookahead `{lookahead}`",
                        conflict.state.index()
                    ),
                ),
            };
            let mut actions = competing_actions(grammar, conflict);
            actions.push(format!(
                "resolved as: {}",
                render_action(grammar, conflict.resolution)
            ));
            let mut notes = Vec::new();
            if conflict.merge_artefact {
                notes.push(format!(
                    "{MERGE_ARTEFACT}: this conflict is an LALR(1) merge artefact; no canonical \
                     LR(1) state with these items has it"
                ));
            } else if matches!(conflict.kind, ConflictKind::ReduceReduce { .. })
                && let MergeArtefactCheck::BudgetExceeded { budget } = automaton.merge_check()
            {
                notes.push(format!(
                    "merge-artefact classification is inconclusive: canonical LR(1) exceeded \
                     the {budget}-state budget"
                ));
            }
            LrDiagnostic {
                code,
                message,
                path,
                items: state
                    .items()
                    .iter()
                    .map(|item| item.render(grammar))
                    .collect(),
                actions,
                notes,
            }
        })
        .collect()
}

/// Renders the whole automaton bison-style.
pub fn automaton_dump(grammar: &LrGrammar, automaton: &Automaton) -> String {
    let mut text = String::new();
    text.push_str("Grammar\n\n");
    for (index, _) in grammar.rules().iter().enumerate() {
        let _ = writeln!(
            text,
            "{index:>5} {}",
            grammar.render_rule(RuleId::new(index as u32))
        );
    }
    text.push('\n');
    let paths = shortest_paths(automaton);
    for state in automaton.states() {
        render_state(
            &mut text,
            grammar,
            automaton,
            state,
            paths[state.id().index()].as_ref(),
        );
    }
    render_conflict_summary(&mut text, grammar, automaton);
    render_end_marker_resolutions(&mut text, automaton);
    render_helper_copies(&mut text, grammar);
    text
}

/// Renders how many reduce/reduce conflicts on the end marker declaration order settled.
fn render_end_marker_resolutions(text: &mut String, automaton: &Automaton) {
    let count = automaton.end_marker_resolutions().len();
    if count > 0 {
        let _ = writeln!(
            text,
            "{count} reduce/reduce conflicts on $end resolved by declaration order."
        );
    }
}

/// Renders the per-site helper copies the construction made, each with its original.
fn render_helper_copies(text: &mut String, grammar: &LrGrammar) {
    let copies: Vec<&super::grammar::Nonterminal> = grammar
        .nonterminals()
        .iter()
        .filter(|nonterminal| matches!(nonterminal.origin, NonterminalOrigin::HelperCopy { .. }))
        .collect();
    if copies.is_empty() {
        return;
    }
    text.push_str("\nHelper copies\n\n");
    for copy in copies {
        let NonterminalOrigin::HelperCopy { original } = copy.origin else {
            unreachable!("only helper copies were kept");
        };
        let _ = writeln!(
            text,
            "{} copies {}",
            copy.name,
            grammar.nonterminal(original).name
        );
    }
}

/// Renders one state: its shortest path, items, the conflicts precedence or declaration order
/// resolved, actions with rejected alternatives in brackets, then gotos.
fn render_state(
    text: &mut String,
    grammar: &LrGrammar,
    automaton: &Automaton,
    state: &State,
    path: Option<&SymbolPath>,
) {
    let _ = writeln!(text, "State {}\n", state.id().index());
    if let Some(path) = path {
        let _ = writeln!(text, "    path: {}\n", render_path(grammar, path, None));
    }
    for item in state.items() {
        let _ = writeln!(text, "{:>5} {}", item.rule.index(), item.render(grammar));
    }
    text.push('\n');

    let mut wrote_resolved = false;
    for resolved in automaton
        .scoped_resolutions()
        .iter()
        .filter(|resolved| resolved.state == state.id())
    {
        let rules = resolved
            .rules
            .iter()
            .map(|rule| rule.index().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let verdict = match resolved.resolution {
            Resolution::Shift => "shift".to_owned(),
            Resolution::Reduce => format!(
                "reduce using rule {}",
                resolved
                    .reduced
                    .expect("a scoped reduce records its selected rule")
                    .index()
            ),
            Resolution::Error => "an error".to_owned(),
        };
        let _ = writeln!(
            text,
            "    Conflict involving rule(s) {rules} and token {} resolved by scoped declaration {} ({verdict}).",
            grammar.terminal_name(resolved.lookahead),
            resolved.declaration,
        );
        wrote_resolved = true;
    }
    for resolved in automaton
        .resolved_conflicts()
        .iter()
        .filter(|resolved| resolved.state == state.id())
    {
        let verdict = match resolved.resolution {
            Resolution::Shift => "shift",
            Resolution::Reduce => "reduce",
            Resolution::Error => "an error",
        };
        let _ = writeln!(
            text,
            "    Conflict between rule {} and token {} resolved by precedence ({verdict}).",
            resolved.reduce.index(),
            grammar.terminal_name(resolved.lookahead)
        );
        wrote_resolved = true;
    }
    for resolution in automaton
        .end_marker_resolutions()
        .iter()
        .filter(|resolution| resolution.state == state.id())
    {
        let rules: Vec<String> = resolution
            .rules
            .iter()
            .map(|rule| rule.index().to_string())
            .collect();
        let (head, last) = rules.split_at(rules.len() - 1);
        let _ = writeln!(
            text,
            "    Conflict between rules {} and {} resolved on $end by declaration order (reduce using rule {}).",
            head.join(", "),
            last[0],
            resolution.chosen.index()
        );
        wrote_resolved = true;
    }
    if wrote_resolved {
        text.push('\n');
    }

    let conflicts: Vec<&Conflict> = automaton
        .conflicts()
        .iter()
        .filter(|conflict| conflict.state == state.id())
        .collect();
    let mut wrote_action = false;
    for (terminal, action) in state.actions() {
        if let Action::Reduce(rule) = action
            && state.default_reduction() == Some(*rule)
        {
            continue;
        }
        let name = grammar.terminal_name(*terminal);
        let _ = writeln!(text, "    {name:<12}{}", render_action(grammar, *action));
        wrote_action = true;
        for conflict in conflicts
            .iter()
            .filter(|conflict| conflict.lookahead == *terminal)
        {
            for rejected in rejected_actions(conflict) {
                let _ = writeln!(text, "    {name:<12}[{}]", render_action(grammar, rejected));
            }
        }
    }
    if let Some(rule) = state.default_reduction() {
        let _ = writeln!(
            text,
            "    {:<12}{}",
            "$default",
            render_action(grammar, Action::Reduce(rule))
        );
        for conflict in &conflicts {
            let _ = writeln!(
                text,
                "    {:<12}[{}]",
                grammar.terminal_name(conflict.lookahead),
                rejected_actions(conflict)
                    .map(|rejected| render_action(grammar, rejected))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
        wrote_action = true;
    }
    if wrote_action {
        text.push('\n');
    }

    let mut wrote_goto = false;
    for (symbol, target) in state.transitions() {
        if let Symbol::Nonterminal(_) = symbol {
            let _ = writeln!(
                text,
                "    {:<12}go to state {}",
                grammar.symbol_name(*symbol),
                target.index()
            );
            wrote_goto = true;
        }
    }
    if wrote_goto {
        text.push('\n');
    }
    text.push('\n');
}

/// The actions a conflict rejected: every competitor except the resolution.
fn rejected_actions(conflict: &Conflict) -> impl Iterator<Item = Action> + '_ {
    let competitors: Vec<Action> = match &conflict.kind {
        ConflictKind::ShiftReduce { shift, reduce } => {
            vec![Action::Shift(*shift), Action::Reduce(*reduce)]
        }
        ConflictKind::ReduceReduce { rules } => {
            rules.iter().map(|rule| Action::Reduce(*rule)).collect()
        }
    };
    competitors
        .into_iter()
        .filter(move |action| *action != conflict.resolution)
}

/// Conflict counts of one state, accumulated while walking the sorted conflict list.
struct StateConflictCounts {
    /// The state's index.
    state: usize,
    /// Shift/reduce conflicts in the state.
    shift_reduce: usize,
    /// Reduce/reduce conflicts in the state.
    reduce_reduce: usize,
}

/// Renders the per-state conflict counts and the merge-artefact check outcome.
fn render_conflict_summary(text: &mut String, grammar: &LrGrammar, automaton: &Automaton) {
    if automaton.conflicts().is_empty() {
        text.push_str("No conflicts.\n");
        return;
    }
    text.push_str("Conflicts\n\n");
    let mut current: Option<StateConflictCounts> = None;
    let flush = |text: &mut String, current: Option<StateConflictCounts>| {
        if let Some(counts) = current {
            let mut parts = Vec::new();
            if counts.shift_reduce > 0 {
                parts.push(format!("{} shift/reduce", counts.shift_reduce));
            }
            if counts.reduce_reduce > 0 {
                parts.push(format!("{} reduce/reduce", counts.reduce_reduce));
            }
            let _ = writeln!(
                text,
                "State {} conflicts: {}",
                counts.state,
                parts.join(", ")
            );
        }
    };
    for conflict in automaton.conflicts() {
        let state = conflict.state.index();
        if current.as_ref().map(|counts| counts.state) != Some(state) {
            flush(text, current.take());
            current = Some(StateConflictCounts {
                state,
                shift_reduce: 0,
                reduce_reduce: 0,
            });
        }
        let counts = current.as_mut().expect("the current state was just set");
        match conflict.kind {
            ConflictKind::ShiftReduce { .. } => counts.shift_reduce += 1,
            ConflictKind::ReduceReduce { .. } => counts.reduce_reduce += 1,
        }
        if conflict.merge_artefact {
            let _ = writeln!(
                text,
                "State {state} lookahead {}: reduce/reduce conflict is an LALR merge artefact ({MERGE_ARTEFACT})",
                grammar.terminal_name(conflict.lookahead)
            );
        }
    }
    flush(text, current);
    match automaton.merge_check() {
        MergeArtefactCheck::NotNeeded => {}
        MergeArtefactCheck::Disabled => {
            text.push_str("Merge-artefact check disabled.\n");
        }
        MergeArtefactCheck::Verified { lr1_states } => {
            let _ = writeln!(
                text,
                "Merge-artefact check built {lr1_states} canonical LR(1) states."
            );
        }
        MergeArtefactCheck::BudgetExceeded { budget } => {
            let _ = writeln!(
                text,
                "Merge-artefact check inconclusive: canonical LR(1) exceeded {budget} states."
            );
        }
    }
}
