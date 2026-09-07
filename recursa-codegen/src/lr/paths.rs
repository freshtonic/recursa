#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Shortest symbol paths from a start state to every automaton state.
//!
//! A conflict diagnostic names its state by number, which says nothing about how the parser
//! gets there. The shortest path does: rendered like the prefix of a bison counterexample,
//! `stmt -> SELECT target_list . FROM` reads as "after `SELECT target_list`, on `FROM`". The
//! paths come from one breadth-first search over the automaton's transitions, seeded with every
//! start state in start-rule order and visiting each state's transitions in ascending symbol
//! order, so equal-length candidates are settled deterministically: the earlier start type, then
//! the smaller symbol at the first point of difference.

use std::collections::VecDeque;

use super::automaton::{Automaton, StateId};
use super::grammar::{LrGrammar, Symbol, TerminalId};
use crate::TypeId;

#[derive(Clone, Debug, Eq, PartialEq)]
/// The shortest path to one state: the grammar type whose start state it leaves from and the
/// symbols of the transitions taken.
pub struct SymbolPath {
    /// The grammar type of the start state.
    pub start: TypeId,
    /// The transition symbols, in order.
    pub symbols: Vec<Symbol>,
}

/// One breadth-first search step recorded per state: the state and symbol it was reached by.
#[derive(Clone, Copy)]
struct Reached {
    /// The start type the search entered from.
    start: TypeId,
    /// The previous state and the symbol of the transition, absent for a start state.
    from: Option<(StateId, Symbol)>,
}

/// Computes the shortest path to every state, indexed by state; `None` for a state no start
/// state reaches, which a well-formed automaton has none of.
pub fn shortest_paths(automaton: &Automaton) -> Vec<Option<SymbolPath>> {
    let states = automaton.states();
    let mut reached: Vec<Option<Reached>> = vec![None; states.len()];
    let mut queue = VecDeque::new();
    for (type_id, state) in automaton.start_states() {
        if reached[state.index()].is_none() {
            reached[state.index()] = Some(Reached {
                start: *type_id,
                from: None,
            });
            queue.push_back(*state);
        }
    }
    while let Some(state) = queue.pop_front() {
        let start = reached[state.index()]
            .expect("a queued state was reached")
            .start;
        for (symbol, target) in states[state.index()].transitions() {
            if reached[target.index()].is_none() {
                reached[target.index()] = Some(Reached {
                    start,
                    from: Some((state, *symbol)),
                });
                queue.push_back(*target);
            }
        }
    }
    (0..states.len())
        .map(|index| {
            let entry = reached[index]?;
            let mut symbols = Vec::new();
            let mut current = entry.from;
            while let Some((previous, symbol)) = current {
                symbols.push(symbol);
                current = reached[previous.index()]
                    .expect("a reached state's predecessor was reached first")
                    .from;
            }
            symbols.reverse();
            Some(SymbolPath {
                start: entry.start,
                symbols,
            })
        })
        .collect()
}

/// Renders a path bison-style: the start type's nonterminal, an arrow, the symbols, a dot, and
/// the lookahead when there is one, as in `S -> i S . e`.
pub fn render_path(
    grammar: &LrGrammar,
    path: &SymbolPath,
    lookahead: Option<TerminalId>,
) -> String {
    let start = grammar
        .starts()
        .iter()
        .find(|start| start.type_id == path.start)
        .expect("a path starts at a type with a start rule");
    let head = grammar.rule(start.rule).rhs[0];
    let mut text = grammar.symbol_name(head).to_owned();
    text.push_str(" ->");
    for symbol in &path.symbols {
        text.push(' ');
        text.push_str(grammar.symbol_name(*symbol));
    }
    text.push_str(" .");
    if let Some(lookahead) = lookahead {
        text.push(' ');
        text.push_str(grammar.terminal_name(lookahead));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::super::automaton::{Automaton, AutomatonOptions, NoPrecedence};
    use super::super::textbook::expression_grammar;
    use super::*;

    #[test]
    fn every_expression_state_has_a_shortest_path_from_the_one_start_state() {
        let grammar = expression_grammar();
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        let paths = shortest_paths(&automaton);
        let rendered: Vec<String> = paths
            .iter()
            .map(|path| {
                render_path(
                    &grammar,
                    path.as_ref().expect("every state is reachable"),
                    None,
                )
            })
            .collect();
        // State 6 (`F -> ( E . )`) is reached by `( E`, not by any longer derivation of `E`;
        // state 9 (`F -> ( E ) .`) needs the three symbols and no fewer.
        assert_eq!(
            rendered,
            [
                "E -> .",
                "E -> ( .",
                "E -> id .",
                "E -> E .",
                "E -> T .",
                "E -> F .",
                "E -> ( E .",
                "E -> E + .",
                "E -> T * .",
                "E -> ( E ) .",
                "E -> E + T .",
                "E -> T * F .",
            ]
        );
        let plus = TerminalId::new(0);
        assert_eq!(
            render_path(&grammar, paths[6].as_ref().unwrap(), Some(plus)),
            "E -> ( E . +"
        );
    }
}
