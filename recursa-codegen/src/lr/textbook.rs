//! Textbook grammars with known LALR(1) outcomes: state counts, tables and conflicts.

use super::automaton::{
    Action, Automaton, AutomatonOptions, ConflictKind, MergeArtefactCheck, NoPrecedence,
    Resolution, StateId, canonical_build_count, reset_canonical_build_count,
};
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
    assert_eq!(
        automaton.start_state(crate::TypeId::new(0)),
        Some(StateId::new(0))
    );

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
        [
            "F -> id",
            "T -> F",
            "E -> T",
            "F -> id",
            "T -> F",
            "F -> id",
            "T -> T * F",
            "E -> E + T"
        ]
    );
    assert_eq!(trace.consumed, 5);

    // Prefix acceptance: a token with no action ends the parse without consuming it.
    let input: Vec<_> = ["id", "+", "id", ")"].map(terminal).to_vec();
    let trace = super::tables::simulate(&grammar, &decoded, 0, &input).unwrap();
    assert_eq!(trace.consumed, 3);
    assert_eq!(
        trace.reductions.last().map(String::as_str),
        Some("E -> E + T")
    );
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
    assert_eq!(items, ["S -> i S .", "S -> i S . e S"]);
}

#[test]
fn scoped_shift_resolution_uses_source_owners_and_is_recorded_in_the_dump() {
    let grammar = dangling_else_grammar();
    let mut automaton = build(&grammar);
    let declaration = crate::LrConflictResolution {
        owners: vec![crate::LrRuleOwner::Type(crate::TypeId::new(0))],
        against: vec![crate::LrRuleOwner::Type(crate::TypeId::new(0))],
        action: crate::LrConflictAction::Shift,
        lookaheads: vec![crate::KindId::new(1)],
        expected: 1,
    };
    automaton
        .apply_scoped_resolutions(&grammar, &[declaration])
        .expect("the exact dangling-else cell matches");
    assert!(automaton.conflicts().is_empty());
    assert_eq!(automaton.scoped_resolutions().len(), 1);
    assert_eq!(
        automaton.scoped_resolutions()[0].resolution,
        Resolution::Shift
    );
    let dump = automaton_dump(&grammar, &automaton);
    assert!(
        dump.contains("resolved by scoped declaration 0 (shift)"),
        "{dump}"
    );
}

#[test]
fn scoped_resolution_validation_is_atomic_and_fail_closed() {
    assert_eq!(
        crate::lr::report::INVALID_SCOPED_RESOLUTION,
        "RCA0407",
        "invalid scoped declarations are reported under the registered analysis code",
    );
    let grammar = dangling_else_grammar();
    let base = crate::LrConflictResolution {
        owners: vec![crate::LrRuleOwner::Type(crate::TypeId::new(0))],
        against: vec![crate::LrRuleOwner::Type(crate::TypeId::new(0))],
        action: crate::LrConflictAction::Shift,
        lookaheads: vec![crate::KindId::new(1)],
        expected: 1,
    };
    for (declarations, expected_message) in [
        (
            vec![crate::LrConflictResolution {
                expected: 2,
                ..base.clone()
            }],
            "expected 2 conflict cell(s) but matched 1",
        ),
        (
            vec![crate::LrConflictResolution {
                lookaheads: vec![crate::KindId::new(0)],
                ..base.clone()
            }],
            "matched no conflict on lookahead `i`",
        ),
        (vec![base.clone(), base.clone()], "both select state"),
    ] {
        let mut automaton = build(&grammar);
        let errors = automaton
            .apply_scoped_resolutions(&grammar, &declarations)
            .expect_err("the declaration must fail closed");
        assert!(
            errors.iter().any(|error| error.contains(expected_message)),
            "{errors:?}"
        );
        assert_eq!(automaton.conflicts().len(), 1, "validation is atomic");
        assert!(automaton.scoped_resolutions().is_empty());
    }
}

#[test]
fn scoped_reduce_resolution_selects_the_authored_winner_of_reduce_reduce() {
    let mut textbook = TextbookGrammar::new(&["x", "tail"]);
    textbook.start("S");
    textbook.rule("S", "A tail");
    textbook.rule("S", "B tail");
    textbook.rule("A", "x");
    textbook.rule("B", "x");
    let grammar = textbook.finish();
    reset_canonical_build_count();
    let mut automaton = Automaton::build(
        &grammar,
        &NoPrecedence,
        AutomatonOptions {
            check_merge_artefacts: false,
            ..AutomatonOptions::default()
        },
    );
    let conflict = automaton
        .conflicts()
        .iter()
        .find(|conflict| grammar.terminal_name(conflict.lookahead) == "tail")
        .expect("A/B has a reduce/reduce cell on tail");
    assert!(matches!(conflict.kind, ConflictKind::ReduceReduce { .. }));
    // The synthetic start follows S, then A and B; select B even though bison's fallback chose A.
    automaton
        .apply_scoped_resolutions(
            &grammar,
            &[crate::LrConflictResolution {
                owners: vec![crate::LrRuleOwner::Type(crate::TypeId::new(3))],
                against: vec![crate::LrRuleOwner::Type(crate::TypeId::new(2))],
                action: crate::LrConflictAction::Reduce,
                lookaheads: vec![crate::KindId::new(1)],
                expected: 1,
            }],
        )
        .expect("the exact competing source owners select one winner");
    automaton.classify_merge_artefacts(
        &grammar,
        AutomatonOptions {
            lr1_state_budget: 0,
            ..AutomatonOptions::default()
        },
    );
    assert!(automaton.conflicts().is_empty());
    assert_eq!(automaton.merge_check(), &MergeArtefactCheck::NotNeeded);
    assert_eq!(canonical_build_count(), 0);
    let resolved = &automaton.scoped_resolutions()[0];
    assert_eq!(resolved.resolution, Resolution::Reduce);
    assert_eq!(
        grammar.render_rule(resolved.reduced.expect("reduce winner")),
        "B -> x"
    );
}

#[test]
fn an_exhausted_canonical_budget_leaves_a_hard_conflict_and_reports_inconclusive() {
    let grammar = lalr_merge_grammar();
    reset_canonical_build_count();
    let automaton = Automaton::build(
        &grammar,
        &NoPrecedence,
        AutomatonOptions {
            lr1_state_budget: 0,
            ..AutomatonOptions::default()
        },
    );
    assert_eq!(
        automaton.merge_check(),
        &MergeArtefactCheck::BudgetExceeded { budget: 0 }
    );
    assert_eq!(automaton.conflicts().len(), 2);
    assert_eq!(canonical_build_count(), 1);
    let diagnostics = super::report::conflict_diagnostics(&grammar, &automaton);
    assert_eq!(diagnostics.len(), 2);
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.code, super::report::REDUCE_REDUCE);
        assert_eq!(
            diagnostic.notes,
            [
                "merge-artefact classification is inconclusive: canonical LR(1) exceeded the \
              0-state budget"
            ]
        );
    }
    assert!(
        automaton_dump(&grammar, &automaton)
            .contains("Merge-artefact check inconclusive: canonical LR(1) exceeded 0 states.")
    );
}

#[test]
fn inconclusive_reclassification_clears_an_older_verified_artefact_verdict() {
    let grammar = lalr_merge_grammar();
    let mut automaton = build(&grammar);
    assert!(
        automaton
            .conflicts()
            .iter()
            .all(|conflict| conflict.merge_artefact)
    );

    reset_canonical_build_count();
    automaton.classify_merge_artefacts(
        &grammar,
        AutomatonOptions {
            lr1_state_budget: 0,
            ..AutomatonOptions::default()
        },
    );
    assert_eq!(canonical_build_count(), 1);
    assert_eq!(
        automaton.merge_check(),
        &MergeArtefactCheck::BudgetExceeded { budget: 0 }
    );
    assert!(
        automaton
            .conflicts()
            .iter()
            .all(|conflict| !conflict.merge_artefact)
    );
    assert!(
        super::report::conflict_diagnostics(&grammar, &automaton)
            .iter()
            .all(|diagnostic| diagnostic
                .notes
                .iter()
                .all(|note| !note.starts_with(super::report::MERGE_ARTEFACT)))
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
        let rendered: Vec<String> = rules
            .iter()
            .map(|rule| grammar.render_rule(*rule))
            .collect();
        assert_eq!(rendered, ["A -> c", "B -> c"]);
        assert!(
            conflict.merge_artefact,
            "the merged `c` state is an LALR artefact"
        );
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
    assert_eq!(
        automaton.start_state(crate::TypeId::new(0)),
        Some(StateId::new(0))
    );
    assert_eq!(
        automaton.start_state(crate::TypeId::new(2)),
        Some(StateId::new(1))
    );
    assert!(automaton.conflicts().is_empty());
    let tables = LrTables::pack(&grammar, &automaton);
    assert_eq!(
        tables.start_states.len(),
        3,
        "indexed by the highest grammar type"
    );
    assert_eq!(tables.start_states[0], 0);
    assert_eq!(tables.start_states[2], 1);
}

/// pg-sql's `ColId` beside `type_function_name` (#125): two token wrappers over `IDENT` that
/// several roots can end in. `Expr` continues past the wrapper (`ColumnRef -> ColId`,
/// `FuncCall -> TypeFunctionName ( )`), `FuncName` ends in the later-declared wrapper alone
/// (`QualifiedName -> ColId . ColId | TypeFunctionName`), and `Name` is genuinely ambiguous at
/// the end of input (`ColId | TypeFunctionName`). Every root reaches the one LR(0) state
/// `{ColIdText -> IDENT ., TypeFunctionNameText -> IDENT .}`, whose merged lookaheads hold the
/// end marker for both wrappers.
fn wrapper_grammar() -> LrGrammar {
    let mut grammar = TextbookGrammar::new(&["IDENT", "(", ")", "."]);
    grammar.token_rule("ColIdText", "IDENT");
    grammar.token_rule("TypeFunctionNameText", "IDENT");
    grammar.rule("ColId", "ColIdText");
    grammar.rule("TypeFunctionName", "TypeFunctionNameText");
    grammar.rule("Expr", "ColumnRef");
    grammar.rule("Expr", "FuncCall");
    grammar.rule("ColumnRef", "ColId");
    grammar.rule("FuncCall", "TypeFunctionName ( )");
    grammar.rule("FuncName", "QualifiedName");
    grammar.rule("FuncName", "TypeFunctionName");
    grammar.rule("QualifiedName", "ColId . ColId");
    grammar.rule("Name", "ColId");
    grammar.rule("Name", "TypeFunctionName");
    for root in [
        "Expr",
        "FuncName",
        "Name",
        "ColIdText",
        "TypeFunctionNameText",
    ] {
        grammar.start(root);
    }
    grammar.finish()
}

/// Simulates the tables from the start state of `root` over `input`, returning the reductions.
fn reductions_of(
    grammar: &LrGrammar,
    automaton: &Automaton,
    root: &str,
    input: &[&str],
) -> Result<Vec<String>, super::tables::SimulationError> {
    let tables = LrTables::pack(grammar, automaton);
    let terminal = |name: &str| {
        (0..grammar.terminal_count())
            .map(|index| super::grammar::TerminalId::new(index as u32))
            .find(|terminal| grammar.terminal_name(*terminal) == name)
            .unwrap()
    };
    let root = grammar.nonterminal_by_name(root).unwrap();
    let start = automaton
        .start_states()
        .iter()
        .find(|(_, state)| {
            let kernel = automaton.states()[state.index()].kernel();
            grammar.rule(kernel[0].rule).rhs == [super::grammar::Symbol::Nonterminal(root)]
        })
        .map(|(_, state)| state.index() as u32)
        .expect("the root has a start state");
    let input: Vec<_> = input.iter().map(|name| terminal(name)).collect();
    super::tables::simulate(grammar, &tables, start, &input).map(|trace| trace.reductions)
}

#[test]
fn end_marker_conflicts_between_token_wrappers_are_settled_by_declaration_order() {
    let grammar = wrapper_grammar();
    let automaton = build(&grammar);
    assert!(
        automaton.conflicts().is_empty(),
        "{:?}",
        automaton.conflicts()
    );

    // The one merged state records the resolution; the earliest-declared wrapper wins.
    let col_id_text = super::grammar::RuleId::new(0);
    let type_function_name_text = super::grammar::RuleId::new(1);
    let [resolution] = automaton.end_marker_resolutions() else {
        panic!("one resolution: {:?}", automaton.end_marker_resolutions());
    };
    assert_eq!(resolution.rules, vec![col_id_text, type_function_name_text]);
    assert_eq!(resolution.chosen, col_id_text);
    let state = &automaton.states()[resolution.state.index()];
    let kernel: Vec<String> = state
        .kernel()
        .iter()
        .map(|item| item.render(&grammar))
        .collect();
    assert_eq!(
        kernel,
        ["ColIdText -> IDENT .", "TypeFunctionNameText -> IDENT ."]
    );

    // Every continuation terminal keeps its own explicit action: only `$end` was decided.
    let terminal = |name: &str| {
        (0..grammar.terminal_count())
            .map(|index| super::grammar::TerminalId::new(index as u32))
            .find(|terminal| grammar.terminal_name(*terminal) == name)
            .unwrap()
    };
    assert_eq!(
        state.actions()[&grammar.end_marker()],
        Action::Reduce(col_id_text)
    );
    assert_eq!(
        state.actions()[&terminal("(")],
        Action::Reduce(type_function_name_text)
    );
    assert_eq!(state.actions()[&terminal(".")], Action::Reduce(col_id_text));
    assert_eq!(state.default_reduction(), Some(col_id_text));

    let dump = automaton_dump(&grammar, &automaton);
    assert!(dump.contains(
        "    Conflict between rules 0 and 1 resolved on $end by declaration order (reduce using rule 0).\n"
    ), "{dump}");
    assert!(
        dump.contains(
            "No conflicts.\n1 reduce/reduce conflicts on $end resolved by declaration order.\n"
        ),
        "{dump}"
    );

    // A real root that continues past the wrapper reduces the right wrapper on the real
    // continuation terminal, and the wrapper the end of input needs when the root ends there.
    let reductions = |root, input: &[&str]| reductions_of(&grammar, &automaton, root, input);
    assert_eq!(
        reductions("Expr", &["IDENT", "(", ")"]).unwrap(),
        [
            "TypeFunctionNameText -> IDENT",
            "TypeFunctionName -> TypeFunctionNameText",
            "FuncCall -> TypeFunctionName ( )",
            "Expr -> FuncCall"
        ]
    );
    assert_eq!(
        reductions("Expr", &["IDENT"]).unwrap(),
        [
            "ColIdText -> IDENT",
            "ColId -> ColIdText",
            "ColumnRef -> ColId",
            "Expr -> ColumnRef"
        ]
    );
    assert_eq!(
        reductions("FuncName", &["IDENT", ".", "IDENT"]).unwrap(),
        [
            "ColIdText -> IDENT",
            "ColId -> ColIdText",
            "ColIdText -> IDENT",
            "ColId -> ColIdText",
            "QualifiedName -> ColId . ColId",
            "FuncName -> QualifiedName"
        ]
    );
    // A genuinely ambiguous root yields the earliest-declared wrapper.
    assert_eq!(
        reductions("Name", &["IDENT"]).unwrap(),
        ["ColIdText -> IDENT", "ColId -> ColIdText", "Name -> ColId"]
    );
    // The limit of the rule: a root context that can end only in the later-declared wrapper takes
    // the earlier one at the end of input and then finds no action. The failing position is the
    // end of input.
    assert_eq!(reductions("FuncName", &["IDENT"]).unwrap_err().position, 1);
    // The bare wrapper roots are unaffected: their start states reach one-item states.
    assert_eq!(
        reductions("ColIdText", &["IDENT"]).unwrap(),
        ["ColIdText -> IDENT"]
    );
    assert_eq!(
        reductions("TypeFunctionNameText", &["IDENT"]).unwrap(),
        ["TypeFunctionNameText -> IDENT"]
    );
}

#[test]
fn wrapper_conflicts_that_also_exist_on_a_real_terminal_stay_conflicts() {
    // A root that continues past either wrapper with `(` and can end in either makes the
    // wrappers compete on `(` as well as on `$end`: neither is settled, and both are reported.
    let mut grammar = TextbookGrammar::new(&["IDENT", "(", ")"]);
    grammar.token_rule("ColIdText", "IDENT");
    grammar.token_rule("TypeFunctionNameText", "IDENT");
    grammar.rule("ColId", "ColIdText");
    grammar.rule("TypeFunctionName", "TypeFunctionNameText");
    grammar.rule("Expr", "ColId ( )");
    grammar.rule("Expr", "TypeFunctionName ( )");
    grammar.rule("Expr", "ColId");
    grammar.rule("Expr", "TypeFunctionName");
    grammar.start("Expr");
    let grammar = grammar.finish();
    let automaton = build(&grammar);
    assert!(automaton.end_marker_resolutions().is_empty());
    let lookaheads: Vec<&str> = automaton
        .conflicts()
        .iter()
        .map(|conflict| grammar.terminal_name(conflict.lookahead))
        .collect();
    assert_eq!(lookaheads, ["(", "$end"]);
    assert!(
        automaton
            .conflicts()
            .iter()
            .all(|conflict| matches!(conflict.kind, ConflictKind::ReduceReduce { .. }))
    );
}

/// One shared `opt(X)` at two sites whose contexts follow it and `Y -> X` by swapped terminals
/// (#125): `A -> a opt(X) c` beside `A2 -> a Y d`, and `B -> b opt(X) d` beside `B2 -> b Y c`.
/// Both contexts reach the one LR(0) state `{opt(X) -> X ., Y -> X .}`, whose merged lookaheads
/// put `c` and `d` on both reductions; no canonical LR(1) state has that conflict.
fn shared_optional_grammar() -> LrGrammar {
    let mut grammar = TextbookGrammar::new(&["a", "b", "c", "d", "x"]);
    grammar.start("S");
    grammar.helper_rule("opt(X)", "");
    grammar.helper_rule("opt(X)", "X");
    grammar.rule("S", "A");
    grammar.rule("S", "A2");
    grammar.rule("S", "B");
    grammar.rule("S", "B2");
    grammar.rule("A", "a opt(X) c");
    grammar.rule("A2", "a Y d");
    grammar.rule("B", "b opt(X) d");
    grammar.rule("B2", "b Y c");
    grammar.rule("Y", "X");
    grammar.rule("X", "x");
    grammar.finish()
}

#[test]
fn a_shared_helper_whose_merged_lookaheads_conflict_is_copied_per_site_class() {
    let shared = shared_optional_grammar();
    let automaton = build(&shared);
    let lookaheads: Vec<(&str, bool)> = automaton
        .conflicts()
        .iter()
        .map(|conflict| {
            (
                shared.terminal_name(conflict.lookahead),
                conflict.merge_artefact,
            )
        })
        .collect();
    assert_eq!(lookaheads, [("c", true), ("d", true)]);
    let helper = shared.nonterminal_by_name("opt(X)").unwrap();
    assert_eq!(
        super::helpers::polluted_helpers(&shared, &automaton),
        [helper].into()
    );
    let sites = super::helpers::sites_of(&shared, helper);
    assert_eq!(
        sites
            .iter()
            .map(|site| shared.render_rule(site.rule))
            .collect::<Vec<_>>(),
        ["A -> a opt(X) c", "B -> b opt(X) d"]
    );

    reset_canonical_build_count();
    let built = super::helpers::build_with_helper_copies(
        shared.clone(),
        |_| Ok(NoPrecedence),
        AutomatonOptions {
            lr1_state_budget: 0,
            ..AutomatonOptions::default()
        },
    )
    .unwrap();
    assert!(
        built.automaton.conflicts().is_empty(),
        "{:?}",
        built.automaton.conflicts()
    );
    assert_eq!(built.rounds, 2);
    assert_eq!(
        built.automaton.merge_check(),
        &MergeArtefactCheck::NotNeeded
    );
    assert_eq!(canonical_build_count(), 0);
    let [split] = built.splits.as_slice() else {
        panic!("one split: {:?}", built.splits);
    };
    assert_eq!(split.original, helper);
    let [copy] = split.copies.as_slice() else {
        panic!("one copy: {:?}", split.copies);
    };
    let grammar = &built.grammar;
    assert_eq!(grammar.nonterminal(copy.copy).name, "opt(X)@B");
    assert_eq!(copy.sites, [sites[1]]);
    assert_eq!(
        grammar.nonterminal(copy.copy).origin,
        super::grammar::NonterminalOrigin::HelperCopy { original: helper }
    );
    // The copy's rules are the original's, appended; the original keeps its first site and the
    // copy owns the second.
    let rendered: Vec<String> = (0..grammar.rules().len())
        .map(|index| grammar.render_rule(super::grammar::RuleId::new(index as u32)))
        .collect();
    assert_eq!(
        rendered,
        [
            "$accept(S) -> S",
            "opt(X) -> %empty",
            "opt(X) -> X",
            "S -> A",
            "S -> A2",
            "S -> B",
            "S -> B2",
            "A -> a opt(X) c",
            "A2 -> a Y d",
            "B -> b opt(X)@B d",
            "B2 -> b Y c",
            "Y -> X",
            "X -> x",
            "opt(X)@B -> %empty",
            "opt(X)@B -> X",
        ]
    );
    let original_rules: Vec<&super::grammar::Rule> = grammar
        .rules_of(helper)
        .iter()
        .map(|rule| grammar.rule(*rule))
        .collect();
    let copy_rules: Vec<&super::grammar::Rule> = grammar
        .rules_of(copy.copy)
        .iter()
        .map(|rule| grammar.rule(*rule))
        .collect();
    for (original, copied) in original_rules.iter().zip(&copy_rules) {
        assert_eq!(original.action, copied.action);
        assert_eq!(original.fold, copied.fold);
        assert_eq!(original.tail, copied.tail);
        assert_eq!(original.value_pops, copied.value_pops);
    }
    let dump = automaton_dump(grammar, &built.automaton);
    assert!(
        dump.ends_with("No conflicts.\n\nHelper copies\n\nopt(X)@B copies opt(X)\n"),
        "{dump}"
    );

    // Every sentence parses to its own derivation once the copies separate the contexts.
    let reductions = |input: &[&str]| reductions_of(grammar, &built.automaton, "S", input).unwrap();
    assert_eq!(
        reductions(&["a", "x", "c"]),
        ["X -> x", "opt(X) -> X", "A -> a opt(X) c", "S -> A"]
    );
    assert_eq!(
        reductions(&["a", "x", "d"]),
        ["X -> x", "Y -> X", "A2 -> a Y d", "S -> A2"]
    );
    assert_eq!(
        reductions(&["a", "c"]),
        ["opt(X) -> %empty", "A -> a opt(X) c", "S -> A"]
    );
    assert_eq!(
        reductions(&["b", "x", "d"]),
        ["X -> x", "opt(X)@B -> X", "B -> b opt(X)@B d", "S -> B"]
    );
    assert_eq!(
        reductions(&["b", "x", "c"]),
        ["X -> x", "Y -> X", "B2 -> b Y c", "S -> B2"]
    );
    assert_eq!(
        reductions(&["b", "d"]),
        ["opt(X)@B -> %empty", "B -> b opt(X)@B d", "S -> B"]
    );
}

#[test]
fn sites_that_share_a_state_share_a_copy_and_a_genuine_conflict_copies_nothing_useful() {
    // Both sites of `opt(X)` sit in the state after `a`, so a copy per site would make the
    // parser choose between `A` and `B` before `c`; they stay one class, and the reduce/reduce
    // conflict on `c` after `x`, which canonical LR(1) has too, remains.
    let mut grammar = TextbookGrammar::new(&["a", "c", "e", "f", "x"]);
    grammar.start("S");
    grammar.helper_rule("opt(X)", "");
    grammar.helper_rule("opt(X)", "X");
    grammar.rule("S", "A");
    grammar.rule("S", "B");
    grammar.rule("A", "a opt(X) c e");
    grammar.rule("B", "a opt(X) c f");
    grammar.rule("Y", "X");
    grammar.rule("A", "a Y c f");
    grammar.rule("X", "x");
    let shared = grammar.finish();
    let built = super::helpers::build_with_helper_copies(
        shared.clone(),
        |_| Ok(NoPrecedence),
        AutomatonOptions::default(),
    )
    .unwrap();
    assert!(built.splits.is_empty(), "{:?}", built.splits);
    assert_eq!(built.rounds, 1);
    assert_eq!(built.grammar, shared);
    let genuine: Vec<(&str, bool)> = built
        .automaton
        .conflicts()
        .iter()
        .map(|conflict| {
            (
                shared.terminal_name(conflict.lookahead),
                conflict.merge_artefact,
            )
        })
        .collect();
    assert_eq!(genuine, [("c", false)]);
}
