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
//! `table_driven` style (`generation::table_driven`). Shift/reduce conflicts between Pratt rules
//! and operator tokens are settled by
//! [`BindingPowerPrecedence`](crate::lr::precedence::BindingPowerPrecedence) through the
//! [`ConflictResolver`](crate::lr::automaton::ConflictResolver) seam.

/// LR(0) item sets, DeRemer–Pennello lookaheads and conflict detection.
pub mod automaton;
/// The LR grammar model the lowering produces and the automaton consumes.
pub mod grammar;
/// Lookahead-filter substitution over lowered rules.
pub mod lookahead;
/// Lowering from the normalized grammar to LR rules.
pub mod lowering;
/// Shortest symbol paths from the start states, for diagnostics and the dump.
pub mod paths;
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
    RuleId, RulePrecedence, StartRule, Symbol, TerminalFilter, TerminalId,
    UnrepresentableBindingPowers,
};
pub use lowering::{TypeRoles, lower};
pub use paths::{SymbolPath, render_path, shortest_paths};
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

    /// Appends a fixed token type named `name` over one new kind named `kind`.
    fn push_token(
        grammar: &mut GrammarDescription,
        location: crate::LocationId,
        name: &str,
        kind: &str,
    ) -> TypeId {
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
    }

    /// A normalized Pratt grammar: `Expr` with a `Number` atom and an infix `Expr::Add` over
    /// `PLUS` with the given binding powers.
    fn pratt_grammar(lbp: u8, rbp: u8) -> GrammarDescription {
        let mut grammar = GrammarDescription::new("pratt", 4, 1, TextEquivalence::Sensitive);
        let location = grammar.push_location(Location::new(SourceId::new(0), 0..1));
        let number = push_token(&mut grammar, location, "Number", "NUMBER");
        let plus = push_token(&mut grammar, location, "Plus", "PLUS");
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

    /// A normalized `Pair` sequence over `Expr` with three sites carrying the given exclusion
    /// sets, appended to the Pratt grammar; returns the grammar and the sites.
    fn pratt_grammar_with_exclusion_sites(
        excluded: [&[u32]; 3],
    ) -> (GrammarDescription, TypeRoles) {
        let mut grammar = pratt_grammar(10, 11);
        let expr = TypeId::new(2);
        let add_declaration = crate::DeclId::new(4);
        assert_eq!(grammar.declarations[add_declaration.index()].name, "Expr::Add");
        let location = grammar.locations.len() - 1;
        let location = crate::LocationId::new(location as u32);
        let pair_declaration = grammar.push_declaration(Declaration::new("Pair", location));
        let pair = TypeId::new(grammar.types.len() as u32);
        let mut roles = TypeRoles::default();
        let sites: Vec<_> = excluded
            .iter()
            .map(|excluded| {
                let site = grammar.push_site(ParseSite::new(
                    pair,
                    expr,
                    pair_declaration,
                    ContainerShape::Direct,
                ));
                if !excluded.is_empty() {
                    roles.pratt_exclusions.insert(
                        site,
                        crate::lr::lowering::PrattRestriction {
                            type_id: expr,
                            excluded: excluded.iter().map(|id| crate::DeclId::new(*id)).collect(),
                            min_bp: 0,
                        },
                    );
                }
                site
            })
            .collect();
        grammar.push_type(TypeDescription::new(
            "Pair",
            pair_declaration,
            TypeShape::Sequence { sites },
        ));
        (grammar, roles)
    }

    #[test]
    fn sites_with_equal_exclusion_sets_share_one_clone() {
        let (grammar, roles) = pratt_grammar_with_exclusion_sites([&[4], &[4], &[]]);
        let lowered = lower(&grammar, &roles).expect("the exclusion sites lower");
        let names: Vec<&str> = lowered
            .nonterminals()
            .iter()
            .map(|nonterminal| nonterminal.name.as_str())
            .collect();
        assert_eq!(names, ["Expr", "Pair", "Expr[excl Add]"]);
        let pair = lowered.nonterminal_by_name("Pair").unwrap();
        assert_eq!(
            lowered.render_rule(lowered.rules_of(pair)[0]),
            "Pair -> Expr[excl Add] Expr[excl Add] Expr"
        );
        let clone = lowered.nonterminal_by_name("Expr[excl Add]").unwrap();
        let rules: Vec<String> = lowered
            .rules_of(clone)
            .iter()
            .map(|rule| lowered.render_rule(*rule))
            .collect();
        assert_eq!(rules, ["Expr[excl Add] -> Number"]);
    }

    /// The Pratt grammar plus a postfix `Expr::Index` whose operand follows `[` in a fixed-token
    /// attachment, closed by `]` when `closing`, with minimum binding power `min_bp`, the way a
    /// postfix tail operand inherits its `bp`.
    fn pratt_grammar_with_index_operand(min_bp: u8, closing: bool) -> (GrammarDescription, TypeRoles) {
        let mut grammar = pratt_grammar(10, 11);
        let expr = TypeId::new(2);
        let location = crate::LocationId::new(0);
        let open = push_token(&mut grammar, location, "Open", "LBRACKET");
        let close = push_token(&mut grammar, location, "Close", "RBRACKET");
        let index_declaration =
            grammar.push_declaration(Declaration::new("Expr::Index", location));
        let unit = TypeId::new(grammar.types.len() as u32);
        let mut sites = vec![
            grammar.push_site(ParseSite::new(unit, open, index_declaration, ContainerShape::Direct)),
            grammar.push_site(
                ParseSite::new(unit, expr, index_declaration, ContainerShape::Direct)
                    .with_min_bp(min_bp),
            ),
        ];
        if closing {
            sites.push(grammar.push_site(ParseSite::new(
                unit,
                close,
                index_declaration,
                ContainerShape::Direct,
            )));
        }
        grammar.push_type(TypeDescription::new(
            "Expr::__recursa_attachment",
            index_declaration,
            TypeShape::Sequence { sites },
        ));
        let operand = grammar.push_site(ParseSite::new(
            expr,
            unit,
            index_declaration,
            ContainerShape::Direct,
        ));
        let TypeShape::Pratt { variants } = &mut grammar.types[expr.index()].shape else {
            panic!("Expr is the Pratt type");
        };
        variants.push(PrattVariant {
            declaration: index_declaration,
            position: PrattPosition::Postfix { bp: min_bp },
            sites: vec![operand],
        });
        let mut roles = TypeRoles::default();
        roles.attachment_units.insert(unit);
        (grammar, roles)
    }

    /// The Pratt grammar plus a postfix `Expr::Between` written with bare operands:
    /// `Expr -> Expr BETWEEN Expr AND Expr`, both operands at minimum binding power `min_bp`.
    fn pratt_grammar_with_bare_between(min_bp: u8) -> GrammarDescription {
        let mut grammar = pratt_grammar(10, 11);
        let expr = TypeId::new(2);
        let location = crate::LocationId::new(0);
        let between = push_token(&mut grammar, location, "Between", "BETWEEN");
        let and = push_token(&mut grammar, location, "And", "AND");
        let declaration = grammar.push_declaration(Declaration::new("Expr::Between", location));
        let sites = vec![
            grammar.push_site(ParseSite::new(expr, between, declaration, ContainerShape::Direct)),
            grammar.push_site(
                ParseSite::new(expr, expr, declaration, ContainerShape::Direct).with_min_bp(min_bp),
            ),
            grammar.push_site(ParseSite::new(expr, and, declaration, ContainerShape::Direct)),
            grammar.push_site(
                ParseSite::new(expr, expr, declaration, ContainerShape::Direct).with_min_bp(min_bp),
            ),
        ];
        let TypeShape::Pratt { variants } = &mut grammar.types[expr.index()].shape else {
            panic!("Expr is the Pratt type");
        };
        variants.push(PrattVariant {
            declaration,
            position: PrattPosition::Postfix { bp: min_bp },
            sites,
        });
        grammar
    }

    /// Renders every rule of the nonterminal named `name`.
    fn rules_named(grammar: &crate::lr::LrGrammar, name: &str) -> Vec<String> {
        let nonterminal = grammar
            .nonterminal_by_name(name)
            .unwrap_or_else(|| panic!("no nonterminal named {name}"));
        grammar
            .rules_of(nonterminal)
            .iter()
            .map(|rule| grammar.render_rule(*rule))
            .collect()
    }

    #[test]
    fn a_bare_operand_followed_by_more_syntax_lowers_to_its_minimum_binding_power_clone() {
        // `Expr -> Expr BETWEEN Expr AND Expr`: the middle operand is parsed with `parse_bp(6)`,
        // so it lowers to `Expr[min 6]`, the type minus every LED variant below 6; the last
        // operand is bare, so the rule's own precedence decides where it stops.
        let grammar = lower(&pratt_grammar_with_bare_between(6), &TypeRoles::default())
            .expect("a bare interleaved operand lowers");
        assert_eq!(
            rules_named(&grammar, "Expr"),
            [
                "Expr -> Number",
                "Expr -> Expr Plus Expr",
                "Expr -> Expr Between Expr[min 6] And Expr",
            ]
        );
        assert_eq!(
            rules_named(&grammar, "Expr[min 6]"),
            [
                "Expr[min 6] -> Number",
                "Expr[min 6] -> Expr[min 6] Plus Expr",
                "Expr[min 6] -> Expr[min 6] Between Expr[min 6] And Expr",
            ]
        );
        let clone = grammar.nonterminal_by_name("Expr[min 6]").unwrap();
        assert_eq!(
            grammar.nonterminal(clone).origin,
            crate::lr::NonterminalOrigin::Restriction {
                type_id: TypeId::new(2),
                excluded: Vec::new(),
                min_bp: 6,
            }
        );
        // The clone's rules compare their precedence with the base type's tokens.
        let expr = grammar.nonterminal_by_name("Expr").unwrap();
        for rule in grammar.rules_of(clone) {
            if let Some(precedence) = grammar.rule(*rule).precedence {
                assert_eq!(precedence.owner, expr);
            }
        }
        // A minimum of 20 keeps the atom and `between` itself (its bp is the minimum), not `+`.
        let grammar = lower(&pratt_grammar_with_bare_between(20), &TypeRoles::default()).unwrap();
        assert_eq!(
            rules_named(&grammar, "Expr[min 20]"),
            [
                "Expr[min 20] -> Number",
                "Expr[min 20] -> Expr[min 20] Between Expr[min 20] And Expr",
            ]
        );
        // Without a minimum both operands are the full language in both styles.
        let grammar = lower(&pratt_grammar_with_bare_between(0), &TypeRoles::default()).unwrap();
        assert_eq!(
            grammar.render_rule(crate::lr::RuleId::new(2)),
            "Expr -> Expr Between Expr And Expr"
        );
        assert!(grammar.nonterminal_by_name("Expr[min 0]").is_none());
    }

    #[test]
    fn a_delimited_operand_lowers_to_its_minimum_binding_power_clone_inside_the_attachment() {
        // `a[b]` at postfix bp 50: recursive descent parses `b` with `parse_bp(50)`, so only
        // the postfix operators at 50 or above may extend it; the attachment's operand is the
        // same clone, and the attachment inherits the variant's precedence for its own reduce.
        let (grammar, roles) = pratt_grammar_with_index_operand(50, true);
        let lowered = lower(&grammar, &roles).expect("a closed delimited operand lowers");
        assert_eq!(
            rules_named(&lowered, "Expr::__recursa_attachment"),
            ["Expr::__recursa_attachment -> Open Expr[min 50] Close"]
        );
        assert_eq!(
            rules_named(&lowered, "Expr[min 50]"),
            [
                "Expr[min 50] -> Number",
                "Expr[min 50] -> Expr[min 50] Expr::__recursa_attachment",
            ]
        );
        let unit = lowered.nonterminal_by_name("Expr::__recursa_attachment").unwrap();
        let rule = lowered.rule(lowered.rules_of(unit)[0]);
        assert_eq!(
            rule.precedence.map(|p| (p.level, p.owner)),
            Some((50, lowered.nonterminal_by_name("Expr").unwrap()))
        );
        // An open attachment (`[` before the operand, nothing after) lowers the same way.
        let (grammar, roles) = pratt_grammar_with_index_operand(50, false);
        let lowered = lower(&grammar, &roles).expect("an open attachment lowers");
        assert_eq!(
            rules_named(&lowered, "Expr::__recursa_attachment"),
            ["Expr::__recursa_attachment -> Open Expr[min 50]"]
        );
        // Without a minimum the delimited operand is the full language in both styles.
        let (grammar, roles) = pratt_grammar_with_index_operand(0, true);
        let lowered = lower(&grammar, &roles).expect("an unrestricted delimited operand lowers");
        assert_eq!(
            rules_named(&lowered, "Expr::__recursa_attachment"),
            ["Expr::__recursa_attachment -> Open Expr Close"]
        );
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
