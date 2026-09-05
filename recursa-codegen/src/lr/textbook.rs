//! Textbook grammars with known LALR(1) outcomes: state counts, tables and conflicts.

use super::automaton::{Action, Automaton, AutomatonOptions, ConflictKind, NoPrecedence, StateId};
use super::grammar::{LrGrammar, TextbookGrammar};
use super::report::automaton_dump;
use super::tables::{LrTables, PACT_NINF};

/// Dragon book figure 4.1: `E -> E + T | T`, `T -> T * F | F`, `F -> ( E ) | id`.
pub(super) fn expression_grammar() -> LrGrammar {
    let mut grammar = TextbookGrammar::new(&["+", "*", "(", ")", "id"]);
    grammar.start("E");
    grammar.rule("E", "E + T");
    grammar.rule("E", "T");
    grammar.rule("T", "T * F");
    grammar.rule("T", "F");
    grammar.rule("F", "( E )");
    grammar.rule("F", "id");
    grammar.finish()
}

/// The dangling else: `S -> i S | i S e S | a`.
pub(super) fn dangling_else_grammar() -> LrGrammar {
    let mut grammar = TextbookGrammar::new(&["i", "e", "a"]);
    grammar.start("S");
    grammar.rule("S", "i S");
    grammar.rule("S", "i S e S");
    grammar.rule("S", "a");
    grammar.finish()
}

/// Dragon book example 4.58: LR(1) but not LALR(1) because merging `A -> c .` / `B -> c .`
/// states creates reduce/reduce conflicts.
pub(super) fn lalr_merge_grammar() -> LrGrammar {
    let mut grammar = TextbookGrammar::new(&["a", "b", "c", "d", "e"]);
    grammar.start("S");
    grammar.rule("S", "a A d");
    grammar.rule("S", "b B d");
    grammar.rule("S", "a B e");
    grammar.rule("S", "b A e");
    grammar.rule("A", "c");
    grammar.rule("B", "c");
    grammar.finish()
}

/// A grammar whose reduce/reduce conflict is genuine: `S -> A | B`, `A -> x`, `B -> x`.
pub(super) fn ambiguous_grammar() -> LrGrammar {
    let mut grammar = TextbookGrammar::new(&["x"]);
    grammar.start("S");
    grammar.rule("S", "A");
    grammar.rule("S", "B");
    grammar.rule("A", "x");
    grammar.rule("B", "x");
    grammar.finish()
}

fn build(grammar: &LrGrammar) -> Automaton {
    Automaton::build(grammar, &NoPrecedence, AutomatonOptions::default())
}

#[test]
fn expression_grammar_has_the_textbook_twelve_states_and_no_conflicts() {
    let grammar = expression_grammar();
    let automaton = build(&grammar);
    assert_eq!(automaton.states().len(), 12);
    assert!(automaton.conflicts().is_empty());
    assert_eq!(automaton.start_state(crate::TypeId::new(0)), Some(StateId::new(0)));

    // The state reached by `id` reduces `F -> id` on exactly FOLLOW(F).
    let id = grammar.nonterminal_by_name("F").unwrap();
    let id_state = automaton
        .states()
        .iter()
        .position(|state| {
            state.reductions().len() == 1
                && grammar.rule(state.reductions()[0].rule).lhs == id
                && grammar.rule(state.reductions()[0].rule).rhs.len() == 1
        })
        .expect("a state reduces F -> id");
    let lookaheads: Vec<&str> = automaton.states()[id_state].reductions()[0]
        .lookaheads
        .iter()
        .map(|terminal| grammar.terminal_name(terminal))
        .collect();
    assert_eq!(lookaheads, ["+", "*", ")", "$end"]);
}

#[test]
fn expression_grammar_dump_matches_the_bison_style_golden() {
    let grammar = expression_grammar();
    let automaton = build(&grammar);
    let dump = automaton_dump(&grammar, &automaton);
    let expected = include_str!("goldens/expression.dump.txt");
    assert_eq!(dump, expected, "automaton dump drifted:\n{dump}");
}

#[test]
fn expression_grammar_tables_drive_a_prefix_parse() {
    let grammar = expression_grammar();
    let automaton = build(&grammar);
    let tables = LrTables::pack(&grammar, &automaton);
    assert_eq!(tables.start_states, vec![0]);
    assert_eq!(tables.r2, vec![1, 3, 1, 3, 1, 3, 1]);
    assert_eq!(tables.value_pops, vec![1, 2, 1, 2, 1, 1, 0]);
    assert_eq!(tables.r1.len(), 7);
    assert_eq!(tables.defact.len(), 12);
    // Every state that only reduces has no explicit row.
    for state in automaton.states() {
        if state.shifts().next().is_none() && state.reductions().len() == 1 {
            assert_eq!(tables.pact[state.id().index()], PACT_NINF);
        }
    }

    let encoded = tables.encode();
    let decoded = LrTables::decode(&encoded).unwrap();
    assert_eq!(decoded, tables);

    let terminal = |name: &str| {
        (0..grammar.terminal_count())
            .map(|index| super::grammar::TerminalId::new(index as u32))
            .find(|terminal| grammar.terminal_name(*terminal) == name)
            .unwrap()
    };
    let input: Vec<_> = ["id", "+", "id", "*", "id"].map(terminal).to_vec();
    let trace = super::tables::simulate(&grammar, &decoded, 0, &input).unwrap();
    assert_eq!(
        trace.reductions,
        ["F -> id", "T -> F", "E -> T", "F -> id", "T -> F", "F -> id", "T -> T * F", "E -> E + T"]
    );
    assert_eq!(trace.consumed, 5);

    // Prefix acceptance: a token with no action ends the parse without consuming it.
    let input: Vec<_> = ["id", "+", "id", ")"].map(terminal).to_vec();
    let trace = super::tables::simulate(&grammar, &decoded, 0, &input).unwrap();
    assert_eq!(trace.consumed, 3);
    assert_eq!(trace.reductions.last().map(String::as_str), Some("E -> E + T"));
}

#[test]
fn dangling_else_reports_one_shift_reduce_conflict_resolved_as_shift() {
    let grammar = dangling_else_grammar();
    let automaton = build(&grammar);
    assert_eq!(automaton.conflicts().len(), 1);
    let conflict = &automaton.conflicts()[0];
    assert_eq!(grammar.terminal_name(conflict.lookahead), "e");
    let ConflictKind::ShiftReduce { shift, reduce } = conflict.kind else {
        panic!("expected a shift/reduce conflict");
    };
    assert_eq!(grammar.render_rule(reduce), "S -> i S");
    assert_eq!(conflict.resolution, Action::Shift(shift));
    assert!(!conflict.merge_artefact);
    let state = &automaton.states()[conflict.state.index()];
    let items: Vec<String> = state
        .items()
        .iter()
        .map(|item| item.render(&grammar))
        .collect();
    assert_eq!(
        items,
        ["S -> i S .", "S -> i S . e S"]
    );

}

#[test]
fn lalr_merge_artefacts_are_reduce_reduce_conflicts_absent_in_canonical_lr1() {
    let grammar = lalr_merge_grammar();
    let automaton = build(&grammar);
    let conflicts = automaton.conflicts();
    assert_eq!(conflicts.len(), 2);
    for conflict in conflicts {
        let ConflictKind::ReduceReduce { ref rules } = conflict.kind else {
            panic!("expected reduce/reduce conflicts");
        };
        let rendered: Vec<String> = rules.iter().map(|rule| grammar.render_rule(*rule)).collect();
        assert_eq!(rendered, ["A -> c", "B -> c"]);
        assert!(conflict.merge_artefact, "the merged `c` state is an LALR artefact");
        assert_eq!(conflict.resolution, Action::Reduce(rules[0]));
    }
    let lookaheads: Vec<&str> = conflicts
        .iter()
        .map(|conflict| grammar.terminal_name(conflict.lookahead))
        .collect();
    assert_eq!(lookaheads, ["d", "e"]);

}

#[test]
fn genuine_reduce_reduce_conflicts_carry_no_merge_artefact_note() {
    let grammar = ambiguous_grammar();
    let automaton = build(&grammar);
    assert_eq!(automaton.conflicts().len(), 1);
    let conflict = &automaton.conflicts()[0];
    assert_eq!(grammar.terminal_name(conflict.lookahead), "$end");
    assert!(!conflict.merge_artefact);
    assert!(matches!(conflict.kind, ConflictKind::ReduceReduce { .. }));
}

#[test]
fn every_start_rule_gets_its_own_start_state() {
    let mut grammar = TextbookGrammar::new(&["+", "id"]);
    grammar.start("E");
    grammar.start("T");
    grammar.rule("E", "E + T");
    grammar.rule("E", "T");
    grammar.rule("T", "id");
    let grammar = grammar.finish();
    let automaton = build(&grammar);
    assert_eq!(automaton.start_state(crate::TypeId::new(0)), Some(StateId::new(0)));
    assert_eq!(automaton.start_state(crate::TypeId::new(2)), Some(StateId::new(1)));
    assert!(automaton.conflicts().is_empty());
    let tables = LrTables::pack(&grammar, &automaton);
    assert_eq!(tables.start_states.len(), 3, "indexed by the highest grammar type");
    assert_eq!(tables.start_states[0], 0);
    assert_eq!(tables.start_states[2], 1);
}
