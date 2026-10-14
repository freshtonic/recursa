//! Lowering of the source-analysis fixtures; the pg-sql run is the `lr_pg_sql` integration test.

use std::path::Path;

use super::LrAnalysis;
use super::automaton::{Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
use super::grammar::{
    Associativity, ElementValue, LrGrammar, NonterminalOrigin, OptionalContent, RuleAction, Symbol,
    TerminalId,
};
use super::lowering::{TypeRoles, lower};
use super::precedence::PrecedenceResolver;
use super::report::{automaton_dump, conflict_diagnostics};
use super::tables::{LrTables, simulate};
use crate::materialization::LoweredGrammar;
use crate::source_analysis::{SourceAnalysisInput, analyze_source_tree};

/// Runs source analysis over one Cargo package root and selected root file.
fn lowered_tree(manifest_dir: &Path, root: &str) -> LoweredGrammar {
    analyze_source_tree(
        SourceAnalysisInput::rust_2024(manifest_dir, root),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| {
        panic!(
            "{} failed source analysis:\n{error}",
            manifest_dir.display()
        )
    })
    .analyzed
    .lowered
}

/// Runs source analysis over the container grammar of the table-driven generation fixture.
fn lowered_containers() -> LoweredGrammar {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generation/table-driven-containers");
    lowered_tree(&fixture, "src/grammar.rs")
}

/// Renders every rule of one nonterminal as `lhs -> rhs`.
fn rules_of(grammar: &LrGrammar, name: &str) -> Vec<String> {
    let id = grammar
        .nonterminal_by_name(name)
        .unwrap_or_else(|| panic!("no nonterminal named {name}"));
    grammar
        .rules_of(id)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect()
}

/// Runs source analysis over one fixture under `tests/fixtures/source-analysis`.
fn lowered_fixture(name: &str) -> LoweredGrammar {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/source-analysis")
        .join(name);
    lowered_tree(&fixture, "grammar.rs")
}

/// Lowers a fixture to an LR grammar with the roles its correlations state.
fn lr_fixture(name: &str) -> (LoweredGrammar, LrGrammar) {
    let lowered = lowered_fixture(name);
    let roles = TypeRoles::from_lowered(&lowered);
    let grammar = lower(&lowered.grammar, &roles).expect("fixtures have no text predicates");
    (lowered, grammar)
}

/// Finds a terminal by its token-kind name.
fn terminal(grammar: &LrGrammar, name: &str) -> TerminalId {
    (0..grammar.terminal_count())
        .map(|index| TerminalId::new(index as u32))
        .find(|terminal| grammar.terminal_name(*terminal) == name)
        .unwrap_or_else(|| panic!("no terminal named {name}"))
}

/// Finds the grammar type whose normalized name is `name`.
fn type_named(lowered: &LoweredGrammar, name: &str) -> crate::TypeId {
    let index = lowered
        .grammar
        .types
        .iter()
        .position(|description| description.name == name)
        .unwrap_or_else(|| panic!("no type named {name}"));
    crate::TypeId::new(index as u32)
}

#[test]
fn the_merged_enclosure_fixture_is_conflict_free_with_its_two_armed_close() {
    // The grammar Pretty's enclosure proof reads in #142: the close of the grouping atom names
    // the merged kind beside the token it merges, so the automaton chooses between the arms by
    // the terminal the feed produced and never by a lookahead.
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generation/merged-enclosure");
    let lowered = lowered_tree(&fixture, "src/grammar.rs");
    let roles = TypeRoles::from_lowered(&lowered);
    let grammar = lower(&lowered.grammar, &roles).expect("the fixture has no text predicate");
    assert_eq!(
        rules_of(&grammar, "grammar::QueryClose"),
        [
            "grammar::QueryClose -> RPAREN",
            "grammar::QueryClose -> RPAREN_SELECT_LA",
        ],
        "both close arms survive lowering, and neither is a substitution twin"
    );
    let precedence = PrecedenceResolver::new(&grammar).expect("the resolver is infallible");
    let automaton = Automaton::build(&grammar, &precedence, AutomatonOptions::default());
    assert!(
        automaton.conflicts().is_empty(),
        "{:#?}",
        automaton.conflicts()
    );
}

#[test]
fn arith_operator_variants_lower_to_plain_rules_with_declared_levels() {
    let (_, grammar) = lr_fixture("arith");
    let expr = grammar
        .nonterminal_by_name("arith::Expr")
        .expect("the expression enum");
    let rules: Vec<String> = grammar
        .rules_of(expr)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    assert_eq!(
        rules,
        [
            "arith::Expr -> arith::Atom<arith::IntAtom>",
            "arith::Expr -> arith::Atom<arith::IdentAtom>",
            "arith::Expr -> LPAREN arith::Expr RPAREN",
            "arith::Expr -> MINUS arith::Expr",
            "arith::Expr -> arith::Expr PLUS arith::Expr",
            "arith::Expr -> arith::Expr MINUS arith::Expr",
            "arith::Expr -> arith::Expr STAR arith::Expr",
            "arith::Expr -> arith::Expr SLASH arith::Expr",
        ]
    );
    let precedences: Vec<Option<(u8, Associativity)>> = grammar
        .rules_of(expr)
        .iter()
        .map(|rule| {
            grammar
                .rule(*rule)
                .precedence
                .map(|precedence| (precedence.level, precedence.associativity))
        })
        .collect();
    assert_eq!(
        precedences,
        [
            None,
            None,
            None,
            Some((30, Associativity::Right)),
            Some((10, Associativity::Left)),
            Some((10, Associativity::Left)),
            Some((20, Associativity::Left)),
            Some((20, Associativity::Left)),
        ]
    );
    let value_pops: Vec<u32> = grammar
        .rules_of(expr)
        .iter()
        .map(|rule| grammar.rule(*rule).value_pops)
        .collect();
    assert_eq!(value_pops, [1, 1, 1, 1, 2, 2, 2, 2]);

    // Without precedence the naive lowering is ambiguous exactly where declared precedence
    // resolves it: every conflict is a shift/reduce between an infix reduce and an operator shift.
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(!automaton.conflicts().is_empty());
    for conflict in automaton.conflicts() {
        let ConflictKind::ShiftReduce { reduce, .. } = conflict.kind else {
            panic!("the naive expression lowering only produces shift/reduce conflicts");
        };
        assert!(
            grammar.rule(reduce).precedence.is_some(),
            "every competing reduce is an operator rule carrying its declared level"
        );
    }
    let diagnostics = conflict_diagnostics(&grammar, &automaton);
    assert_eq!(diagnostics.len(), automaton.conflicts().len());
    // The fixed-token group around the parenthesised atom's operand is a cardinality-one
    // attachment, so it inlines into `Expr`'s own rule instead of a private nonterminal: no
    // `arith::Expr::__recursa_attachment` nonterminal exists any more.
    assert!(
        grammar
            .nonterminal_by_name("arith::Expr::__recursa_attachment")
            .is_none()
    );
    assert!(
        grammar
            .rules_of(expr)
            .iter()
            .any(|rule| grammar.render_rule(*rule) == "arith::Expr -> LPAREN arith::Expr RPAREN")
    );
}

#[test]
fn sql_fixture_inlines_fixed_tokens_and_parses_from_every_start_state() {
    let (lowered, grammar) = lr_fixture("sql");
    let create_table = grammar
        .nonterminal_by_name("sql::CreateTable")
        .expect("the node application");
    let rules: Vec<String> = grammar
        .rules_of(create_table)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    assert_eq!(rules, ["sql::CreateTable -> CREATE TABLE Rest"]);
    let rule = grammar.rule(grammar.rules_of(create_table)[0]);
    assert_eq!(
        rule.value_pops, 1,
        "fixed tokens carry no value; `Rest` does"
    );
    assert!(matches!(rule.action, RuleAction::Construct { .. }));
    let rest = grammar
        .nonterminal_by_name("Rest")
        .expect("the content token");
    assert!(matches!(
        grammar.rule(grammar.rules_of(rest)[0]).action,
        RuleAction::Token { .. }
    ));

    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(
        automaton.conflicts().is_empty(),
        "{:?}",
        automaton.conflicts()
    );
    for application in &lowered.lowering_map.applications {
        assert!(
            automaton.start_state(application.lowered).is_some(),
            "every node application has a start state"
        );
    }

    let tables = LrTables::pack(&grammar, &automaton);
    let decoded = LrTables::decode(&tables.encode()).unwrap();
    assert_eq!(decoded, tables);
    let statement = type_named(&lowered, "sql::Statement");
    let start = tables.start_states[statement.index()];
    let input = [
        terminal(&grammar, "CREATE"),
        terminal(&grammar, "TABLE"),
        terminal(&grammar, "Rest"),
        terminal(&grammar, "DROP"),
    ];
    let trace = simulate(&grammar, &tables, start, &input).unwrap();
    assert_eq!(trace.consumed, 3, "the root ends before the next statement");
    assert_eq!(
        trace.reductions,
        [
            "Rest -> Rest",
            "sql::CreateTable -> CREATE TABLE Rest",
            "sql::Statement -> sql::CreateTable"
        ]
    );
    // The same tables parse a `DropView` from its own start state.
    let drop_view = type_named(&lowered, "sql::DropView");
    let input = [
        terminal(&grammar, "DROP"),
        terminal(&grammar, "VIEW"),
        terminal(&grammar, "Rest"),
    ];
    let trace = simulate(
        &grammar,
        &tables,
        tables.start_states[drop_view.index()],
        &input,
    )
    .unwrap();
    assert_eq!(
        trace.reductions.last().map(String::as_str),
        Some("sql::DropView -> DROP VIEW Rest")
    );
}

#[test]
fn json_fixture_lowers_separated_lists_to_left_recursive_helpers() {
    let (lowered, grammar) = lr_fixture("json");
    let member = ElementValue {
        type_id: type_named(&lowered, "json::Member"),
        boxes: 0,
    };
    let object = grammar
        .nonterminal_by_name("json::Object")
        .expect("the object node");
    let rules: Vec<String> = grammar
        .rules_of(object)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    assert_eq!(
        rules,
        ["json::Object -> LBRACE list(json::Member, sep=COMMA) RBRACE"]
    );
    let list = grammar
        .nonterminal_by_name("list(json::Member, sep=COMMA)")
        .unwrap();
    let list_rules: Vec<(String, u32, RuleAction)> = grammar
        .rules_of(list)
        .iter()
        .map(|rule| {
            (
                grammar.render_rule(*rule),
                grammar.rule(*rule).value_pops,
                grammar.rule(*rule).action.clone(),
            )
        })
        .collect();
    assert_eq!(
        list_rules,
        [
            (
                "list(json::Member, sep=COMMA) -> %empty".to_owned(),
                0,
                RuleAction::ListEmpty { element: member }
            ),
            (
                "list(json::Member, sep=COMMA) -> list1(json::Member, sep=COMMA)".to_owned(),
                1,
                RuleAction::ListFromNonEmpty { element: member }
            ),
        ]
    );
    let core = grammar
        .nonterminal_by_name("list1(json::Member, sep=COMMA)")
        .unwrap();
    let core_rules: Vec<String> = grammar
        .rules_of(core)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    assert_eq!(
        core_rules,
        [
            "list1(json::Member, sep=COMMA) -> json::Member",
            "list1(json::Member, sep=COMMA) -> list1(json::Member, sep=COMMA) COMMA json::Member",
        ]
    );
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(
        automaton.conflicts().is_empty(),
        "{:?}",
        automaton.conflicts()
    );
}

#[test]
fn optq_fixture_lowers_optionals_to_nullable_helpers_without_conflicts() {
    let (_, grammar) = lr_fixture("optq");
    let print = grammar.nonterminal_by_name("optq::PrintStmt").unwrap();
    let rules: Vec<String> = grammar
        .rules_of(print)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    assert_eq!(rules, ["optq::PrintStmt -> opt(optq::Debug) PRINT Rest"]);
    let opt = grammar.nonterminal_by_name("opt(optq::Debug)").unwrap();
    assert!(grammar.nonterminal(opt).carries_value);
    let opt_rules: Vec<String> = grammar
        .rules_of(opt)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    assert_eq!(
        opt_rules,
        [
            "opt(optq::Debug) -> %empty",
            "opt(optq::Debug) -> optq::Debug"
        ]
    );
    let debug = grammar.nonterminal_by_name("optq::Debug").unwrap();
    assert_eq!(
        grammar.render_rule(grammar.rules_of(debug)[0]),
        "optq::Debug -> DEBUG"
    );
    assert_eq!(grammar.rule(grammar.rules_of(debug)[0]).value_pops, 0);

    // The LL(2) shared prefix is no problem for LR: the optional reduces on PRINT/READ/SET.
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(
        automaton.conflicts().is_empty(),
        "{:?}",
        automaton.conflicts()
    );
}

#[test]
fn construction_is_deterministic_across_builds() {
    let (_, grammar) = lr_fixture("json");
    let first = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    let second = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert_eq!(
        automaton_dump(&grammar, &first),
        automaton_dump(&grammar, &second)
    );
    assert_eq!(
        LrTables::pack(&grammar, &first).encode(),
        LrTables::pack(&grammar, &second).encode()
    );
    let (_, again) = lr_fixture("json");
    assert_eq!(format!("{grammar:?}"), format!("{again:?}"));
}

#[test]
fn the_whole_analysis_runs_from_one_entry_point() {
    let lowered = lowered_fixture("json");
    let analysis = LrAnalysis::from_lowered(&lowered, AutomatonOptions::default())
        .expect("json has no text predicates");
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.tables,
        LrTables::pack(&analysis.grammar, &analysis.automaton)
    );
    assert!(analysis.dump().starts_with("Grammar\n\n"));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/source-analysis/json");
    let from_source = LrAnalysis::analyze_source(
        &fixture,
        Path::new("grammar.rs"),
        AutomatonOptions::default(),
    )
    .expect("the public entry point reaches the same analysis");
    assert_eq!(from_source.dump(), analysis.dump());
}

#[test]
fn analysis_statistics_account_for_the_complete_machine() {
    let lowered = lowered_fixture("json");
    let analysis = LrAnalysis::from_lowered(&lowered, AutomatonOptions::default())
        .expect("json has no text predicates");
    let statistics = analysis.statistics();

    assert_eq!(statistics.terminals, analysis.grammar.terminal_count());
    assert_eq!(
        statistics.nonterminals.total(),
        analysis.grammar.nonterminal_count()
    );
    assert_eq!(statistics.rules.total(), analysis.grammar.rules().len());
    assert_eq!(statistics.states.total(), analysis.automaton.states().len());
    assert_eq!(statistics.starts.len(), analysis.grammar.starts().len());
    assert_eq!(
        statistics
            .tables
            .iter()
            .map(|table| table.cells)
            .sum::<usize>(),
        analysis.tables.cell_count()
            + analysis.tables.lookahead.len()
            + analysis.tables.folds.len()
            + analysis.tables.tail.len()
    );
    assert!(
        statistics
            .render()
            .starts_with("lr-statistics-v1\nterminals=")
    );
}

#[test]
fn cskw_admission_sets_lower_to_one_rule_per_member_kind_yielding_the_token() {
    let (_, grammar) = lr_fixture("cskw");
    let column_name = grammar
        .nonterminal_by_name("ColumnName")
        .expect("the content token that admits AS");
    let rules: Vec<(String, u32, RuleAction)> = grammar
        .rules_of(column_name)
        .iter()
        .map(|rule| {
            (
                grammar.render_rule(*rule),
                grammar.rule(*rule).value_pops,
                grammar.rule(*rule).action.clone(),
            )
        })
        .collect();
    let type_id = match grammar.rule(grammar.rules_of(column_name)[0]).action {
        RuleAction::Token { type_id } => type_id,
        ref action => panic!("expected a token rule, found {action:?}"),
    };
    assert_eq!(
        rules,
        [
            (
                "ColumnName -> AS".to_owned(),
                0,
                RuleAction::Token { type_id }
            ),
            (
                "ColumnName -> Word".to_owned(),
                0,
                RuleAction::Token { type_id }
            ),
        ],
        "one rule per admitted kind, each yielding the token"
    );
    assert!(grammar.nonterminal(column_name).carries_value);
    let alias = grammar
        .nonterminal_by_name("Alias")
        .expect("the plain content token");
    assert_eq!(grammar.rules_of(alias).len(), 1);

    // `AS` is both an admitted identifier and the alias keyword; LALR(1) tells them apart by the
    // state, so the fixture builds without conflicts.
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(
        automaton.conflicts().is_empty(),
        "{:?}",
        automaton.conflicts()
    );
    let start = grammar.starts().iter().find(|start| {
        matches!(grammar.rule(start.rule).rhs[0], Symbol::Nonterminal(id) if id == column_name)
    });
    assert!(start.is_some(), "content tokens have a start state");
}

/// Lowers the container fixture grammar and builds its automaton.
fn containers() -> (LoweredGrammar, LrGrammar, Automaton) {
    let lowered = lowered_containers();
    let roles = TypeRoles::from_lowered(&lowered);
    let grammar =
        lower(&lowered.grammar, &roles).expect("the container fixture has no text predicates");
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    (lowered, grammar, automaton)
}

#[test]
fn containers_fixture_builds_without_conflicts_and_deterministically() {
    let (_, grammar, automaton) = containers();
    assert!(
        automaton.conflicts().is_empty(),
        "{:?}",
        automaton.conflicts()
    );
    let (_, again, again_automaton) = containers();
    assert_eq!(format!("{grammar:?}"), format!("{again:?}"));
    assert_eq!(
        automaton_dump(&grammar, &automaton),
        automaton_dump(&again, &again_automaton)
    );
}

#[test]
fn fixed_only_choices_are_inlined_as_terminals_and_valued_ones_by_admitted_first_kinds() {
    let (_, grammar, _) = containers();
    // `Let`: an erased group inlined as `ONE`, a presence envelope, and `Option<Box<Alias>>`
    // inlined because `Alias` begins with `AS`, which `Name` admits.
    assert_eq!(
        rules_of(&grammar, "grammar::Let"),
        [
            "grammar::Let -> LET grammar::Let::__recursa_presence_envelope grammar::Name EQ grammar::Value SEMI",
            "grammar::Let -> LET grammar::Let::__recursa_presence_envelope grammar::Name EQ grammar::Value grammar::Alias SEMI",
            "grammar::Let -> LET ONE grammar::Let::__recursa_presence_envelope grammar::Name EQ grammar::Value SEMI",
            "grammar::Let -> LET ONE grammar::Let::__recursa_presence_envelope grammar::Name EQ grammar::Value grammar::Alias SEMI",
        ]
    );
    let envelope = grammar
        .nonterminal_by_name("grammar::Let::__recursa_presence_envelope")
        .unwrap();
    let envelope_rules: Vec<(String, RuleAction)> = grammar
        .rules_of(envelope)
        .iter()
        .map(|rule| {
            (
                grammar.render_rule(*rule),
                grammar.rule(*rule).action.clone(),
            )
        })
        .collect();
    let type_id = match envelope_rules[0].1 {
        RuleAction::Envelope { type_id, .. } => type_id,
        ref action => panic!("expected an envelope action, found {action:?}"),
    };
    assert_eq!(
        envelope_rules,
        [
            (
                "grammar::Let::__recursa_presence_envelope -> IF EXISTS".to_owned(),
                RuleAction::Envelope {
                    type_id,
                    present: false
                }
            ),
            (
                "grammar::Let::__recursa_presence_envelope -> IF NOT EXISTS".to_owned(),
                RuleAction::Envelope {
                    type_id,
                    present: true
                }
            ),
        ]
    );
    // `Print`: a bare presence inlined; `Option<Guard>` delegated because `IF` is not admitted.
    assert_eq!(
        rules_of(&grammar, "grammar::Print"),
        [
            "grammar::Print -> PRINT grammar::Value opt(grammar::Guard) SEMI",
            "grammar::Print -> PRINT NOT grammar::Value opt(grammar::Guard) SEMI",
        ]
    );
    assert_eq!(
        rules_of(&grammar, "opt(grammar::Guard)"),
        [
            "opt(grammar::Guard) -> %empty",
            "opt(grammar::Guard) -> grammar::Guard"
        ]
    );
    // The gram.y `arg_class` shape: `Option<Mode>` inlined, so no epsilon reduce competes with
    // the shift of `IN` or `OUT`, which `Callee` admits.
    assert_eq!(
        rules_of(&grammar, "grammar::Param"),
        [
            "grammar::Param -> grammar::Name",
            "grammar::Param -> grammar::Mode grammar::Name"
        ]
    );
}

#[test]
fn separated_list_continuation_inlines_its_adjacent_optional_tail() {
    let (_, grammar, automaton) = containers();
    assert!(
        automaton.conflicts().is_empty(),
        "{:?}",
        automaton.conflicts()
    );
    assert_eq!(
        rules_of(&grammar, "grammar::FunctionTail"),
        [
            "grammar::FunctionTail -> VTAIL list1(grammar::Value, sep=COMMA) SEMI",
            "grammar::FunctionTail -> VTAIL list1(grammar::Value, sep=COMMA) grammar::TrailingVariadic SEMI",
        ]
    );
    assert!(
        grammar
            .nonterminal_by_name("opt(grammar::TrailingVariadic)")
            .is_none(),
        "the tail must inline: an epsilon helper reintroduces the list/optional conflict"
    );
}

#[test]
fn lists_lower_to_a_left_recursive_core_with_inlined_or_delegated_emptiness_and_trailing() {
    let (lowered, grammar, _) = containers();
    let value = ElementValue {
        type_id: type_named(&lowered, "grammar::Value"),
        boxes: 0,
    };
    let core = grammar
        .nonterminal_by_name("list1(grammar::Value, sep=COMMA)")
        .unwrap();
    let core_rules: Vec<(String, u32, RuleAction)> = grammar
        .rules_of(core)
        .iter()
        .map(|rule| {
            (
                grammar.render_rule(*rule),
                grammar.rule(*rule).value_pops,
                grammar.rule(*rule).action.clone(),
            )
        })
        .collect();
    assert_eq!(
        core_rules,
        [
            (
                "list1(grammar::Value, sep=COMMA) -> grammar::Value".to_owned(),
                1,
                RuleAction::ListFirst { element: value }
            ),
            (
                "list1(grammar::Value, sep=COMMA) -> list1(grammar::Value, sep=COMMA) COMMA grammar::Value".to_owned(),
                2,
                RuleAction::ListPush { element: value }
            ),
        ]
    );
    // `Vec<Value>` with a trailing separator, inlined: absent, the core, the core and a comma.
    assert_eq!(
        rules_of(&grammar, "grammar::Args"),
        [
            "grammar::Args -> LPAREN RPAREN",
            "grammar::Args -> LPAREN list1(grammar::Value, sep=COMMA) RPAREN",
            "grammar::Args -> LPAREN list1(grammar::Value, sep=COMMA) COMMA RPAREN",
        ]
    );
    // `Option<Vec1<Value>>` inlined; `Vec<Box<Value>>` shares nothing with the unboxed core.
    assert_eq!(
        rules_of(&grammar, "grammar::List"),
        [
            "grammar::List -> LBRACKET RBRACKET",
            "grammar::List -> LBRACKET list1(grammar::Value, sep=COMMA) RBRACKET",
        ]
    );
    assert_eq!(
        rules_of(&grammar, "grammar::Boxes"),
        [
            "grammar::Boxes -> BOXES SEMI",
            "grammar::Boxes -> BOXES list1(Box<grammar::Value>, sep=COMMA) SEMI",
        ]
    );
    let boxed_core = grammar
        .nonterminal_by_name("list1(Box<grammar::Value>, sep=COMMA)")
        .unwrap();
    assert_eq!(
        grammar.rule(grammar.rules_of(boxed_core)[0]).action,
        RuleAction::ListFirst {
            element: ElementValue {
                type_id: value.type_id,
                boxes: 1
            }
        }
    );
    // `Vec1<Name>` with a trailing separator and a presence, both inlined.
    assert_eq!(
        rules_of(&grammar, "grammar::With"),
        [
            "grammar::With -> WITH list1(grammar::Name, sep=PIPE) SEMI",
            "grammar::With -> WITH list1(grammar::Name, sep=PIPE) PIPE SEMI",
            "grammar::With -> WITH RECURSIVE list1(grammar::Name, sep=PIPE) SEMI",
            "grammar::With -> WITH RECURSIVE list1(grammar::Name, sep=PIPE) PIPE SEMI",
        ]
    );
    // `Vec<Statement>` without a separator, inlined because a statement can begin with a callee.
    assert_eq!(
        rules_of(&grammar, "grammar::Block"),
        [
            "grammar::Block -> LBRACE RBRACE",
            "grammar::Block -> LBRACE list1(grammar::Statement) RBRACE"
        ]
    );
    // `Vec<Bang>` and `Option<Vec1<Bang>>` are delegated: `BANG` is no admitted keyword.
    let bang = ElementValue {
        type_id: type_named(&lowered, "grammar::Bang"),
        boxes: 0,
    };
    assert_eq!(
        rules_of(&grammar, "grammar::Marks"),
        ["grammar::Marks -> MARKS list(grammar::Bang) COLON opt(list1(grammar::Bang)) SEMI"]
    );
    let list = grammar.nonterminal_by_name("list(grammar::Bang)").unwrap();
    assert_eq!(
        grammar
            .rules_of(list)
            .iter()
            .map(|rule| grammar.rule(*rule).action.clone())
            .collect::<Vec<_>>(),
        [
            RuleAction::ListEmpty { element: bang },
            RuleAction::ListFromNonEmpty { element: bang },
        ]
    );
    let optional_list = grammar
        .nonterminal_by_name("opt(list1(grammar::Bang))")
        .unwrap();
    assert_eq!(
        grammar.rule(grammar.rules_of(optional_list)[1]).action,
        RuleAction::OptionalPresent {
            content: OptionalContent::NonEmptyList {
                element: bang,
                boxes: 0
            }
        }
    );
}

#[test]
fn choices_past_the_inline_budget_are_demoted_to_helpers_from_the_right_valued_first() {
    let (lowered, grammar, _) = containers();
    // Six fixed-only choices: the two rightmost presences become `presence(...)` helpers and
    // the sixteen remaining alternatives stay inlined.
    let modifiers = rules_of(&grammar, "grammar::Modifiers");
    assert_eq!(modifiers.len(), 16);
    assert_eq!(
        modifiers[0],
        "grammar::Modifiers -> MODS presence(FIVE) presence(SIX) grammar::Name SEMI"
    );
    assert_eq!(
        modifiers[15],
        "grammar::Modifiers -> MODS ONE TWO THREE FOUR presence(FIVE) presence(SIX) grammar::Name SEMI"
    );
    assert_eq!(
        rules_of(&grammar, "presence(SIX)"),
        ["presence(SIX) -> %empty", "presence(SIX) -> SIX"]
    );
    let presence = grammar.nonterminal_by_name("presence(SIX)").unwrap();
    assert_eq!(
        grammar.rule(grammar.rules_of(presence)[1]).action,
        RuleAction::OptionalPresent {
            content: OptionalContent::Presence
        }
    );
    // Five erased groups: the rightmost becomes an erased `opt(FIVE)` helper without a value.
    let erased = rules_of(&grammar, "grammar::Erased");
    assert_eq!(erased.len(), 16);
    assert_eq!(
        erased[0],
        "grammar::Erased -> ERASED opt(FIVE) grammar::Name SEMI"
    );
    let helper = grammar.nonterminal_by_name("opt(FIVE)").unwrap();
    assert!(!grammar.nonterminal(helper).carries_value);
    assert_eq!(
        grammar.rule(grammar.rules_of(helper)[0]).action,
        RuleAction::OptionalAbsent {
            content: OptionalContent::Erased
        }
    );
    // Five valued optionals of unit nodes: the rightmost becomes `opt(KwTemp)`.
    let options = rules_of(&grammar, "grammar::Options");
    assert_eq!(options.len(), 16);
    assert_eq!(
        options[0],
        "grammar::Options -> OPTIONS opt(grammar::KwTemp) SEMI"
    );
    assert_eq!(
        options[15],
        "grammar::Options -> OPTIONS grammar::KwAs grammar::KwIn grammar::KwOut grammar::KwOnly opt(grammar::KwTemp) SEMI"
    );
    let temp = grammar.nonterminal_by_name("opt(grammar::KwTemp)").unwrap();
    assert_eq!(
        grammar.rule(grammar.rules_of(temp)[1]).action,
        RuleAction::OptionalPresent {
            content: OptionalContent::Element(ElementValue {
                type_id: type_named(&lowered, "grammar::KwTemp"),
                boxes: 0
            })
        }
    );
    // Four erased groups and a trailing separator: the trailing choice becomes `list1t`.
    let trail = rules_of(&grammar, "grammar::Trail");
    assert_eq!(trail.len(), 16);
    assert_eq!(
        trail[0],
        "grammar::Trail -> TRAIL list1t(grammar::Name, sep=COMMA) SEMI"
    );
    assert_eq!(
        rules_of(&grammar, "list1t(grammar::Name, sep=COMMA)"),
        [
            "list1t(grammar::Name, sep=COMMA) -> list1(grammar::Name, sep=COMMA)",
            "list1t(grammar::Name, sep=COMMA) -> list1(grammar::Name, sep=COMMA) COMMA",
        ]
    );
}

#[test]
fn attachment_units_pass_their_semantic_value_through_and_inline_their_own_groups() {
    let (lowered, grammar, _) = containers();
    let value = type_named(&lowered, "grammar::Value");
    // Per-element parentheses around a `Vec1<Value>`: the unit is the list element and its
    // reduce is a pass-through, so the list element value is `Value`.
    assert_eq!(
        rules_of(&grammar, "grammar::Tuple"),
        ["grammar::Tuple -> list1(grammar::Tuple::__recursa_attachment, sep=PIPE)"]
    );
    let unit = grammar
        .nonterminal_by_name("grammar::Tuple::__recursa_attachment")
        .unwrap();
    assert_eq!(
        grammar.render_rule(grammar.rules_of(unit)[0]),
        "grammar::Tuple::__recursa_attachment -> LPAREN grammar::Value RPAREN"
    );
    assert!(matches!(
        grammar.rule(grammar.rules_of(unit)[0]).action,
        RuleAction::Attachment { .. }
    ));
    let core = grammar
        .nonterminal_by_name("list1(grammar::Tuple::__recursa_attachment, sep=PIPE)")
        .unwrap();
    assert_eq!(
        grammar.rule(grammar.rules_of(core)[0]).action,
        RuleAction::ListFirst {
            element: ElementValue {
                type_id: value,
                boxes: 0
            }
        }
    );
    // `Option<Box<Value>>` behind `RETURNS`: the helper wraps the child's value, boxed once.
    assert_eq!(
        rules_of(&grammar, "opt(grammar::Define::__recursa_attachment)"),
        [
            "opt(grammar::Define::__recursa_attachment) -> %empty",
            "opt(grammar::Define::__recursa_attachment) -> grammar::Define::__recursa_attachment",
        ]
    );
    let returns = grammar
        .nonterminal_by_name("opt(grammar::Define::__recursa_attachment)")
        .unwrap();
    assert_eq!(
        grammar.rule(grammar.rules_of(returns)[1]).action,
        RuleAction::OptionalPresent {
            content: OptionalContent::Element(ElementValue {
                type_id: value,
                boxes: 1
            })
        }
    );
    // An erased group inside the attachment is inlined into the unit's own alternatives.
    assert_eq!(
        rules_of(&grammar, "grammar::Filter::__recursa_attachment"),
        [
            "grammar::Filter::__recursa_attachment -> COLON grammar::Value",
            "grammar::Filter::__recursa_attachment -> COLON NOT grammar::Value",
        ]
    );
}

#[test]
fn site_forms_map_every_element_to_its_symbols() {
    let (_, grammar, _) = containers();
    let let_rules = grammar.rules_of(grammar.nonterminal_by_name("grammar::Let").unwrap());
    // `LET ONE envelope Name EQ Value Alias SEMI`: eight elements, the erased group and the
    // alias present and inlined, everything else required.
    let RuleAction::Construct { sites, .. } = &grammar.rule(let_rules[3]).action else {
        panic!("a node rule constructs");
    };
    let forms: Vec<(u32, u32, bool, bool)> = sites
        .iter()
        .map(|form| (form.start, form.len, form.present, form.inlined))
        .collect();
    assert_eq!(
        forms,
        [
            (0, 1, true, false),
            (1, 1, true, true),
            (2, 1, true, false),
            (3, 1, true, false),
            (4, 1, true, false),
            (5, 1, true, false),
            (6, 1, true, true),
            (7, 1, true, false),
        ]
    );
    // The first alternative has both choices absent: zero-length forms at the same offset.
    let RuleAction::Construct { sites, .. } = &grammar.rule(let_rules[0]).action else {
        panic!("a node rule constructs");
    };
    assert_eq!(
        (sites[1].len, sites[1].present, sites[1].inlined),
        (0, false, true)
    );
    assert_eq!(
        (sites[6].start, sites[6].len, sites[6].present),
        (5, 0, false)
    );
    assert_eq!(grammar.rule(let_rules[0]).value_pops, 3);
    assert_eq!(grammar.rule(let_rules[3]).value_pops, 4);
}

#[test]
fn a_restricted_expression_substitutes_its_first_and_last_operands() {
    let (_, grammar) = lr_fixture("restricted");
    // `BExpr` admits three of `Expr`'s five variants, and each admitted rule is the base's own
    // rule with exactly one substitution: a recursive operand that is the rule's first or last
    // symbol becomes `BExpr`, and every other operand keeps `Expr`. That is `gram.y`'s
    // `b_expr`: `b_expr '+' b_expr`, but `'(' a_expr ')'`.
    assert_eq!(
        rules_of(&grammar, "BExpr"),
        [
            "BExpr -> restricted::Name",
            "BExpr -> LPAREN restricted::Expr RPAREN",
            "BExpr -> BExpr PLUS BExpr",
        ]
    );
    // The base keeps every variant, boolean `And` included, and the restricted operand of
    // `Between` is the only site that parses through `BExpr`.
    assert_eq!(
        rules_of(&grammar, "restricted::Expr"),
        [
            "restricted::Expr -> restricted::Name",
            "restricted::Expr -> LPAREN restricted::Expr RPAREN",
            "restricted::Expr -> restricted::Expr AND restricted::Expr",
            "restricted::Expr -> restricted::Expr PLUS restricted::Expr",
            "restricted::Expr -> restricted::Expr BETWEEN BExpr AND restricted::Expr",
        ]
    );
    // Every rule of the restriction constructs the base type, so a field that names `BExpr`
    // needs no conversion and no type of its own.
    let restricted = grammar
        .nonterminal_by_name("BExpr")
        .expect("the restriction has its own nonterminal");
    let base = grammar
        .nonterminal_by_name("restricted::Expr")
        .expect("the base has its own nonterminal");
    for rule in grammar.rules_of(restricted) {
        let RuleAction::Construct { type_id, .. } = grammar.rule(*rule).action else {
            panic!("a restricted rule constructs the base type");
        };
        let NonterminalOrigin::DeclaredRestriction { type_id: declared } =
            grammar.nonterminal(restricted).origin
        else {
            panic!("a restriction's nonterminal records the base it builds");
        };
        assert_eq!(type_id, declared);
        assert!(matches!(
            grammar.nonterminal(base).origin,
            NonterminalOrigin::Type(base_type) if base_type == type_id
        ));
    }
}

#[test]
fn a_leading_restriction_substitutes_only_its_first_operand() {
    let (_, grammar) = lr_fixture("restricted");
    // `#[restricts(Expr, leading)]` retypes the operand that leads a rule and leaves the one that
    // ends it as the base: the position excludes a leading `(` and nothing else, so the right
    // operand of `+` is still a full `Expr` and may be parenthesized.
    assert_eq!(
        rules_of(&grammar, "Action"),
        [
            "Action -> restricted::Name",
            "Action -> Action PLUS restricted::Expr",
        ]
    );
    // `BExpr`, declared without the option, still restricts both ends.
    assert!(rules_of(&grammar, "BExpr").contains(&"BExpr -> BExpr PLUS BExpr".to_owned()));
}

#[test]
fn a_restricted_expression_builds_a_conflict_free_automaton() {
    let (_, grammar) = lr_fixture("restricted");
    let automaton = Automaton::build(
        &grammar,
        &PrecedenceResolver::new(&grammar).expect("the resolver is infallible"),
        AutomatonOptions::default(),
    );
    assert!(
        automaton.conflicts().is_empty(),
        "{}",
        automaton_dump(&grammar, &automaton)
    );
}

/// Runs the whole table-driven analysis over the precedence generation fixture.
fn precedence_fixture_analysis() -> LrAnalysis {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generation/precedence");
    LrAnalysis::analyze_source(
        &fixture,
        Path::new("src/grammar.rs"),
        AutomatonOptions::default(),
    )
    .unwrap_or_else(|error| panic!("the precedence fixture analyzes:\n{error}"))
}

#[test]
fn the_precedence_fixture_has_no_conflicts_and_records_every_family_as_resolved() {
    let analysis = precedence_fixture_analysis();
    let grammar = &analysis.grammar;
    let unresolved: Vec<String> = analysis
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.render())
        .collect();
    assert!(unresolved.is_empty(), "{}", unresolved.join("\n"));

    // The declaration reaches the terminals: fixed tokens and the content token alike.
    let declared = |name: &str| {
        grammar
            .declared_precedence(terminal(grammar, name))
            .map(|precedence| (precedence.binding_power, precedence.associativity))
    };
    assert_eq!(declared("ESCAPE"), Some((6, Associativity::NonAssociative)));
    assert_eq!(declared("LIKE"), Some((5, Associativity::NonAssociative)));
    assert_eq!(declared("MINUS"), Some((10, Associativity::Left)));
    assert_eq!(declared("DOT"), Some((20, Associativity::Left)));
    assert_eq!(
        declared("NameText"),
        Some((9, Associativity::NonAssociative))
    );
    assert_eq!(declared("SEMI"), None);
    assert_eq!(declared("JSON"), None);

    // The overrides reach the rules of their variant, and every other rule takes its last
    // declared terminal.
    let rule_precedence = |rendered: &str| {
        let rule = grammar
            .rules()
            .iter()
            .enumerate()
            .find(|(index, _)| grammar.render_rule(super::RuleId::new(*index as u32)) == rendered)
            .unwrap_or_else(|| panic!("no rule {rendered}"))
            .1;
        rule.precedence
            .map(|precedence| (precedence.level, precedence.associativity))
    };
    assert_eq!(
        rule_precedence("grammar::Expr -> grammar::SubSelect"),
        Some((12, Associativity::Right))
    );
    assert_eq!(
        rule_precedence("grammar::IsJsonTail -> JSON"),
        Some((8, Associativity::NonAssociative))
    );
    assert_eq!(
        rule_precedence("grammar::IsJsonTail -> JSON VALUE"),
        Some((9, Associativity::NonAssociative))
    );
    assert_eq!(
        rule_precedence("NameText -> BETWEEN"),
        Some((5, Associativity::NonAssociative))
    );
    assert_eq!(
        rule_precedence("grammar::SetScope -> SESSION"),
        Some((1, Associativity::NonAssociative))
    );
    let escape_attachment = grammar
        .rules()
        .iter()
        .enumerate()
        .map(|(index, _)| grammar.render_rule(super::RuleId::new(index as u32)))
        .find(|rendered| rendered.contains(" -> ESCAPE "))
        .expect("the escape clause lowers to a rule starting with ESCAPE");
    assert_eq!(
        rule_precedence(&escape_attachment),
        Some((6, Associativity::NonAssociative))
    );
    assert_eq!(
        rule_precedence("grammar::SubSelect -> LPAREN SELECT grammar::Name RPAREN"),
        None
    );
    assert_eq!(rule_precedence("grammar::Expr -> grammar::Number"), None);

    let settled: Vec<(String, String, super::Resolution)> = analysis
        .automaton
        .resolved_conflicts()
        .iter()
        .map(|resolved| {
            (
                grammar.render_rule(resolved.reduce),
                grammar.terminal_name(resolved.lookahead).to_owned(),
                resolved.resolution,
            )
        })
        .collect();
    let shift = |rule: &str, lookahead: &str| {
        (
            rule.to_owned(),
            lookahead.to_owned(),
            super::Resolution::Shift,
        )
    };
    for expected in [
        // `%prec UMINUS` below `'.'`.
        shift("grammar::Expr -> grammar::SubSelect", "DOT"),
        // `JSON %prec UNBOUNDED` below `VALUE_P` at IDENT's level.
        shift("grammar::IsJsonTail -> JSON", "VALUE"),
        // `BETWEEN` as a column name below `'-'`.
        shift("NameText -> BETWEEN", "MINUS"),
        // `NESTED` as a column name below `PATH`.
        shift("NameText -> NESTED", "PATH"),
        // The literal scope keyword below `CHARACTERISTICS`.
        shift("grammar::SetScope -> SESSION", "CHARACTERISTICS"),
        // The `LIKE` rule (5) against `ESCAPE` (6), which introduces no operator of `Expr`.
        shift(
            "grammar::Expr -> grammar::Expr LIKE grammar::Expr",
            "ESCAPE",
        ),
    ] {
        assert!(
            settled.contains(&expected),
            "{expected:?} not in {settled:#?}"
        );
    }
    // The same rule against an operator of its own right operand: `'+'` (10) is above `LIKE`
    // (5), so the operand keeps growing.
    assert!(settled.contains(&shift(
        "grammar::Expr -> grammar::Expr LIKE grammar::Expr",
        "PLUS"
    )));
    let dump = analysis.dump();
    assert!(dump.contains("resolved by precedence (shift)."), "{dump}");
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");
}

#[test]
fn the_follow_pollution_fixture_settles_its_unit_rule_pair_on_the_end_marker() {
    // #143: `Expr -> SelectWithParens` beside `SelectClause -> SelectWithParens`, two unit rules
    // of two grammar types that `ParenContent` derives. The one state that completes
    // `SelectWithParens` holds both with the end marker as their single shared lookahead, so
    // declaration order takes `Expr` and the pair is no conflict. The two conflicts that remain
    // are the `#125` shared-helper artefact the per-site copy pass removes later.
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generation/follow-pollution");
    let lowered = lowered_tree(&fixture, "src/grammar.rs");
    let roles = TypeRoles::from_lowered(&lowered);
    let grammar = lower(&lowered.grammar, &roles).expect("the fixture has no text predicate");
    let precedence = PrecedenceResolver::new(&grammar).expect("the resolver is infallible");
    let automaton = Automaton::build(&grammar, &precedence, AutomatonOptions::default());
    let settled: Vec<(Vec<String>, String)> = automaton
        .end_marker_resolutions()
        .iter()
        .map(|resolution| {
            (
                resolution
                    .rules
                    .iter()
                    .map(|rule| grammar.render_rule(*rule))
                    .collect(),
                grammar.render_rule(resolution.chosen),
            )
        })
        .collect();
    assert_eq!(
        settled.last().expect("the unit-rule pair is settled"),
        &(
            vec![
                "grammar::Expr -> grammar::SelectWithParens".to_owned(),
                "grammar::SelectClause -> grammar::SelectWithParens".to_owned(),
            ],
            "grammar::Expr -> grammar::SelectWithParens".to_owned(),
        ),
        "{settled:#?}"
    );
    assert!(
        automaton
            .conflicts()
            .iter()
            .all(|conflict| conflict.merge_artefact
                && matches!(conflict.kind, ConflictKind::ReduceReduce { .. })),
        "{:#?}",
        automaton.conflicts()
    );
}
