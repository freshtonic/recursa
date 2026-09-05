//! Lowering of the source-analysis fixtures; the pg-sql run is the `lr_pg_sql` integration test.

use std::path::Path;

use super::automaton::{Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
use super::grammar::{Associativity, LrGrammar, RuleAction, Symbol, TerminalId};
use super::lowering::{TypeRoles, lower};
use super::report::{automaton_dump, conflict_diagnostics};
use super::LrAnalysis;
use super::tables::{LrTables, simulate};
use crate::materialization::LoweredGrammar;
use crate::source_analysis::{SourceAnalysisInput, analyze_source_tree};

/// Runs source analysis over one Cargo package root and selected root file.
fn lowered_tree(manifest_dir: &Path, root: &str) -> LoweredGrammar {
    analyze_source_tree(
        SourceAnalysisInput::rust_2024(manifest_dir, root),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{} failed source analysis:\n{error}", manifest_dir.display()))
    .analyzed
    .lowered
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
fn arith_pratt_variants_lower_to_plain_rules_with_recorded_binding_powers() {
    let (_, grammar) = lr_fixture("arith");
    let expr = grammar.nonterminal_by_name("arith::Expr").expect("the Pratt enum");
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
            "arith::Expr -> arith::Expr::__recursa_attachment",
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
            Some((30, Associativity::None)),
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

    // Without precedence the naive lowering is ambiguous exactly where ticket #115 resolves it:
    // every conflict is a shift/reduce between an infix reduce and an operator shift.
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(!automaton.conflicts().is_empty());
    for conflict in automaton.conflicts() {
        let ConflictKind::ShiftReduce { reduce, .. } = conflict.kind else {
            panic!("naive Pratt lowering only produces shift/reduce conflicts");
        };
        assert!(
            grammar.rule(reduce).precedence.is_some(),
            "every competing reduce is an operator rule carrying its binding powers"
        );
    }
    let diagnostics = conflict_diagnostics(&grammar, &automaton);
    assert_eq!(diagnostics.len(), automaton.conflicts().len());
    let paren = grammar
        .nonterminal_by_name("arith::Expr::__recursa_attachment")
        .expect("the fixed-token group around the parenthesised operand");
    assert_eq!(
        grammar.render_rule(grammar.rules_of(paren)[0]),
        "arith::Expr::__recursa_attachment -> LPAREN arith::Expr RPAREN"
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
    assert_eq!(rule.value_pops, 1, "fixed tokens carry no value; `Rest` does");
    assert!(matches!(rule.action, RuleAction::Construct { .. }));
    let rest = grammar.nonterminal_by_name("Rest").expect("the content token");
    assert!(matches!(
        grammar.rule(grammar.rules_of(rest)[0]).action,
        RuleAction::Token { .. }
    ));

    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
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
        ["Rest -> Rest", "sql::CreateTable -> CREATE TABLE Rest", "sql::Statement -> sql::CreateTable"]
    );
    // The same tables parse a `DropView` from its own start state.
    let drop_view = type_named(&lowered, "sql::DropView");
    let input = [
        terminal(&grammar, "DROP"),
        terminal(&grammar, "VIEW"),
        terminal(&grammar, "Rest"),
    ];
    let trace = simulate(&grammar, &tables, tables.start_states[drop_view.index()], &input).unwrap();
    assert_eq!(trace.reductions.last().map(String::as_str), Some("sql::DropView -> DROP VIEW Rest"));
}

#[test]
fn json_fixture_lowers_separated_lists_to_left_recursive_helpers() {
    let (_, grammar) = lr_fixture("json");
    let object = grammar.nonterminal_by_name("json::Object").expect("the object node");
    let rules: Vec<String> = grammar
        .rules_of(object)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    assert_eq!(rules, ["json::Object -> LBRACE list(json::Member, sep=COMMA) RBRACE"]);
    let list = grammar.nonterminal_by_name("list(json::Member, sep=COMMA)").unwrap();
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
            ("list(json::Member, sep=COMMA) -> %empty".to_owned(), 0, RuleAction::ListEmpty),
            (
                "list(json::Member, sep=COMMA) -> list1(json::Member, sep=COMMA)".to_owned(),
                1,
                RuleAction::ListPass
            ),
        ]
    );
    let core = grammar.nonterminal_by_name("list1(json::Member, sep=COMMA)").unwrap();
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
    assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
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
    assert_eq!(opt_rules, ["opt(optq::Debug) -> %empty", "opt(optq::Debug) -> optq::Debug"]);
    let debug = grammar.nonterminal_by_name("optq::Debug").unwrap();
    assert_eq!(grammar.render_rule(grammar.rules_of(debug)[0]), "optq::Debug -> DEBUG");
    assert_eq!(grammar.rule(grammar.rules_of(debug)[0]).value_pops, 0);

    // The LL(2) shared prefix is no problem for LR: the optional reduces on PRINT/READ/SET.
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
}

#[test]
fn construction_is_deterministic_across_builds() {
    let (_, grammar) = lr_fixture("json");
    let first = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    let second = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert_eq!(automaton_dump(&grammar, &first), automaton_dump(&grammar, &second));
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
    let analysis = LrAnalysis::from_lowered(&lowered, &NoPrecedence, AutomatonOptions::default())
        .expect("json has no text predicates");
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(analysis.tables, LrTables::pack(&analysis.grammar, &analysis.automaton));
    assert!(analysis.dump().starts_with("Grammar\n\n"));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/source-analysis/json");
    let from_source = LrAnalysis::analyze_source(&fixture, Path::new("grammar.rs"), AutomatonOptions::default())
        .expect("the public entry point reaches the same analysis");
    assert_eq!(from_source.dump(), analysis.dump());
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
            ("ColumnName -> AS".to_owned(), 0, RuleAction::Token { type_id }),
            ("ColumnName -> Word".to_owned(), 0, RuleAction::Token { type_id }),
        ],
        "one rule per admitted kind, each yielding the token"
    );
    assert!(grammar.nonterminal(column_name).carries_value);
    let alias = grammar.nonterminal_by_name("Alias").expect("the plain content token");
    assert_eq!(grammar.rules_of(alias).len(), 1);

    // `AS` is both an admitted identifier and the alias keyword; LALR(1) tells them apart by the
    // state, so the fixture builds without conflicts.
    let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
    let start = grammar.starts().iter().find(|start| {
        matches!(grammar.rule(start.rule).rhs[0], Symbol::Nonterminal(id) if id == column_name)
    });
    assert!(start.is_some(), "content tokens have a start state");
}
