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
//! The token precedences are read per nonterminal, so an exclusion clone (`Expr[excl In]`) and
//! a second Pratt type that reuses a token keep their own levels.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::automaton::{ConflictResolver, Item, Resolution, TerminalSet, first_sets};
use super::grammar::{Associativity, LrGrammar, NonterminalId, RuleId, Symbol, TerminalId};
use super::report::{LrDiagnostic, NO_PRECEDENCE_EQUIVALENT};
use crate::PrattPosition;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// The token precedences of a lowered grammar: for each nonterminal, the level every terminal
/// introduces as an infix or postfix operator.
pub struct BindingPowerPrecedence {
    /// The level of `(nonterminal, terminal)`: the `lbp` of the infix or the `bp` of the postfix
    /// rule of `nonterminal` whose tail starts with `terminal`, directly or through the FIRST
    /// set of a leading nonterminal such as an operator admission set.
    tokens: BTreeMap<(NonterminalId, TerminalId), u8>,
}

impl BindingPowerPrecedence {
    /// Reads the token precedences off `grammar`, or reports `RCA0402` for every binding-power
    /// combination the precedence model cannot reproduce: a terminal that introduces operators
    /// of one nonterminal at two levels (bison gives a token one precedence), and an infix rule
    /// with `rbp = lbp - 1` beside an operator of the same nonterminal at exactly that level
    /// (the Pratt loop lets the right operand bind that operator, rule precedence reduces first).
    pub fn new(grammar: &LrGrammar) -> Result<Self, Vec<LrDiagnostic>> {
        let nullable = grammar.nullable_nonterminals();
        let first = first_sets(grammar, &nullable);
        let mut tokens: BTreeMap<(NonterminalId, TerminalId), (u8, RuleId)> = BTreeMap::new();
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
            debug_assert_eq!(
                rule.rhs.first(),
                Some(&Symbol::Nonterminal(rule.lhs)),
                "an infix or postfix rule starts with its implicit left operand"
            );
            for terminal in sequence_first(grammar, &first, &nullable, &rule.rhs[1..]) {
                match tokens.get(&(rule.lhs, terminal)) {
                    Some(&(level, other)) if level != precedence.level => {
                        diagnostics.push(LrDiagnostic {
                            code: NO_PRECEDENCE_EQUIVALENT,
                            message: format!(
                                "terminal `{}` introduces operators of `{}` at binding powers \
                                 {level} (`{}`) and {} (`{}`); the table-driven parser style \
                                 gives a token one precedence",
                                grammar.terminal_name(terminal),
                                grammar.nonterminal(rule.lhs).name,
                                grammar.render_rule(other),
                                precedence.level,
                                grammar.render_rule(id),
                            ),
                            items: Vec::new(),
                            actions: Vec::new(),
                            notes: Vec::new(),
                        });
                    }
                    Some(_) => {}
                    None => {
                        tokens.insert((rule.lhs, terminal), (precedence.level, id));
                    }
                }
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
            let clash = tokens
                .range((rule.lhs, TerminalId::new(0))..=(rule.lhs, grammar.end_marker()))
                .find(|(_, (level, _))| *level == rbp);
            if let Some(((_, terminal), (_, other))) = clash {
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
                .map(|(key, (level, _))| (key, level))
                .collect(),
        })
    }

    /// The precedence `terminal` has after a rule of `nonterminal`: the level of the infix or
    /// postfix operator it introduces there, if any.
    pub fn token_precedence(&self, nonterminal: NonterminalId, terminal: TerminalId) -> Option<u8> {
        self.tokens.get(&(nonterminal, terminal)).copied()
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
        let token = self.token_precedence(rule.lhs, lookahead)?;
        Some(match precedence.level.cmp(&token) {
            Ordering::Greater => Resolution::Reduce,
            Ordering::Less => Resolution::Shift,
            Ordering::Equal => match precedence.associativity {
                Associativity::Left => Resolution::Reduce,
                // `rbp = lbp` or `lbp - 1`: the right operand binds the operator first.
                Associativity::Right => Resolution::Shift,
                // A prefix operand binds every operator whose `lbp` reaches its `bp`; a postfix
                // rule is complete once its operator is shifted and never meets a shift.
                Associativity::None => Resolution::Shift,
            },
        })
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
        assert_eq!(precedence.token_precedence(grammar.nonterminal_by_name("E").unwrap(), terminal(&grammar, "^")), Some(30));
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
    fn a_terminal_introducing_operators_at_two_levels_is_rca0402() {
        let mut grammar = TextbookGrammar::new(&["id", "+", "x"]);
        grammar.rule("E", "id");
        grammar.pratt_rule("E", "E + E", PrattPosition::Infix { lbp: 10, rbp: 11 });
        grammar.pratt_rule("E", "E + x", PrattPosition::Postfix { bp: 20 });
        grammar.start("E");
        let grammar = grammar.finish();
        let error = BindingPowerPrecedence::new(&grammar).unwrap_err();
        assert_eq!(error.len(), 1);
        assert_eq!(error[0].code, "RCA0402");
        assert_eq!(
            error[0].message,
            "terminal `+` introduces operators of `E` at binding powers 10 (`E -> E + E`) and \
             20 (`E -> E + x`); the table-driven parser style gives a token one precedence"
        );
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
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "+")), Some(10));
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "-")), Some(10));
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "*")), Some(20));
        assert_eq!(precedence.token_precedence(expr, terminal(&grammar, "id")), None);
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
