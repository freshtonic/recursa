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
//! every grammar type that gets a start state. Then:
//!
//! - `next` holds no trigger: the position keeps `TOKEN`.
//! - `next` holds only triggers: the position becomes `MERGED`.
//! - `next` holds both and `β` is not empty: the position is ambiguous, `RCA0404`, naming the
//!   node and field the position belongs to. A rule like `NOT expr` where `expr` can begin with
//!   `BETWEEN` and with a name would need the feed to look past the trigger.
//! - `next` holds both and `β` is empty (`TOKEN` ends the rule): the rule is emitted twice, once
//!   with `TOKEN` and once with `MERGED`, the twin directly after the original so a type's rules
//!   stay contiguous for the reduce dispatcher. The twins are conflict-free by construction: the
//!   automaton chooses between them by the terminal it shifts, never by a lookahead, and every
//!   state the twins lead to has the same items as the original's. What FOLLOW cannot tell
//!   apart is decided at run time by the feed, which is exactly what the declaration promises.
//!   Because every grammar type is an entry point, `$end` is in the FOLLOW of every type
//!   nonterminal, so a rule of a type that ends in `TOKEN` gets its twin whenever a trigger can
//!   follow the type at all.
//!
//! A grammar without a `lookahead` block leaves this pass without touching a rule, which keeps
//! the LR grammar of such a grammar identical to what it was before the pass existed.

use std::collections::HashMap;

use super::automaton::TerminalSet;
use super::{LrGrammar, NonterminalId, Rule, RuleAction, Symbol, TerminalFilter, TerminalId};
use super::report::{LOOKAHEAD_AMBIGUITY, LrDiagnostic};
use crate::{GrammarDescription, SiteId};

/// Rule provenance the lowering records: the parse site of every position of a site-derived
/// rule, keyed by rule index, so an ambiguous position can name its node and field.
pub(super) type RuleSites = HashMap<usize, Vec<SiteId>>;

/// One filter over LR terminals.
struct Filter {
    /// The merged terminal.
    merged: TerminalId,
    /// The triggers.
    triggers: TerminalSet,
    /// The merged kind's name, for diagnostics.
    name: String,
}

/// What the pass decided for one filtered position.
enum Decision {
    /// Keep the token.
    Keep,
    /// Substitute the merged terminal.
    Merge,
    /// Emit the rule with both terminals.
    Twin,
}

/// Applies every lookahead filter of `description` to the rules of `grammar`.
///
/// `sites` is the lowering's rule provenance and `start_nonterminals` names the nonterminals
/// that receive a start rule, whose FOLLOW is seeded with the end marker. Every ambiguous
/// position is one `RCA0404` diagnostic; when any exists the grammar is left unchanged.
pub(super) fn apply(
    grammar: &mut LrGrammar,
    description: &GrammarDescription,
    sites: &RuleSites,
    start_nonterminals: &[NonterminalId],
) -> Result<(), Vec<LrDiagnostic>> {
    if description.lookahead_filters.is_empty() {
        return Ok(());
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
            name: description.kinds[filter.merged.index()]
                .authored
                .clone()
                .unwrap_or_else(|| description.kinds[filter.merged.index()].name.clone()),
        });
    }

    let nullable = grammar.nullable_nonterminals();
    let first = first_sets(grammar, &nullable);
    let follow = follow_sets(grammar, &nullable, &first, start_nonterminals);

    let mut diagnostics = Vec::new();
    let mut rules = Vec::new();
    for (index, rule) in grammar.rules().iter().enumerate() {
        let mut substituted = rule.clone();
        let mut twin = None;
        for (position, symbol) in rule.rhs.iter().enumerate() {
            let Symbol::Terminal(terminal) = symbol else {
                continue;
            };
            let Some(filter) = filter_of[terminal.index()].map(|index| &filters[index]) else {
                continue;
            };
            let rest = &rule.rhs[position + 1..];
            let (next, rest_nullable) = first_of_sequence(rest, &nullable, &first, width);
            let mut next = next;
            if rest_nullable {
                next.union_assign(&follow[rule.lhs.index()]);
            }
            let has_trigger = next.intersects(&filter.triggers);
            let has_other = next.iter().any(|candidate| !filter.triggers.contains(candidate));
            let decision = match (has_trigger, has_other, rest.is_empty()) {
                (false, _, _) => Decision::Keep,
                (true, false, _) => Decision::Merge,
                (true, true, true) => Decision::Twin,
                (true, true, false) => {
                    diagnostics.push(ambiguity(
                        grammar,
                        description,
                        sites,
                        index,
                        position,
                        filter,
                        &next,
                    ));
                    Decision::Keep
                }
            };
            match decision {
                Decision::Keep => {}
                Decision::Merge => substituted.rhs[position] = Symbol::Terminal(filter.merged),
                Decision::Twin => {
                    let mut merged = substituted.clone();
                    merged.rhs[position] = Symbol::Terminal(filter.merged);
                    twin = Some(merged);
                }
            }
        }
        rules.push(substituted);
        rules.extend(twin);
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
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
    Ok(())
}

/// Builds the `RCA0404` diagnostic for one ambiguous position.
fn ambiguity(
    grammar: &LrGrammar,
    description: &GrammarDescription,
    sites: &RuleSites,
    rule_index: usize,
    position: usize,
    filter: &Filter,
    next: &TerminalSet,
) -> LrDiagnostic {
    let rule = &grammar.rules()[rule_index];
    let Symbol::Terminal(token) = rule.rhs[position] else {
        unreachable!("only terminal positions are filtered")
    };
    let mut triggers = Vec::new();
    let mut others = Vec::new();
    for terminal in next.iter() {
        let name = format!("`{}`", grammar.terminal_name(terminal));
        if filter.triggers.contains(terminal) {
            triggers.push(name);
        } else {
            others.push(name);
        }
    }
    let place = position_name(grammar, description, sites, rule_index, position, rule);
    LrDiagnostic {
        code: LOOKAHEAD_AMBIGUITY,
        message: format!(
            "ambiguous lookahead-filter substitution of `{}` in {place}: what follows it can \
             begin with trigger {} and with {}, so the feed's `{}` cannot be predicted; split the \
             rule so that the trigger and non-trigger cases are separate positions",
            grammar.terminal_name(token),
            triggers.join(", "),
            others.join(", "),
            filter.name,
        ),
        items: vec![grammar.render_rule(super::RuleId::new(rule_index as u32))],
        actions: Vec::new(),
        notes: Vec::new(),
    }
}

/// Names the node and field a rule position belongs to, falling back to the rule's type and
/// variant when the lowering recorded no site for it.
fn position_name(
    grammar: &LrGrammar,
    description: &GrammarDescription,
    sites: &RuleSites,
    rule_index: usize,
    position: usize,
    rule: &Rule,
) -> String {
    // A `#[tok(...)]` group is attached to the field that follows it, so the field named is the
    // first field site at or after the position; a token that ends its rule names the node.
    let authored = sites.get(&rule_index).and_then(|sites| {
        sites[position.min(sites.len())..]
            .iter()
            .map(|site| description.sites[site.index()].declaration)
            .filter_map(|declaration| description.declarations[declaration.index()].authored.as_ref())
            .find(|authored| {
                matches!(authored.role, crate::AuthoredRole::Field | crate::AuthoredRole::Presence)
            })
            .or_else(|| {
                sites.get(position).and_then(|site| {
                    let declaration = description.sites[site.index()].declaration;
                    description.declarations[declaration.index()].authored.as_ref()
                })
            })
    });
    if let Some(authored) = authored {
        let node = &authored.node;
        return match (&authored.variant, &authored.field) {
            (Some(variant), Some(field)) => {
                format!("node `{node}`, variant `{variant}`, field `{field}`")
            }
            (Some(variant), None) => format!("node `{node}`, variant `{variant}`"),
            (None, Some(field)) => format!("node `{node}`, field `{field}`"),
            (None, None) => format!("node `{node}`"),
        };
    }
    match &rule.action {
        RuleAction::Construct {
            type_id,
            variant: Some(variant),
        } => format!(
            "node `{}`, variant `{}`",
            description.types[type_id.index()].name,
            description.declarations[variant.index()].name
        ),
        RuleAction::Construct { type_id, .. }
        | RuleAction::FixedSequence { type_id }
        | RuleAction::Token { type_id }
        | RuleAction::Accept { type_id } => {
            format!("node `{}`", description.types[type_id.index()].name)
        }
        _ => format!("rule `{}`", grammar.nonterminal(rule.lhs).name),
    }
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
    //! PostgreSQL `NOT BETWEEN` shape conflicts without the filter and not with it, an ambiguous
    //! position is `RCA0404`, and a token at the end of a rule follows the FOLLOW rule.

    use super::super::automaton::{Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
    use super::super::{LrGrammar, TerminalId};
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

    /// A small grammar over the fixture's tokens, for the ambiguous and end-of-rule shapes.
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
        let automaton = Automaton::build(&with, &NoPrecedence, AutomatonOptions::default());
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
    fn an_ambiguous_substitution_reports_rca0404_naming_node_and_field() {
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
        let (_, result) = lr(&source);
        let diagnostics = result.expect_err("`NOT Thing` is ambiguous: `Thing` begins with `BETWEEN` and with a name");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code, "RCA0404");
        assert_eq!(
            diagnostic.message,
            "ambiguous lookahead-filter substitution of `NOT` in node `Wrap`, field `inner`: what \
             follows it can begin with trigger `BETWEEN` and with `NameText`, so the feed's \
             `NOT_LA` cannot be predicted; split the rule so that the trigger and non-trigger \
             cases are separate positions"
        );
        assert_eq!(diagnostic.items, ["Wrap -> NOT Thing"]);
        assert!(diagnostic.render().starts_with("error[RCA0404]: "));
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
        let grammar = result.expect("end-of-rule positions never fail");
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
}
