#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! LR lowering, LALR(1) construction, comb-vector tables and conflict reports for the
//! table-driven parser style.
//!
//! This is a source-independent analysis beside [`analyze`](crate::analyze): it lowers a
//! normalized grammar to an [`LrGrammar`](crate::lr::grammar::LrGrammar), builds one LALR(1)
//! [`Automaton`](crate::lr::automaton::Automaton) with a start state per grammar type, packs it
//! into [`LrTables`](crate::lr::tables::LrTables) and reports every conflict as a diagnostic.
//! Generation runs it from the renderer-selection point when the grammar selects the
//! `table_driven` style (`generation::table_driven`). Shift/reduce conflicts between Pratt rules
//! and operator tokens are settled by
//! [`BindingPowerPrecedence`](crate::lr::precedence::BindingPowerPrecedence) through the
//! [`ConflictResolver`](crate::lr::automaton::ConflictResolver) seam.

/// LR(0) item sets, DeRemer–Pennello lookaheads and conflict detection.
pub mod automaton;
/// The LR grammar model the lowering produces and the automaton consumes.
pub mod grammar;
/// Lowering from the normalized grammar to LR rules.
pub mod lowering;
/// Binding-power precedence, the conflict resolver of the table-driven style.
pub mod precedence;
/// Bison-style automaton dump and conflict diagnostics.
pub mod report;
/// Comb-vector table packing and its deterministic encoding.
pub mod tables;

#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod textbook;

pub use automaton::{
    Action, Automaton, AutomatonOptions, Conflict, ConflictKind, ConflictResolver, Item,
    NoPrecedence, ResolvedConflict, Resolution, State, StateId, TerminalSet,
};
pub use grammar::{
    Associativity, LrGrammar, Nonterminal, NonterminalId, NonterminalOrigin, Rule, RuleAction,
    RuleId, RulePrecedence, StartRule, Symbol, TerminalId, UnrepresentableBindingPowers,
};
pub use lowering::{TypeRoles, lower};
pub use precedence::BindingPowerPrecedence;
pub use report::{LrDiagnostic, automaton_dump, conflict_diagnostics};
pub use tables::{EncodedLrTables, LrTables, PACT_NINF, TABLE_NINF};

use std::path::Path;

use crate::GenerateError;
use crate::materialization::LoweredGrammar;

#[derive(Clone, Debug)]
/// The complete table-driven analysis of one grammar: rules, automaton, packed tables and the
/// conflict diagnostics, which are hard build errors when non-empty.
pub struct LrAnalysis {
    /// The lowered LR grammar.
    pub grammar: LrGrammar,
    /// The LALR(1) automaton.
    pub automaton: Automaton,
    /// The packed tables.
    pub tables: LrTables,
    /// One diagnostic per conflict, in (state, lookahead) order.
    pub diagnostics: Vec<LrDiagnostic>,
}

#[derive(Debug)]
/// Why an analysis produced no tables.
pub enum LrAnalysisError {
    /// Source analysis of the grammar failed before lowering.
    SourceAnalysis(GenerateError),
    /// The grammar carries text predicates (`RCA0403`) or Pratt binding powers with no
    /// precedence equivalent (`RCA0402`), so it cannot be lowered.
    Lowering(Vec<LrDiagnostic>),
}

impl std::fmt::Display for LrAnalysisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceAnalysis(error) => write!(formatter, "{error}"),
            Self::Lowering(diagnostics) => {
                for diagnostic in diagnostics {
                    formatter.write_str(&diagnostic.render())?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LrAnalysisError {}

impl LrAnalysis {
    /// Runs the whole analysis over a lowered grammar, resolving shift/reduce conflicts by
    /// binding-power precedence; the error is every `RCA0402`/`RCA0403` the lowering reports.
    pub(crate) fn from_lowered(
        lowered: &LoweredGrammar,
        options: AutomatonOptions,
    ) -> Result<Self, Vec<LrDiagnostic>> {
        let roles = TypeRoles::from_lowered(lowered);
        let grammar = lower(&lowered.grammar, &roles)?;
        let precedence = BindingPowerPrecedence::new(&grammar)?;
        let automaton = Automaton::build(&grammar, &precedence, options);
        let tables = LrTables::pack(&grammar, &automaton);
        let diagnostics = conflict_diagnostics(&grammar, &automaton);
        Ok(Self {
            grammar,
            automaton,
            tables,
            diagnostics,
        })
    }

    /// Runs source analysis over a Rust 2024 grammar package and then the whole table-driven
    /// analysis, with binding-power precedence.
    ///
    /// `selected_root` is the grammar's root file relative to `manifest_dir`, as
    /// [`generate`](crate::generate) takes it. The module tree is read under Cargo's default
    /// configuration snapshot, which is what the recursa fixtures and pg-sql declare. This is
    /// the entry point for tooling that wants the automaton of a grammar outside its build
    /// script; generation itself is wired by ticket #113.
    pub fn analyze_source(
        manifest_dir: &Path,
        selected_root: &Path,
        options: AutomatonOptions,
    ) -> Result<Self, LrAnalysisError> {
        let input = crate::source_analysis::SourceAnalysisInput::rust_2024(
            manifest_dir,
            selected_root
                .to_str()
                .expect("grammar root paths are UTF-8"),
        );
        let analyzed = crate::source_analysis::analyze_source_tree(input, &mut Vec::new())
            .map_err(LrAnalysisError::SourceAnalysis)?;
        Self::from_lowered(&analyzed.analyzed.lowered, options).map_err(LrAnalysisError::Lowering)
    }

    /// Renders the bison-style automaton dump.
    pub fn dump(&self) -> String {
        automaton_dump(&self.grammar, &self.automaton)
    }
}

#[cfg(test)]
mod tests {
    //! The diagnostic-producing tests the build diagnostic registry's evidence ledger names.

    use super::automaton::{Action, Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
    use super::lowering::{TypeRoles, lower};
    use super::report::conflict_diagnostics;
    use super::textbook::{dangling_else_grammar, lalr_merge_grammar};
    use crate::{
        AtomDescription, AtomExpression, ContainerShape, Declaration, GrammarDescription,
        KindDescription, KindId, Location, ParseSite, PrattPosition, PrattVariant, SourceId,
        TextEquivalence, TextPredicate, TypeDescription, TypeId, TypeShape,
    };

    #[test]
    fn shift_reduce_conflicts_report_rca0400_with_items_lookahead_and_actions() {
        let grammar = dangling_else_grammar();
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        let diagnostics = conflict_diagnostics(&grammar, &automaton);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code, "RCA0400");
        assert_eq!(diagnostic.items, ["S -> i S .", "S -> i S . e S"]);
        assert_eq!(
            diagnostic.actions,
            [
                "shift, and go to state 5",
                "reduce using rule 1 (S -> i S)",
                "resolved as: shift, and go to state 5",
            ]
        );
        assert!(diagnostic.notes.is_empty());
        let text = diagnostic.render();
        assert!(
            text.starts_with("error[RCA0400]: shift/reduce conflict in state 4 on lookahead `e`"),
            "{text}"
        );
    }

    #[test]
    fn lalr_merge_artefacts_report_rca0401_with_an_rca0405_note() {
        let grammar = lalr_merge_grammar();
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        let diagnostics = conflict_diagnostics(&grammar, &automaton);
        assert_eq!(diagnostics.len(), 2);
        for (diagnostic, lookahead) in diagnostics.iter().zip(["d", "e"]) {
            assert_eq!(diagnostic.code, "RCA0401");
            assert_eq!(
                diagnostic.message,
                format!("reduce/reduce conflict in state 4 on lookahead `{lookahead}`")
            );
            assert_eq!(diagnostic.items, ["A -> c .", "B -> c ."]);
            assert_eq!(
                diagnostic.actions,
                [
                    "reduce using rule 5 (A -> c)",
                    "reduce using rule 6 (B -> c)",
                    "resolved as: reduce using rule 5 (A -> c)",
                ]
            );
            assert_eq!(diagnostic.notes.len(), 1);
            assert!(diagnostic.notes[0].starts_with("RCA0405: "), "{}", diagnostic.notes[0]);
        }

        // Without the canonical LR(1) check no conflict can be called an artefact.
        let unchecked = Automaton::build(
            &grammar,
            &NoPrecedence,
            AutomatonOptions {
                check_merge_artefacts: false,
                ..AutomatonOptions::default()
            },
        );
        assert!(unchecked.conflicts().iter().all(|conflict| !conflict.merge_artefact));
        assert!(matches!(
            unchecked.conflicts()[0].kind,
            ConflictKind::ReduceReduce { .. }
        ));
        assert!(matches!(unchecked.conflicts()[0].resolution, Action::Reduce(_)));
    }

    /// A normalized grammar with one token type refined by an exact-text predicate.
    fn grammar_with_text_predicate() -> GrammarDescription {
        let mut grammar = GrammarDescription::new("predicated", 1, 1, TextEquivalence::Sensitive);
        let location = grammar.push_location(Location::new(SourceId::new(0), 0..1));
        let declaration = grammar.push_declaration(Declaration::new("word", location));
        let kind = grammar.push_kind(KindDescription::new("WORD", declaration));
        let atom = grammar.push_atom(AtomDescription::new(
            "word",
            declaration,
            AtomExpression::Kind(kind),
        ));
        let predicate = grammar.push_text_predicate(TextPredicate::Exact("select".to_owned()));
        grammar.push_type(TypeDescription::new(
            "SelectWord",
            declaration,
            TypeShape::Token {
                atom,
                text_predicate: Some(predicate),
            },
        ));
        assert_eq!(kind, KindId::new(0));
        grammar
    }

    /// A normalized Pratt grammar: `Expr` with a `Number` atom and an infix `Expr::Add` over
    /// `PLUS` with the given binding powers.
    fn pratt_grammar(lbp: u8, rbp: u8) -> GrammarDescription {
        let mut grammar = GrammarDescription::new("pratt", 2, 1, TextEquivalence::Sensitive);
        let location = grammar.push_location(Location::new(SourceId::new(0), 0..1));
        let mut token = |grammar: &mut GrammarDescription, name: &str, kind: &str| {
            let declaration = grammar.push_declaration(Declaration::new(name, location));
            let kind = grammar.push_kind(KindDescription::new(kind, declaration));
            let atom = grammar.push_atom(AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        };
        let number = token(&mut grammar, "Number", "NUMBER");
        let plus = token(&mut grammar, "Plus", "PLUS");
        let expr_declaration = grammar.push_declaration(Declaration::new("Expr", location));
        let atom_declaration =
            grammar.push_declaration(Declaration::new("Expr::Number", location));
        let add_declaration = grammar.push_declaration(Declaration::new("Expr::Add", location));
        let expr = TypeId::new(grammar.types.len() as u32);
        let number_site = grammar.push_site(ParseSite::new(
            expr,
            number,
            atom_declaration,
            ContainerShape::Direct,
        ));
        let plus_site =
            grammar.push_site(ParseSite::new(expr, plus, add_declaration, ContainerShape::Direct));
        let rhs_site = grammar.push_site(
            ParseSite::new(expr, expr, add_declaration, ContainerShape::Direct).with_min_bp(rbp),
        );
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Expr",
                expr_declaration,
                TypeShape::Pratt {
                    variants: vec![
                        PrattVariant {
                            declaration: atom_declaration,
                            position: PrattPosition::Atom,
                            sites: vec![number_site],
                        },
                        PrattVariant {
                            declaration: add_declaration,
                            position: PrattPosition::Infix { lbp, rbp },
                            sites: vec![plus_site, rhs_site],
                        },
                    ],
                },
            )),
            expr
        );
        grammar
    }

    #[test]
    fn unrepresentable_binding_powers_are_rejected_with_rca0402() {
        let error = lower(&pratt_grammar(10, 13), &TypeRoles::default()).unwrap_err();
        assert_eq!(error.len(), 1);
        assert_eq!(error[0].code, "RCA0402");
        assert_eq!(
            error[0].message,
            "Pratt variant `Expr::Add` has lbp = 10 and rbp = 13, which no rule precedence \
             reproduces; the table-driven parser style needs rbp = lbp + 1 (left-associative), \
             rbp = lbp or rbp = lbp - 1 (right-associative)"
        );
        assert!(error[0].render().starts_with("error[RCA0402]: Pratt variant `Expr::Add`"));
        for (lbp, rbp) in [(10, 11), (10, 10), (10, 9)] {
            let grammar = lower(&pratt_grammar(lbp, rbp), &TypeRoles::default())
                .unwrap_or_else(|error| panic!("lbp = {lbp}, rbp = {rbp} lowers: {error:?}"));
            let add = crate::lr::RuleId::new(1);
            assert_eq!(grammar.render_rule(add), "Expr -> Expr Plus Expr");
            assert_eq!(grammar.rule(add).precedence.map(|precedence| precedence.level), Some(lbp));
        }
    }

    #[test]
    fn text_predicates_are_rejected_with_rca0403() {
        let grammar = grammar_with_text_predicate();
        let error = lower(&grammar, &TypeRoles::default()).unwrap_err();
        assert_eq!(error.len(), 1);
        assert_eq!(error[0].code, "RCA0403");
        assert_eq!(
            error[0].message,
            "token type `SelectWord` matches exact text `select`; text predicates are not \
             supported by the table-driven parser style"
        );
    }
}
