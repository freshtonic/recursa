//! Lowering of the source-analysis fixtures; the pg-sql run is the `lr_pg_sql` integration test.

use std::path::Path;

use super::automaton::{Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
use super::precedence::BindingPowerPrecedence;
use super::grammar::{
    Associativity, LrGrammar, NonterminalOrigin, RuleAction, Symbol, TerminalId,
};
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
    let analysis = LrAnalysis::from_lowered(&lowered, AutomatonOptions::default())
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

/// Lowers the table-driven Pratt generation fixture: the textbook expression grammar as a
/// `#[pratt]` enum with an exclusion site.
fn pratt_fixture() -> (LoweredGrammar, LrGrammar) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generation/table-driven-pratt");
    let lowered = lowered_tree(&fixture, "src/grammar.rs");
    let roles = TypeRoles::from_lowered(&lowered);
    let grammar = lower(&lowered.grammar, &roles).expect("the Pratt fixture lowers");
    (lowered, grammar)
}

/// Simulates a parse of `source` from the fixture's `Expr` start state and returns the
/// reductions of `Expr` rules, in order; token spellings are single characters or `in`.
fn expr_reductions(lowered: &LoweredGrammar, grammar: &LrGrammar, automaton: &Automaton, source: &str) -> Vec<String> {
    let tables = LrTables::pack(grammar, automaton);
    let start = automaton
        .start_state(type_named(lowered, "grammar::Expr"))
        .expect("Expr has a start state");
    let input: Vec<TerminalId> = source
        .split_whitespace()
        .map(|word| match word {
            "+" => "PLUS",
            "-" => "MINUS",
            "*" => "STAR",
            "^" => "CARET",
            "!" => "BANG",
            "in" => "IN",
            "position" => "POSITION",
            "(" => "LPAREN",
            ")" => "RPAREN",
            _ => "IntegerText",
        })
        .map(|name| terminal(grammar, name))
        .collect();
    let trace = simulate(grammar, &tables, start.index() as u32, &input)
        .unwrap_or_else(|error| panic!("{source} does not parse: {error:?}"));
    assert_eq!(trace.consumed, input.len());
    trace
        .reductions
        .into_iter()
        .filter(|rule| rule.starts_with("grammar::Expr"))
        .collect()
}

#[test]
fn binding_power_precedence_resolves_every_pratt_conflict_of_the_expression_fixture() {
    let (lowered, grammar) = pratt_fixture();
    let naive = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
    assert!(!naive.conflicts().is_empty(), "the naive lowering conflicts on every operator");

    let precedence = BindingPowerPrecedence::new(&grammar)
        .expect("every binding power of the fixture has a precedence equivalent");
    let automaton = Automaton::build(&grammar, &precedence, AutomatonOptions::default());
    assert!(
        automaton.conflicts().is_empty(),
        "{}",
        automaton_dump(&grammar, &automaton)
    );
    assert_eq!(
        automaton.resolved_conflicts().len(),
        naive.conflicts().len(),
        "precedence settles exactly the conflicts the naive lowering reports"
    );
    let dump = automaton_dump(&grammar, &automaton);
    assert!(dump.contains("resolved by precedence (reduce)."), "{dump}");
    assert!(dump.contains("resolved by precedence (shift)."), "{dump}");
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");

    let integer = "grammar::Expr -> grammar::Integer";
    let subtract = "grammar::Expr -> grammar::Expr MINUS grammar::Expr";
    let power = "grammar::Expr -> grammar::Expr CARET grammar::Expr";
    let multiply = "grammar::Expr -> grammar::Expr STAR grammar::Expr";
    let add = "grammar::Expr -> grammar::Expr PLUS grammar::Expr";
    let negate = "grammar::Expr -> MINUS grammar::Expr";
    let factorial = "grammar::Expr -> grammar::Expr BANG";
    let membership = "grammar::Expr -> grammar::Expr IN grammar::Expr";
    // `rbp = lbp + 1` is left-associative: `1 - 2 - 3` reduces `1 - 2` before the second `-`.
    assert_eq!(
        expr_reductions(&lowered, &grammar, &automaton, "1 - 2 - 3"),
        [integer, integer, subtract, integer, subtract]
    );
    // `rbp = lbp` is right-associative: `2 ^ 3 ^ 2` shifts the second `^` before reducing.
    assert_eq!(
        expr_reductions(&lowered, &grammar, &automaton, "2 ^ 3 ^ 2"),
        [integer, integer, integer, power, power]
    );
    // A prefix operator reduces before a lower-binding infix and shifts a higher-binding one.
    assert_eq!(
        expr_reductions(&lowered, &grammar, &automaton, "- 1 * 2"),
        [integer, negate, integer, multiply]
    );
    assert_eq!(
        expr_reductions(&lowered, &grammar, &automaton, "- 2 ^ 2"),
        [integer, integer, power, negate]
    );
    // A postfix operator binds tighter than every infix operator.
    assert_eq!(
        expr_reductions(&lowered, &grammar, &automaton, "1 + 2 !"),
        [integer, integer, factorial, add]
    );
    // The lowest level: `1 + 2 in 3` is `(1 + 2) in 3`.
    assert_eq!(
        expr_reductions(&lowered, &grammar, &automaton, "1 + 2 in 3"),
        [integer, integer, add, integer, membership]
    );
}

#[test]
fn an_exclusion_site_clones_the_pratt_nonterminal_without_the_excluded_operators() {
    let (lowered, grammar) = pratt_fixture();
    let expr = type_named(&lowered, "grammar::Expr");
    let excluded = lowered
        .grammar
        .declarations
        .iter()
        .position(|declaration| declaration.name == "grammar::Expr::In")
        .map(|index| crate::DeclId::new(index as u32))
        .expect("the In variant is declared");
    let clone = grammar
        .nonterminal_by_name("grammar::Expr[excl In]")
        .expect("the needle's exclusion set clones Expr");
    assert_eq!(
        grammar.nonterminal(clone).origin,
        NonterminalOrigin::Exclusion {
            type_id: expr,
            excluded: vec![excluded],
        }
    );
    assert!(grammar.nonterminal(clone).carries_value);
    let rules: Vec<String> = grammar
        .rules_of(clone)
        .iter()
        .map(|rule| grammar.render_rule(*rule))
        .collect();
    // Atoms keep the full language (the parenthesised operand stays the original attachment,
    // as gram.y's `b_expr: '(' a_expr ')'`), prefix and infix operands carry the exclusion,
    // and the excluded `In` rule is gone.
    assert_eq!(
        rules,
        [
            "grammar::Expr[excl In] -> grammar::Integer",
            "grammar::Expr[excl In] -> grammar::Name",
            "grammar::Expr[excl In] -> grammar::Position",
            "grammar::Expr[excl In] -> grammar::Expr::__recursa_attachment",
            "grammar::Expr[excl In] -> MINUS grammar::Expr[excl In]",
            "grammar::Expr[excl In] -> grammar::Expr[excl In] BANG",
            "grammar::Expr[excl In] -> grammar::Expr[excl In] CARET grammar::Expr[excl In]",
            "grammar::Expr[excl In] -> grammar::Expr[excl In] STAR grammar::Expr[excl In]",
            "grammar::Expr[excl In] -> grammar::Expr[excl In] PLUS grammar::Expr[excl In]",
            "grammar::Expr[excl In] -> grammar::Expr[excl In] MINUS grammar::Expr[excl In]",
        ]
    );
    let original: Vec<(u32, Option<(u8, Associativity)>)> = grammar
        .rules_of(grammar.nonterminal_by_name("grammar::Expr").unwrap())
        .iter()
        .filter(|rule| !grammar.render_rule(**rule).contains(" IN "))
        .map(|rule| {
            let rule = grammar.rule(*rule);
            (rule.value_pops, rule.precedence.map(|p| (p.level, p.associativity)))
        })
        .collect();
    let cloned: Vec<(u32, Option<(u8, Associativity)>)> = grammar
        .rules_of(clone)
        .iter()
        .map(|rule| {
            let rule = grammar.rule(*rule);
            assert!(
                matches!(rule.action, RuleAction::Construct { type_id, variant: Some(_) } if type_id == expr),
                "a clone rule builds the original type"
            );
            (rule.value_pops, rule.precedence.map(|p| (p.level, p.associativity)))
        })
        .collect();
    assert_eq!(cloned, original, "clone rules keep the value pops and binding powers");
    let position = grammar.nonterminal_by_name("grammar::Position").unwrap();
    assert_eq!(
        grammar.render_rule(grammar.rules_of(position)[0]),
        "grammar::Position -> POSITION LPAREN grammar::Expr[excl In] IN grammar::Expr RPAREN"
    );
    assert_eq!(
        grammar
            .nonterminals()
            .iter()
            .filter(|nonterminal| nonterminal.name.contains("[excl"))
            .count(),
        1,
        "one exclusion set, one clone"
    );

    let precedence = BindingPowerPrecedence::new(&grammar).expect("the clone keeps the levels");
    let automaton = Automaton::build(&grammar, &precedence, AutomatonOptions::default());
    assert!(automaton.conflicts().is_empty(), "{}", automaton_dump(&grammar, &automaton));
    let dump = automaton_dump(&grammar, &automaton);
    assert!(
        dump.contains("grammar::Expr[excl In] -> grammar::Expr[excl In] PLUS grammar::Expr[excl In]"),
        "{dump}"
    );
    assert!(
        dump.contains("grammar::Position -> POSITION LPAREN . grammar::Expr[excl In] IN grammar::Expr RPAREN"),
        "{dump}"
    );
    // The needle stops at `in`; the haystack takes the rest.
    assert_eq!(
        expr_reductions(&lowered, &grammar, &automaton, "position ( 1 + 2 in 3 in 4 )"),
        [
            "grammar::Expr[excl In] -> grammar::Integer",
            "grammar::Expr[excl In] -> grammar::Integer",
            "grammar::Expr[excl In] -> grammar::Expr[excl In] PLUS grammar::Expr[excl In]",
            "grammar::Expr -> grammar::Integer",
            "grammar::Expr -> grammar::Integer",
            "grammar::Expr -> grammar::Expr IN grammar::Expr",
            "grammar::Expr -> grammar::Position",
        ]
    );
}
