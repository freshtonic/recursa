#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! LR lowering, LALR(1) construction, comb-vector tables and conflict reports for the
//! table-driven parser style.
//!
//! This is a source-independent analysis beside [`analyze`](crate::analyze): it lowers a
//! normalized grammar to an [`LrGrammar`](crate::lr::grammar::LrGrammar), builds one LALR(1)
//! [`Automaton`](crate::lr::automaton::Automaton) with a start state per grammar type, packs it
//! into [`LrTables`](crate::lr::tables::LrTables) and reports every conflict as a diagnostic
//! that names the shortest path to its state ([`paths`](crate::lr::paths)).
//! Generation runs it from the renderer-selection point when the grammar selects the
//! `table_driven` style (`generation::table_driven`); ticket #115 plugs binding-power precedence
//! into the [`ConflictResolver`](crate::lr::automaton::ConflictResolver) seam.

/// LR(0) item sets, DeRemer–Pennello lookaheads and conflict detection.
pub mod automaton;
/// The LR grammar model the lowering produces and the automaton consumes.
pub mod grammar;
/// Lowering from the normalized grammar to LR rules.
pub mod lowering;
/// Shortest symbol paths from the start states, for diagnostics and the dump.
pub mod paths;
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
    NoPrecedence, Resolution, State, StateId, TerminalSet,
};
pub use grammar::{
    Associativity, LrGrammar, Nonterminal, NonterminalId, NonterminalOrigin, Rule, RuleAction,
    RuleId, RulePrecedence, StartRule, Symbol, TerminalId,
};
pub use lowering::{TypeRoles, lower};
pub use paths::{SymbolPath, render_path, shortest_paths};
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
    /// The grammar carries text predicates (`RCA0403`), so it cannot be lowered.
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
    /// Runs the whole analysis over a lowered grammar.
    pub(crate) fn from_lowered(
        lowered: &LoweredGrammar,
        resolver: &dyn ConflictResolver,
        options: AutomatonOptions,
    ) -> Result<Self, Vec<LrDiagnostic>> {
        let roles = TypeRoles::from_lowered(lowered);
        let grammar = lower(&lowered.grammar, &roles)?;
        let automaton = Automaton::build(&grammar, resolver, options);
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
    /// analysis, with no precedence resolution.
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
        Self::from_lowered(&analyzed.analyzed.lowered, &NoPrecedence, options)
            .map_err(LrAnalysisError::Lowering)
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
        AtomDescription, AtomExpression, Declaration, GrammarDescription, KindDescription,
        KindId, Location, SourceId, TextEquivalence, TextPredicate, TypeDescription, TypeShape,
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
        assert_eq!(diagnostic.path, "S -> i S . e");
        let text = diagnostic.render();
        assert!(
            text.starts_with(
                "error[RCA0400]: shift/reduce conflict in state 4 on lookahead `e`\n  path: S -> i S . e\n  items:\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn conflict_paths_are_the_shortest_and_break_ties_by_symbol_order() {
        // The merged state `A -> c .` / `B -> c .` is reached by both `a c` and `b c`; the
        // paths have equal length and `a` sorts first.
        let grammar = lalr_merge_grammar();
        let automaton = Automaton::build(&grammar, &NoPrecedence, AutomatonOptions::default());
        let diagnostics = conflict_diagnostics(&grammar, &automaton);
        let paths: Vec<&str> = diagnostics.iter().map(|diagnostic| diagnostic.path.as_str()).collect();
        assert_eq!(paths, ["S -> a c . d", "S -> a c . e"]);
        let dump = super::automaton_dump(&grammar, &automaton);
        assert!(dump.contains("State 4\n\n    path: S -> a c .\n\n"), "{dump}");
        assert!(dump.contains("State 0\n\n    path: S -> .\n\n"), "{dump}");
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
