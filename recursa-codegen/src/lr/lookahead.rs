//! Lookahead-filter substitution over lowered rules (`docs/table-driven-parsing.md`, section 5).
//!
//! A `lookahead { MERGED = TOKEN before { TRIGGER, ... } }` declaration makes the table-driven
//! token feed read `TOKEN` as `MERGED` whenever the next record is a trigger. This pass rewrites
//! the lowered rules so that the automaton expects what the feed produces. It runs inside
//! [`lower`](crate::lr::lowering::lower) after every type rule exists and before the start rules
//! are added, over FIRST and FOLLOW of the *unsubstituted* grammar, because the feed decides on
//! lexed kinds and a merged kind is never a trigger.
//!
//! # The substitution rule
//!
//! For every rule `A -> α TOKEN β` let `next` be the terminals that can follow `TOKEN`:
//! `FIRST(β)`, plus `FOLLOW(A)` when `β` is nullable, where FOLLOW is seeded with `$end` for
//! every grammar type that gets a start state. The decision is the same at every position of
//! the rule, whether `TOKEN` ends it or not:
//!
//! - `next` holds no trigger: the position keeps `TOKEN`.
//! - `next` holds only triggers: the position becomes `MERGED`.
//! - `next` holds both: the position is twinned. The rule is emitted twice, once with `TOKEN`
//!   and once with `MERGED` at that position, as gram.y writes `NOT a_expr | NOT_LA a_expr` by
//!   hand beside `a_expr NOT_LA LIKE a_expr`. The twins are conflict-free by construction: the
//!   automaton chooses between them by the terminal it shifts, never by a lookahead, and every
//!   state a twin leads to has the items of the state the original leads to, so what FOLLOW or
//!   FIRST cannot tell apart is decided at run time by the feed, which is exactly what the
//!   declaration promises. Because every grammar type is an entry point, `$end` is in the
//!   FOLLOW of every type nonterminal, so a rule of a type that ends in `TOKEN` gets its twin
//!   whenever a trigger can follow the type at all.
//!
//! A twin is a clone of its original: same action, value pops, precedence, fold and tail
//! decision, so the reduce dispatcher and the renderer treat it as one more rule of the same
//! type, and it needs no site provenance of its own. Twins follow their original directly so a
//! type's rules stay contiguous. A rule with `k` twinned positions is emitted once per
//! combination, `2^k` rules: the original (every position `TOKEN`) first, then the combinations
//! in binary order with the leftmost twinned position as the low bit, so that a rule with one
//! twinned position is `original, twin` and the order is a function of the rule alone.
//!
//! No position is ambiguous, so the pass has no diagnostic: whichever kind the feed produces,
//! one twin shifts it. A grammar that needs more than the trigger to choose (`a NOT like`
//! where `like` may also be a name after that `NOT`) surfaces as an ordinary conflict of the
//! automaton on `MERGED`, the conflict its unfiltered form has on `TOKEN`; the filter neither
//! creates nor hides it. `RCA0404`, which named such a position before a twin could stand
//! inside a rule, is retired.
//!
//! A grammar without a `lookahead` block leaves this pass without touching a rule, which keeps
//! the LR grammar of such a grammar identical to what it was before the pass existed.

use super::automaton::TerminalSet;
use super::{LrGrammar, NonterminalId, Rule, Symbol, TerminalFilter, TerminalId};
use crate::GrammarDescription;

/// One filter over LR terminals.
struct Filter {
    /// The merged terminal.
    merged: TerminalId,
    /// The triggers.
    triggers: TerminalSet,
}

/// Applies every lookahead filter of `description` to the rules of `grammar`.
///
/// `start_nonterminals` names the nonterminals that receive a start rule, whose FOLLOW is
/// seeded with the end marker. The rule list is replaced by the substituted rules with every
/// twin directly after its original; the grammar's terminal filters are recorded for the
/// packed tables.
pub(super) fn apply(
    grammar: &mut LrGrammar,
    description: &GrammarDescription,
    start_nonterminals: &[NonterminalId],
) {
    if description.lookahead_filters.is_empty() {
        return;
    }
    let width = grammar.terminal_count();
    let mut filter_of: Vec<Option<usize>> = vec![None; width];
    let mut filters = Vec::new();
    for filter in &description.lookahead_filters {
        let mut triggers = TerminalSet::empty(width);
        for trigger in &filter.triggers {
            triggers.insert(TerminalId::new(u32::from(trigger.get())));
        }
        filter_of[usize::from(filter.token.get())] = Some(filters.len());
        filters.push(Filter {
            merged: TerminalId::new(u32::from(filter.merged.get())),
            triggers,
        });
    }

    let nullable = grammar.nullable_nonterminals();
    let first = first_sets(grammar, &nullable);
    let follow = follow_sets(grammar, &nullable, &first, start_nonterminals);

    let mut rules = Vec::with_capacity(grammar.rules().len());
    for rule in grammar.rules() {
        let mut substituted = rule.clone();
        // The twinned positions of the rule, in rule order, each with its merged terminal.
        let mut twinned: Vec<(usize, TerminalId)> = Vec::new();
        for (position, symbol) in rule.rhs.iter().enumerate() {
            let Symbol::Terminal(terminal) = symbol else {
                continue;
            };
            let Some(filter) = filter_of[terminal.index()].map(|index| &filters[index]) else {
                continue;
            };
            let (mut next, rest_nullable) =
                first_of_sequence(&rule.rhs[position + 1..], &nullable, &first, width);
            if rest_nullable {
                next.union_assign(&follow[rule.lhs.index()]);
            }
            let has_trigger = next.intersects(&filter.triggers);
            let has_other = next.iter().any(|candidate| !filter.triggers.contains(candidate));
            match (has_trigger, has_other) {
                (false, _) => {}
                (true, false) => substituted.rhs[position] = Symbol::Terminal(filter.merged),
                (true, true) => twinned.push((position, filter.merged)),
            }
        }
        rules.extend(twins(&substituted, &twinned));
    }
    grammar.replace_rules(rules);
    grammar.set_filters(
        description
            .lookahead_filters
            .iter()
            .map(|filter| TerminalFilter {
                token: TerminalId::new(u32::from(filter.token.get())),
                merged: TerminalId::new(u32::from(filter.merged.get())),
                triggers: filter
                    .triggers
                    .iter()
                    .map(|trigger| TerminalId::new(u32::from(trigger.get())))
                    .collect(),
            })
            .collect(),
    );
}

/// The rule and its twins: one rule per combination of the `twinned` positions, the original
/// first and the combinations in binary order with the leftmost position as the low bit. A rule
/// with no twinned position is itself alone.
fn twins(rule: &Rule, twinned: &[(usize, TerminalId)]) -> Vec<Rule> {
    (0..1usize << twinned.len())
        .map(|combination| {
            let mut twin = rule.clone();
            for (bit, (position, merged)) in twinned.iter().enumerate() {
                if combination & (1 << bit) != 0 {
                    twin.rhs[*position] = Symbol::Terminal(*merged);
                }
            }
            twin
        })
        .collect()
}

/// FIRST of a symbol sequence and whether the whole sequence is nullable.
fn first_of_sequence(
    symbols: &[Symbol],
    nullable: &[bool],
    first: &[TerminalSet],
    width: usize,
) -> (TerminalSet, bool) {
    let mut set = TerminalSet::empty(width);
    for symbol in symbols {
        match symbol {
            Symbol::Terminal(terminal) => {
                set.insert(*terminal);
                return (set, false);
            }
            Symbol::Nonterminal(id) => {
                set.union_assign(&first[id.index()]);
                if !nullable[id.index()] {
                    return (set, false);
                }
            }
        }
    }
    (set, true)
}

/// Computes FIRST of every nonterminal.
fn first_sets(grammar: &LrGrammar, nullable: &[bool]) -> Vec<TerminalSet> {
    let width = grammar.terminal_count();
    let mut first = vec![TerminalSet::empty(width); grammar.nonterminal_count()];
    let mut changed = true;
    while changed {
        changed = false;
        for rule in grammar.rules() {
            let (set, _) = first_of_sequence(&rule.rhs, nullable, &first, width);
            changed |= first[rule.lhs.index()].union_assign(&set);
        }
    }
    first
}

/// Computes FOLLOW of every nonterminal, seeding the end marker into every start nonterminal.
fn follow_sets(
    grammar: &LrGrammar,
    nullable: &[bool],
    first: &[TerminalSet],
    start_nonterminals: &[NonterminalId],
) -> Vec<TerminalSet> {
    let width = grammar.terminal_count();
    let mut follow = vec![TerminalSet::empty(width); grammar.nonterminal_count()];
    for start in start_nonterminals {
        follow[start.index()].insert(grammar.end_marker());
    }
    let mut changed = true;
    while changed {
        changed = false;
        for rule in grammar.rules() {
            for (position, symbol) in rule.rhs.iter().enumerate() {
                let Symbol::Nonterminal(id) = symbol else {
                    continue;
                };
                let (set, rest_nullable) =
                    first_of_sequence(&rule.rhs[position + 1..], nullable, first, width);
                changed |= follow[id.index()].union_assign(&set);
                if rest_nullable {
                    let lhs_follow = follow[rule.lhs.index()].clone();
                    changed |= follow[id.index()].union_assign(&lhs_follow);
                }
            }
        }
    }
    follow
}

#[cfg(test)]
mod tests {
    //! The lookahead-filter tests: the normalized grammar carries the declaration, the
    //! PostgreSQL `NOT BETWEEN` shape conflicts without the filter and not with it, a token
    //! inside a rule and at its end is twinned when what follows it mixes triggers and others
    //! (gram.y's `NOT a_expr | NOT_LA a_expr`, as a Pratt prefix and as a plain field), several
    //! twinned positions multiply out, and the packed tables carry the filter.

    use super::super::automaton::{Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
    use super::super::precedence::BindingPowerPrecedence;
    use super::super::{LrGrammar, RuleId, TerminalId};
    use super::super::lowering::{TypeRoles, lower};
    use super::super::report::LrDiagnostic;
    use super::super::tables::{LrTables, simulate};
    use crate::materialization::LoweredGrammar;
    use crate::source_analysis::{SourceAnalysisInput, analyze_source_tree};

    /// The twin fixture grammar as written: table-driven, with the filter.
    const FIXTURE: &str = include_str!("../../tests/fixtures/generation/lookahead/src/grammar.rs");

    /// The fixture's `lookahead` line.
    const FILTER: &str = "    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }\n";

    /// The `NOT BETWEEN` fixture with or without its `lookahead` declaration.
    fn not_between_grammar(lookahead: bool) -> String {
        assert_eq!(FIXTURE.matches(FILTER).count(), 1);
        if lookahead {
            FIXTURE.to_owned()
        } else {
            FIXTURE.replace(FILTER, "")
        }
    }

    /// A small grammar over the fixture's tokens, for the inside-a-rule and end-of-rule shapes.
    fn small_grammar(nodes: &str) -> String {
        let tokens_start = FIXTURE.find("recursa::tokens! {").expect("the fixture declares tokens");
        let tokens_end = FIXTURE.find("#[derive(recursa::Node, Debug, Clone)]").expect("the fixture has nodes");
        format!(
            "recursa::grammar! {{ module = crate, keyword_matching = sensitive, max_lookahead = 3 }}\n{}{nodes}\n\
             #[derive(recursa::Node)]\npub struct Name<'input> {{\n    #[lex(pattern = r\"[A-Za-z_][A-Za-z0-9_]*\")]\n    pub text: NameText<'input>,\n}}\n\
             #[derive(recursa::Node)]\n#[tok(BETWEEN)]\npub struct BetweenMark;\n",
            &FIXTURE[tokens_start..tokens_end]
        )
    }

    /// Runs source analysis over one inline grammar.
    fn lowered(source: &str) -> LoweredGrammar {
        let package = tempfile::tempdir().expect("temporary lookahead fixture");
        std::fs::write(package.path().join("grammar.rs"), source).expect("write fixture");
        analyze_source_tree(
            SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("fixture failed source analysis:\n{error}"))
        .analyzed
        .lowered
    }

    /// Lowers an inline grammar to an LR grammar.
    fn lr(source: &str) -> (LoweredGrammar, Result<LrGrammar, Vec<LrDiagnostic>>) {
        let lowered = lowered(source);
        let roles = TypeRoles::from_lowered(&lowered);
        let grammar = lower(&lowered.grammar, &roles);
        (lowered, grammar)
    }

    /// Renders the rules of the nonterminal named `name`.
    fn rules_of(grammar: &LrGrammar, name: &str) -> Vec<String> {
        let (id, _) = grammar
            .nonterminals()
            .iter()
            .enumerate()
            .find(|(_, nonterminal)| nonterminal.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "no nonterminal named {name} among {:?}",
                    grammar.nonterminals().iter().map(|nonterminal| &nonterminal.name).collect::<Vec<_>>()
                )
            });
        grammar
            .rules_of(super::NonterminalId::new(id as u32))
            .iter()
            .map(|rule| grammar.render_rule(*rule))
            .collect()
    }

    /// The index of the one rule rendering as `text`.
    fn rule_index(grammar: &LrGrammar, text: &str) -> usize {
        let indexes: Vec<usize> = (0..grammar.rules().len())
            .filter(|index| grammar.render_rule(RuleId::new(*index as u32)) == text)
            .collect();
        assert_eq!(indexes.len(), 1, "one rule renders as `{text}`: {indexes:?}");
        indexes[0]
    }

    /// Asserts that the rule rendering as `twin` directly follows the one rendering as
    /// `original` and is its clone but for the right-hand side.
    fn assert_twin_follows(grammar: &LrGrammar, original: &str, twin: &str) {
        let index = rule_index(grammar, original);
        assert_eq!(rule_index(grammar, twin), index + 1, "`{twin}` directly follows `{original}`");
        let (original, twin) = (&grammar.rules()[index], &grammar.rules()[index + 1]);
        assert_eq!(original.lhs, twin.lhs);
        assert_eq!(original.rhs.len(), twin.rhs.len());
        assert_eq!(original.value_pops, twin.value_pops);
        assert_eq!(original.precedence, twin.precedence);
        assert_eq!(original.action, twin.action);
        assert_eq!(original.fold, twin.fold);
        assert_eq!(original.tail, twin.tail);
    }

    /// Finds a terminal by name.
    fn terminal(grammar: &LrGrammar, name: &str) -> TerminalId {
        (0..grammar.terminal_count())
            .map(|index| TerminalId::new(index as u32))
            .find(|terminal| grammar.terminal_name(*terminal) == name)
            .unwrap_or_else(|| panic!("no terminal named {name}"))
    }

    #[test]
    fn the_normalized_grammar_carries_the_filter_and_its_merged_kind() {
        let lowered = lowered(&not_between_grammar(true));
        let grammar = &lowered.grammar;
        let kind = |authored: &str| {
            grammar
                .kinds
                .iter()
                .position(|kind| kind.authored.as_deref() == Some(authored))
                .map(|index| crate::KindId::new(index as u16))
                .unwrap_or_else(|| panic!("no kind authored as {authored}"))
        };
        assert_eq!(
            grammar.lookahead_filters,
            [crate::LookaheadFilter {
                merged: kind("NOT_LA"),
                token: kind("NOT"),
                triggers: vec![kind("BETWEEN"), kind("IN"), kind("LIKE")],
            }]
        );
        // The merged kind is the last kind and no grammar type matches it.
        assert_eq!(kind("NOT_LA").index(), grammar.kinds.len() - 1);
        assert_eq!(grammar.kinds[kind("NOT_LA").index()].name, "merged:NOT_LA");
        assert!(grammar.types.iter().all(|description| match &description.shape {
            crate::TypeShape::Token { atom, .. } => {
                grammar.atoms[atom.index()].expression != crate::AtomExpression::Kind(kind("NOT_LA"))
            }
            _ => true,
        }));
    }

    #[test]
    fn the_not_between_shape_conflicts_without_the_filter_and_not_with_it() {
        let (_, without) = lr(&not_between_grammar(false));
        let without = without.expect("the fixture lowers");
        let automaton = Automaton::build(&without, &NoPrecedence, AutomatonOptions::default());
        let not = terminal(&without, "NOT");
        assert!(
            automaton.conflicts().iter().any(|conflict| {
                conflict.lookahead == not && matches!(conflict.kind, ConflictKind::ReduceReduce { .. })
            }),
            "without the filter, `Subject` and `Operand` both reduce on `NOT`: {:?}",
            automaton.conflicts()
        );
        assert_eq!(rules_of(&without, "grammar::NotBetween"), ["grammar::NotBetween -> grammar::Subject NOT BETWEEN grammar::Number AND grammar::Number"]);

        let (_, with) = lr(&not_between_grammar(true));
        let with = with.expect("the fixture lowers with the filter");
        let precedence = BindingPowerPrecedence::new(&with).expect("the fixture's binding powers have precedence equivalents");
        let automaton = Automaton::build(&with, &precedence, AutomatonOptions::default());
        assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
        assert_eq!(rules_of(&with, "grammar::NotBetween"), ["grammar::NotBetween -> grammar::Subject NOT_LA BETWEEN grammar::Number AND grammar::Number"]);
        assert_eq!(rules_of(&with, "grammar::NotIn"), ["grammar::NotIn -> grammar::Subject NOT_LA IN LPAREN grammar::Number RPAREN"]);
        assert_eq!(rules_of(&with, "grammar::NotLike"), ["grammar::NotLike -> grammar::Subject NOT_LA LIKE grammar::Name"]);
        // `NOT` before a condition, which never begins with a trigger, stays `NOT`; so does the
        // unary mark, which ends its rule and is followed only by a condition.
        assert_eq!(rules_of(&with, "grammar::AndNot"), ["grammar::AndNot -> grammar::Operand NOT grammar::Cond"]);
        assert_eq!(rules_of(&with, "grammar::NotMark"), ["grammar::NotMark -> NOT"]);
        assert_eq!(with.terminal_count(), without.terminal_count() + 1);
        // The tables parse both readings of `NOT`.
        let tables = LrTables::pack(&with, &automaton);
        let rule_type = with.starts().iter().find(|start| with.nonterminal(with.rule(start.rule).lhs).name == "$accept(grammar::Rule)").expect("Rule has a start rule");
        let start = automaton.start_state(rule_type.type_id).expect("start state").index() as u32;
        let name = terminal(&with, "NameText");
        let number = terminal(&with, "NumberText");
        let word = |spelling: &str| terminal(&with, spelling);
        // `a not between 1 and 2;`
        let trace = simulate(&with, &tables, start, &[name, word("NOT_LA"), word("BETWEEN"), number, word("AND"), number, word("SEMI")]).expect("NOT BETWEEN parses");
        assert_eq!(trace.consumed, 7);
        assert!(trace.reductions.iter().any(|rule| rule == "grammar::Subject -> grammar::Name"), "{:?}", trace.reductions);
        // `a not not b;`
        let trace = simulate(&with, &tables, start, &[name, word("NOT"), word("NOT"), name, word("SEMI")]).expect("a conjunction with a unary NOT parses");
        assert_eq!(trace.consumed, 5);
        assert!(trace.reductions.iter().any(|rule| rule == "grammar::Operand -> grammar::Name"), "{:?}", trace.reductions);
    }

    #[test]
    fn a_pratt_prefix_whose_operand_can_begin_with_a_trigger_is_twinned() {
        // gram.y's `NOT a_expr | NOT_LA a_expr`: the operand of prefix `NOT` can begin with
        // `like` or `between` as a name, so the prefix rule is emitted with both kinds, and the
        // tests keep the merged kind alone.
        let (_, with) = lr(&not_between_grammar(true));
        let with = with.expect("the fixture lowers with the filter");
        let rendered: Vec<String> = (0..with.rules().len())
            .map(|index| with.render_rule(RuleId::new(index as u32)))
            .collect();
        let original = rendered
            .iter()
            .find(|rule| rule.starts_with("grammar::Expr::") && rule.ends_with(" -> NOT grammar::Expr[min 10]"))
            .unwrap_or_else(|| panic!("the prefix operand attachment keeps `NOT`: {rendered:#?}"));
        let twin = original.replace("-> NOT ", "-> NOT_LA ");
        assert_twin_follows(&with, original, &twin);
        let tests: Vec<&String> = rendered
            .iter()
            .filter(|rule| rule.starts_with("grammar::Expr::") && rule.contains("-> NOT"))
            .collect();
        assert_eq!(tests.len(), 5, "the twins and the three tests: {tests:#?}");
        assert!(tests.iter().any(|rule| rule.ends_with(" -> NOT_LA LIKE grammar::Expr[min 20]")), "{tests:#?}");
        assert!(tests.iter().any(|rule| rule.ends_with(" -> NOT_LA BETWEEN grammar::Expr[min 20]")), "{tests:#?}");
        assert!(tests.iter().any(|rule| rule.ends_with(" -> NOT_LA IN LPAREN grammar::Expr[min 20] RPAREN")), "{tests:#?}");
        assert!(!rendered.iter().any(|rule| rule.contains("grammar::Expr[min 10] -> NOT ")), "the clone's prefix is the attachment's: {rendered:#?}");

        // The automaton is conflict-free under binding-power precedence, and the tables parse
        // every reading of `NOT`: `not a not like b;`, `not like like b;`, `not between;`.
        let precedence = BindingPowerPrecedence::new(&with).expect("the fixture's binding powers have precedence equivalents");
        let automaton = Automaton::build(&with, &precedence, AutomatonOptions::default());
        assert!(automaton.conflicts().is_empty(), "{:#?}", automaton.conflicts());
        let tables = LrTables::pack(&with, &automaton);
        let expr_rule = with.starts().iter().find(|start| with.nonterminal(with.rule(start.rule).lhs).name == "$accept(grammar::ExprRule)").expect("ExprRule has a start rule");
        let start = automaton.start_state(expr_rule.type_id).expect("start state").index() as u32;
        let word = |spelling: &str| terminal(&with, spelling);
        let name = word("NameText");
        let trace = simulate(&with, &tables, start, &[word("NOT"), name, word("NOT_LA"), word("LIKE"), name, word("SEMI")]).expect("`not a not like b;` parses");
        assert_eq!(trace.consumed, 6);
        assert_eq!(trace.reductions.iter().filter(|rule| rule.contains("-> NOT grammar::Expr[min 10]")).count(), 1, "{:#?}", trace.reductions);
        let trace = simulate(&with, &tables, start, &[word("NOT_LA"), word("LIKE"), word("LIKE"), name, word("SEMI")]).expect("`not like like b;` parses");
        assert_eq!(trace.consumed, 5);
        assert_eq!(trace.reductions.iter().filter(|rule| rule.contains("-> NOT_LA grammar::Expr[min 10]")).count(), 1, "{:#?}", trace.reductions);
        let trace = simulate(&with, &tables, start, &[word("NOT_LA"), word("BETWEEN"), word("SEMI")]).expect("`not between;` parses");
        assert_eq!(trace.consumed, 3);
        let trace = simulate(&with, &tables, start, &[word("NOT"), word("NOT"), name, word("SEMI")]).expect("`not not a;` parses");
        assert_eq!(trace.consumed, 4);
    }

    #[test]
    fn a_token_inside_a_rule_is_twinned_when_what_follows_mixes_triggers_and_others() {
        let source = small_grammar(
            r#"
#[derive(recursa::Node)]
pub struct Wrap<'input> {
    #[tok(NOT)]
    pub inner: Thing<'input>,
}

#[derive(recursa::Node)]
pub enum Thing<'input> {
    Between { mark: BetweenMark },
    Name { name: Name<'input> },
}
"#,
        );
        let (lowered, result) = lr(&source);
        let grammar = result.expect("`NOT Thing` twins: `Thing` begins with `BETWEEN` and with a name");
        assert_eq!(rules_of(&grammar, "Wrap"), ["Wrap -> NOT Thing", "Wrap -> NOT_LA Thing"]);
        assert_twin_follows(&grammar, "Wrap -> NOT Thing", "Wrap -> NOT_LA Thing");
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
        let tables = LrTables::pack(&grammar, &automaton);
        let wrap = lowered.grammar.types.iter().position(|description| description.name == "Wrap").expect("Wrap");
        let start = automaton.start_state(crate::TypeId::new(wrap as u32)).expect("start").index() as u32;
        let trace = simulate(&grammar, &tables, start, &[terminal(&grammar, "NOT_LA"), terminal(&grammar, "BETWEEN")]).expect("`not between` parses");
        assert_eq!(trace.consumed, 2);
        assert_eq!(trace.reductions.last().map(String::as_str), Some("Wrap -> NOT_LA Thing"), "{:?}", trace.reductions);
        let trace = simulate(&grammar, &tables, start, &[terminal(&grammar, "NOT"), terminal(&grammar, "NameText")]).expect("`not a` parses");
        assert_eq!(trace.consumed, 2);
        assert_eq!(trace.reductions.last().map(String::as_str), Some("Wrap -> NOT Thing"), "{:?}", trace.reductions);
    }

    #[test]
    fn a_rule_with_two_twinned_positions_is_emitted_once_per_combination() {
        let source = small_grammar(
            r#"
#[derive(recursa::Node)]
pub struct Twice<'input> {
    #[tok(NOT)]
    pub first: Thing<'input>,
    #[tok(NOT)]
    pub second: Thing<'input>,
}

#[derive(recursa::Node)]
pub enum Thing<'input> {
    Between { mark: BetweenMark },
    Name { name: Name<'input> },
}
"#,
        );
        let (lowered, result) = lr(&source);
        let grammar = result.expect("lowers");
        // The original first, then the combinations with the leftmost position as the low bit.
        assert_eq!(
            rules_of(&grammar, "Twice"),
            [
                "Twice -> NOT Thing NOT Thing",
                "Twice -> NOT_LA Thing NOT Thing",
                "Twice -> NOT Thing NOT_LA Thing",
                "Twice -> NOT_LA Thing NOT_LA Thing",
            ]
        );
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
        let tables = LrTables::pack(&grammar, &automaton);
        let twice = lowered.grammar.types.iter().position(|description| description.name == "Twice").expect("Twice");
        let start = automaton.start_state(crate::TypeId::new(twice as u32)).expect("start").index() as u32;
        let word = |spelling: &str| terminal(&grammar, spelling);
        let trace = simulate(&grammar, &tables, start, &[word("NOT"), word("NameText"), word("NOT_LA"), word("BETWEEN")]).expect("`not a not between` parses");
        assert_eq!(trace.consumed, 4);
        assert_eq!(trace.reductions.last().map(String::as_str), Some("Twice -> NOT Thing NOT_LA Thing"), "{:?}", trace.reductions);
    }

    #[test]
    fn a_token_ending_a_rule_is_twinned_only_when_its_follow_mixes_triggers_and_others() {
        let source = small_grammar(
            r#"
#[derive(recursa::Node)]
#[tok(this, NOT)]
pub struct Negated<'input> {
    pub name: Name<'input>,
}

#[derive(recursa::Node)]
pub struct Pair<'input> {
    pub left: Negated<'input>,
    pub right: Tail<'input>,
}

#[derive(recursa::Node)]
pub enum Tail<'input> {
    Between { mark: BetweenMark },
    Name { name: Name<'input> },
}

#[derive(recursa::Node)]
#[tok(this, NOT)]
pub struct Plain<'input> {
    pub name: Name<'input>,
}

#[derive(recursa::Node)]
pub struct Other<'input> {
    pub left: Plain<'input>,
    pub right: Name<'input>,
}
"#,
        );
        let (lowered, result) = lr(&source);
        let grammar = result.expect("lowers");
        // `Negated` is followed by `BETWEEN` (a trigger) and by a name or `$end`: both twins.
        assert_eq!(
            rules_of(&grammar, "Negated"),
            ["Negated -> Name NOT", "Negated -> Name NOT_LA"]
        );
        // `Plain` is followed by a name or `$end` only: the token stays.
        assert_eq!(rules_of(&grammar, "Plain"), ["Plain -> Name NOT"]);
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
        let tables = LrTables::pack(&grammar, &automaton);
        let pair = lowered.grammar.types.iter().position(|description| description.name == "Pair").expect("Pair");
        let start = automaton.start_state(crate::TypeId::new(pair as u32)).expect("start").index() as u32;
        let name = terminal(&grammar, "NameText");
        let trace = simulate(&grammar, &tables, start, &[name, terminal(&grammar, "NOT_LA"), terminal(&grammar, "BETWEEN")]).expect("`a not between` parses");
        assert_eq!(trace.consumed, 3);
        let trace = simulate(&grammar, &tables, start, &[name, terminal(&grammar, "NOT"), name]).expect("`a not b` parses");
        assert_eq!(trace.consumed, 3);
    }

    #[test]
    fn the_packed_tables_carry_the_filter_as_token_merged_trigger_triples() {
        let (_, with) = lr(&not_between_grammar(true));
        let with = with.expect("lowers");
        let word = |name: &str| terminal(&with, name).index() as u32;
        assert_eq!(
            with.filters(),
            [super::TerminalFilter {
                token: terminal(&with, "NOT"),
                merged: terminal(&with, "NOT_LA"),
                triggers: vec![terminal(&with, "BETWEEN"), terminal(&with, "IN"), terminal(&with, "LIKE")],
            }]
        );
        let automaton = Automaton::build(&with, &NoPrecedence, AutomatonOptions::default());
        let tables = LrTables::pack(&with, &automaton);
        assert_eq!(
            tables.lookahead,
            [
                word("NOT"), word("NOT_LA"), word("BETWEEN"),
                word("NOT"), word("NOT_LA"), word("IN"),
                word("NOT"), word("NOT_LA"), word("LIKE"),
            ]
        );
        let decoded = LrTables::decode(&tables.encode()).expect("round trip");
        assert_eq!(decoded, tables);

        let (_, without) = lr(&not_between_grammar(false));
        let without = without.expect("lowers");
        assert!(without.filters().is_empty());
        let automaton = Automaton::build(&without, &NoPrecedence, AutomatonOptions::default());
        assert!(LrTables::pack(&without, &automaton).lookahead.is_empty());
    }

    #[test]
    fn a_grammar_without_a_lookahead_block_lowers_exactly_as_before() {
        // The pass is a no-op without a declaration: rule count, order and text are unchanged.
        let (lowered, result) = lr(&not_between_grammar(false));
        let grammar = result.expect("lowers");
        let roles = TypeRoles::from_lowered(&lowered);
        let again = lower(&lowered.grammar, &roles).expect("lowers again");
        assert_eq!(grammar, again);
        assert!(grammar.rules().iter().all(|rule| rule.rhs.iter().all(|symbol| match symbol {
            super::Symbol::Terminal(terminal) => !grammar.terminal_name(*terminal).ends_with("_LA"),
            super::Symbol::Nonterminal(_) => true,
        })));
    }

    /// The pg-sql window-frame shape of recursa#128: a keyword that names a column also opens a
    /// frame extent, and the token that decides is the merged kind of a lookahead filter.
    ///
    /// `ROWS BETWEEN . NOT_LA` is pg-sql's one conflict no declaration could retire before this
    /// ticket: the reduce takes `BETWEEN` as a column name that a `NOT LIKE` test may follow,
    /// the shift takes it as the frame keyword whose first bound begins with the prefix twin
    /// `NOT_LA`. gram.y settles it by giving `NOT_LA` `BETWEEN`'s own `%nonassoc` level
    /// (gram.y:837), which makes the tie an error action.
    ///
    /// `level` is the body of the one `precedence` level, which the callers vary to name the
    /// merged kind, the base token, or neither.
    fn frame_grammar(level: &str) -> String {
        format!(
            r#"
recursa::grammar! {{ module = crate, keyword_matching = sensitive, max_lookahead = 3 }}
recursa::tokens! {{
    ignore = r"[ \t\n\r]+",
    keywords {{
        NOT => "not",
        BETWEEN => "between",
        AND => "and",
        LIKE => "like",
        ROWS => "rows",
        PRECEDING => "preceding",
    }}
    punctuation {{ SEMI => ";" }}
    lookahead {{ NOT_LA = NOT before {{ BETWEEN, LIKE }} }}
    precedence {{ {level} }}
}}

#[derive(recursa::Node, Debug, Clone)]
#[tok(ROWS, this, SEMI)]
pub struct Frame<'input> {{
    pub extent: Extent<'input>,
}}

#[derive(recursa::Node, Debug, Clone)]
pub enum Extent<'input> {{
    Between {{
        #[tok(BETWEEN)]
        low: Bound<'input>,
        #[tok(AND)]
        high: Bound<'input>,
    }},
    Single(Bound<'input>),
}}

#[derive(recursa::Node, Debug, Clone)]
#[tok(this, PRECEDING)]
pub struct Bound<'input> {{
    pub offset: Expr<'input>,
}}

#[derive(recursa::Node, Debug, Clone)]
#[pratt]
pub enum Expr<'input> {{
    Column(Ident<'input>),
    #[parse(prefix, bp = 10)]
    Not(#[tok(NOT, this)] Box<Self>),
    #[parse(postfix, bp = 20)]
    NotLike(Box<Self>, #[tok(NOT, LIKE, this)] Box<Self>),
    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok(LIKE)]
    Like(Box<Self>, Box<Self>),
}}

#[derive(recursa::Node, Debug, Clone)]
pub struct Ident<'input> {{
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(BETWEEN, LIKE))]
    pub text: IdentText<'input>,
}}
"#
        )
    }

    /// The conflicts of `frame_grammar` whose lookahead is the merged kind, each as its kind
    /// and the rule that wants to reduce, which is stable across state numbering.
    fn merged_conflicts(grammar: &LrGrammar) -> Vec<String> {
        let precedence =
            BindingPowerPrecedence::new(grammar).expect("the fixture has a precedence equivalent");
        let automaton = Automaton::build(grammar, &precedence, AutomatonOptions::default());
        let merged = terminal(grammar, "NOT_LA");
        automaton
            .conflicts()
            .iter()
            .filter(|conflict| conflict.lookahead == merged)
            .map(|conflict| match &conflict.kind {
                ConflictKind::ShiftReduce { reduce, .. } => {
                    format!("shift/reduce against `{}`", grammar.render_rule(*reduce))
                }
                ConflictKind::ReduceReduce { rules } => format!(
                    "reduce/reduce among [{}]",
                    rules
                        .iter()
                        .map(|rule| grammar.render_rule(*rule))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
            .collect()
    }

    #[test]
    fn a_merged_kind_named_in_the_precedence_block_settles_the_window_frame_conflict() {
        let (_, undeclared) = lr(&frame_grammar("nonassoc(bp = 20) { BETWEEN, LIKE },"));
        let undeclared = undeclared.expect("the frame fixture lowers");
        assert_eq!(
            merged_conflicts(&undeclared),
            ["shift/reduce against `IdentText -> BETWEEN`"],
            "without the merged kind's level `ROWS BETWEEN . NOT_LA` is a conflict"
        );

        let (_, declared) = lr(&frame_grammar("nonassoc(bp = 20) { BETWEEN, LIKE, NOT_LA },"));
        let declared = declared.expect("the frame fixture lowers with the merged kind declared");
        assert_eq!(
            merged_conflicts(&declared),
            Vec::<String>::new(),
            "the merged kind's level settles every conflict on it"
        );
    }

    #[test]
    fn a_merged_kind_does_not_inherit_the_level_of_the_token_it_merges() {
        // gram.y gives `NOT` level 834 and `NOT_LA` level 837, and where a twin is to take its
        // base token's level it says so (`NOT_LA a_expr %prec NOT`, gram.y:14855). A twin that
        // inherited silently would hand `WITH_LA` and `WITHOUT_LA` the `IDENT` level of `WITH`
        // and `WITHOUT`, which bison gives neither, and retire conflicts nobody declared away.
        // pg-sql measured the same thing on the real grammar: declaring the base token leaves
        // the conflict count at 33, declaring the twin takes it to 32.
        let (_, base) = lr(&frame_grammar("nonassoc(bp = 20) { BETWEEN, LIKE, NOT },"));
        let base = base.expect("the frame fixture lowers with the base token declared");
        assert_eq!(
            merged_conflicts(&base),
            ["shift/reduce against `IdentText -> BETWEEN`"],
            "the base token's level does not reach its twin"
        );
        assert!(
            base.declared_precedence(terminal(&base, "NOT")).is_some(),
            "the base token itself does take the level"
        );
        assert_eq!(base.declared_precedence(terminal(&base, "NOT_LA")), None);
    }
}
