//! Lookahead-filter substitution over lowered rules (`docs/table-driven-parsing.md`,
//! "Lookahead filters").
//!
//! A `lookahead { MERGED = TOKEN before { TRIGGER, ... } }` declaration makes the table-driven
//! token feed read `TOKEN` as `MERGED` whenever the next record is a trigger. This pass rewrites
//! the lowered rules so that the automaton expects what the feed produces. It runs inside
//! [`lower`] after every type rule exists and before the start rules
//! are added, over FIRST of the *unsubstituted* grammar, because the feed decides on lexed kinds
//! and a merged kind is never a trigger.
//!
//! # The substitution rule
//!
//! For every rule `A -> α TOKEN β` the decision is taken over `FIRST(β)` alone — the terminals
//! the *same rule* can put after `TOKEN`:
//!
//! - `FIRST(β)` holds no trigger: the position keeps `TOKEN`.
//! - `FIRST(β)` holds only triggers and `β` is not nullable: the position becomes `MERGED`, as
//!   `gram.y` writes `WITH_LA TIME ZONE` and `NULLS_LA FIRST_P`.
//! - `FIRST(β)` holds a trigger beside another terminal, or `β` is nullable: the position is
//!   twinned. The rule is emitted once for `TOKEN` and once for each applicable merged kind at
//!   that position, as gram.y writes `NOT a_expr | NOT_LA a_expr` by hand beside
//!   `a_expr NOT_LA LIKE a_expr`. If several disjoint filters merge the same source token, every
//!   applicable merged kind gets a sibling as well. The twins are conflict-free by construction:
//!   the automaton chooses between them by the terminal it shifts, never by a lookahead, and
//!   every state a twin leads to has the items of the state the original leads to, so what FIRST
//!   cannot tell apart is decided at run time by the feed, which is exactly what the declaration
//!   promises.
//!
//! A nullable `β` always keeps the source token because `FIRST(β)` does not describe what
//! follows the rule when `β` derives nothing; that is `FOLLOW(A)`, and **`FOLLOW` never drives a
//! substitution** (see "Why FOLLOW is not enough"). A nullable `β` whose `FIRST` holds a trigger
//! therefore mixes the two: the trigger earns the merged twin, and the empty derivation keeps the
//! plain one.
//!
//! # Why FOLLOW is not enough
//!
//! A rule-final `TOKEN` has an empty `β`, so a pass that decided over `FOLLOW(A)` decided over
//! the terminals that follow `A` *anywhere in the grammar*. For a keyword-admission nonterminal
//! such as `ColLabelText -> NOT`, `FOLLOW` reaches nearly every trigger, so such a pass derived
//! `ColLabelText -> NOT_LA`, `ColIdText -> WITHOUT_LA` and `ColLabelText -> FORMAT_LA` — twins
//! `gram.y` never writes. `gram.y` has no `_LA` token in `unreserved_keyword`,
//! `col_name_keyword`, `type_func_name_keyword`, `reserved_keyword`, `bare_label_keyword`,
//! `ColId`, `ColLabel` or `NonReservedWord`; it admits a merged kind only where it names one by
//! hand. Those FOLLOW-driven twins are what made a pre-19 PostgreSQL build accept
//! `COPY t TO STDOUT (FORMAT json)` (freshtonic/pg-sql#87). FOLLOW is therefore not consulted at
//! all: a rule-final `TOKEN` keeps the source token unless the grammar names the merged kind at
//! that site.
//!
//! # Naming a merged kind at a site
//!
//! A token position names a merged kind by its declared name — `#[tok(WITH_LA)]`,
//! `#[tok(NOT_LA, LIKE, this)]` — the way `gram.y` writes `opt_with: WITH | WITH_LA`,
//! `any_with: WITH | WITH_LA`, `utility_option_name: ... | FORMAT_LA` and
//! `opt_nulls_order: NULLS_LA FIRST_P`. Such a position carries the merged terminal from
//! lowering (the LR lowering pass reads the site's admitted merged kind), so this pass never
//! sees its source token and never revisits it. A name that is neither a token nor a declared
//! merged kind is RCA3106; a `#[sep(...)]` separator cannot admit one, which is RCA3153.
//!
//! A declaration whose merged kind no site ever admits and no `FIRST` ever substitutes is not an
//! error: the filter still fires at run time, and the merged kind no state has an action for is
//! a syntax error there — which is what PostgreSQL reports for the same input.
//!
//! A twin is a clone of its original: same action, value pops, precedence, fold and tail
//! decision, so the reduce dispatcher and the renderer treat it as one more rule of the same
//! type, and it needs no site provenance of its own. Twins follow their original directly so a
//! type's rules stay contiguous. A rule with `k` twinned positions is emitted once per Cartesian
//! combination of the source and merged choices. The first choice at every position is emitted
//! first, then combinations in mixed-radix order with the leftmost position changing fastest;
//! with one filter this remains `original, twin`, and the order is a function of the rule alone.
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
use super::{LrGrammar, Rule, Symbol, TerminalFilter, TerminalId};
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
/// The rule list is replaced by the substituted rules with every twin directly after its
/// original; the grammar's terminal filters are recorded for the packed tables.
pub(super) fn apply(grammar: &mut LrGrammar, description: &GrammarDescription) {
    if description.lookahead_filters.is_empty() {
        return;
    }
    let width = grammar.terminal_count();
    let mut filter_of: Vec<Vec<usize>> = vec![Vec::new(); width];
    let mut filters = Vec::new();
    for filter in &description.lookahead_filters {
        let mut triggers = TerminalSet::empty(width);
        for trigger in &filter.triggers {
            triggers.insert(TerminalId::new(u32::from(trigger.get())));
        }
        filter_of[usize::from(filter.token.get())].push(filters.len());
        filters.push(Filter {
            merged: TerminalId::new(u32::from(filter.merged.get())),
            triggers,
        });
    }

    let nullable = grammar.nullable_nonterminals();
    let first = first_sets(grammar, &nullable);
    let mut rules = Vec::with_capacity(grammar.rules().len());
    for rule in grammar.rules() {
        let mut substituted = rule.clone();
        // The twinned positions of the rule, in rule order, each with all terminals the feed can
        // present there. The source token is included when an unfiltered continuation is also
        // possible; disjoint filters contribute one merged choice apiece.
        let mut twinned: Vec<(usize, Vec<TerminalId>)> = Vec::new();
        for (position, symbol) in rule.rhs.iter().enumerate() {
            let Symbol::Terminal(terminal) = symbol else {
                continue;
            };
            let filter_indexes = &filter_of[terminal.index()];
            if filter_indexes.is_empty() {
                continue;
            }
            // Only what the same rule puts after the position decides it. A nullable rest
            // leaves the decision to FOLLOW, which this pass never consults, so the position
            // keeps the source token for that derivation.
            let (next, rest_nullable) =
                first_of_sequence(&rule.rhs[position + 1..], &nullable, &first, width);
            let mut choices = Vec::new();
            let mut trigger_union = TerminalSet::empty(width);
            for index in filter_indexes {
                let filter = &filters[*index];
                if next.intersects(&filter.triggers) {
                    choices.push(filter.merged);
                    trigger_union.union_assign(&filter.triggers);
                }
            }
            if choices.is_empty() {
                continue;
            }
            let has_base = rest_nullable
                || next
                    .iter()
                    .any(|candidate| !trigger_union.contains(candidate));
            if choices.len() == 1 && !has_base {
                substituted.rhs[position] = Symbol::Terminal(choices[0]);
            } else {
                if has_base {
                    choices.insert(0, *terminal);
                }
                twinned.push((position, choices));
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

/// The rule and its twins: one rule per Cartesian combination of the choices at every twinned
/// position. The first choice at each position is emitted first, and the leftmost position is
/// the fastest-changing digit. A rule with no twinned position is itself alone.
fn twins(rule: &Rule, twinned: &[(usize, Vec<TerminalId>)]) -> Vec<Rule> {
    let combinations = twinned.iter().fold(1usize, |count, (_, choices)| {
        count
            .checked_mul(choices.len())
            .expect("lookahead-filter twin count overflow")
    });
    (0..combinations)
        .map(|mut combination| {
            let mut twin = rule.clone();
            for (position, choices) in twinned {
                let choice = combination % choices.len();
                combination /= choices.len();
                twin.rhs[*position] = Symbol::Terminal(choices[choice]);
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

#[cfg(test)]
mod tests {
    //! The lookahead-filter tests: the normalized grammar carries the declaration, the
    //! PostgreSQL `NOT BETWEEN` shape conflicts without the filter and not with it, a token
    //! inside a rule and at its end is twinned when what follows it mixes triggers and others
    //! (gram.y's `NOT a_expr | NOT_LA a_expr`, as a prefix operator and as a plain field), several
    //! twinned positions multiply out, and the packed tables carry the filter.

    use super::super::automaton::{Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
    use super::super::lowering::{TypeRoles, lower};
    use super::super::precedence::PrecedenceResolver;
    use super::super::report::LrDiagnostic;
    use super::super::tables::{LrTables, TableAction, simulate};
    use super::super::{LrGrammar, NonterminalId, RuleId, TerminalId};
    use crate::materialization::LoweredGrammar;
    use crate::source_analysis::{SourceAnalysisInput, analyze_source_tree};

    /// The twin fixture grammar as written: table-driven, with the filter.
    const FIXTURE: &str = include_str!("../../tests/fixtures/generation/lookahead/src/grammar.rs");

    /// The fixture's `lookahead` line.
    const FILTER: &str = "    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }\n";

    /// The merged kind's membership of the fixture's `precedence` level, which only exists
    /// while the filter that declares the kind does.
    const MERGED_LEVEL: &str = ", NOT_LA }";

    /// The first and last line of the fixture's merged-kind admission block, whose rules name
    /// the merged kind and so exist only while the filter that declares it does.
    const ADMISSION: (&str, &str) = ("// merged-admission-start", "// merged-admission-end");

    /// The `NOT BETWEEN` fixture with or without its `lookahead` declaration.
    fn not_between_grammar(lookahead: bool) -> String {
        assert_eq!(FIXTURE.matches(FILTER).count(), 1);
        assert_eq!(FIXTURE.matches(MERGED_LEVEL).count(), 1);
        if lookahead {
            FIXTURE.to_owned()
        } else {
            let start = FIXTURE
                .find(ADMISSION.0)
                .expect("the fixture has an admission block");
            let end = FIXTURE
                .find(ADMISSION.1)
                .expect("the fixture's admission block is closed")
                + ADMISSION.1.len();
            let without_admission = format!("{}{}", &FIXTURE[..start], &FIXTURE[end..]);
            without_admission
                .replace(FILTER, "")
                .replace(MERGED_LEVEL, " }")
        }
    }

    /// A small grammar over the fixture's tokens, for the inside-a-rule and end-of-rule shapes.
    fn small_grammar(nodes: &str) -> String {
        let tokens_start = FIXTURE
            .find("recursa::tokens! {")
            .expect("the fixture declares tokens");
        let tokens_end = FIXTURE
            .find("recursa::ast_node! {")
            .expect("the fixture has nodes");
        format!(
            r#"recursa::grammar! {{ module = crate, keyword_matching = sensitive }}
{}
{nodes}
recursa::ast_node! {{

    pub struct Name {{
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }}
}}
recursa::ast_node! {{

    #[tok(BETWEEN)]
    pub struct BetweenMark;
}}
"#,
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
                    grammar
                        .nonterminals()
                        .iter()
                        .map(|nonterminal| &nonterminal.name)
                        .collect::<Vec<_>>()
                )
            });
        grammar
            .rules_of(NonterminalId::new(id as u32))
            .iter()
            .map(|rule| grammar.render_rule(*rule))
            .collect()
    }

    /// The index of the one rule rendering as `text`.
    fn rule_index(grammar: &LrGrammar, text: &str) -> usize {
        let indexes: Vec<usize> = (0..grammar.rules().len())
            .filter(|index| grammar.render_rule(RuleId::new(*index as u32)) == text)
            .collect();
        assert_eq!(
            indexes.len(),
            1,
            "one rule renders as `{text}`: {indexes:?}"
        );
        indexes[0]
    }

    /// Asserts that the rule rendering as `twin` directly follows the one rendering as
    /// `original` and is its clone but for the right-hand side.
    ///
    /// Precedence is deliberately not compared: a twin takes the declared level of its own
    /// last terminal, which is the merged kind, so the two agree only where the variant names
    /// a level itself, as gram.y writes `NOT_LA a_expr %prec NOT` (`gram.y:14855`).
    fn assert_twin_follows(grammar: &LrGrammar, original: &str, twin: &str) {
        let index = rule_index(grammar, original);
        assert_eq!(
            rule_index(grammar, twin),
            index + 1,
            "`{twin}` directly follows `{original}`"
        );
        let (original, twin) = (&grammar.rules()[index], &grammar.rules()[index + 1]);
        assert_eq!(original.lhs, twin.lhs);
        assert_eq!(original.rhs.len(), twin.rhs.len());
        assert_eq!(original.value_pops, twin.value_pops);
        assert_eq!(original.action, twin.action);
        assert_eq!(original.fold, twin.fold);
        assert_eq!(original.tail, twin.tail);
    }

    /// Rewrites a lexed terminal sequence the way the runtime token feed does: a filtered token
    /// immediately before one of its triggers is presented as its merged terminal.
    ///
    /// The substitution depends on the pair of records alone, never on the state, so rewriting
    /// the whole sequence up front is exactly what the parser does record by record. It reads
    /// the packed `lookahead` triples, the table the runtime decodes.
    fn feed(tables: &LrTables, lexed: &[TerminalId]) -> Vec<TerminalId> {
        lexed
            .iter()
            .enumerate()
            .map(|(position, terminal)| {
                let next = lexed.get(position + 1);
                tables
                    .lookahead
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .find(|triple| {
                        triple[0] == terminal.index() as u32
                            && next.is_some_and(|next| triple[2] == next.index() as u32)
                    })
                    .map_or(*terminal, |triple| TerminalId::new(triple[1]))
            })
            .collect()
    }

    /// The start state of the grammar type named `name`.
    fn start_state(lowered: &LoweredGrammar, automaton: &Automaton, name: &str) -> u32 {
        let type_id = lowered
            .grammar
            .types
            .iter()
            .position(|description| description.name == name)
            .unwrap_or_else(|| panic!("no grammar type named {name}"));
        automaton
            .start_state(crate::TypeId::new(type_id as u32))
            .unwrap_or_else(|| panic!("{name} has no start state"))
            .index() as u32
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
        assert!(
            grammar
                .types
                .iter()
                .all(|description| match &description.shape {
                    crate::TypeShape::Token { atom, .. } => {
                        grammar.atoms[atom.index()].expression
                            != crate::AtomExpression::Kind(kind("NOT_LA"))
                    }
                    _ => true,
                })
        );
    }

    #[test]
    fn a_content_trigger_rewrites_a_token_in_a_synthesized_attachment_rule() {
        let source = r#"recursa::grammar! {
    module = crate,
    keyword_matching = sensitive,
}

recursa::ast_node! {

    pub enum Expr {
        Unicode(UnicodeString),
    }
}

recursa::ast_node! {

    pub struct UnicodeString {
        #[lex(pattern = r"(?i:U)&'(?:[^']|'')*'")]
        pub literal: UnicodeStringLit,
        pub suffix: Option<UescapeSuffix>,
    }
}

recursa::ast_node! {

    pub struct UescapeSuffix {
        #[tok(UESCAPE, this)]
        pub escape: StringLit,
    }
}

recursa::ast_node! {

    pub struct LiteralBindings {
        #[lex(pattern = r"'[^']*'")]
        pub string: StringLit,
    }
}

// Deliberately declared after the content-token field. Source-order normalization moves the
// content definition ahead of this fixed token, so every pending name index must be remapped.
recursa::tokens! {
    keywords { UESCAPE => "uescape" }
    lookahead { UESCAPE_LA = UESCAPE before { StringLit } }
}
"#;
        let (_, grammar) = lr(source);
        let grammar = grammar.expect("the attachment fixture lowers");
        let rendered = grammar
            .rules()
            .iter()
            .enumerate()
            .map(|(index, _)| grammar.render_rule(RuleId::new(index as u32)))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|rule| rule.contains("UESCAPE_LA") && rule.contains("StringLit")),
            "the attachment must consume the merged token before its content trigger: {rendered:#?}",
        );
        assert!(
            rendered
                .iter()
                .all(|rule| !(rule.contains("UESCAPE ") && rule.contains("StringLit"))),
            "the raw token cannot precede an always-triggering content kind: {rendered:#?}",
        );
    }

    #[test]
    fn a_fixed_trigger_rewrites_a_token_in_synthesized_presence_rules() {
        let source = r#"recursa::grammar! { module = crate, keyword_matching = sensitive }

recursa::tokens! {
    keywords { WITH => "with", ORDINALITY => "ordinality", SELECT => "select" }
    lookahead { WITH_LA = WITH before { ORDINALITY } }
}

recursa::ast_node! {

    #[tok(SELECT, this)]
    pub struct Root {
        #[presence(WITH, ORDINALITY)]
        pub ordinality: bool,
    }
}
"#;
        let (_, grammar) = lr(source);
        let grammar = grammar.expect("the presence fixture lowers");
        let rendered = grammar
            .rules()
            .iter()
            .enumerate()
            .map(|(index, _)| grammar.render_rule(RuleId::new(index as u32)))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|rule| rule.contains("WITH_LA ORDINALITY")),
            "the presence must consume the merged token before its fixed trigger: {rendered:#?}",
        );
        assert!(
            rendered
                .iter()
                .all(|rule| !rule.contains("WITH ORDINALITY")),
            "the raw token cannot precede an always-triggering fixed kind: {rendered:#?}",
        );
    }

    #[test]
    fn the_not_between_shape_conflicts_without_the_filter_and_not_with_it() {
        let (_, without) = lr(&not_between_grammar(false));
        let without = without.expect("the fixture lowers");
        let automaton = Automaton::build(&without, &NoPrecedence, AutomatonOptions::default());
        let not = terminal(&without, "NOT");
        assert!(
            automaton.conflicts().iter().any(|conflict| {
                conflict.lookahead == not
                    && matches!(conflict.kind, ConflictKind::ReduceReduce { .. })
            }),
            "without the filter, `Subject` and `Operand` both reduce on `NOT`: {:?}",
            automaton.conflicts()
        );
        assert_eq!(
            rules_of(&without, "grammar::NotBetween"),
            [
                "grammar::NotBetween -> grammar::Subject NOT BETWEEN grammar::Number AND grammar::Number"
            ]
        );

        let (_, with) = lr(&not_between_grammar(true));
        let with = with.expect("the fixture lowers with the filter");
        let precedence = PrecedenceResolver::new(&with).expect("the resolver is infallible");
        let automaton = Automaton::build(&with, &precedence, AutomatonOptions::default());
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        assert_eq!(
            rules_of(&with, "grammar::NotBetween"),
            [
                "grammar::NotBetween -> grammar::Subject NOT_LA BETWEEN grammar::Number AND grammar::Number"
            ]
        );
        assert_eq!(
            rules_of(&with, "grammar::NotIn"),
            ["grammar::NotIn -> grammar::Subject NOT_LA IN LPAREN grammar::Number RPAREN"]
        );
        assert_eq!(
            rules_of(&with, "grammar::NotLike"),
            ["grammar::NotLike -> grammar::Subject NOT_LA LIKE grammar::Name"]
        );
        // `NOT` before a condition, which never begins with a trigger, stays `NOT`; so does the
        // unary mark, which ends its rule and is followed only by a condition.
        assert_eq!(
            rules_of(&with, "grammar::AndNot"),
            ["grammar::AndNot -> grammar::Operand NOT grammar::Cond"]
        );
        assert_eq!(
            rules_of(&with, "grammar::NotMark"),
            ["grammar::NotMark -> NOT"]
        );
        assert_eq!(with.terminal_count(), without.terminal_count() + 1);
        // The tables parse both readings of `NOT`.
        let tables = LrTables::pack(&with, &automaton);
        let rule_type = with
            .starts()
            .iter()
            .find(|start| {
                with.nonterminal(with.rule(start.rule).lhs).name == "$accept(grammar::Rule)"
            })
            .expect("Rule has a start rule");
        let start = automaton
            .start_state(rule_type.type_id)
            .expect("start state")
            .index() as u32;
        let name = terminal(&with, "NameText");
        let number = terminal(&with, "NumberText");
        let word = |spelling: &str| terminal(&with, spelling);
        // `a not between 1 and 2;`
        let trace = simulate(
            &with,
            &tables,
            start,
            &[
                name,
                word("NOT_LA"),
                word("BETWEEN"),
                number,
                word("AND"),
                number,
                word("SEMI"),
            ],
        )
        .expect("NOT BETWEEN parses");
        assert_eq!(trace.consumed, 7);
        assert!(
            trace
                .reductions
                .iter()
                .any(|rule| rule == "grammar::Subject -> grammar::Name"),
            "{:?}",
            trace.reductions
        );
        // `a not not b;`
        let trace = simulate(
            &with,
            &tables,
            start,
            &[name, word("NOT"), word("NOT"), name, word("SEMI")],
        )
        .expect("a conjunction with a unary NOT parses");
        assert_eq!(trace.consumed, 5);
        assert!(
            trace
                .reductions
                .iter()
                .any(|rule| rule == "grammar::Operand -> grammar::Name"),
            "{:?}",
            trace.reductions
        );
    }

    #[test]
    fn a_prefix_operator_whose_operand_can_begin_with_a_trigger_is_twinned() {
        // gram.y's `NOT a_expr | NOT_LA a_expr`: the operand of prefix `NOT` can begin with
        // `like` or `between` as a name, so the prefix rule is emitted with both kinds, and the
        // tests keep the merged kind alone.
        let (_, with) = lr(&not_between_grammar(true));
        let with = with.expect("the fixture lowers with the filter");
        let rendered: Vec<String> = (0..with.rules().len())
            .map(|index| with.render_rule(RuleId::new(index as u32)))
            .collect();
        let original = "grammar::Expr -> NOT grammar::Expr";
        let twin = "grammar::Expr -> NOT_LA grammar::Expr";
        assert!(
            rendered.iter().any(|rule| rule == original),
            "the prefix rule keeps `NOT`: {rendered:#?}"
        );
        assert_twin_follows(&with, original, twin);
        // `#[parse(prec = NOT)]` is gram.y's `NOT_LA a_expr %prec NOT`: both twins reduce at
        // `NOT`'s level even though the twin's own last terminal is the merged kind.
        assert_eq!(
            with.rules()[rule_index(&with, original)].precedence,
            with.rules()[rule_index(&with, twin)].precedence,
        );
        let tests: Vec<&String> = rendered
            .iter()
            .filter(|rule| rule.starts_with("grammar::Expr ") && rule.contains("NOT"))
            .collect();
        assert_eq!(tests.len(), 5, "the twins and the three tests: {tests:#?}");
        for expected in [
            "grammar::Expr -> grammar::Expr NOT_LA LIKE grammar::Expr",
            "grammar::Expr -> grammar::Expr NOT_LA BETWEEN grammar::Expr AND grammar::Expr",
            "grammar::Expr -> grammar::Expr NOT_LA IN LPAREN grammar::Expr RPAREN",
        ] {
            assert!(
                tests.iter().any(|rule| *rule == expected),
                "{expected}\n{tests:#?}"
            );
        }

        // The automaton is conflict-free under declared precedence, and the tables parse
        // every reading of `NOT`: `not a not like b;`, `not like like b;`, `not between;`.
        let precedence = PrecedenceResolver::new(&with).expect("the resolver is infallible");
        let automaton = Automaton::build(&with, &precedence, AutomatonOptions::default());
        assert!(
            automaton.conflicts().is_empty(),
            "{:#?}",
            automaton.conflicts()
        );
        let tables = LrTables::pack(&with, &automaton);
        let expr_rule = with
            .starts()
            .iter()
            .find(|start| {
                with.nonterminal(with.rule(start.rule).lhs).name == "$accept(grammar::ExprRule)"
            })
            .expect("ExprRule has a start rule");
        let start = automaton
            .start_state(expr_rule.type_id)
            .expect("start state")
            .index() as u32;
        let word = |spelling: &str| terminal(&with, spelling);
        let name = word("NameText");
        let trace = simulate(
            &with,
            &tables,
            start,
            &[
                word("NOT"),
                name,
                word("NOT_LA"),
                word("LIKE"),
                name,
                word("SEMI"),
            ],
        )
        .expect("`not a not like b;` parses");
        assert_eq!(trace.consumed, 6);
        assert_eq!(
            trace
                .reductions
                .iter()
                .filter(|rule| *rule == "grammar::Expr -> NOT grammar::Expr")
                .count(),
            1,
            "{:#?}",
            trace.reductions
        );
        let trace = simulate(
            &with,
            &tables,
            start,
            &[
                word("NOT_LA"),
                word("LIKE"),
                word("LIKE"),
                name,
                word("SEMI"),
            ],
        )
        .expect("`not like like b;` parses");
        assert_eq!(trace.consumed, 5);
        assert_eq!(
            trace
                .reductions
                .iter()
                .filter(|rule| *rule == "grammar::Expr -> NOT_LA grammar::Expr")
                .count(),
            1,
            "{:#?}",
            trace.reductions
        );
        let trace = simulate(
            &with,
            &tables,
            start,
            &[word("NOT_LA"), word("BETWEEN"), word("SEMI")],
        )
        .expect("`not between;` parses");
        assert_eq!(trace.consumed, 3);
        let trace = simulate(
            &with,
            &tables,
            start,
            &[word("NOT"), word("NOT"), name, word("SEMI")],
        )
        .expect("`not not a;` parses");
        assert_eq!(trace.consumed, 4);
    }

    #[test]
    fn a_token_inside_a_rule_is_twinned_when_what_follows_mixes_triggers_and_others() {
        let source = small_grammar(
            r#"recursa::ast_node! {

    pub struct Wrap {
        #[tok(NOT)]
        pub inner: Thing,
    }
}

recursa::ast_node! {

    pub enum Thing {
        Between { mark: BetweenMark },
        Name { name: Name },
    }
}
"#,
        );
        let (lowered, result) = lr(&source);
        let grammar =
            result.expect("`NOT Thing` twins: `Thing` begins with `BETWEEN` and with a name");
        assert_eq!(
            rules_of(&grammar, "Wrap"),
            ["Wrap -> NOT Thing", "Wrap -> NOT_LA Thing"]
        );
        assert_twin_follows(&grammar, "Wrap -> NOT Thing", "Wrap -> NOT_LA Thing");
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let tables = LrTables::pack(&grammar, &automaton);
        let wrap = lowered
            .grammar
            .types
            .iter()
            .position(|description| description.name == "Wrap")
            .expect("Wrap");
        let start = automaton
            .start_state(crate::TypeId::new(wrap as u32))
            .expect("start")
            .index() as u32;
        let trace = simulate(
            &grammar,
            &tables,
            start,
            &[terminal(&grammar, "NOT_LA"), terminal(&grammar, "BETWEEN")],
        )
        .expect("`not between` parses");
        assert_eq!(trace.consumed, 2);
        assert_eq!(
            trace.reductions.last().map(String::as_str),
            Some("Wrap -> NOT_LA Thing"),
            "{:?}",
            trace.reductions
        );
        let trace = simulate(
            &grammar,
            &tables,
            start,
            &[terminal(&grammar, "NOT"), terminal(&grammar, "NameText")],
        )
        .expect("`not a` parses");
        assert_eq!(trace.consumed, 2);
        assert_eq!(
            trace.reductions.last().map(String::as_str),
            Some("Wrap -> NOT Thing"),
            "{:?}",
            trace.reductions
        );
    }

    #[test]
    fn a_merged_kind_absent_from_a_state_is_a_syntax_error_there() {
        // The filter is absolute, as bison's token replacement is. `Plain -> NOT Name` keeps the
        // source token, because nothing that follows `NOT` there is a trigger, so the state has
        // no action for the merged kind at all. `not between` therefore fails on the `not` the
        // feed presented as `NOT_LA`, the way `gram.y` rejects `COPY t TO STDOUT (FORMAT json)`
        // for having no `FORMAT_LA` arm in `copy_generic_opt_elem`.
        let source = small_grammar(
            r#"recursa::ast_node! {

    pub struct Plain {
        #[tok(NOT)]
        pub name: Name,
    }
}

recursa::ast_node! {

    pub struct Test {
        #[tok(NOT, BETWEEN)]
        pub name: Name,
    }
}
"#,
        );
        let (lowered, result) = lr(&source);
        let grammar = result.expect("`NOT Name` and `NOT BETWEEN Name` lower");
        assert_eq!(rules_of(&grammar, "Plain"), ["Plain -> NOT Name"]);
        assert_eq!(rules_of(&grammar, "Test"), ["Test -> NOT_LA BETWEEN Name"]);
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let tables = LrTables::pack(&grammar, &automaton);
        let plain = start_state(&lowered, &automaton, "Plain");
        let test = start_state(&lowered, &automaton, "Test");
        let (not, not_la) = (terminal(&grammar, "NOT"), terminal(&grammar, "NOT_LA"));
        let (between, name) = (
            terminal(&grammar, "BETWEEN"),
            terminal(&grammar, "NameText"),
        );

        // The row of `Plain`'s start state shifts the source token and has nothing for the
        // merged kind; the row of `Test`'s start state is the mirror image.
        assert!(matches!(
            tables.action(plain, not.index() as u32),
            TableAction::Shift(_)
        ));
        assert_eq!(
            tables.action(plain, not_la.index() as u32),
            TableAction::Error
        );
        assert_eq!(tables.action(test, not.index() as u32), TableAction::Error);
        assert!(matches!(
            tables.action(test, not_la.index() as u32),
            TableAction::Shift(_)
        ));

        // `not between` from `Plain`: the feed presents `NOT_LA`, which the state rejects at the
        // first token. The source terminal is never retried.
        let lexed = [not, between];
        let presented = feed(&tables, &lexed);
        assert_eq!(presented, [not_la, between]);
        let error = simulate(&grammar, &tables, plain, &presented)
            .expect_err("`not between` has no reading as `Plain`");
        assert_eq!(error.position, 0);
        assert_eq!(error.state, plain);

        // `not a`, the same context without a trigger, still parses.
        let lexed = [not, name];
        let presented = feed(&tables, &lexed);
        assert_eq!(presented, lexed, "`NOT` before a name is no merge");
        let trace = simulate(&grammar, &tables, plain, &presented).expect("`not a` parses");
        assert_eq!(trace.consumed, 2);

        // And the merged kind parses where the grammar admits it.
        let lexed = [not, between, name];
        let presented = feed(&tables, &lexed);
        assert_eq!(presented, [not_la, between, name]);
        let trace = simulate(&grammar, &tables, test, &presented).expect("`not between a` parses");
        assert_eq!(trace.consumed, 3);
        assert_eq!(
            trace.reductions.last().map(String::as_str),
            Some("Test -> NOT_LA BETWEEN Name"),
            "{:?}",
            trace.reductions
        );
    }

    #[test]
    fn disjoint_filters_sharing_a_token_emit_each_merged_kind_and_the_base() {
        let source = small_grammar(
            r#"recursa::ast_node! {

    pub struct Wrap {
        #[tok(NOT)]
        pub inner: Thing,
    }
}

recursa::ast_node! {

    pub struct OnlyWrap {
        #[tok(NOT)]
        pub inner: TriggerThing,
    }
}

recursa::ast_node! {

    pub enum Thing {
        Between { mark: BetweenMark },
        In { mark: InMark },
        Name { name: Name },
    }
}

recursa::ast_node! {

    pub enum TriggerThing {
        Between { mark: BetweenMark },
        In { mark: InMark },
    }
}

recursa::ast_node! {

    #[tok(IN)]
    pub struct InMark;
}
"#,
        )
        .replace(
            "lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }",
            "lookahead { NOT_LA = NOT before { BETWEEN }, NOT_LB = NOT before { IN } }",
        );
        let (lowered, result) = lr(&source);
        let grammar = result.expect("disjoint filters lower");
        assert_eq!(
            rules_of(&grammar, "Wrap"),
            [
                "Wrap -> NOT Thing",
                "Wrap -> NOT_LA Thing",
                "Wrap -> NOT_LB Thing",
            ]
        );
        assert_eq!(
            rules_of(&grammar, "OnlyWrap"),
            [
                "OnlyWrap -> NOT_LA TriggerThing",
                "OnlyWrap -> NOT_LB TriggerThing",
            ]
        );
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let tables = LrTables::pack(&grammar, &automaton);
        let wrap = lowered
            .grammar
            .types
            .iter()
            .position(|description| description.name == "Wrap")
            .expect("Wrap");
        let start = automaton
            .start_state(crate::TypeId::new(wrap as u32))
            .expect("start")
            .index() as u32;
        let word = |name: &str| terminal(&grammar, name);
        for (merged, trigger) in [("NOT_LA", "BETWEEN"), ("NOT_LB", "IN")] {
            let trace = simulate(&grammar, &tables, start, &[word(merged), word(trigger)])
                .expect("a filtered source token parses");
            assert_eq!(trace.consumed, 2);
        }
        let trace = simulate(&grammar, &tables, start, &[word("NOT"), word("NameText")])
            .expect("the unfiltered source token parses");
        assert_eq!(trace.consumed, 2);
        let only_wrap = lowered
            .grammar
            .types
            .iter()
            .position(|description| description.name == "OnlyWrap")
            .expect("OnlyWrap");
        let only_start = automaton
            .start_state(crate::TypeId::new(only_wrap as u32))
            .expect("OnlyWrap start")
            .index() as u32;
        for (merged, trigger) in [("NOT_LA", "BETWEEN"), ("NOT_LB", "IN")] {
            let trace = simulate(
                &grammar,
                &tables,
                only_start,
                &[word(merged), word(trigger)],
            )
            .expect("a filtered source token parses without a base alternative");
            assert_eq!(trace.consumed, 2);
        }
    }

    #[test]
    fn a_rule_with_two_twinned_positions_is_emitted_once_per_combination() {
        let source = small_grammar(
            r#"recursa::ast_node! {

    pub struct Twice {
        #[tok(NOT)]
        pub first: Thing,
        #[tok(NOT)]
        pub second: Thing,
    }
}

recursa::ast_node! {

    pub enum Thing {
        Between { mark: BetweenMark },
        Name { name: Name },
    }
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
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let tables = LrTables::pack(&grammar, &automaton);
        let twice = lowered
            .grammar
            .types
            .iter()
            .position(|description| description.name == "Twice")
            .expect("Twice");
        let start = automaton
            .start_state(crate::TypeId::new(twice as u32))
            .expect("start")
            .index() as u32;
        let word = |spelling: &str| terminal(&grammar, spelling);
        let trace = simulate(
            &grammar,
            &tables,
            start,
            &[
                word("NOT"),
                word("NameText"),
                word("NOT_LA"),
                word("BETWEEN"),
            ],
        )
        .expect("`not a not between` parses");
        assert_eq!(trace.consumed, 4);
        assert_eq!(
            trace.reductions.last().map(String::as_str),
            Some("Twice -> NOT Thing NOT_LA Thing"),
            "{:?}",
            trace.reductions
        );
    }

    #[test]
    fn a_rule_final_token_is_never_twinned_by_the_follow_of_its_rule() {
        // `Negated -> Name NOT` ends in the filtered token, so nothing inside the rule follows
        // it; what may follow is `FOLLOW(Negated)`, which `Pair` fills with `BETWEEN`, a
        // trigger. FOLLOW is a property of the whole grammar, not of the site, so it never
        // admits a merged kind: `gram.y` keeps every `_LA` token out of `ColId`, `ColLabel` and
        // every keyword category, and admits one only where it names one. `Negated` therefore
        // keeps `NOT` alone, and `Plain`, whose FOLLOW holds no trigger, is unchanged.
        let source = small_grammar(
            r#"recursa::ast_node! {

    #[tok(this, NOT)]
    pub struct Negated {
        pub name: Name,
    }
}

recursa::ast_node! {

    pub struct Pair {
        pub left: Negated,
        pub right: Tail,
    }
}

recursa::ast_node! {

    pub enum Tail {
        Between { mark: BetweenMark },
        Name { name: Name },
    }
}

recursa::ast_node! {

    #[tok(this, NOT)]
    pub struct Plain {
        pub name: Name,
    }
}

recursa::ast_node! {

    pub struct Other {
        pub left: Plain,
        pub right: Name,
    }
}
"#,
        );
        let (lowered, result) = lr(&source);
        let grammar = result.expect("lowers");
        assert_eq!(rules_of(&grammar, "Negated"), ["Negated -> Name NOT"]);
        assert_eq!(rules_of(&grammar, "Plain"), ["Plain -> Name NOT"]);
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let tables = LrTables::pack(&grammar, &automaton);
        let start = start_state(&lowered, &automaton, "Pair");
        let name = terminal(&grammar, "NameText");
        let (not, not_la) = (terminal(&grammar, "NOT"), terminal(&grammar, "NOT_LA"));

        // `a not between` is now a syntax error on the `not` the feed merged, exactly where
        // PostgreSQL reports it, because no rule of this grammar admits `NOT_LA`.
        let presented = feed(&tables, &[name, not, terminal(&grammar, "BETWEEN")]);
        assert_eq!(presented, [name, not_la, terminal(&grammar, "BETWEEN")]);
        let error = simulate(&grammar, &tables, start, &presented)
            .expect_err("`a not between` has no reading of this grammar");
        assert_eq!(error.position, 1);

        // `a not b` still parses: no trigger, no merge.
        let presented = feed(&tables, &[name, not, name]);
        assert_eq!(presented, [name, not, name]);
        let trace = simulate(&grammar, &tables, start, &presented).expect("`a not b` parses");
        assert_eq!(trace.consumed, 3);
    }

    #[test]
    fn a_site_that_names_a_merged_kind_admits_it_there_and_only_there() {
        // The admission form, `gram.y`'s `opt_with: WITH | WITH_LA`: the token position names
        // the merged kind by its declared name, so lowering puts the merged terminal there and
        // the substitution pass never revisits it. `Negated` admits `NOT_LA` and `Plain`, the
        // same shape written with `NOT`, does not.
        let source = small_grammar(
            r#"recursa::ast_node! {

    #[tok(this, NOT_LA)]
    pub struct Negated {
        pub name: Name,
    }
}

recursa::ast_node! {

    pub struct Pair {
        pub left: Negated,
        pub right: Tail,
    }
}

recursa::ast_node! {

    pub enum Tail {
        Between { mark: BetweenMark },
        Name { name: Name },
    }
}

recursa::ast_node! {

    #[tok(this, NOT)]
    pub struct Plain {
        pub name: Name,
    }
}

recursa::ast_node! {

    pub struct Other {
        pub left: Plain,
        pub right: Name,
    }
}
"#,
        );
        let (lowered, result) = lr(&source);
        let grammar = result.expect("lowers");
        assert_eq!(rules_of(&grammar, "Negated"), ["Negated -> Name NOT_LA"]);
        assert_eq!(rules_of(&grammar, "Plain"), ["Plain -> Name NOT"]);
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let tables = LrTables::pack(&grammar, &automaton);
        let start = start_state(&lowered, &automaton, "Pair");
        let name = terminal(&grammar, "NameText");
        let (not, not_la) = (terminal(&grammar, "NOT"), terminal(&grammar, "NOT_LA"));

        // `a not between` parses through the admission.
        let presented = feed(&tables, &[name, not, terminal(&grammar, "BETWEEN")]);
        assert_eq!(presented, [name, not_la, terminal(&grammar, "BETWEEN")]);
        let trace = simulate(&grammar, &tables, start, &presented).expect("`a not between` parses");
        assert_eq!(trace.consumed, 3);
        assert!(
            trace
                .reductions
                .iter()
                .any(|reduction| reduction == "Negated -> Name NOT_LA"),
            "{:?}",
            trace.reductions
        );

        // `a not b` is the error now: the admitted position wants the merged kind, which the
        // feed only produces before a trigger.
        let presented = feed(&tables, &[name, not, name]);
        assert_eq!(presented, [name, not, name]);
        simulate(&grammar, &tables, start, &presented)
            .expect_err("`a not b` has no reading of this grammar");
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
                triggers: vec![
                    terminal(&with, "BETWEEN"),
                    terminal(&with, "IN"),
                    terminal(&with, "LIKE")
                ],
            }]
        );
        let automaton = Automaton::build(&with, &NoPrecedence, AutomatonOptions::default());
        let tables = LrTables::pack(&with, &automaton);
        assert_eq!(
            tables.lookahead,
            [
                word("NOT"),
                word("NOT_LA"),
                word("BETWEEN"),
                word("NOT"),
                word("NOT_LA"),
                word("IN"),
                word("NOT"),
                word("NOT_LA"),
                word("LIKE"),
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
        assert!(
            grammar
                .rules()
                .iter()
                .all(|rule| rule.rhs.iter().all(|symbol| match symbol {
                    super::Symbol::Terminal(terminal) =>
                        !grammar.terminal_name(*terminal).ends_with("_LA"),
                    super::Symbol::Nonterminal(_) => true,
                }))
        );
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
            r#"recursa::grammar! {{ module = crate, keyword_matching = sensitive }}
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
    precedence {{ right(bp = 5) {{ UNOT }}, {level} }}
}}

recursa::ast_node! {{
    #[derive(Debug, Clone)]
    #[tok(ROWS, this, SEMI)]
    pub struct Frame {{
        pub extent: Extent,
    }}
}}

recursa::ast_node! {{
    #[derive(Debug, Clone)]
    pub enum Extent {{
        Between {{
            #[tok(BETWEEN)]
            low: Bound,
            #[tok(AND)]
            high: Bound,
        }},
        Single(Bound),
    }}
}}

recursa::ast_node! {{
    #[derive(Debug, Clone)]
    #[tok(this, PRECEDING)]
    pub struct Bound {{
        pub offset: Expr,
    }}
}}

recursa::ast_node! {{
    #[derive(Debug, Clone)]
    pub enum Expr {{
        Column(Ident),
        #[parse(prec = UNOT)]
        Not(#[tok(NOT, this)] boxed!(Self)),
        NotLike(boxed!(Self), #[tok(NOT, LIKE, this)] boxed!(Self)),
        Like(boxed!(Self), #[tok(LIKE, this)] boxed!(Self)),
    }}
}}

recursa::ast_node! {{
    #[derive(Debug, Clone)]
    pub struct Ident {{
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(BETWEEN, LIKE))]
        pub text: IdentText,
    }}
}}
"#
        )
    }

    /// The conflicts of `frame_grammar` whose lookahead is the merged kind, each as its kind
    /// and the rule that wants to reduce, which is stable across state numbering.
    fn merged_conflicts(grammar: &LrGrammar) -> Vec<String> {
        let precedence = PrecedenceResolver::new(grammar).expect("the resolver is infallible");
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
        assert!(
            merged_conflicts(&undeclared)
                .contains(&"shift/reduce against `IdentText -> BETWEEN`".to_owned()),
            "without the merged kind's level `ROWS BETWEEN . NOT_LA` is a conflict: {:?}",
            merged_conflicts(&undeclared),
        );

        let (_, declared) = lr(&frame_grammar(
            "nonassoc(bp = 20) { BETWEEN, LIKE, NOT_LA },",
        ));
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
        assert!(
            merged_conflicts(&base)
                .contains(&"shift/reduce against `IdentText -> BETWEEN`".to_owned()),
            "the base token's level does not reach its twin: {:?}",
            merged_conflicts(&base),
        );
        assert!(
            base.declared_precedence(terminal(&base, "NOT")).is_some(),
            "the base token itself does take the level"
        );
        assert_eq!(base.declared_precedence(terminal(&base, "NOT_LA")), None);
    }
}
