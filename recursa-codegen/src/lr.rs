#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! LR lowering, LALR(1) construction, comb-vector tables and conflict reports for the Recursa
//! parser.
//!
//! This is a source-independent analysis beside
//! [`LrAnalysis::analyze_source`](crate::lr::LrAnalysis::analyze_source): it lowers a
//! normalized grammar to an [`LrGrammar`](crate::lr::grammar::LrGrammar), builds one LALR(1)
//! [`Automaton`](crate::lr::automaton::Automaton) with a start state per grammar type, packs it
//! into [`LrTables`](crate::lr::tables::LrTables) and reports every conflict as a diagnostic
//! that names the shortest path to its state ([`paths`](crate::lr::paths)).
//! Generation runs it for every grammar. Shift/reduce conflicts the token grammar's
//! `precedence` declaration covers ([`declared_precedence`](crate::lr::declared_precedence))
//! are settled by [`PrecedenceResolver`](crate::lr::precedence::PrecedenceResolver) through the
//! [`ConflictResolver`](crate::lr::automaton::ConflictResolver) seam. A shared helper whose
//! merged lookaheads make a reduce/reduce conflict is copied per use-site class and the
//! automaton rebuilt ([`helpers`](crate::lr::helpers)).

/// LR(0) item sets, DeRemer–Pennello lookaheads and conflict detection.
pub mod automaton;
/// Rule precedence from the `precedence` declaration, bison's token-precedence facility.
pub mod declared_precedence;
/// The LR grammar model the lowering produces and the automaton consumes.
pub mod grammar;
/// Per-site copies of shared helper nonterminals.
pub mod helpers;
/// Lookahead-filter substitution over lowered rules.
pub mod lookahead;
/// Lowering from the normalized grammar to LR rules.
pub mod lowering;
/// Shortest symbol paths from the start states, for diagnostics and the dump.
pub mod paths;
/// Declared precedence, the conflict resolver of the Recursa parser.
pub mod precedence;
/// Bison-style automaton dump and conflict diagnostics.
pub mod report;
/// Deterministic structural statistics for generated LR machines.
pub mod statistics;
/// Comb-vector table packing and its deterministic encoding.
pub mod tables;

#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod textbook;

pub use automaton::{
    Action, Automaton, AutomatonOptions, Conflict, ConflictKind, ConflictResolver,
    EndMarkerResolution, Item, NoPrecedence, Resolution, ResolvedConflict,
    ScopedConflictResolution, State, StateId, TerminalSet,
};
pub use grammar::{
    Associativity, DeclaredPrecedence, ElementValue, LrGrammar, Nonterminal, NonterminalId,
    NonterminalOrigin, OptionalContent, Rule, RuleAction, RuleId, RulePrecedence, SiteForm,
    StartRule, Symbol, TerminalFilter, TerminalId,
};
pub use helpers::{CopiedBuild, HelperSite, HelperSplit, SiteClassCopy, build_with_helper_copies};
pub use lowering::{TypeRoles, lower};
pub use paths::{SymbolPath, render_path, shortest_paths};
pub use precedence::PrecedenceResolver;
pub use report::{LrDiagnostic, automaton_dump, conflict_diagnostics};
pub use statistics::{
    ContentTokenStatistics, KernelMixStatistics, LrOrigin, LrStatistics, NormalizedShapeStatistics,
    OriginStatistics, RuleActionStatistics, StartStatistics, StateStatistics, TableStatistics,
    TypeRoleStatistics,
};
pub use tables::{EncodedLrTables, LrTables, PACT_NINF, TABLE_NINF};

use std::path::Path;

use crate::GenerateError;
use crate::materialization::LoweredGrammar;
use crate::profile::Span;

#[derive(Clone, Debug)]
/// The complete LR analysis of one grammar: rules, automaton, packed tables and conflict
/// diagnostics, which are hard build errors when non-empty.
pub struct LrAnalysis {
    /// The lowered LR grammar.
    pub grammar: LrGrammar,
    /// The LALR(1) automaton.
    pub automaton: Automaton,
    /// The packed tables.
    pub tables: LrTables,
    /// One diagnostic per conflict, in (state, lookahead) order.
    pub diagnostics: Vec<LrDiagnostic>,
    /// Deterministic structural statistics for this analysis.
    pub statistics: LrStatistics,
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
    /// Runs the whole analysis over a lowered grammar, resolving shift/reduce conflicts by
    /// declared precedence; the error is every `RCA0403` the lowering reports.
    pub(crate) fn from_lowered(
        lowered: &LoweredGrammar,
        options: AutomatonOptions,
    ) -> Result<Self, Vec<LrDiagnostic>> {
        let _profile = Span::enter("lr.analysis");
        let overrides = capture_override_diagnostics(lowered);
        if !overrides.is_empty() {
            return Err(overrides);
        }
        let roles = TypeRoles::from_lowered(lowered);
        let grammar = {
            let _stage = Span::enter("lowering");
            lower(&lowered.grammar, &roles)?
        };
        let unclassified = AutomatonOptions {
            check_merge_artefacts: false,
            ..options
        };
        let CopiedBuild {
            grammar,
            mut automaton,
            ..
        } = build_with_helper_copies(grammar, PrecedenceResolver::new, unclassified)?;
        let scoped_diagnostics = {
            let _stage = Span::enter("scoped-conflict-resolutions");
            automaton
                .apply_scoped_resolutions(&grammar, &lowered.grammar.lr_conflict_resolutions)
                .err()
                .into_iter()
                .flatten()
                .map(|message| LrDiagnostic {
                    code: report::INVALID_SCOPED_RESOLUTION,
                    message,
                    path: String::new(),
                    items: Vec::new(),
                    actions: Vec::new(),
                    notes: Vec::new(),
                })
                .collect::<Vec<_>>()
        };
        automaton.classify_merge_artefacts(&grammar, options);
        let tables = {
            let _stage = Span::enter("table-packing");
            LrTables::pack(&grammar, &automaton)
        };
        let statistics = LrStatistics::collect(lowered, &roles, &grammar, &automaton, &tables);
        let mut diagnostics = scoped_diagnostics;
        {
            let _stage = Span::enter("conflict-diagnostics");
            diagnostics.extend(conflict_diagnostics(&grammar, &automaton));
        }
        Ok(Self {
            grammar,
            automaton,
            tables,
            diagnostics,
            statistics,
        })
    }

    /// Runs source analysis over a Rust 2024 grammar package and then the whole LR analysis, with
    /// declared precedence.
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

    /// Returns deterministic structural statistics for the normalized grammar and generated
    /// LR machine.
    pub fn statistics(&self) -> &LrStatistics {
        &self.statistics
    }
}

/// Reports `RCA0406` for every node, variant or field that overrides the capture policy.
///
/// The LR fold resolves an inherited policy from the entry type's context. An override on a node,
/// variant or field would reach its children through the consuming site, which a child rule is
/// reduced before knowing, so such overrides are rejected.
fn capture_override_diagnostics(lowered: &LoweredGrammar) -> Vec<LrDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut report = |what: String| {
        diagnostics.push(LrDiagnostic {
            code: report::CAPTURE_OVERRIDE,
            message: format!(
                "{what} overrides the capture policy with `#[spanned(...)]`; the Recursa parser \
                 resolves capture policies from the entry type only"
            ),
            path: String::new(),
            items: Vec::new(),
            actions: Vec::new(),
            notes: Vec::new(),
        });
    };
    for node in &lowered.source_modeled.resolved().nodes {
        if node.capture.is_some() {
            report(format!("node `{}`", node.ident));
        }
        if let crate::resolution::ResolvedNodeBody::Enum { variants } = &node.body {
            for variant in variants.iter().filter(|variant| variant.capture.is_some()) {
                report(format!("variant `{}::{}`", node.ident, variant.ident));
            }
        }
        for field in node.fields.iter().filter(|field| field.capture.is_some()) {
            report(format!(
                "field `{}` of node `{}`",
                field
                    .syntax
                    .ident
                    .as_ref()
                    .map_or_else(|| "?".to_owned(), ToString::to_string),
                node.ident
            ));
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    //! The diagnostic-producing tests the build diagnostic registry's evidence ledger names.

    use super::automaton::{Action, Automaton, AutomatonOptions, ConflictKind, NoPrecedence};
    use super::lowering::{TypeRoles, lower};
    use super::report::conflict_diagnostics;
    use super::textbook::{dangling_else_grammar, lalr_merge_grammar};
    use crate::{
        AtomDescription, AtomExpression, Declaration, GrammarDescription, KindDescription, KindId,
        Location, SourceId, TextEquivalence, TextPredicate, TypeDescription, TypeShape,
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
        let paths: Vec<&str> = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.path.as_str())
            .collect();
        assert_eq!(paths, ["S -> a c . d", "S -> a c . e"]);
        let dump = super::automaton_dump(&grammar, &automaton);
        assert!(
            dump.contains("State 4\n\n    path: S -> a c .\n\n"),
            "{dump}"
        );
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
            assert!(
                diagnostic.notes[0].starts_with("RCA0405: "),
                "{}",
                diagnostic.notes[0]
            );
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
        assert!(
            unchecked
                .conflicts()
                .iter()
                .all(|conflict| !conflict.merge_artefact)
        );
        assert!(matches!(
            unchecked.conflicts()[0].kind,
            ConflictKind::ReduceReduce { .. }
        ));
        assert!(matches!(
            unchecked.conflicts()[0].resolution,
            Action::Reduce(_)
        ));
    }

    /// A normalized grammar with one token type refined by an exact-text predicate.
    fn grammar_with_text_predicate() -> GrammarDescription {
        let mut grammar = GrammarDescription::new("predicated", 1, TextEquivalence::Sensitive);
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
    fn capture_overrides_are_rejected_with_rca0406() {
        // A grammar whose only departure from the defaults is one `#[spanned(...)]` per level.
        let package = tempfile::tempdir().unwrap();
        std::fs::write(
            package.path().join("grammar.rs"),
            r#"
recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        LET => "let",
    }
    punctuation {
        SEMI => ";",
    }
}

recursa::ast_node! {

    pub enum Statement {
        #[spanned(optional)]
        Let(Let),
    }
}

recursa::ast_node! {

    #[spanned(off)]
    #[tok(LET, this, SEMI)]
    pub struct Let {
        #[spanned(mandatory)]
        pub name: Name,
    }
}

recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}
"#,
        )
        .unwrap();
        let lowered = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect("the grammar passes source analysis")
        .analyzed
        .lowered;
        let error =
            super::LrAnalysis::from_lowered(&lowered, AutomatonOptions::default()).unwrap_err();
        let messages: Vec<String> = error
            .iter()
            .map(|diagnostic| {
                assert_eq!(diagnostic.code, "RCA0406");
                diagnostic.message.clone()
            })
            .collect();
        assert_eq!(
            messages,
            [
                "variant `Statement::Let` overrides the capture policy with `#[spanned(...)]`; the \
                 Recursa parser resolves capture policies from the entry type only",
                "node `Let` overrides the capture policy with `#[spanned(...)]`; the Recursa \
                 parser resolves capture policies from the entry type only",
                "field `name` of node `Let` overrides the capture policy with `#[spanned(...)]`; \
                 the Recursa parser resolves capture policies from the entry type only",
            ]
        );
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
             supported by the Recursa parser"
        );
    }
}
