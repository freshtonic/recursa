#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Binding-power precedence: the [`ConflictResolver`] of the table-driven style.
//!
//! A shift/reduce conflict between a Pratt rule and an operator token is the decision the
//! recursive Pratt loop takes with binding powers, so the resolver takes it the same way, in
//! bison's terms. The reducing rule's precedence is its level: `lbp` for an infix rule, `bp`
//! for a prefix or postfix rule, and an interleaved shape (`BETWEEN a AND b`) carries the
//! binding powers of its leading operator. The lookahead's precedence is the level of the infix
//! or postfix operator it introduces in the reducing rule's nonterminal. The higher precedence
//! wins: a higher rule reduces, a higher token shifts. A tie goes to the rule's associativity:
//! `rbp = lbp + 1` reduces (left-associative), `rbp = lbp` or `lbp - 1` shifts
//! (right-associative), and a prefix rule shifts because its operand binds every operator whose
//! `lbp` reaches its `bp`. A terminal that introduces no operator of that nonterminal has no
//! precedence, so the conflict stays a conflict and is reported as `RCA0400`.
//!
//! Bison declares one precedence per token; here it is read off the rules, and a terminal may
//! introduce several operators of one nonterminal at different levels (`/` starts both `a / b`
//! and `a / ANY (...)` in pg-sql, which recursive descent tells apart with a second token). The
//! resolver then takes the verdict every level agrees on, and leaves the conflict when the
//! levels straddle the rule's, because no one-token decision reproduces the Pratt parse there.
//!
//! The token precedences are read per nonterminal, so an exclusion clone (`Expr[excl In]`) and
//! a second Pratt type that reuses a token keep their own levels. A fixed-token attachment
//! around an operand (`Plus -> + Expr` for `Expr -> Expr Plus`) carries its variant's
//! precedence and names `Expr` as its owner, so its reduce is decided against `Expr`'s tokens;
//! that is what gives an interleaved shape's middle operand its minimum binding power.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::automaton::{ConflictResolver, Item, Resolution, TerminalSet, first_sets};
use super::grammar::{
    Associativity, LrGrammar, NonterminalId, RuleId, RulePrecedence, Symbol, TerminalId,
};
use super::report::{LrDiagnostic, NO_PRECEDENCE_EQUIVALENT};
use crate::PrattPosition;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// The token precedences of a lowered grammar: for each nonterminal, the levels every terminal
/// introduces as an infix or postfix operator.
pub struct BindingPowerPrecedence {
    /// The levels of `(nonterminal, terminal)`: the `lbp` of each infix and the `bp` of each
    /// postfix rule of `nonterminal` whose tail starts with `terminal`, directly or through the
    /// FIRST set of a leading nonterminal such as an operator admission set. Usually one level;
    /// several, in ascending order, when the terminal starts operators that recursive descent
    /// tells apart with a second token.
    tokens: BTreeMap<(NonterminalId, TerminalId), Vec<u8>>,
}

impl BindingPowerPrecedence {
    /// Reads the token precedences off `grammar`, or reports `RCA0402` for an infix rule with
    /// `rbp = lbp - 1` beside an operator of the same nonterminal at exactly that level: the
    /// Pratt loop lets the right operand bind that operator, rule precedence reduces first.
    pub fn new(grammar: &LrGrammar) -> Result<Self, Vec<LrDiagnostic>> {
        let nullable = grammar.nullable_nonterminals();
        let first = first_sets(grammar, &nullable);
        let mut tokens: BTreeMap<(NonterminalId, TerminalId), BTreeMap<u8, RuleId>> =
            BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (index, rule) in grammar.rules().iter().enumerate() {
            let id = RuleId::new(index as u32);
            let Some(precedence) = rule.precedence else {
                continue;
            };
            if !matches!(
                precedence.position,
                PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
            ) {
                continue;
            }
            // Only the Pratt rule itself, which starts with its implicit left operand,
            // introduces the operator; an attachment around an operand carries the precedence
            // for its own reduce and introduces nothing.
            if rule.rhs.first() != Some(&Symbol::Nonterminal(rule.lhs)) {
                continue;
            }
            for terminal in sequence_first(grammar, &first, &nullable, &rule.rhs[1..]) {
                tokens
                    .entry((precedence.owner, terminal))
                    .or_default()
                    .entry(precedence.level)
                    .or_insert(id);
            }
        }
        for (index, rule) in grammar.rules().iter().enumerate() {
            let Some(precedence) = rule.precedence else {
                continue;
            };
            let PrattPosition::Infix { lbp, rbp } = precedence.position else {
                continue;
            };
            if rbp.checked_add(1) != Some(lbp) {
                continue;
            }
            // Only a bare right operand relies on precedence; an operand inside an attachment
            // or a clone parses with its own minimum binding power and is exact.
            let owner = precedence.owner;
            if rule.rhs.last() != Some(&Symbol::Nonterminal(owner)) {
                continue;
            }
            let clash = tokens
                .range((owner, TerminalId::new(0))..=(owner, grammar.end_marker()))
                .find_map(|((_, terminal), levels)| {
                    levels.get(&rbp).map(|other| (terminal, other))
                });
            if let Some((terminal, other)) = clash {
                diagnostics.push(LrDiagnostic {
                    code: NO_PRECEDENCE_EQUIVALENT,
                    message: format!(
                        "rule `{}` has rbp = lbp - 1 = {rbp}, and `{}` introduces `{}` at \
                         binding power {rbp}: the Pratt loop lets the right operand bind that \
                         operator, but rule precedence {lbp} reduces first; use rbp = lbp for \
                         a right-associative operator beside one at the next lower level",
                        grammar.render_rule(RuleId::new(index as u32)),
                        grammar.render_rule(*other),
                        grammar.terminal_name(*terminal),
                    ),
                    path: String::new(),
                    items: Vec::new(),
                    actions: Vec::new(),
                    notes: Vec::new(),
                });
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        Ok(Self {
            tokens: tokens
                .into_iter()
                .map(|(key, levels)| (key, levels.into_keys().collect()))
                .collect(),
        })
    }

    /// The precedences `terminal` has after a rule of `nonterminal`: the levels of the infix
    /// and postfix operators it introduces there; empty when it introduces none.
    pub fn token_precedence(&self, nonterminal: NonterminalId, terminal: TerminalId) -> &[u8] {
        self.tokens
            .get(&(nonterminal, terminal))
            .map_or(&[], |levels| levels.as_slice())
    }
}

/// Bison's verdict for one rule level against one token level.
fn verdict(rule: &RulePrecedence, token: u8) -> Resolution {
    match rule.level.cmp(&token) {
        Ordering::Greater => Resolution::Reduce,
        Ordering::Less => Resolution::Shift,
        Ordering::Equal => match rule.associativity {
            Associativity::Left => Resolution::Reduce,
            // `rbp = lbp` or `lbp - 1`: the right operand binds the operator first.
            Associativity::Right => Resolution::Shift,
            // A prefix operand binds every operator whose `lbp` reaches its `bp`; a postfix
            // rule is complete once its operator is shifted and never meets a shift.
            Associativity::None => Resolution::Shift,
        },
    }
}

impl ConflictResolver for BindingPowerPrecedence {
    fn resolve(
        &self,
        grammar: &LrGrammar,
        reduce: RuleId,
        lookahead: TerminalId,
        _shift_items: &[Item],
    ) -> Option<Resolution> {
        let rule = grammar.rule(reduce);
        let precedence = rule.precedence?;
        let mut verdicts = self
            .token_precedence(precedence.owner, lookahead)
            .iter()
            .map(|token| verdict(&precedence, *token));
        let first = verdicts.next()?;
        verdicts.all(|other| other == first).then_some(first)
    }
}

/// FIRST of a symbol sequence: the terminals that can start it, read through nullable prefixes.
fn sequence_first(
    grammar: &LrGrammar,
    first: &[TerminalSet],
    nullable: &[bool],
    symbols: &[Symbol],
) -> Vec<TerminalId> {
    let mut result = TerminalSet::empty(grammar.terminal_count());
    for symbol in symbols {
        match *symbol {
            Symbol::Terminal(terminal) => {
                result.insert(terminal);
                break;
            }
            Symbol::Nonterminal(nonterminal) => {
                result.union_assign(&first[nonterminal.index()]);
                if !nullable[nonterminal.index()] {
                    break;
                }
            }
        }
    }
    result.iter().collect()
}

#[cfg(test)]
mod tests {
    //! The resolver over textbook grammars with authored binding powers.

    use super::BindingPowerPrecedence;
    use crate::PrattPosition;
    use crate::lr::automaton::{Automaton, AutomatonOptions, ConflictKind, Resolution};
    use crate::lr::grammar::{LrGrammar, TerminalId, TextbookGrammar};
    use crate::lr::tables::{LrTables, simulate};

    /// Finds a terminal by name.
    fn terminal(grammar: &LrGrammar, name: &str) -> TerminalId {
        (0..grammar.terminal_count())
            .map(|index| TerminalId::new(index as u32))
            .find(|terminal| grammar.terminal_name(*terminal) == name)
            .unwrap_or_else(|| panic!("no terminal named {name}"))
    }

    /// Builds the automaton with binding-power precedence and returns it with the resolver.
    fn build(grammar: &LrGrammar) -> (BindingPowerPrecedence, Automaton) {
        let precedence =
            BindingPowerPrecedence::new(grammar).expect("the grammar has a precedence equivalent");
        let automaton = Automaton::build(grammar, &precedence, AutomatonOptions::default());
        (precedence, automaton)
    }

    /// Parses `source` (space-separated terminal names) from `E` and returns the rules reduced.
    fn reductions(grammar: &LrGrammar, automaton: &Automaton, source: &str) -> Vec<String> {
        let tables = LrTables::pack(grammar, automaton);
        let start = automaton.start_states()[0].1.index() as u32;
        let input: Vec<TerminalId> = source
            .split_whitespace()
            .map(|name| terminal(grammar, name))
            .collect();
        simulate(grammar, &tables, start, &input)
            .unwrap_or_else(|error| panic!("{source} does not parse: {error:?}"))
            .reductions
    }

    #[test]
    fn rbp_one_below_lbp_alone_is_right_associative() {
        let mut grammar = TextbookGrammar::new(&["id", "^", "+"]);
        grammar.rule("E", "id");
        grammar.pratt_rule("E", "E ^ E", PrattPosition::Infix { lbp: 30, rbp: 29 });
        grammar.pratt_rule("E", "E + E", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.start("E");
        let grammar = grammar.finish();
        let (precedence, automaton) = build(&grammar);
        assert!(automaton.conflicts().is_empty());
        assert_eq!(
            precedence.token_precedence(grammar.nonterminal_by_name("E").unwrap(), terminal(&grammar, "^")),
            [30]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id ^ id ^ id"),
            ["E -> id", "E -> id", "E -> id", "E -> E ^ E", "E -> E ^ E"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id + id + id"),
            ["E -> id", "E -> id", "E -> E + E", "E -> id", "E -> E + E"]
        );
    }

    #[test]
    fn rbp_one_below_lbp_beside_an_operator_at_that_level_is_rca0402() {
        let mut grammar = TextbookGrammar::new(&["id", "^", "+"]);
        grammar.rule("E", "id");
        grammar.pratt_rule("E", "E ^ E", PrattPosition::Infix { lbp: 30, rbp: 29 });
        grammar.pratt_rule("E", "E + E", PrattPosition::Infix { lbp: 29, rbp: 30 });
        grammar.start("E");
        let grammar = grammar.finish();
        let error = BindingPowerPrecedence::new(&grammar).unwrap_err();
        assert_eq!(error.len(), 1);
        assert_eq!(error[0].code, "RCA0402");
        assert!(
            error[0].message.starts_with(
                "rule `E -> E ^ E` has rbp = lbp - 1 = 29, and `E -> E + E` introduces `+` at \
                 binding power 29"
            ),
            "{}",
            error[0].message
        );
    }

    #[test]
    fn a_terminal_introducing_operators_at_two_levels_resolves_only_when_they_agree() {
        // `+` starts an infix at 10 and a postfix `+ x` at 20, as `/` starts `a / b` and
        // `a / ANY (...)` in pg-sql. Against `*` (15) and `+` itself (10, where the infix
        // reduces and the postfix shifts) the levels straddle the rule, so the conflict stays;
        // against `^` (25) and `&` (5) every level gives the same verdict.
        let mut grammar = TextbookGrammar::new(&["id", "+", "x", "*", "^", "&"]);
        grammar.rule("E", "id");
        grammar.pratt_rule("E", "E + E", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.pratt_rule("E", "E + x", PrattPosition::Postfix { bp: 20 });
        grammar.pratt_rule("E", "E * E", PrattPosition::Infix { lbp: 15, rbp: 16 });
        grammar.pratt_rule("E", "E ^ E", PrattPosition::Infix { lbp: 25, rbp: 26 });
        grammar.pratt_rule("E", "E & E", PrattPosition::Infix { lbp: 5, rbp: 6 });
        grammar.start("E");
        let grammar = grammar.finish();
        let (precedence, automaton) = build(&grammar);
        let expr = grammar.nonterminal_by_name("E").unwrap();
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "+")), [10, 20]);
        let plus = terminal(&grammar, "+");
        let unresolved: Vec<String> = automaton
            .conflicts()
            .iter()
            .map(|conflict| {
                assert_eq!(conflict.lookahead, plus);
                let ConflictKind::ShiftReduce { reduce, .. } = conflict.kind else {
                    panic!("only shift/reduce conflicts remain");
                };
                grammar.render_rule(reduce)
            })
            .collect();
        assert_eq!(unresolved, ["E -> E + E", "E -> E * E"]);
        let settled: Vec<(String, Resolution)> = automaton
            .resolved_conflicts()
            .iter()
            .filter(|resolved| resolved.lookahead == plus)
            .map(|resolved| (grammar.render_rule(resolved.reduce), resolved.resolution))
            .collect();
        assert!(settled.contains(&("E -> E ^ E".to_owned(), Resolution::Reduce)), "{settled:?}");
        assert!(settled.contains(&("E -> E & E".to_owned(), Resolution::Shift)), "{settled:?}");
        assert!(!settled.iter().any(|(rule, _)| rule == "E -> E * E"), "{settled:?}");
    }

    #[test]
    fn an_operator_introduced_through_a_nonterminal_takes_its_first_set() {
        let mut grammar = TextbookGrammar::new(&["id", "+", "-", "*"]);
        grammar.rule("E", "id");
        grammar.rule("Op", "+");
        grammar.rule("Op", "-");
        grammar.pratt_rule("E", "- E", PrattPosition::Prefix { bp: 30 });
        grammar.pratt_rule("E", "E Op E", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.pratt_rule("E", "E * E", PrattPosition::Infix { lbp: 20, rbp: 21 });
        grammar.start("E");
        let grammar = grammar.finish();
        let (precedence, automaton) = build(&grammar);
        let expr = grammar.nonterminal_by_name("E").unwrap();
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "+")), [10]);
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "-")), [10]);
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "*")), [20]);
        assert!(precedence.token_precedence(expr, terminal(&grammar, "id")).is_empty());
        assert!(automaton.conflicts().is_empty());
        assert_eq!(
            reductions(&grammar, &automaton, "- id - id * id"),
            [
                "E -> id",
                "E -> - E",
                "Op -> -",
                "E -> id",
                "E -> id",
                "E -> E * E",
                "E -> E Op E"
            ]
        );
    }

    #[test]
    fn an_operator_attachment_reduces_with_its_variants_precedence() {
        // pg-sql attaches the operator token to the operand field, so an infix variant lowers to
        // `E -> E Plus` with `Plus -> + E`; the attachment's reduce meets the next operator's
        // shift and must decide it with the variant's binding powers against E's tokens.
        let mut grammar = TextbookGrammar::new(&["id", "+", "*"]);
        grammar.rule("E", "id");
        grammar.pratt_rule("E", "E Plus", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.pratt_rule("E", "E Star", PrattPosition::Infix { lbp: 20, rbp: 21 });
        grammar.attachment_rule("Plus", "+ E", "E", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.attachment_rule("Star", "* E", "E", PrattPosition::Infix { lbp: 20, rbp: 21 });
        grammar.start("E");
        let grammar = grammar.finish();
        let (precedence, automaton) = build(&grammar);
        let expr = grammar.nonterminal_by_name("E").unwrap();
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "+")), [10]);
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "*")), [20]);
        assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
        assert_eq!(
            reductions(&grammar, &automaton, "id + id * id + id"),
            [
                "E -> id",
                "E -> id",
                "E -> id",
                "Star -> * E",
                "E -> E Star",
                "Plus -> + E",
                "E -> E Plus",
                "E -> id",
                "Plus -> + E",
                "E -> E Plus"
            ]
        );
    }

    #[test]
    fn a_prefix_rule_at_the_token_level_shifts_and_a_postfix_rule_meets_no_shift() {
        // Prefix `-` at bp 10 against infix `+` at lbp 10: the operand binds `+` (10 >= 10),
        // so the tie shifts and `- id + id` is `-(id + id)`. Postfix `!` at bp 10: once `!`
        // is shifted the rule is complete on its own, so no tie ever arises; `id + id !`
        // reduces `+` first (left-associative tie against `!` at 10) as `parse_bp(11)` stops.
        let mut grammar = TextbookGrammar::new(&["id", "-", "+", "!"]);
        grammar.rule("E", "id");
        grammar.pratt_rule("E", "- E", PrattPosition::Prefix { bp: 10 });
        grammar.pratt_rule("E", "E + E", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.pratt_rule("E", "E !", PrattPosition::Postfix { bp: 10 });
        grammar.start("E");
        let grammar = grammar.finish();
        let (_, automaton) = build(&grammar);
        assert!(automaton.conflicts().is_empty(), "{:?}", automaton.conflicts());
        let ties: Vec<(String, String, Resolution)> = automaton
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
        assert!(
            ties.contains(&("E -> - E".to_owned(), "+".to_owned(), Resolution::Shift)),
            "{ties:?}"
        );
        assert!(
            ties.contains(&("E -> E + E".to_owned(), "!".to_owned(), Resolution::Reduce)),
            "{ties:?}"
        );
        assert!(
            !ties.iter().any(|(rule, _, _)| rule == "E -> E !"),
            "a complete postfix rule never competes with a shift: {ties:?}"
        );
        assert_eq!(
            reductions(&grammar, &automaton, "- id + id"),
            ["E -> id", "E -> id", "E -> E + E", "E -> - E"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id + id !"),
            ["E -> id", "E -> id", "E -> E + E", "E -> E !"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id ! + id"),
            ["E -> id", "E -> E !", "E -> id", "E -> E + E"]
        );
    }

    #[test]
    fn a_terminal_that_introduces_no_operator_leaves_the_conflict() {
        let mut grammar = TextbookGrammar::new(&["id", "+", "x"]);
        grammar.rule("E", "id");
        grammar.pratt_rule("E", "E + E", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.rule("E", "E x");
        grammar.start("E");
        let grammar = grammar.finish();
        let (_, automaton) = build(&grammar);
        assert_eq!(automaton.conflicts().len(), 1);
        let conflict = &automaton.conflicts()[0];
        assert_eq!(conflict.lookahead, terminal(&grammar, "x"));
        assert!(matches!(conflict.kind, ConflictKind::ShiftReduce { .. }));
        assert_eq!(automaton.resolved_conflicts().len(), 1);
        assert_eq!(automaton.resolved_conflicts()[0].resolution, Resolution::Reduce);
    }
}
