//! Parse-plan construction, validation, dispatch, Pratt, container, and FOLLOW rendering.
//!
//! This module keeps source-aware construction beside executable parser emission. Frozen
//! semantic decisions are consumed here without recomputing analysis during rendering.

use super::*;

#[cfg(test)]
std::thread_local! {
    /// Structural emission-lookup work isolated to the current test thread.
    static EMISSION_LOOKUP_INSTRUMENTATION: std::cell::Cell<EmissionLookupInstrumentation> =
        const { std::cell::Cell::new(EmissionLookupInstrumentation::EMPTY) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
/// Structural work performed while joining frozen decisions to one emission plan.
pub(super) struct EmissionLookupInstrumentation {
    /// Candidate dispatch records inspected by linear lookups.
    pub(super) dispatch_candidates: usize,
    /// Candidate failure-edge records inspected by linear lookups.
    pub(super) failure_edge_candidates: usize,
    /// Candidate container-machine records inspected by linear lookups.
    pub(super) container_machine_candidates: usize,
    /// Complete Pratt extender ExpectedSets constructed.
    pub(super) pratt_extender_builds: usize,
    /// Pratt branches inspected while constructing extender ExpectedSets.
    pub(super) pratt_extender_branch_candidates: usize,
}

#[cfg(test)]
impl EmissionLookupInstrumentation {
    /// Empty structural-work snapshot.
    const EMPTY: Self = Self {
        dispatch_candidates: 0,
        failure_edge_candidates: 0,
        container_machine_candidates: 0,
        pratt_extender_builds: 0,
        pratt_extender_branch_candidates: 0,
    };
}

/// Resets emission lookup instrumentation on the current test thread.
#[cfg(test)]
pub(super) fn reset_emission_lookup_instrumentation() {
    EMISSION_LOOKUP_INSTRUMENTATION.set(EmissionLookupInstrumentation::EMPTY);
}

/// Returns emission lookup instrumentation on the current test thread.
#[cfg(test)]
pub(super) fn emission_lookup_instrumentation() -> EmissionLookupInstrumentation {
    EMISSION_LOOKUP_INSTRUMENTATION.get()
}

#[cfg(test)]
/// Adds structural work to the current test thread's emission snapshot.
fn observe_emission_lookup_work(update: impl FnOnce(&mut EmissionLookupInstrumentation)) {
    EMISSION_LOOKUP_INSTRUMENTATION.with(|counter| {
        let mut instrumentation = counter.get();
        update(&mut instrumentation);
        counter.set(instrumentation);
    });
}

/// Finds one dispatch while recording aggregate candidate work for focused tests.
#[inline]
fn find_dispatch(
    candidates: &[crate::DispatchPlan],
    mut predicate: impl FnMut(&crate::DispatchPlan) -> bool,
) -> Option<&crate::DispatchPlan> {
    #[cfg(test)]
    let mut inspected = 0;
    let result = candidates.iter().find(|candidate| {
        #[cfg(test)]
        {
            inspected += 1;
        }
        predicate(candidate)
    });
    #[cfg(test)]
    observe_emission_lookup_work(|work| work.dispatch_candidates += inspected);
    result
}

/// Dense, generation-local access to frozen semantic decisions and derived Pratt expectations.
pub(super) struct EmissionLookupIndex<'a> {
    /// Authoritative frozen decisions retained for dispatch traversal.
    decisions: &'a crate::DecisionProducts,
    /// One container-machine slot per dense parse-site identity.
    containers_by_site: Vec<Option<&'a crate::ContainerMachine>>,
    /// Owner-local failure edges indexed by failure kind.
    failures_by_owner: Vec<[Option<&'a crate::FailureEdge>; 8]>,
    /// Site-local failure edges indexed by failure kind.
    failures_by_site: Vec<[Option<&'a crate::FailureEdge>; 8]>,
    /// Grammar-scoped mapping from token kinds to emitted expectation atoms.
    atom_index: &'a ExpectedAtomIndex,
    /// Runtime token-kind width shared by every emitted expectation.
    kind_count: usize,
    /// Active and inactive extender sets memoized by semantic owner and binding threshold.
    pratt_thresholds: std::cell::RefCell<BTreeMap<(crate::TypeId, u8), (ExpectedSet, ExpectedSet)>>,
    /// Complete extender sets memoized by semantic owner.
    pratt_all: std::cell::RefCell<BTreeMap<crate::TypeId, ExpectedSet>>,
    /// Accepted Pratt extender kinds per dense parse-site ID, empty without any acceptance.
    greedy_by_site: Vec<ExpectedSet>,
}

impl<'a> EmissionLookupIndex<'a> {
    /// Indexes one validated decision snapshot without changing its canonical order or identity.
    pub(super) fn new(
        decisions: &'a crate::DecisionProducts,
        atom_index: &'a ExpectedAtomIndex,
        type_count: usize,
        site_count: usize,
        kind_count: usize,
    ) -> Self {
        let mut containers_by_site = vec![None; site_count];
        for machine in decisions.container_machines() {
            if let Some(slot) = containers_by_site.get_mut(machine.site().index())
                && slot.is_none()
            {
                *slot = Some(machine);
            }
        }
        let mut failures_by_owner = vec![[None; 8]; type_count];
        let mut failures_by_site = vec![[None; 8]; site_count];
        for edge in decisions.failure_edges() {
            let slots = edge.site().map_or_else(
                || failures_by_owner.get_mut(edge.owner().index()),
                |site| failures_by_site.get_mut(site.index()),
            );
            if let Some(slot) =
                slots.and_then(|slots| slots.get_mut(failure_kind_index(edge.kind())))
                && slot.is_none()
            {
                *slot = Some(edge);
            }
        }
        let greedy_by_site = decisions
            .greedy_pratt_boundary_by_site()
            .iter()
            .map(|kinds| ExpectedSet::from_kinds(kinds, kind_count))
            .collect();
        Self {
            decisions,
            containers_by_site,
            failures_by_owner,
            failures_by_site,
            atom_index,
            kind_count,
            pratt_thresholds: std::cell::RefCell::new(BTreeMap::new()),
            pratt_all: std::cell::RefCell::new(BTreeMap::new()),
            greedy_by_site,
        }
    }

    /// Returns the boundary kinds a `#[greedy(...)]` acceptance lets a Pratt loop extend on.
    pub(super) fn greedy_acceptance(&self, site: crate::SiteId) -> Option<&ExpectedSet> {
        self.greedy_by_site
            .get(site.index())
            .filter(|accepted| !accepted.is_empty())
    }

    /// Selects one dispatch through the unchanged canonical linear route.
    fn dispatch(
        &self,
        owner: crate::TypeId,
        kind: DispatchKind,
    ) -> Option<&'a crate::DispatchPlan> {
        find_dispatch(self.decisions.dispatches(), |dispatch| {
            dispatch.owner() == owner && dispatch.kind() == kind
        })
    }

    /// Selects one container machine by its dense semantic site identity.
    fn container(&self, site: SiteId) -> Option<&'a crate::ContainerMachine> {
        let indexed = self.containers_by_site.get(site.index()).copied().flatten();
        #[cfg(test)]
        assert_eq!(
            indexed,
            self.decisions
                .container_machines()
                .iter()
                .find(|machine| machine.site() == site),
            "dense container lookup diverged from the canonical linear lookup",
        );
        indexed
    }

    /// Selects one failure edge attached to a semantic owner.
    fn owner_failure(
        &self,
        owner: crate::TypeId,
        kind: FailureEdgeKind,
    ) -> Option<&'a crate::FailureEdge> {
        let indexed = self
            .failures_by_owner
            .get(owner.index())
            .and_then(|slots| slots[failure_kind_index(kind)]);
        #[cfg(test)]
        assert_eq!(
            indexed,
            self.decisions.failure_edges().iter().find(|edge| {
                edge.owner() == owner && edge.site().is_none() && edge.kind() == kind
            }),
            "dense owner failure lookup diverged from the canonical linear lookup",
        );
        indexed
    }

    /// Selects one failure edge attached to a dense parse-site identity.
    fn site_failure(&self, site: SiteId, kind: FailureEdgeKind) -> Option<&'a crate::FailureEdge> {
        let indexed = self
            .failures_by_site
            .get(site.index())
            .and_then(|slots| slots[failure_kind_index(kind)]);
        #[cfg(test)]
        assert_eq!(
            indexed,
            self.decisions
                .failure_edges()
                .iter()
                .find(|edge| edge.site() == Some(site) && edge.kind() == kind),
            "dense site failure lookup diverged from the canonical linear lookup",
        );
        indexed
    }

    /// Returns active and inactive Pratt extenders for one binding threshold.
    fn pratt_extenders_at_min_bp(
        &self,
        owner: crate::TypeId,
        min_bp: u8,
    ) -> Result<(ExpectedSet, ExpectedSet), String> {
        if let Some(cached) = self
            .pratt_thresholds
            .borrow()
            .get(&(owner, min_bp))
            .cloned()
        {
            #[cfg(test)]
            {
                let active = build_pratt_extenders(
                    self,
                    owner,
                    |position| pratt_led_lbp(position) >= min_bp,
                    false,
                )?;
                let inactive = build_pratt_extenders(
                    self,
                    owner,
                    |position| pratt_led_lbp(position) < min_bp,
                    false,
                )?;
                assert_eq!(cached, (active, inactive), "cached Pratt threshold drifted");
            }
            return Ok(cached);
        }
        let active = build_pratt_extenders(
            self,
            owner,
            |position| pratt_led_lbp(position) >= min_bp,
            true,
        )?;
        let inactive = build_pratt_extenders(
            self,
            owner,
            |position| pratt_led_lbp(position) < min_bp,
            true,
        )?;
        self.pratt_thresholds
            .borrow_mut()
            .insert((owner, min_bp), (active.clone(), inactive.clone()));
        Ok((active, inactive))
    }

    /// Returns every Pratt extender belonging to one semantic owner.
    fn pratt_extenders(&self, owner: crate::TypeId) -> Result<ExpectedSet, String> {
        if let Some(cached) = self.pratt_all.borrow().get(&owner).cloned() {
            #[cfg(test)]
            assert_eq!(
                cached,
                build_pratt_extenders(self, owner, |_| true, false)?,
                "cached complete Pratt extender family drifted",
            );
            return Ok(cached);
        }
        let expected = build_pratt_extenders(self, owner, |_| true, true)?;
        self.pratt_all.borrow_mut().insert(owner, expected.clone());
        Ok(expected)
    }
}

/// Returns the stable dense slot assigned to one failure-edge kind.
const fn failure_kind_index(kind: FailureEdgeKind) -> usize {
    match kind {
        FailureEdgeKind::RequiredSite => 0,
        FailureEdgeKind::EnumDispatch => 1,
        FailureEdgeKind::PrattNudDispatch => 2,
        FailureEdgeKind::PrattLedDispatch => 3,
        FailureEdgeKind::PrattCallerFollowGuard => 4,
        FailureEdgeKind::ContainerMinimum => 5,
        FailureEdgeKind::ContainerContinuation => 6,
        FailureEdgeKind::ContainerAfterSeparator => 7,
    }
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    use super::{
        EmissionLookupIndex, ExpectedAtomIndex, ExpectedSet,
        OptionalViabilityDecisionTerminalParsePlan, PlannedContainer, PlannedFollow,
        PlannedTextPredicate, TextEquivalence, encode_dispatch_positions, planned_container,
        pratt_operand_follow,
    };
    use crate::{
        AtomDescription, AtomExpression, ContainerShape, Declaration, GrammarDescription,
        KindDescription, Location, OptionalAction, ParseSite, SourceId, TextPredicate,
        TypeDescription, TypeShape,
    };
    macro_rules! focused_case {
        ($name:ident) => {
            #[test]
            fn $name() {
                test_support::$name();
            }
        };
    }
    focused_case!(predictive_compilation_rejects_missing_and_conflicting_frozen_products);
    focused_case!(large_predictive_selection_emits_each_position_matcher_once);
    focused_case!(pratt_branches_are_typechecked_in_isolated_helpers);
    focused_case!(predictive_variants_are_typechecked_in_isolated_helpers);
    focused_case!(large_ordinary_predictive_dispatch_uses_compact_tables);
    focused_case!(single_digit_packed_dispatch_is_clippy_clean);
    focused_case!(large_expected_set_batch_uses_canonical_atom_index);
    focused_case!(grammar_wide_expected_set_pool_is_canonical);
    focused_case!(stale_grammar_wide_expected_set_pool_fails_before_rendering);
    focused_case!(balanced_root_failure_belongs_to_grammar_wide_expected_set_pool);
    focused_case!(overlapping_rectangles_preserve_full_ordinary_routes);
    focused_case!(corrupted_joined_dispatch_fails_before_publication_with_rca9105);
    focused_case!(corrupted_pratt_branch_follow_and_failure_identities_fail_before_publication);
    focused_case!(corrupted_real_plan_family_content_fails_before_publication);

    #[test]
    fn pratt_operand_follow_removes_active_static_extenders_before_adding_stops() {
        let follow = PlannedFollow::StaticAndCaller(ExpectedSet {
            words: vec![0b1111],
            atoms: vec![0, 1, 2, 3],
        });
        let active = ExpectedSet {
            words: vec![0b0101],
            atoms: vec![0, 2],
        };
        let inactive = ExpectedSet {
            words: vec![0b1_0001],
            atoms: vec![0, 4],
        };

        assert_eq!(
            pratt_operand_follow(&follow, &active, &inactive),
            PlannedFollow::StaticAndCaller(ExpectedSet {
                words: vec![0b1_1010],
                atoms: vec![1, 3, 4],
            }),
        );
    }

    #[test]
    fn pratt_operand_stops_preserve_an_inherited_authored_boundary() {
        let active = ExpectedSet {
            words: vec![0b0101],
            atoms: vec![0, 2],
        };
        let inactive = ExpectedSet {
            words: vec![0b1_0001],
            atoms: vec![0, 4],
        };

        assert_eq!(
            pratt_operand_follow(&PlannedFollow::Caller, &active, &inactive),
            PlannedFollow::CallerAndPrattStops(ExpectedSet {
                words: vec![0b1_0000],
                atoms: vec![4],
            }),
        );
    }

    #[test]
    fn large_position_dispatch_uses_one_packed_matcher() {
        let positions = (0..1_520)
            .map(|position| {
                let predicate = match position % 4 {
                    0 => None,
                    1 => Some(PlannedTextPredicate::Exact(format!("exact-{position}"))),
                    2 => Some(PlannedTextPredicate::OneOf(vec![
                        format!("one-{position}"),
                        format!("two-{position}"),
                    ])),
                    _ => Some(PlannedTextPredicate::NotIn(vec![format!("not-{position}")])),
                };
                (position, predicate)
            })
            .collect::<Vec<_>>();
        let encoded = encode_dispatch_positions(TextEquivalence::AsciiInsensitive, &positions);
        assert_eq!(encoded.table.len(), positions.len() * 4);
        assert_eq!(
            &encoded.table[..16],
            &[0, 0, 0, 0, 1, 4, 0, 1, 2, 5, 1, 2, 3, 6, 3, 1]
        );
        assert_eq!(
            &encoded.spellings[..4],
            &["exact-1", "one-2", "two-2", "not-3"]
        );

        let (width, literal) = super::packed_table(&encoded.table);
        assert_eq!(width, 2, "position cells above 63 pack into two digits");
        assert!(literal.to_string().starts_with("b\""));
    }

    #[test]
    fn position_metadata_preserves_source_validating_any_predicates() {
        let positions = vec![(0, None), (1, Some(PlannedTextPredicate::Any))];

        let encoded = encode_dispatch_positions(TextEquivalence::Sensitive, &positions);

        assert_eq!(encoded.table, [0, 0, 0, 0, 1, 7, 0, 0]);
        assert!(encoded.spellings.is_empty());
    }

    #[test]
    fn refined_optional_element_retains_exact_selection_before_caller_follow() {
        let mut grammar =
            GrammarDescription::new("refined-caller-optional", 1, 2, TextEquivalence::Sensitive);
        let mut next_byte = 0;
        let mut declaration = |grammar: &mut GrammarDescription, name: &str| {
            let byte = next_byte;
            next_byte += 1;
            let location = grammar.push_location(Location::new(SourceId::new(0), byte..byte + 1));
            grammar.push_declaration(Declaration::new(name, location))
        };
        let kind_declaration = declaration(&mut grammar, "WORD");
        let word = grammar.push_kind(KindDescription::new("WORD", kind_declaration));
        let atom_declaration = declaration(&mut grammar, "WordAtom");
        let atom = grammar.push_atom(AtomDescription::new(
            "WordAtom",
            atom_declaration,
            AtomExpression::Kind(word),
        ));
        let token = |grammar: &mut GrammarDescription,
                     declaration: crate::DeclId,
                     name: &str,
                     spelling: &str| {
            let predicate = grammar.push_text_predicate(TextPredicate::Exact(spelling.to_owned()));
            grammar.push_type(TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: Some(predicate),
                },
            ))
        };
        let open_declaration = declaration(&mut grammar, "OPEN");
        let open = token(&mut grammar, open_declaration, "OPEN", "OPEN");
        let close_declaration = declaration(&mut grammar, "CLOSE");
        let close = token(&mut grammar, close_declaration, "CLOSE", "CLOSE");

        let inner = grammar.next_type_id();
        let inner_declaration = declaration(&mut grammar, "Inner");
        let optional_declaration = declaration(&mut grammar, "Inner::optional");
        let optional = grammar.push_site(ParseSite::new(
            inner,
            open,
            optional_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Inner",
                inner_declaration,
                TypeShape::Sequence {
                    sites: vec![optional],
                },
            )),
            inner,
        );
        let root = grammar.next_type_id();
        let root_declaration = declaration(&mut grammar, "Root");
        let inner_site_declaration = declaration(&mut grammar, "Root::inner");
        let inner_site = grammar.push_site(ParseSite::new(
            root,
            inner,
            inner_site_declaration,
            ContainerShape::Direct,
        ));
        let close_site_declaration = declaration(&mut grammar, "Root::close");
        let close_site = grammar.push_site(ParseSite::new(
            root,
            close,
            close_site_declaration,
            ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(TypeDescription::new(
                "Root",
                root_declaration,
                TypeShape::Sequence {
                    sites: vec![inner_site, close_site],
                },
            )),
            root,
        );
        grammar.entry_types = vec![root];

        let report = crate::analyze(&grammar);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let output = report.output.expect("the refined caller grammar is valid");
        let machine = &output.decisions().container_machines()[optional.index()];
        assert!(matches!(machine.stop(), crate::FollowRecipe::Caller));
        let atoms = ExpectedAtomIndex::from_grammar(&grammar).expect("valid direct atom index");
        let lookups = EmissionLookupIndex::new(
            output.decisions(),
            &atoms,
            grammar.types.len(),
            grammar.sites.len(),
            grammar.kind_count,
        );
        let planned =
            planned_container(machine, &lookups).expect("the refined optional joins to generation");
        let PlannedContainer::Optional {
            decision: Some(decision),
            ..
        } = planned
        else {
            panic!("refined optional selection must not fall back to the kind-only selector");
        };
        let root = &decision.states[decision.root.index()];
        assert!(
            root.terminals
                .contains(&OptionalViabilityDecisionTerminalParsePlan {
                    action: OptionalAction::Skip,
                    on_caller_follow: true,
                },)
        );
        assert_eq!(
            root.edges[0].position.text_predicate,
            Some(PlannedTextPredicate::Exact("OPEN".to_owned())),
        );
    }
}

/// Packed position metadata and its flattened canonical spelling pool.
struct EncodedDispatchPositions {
    /// Four cells per position: expected-set index, predicate mode, spelling start, spelling count.
    table: Vec<usize>,
    /// Canonical spellings referenced by the table ranges.
    spellings: Vec<String>,
}

/// Encodes frozen dispatch positions without rediscovering token or spelling semantics.
fn encode_dispatch_positions(
    text_equivalence: TextEquivalence,
    positions: &[(usize, Option<PlannedTextPredicate>)],
) -> EncodedDispatchPositions {
    let mut table = Vec::with_capacity(positions.len() * 4);
    let mut spellings = Vec::new();
    for (expected_index, predicate) in positions {
        let spelling_start = spellings.len();
        let predicate_mode = match predicate {
            None => 0,
            Some(PlannedTextPredicate::Any) => 7,
            Some(PlannedTextPredicate::Exact(exact)) => {
                spellings.push(exact.clone());
                match text_equivalence {
                    TextEquivalence::Sensitive => 1,
                    TextEquivalence::AsciiInsensitive => 4,
                }
            }
            Some(PlannedTextPredicate::OneOf(options)) => {
                spellings.extend(options.iter().cloned());
                match text_equivalence {
                    TextEquivalence::Sensitive => 2,
                    TextEquivalence::AsciiInsensitive => 5,
                }
            }
            Some(PlannedTextPredicate::NotIn(options)) => {
                spellings.extend(options.iter().cloned());
                match text_equivalence {
                    TextEquivalence::Sensitive => 3,
                    TextEquivalence::AsciiInsensitive => 6,
                }
            }
        };
        table.extend([
            *expected_index,
            predicate_mode,
            spelling_start,
            spellings.len() - spelling_start,
        ]);
    }
    EncodedDispatchPositions { table, spellings }
}

#[derive(Clone, Debug, Hash)]
/// Complete frozen contract for emitting one authored node parser.
pub(super) struct NodeParsePlan {
    /// Stable node identity used to join semantic and generation plans.
    pub(super) node: NodeId,
    /// Canonical schema key published for the node.
    pub(super) semantic_key: String,
    /// Module that owns the emitted parser implementation.
    pub(super) module: ModuleId,
    /// Authored Rust identifier retained for emitted declarations.
    pub(super) ident: Ident,
    /// Whether loader compatibility requires a public materialization.
    pub(super) publicly_materialized: bool,
    /// Stable loader symbol used to discover this node.
    pub(super) loader_anchor: String,
    /// Authored generic parameters preserved by parser emission.
    pub(super) generics: Generics,
    /// Lifetime used for source-borrowing fields in generated types.
    pub(super) input_lifetime: syn::Lifetime,
    /// Frozen structural or Pratt parsing strategy.
    pub(super) body: NodeBodyPlan,
    /// Materialized applications emitted in canonical application order.
    pub(super) parse_applications: Vec<NodeApplicationParsePlan>,
    /// Source-capture policy for the completed node.
    pub(super) capture: PlannedCapturePolicy,
    /// Trait recipes attached to the generated node declaration.
    pub(super) configured_traits: Vec<ConfiguredTraitRecipe>,
    /// Whether Explorer visitation was explicitly configured for this node.
    pub(super) explorer_visit_configured: bool,
    /// Formatting hints shared with Pretty emission.
    pub(super) pretty: PrettyHints,
}

#[derive(Clone, Debug, Hash)]
/// Parser specialization for one materialized node application.
pub(super) struct NodeApplicationParsePlan {
    /// Stable identity of the materialized application.
    pub(super) application: MaterializedApplicationId,
    /// Node family specialized by this application.
    pub(super) node: NodeId,
    /// Canonical parser index assigned during planning.
    pub(super) parse_index: usize,
    /// Concrete generated result type.
    pub(super) ty: Type,
    /// Source-independent form used by static metadata.
    pub(super) static_ty: Type,
    /// Frozen parse body specialized for the application.
    pub(super) body: NodeBodyPlan,
}

#[derive(Clone, Debug, Hash)]
/// Frozen choice of structural, variant-dispatch, or Pratt parsing.
pub(super) enum NodeBodyPlan {
    /// Parses a single field sequence into a struct.
    Struct {
        /// Construction style for named, tuple, or unit fields.
        style: ResolvedFieldStyle,
        /// Elements parsed in authored field order.
        elements: Vec<ElementParsePlan>,
    },
    /// Selects and parses one enum variant.
    Enum {
        /// Variant bodies in authored declaration order.
        variants: Vec<VariantParsePlan>,
        /// Validated predictive selector for those variants.
        dispatch: Box<PredictiveDispatchPlan>,
    },
    /// Parses an expression family using frozen Pratt roles.
    Pratt(Box<PrattParsePlan>),
}

#[derive(Clone, Debug, Hash)]
/// Validated prefix/infix dispatch contract for a Pratt node.
pub(super) struct PrattParsePlan {
    /// Frozen prefix-variant count checked before emission.
    pub(super) expected_nud_count: usize,
    /// Frozen extender-variant count checked before emission.
    pub(super) expected_led_count: usize,
    /// Prefix branches in authored variant order.
    pub(super) nud: Vec<PrattNudParsePlan>,
    /// Extender branches in authored variant order.
    pub(super) led: Vec<PrattLedParsePlan>,
    /// Predictive selector for the initial expression branch.
    pub(super) nud_dispatch: PredictiveDispatchPlan,
    /// Optional selector for expression extenders.
    pub(super) led_dispatch: Option<PredictiveDispatchPlan>,
    /// Expected evidence reported when no prefix branch matches.
    pub(super) nud_failure: PlannedFollow,
    /// Tokens that may legally follow prefix selection.
    pub(super) nud_selection_follow: ExpectedSet,
    /// Optional binding-position guards keyed by caller FOLLOW.
    pub(super) caller_follow_guard: Option<Vec<PrattGuardBranchPlan>>,
}

#[derive(Clone, Debug, Hash)]
/// Prefix branch paired with its authored identity and binding position.
pub(super) struct PrattNudParsePlan {
    /// Authored variant index retained for diagnostics and identity.
    pub(super) source_index: usize,
    /// Frozen prefix variant body.
    pub(super) variant: VariantParsePlan,
    /// Binding-power position assigned by semantic analysis.
    pub(super) position: PrattPosition,
    /// Direct recursive operand associations, empty for atoms.
    pub(super) operands: Vec<PrattOperandParsePlan>,
}

#[derive(Clone, Debug, Hash)]
/// Extender branch paired with its left operand and binding position.
pub(super) struct PrattLedParsePlan {
    /// Authored variant index retained for diagnostics and identity.
    pub(super) source_index: usize,
    /// Frozen extender variant body.
    pub(super) variant: VariantParsePlan,
    /// Element receiving the already-parsed left operand.
    pub(super) lhs: ElementParsePlan,
    /// Binding-power position assigned by semantic analysis.
    pub(super) position: PrattPosition,
    /// Direct recursive tail operand associations, empty without recursion.
    pub(super) operands: Vec<PrattOperandParsePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// One generated recursive Pratt operand and its exact threshold.
pub(super) struct PrattOperandParsePlan {
    /// Index of the associated element in the branch tail.
    pub(super) element_index: usize,
    /// Minimum binding power supplied at that element.
    pub(super) minimum_binding_power: u8,
}

#[derive(Clone, Debug, Hash)]
/// Caller-FOLLOW guard for one Pratt binding position.
pub(super) struct PrattGuardBranchPlan {
    /// Pratt position at which the guard applies.
    pub(super) position: PrattPosition,
    /// Caller-FOLLOW set admitted at that position.
    pub(super) expected: ExpectedSet,
}

#[derive(Clone, Debug, Hash)]
/// Construction and element contract for one resolved enum variant.
pub(super) struct VariantParsePlan {
    /// Stable resolved identity used across semantic products.
    pub(super) resolved_variant: ResolvedVariantId,
    /// Rust identifier used for construction.
    pub(super) ident: Ident,
    /// Named, tuple, or unit construction style.
    pub(super) style: ResolvedFieldStyle,
    /// Parse elements in authored field order.
    pub(super) elements: Vec<ElementParsePlan>,
    /// Optional source-capture policy for the variant.
    pub(super) capture: Option<PlannedCapturePolicy>,
    /// Formatting hints retained for shared projections.
    pub(super) pretty: PrettyHints,
}

#[derive(Clone, Debug, Hash)]
/// Predictive graph plus the semantic identity used to validate it.
pub(super) struct PredictiveDispatchPlan {
    /// Root state of the frozen predictive graph.
    pub(super) root: DispatchStateId,
    /// Canonical predictive states; shared suffixes occur once.
    pub(super) states: Vec<DispatchTrieNodePlan>,
    /// Sequence count used to detect stale semantic products.
    pub(super) frozen_sequence_count: usize,
    /// Trie-node count used to detect incomplete reconstruction.
    pub(super) frozen_node_count: usize,
    /// Deterministic identity of the analyzed dispatch family.
    pub(super) frozen_identity: u64,
    /// Optional delimiter scan replacing one recursively ambiguous opener edge.
    pub(super) balanced: Option<BalancedDispatchPlan>,
}

#[derive(Clone, Debug, Hash)]
/// Generated form of one frozen balanced-delimiter dispatch cut.
pub(super) struct BalancedDispatchPlan {
    /// Opening token kind expected at the current cursor.
    pub(super) open: ExpectedSet,
    /// Closing token kind that ends the scan at nesting depth zero.
    pub(super) close: ExpectedSet,
    /// Root of the residual graph evaluated after the matching close.
    pub(super) after_close_root: DispatchStateId,
    /// Residual graph evaluated after the matching close.
    pub(super) after_close_states: Vec<DispatchTrieNodePlan>,
    /// Frozen residual node count used to reject incomplete joins.
    pub(super) frozen_node_count: usize,
    /// Frozen residual identity used to reject mutated joins.
    pub(super) frozen_identity: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
/// Dense identity of one state in a frozen predictive graph.
pub(super) struct DispatchStateId(usize);

impl DispatchStateId {
    /// Creates an arena identity for focused invariant tests.
    #[cfg(test)]
    pub(super) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Returns the dense arena index.
    pub(super) const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Default, Hash)]
/// One selectable state in the frozen predictive graph.
pub(super) struct DispatchTrieNodePlan {
    /// Selection made when the consumed prefix ends here.
    pub(super) target: Option<DispatchTargetPlan>,
    /// Outgoing positions in canonical predictive order.
    pub(super) edges: Vec<DispatchTrieEdgePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Variant selection reached at a predictive trie state.
pub(super) struct DispatchTargetPlan {
    /// Zero-based branch selected by the trie.
    pub(super) variant: usize,
    /// Closed selection policy retained from semantic analysis.
    pub(super) mode: crate::DispatchTerminalMode,
}

/// Returns the stable packed-table digit for one closed terminal mode.
const fn dispatch_terminal_mode_code(mode: crate::DispatchTerminalMode) -> usize {
    match mode {
        crate::DispatchTerminalMode::Immediate => 0,
        crate::DispatchTerminalMode::CallerFollow => 1,
        crate::DispatchTerminalMode::FallbackAfterEdges => 2,
    }
}

#[derive(Clone, Debug, Hash)]
/// Ordered transition from one predictive trie state to another.
pub(super) struct DispatchTrieEdgePlan {
    /// Token/text condition labeling this trie edge.
    pub(super) position: DispatchPositionPlan,
    /// Graph state reached after the position matches.
    pub(super) next: DispatchStateId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
/// Token expectation and spelling refinement labeling a dispatch edge.
pub(super) struct DispatchPositionPlan {
    /// Token-kind expectation tested at this position.
    pub(super) expected: ExpectedSet,
    /// Optional spelling refinement for atom-like token kinds.
    pub(super) text_predicate: Option<PlannedTextPredicate>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
/// Frozen spelling constraint refining a token-kind match.
pub(super) enum PlannedTextPredicate {
    /// Accepts every spelling admitted by the token kind.
    Any,
    /// Accepts exactly one spelling.
    Exact(String),
    /// Accepts one spelling from a canonical set.
    OneOf(Vec<String>),
    /// Accepts every spelling except those in the set.
    NotIn(Vec<String>),
}

#[derive(Clone, Debug, Hash)]
/// Schema provenance and parser recipe for one authored element.
pub(super) struct ElementParsePlan {
    /// Schema provenance exposed for Explorer occurrences.
    pub(super) explorer_source: ExplorerElementSource,
    /// Stable element name used by generated metadata.
    pub(super) metadata_name: String,
    /// Fixed or semantic parsing recipe.
    pub(super) kind: ElementParseKind,
}

#[derive(Clone, Debug, Hash)]
/// Fixed-token envelope around a semantic container.
pub(super) struct FixedAttachmentParsePlan {
    /// Fixed syntax consumed before the semantic child.
    pub(super) before: Vec<AttachedFixedSyntaxParsePlan>,
    /// Container operation that parses the semantic child.
    pub(super) semantic: Box<PlannedContainer>,
    /// Fixed syntax consumed after the semantic child.
    pub(super) after: Vec<AttachedFixedSyntaxParsePlan>,
}

#[derive(Clone, Debug, Hash)]
/// Classifies required and erased-optional syntax in a generated attachment.
pub(super) enum AttachedFixedSyntaxParsePlan {
    /// Consumes one required fixed token.
    Required(AttachedFixedParsePlan),
    /// Consumes a fixed sequence atomically when its full spelling is present.
    Optional(Vec<AttachedFixedParsePlan>),
}

#[derive(Clone, Debug, Hash)]
/// Matching, capture, and formatting contract for an attached token.
pub(super) struct AttachedFixedParsePlan {
    /// Token expectation used for matching and diagnostics.
    pub(super) expected: ExpectedSet,
    /// Source-capture policy for the fixed token.
    pub(super) capture: PlannedCapturePolicy,
    /// Canonical fixed spelling.
    pub(super) spelling: String,
    /// Pretty spacing adjacent to the spelling.
    pub(super) spacing: PrettySpacing,
}

#[derive(Clone, Debug, Hash)]
/// Parser recipe for a fixed token or semantic field.
pub(super) enum ElementParseKind {
    /// Matches an authored fixed token and records its accessor.
    Fixed {
        #[cfg_attr(not(feature = "explorer"), allow(dead_code))]
        /// Runtime token kind exposed to Explorer.
        kind: u16,
        /// Expected set used for matching and failure evidence.
        expected: ExpectedSet,
        /// Source-capture policy for the token.
        capture: PlannedCapturePolicy,
        /// Canonical authored spelling.
        spelling: String,
        /// Pretty spacing adjacent to the token.
        spacing: PrettySpacing,
        /// Generated accessor identifying this occurrence.
        accessor: Ident,
    },
    /// Parses one erased fixed-token group atomically when its full sequence is present.
    OptionalFixedTokenGroup {
        /// Fixed tokens selected and consumed as one atomic sequence.
        tokens: Vec<AttachedFixedParsePlan>,
        /// Generated span accessor identifying the optional occurrence.
        accessor: Ident,
    },
    /// Parses an authored semantic field through its selected value projection.
    Semantic {
        /// Construction slot receiving the parsed value.
        field: FieldPlan,
        /// Selects direct semantic parsing or boolean fixed-presence projection.
        projection: Box<SemanticParseProjection>,
    },
}

#[derive(Clone, Debug, Hash)]
/// Classifies how generated parsing constructs one authored semantic field value.
pub(super) enum SemanticParseProjection {
    /// Parses the authored field type through its container policy.
    Direct {
        /// Authored terminal type before materialization.
        terminal: Box<Type>,
        /// Resolved node family, when the terminal is generated.
        terminal_node: Option<NodeId>,
        /// Materialized generated-node application, when available.
        terminal_application: Option<MaterializedApplicationId>,
        /// Grammar-erased terminal projection for Explorer schema.
        #[cfg_attr(not(feature = "explorer"), allow(dead_code))]
        explorer_terminal: ExplorerTerminalPlan,
        /// Authored type wrappers, outermost first.
        wrappers: Vec<FieldWrapper>,
        /// Optionality or repetition control-flow recipe.
        container: Box<PlannedContainer>,
        /// Source-capture policy for the stored field value.
        capture: PlannedCapturePolicy,
        /// Source-capture policy applied to each parsed child.
        child_capture: PlannedCapturePolicy,
        /// Minimum binding power when parsing a Pratt child.
        pratt_min_bp: Option<u8>,
        /// LED ordinals excluded at this authored field boundary.
        pratt_excluded_led: Vec<usize>,
        /// Canonical repetition separator spelling, if any.
        separator: Option<String>,
        /// Pretty spacing declared by the repetition separator, if any.
        separator_spacing: Option<PrettySpacing>,
        /// Fixed tokens attached around the semantic child.
        attachment: Option<Box<FixedAttachmentParsePlan>>,
        /// Whether the terminal hides generated-node provenance.
        opaque_provenance: bool,
        /// Pretty line-breaking policy for repeated children.
        repetition_break: PlannedRepetitionBreak,
        /// Formatting hints retained for the field.
        pretty: PrettyHints,
    },
    /// Parses required fixed syntax around one atomic optional sequence stored as `bool`.
    FixedPresence {
        /// Fixed syntax before the optional sequence.
        before: Vec<AttachedFixedSyntaxParsePlan>,
        /// Fixed tokens selected and consumed as one atomic sequence.
        sequence: Vec<AttachedFixedParsePlan>,
        /// Fixed syntax after the optional sequence.
        after: Vec<AttachedFixedSyntaxParsePlan>,
        /// Formatting hints retained for the field.
        pretty: PrettyHints,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Pretty break behavior retained for a repeated parse field.
pub(super) enum PlannedRepetitionBreak {
    /// Allows the formatter to choose a repetition break.
    Soft,
    /// Breaks between occurrences of one materialized statement type.
    TopLevelStatement(MaterializedApplicationId),
    /// Breaks between occurrences from one statement node family.
    TopLevelStatementFamily(NodeId),
}

#[derive(Clone, Debug, Hash)]
/// Construction position and optional named binding for a parsed field.
pub(super) struct FieldPlan {
    /// Named-field binding, absent for tuple and unit positions.
    pub(super) ident: Option<Ident>,
    /// Zero-based construction position in authored field order.
    pub(super) index: usize,
    /// Generated accessor when the field is addressable by name.
    pub(super) accessor: Option<Ident>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Storage constructor applied around an authored terminal type.
pub(super) enum FieldWrapper {
    /// Allocates the child behind `Box`.
    Boxed,
    /// Stores absence or one child in `Option`.
    Optional,
    /// Collects any number of children.
    ZeroOrMore,
    /// Collects children while enforcing a non-empty result.
    OneOrMore,
}

#[derive(Clone, Debug, Hash)]
/// Control-flow and failure contract for direct, optional, or repeated parsing.
pub(super) enum PlannedContainer {
    /// Requires exactly one child.
    Direct {
        /// FOLLOW context passed while parsing the child.
        child_follow: PlannedFollow,
        /// Failure evidence when the required child is absent.
        required_failure: PlannedFollow,
    },
    /// Parses at most one child when its selector matches.
    Optional {
        /// Root FIRST-kind fallback that decides paths omitted from the sparse viability graph.
        selector: ExpectedSet,
        /// Sparse bounded consume-vs-skip overrides when the root selector is insufficient.
        ///
        /// This is absent for declaration-only plans and when analysis proves an unrefined
        /// element FIRST exact-disjoint from static FOLLOW.
        decision: Option<Box<OptionalViabilityDecisionParsePlan>>,
        /// FOLLOW context passed while parsing the child.
        child_follow: PlannedFollow,
    },
    /// Parses a bounded or unbounded sequence with optional separators.
    Repeated {
        /// Collection cardinality and construction operation.
        operation: ContainerOperation,
        /// FIRST set that begins another element.
        element_selector: ExpectedSet,
        /// FIRST set for a configured separator.
        separator_selector: Option<ExpectedSet>,
        /// Whether a separator may terminate the sequence.
        optional_trailing: bool,
        /// FOLLOW evidence that legally ends repetition.
        stop: PlannedFollow,
        /// FOLLOW context passed to each child parser.
        child_follow: PlannedFollow,
        /// Failure emitted when the minimum count is not met.
        minimum_failure: Option<Box<PlannedFollow>>,
        /// Failure when neither another element nor a legal stop matches.
        continuation_failure: Box<PlannedFollow>,
        /// Failure when a separator is not followed by an element.
        after_separator_failure: Option<Box<PlannedFollow>>,
    },
}

#[derive(Clone, Debug, Hash)]
/// Generated representation of one sparse frozen optional consume-vs-skip decision.
///
/// Missing graph actions deliberately delegate to the optional container's root kind selector.
pub(super) struct OptionalViabilityDecisionParsePlan {
    /// Root state of the compact optional viability decision graph.
    root: DispatchStateId,
    /// Canonical graph states; structurally shared suffixes occur once.
    states: Vec<OptionalViabilityDecisionNodeParsePlan>,
    /// Trie-node count retained for validation.
    frozen_node_count: usize,
    /// Deterministic identity of the joined decision graph.
    frozen_identity: u64,
}

#[derive(Clone, Debug, Hash)]
/// One generated state in an optional consume-vs-skip decision graph.
pub(super) struct OptionalViabilityDecisionNodeParsePlan {
    /// Ordered caller-dependent and unconditional actions at this prefix.
    terminals: Vec<OptionalViabilityDecisionTerminalParsePlan>,
    /// Pairwise-disjoint exact token/text partitions extending this prefix.
    edges: Vec<DispatchTrieEdgePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// One generated optional action with its caller-FOLLOW condition.
pub(super) struct OptionalViabilityDecisionTerminalParsePlan {
    /// Consume or skip action selected at this terminal.
    action: crate::OptionalAction,
    /// Whether the action is available only through caller FOLLOW.
    on_caller_follow: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
/// Static and caller-dependent FOLLOW policy used by emitted parsers.
pub(super) enum PlannedFollow {
    /// Uses only the caller-supplied FOLLOW set.
    Caller,
    /// Uses a frozen FOLLOW set independent of the caller.
    Static(ExpectedSet),
    /// Unions a frozen FOLLOW set with the caller context.
    StaticAndCaller(ExpectedSet),
    /// Adds generated Pratt stops without replacing the inherited static boundary.
    CallerAndPrattStops(ExpectedSet),
    /// Composes the wrapped policy, then marks the `#[greedy(...)]`-accepted kinds so a
    /// Pratt child keeps extending on them instead of reporting an ambiguity.
    Accepting {
        #[doc = "FOLLOW policy the accepting site would use without the acceptance."]
        inner: Box<PlannedFollow>,
        #[doc = "Boundary kinds the acceptance lets the Pratt child extend on."]
        accepted: ExpectedSet,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
/// Runtime-width token expectations with optional atom refinements.
pub(super) struct ExpectedSet {
    /// Dense token-kind bitset in runtime word order.
    pub(super) words: Vec<u64>,
    /// Sorted, deduplicated atom indices refining the kind set.
    pub(super) atoms: Vec<u32>,
}

#[derive(Clone, Debug)]
/// Grammar-scoped direct lookup from token kinds to their canonical atom identities.
pub(super) struct ExpectedAtomIndex {
    /// First direct atom declared for each grammar-local token kind.
    by_kind: Vec<Option<u32>>,
}

impl ExpectedAtomIndex {
    /// Indexes direct kind atoms once while preserving the former first-match rule.
    pub(super) fn from_grammar(grammar: &crate::GrammarDescription) -> Result<Self, String> {
        let mut by_kind = vec![None; grammar.kind_count];
        for (atom, description) in grammar.atoms.iter().enumerate() {
            let AtomExpression::Kind(kind) = description.expression else {
                continue;
            };
            let slot = by_kind
                .get_mut(kind.index())
                .ok_or_else(|| "generated expectation kind exceeds grammar width".to_owned())?;
            if slot.is_none() {
                *slot = Some(
                    atom.try_into()
                        .map_err(|_| "generated expectation atom exceeds runtime width")?,
                );
            }
        }
        Ok(Self { by_kind })
    }

    /// Returns the canonical direct atom for one validated token kind.
    pub(super) fn get(&self, kind: crate::KindId) -> Option<u32> {
        self.by_kind.get(kind.index()).copied().flatten()
    }
}

impl ExpectedSet {
    /// Allocates an empty expectation with the runtime kind-set width.
    pub(super) fn empty(kind_count: usize) -> Self {
        Self {
            words: vec![0; kind_count.div_ceil(64)],
            atoms: Vec::new(),
        }
    }

    /// Copies semantic token kinds into the runtime-width representation.
    pub(super) fn from_kinds(kinds: &KindSet, kind_count: usize) -> Self {
        let mut words = kinds.words().to_vec();
        words.resize(kind_count.div_ceil(64), 0);
        Self {
            words,
            atoms: Vec::new(),
        }
    }

    /// Unions FIRST positions and records their sorted atom refinements.
    pub(super) fn from_positions(
        positions: &[FirstPosition],
        atom_index: &ExpectedAtomIndex,
        kind_count: usize,
    ) -> Result<Self, String> {
        let mut value = Self::empty(kind_count);
        for position in positions {
            for (left, right) in value.words.iter_mut().zip(position.kinds.words()) {
                *left |= *right;
            }
            for kind in position.kinds.iter() {
                if let Some(atom) = atom_index.get(kind) {
                    value.atoms.push(atom);
                }
            }
        }
        value.atoms.sort_unstable();
        value.atoms.dedup();
        Ok(value)
    }

    /// Unions another expectation in place and restores atom uniqueness.
    pub(super) fn union_assign(&mut self, other: &Self) {
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            *left |= *right;
        }
        self.atoms.extend(other.atoms.iter().copied());
        self.atoms.sort_unstable();
        self.atoms.dedup();
    }

    /// Removes another expectation's token and diagnostic members in place.
    pub(super) fn difference_assign(&mut self, other: &Self) {
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            *left &= !right;
        }
        self.atoms
            .retain(|atom| other.atoms.binary_search(atom).is_err());
    }

    /// Reports whether empty.
    pub(super) fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0) && self.atoms.is_empty()
    }
}

impl PrattParsePlan {
    /// Verifies bitset width, atom bounds, ordering, and uniqueness.
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.nud.len() != self.expected_nud_count || self.nud.is_empty() {
            return Err("the frozen Pratt plan lacks its complete nud dispatch family".to_owned());
        }
        if self.led.len() != self.expected_led_count {
            return Err("the frozen Pratt plan lacks its complete led dispatch family".to_owned());
        }
        if matches!(&self.nud_failure, PlannedFollow::Static(expected) if expected.is_empty()) {
            return Err("the frozen Pratt plan lacks its nud failure family".to_owned());
        }
        validate_predictive_dispatch(&self.nud_dispatch, self.nud.len())?;
        if self.led.is_empty() {
            if self.caller_follow_guard.is_some()
                || self.led_dispatch.is_some()
                || !self.nud_selection_follow.is_empty()
            {
                return Err(
                    "a Pratt plan without led branches has a caller-FOLLOW guard".to_owned(),
                );
            }
            return Ok(());
        }
        let Some(guard) = &self.caller_follow_guard else {
            return Err("the frozen Pratt plan lacks its caller-FOLLOW guard family".to_owned());
        };
        let Some(led_dispatch) = &self.led_dispatch else {
            return Err("the frozen Pratt plan lacks its led dispatch family".to_owned());
        };
        validate_predictive_dispatch(led_dispatch, self.led.len())?;
        if guard.len() != self.led.len() {
            return Err("the frozen Pratt caller-FOLLOW guard is corrupt".to_owned());
        }
        for branch in &self.led {
            let operands_agree = |threshold: u8| {
                branch.operands.iter().all(|operand| {
                    operand.minimum_binding_power == threshold
                        && operand.element_index < branch.variant.elements.len()
                })
            };
            match branch.position {
                PrattPosition::Infix { rbp, .. }
                    if !branch.operands.is_empty() && operands_agree(rbp) => {}
                PrattPosition::Postfix { bp } if operands_agree(bp) => {}
                _ => {
                    return Err("the frozen Pratt led binding-power family is corrupt".to_owned());
                }
            }
        }
        if guard.iter().any(|branch| branch.expected.is_empty()) {
            return Err("the frozen Pratt caller-FOLLOW guard is corrupt".to_owned());
        }
        let mut expected_follow = ExpectedSet::empty(self.nud_selection_follow.words.len() * 64);
        for branch in guard {
            expected_follow.union_assign(&branch.expected);
        }
        if expected_follow != self.nud_selection_follow {
            return Err("the frozen Pratt nud FOLLOW family is corrupt".to_owned());
        }
        for branch in &self.nud {
            let operands_agree = |threshold: u8| {
                branch.operands.iter().all(|operand| {
                    operand.minimum_binding_power == threshold
                        && operand.element_index < branch.variant.elements.len()
                })
            };
            match branch.position {
                PrattPosition::Atom if branch.operands.is_empty() => {}
                PrattPosition::Prefix { bp }
                    if !branch.operands.is_empty() && operands_agree(bp) => {}
                _ => {
                    return Err(
                        "the frozen Pratt prefix binding-power family is corrupt".to_owned()
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Canonical grammar-wide storage identities for emitted static expectations.
pub(super) struct ExpectedSetPool {
    /// Strictly sorted unique expectations addressed by generated parsers.
    values: Vec<ExpectedSet>,
}

impl ExpectedSetPool {
    /// Canonicalizes one stream of emitted static expectations.
    pub(super) fn from_values(values: impl IntoIterator<Item = ExpectedSet>) -> Self {
        Self {
            values: values
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        }
    }

    /// Collects every static expectation consumed by generated token and Node parsers.
    pub(super) fn from_plans(
        nodes: &[NodeParsePlan],
        tokens: &[TokenParsePlan],
        kind_count: usize,
    ) -> Self {
        let mut values = BTreeSet::new();
        values.extend(tokens.iter().map(|token| token.expected.clone()));
        for application in nodes.iter().flat_map(|node| &node.parse_applications) {
            collect_body_expected_sets(&application.body, kind_count, &mut values);
        }
        Self::from_values(values)
    }

    /// Returns the canonical expectation values in their generated index order.
    pub(super) fn values(&self) -> &[ExpectedSet] {
        &self.values
    }

    /// Resolves one expectation to its stable grammar-local storage index.
    pub(super) fn id(&self, expected: &ExpectedSet) -> Option<usize> {
        self.values.binary_search(expected).ok()
    }

    /// Rejects stale, non-canonical, or runtime-invalid grammar-wide storage.
    pub(super) fn validate(
        &self,
        nodes: &[NodeParsePlan],
        tokens: &[TokenParsePlan],
        kind_count: usize,
        atom_count: usize,
    ) -> Result<(), String> {
        let expected_words = kind_count.div_ceil(64);
        let tail_bits = kind_count % 64;
        let tail_mask = (tail_bits != 0).then(|| (1_u64 << tail_bits) - 1);
        if self.values.iter().any(|expected| {
            expected.words.len() != expected_words
                || tail_mask
                    .is_some_and(|mask| expected.words.last().is_some_and(|word| word & !mask != 0))
                || expected
                    .atoms
                    .iter()
                    .any(|atom| *atom as usize >= atom_count)
                || expected.atoms.windows(2).any(|pair| pair[0] >= pair[1])
        }) {
            return Err("the grammar-wide expectation pool contains an invalid set".to_owned());
        }
        if *self != Self::from_plans(nodes, tokens, kind_count) {
            return Err(
                "the grammar-wide expectation pool diverges from emitted parser plans".to_owned(),
            );
        }
        Ok(())
    }
}

/// Adds the static component of one frozen FOLLOW policy to a generated expectation pool.
fn collect_follow_expected_set(follow: &PlannedFollow, values: &mut BTreeSet<ExpectedSet>) {
    match follow {
        PlannedFollow::Caller => {}
        PlannedFollow::Accepting { inner, accepted } => {
            collect_follow_expected_set(inner, values);
            values.insert(accepted.clone());
        }
        PlannedFollow::Static(expected)
        | PlannedFollow::StaticAndCaller(expected)
        | PlannedFollow::CallerAndPrattStops(expected) => {
            values.insert(expected.clone());
        }
    }
}

/// Adds every expectation emitted for one required or optional fixed-syntax envelope.
fn collect_fixed_syntax_expected_sets(
    syntax: &[AttachedFixedSyntaxParsePlan],
    values: &mut BTreeSet<ExpectedSet>,
) {
    for syntax in syntax {
        match syntax {
            AttachedFixedSyntaxParsePlan::Required(fixed) => {
                values.insert(fixed.expected.clone());
            }
            AttachedFixedSyntaxParsePlan::Optional(tokens) => {
                values.extend(tokens.iter().map(|fixed| fixed.expected.clone()));
            }
        }
    }
}

/// Adds every expectation emitted while parsing one semantic container.
fn collect_container_expected_sets(
    container: &PlannedContainer,
    values: &mut BTreeSet<ExpectedSet>,
) {
    match container {
        PlannedContainer::Direct {
            child_follow,
            required_failure,
        } => {
            collect_follow_expected_set(child_follow, values);
            collect_follow_expected_set(required_failure, values);
        }
        PlannedContainer::Optional {
            selector,
            decision,
            child_follow,
        } => {
            values.insert(selector.clone());
            if let Some(decision) = decision {
                for state in &decision.states {
                    values.extend(
                        state
                            .edges
                            .iter()
                            .map(|edge| edge.position.expected.clone()),
                    );
                }
            }
            collect_follow_expected_set(child_follow, values);
        }
        PlannedContainer::Repeated {
            element_selector,
            separator_selector,
            stop,
            child_follow,
            minimum_failure,
            continuation_failure,
            after_separator_failure,
            ..
        } => {
            values.insert(element_selector.clone());
            values.extend(separator_selector.iter().cloned());
            collect_follow_expected_set(stop, values);
            collect_follow_expected_set(child_follow, values);
            if let Some(failure) = minimum_failure {
                collect_follow_expected_set(failure, values);
            }
            collect_follow_expected_set(continuation_failure, values);
            if let Some(failure) = after_separator_failure {
                collect_follow_expected_set(failure, values);
            }
        }
    }
}

/// Adds every expectation emitted around one attached semantic child.
fn collect_attachment_expected_sets(
    attachment: &FixedAttachmentParsePlan,
    values: &mut BTreeSet<ExpectedSet>,
) {
    collect_fixed_syntax_expected_sets(&attachment.before, values);
    collect_container_expected_sets(&attachment.semantic, values);
    collect_fixed_syntax_expected_sets(&attachment.after, values);
}

/// Adds every expectation emitted for one structural element sequence.
fn collect_element_expected_sets(
    elements: &[ElementParsePlan],
    values: &mut BTreeSet<ExpectedSet>,
) {
    for element in elements {
        match &element.kind {
            ElementParseKind::Fixed { expected, .. } => {
                values.insert(expected.clone());
            }
            ElementParseKind::OptionalFixedTokenGroup { tokens, .. } => {
                values.extend(tokens.iter().map(|fixed| fixed.expected.clone()));
            }
            ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
                SemanticParseProjection::FixedPresence {
                    before,
                    sequence,
                    after,
                    ..
                } => {
                    collect_fixed_syntax_expected_sets(before, values);
                    values.extend(sequence.iter().map(|fixed| fixed.expected.clone()));
                    collect_fixed_syntax_expected_sets(after, values);
                }
                SemanticParseProjection::Direct {
                    container,
                    attachment,
                    ..
                } => {
                    collect_container_expected_sets(container, values);
                    if let Some(attachment) = attachment {
                        collect_attachment_expected_sets(attachment, values);
                    }
                }
            },
        }
    }
}

/// Reconstructs the frozen failure expectation rendered for one predictive graph state.
fn dispatch_state_expected_set(
    state: &DispatchTrieNodePlan,
    root_extra: Option<&ExpectedSet>,
    kind_count: usize,
) -> ExpectedSet {
    let mut expected = ExpectedSet::empty(kind_count);
    for edge in &state.edges {
        expected.union_assign(&edge.position.expected);
    }
    if let Some(additional) = root_extra {
        expected.union_assign(additional);
    }
    expected
}

/// Adds edge tests and state failures emitted by ordinary enum dispatch.
fn collect_failing_dispatch_expected_sets(
    dispatch: &PredictiveDispatchPlan,
    kind_count: usize,
    values: &mut BTreeSet<ExpectedSet>,
) {
    for (index, state) in dispatch.states.iter().enumerate() {
        values.extend(
            state
                .edges
                .iter()
                .map(|edge| edge.position.expected.clone()),
        );
        values.insert(dispatch_state_expected_set(
            state,
            (index == dispatch.root.index())
                .then_some(dispatch.balanced.as_ref())
                .flatten()
                .map(|balanced| &balanced.open),
            kind_count,
        ));
    }
    if let Some(balanced) = &dispatch.balanced {
        values.insert(balanced.open.clone());
        values.insert(balanced.close.clone());
        for state in &balanced.after_close_states {
            values.extend(
                state
                    .edges
                    .iter()
                    .map(|edge| edge.position.expected.clone()),
            );
            values.insert(dispatch_state_expected_set(state, None, kind_count));
        }
    }
}

/// Adds edge tests emitted by a non-failing Pratt predictive selector.
fn collect_selection_dispatch_expected_sets(
    dispatch: &PredictiveDispatchPlan,
    values: &mut BTreeSet<ExpectedSet>,
) {
    values.extend(
        dispatch
            .states
            .iter()
            .flat_map(|state| &state.edges)
            .map(|edge| edge.position.expected.clone()),
    );
}

/// Adds every static expectation emitted for one materialized Node parser body.
fn collect_body_expected_sets(
    body: &NodeBodyPlan,
    kind_count: usize,
    values: &mut BTreeSet<ExpectedSet>,
) {
    match body {
        NodeBodyPlan::Struct { elements, .. } => collect_element_expected_sets(elements, values),
        NodeBodyPlan::Enum { variants, dispatch } => {
            for variant in variants {
                collect_element_expected_sets(&variant.elements, values);
            }
            collect_failing_dispatch_expected_sets(dispatch, kind_count, values);
        }
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &pratt.nud {
                collect_element_expected_sets(&branch.variant.elements, values);
            }
            for branch in &pratt.led {
                collect_element_expected_sets(&branch.variant.elements, values);
            }
            collect_selection_dispatch_expected_sets(&pratt.nud_dispatch, values);
            if let Some(dispatch) = &pratt.led_dispatch {
                collect_selection_dispatch_expected_sets(dispatch, values);
            }
            collect_follow_expected_set(&pratt.nud_failure, values);
            values.insert(pratt.nud_selection_follow.clone());
            values.extend(
                pratt
                    .caller_follow_guard
                    .iter()
                    .flat_map(|guard| pratt_guard_thresholds(guard, kind_count))
                    .map(|(_, expected)| expected),
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Source-capture policy after inheritance has been resolved or retained.
pub(super) enum PlannedCapturePolicy {
    /// Defers capture choice to the caller context.
    Inherited,
    /// Records no source capture.
    Off,
    /// Captures source when the runtime context enables it.
    Optional,
    /// Requires source capture for the parsed value.
    Mandatory,
}

impl PlannedCapturePolicy {
    /// Returns the stable capture-policy spelling used in plan identities.
    pub(super) const fn canonical_name(self) -> &'static str {
        match self {
            Self::Inherited => "inherited",
            Self::Off => "off",
            Self::Optional => "optional",
            Self::Mandatory => "mandatory",
        }
    }

    /// Emits the corresponding runtime capture-policy expression.
    pub(super) fn render(self, facade: &Ident) -> proc_macro2::TokenStream {
        match self {
            Self::Inherited => quote!(context.capture_policy()),
            Self::Off => quote!(#facade::__private::CapturePolicy::Off),
            Self::Optional => quote!(#facade::__private::CapturePolicy::Optional),
            Self::Mandatory => quote!(#facade::__private::CapturePolicy::Mandatory),
        }
    }
}

impl From<CaptureMode> for PlannedCapturePolicy {
    fn from(value: CaptureMode) -> Self {
        match value {
            CaptureMode::Off => Self::Off,
            CaptureMode::Optional => Self::Optional,
            CaptureMode::Mandatory => Self::Mandatory,
        }
    }
}

/// Converts one analyzed FIRST position into a runtime dispatch condition.
pub(super) fn planned_dispatch_position(
    position: &crate::DispatchPosition,
    atom_index: &ExpectedAtomIndex,
    kind_count: usize,
) -> Result<DispatchPositionPlan, String> {
    let text_predicate = position.text_predicate().map(|predicate| match predicate {
        TextPredicate::Any => PlannedTextPredicate::Any,
        TextPredicate::Exact(text) => PlannedTextPredicate::Exact(text.clone()),
        TextPredicate::OneOf(options) => PlannedTextPredicate::OneOf(options.clone()),
        TextPredicate::NotIn(options) => PlannedTextPredicate::NotIn(options.clone()),
    });
    let first_position = FirstPosition {
        kinds: position.kinds().clone(),
        text_predicate: None,
    };
    Ok(DispatchPositionPlan {
        expected: ExpectedSet::from_positions(
            std::slice::from_ref(&first_position),
            atom_index,
            kind_count,
        )?,
        text_predicate,
    })
}

/// Identity of one shared frozen dispatch node.
type InternKey = (Option<crate::DispatchTerminal>, usize, usize);

/// Reconstructs a frozen predictive graph while preserving shared suffix states.
pub(super) fn plan_predictive_dispatch(
    frozen: &crate::DispatchTrieNode,
    variants: &[(crate::DeclId, usize)],
    atom_index: &ExpectedAtomIndex,
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<(DispatchStateId, Vec<DispatchTrieNodePlan>), String> {
    fn intern(
        frozen: &crate::DispatchTrieNode,
        variants: &[(crate::DeclId, usize)],
        atom_index: &ExpectedAtomIndex,
        kind_count: usize,
        states: &mut Vec<DispatchTrieNodePlan>,
        memo: &mut BTreeMap<InternKey, DispatchStateId>,
    ) -> Result<DispatchStateId, String> {
        let key = (
            frozen.terminal(),
            frozen.edges().as_ptr() as usize,
            frozen.edges().len(),
        );
        if let Some(state) = memo.get(&key) {
            return Ok(*state);
        }
        let target = frozen
            .terminal()
            .map(|terminal| {
                variants
                    .iter()
                    .find(|(declaration, _)| *declaration == terminal.declaration())
                    .map(|(_, variant)| DispatchTargetPlan {
                        variant: *variant,
                        mode: terminal.mode(),
                    })
                    .ok_or_else(|| "frozen dispatch terminal lacks a source variant".to_owned())
            })
            .transpose()?;
        let edges = frozen
            .edges()
            .iter()
            .map(|edge| {
                Ok(DispatchTrieEdgePlan {
                    position: planned_dispatch_position(edge.position(), atom_index, kind_count)?,
                    next: intern(edge.next(), variants, atom_index, kind_count, states, memo)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let state = DispatchStateId(states.len());
        states.push(DispatchTrieNodePlan { target, edges });
        memo.insert(key, state);
        Ok(state)
    }

    let mut states = Vec::new();
    let root = intern(
        frozen,
        variants,
        atom_index,
        kind_count,
        &mut states,
        &mut BTreeMap::new(),
    )?;
    instrumentation.frozen_dispatch_nodes_consumed += states.len();
    Ok((root, states))
}

/// Joins sparse optional viability overrides to runtime-width token expectations.
///
/// The resulting graph retains exact override/defer paths only. An action omitted by the frozen
/// graph remains a deliberate delegation to the optional container's root kind selector.
fn plan_optional_viability_decision(
    frozen: &crate::OptionalViabilityDecision,
    atom_index: &ExpectedAtomIndex,
    kind_count: usize,
) -> Result<OptionalViabilityDecisionParsePlan, String> {
    fn intern(
        frozen: &crate::OptionalViabilityDecisionNode,
        atom_index: &ExpectedAtomIndex,
        kind_count: usize,
        states: &mut Vec<OptionalViabilityDecisionNodeParsePlan>,
        memo: &mut BTreeMap<
            (Vec<crate::OptionalViabilityDecisionTerminal>, usize, usize),
            DispatchStateId,
        >,
    ) -> Result<DispatchStateId, String> {
        let key = (
            frozen.terminals().to_vec(),
            frozen.edges().as_ptr() as usize,
            frozen.edges().len(),
        );
        if let Some(state) = memo.get(&key) {
            return Ok(*state);
        }
        let terminals = frozen
            .terminals()
            .iter()
            .map(|terminal| OptionalViabilityDecisionTerminalParsePlan {
                action: terminal.action(),
                on_caller_follow: terminal.on_caller_follow(),
            })
            .collect();
        let edges = frozen
            .edges()
            .iter()
            .map(|edge| {
                Ok(DispatchTrieEdgePlan {
                    position: planned_dispatch_position(edge.position(), atom_index, kind_count)?,
                    next: intern(edge.next(), atom_index, kind_count, states, memo)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let state = DispatchStateId(states.len());
        states.push(OptionalViabilityDecisionNodeParsePlan { terminals, edges });
        memo.insert(key, state);
        Ok(state)
    }

    let mut states = Vec::new();
    let root = intern(
        frozen.trie(),
        atom_index,
        kind_count,
        &mut states,
        &mut BTreeMap::new(),
    )?;
    let frozen_node_count = states.len();
    let frozen_identity = optional_viability_decision_identity(root, &states);
    let plan = OptionalViabilityDecisionParsePlan {
        root,
        states,
        frozen_node_count,
        frozen_identity,
    };
    validate_optional_viability_decision_parse_plan(&plan)?;
    Ok(plan)
}

/// Computes the deterministic identity of one generated optional viability decision graph.
fn optional_viability_decision_identity(
    root: DispatchStateId,
    states: &[OptionalViabilityDecisionNodeParsePlan],
) -> u64 {
    stable_hash(format!("{root:?}:{states:?}").as_bytes())
}

/// Rejects malformed or stale optional viability decision graph structure before rendering.
fn validate_optional_viability_decision_parse_plan(
    decision: &OptionalViabilityDecisionParsePlan,
) -> Result<(), String> {
    fn visit(
        state: DispatchStateId,
        states: &[OptionalViabilityDecisionNodeParsePlan],
        visited: &mut BTreeSet<DispatchStateId>,
    ) -> Result<(), String> {
        let node = states.get(state.index()).ok_or_else(|| {
            "frozen optional viability decision references an unknown state".to_owned()
        })?;
        if !visited.insert(state) {
            return Ok(());
        }
        if node.terminals.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                "frozen optional viability decision contains a duplicate terminal".to_owned(),
            );
        }
        if node.edges.iter().enumerate().any(|(index, edge)| {
            node.edges[index + 1..]
                .iter()
                .any(|other| other.position == edge.position)
        }) {
            return Err("frozen optional viability decision contains duplicate edges".to_owned());
        }
        for edge in &node.edges {
            visit(edge.next, states, visited)?;
        }
        Ok(())
    }

    let mut visited = BTreeSet::new();
    visit(decision.root, &decision.states, &mut visited)?;
    if visited.len() != decision.frozen_node_count
        || decision.frozen_node_count != decision.states.len()
        || optional_viability_decision_identity(decision.root, &decision.states)
            != decision.frozen_identity
    {
        return Err(
            "the joined optional viability decision diverges from its frozen identity".to_owned(),
        );
    }
    Ok(())
}

/// Computes the deterministic identity used to validate a predictive graph.
pub(super) fn predictive_dispatch_identity(
    root: DispatchStateId,
    states: &[DispatchTrieNodePlan],
) -> u64 {
    stable_hash(format!("{root:?}:{states:?}").as_bytes())
}

/// Rejects dispatch plans whose shape or identity differs from semantic analysis.
pub(super) fn validate_predictive_dispatch(
    dispatch: &PredictiveDispatchPlan,
    variants: usize,
) -> Result<usize, String> {
    fn validate_node(
        state: DispatchStateId,
        states: &[DispatchTrieNodePlan],
        variants: usize,
        visited: &mut BTreeSet<DispatchStateId>,
        allow_empty_root: bool,
    ) -> Result<usize, String> {
        let Some(node) = states.get(state.index()) else {
            return Err("frozen predictive dispatch references an unknown state".to_owned());
        };
        if !visited.insert(state) {
            return Ok(0);
        }
        if node.target.is_none() && node.edges.is_empty() && !allow_empty_root {
            return Err("frozen predictive dispatch contains an empty node".to_owned());
        }
        if node.target.is_some_and(|target| target.variant >= variants) {
            return Err("frozen predictive dispatch references an unknown variant".to_owned());
        }
        if node.target.is_some_and(|target| {
            matches!(target.mode, crate::DispatchTerminalMode::Immediate) && !node.edges.is_empty()
        }) {
            return Err("frozen predictive dispatch extends an immediate terminal".to_owned());
        }
        if node.target.is_some_and(|target| {
            matches!(target.mode, crate::DispatchTerminalMode::FallbackAfterEdges)
                && node.edges.is_empty()
        }) {
            return Err("frozen predictive dispatch fallback lacks competitor edges".to_owned());
        }
        if node.edges.iter().enumerate().any(|(index, edge)| {
            node.edges[index + 1..]
                .iter()
                .any(|other| other.position == edge.position)
        }) {
            return Err("frozen predictive dispatch contains duplicate edges".to_owned());
        }
        let mut count = 1;
        for edge in &node.edges {
            count += validate_node(edge.next, states, variants, visited, false)?;
        }
        Ok(count)
    }

    let count = validate_node(
        dispatch.root,
        &dispatch.states,
        variants,
        &mut BTreeSet::new(),
        dispatch.balanced.is_some(),
    )?;
    if count != dispatch.frozen_node_count
        || count != dispatch.states.len()
        || predictive_dispatch_identity(dispatch.root, &dispatch.states) != dispatch.frozen_identity
    {
        return Err("the joined predictive plan diverges from its frozen identity".to_owned());
    }
    let mut total = count;
    if let Some(balanced) = &dispatch.balanced {
        if balanced.open.is_empty() || balanced.close.is_empty() || balanced.open == balanced.close
        {
            return Err("balanced dispatch has invalid delimiter expectations".to_owned());
        }
        let balanced_count = validate_node(
            balanced.after_close_root,
            &balanced.after_close_states,
            variants,
            &mut BTreeSet::new(),
            false,
        )?;
        if balanced_count != balanced.frozen_node_count
            || balanced_count != balanced.after_close_states.len()
            || predictive_dispatch_identity(balanced.after_close_root, &balanced.after_close_states)
                != balanced.frozen_identity
        {
            return Err(
                "the joined balanced residual diverges from its frozen identity".to_owned(),
            );
        }
        total += balanced_count;
    }
    Ok(total)
}

/// Freezes all parse applications and body decisions for one resolved node.
///
/// # Long-function contract
///
/// Rationale: joins one materialized Node application to frozen analysis decisions and authored capture facts.
/// Invariants: no FIRST, FOLLOW, dispatch, or Pratt decision is rediscovered during generation.
/// Phases: resolves source shape, plans applications and body, attaches traits, then validates names.
pub(super) fn plan_node(
    node: &ResolvedNode,
    application: &MaterializedNodeApplication,
    lowered: &crate::materialization::LoweredGrammar,
    lookups: &EmissionLookupIndex<'_>,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<NodeParsePlan, String> {
    let atom_index = lookups.atom_index;
    let kind_count = lookups.kind_count;
    let inherited_capture = node
        .capture
        .unwrap_or(lowered.source_modeled.resolved().build_inputs.spans);
    let application_type = lowered
        .lowering_map
        .applications
        .iter()
        .find(|correlation| correlation.application == application.id)
        .ok_or_else(|| format!("Node `{}` lacks a normalized type correlation", node.ident))?
        .lowered;
    let source = &lowered.source_modeled.resolved().tree.files[node.source.index()];
    let physical_source = normalized_source_identity(&source.path)?;
    let (line, column) = source_line_column(&source.text, node.range.start)?;
    let loader_anchor = declaration_loader_anchor(&physical_source, line, column, &node.ident);
    let declaration_input_lifetime = node
        .generics
        .params
        .iter()
        .find_map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => Some(parameter.lifetime.clone()),
            GenericParam::Type(_) | GenericParam::Const(_) => None,
        })
        .unwrap_or_else(|| syn::parse_quote!('__recursa_input));
    let input_lifetime: syn::Lifetime = syn::parse_quote!('__recursa_input);

    let body = match (
        &application.body,
        &node.body,
        &lowered.grammar.types[application_type.index()].shape,
    ) {
        (
            MaterializedNodeBody::Struct(elements) | MaterializedNodeBody::Presence(elements),
            ResolvedNodeBody::Struct { style },
            TypeShape::Sequence { sites },
        ) => NodeBodyPlan::Struct {
            style: *style,
            elements: plan_elements(
                node,
                None,
                elements,
                sites,
                lowered,
                lookups,
                &input_lifetime,
                None,
                None,
            )?,
        },
        (
            MaterializedNodeBody::Enum(variants),
            ResolvedNodeBody::Enum {
                variants: resolved_variants,
            },
            TypeShape::Enum { alternatives },
        ) => {
            let dispatch = lookups
                .dispatch(application_type, DispatchKind::Enum)
                .ok_or_else(|| format!("enum Node `{}` lacks its dispatch plan", node.ident))?;
            let failure_expected = lookups
                .owner_failure(application_type, FailureEdgeKind::EnumDispatch)
                .ok_or_else(|| {
                    format!("enum Node `{}` lacks its frozen failure edge", node.ident)
                })?;
            let (failure_expected, failure_uses_caller) = match failure_expected.expected() {
                ExpectedRecipe::Static(first) => (first.as_slice(), false),
                ExpectedRecipe::Follow(follow) => (follow.static_first(), follow.uses_caller()),
                _ => {
                    return Err(format!(
                        "enum Node `{}` has an invalid dispatch failure recipe",
                        node.ident
                    ));
                }
            };
            let failure_expected =
                ExpectedSet::from_positions(failure_expected, atom_index, kind_count)?;
            let mut planned = Vec::new();
            let mut variants_by_declaration = Vec::new();
            for variant in variants {
                let resolved_variant = resolved_variants
                    .iter()
                    .find(|candidate| candidate.id == variant.resolved_variant)
                    .ok_or_else(|| "materialized enum variant lacks source shape".to_owned())?;
                let correlation = lowered
                    .lowering_map
                    .variants
                    .iter()
                    .find(|correlation| {
                        correlation.application == application.id
                            && correlation.variant == variant.resolved_variant
                    })
                    .ok_or_else(|| {
                        "materialized enum variant lacks lowering correlation".to_owned()
                    })?;
                let branch = dispatch
                    .branches()
                    .iter()
                    .find(|branch| branch.declaration() == correlation.lowered)
                    .ok_or_else(|| "materialized enum variant lacks dispatch branch".to_owned())?;
                if branch.sequences().is_empty()
                    && !dispatch_reaches_declaration(dispatch, branch.declaration())
                {
                    return Err(format!(
                        "enum variant `{}` lacks a frozen dispatch sequence",
                        variant.name
                    ));
                }
                variants_by_declaration.push((branch.declaration(), planned.len()));
                let alternative = alternatives
                    .iter()
                    .find(|alternative| alternative.declaration == correlation.lowered)
                    .ok_or_else(|| {
                        "enum dispatch branch lacks frozen alternative sites".to_owned()
                    })?;
                planned.push(VariantParsePlan {
                    resolved_variant: variant.resolved_variant,
                    ident: emitted_ident(&variant.name)?,
                    style: resolved_variant.style,
                    elements: plan_elements(
                        node,
                        Some(variant),
                        &variant.elements,
                        &alternative.sites,
                        lowered,
                        lookups,
                        &input_lifetime,
                        None,
                        None,
                    )?,
                    capture: resolved_variant.capture.map(PlannedCapturePolicy::from),
                    pretty: variant.pretty.clone(),
                });
            }
            let frozen_branch_count = dispatch.branches().len();
            let dispatch = plan_joined_dispatch(
                dispatch,
                &variants_by_declaration,
                lowered,
                atom_index,
                kind_count,
                instrumentation,
            )?;
            let mut root_expected = ExpectedSet::empty(kind_count);
            for edge in &dispatch.states[dispatch.root.index()].edges {
                root_expected.union_assign(&edge.position.expected);
            }
            if let Some(balanced) = &dispatch.balanced {
                root_expected.union_assign(&balanced.open);
            }
            if root_expected != failure_expected {
                return Err(format!(
                    "enum Node `{}` dispatch and failure products disagree",
                    node.ident
                ));
            }
            let root_uses_caller =
                dispatch.states[dispatch.root.index()]
                    .target
                    .is_some_and(|target| {
                        matches!(target.mode, crate::DispatchTerminalMode::CallerFollow)
                    });
            if root_uses_caller != failure_uses_caller {
                return Err(format!(
                    "enum Node `{}` dispatch and caller-FOLLOW failure products disagree",
                    node.ident
                ));
            }
            if variants_by_declaration.len() != frozen_branch_count
                || (dispatch.frozen_sequence_count == 0 && dispatch.balanced.is_none())
            {
                return Err(format!(
                    "enum Node `{}` lost a frozen dispatch sequence",
                    node.ident
                ));
            }
            NodeBodyPlan::Enum {
                variants: planned,
                dispatch: Box::new(dispatch),
            }
        }
        (
            MaterializedNodeBody::Pratt(materialized),
            ResolvedNodeBody::Enum { variants: resolved },
            TypeShape::Pratt { variants: semantic },
        ) => NodeBodyPlan::Pratt(Box::new(plan_pratt_node(
            node,
            application,
            application_type,
            materialized,
            resolved,
            semantic,
            lowered,
            lookups,
            &input_lifetime,
            instrumentation,
        )?)),
        _ => {
            return Err(format!(
                "Node `{}` source shape and frozen semantic analysis shape disagree",
                node.ident
            ));
        }
    };
    Ok(NodeParsePlan {
        node: node.id,
        semantic_key: node.key.segments.join("::"),
        module: node.module,
        ident: emitted_ident(&node.ident)?,
        publicly_materialized: publicly_reachable(
            &lowered.source_modeled.resolved().tree,
            node.module,
            &node.visibility,
        ),
        loader_anchor,
        generics: node.generics.clone(),
        input_lifetime: declaration_input_lifetime,
        body,
        parse_applications: Vec::new(),
        capture: inherited_capture.into(),
        configured_traits: Vec::new(),
        explorer_visit_configured: false,
        pretty: application.pretty.clone(),
    })
}

/// Plans a struct or enum body from its resolved authored structure.
///
/// # Long-function contract
///
/// Rationale: keeps structural loader shape correlated with source declarations and normalized identities.
/// Invariants: each loader key and location refers to the selected module inclusion.
/// Phases: projects declaration metadata, body shape, applications, and configured recipes.
pub(super) fn structural_body(
    node: &ResolvedNode,
    source_node: &SourceNode,
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
) -> Result<NodeBodyPlan, String> {
    let body = match (&source_node.body, &node.body) {
        (
            SourceNodeBody::Struct(elements) | SourceNodeBody::Presence(elements),
            ResolvedNodeBody::Struct { style },
        ) => NodeBodyPlan::Struct {
            style: *style,
            elements: declaration_elements(node, None, elements, lowered, kind_count)?,
        },
        (
            SourceNodeBody::Enum(variants),
            ResolvedNodeBody::Enum {
                variants: resolved_variants,
            },
        ) => {
            let variants = variants
                .iter()
                .map(|variant| {
                    let resolved_variant = resolved_variants
                        .iter()
                        .find(|resolved| resolved.id == variant.resolved_variant)
                        .ok_or_else(|| "authored enum variant lacks resolved shape".to_owned())?;
                    Ok(VariantParsePlan {
                        resolved_variant: resolved_variant.id,
                        ident: emitted_ident(&variant.name)?,
                        style: resolved_variant.style,
                        elements: declaration_elements(
                            node,
                            Some((variant, resolved_variant)),
                            &variant.elements,
                            lowered,
                            kind_count,
                        )?,
                        capture: resolved_variant.capture.map(PlannedCapturePolicy::from),
                        pretty: variant.pretty.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            NodeBodyPlan::Enum {
                variants,
                dispatch: Box::new(empty_declaration_dispatch()),
            }
        }
        (
            SourceNodeBody::Pratt(variants),
            ResolvedNodeBody::Enum {
                variants: resolved_variants,
            },
        ) => {
            let mut nud = Vec::new();
            let mut led = Vec::new();
            for (source_index, source_variant) in variants.iter().enumerate() {
                let resolved_variant = resolved_variants
                    .iter()
                    .find(|resolved| resolved.id == source_variant.common.resolved_variant)
                    .ok_or_else(|| "authored Pratt variant lacks resolved shape".to_owned())?;
                let mut variant = VariantParsePlan {
                    resolved_variant: resolved_variant.id,
                    ident: emitted_ident(&source_variant.common.name)?,
                    style: resolved_variant.style,
                    elements: declaration_elements(
                        node,
                        Some((&source_variant.common, resolved_variant)),
                        &source_variant.common.elements,
                        lowered,
                        kind_count,
                    )?,
                    capture: resolved_variant.capture.map(PlannedCapturePolicy::from),
                    pretty: source_variant.common.pretty.clone(),
                };
                let position = match source_variant.position {
                    SourcePrattPosition::Atom => PrattPosition::Atom,
                    SourcePrattPosition::Prefix { bp } => PrattPosition::Prefix { bp },
                    SourcePrattPosition::Infix { lbp, rbp } => PrattPosition::Infix { lbp, rbp },
                    SourcePrattPosition::Postfix { bp } => PrattPosition::Postfix { bp },
                };
                match position {
                    PrattPosition::Atom | PrattPosition::Prefix { .. } => {
                        nud.push(PrattNudParsePlan {
                            source_index,
                            variant,
                            position,
                            operands: Vec::new(),
                        });
                    }
                    PrattPosition::Infix { .. } | PrattPosition::Postfix { .. } => {
                        let lhs_index = variant
                            .elements
                            .iter()
                            .position(|element| {
                                matches!(element.kind, ElementParseKind::Semantic { .. })
                            })
                            .ok_or_else(|| {
                                "authored Pratt led variant lacks its recursive lhs".to_owned()
                            })?;
                        let lhs = variant.elements.remove(lhs_index);
                        led.push(PrattLedParsePlan {
                            source_index,
                            variant,
                            lhs,
                            position,
                            operands: Vec::new(),
                        });
                    }
                }
            }
            NodeBodyPlan::Pratt(Box::new(PrattParsePlan {
                expected_nud_count: nud.len(),
                expected_led_count: led.len(),
                nud,
                led,
                nud_dispatch: empty_declaration_dispatch(),
                led_dispatch: None,
                nud_failure: PlannedFollow::Caller,
                nud_selection_follow: ExpectedSet::empty(kind_count),
                caller_follow_guard: None,
            }))
        }
        _ => {
            return Err(format!(
                "Node `{}` authored and resolved declaration shapes disagree",
                node.ident
            ));
        }
    };
    let mut body = body;
    disambiguate_fixed_accessors(&mut body)?;
    validate_accessor_names(node, &body)?;
    Ok(body)
}

/// Reports whether the authoritative trie selects one declaration on any path.
fn dispatch_trie_reaches_declaration(
    trie: &crate::DispatchTrieNode,
    declaration: crate::DeclId,
) -> bool {
    trie.terminal()
        .is_some_and(|terminal| terminal.declaration() == declaration)
        || trie
            .edges()
            .iter()
            .any(|edge| dispatch_trie_reaches_declaration(edge.next(), declaration))
}

/// Reports whether an ordinary or balanced dispatch selects one declaration.
fn dispatch_reaches_declaration(
    dispatch: &crate::DispatchPlan,
    declaration: crate::DeclId,
) -> bool {
    dispatch_trie_reaches_declaration(dispatch.trie(), declaration)
        || dispatch.balanced().is_some_and(|balanced| {
            dispatch_trie_reaches_declaration(balanced.after_close(), declaration)
        })
}

/// Creates the canonical dispatch plan for an enum with no selectable variants.
pub(super) fn empty_declaration_dispatch() -> PredictiveDispatchPlan {
    let root = DispatchStateId(0);
    let states = vec![DispatchTrieNodePlan::default()];
    PredictiveDispatchPlan {
        frozen_identity: predictive_dispatch_identity(root, &states),
        root,
        states,
        frozen_sequence_count: 0,
        frozen_node_count: 1,
        balanced: None,
    }
}

/// Plans declaration elements in authored field order.
///
/// # Long-function contract
///
/// Rationale: freezes declaration-local loader and visibility facts before render.
/// Invariants: selected-root anchors and semantic keys remain source-relative and deterministic.
/// Phases: resolves inclusion, derives loader anchors, then records generic and visibility facts.
pub(super) fn declaration_elements(
    node: &ResolvedNode,
    variant: Option<(
        &crate::source_model::Variant,
        &crate::resolution::ResolvedNodeVariant,
    )>,
    elements: &[GrammarElement],
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
) -> Result<Vec<ElementParsePlan>, String> {
    let source = lowered.source_modeled.source_model();
    let resolved = lowered.source_modeled.resolved();
    elements
        .iter()
        .map(|element| match element {
            GrammarElement::FixedToken { token_use, .. } => {
                let usage = &source.token_uses[token_use.index()];
                let definition = &source.token_definitions[usage.definition.index()];
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return Err("an authored fixed element references a non-fixed token".to_owned());
                };
                let name = fixed_element_name(node, usage, definition, fixed);
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::Fixed(*token_use),
                    metadata_name: name.clone(),
                    kind: ElementParseKind::Fixed {
                        kind: explorer_kind_index(lowered, definition.id)?,
                        expected: ExpectedSet::empty(kind_count),
                        capture: PlannedCapturePolicy::Inherited,
                        spelling: fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        ),
                        spacing: fixed.pretty_spacing,
                        accessor: fixed_accessor_ident(&name)?,
                    },
                })
            }
            GrammarElement::OptionalFixedTokenGroup { id, tokens, .. } => {
                let metadata_name = format!(
                    "optional_{}",
                    tokens
                        .iter()
                        .map(|token| {
                            let usage = &source.token_uses[token.token_use.index()];
                            snake_case(&source.token_definitions[usage.definition.index()].name)
                        })
                        .collect::<Vec<_>>()
                        .join("_")
                );
                let plan_fixed = |token: &crate::source_model::AttachedFixedToken| {
                    let usage = &source.token_uses[token.token_use.index()];
                    let definition = &source.token_definitions[usage.definition.index()];
                    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                        return Err("an optional group references non-fixed syntax".to_owned());
                    };
                    Ok(AttachedFixedParsePlan {
                        expected: ExpectedSet::empty(kind_count),
                        capture: PlannedCapturePolicy::Mandatory,
                        spelling: fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        ),
                        spacing: fixed.pretty_spacing,
                    })
                };
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::OptionalFixed(*id),
                    metadata_name: metadata_name.clone(),
                    kind: ElementParseKind::OptionalFixedTokenGroup {
                        tokens: tokens.iter().map(plan_fixed).collect::<Result<_, _>>()?,
                        accessor: fixed_accessor_ident(&metadata_name)?,
                    },
                })
            }
            GrammarElement::SemanticField {
                resolved_field,
                projection,
                attachment,
                separator,
                pretty,
                ..
            } => {
                let field = node
                    .fields
                    .iter()
                    .find(|field| field.id == *resolved_field)
                    .ok_or_else(|| "an authored semantic element lacks its field".to_owned())?;
                let field_index = variant
                    .map_or_else(
                        || {
                            node.fields
                                .iter()
                                .position(|candidate| candidate.id == field.id)
                        },
                        |(_, resolved)| {
                            resolved
                                .fields
                                .iter()
                                .position(|candidate| *candidate == field.id)
                        },
                    )
                    .ok_or_else(|| {
                        "an authored semantic element lacks its field index".to_owned()
                    })?;
                let base = field
                    .syntax
                    .ident
                    .clone()
                    .unwrap_or_else(|| format_ident!("field_{field_index}"));
                let (metadata_name, accessor) = variant.map_or_else(
                    || (base.unraw().to_string(), base.clone()),
                    |(source_variant, _)| {
                        let prefix = snake_case(&source_variant.name);
                        let accessor = if base.unraw() == prefix {
                            base.clone()
                        } else {
                            format_ident!("{}_{}", prefix, base)
                        };
                        (format!("{}_{}", prefix, base.unraw()), accessor)
                    },
                );
                let projection = match projection {
                    crate::source_model::SemanticFieldProjection::FixedPresence { tokens } => {
                        let plan_fixed = |token_use: crate::lowering::TokenUseId| {
                            declaration_fixed_token(
                                token_use,
                                lowered,
                                kind_count,
                                PlannedCapturePolicy::Inherited,
                            )
                        };
                        let attachment = attachment
                            .as_ref()
                            .map(|attachment| {
                                declaration_fixed_attachment(attachment, lowered, kind_count)
                            })
                            .transpose()?;
                        SemanticParseProjection::FixedPresence {
                            before: attachment
                                .as_ref()
                                .map_or_else(Vec::new, |attachment| attachment.before.clone()),
                            sequence: tokens
                                .iter()
                                .map(|token| plan_fixed(token.token_use))
                                .collect::<Result<_, _>>()?,
                            after: attachment.map_or_else(Vec::new, |attachment| attachment.after),
                            pretty: pretty.clone(),
                        }
                    }
                    crate::source_model::SemanticFieldProjection::Direct => {
                        let authored = authored_projection(&field.ty, resolved)?;
                        let operation = declaration_container_operation(&authored.wrappers);
                        let separator_behavior = separator
                            .as_ref()
                            .map(|separator| {
                                let usage = &source.token_uses[separator.token_use.index()];
                                let definition =
                                    &source.token_definitions[usage.definition.index()];
                                match &definition.kind {
                                    TokenDefinitionKind::Fixed(fixed) => Ok((
                                        fixed_pretty_spelling(
                                            &fixed.spelling,
                                            lowered.grammar.text_equivalence,
                                        ),
                                        fixed.pretty_spacing,
                                    )),
                                    _ => Err("an authored separator references a non-fixed token"
                                        .to_owned()),
                                }
                            })
                            .transpose()?;
                        let repetition_break = authored
                            .terminal_node
                            .filter(|terminal_node| {
                                separator_behavior
                                    .as_ref()
                                    .is_some_and(|(spelling, _)| spelling == ";")
                                    && matches!(
                                        operation,
                                        ContainerOperation::ZeroOrMore
                                            | ContainerOperation::OneOrMore
                                            | ContainerOperation::OptionalOneOrMore
                                    )
                                    && matches!(
                                        resolved.nodes[terminal_node.index()].body,
                                        ResolvedNodeBody::Enum { .. }
                                    )
                            })
                            .map(PlannedRepetitionBreak::TopLevelStatementFamily)
                            .unwrap_or(PlannedRepetitionBreak::Soft);
                        SemanticParseProjection::Direct {
                            terminal: Box::new(authored.terminal),
                            terminal_node: authored.terminal_node,
                            terminal_application: None,
                            explorer_terminal: declaration_explorer_terminal(
                                lowered,
                                node.id,
                                *resolved_field,
                            )?,
                            wrappers: authored.wrappers,
                            container: Box::new(declaration_container(
                                operation,
                                separator.is_some(),
                                separator
                                    .as_ref()
                                    .is_some_and(|separator| separator.optional_trailing),
                                kind_count,
                            )),
                            capture: PlannedCapturePolicy::Inherited,
                            child_capture: PlannedCapturePolicy::Inherited,
                            pratt_min_bp: None,
                            pratt_excluded_led: Vec::new(),
                            separator: separator_behavior
                                .as_ref()
                                .map(|(spelling, _)| spelling.clone()),
                            separator_spacing: separator_behavior.map(|(_, spacing)| spacing),
                            attachment: attachment
                                .as_ref()
                                .map(|attachment| {
                                    declaration_fixed_attachment(attachment, lowered, kind_count)
                                        .map(Box::new)
                                })
                                .transpose()?,
                            opaque_provenance: authored_type_depends_on_node_generics(
                                &field.ty, node, resolved,
                            ),
                            repetition_break,
                            pretty: pretty.clone(),
                        }
                    }
                };
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::Semantic(*resolved_field),
                    metadata_name,
                    kind: ElementParseKind::Semantic {
                        field: FieldPlan {
                            ident: field.syntax.ident.clone(),
                            index: field_index,
                            accessor: Some(accessor),
                        },
                        projection: Box::new(projection),
                    },
                })
            }
        })
        .collect()
}

/// Freezes fixed tokens attached before and after a semantic field.
pub(super) fn declaration_fixed_attachment(
    attachment: &crate::source_model::FixedTokenAttachment,
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
) -> Result<FixedAttachmentParsePlan, String> {
    let plan_fixed = |token_use: crate::lowering::TokenUseId| {
        declaration_fixed_token(
            token_use,
            lowered,
            kind_count,
            PlannedCapturePolicy::Inherited,
        )
    };
    let plan_syntax = |syntax: &crate::source_model::AttachedFixedSyntax| match syntax {
        crate::source_model::AttachedFixedSyntax::Required(token) => {
            plan_fixed(token.token_use).map(AttachedFixedSyntaxParsePlan::Required)
        }
        crate::source_model::AttachedFixedSyntax::Optional { tokens, .. } => tokens
            .iter()
            .map(|token| plan_fixed(token.token_use))
            .collect::<Result<Vec<_>, _>>()
            .map(AttachedFixedSyntaxParsePlan::Optional),
    };
    Ok(FixedAttachmentParsePlan {
        before: attachment
            .before
            .iter()
            .map(plan_syntax)
            .collect::<Result<_, _>>()?,
        semantic: Box::new(PlannedContainer::Direct {
            child_follow: PlannedFollow::Caller,
            required_failure: PlannedFollow::Caller,
        }),
        after: attachment
            .after
            .iter()
            .map(plan_syntax)
            .collect::<Result<_, _>>()?,
    })
}

/// Freezes one authored fixed-token use for generated attachment or presence parsing.
fn declaration_fixed_token(
    token_use: crate::lowering::TokenUseId,
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
    capture: PlannedCapturePolicy,
) -> Result<AttachedFixedParsePlan, String> {
    let source = lowered.source_modeled.source_model();
    let usage = &source.token_uses[token_use.index()];
    let definition = &source.token_definitions[usage.definition.index()];
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("an authored attachment references non-fixed syntax".to_owned());
    };
    Ok(AttachedFixedParsePlan {
        expected: ExpectedSet::empty(kind_count),
        capture,
        spelling: fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence),
        spacing: fixed.pretty_spacing,
    })
}

/// Projects a resolved terminal into grammar-erased Explorer metadata.
pub(super) fn declaration_explorer_terminal(
    lowered: &crate::materialization::LoweredGrammar,
    node: NodeId,
    field: ResolvedFieldId,
) -> Result<ExplorerTerminalPlan, String> {
    let application = lowered
        .applications
        .iter()
        .find(|application| application.node == node)
        .ok_or_else(|| "an authored explorer field lacks a closed Node application".to_owned())?;
    let explorer = authoritative_explorer_node(lowered, application)?;
    let variants: Vec<&ExplorerVariantPlan> = match &explorer.body {
        ExplorerNodeBodyPlan::Struct(variant) => vec![variant],
        ExplorerNodeBodyPlan::Enum(variants) | ExplorerNodeBodyPlan::Pratt(variants) => {
            variants.iter().collect()
        }
    };
    variants
        .into_iter()
        .flat_map(|variant| &variant.elements)
        .find_map(|element| {
            matches!(
                element.source,
                ExplorerElementSource::Semantic(candidate)
                    | ExplorerElementSource::PrattLhs(candidate)
                    if candidate == field
            )
            .then_some(&element.occurrence)
        })
        .and_then(|occurrence| match occurrence {
            ExplorerOccurrencePlan::Children { terminal, .. } => Some(*terminal),
            ExplorerOccurrencePlan::Leaf => None,
        })
        .ok_or_else(|| "an authored explorer field lacks a materialized terminal".to_owned())
}

/// Derives the runtime collection operation from authored wrappers.
pub(super) fn declaration_container_operation(wrappers: &[FieldWrapper]) -> ContainerOperation {
    let repetition = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore));
    let optional_before_repeat = repetition.is_some_and(|index| {
        wrappers[..index]
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    });
    match repetition.map(|index| wrappers[index]) {
        Some(FieldWrapper::ZeroOrMore) => ContainerOperation::ZeroOrMore,
        Some(FieldWrapper::OneOrMore) if optional_before_repeat => {
            ContainerOperation::OptionalOneOrMore
        }
        Some(FieldWrapper::OneOrMore) => ContainerOperation::OneOrMore,
        Some(FieldWrapper::Boxed | FieldWrapper::Optional) => unreachable!(),
        None if wrappers
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional)) =>
        {
            ContainerOperation::Optional
        }
        None => ContainerOperation::Direct,
    }
}

/// Plans optional or repeated control flow and its failure evidence.
pub(super) fn declaration_container(
    operation: ContainerOperation,
    has_separator: bool,
    optional_trailing: bool,
    kind_count: usize,
) -> PlannedContainer {
    match operation {
        ContainerOperation::Direct => PlannedContainer::Direct {
            child_follow: PlannedFollow::Caller,
            required_failure: PlannedFollow::Caller,
        },
        ContainerOperation::Optional => PlannedContainer::Optional {
            selector: ExpectedSet::empty(kind_count),
            decision: None,
            child_follow: PlannedFollow::Caller,
        },
        operation @ (ContainerOperation::ZeroOrMore
        | ContainerOperation::OneOrMore
        | ContainerOperation::OptionalOneOrMore) => PlannedContainer::Repeated {
            operation,
            element_selector: ExpectedSet::empty(kind_count),
            separator_selector: has_separator.then(|| ExpectedSet::empty(kind_count)),
            optional_trailing,
            stop: PlannedFollow::Caller,
            child_follow: PlannedFollow::Caller,
            minimum_failure: None,
            continuation_failure: Box::new(PlannedFollow::Caller),
            after_separator_failure: None,
        },
    }
}

#[derive(Clone)]
/// Authored terminal type projected through its storage wrappers.
pub(super) struct AuthoredProjection {
    /// Terminal type after applying resolved generic bindings.
    pub(super) terminal: Type,
    /// Generated node family referenced by the terminal, when any.
    pub(super) terminal_node: Option<NodeId>,
    /// Authored storage wrappers, outermost first.
    pub(super) wrappers: Vec<FieldWrapper>,
}

#[derive(Clone, Default)]
/// Generic substitutions available while projecting authored types.
pub(super) struct AuthoredBindings {
    /// Type arguments keyed by their resolved parameter identities.
    pub(super) types: BTreeMap<GenericParameterId, ResolvedType>,
    /// Lifetime arguments keyed by authored parameter names.
    pub(super) lifetimes: BTreeMap<String, syn::Lifetime>,
}

/// Reports whether a type references generics owned by the enclosing node.
pub(super) fn authored_type_depends_on_node_generics(
    ty: &ResolvedType,
    node: &ResolvedNode,
    resolved: &ResolvedSourceGrammar,
) -> bool {
    match &ty.kind {
        ResolvedTypeKind::Semantic {
            use_site,
            target,
            arguments,
            ..
        } => {
            let parameter_is_owned = |parameter| {
                resolved.generic_parameters.iter().any(|candidate| {
                    candidate.id == parameter && candidate.owner == SemanticOwner::Node(node.id)
                })
            };
            matches!(target, SemanticTarget::GenericParameter(parameter) if parameter_is_owned(*parameter))
                || (matches!(target, SemanticTarget::Node(owner) if *owner == node.id)
                    && !node.generics.params.is_empty()
                    && resolved.uses[use_site.index()].authored == "Self")
                || arguments.iter().any(|argument| match argument {
                    ResolvedGenericArgument::Type(argument) => {
                        authored_type_depends_on_node_generics(argument, node, resolved)
                    }
                    ResolvedGenericArgument::Lifetime(_)
                    | ResolvedGenericArgument::Unsupported(_) => false,
                })
        }
        ResolvedTypeKind::Group(inner)
        | ResolvedTypeKind::Parenthesized(inner)
        | ResolvedTypeKind::Reference(inner) => {
            authored_type_depends_on_node_generics(inner, node, resolved)
        }
        ResolvedTypeKind::Tuple(elements) => elements
            .iter()
            .any(|element| authored_type_depends_on_node_generics(element, node, resolved)),
        ResolvedTypeKind::FixedTokenPresence | ResolvedTypeKind::Unsupported => false,
    }
}

/// Resolves an authored terminal and wrappers under the available bindings.
pub(super) fn authored_projection(
    ty: &ResolvedType,
    resolved: &ResolvedSourceGrammar,
) -> Result<AuthoredProjection, String> {
    authored_projection_with(
        ty,
        resolved,
        &AuthoredBindings::default(),
        &mut BTreeSet::new(),
    )
}

/// Resolves an authored projection after extending the current bindings.
///
/// # Long-function contract
///
/// Rationale: projects one materialized body back into authored grammar-element order.
/// Invariants: fixed tokens and semantic fields retain exact source correlations.
/// Phases: indexes lowering correlations, walks authored elements, then validates complete coverage.
pub(super) fn authored_projection_with(
    ty: &ResolvedType,
    resolved: &ResolvedSourceGrammar,
    bindings: &AuthoredBindings,
    active_aliases: &mut BTreeSet<usize>,
) -> Result<AuthoredProjection, String> {
    match &ty.kind {
        ResolvedTypeKind::Group(inner) | ResolvedTypeKind::Parenthesized(inner) => {
            authored_projection_with(inner, resolved, bindings, active_aliases)
        }
        ResolvedTypeKind::Semantic {
            use_site,
            target,
            arguments,
            ..
        } => match target {
            SemanticTarget::GenericParameter(parameter) => {
                bindings.types.get(parameter).map_or_else(
                    || {
                        Ok(AuthoredProjection {
                            terminal: ty.syntax.clone(),
                            terminal_node: None,
                            wrappers: Vec::new(),
                        })
                    },
                    |bound| authored_projection_with(bound, resolved, bindings, active_aliases),
                )
            }
            SemanticTarget::Catalog(catalog) => {
                let argument = arguments.iter().find_map(|argument| match argument {
                    ResolvedGenericArgument::Type(ty) => Some(ty),
                    ResolvedGenericArgument::Lifetime(_)
                    | ResolvedGenericArgument::Unsupported(_) => None,
                });
                let argument = argument.ok_or_else(|| {
                    format!("authored catalog type `{catalog:?}` has no type argument")
                })?;
                let mut projected =
                    authored_projection_with(argument, resolved, bindings, active_aliases)?;
                let wrapper = match catalog {
                    CatalogType::Option => FieldWrapper::Optional,
                    CatalogType::Box => FieldWrapper::Boxed,
                    CatalogType::Vec => FieldWrapper::ZeroOrMore,
                    CatalogType::Vec1 => FieldWrapper::OneOrMore,
                };
                projected.wrappers.insert(0, wrapper);
                Ok(projected)
            }
            SemanticTarget::TypeAlias(alias_id) => {
                if !active_aliases.insert(alias_id.index()) {
                    return Err("an authored type-alias projection contains a cycle".to_owned());
                }
                let alias = &resolved.type_aliases[alias_id.index()];
                let parameter_ids = resolved
                    .generic_parameters
                    .iter()
                    .filter(|parameter| parameter.owner == SemanticOwner::TypeAlias(*alias_id))
                    .map(|parameter| (parameter.ident.as_str(), parameter.id))
                    .collect::<BTreeMap<_, _>>();
                let mut supplied = arguments.iter().peekable();
                let mut alias_bindings = bindings.clone();
                for parameter in &alias.generics.params {
                    match parameter {
                        GenericParam::Lifetime(parameter) => {
                            let Some(ResolvedGenericArgument::Lifetime(argument)) = supplied.next()
                            else {
                                return Err(format!(
                                    "type alias `{}` leaves lifetime `{}` open",
                                    alias.ident, parameter.lifetime
                                ));
                            };
                            let argument = bindings
                                .lifetimes
                                .get(&argument.ident.to_string())
                                .cloned()
                                .unwrap_or_else(|| argument.clone());
                            alias_bindings
                                .lifetimes
                                .insert(parameter.lifetime.ident.to_string(), argument);
                        }
                        GenericParam::Type(parameter) => {
                            let id = parameter_ids
                                .get(parameter.ident.to_string().as_str())
                                .ok_or_else(|| {
                                    format!(
                                        "type alias `{}` lost generic parameter identity",
                                        alias.ident
                                    )
                                })?;
                            let argument = match supplied.peek() {
                                Some(ResolvedGenericArgument::Type(_)) => {
                                    let Some(ResolvedGenericArgument::Type(argument)) =
                                        supplied.next()
                                    else {
                                        unreachable!("peeked a type argument")
                                    };
                                    argument.clone()
                                }
                                Some(_) => {
                                    return Err(format!(
                                        "type alias `{}` has mismatched generic arguments",
                                        alias.ident
                                    ));
                                }
                                None => alias
                                    .defaults
                                    .iter()
                                    .find(|default| default.parameter == *id)
                                    .map(|default| default.ty.clone())
                                    .ok_or_else(|| {
                                        format!(
                                            "type alias `{}` leaves `{}` open",
                                            alias.ident, parameter.ident
                                        )
                                    })?,
                            };
                            alias_bindings.types.insert(*id, argument);
                        }
                        GenericParam::Const(_) => {
                            return Err(format!(
                                "type alias `{}` uses an unsupported const generic",
                                alias.ident
                            ));
                        }
                    }
                }
                if supplied.next().is_some() {
                    return Err(format!(
                        "type alias `{}` has excess generic arguments",
                        alias.ident
                    ));
                }
                let target = alias.target.as_ref().ok_or_else(|| {
                    format!("type alias `{}` has no resolved target", alias.ident)
                })?;
                let projected =
                    authored_projection_with(target, resolved, &alias_bindings, active_aliases);
                active_aliases.remove(&alias_id.index());
                projected
            }
            SemanticTarget::Node(node_id) => Ok(AuthoredProjection {
                terminal: authored_node_type(
                    *node_id,
                    *use_site,
                    arguments,
                    resolved,
                    bindings,
                    active_aliases,
                )?,
                terminal_node: Some(*node_id),
                wrappers: Vec::new(),
            }),
            SemanticTarget::ContentToken(_) => Ok(AuthoredProjection {
                terminal: ty.syntax.clone(),
                terminal_node: None,
                wrappers: Vec::new(),
            }),
        },
        ResolvedTypeKind::FixedTokenPresence
        | ResolvedTypeKind::Reference(_)
        | ResolvedTypeKind::Tuple(_)
        | ResolvedTypeKind::Unsupported => {
            Err("an authored semantic field has no supported declaration projection".to_owned())
        }
    }
}

/// Renders a resolved type while leaving node-owned parameters open.
pub(super) fn authored_open_type(
    ty: &ResolvedType,
    resolved: &ResolvedSourceGrammar,
    bindings: &AuthoredBindings,
    active_aliases: &mut BTreeSet<usize>,
) -> Result<Type, String> {
    let projection = authored_projection_with(ty, resolved, bindings, active_aliases)?;
    let facade = emitted_ident(&resolved.build_inputs.facade.ident)?;
    Ok(render_wrapped_type(
        &facade,
        &projection.terminal,
        &projection.wrappers,
    ))
}

/// Renders the generated node type under the supplied application bindings.
pub(super) fn authored_node_type(
    node_id: NodeId,
    use_site: crate::resolution::SemanticUseId,
    arguments: &[ResolvedGenericArgument],
    resolved: &ResolvedSourceGrammar,
    bindings: &AuthoredBindings,
    active_aliases: &mut BTreeSet<usize>,
) -> Result<Type, String> {
    if resolved.uses[use_site.index()].authored == "Self" {
        return Ok(syn::parse_quote!(Self));
    }
    let node = &resolved.nodes[node_id.index()];
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in &node.key.segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    let mut rendered = syn::punctuated::Punctuated::new();
    for argument in arguments {
        match argument {
            ResolvedGenericArgument::Lifetime(lifetime) => {
                let lifetime = bindings
                    .lifetimes
                    .get(&lifetime.ident.to_string())
                    .cloned()
                    .unwrap_or_else(|| lifetime.clone());
                rendered.push(GenericArgument::Lifetime(lifetime));
            }
            ResolvedGenericArgument::Type(ty) => {
                rendered.push(GenericArgument::Type(authored_open_type(
                    ty,
                    resolved,
                    bindings,
                    active_aliases,
                )?));
            }
            ResolvedGenericArgument::Unsupported(_) => {
                return Err(format!(
                    "Node `{}` has an unsupported authored generic argument",
                    node.ident
                ));
            }
        }
    }
    if !rendered.is_empty() {
        let segment = path
            .segments
            .last_mut()
            .ok_or_else(|| "authored Node path is empty".to_owned())?;
        segment.arguments = PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: rendered,
            gt_token: Default::default(),
        });
    }
    Ok(Type::Path(syn::TypePath { qself: None, path }))
}

/// Visits declaration elements in authored order with their construction indices.
pub(super) fn for_each_element(body: &NodeBodyPlan, visit: &mut impl FnMut(&ElementParsePlan)) {
    match body {
        NodeBodyPlan::Struct { elements, .. } => elements.iter().for_each(visit),
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .flat_map(|variant| &variant.elements)
            .for_each(visit),
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &pratt.nud {
                branch.variant.elements.iter().for_each(&mut *visit);
            }
            for branch in &pratt.led {
                visit(&branch.lhs);
                branch.variant.elements.iter().for_each(&mut *visit);
            }
        }
    }
}

/// Assigns collision-free accessors to repeated fixed-token spellings.
pub(super) fn disambiguate_fixed_accessors(body: &mut NodeBodyPlan) -> Result<(), String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for_each_element(body, &mut |element| {
        let name = match &element.kind {
            ElementParseKind::Fixed { accessor, .. } => Some(accessor.unraw().to_string()),
            ElementParseKind::OptionalFixedTokenGroup { accessor, .. } => {
                Some(accessor.unraw().to_string())
            }
            ElementParseKind::Semantic { field, .. } => field
                .accessor
                .as_ref()
                .map(|accessor| accessor.unraw().to_string()),
        };
        if let Some(name) = name {
            *counts.entry(name).or_default() += 1;
        }
    });
    let mut occupied = counts
        .iter()
        .filter_map(|(name, count)| (*count == 1).then_some(name.clone()))
        .collect::<BTreeSet<_>>();

    fn qualify(
        elements: &mut [ElementParsePlan],
        variant: Option<&Ident>,
        counts: &BTreeMap<String, usize>,
        occupied: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        for (index, element) in elements.iter_mut().enumerate() {
            let accessor = match &mut element.kind {
                ElementParseKind::Fixed { accessor, .. }
                | ElementParseKind::OptionalFixedTokenGroup { accessor, .. } => accessor,
                ElementParseKind::Semantic { .. } => {
                    continue;
                }
            };
            let original = accessor.unraw().to_string();
            if counts.get(&original).copied().unwrap_or(0) <= 1 {
                continue;
            }
            let prefix = variant.map_or_else(
                || "fixed".to_owned(),
                |variant| snake_case(&variant.unraw().to_string()),
            );
            let mut candidate = format!("{prefix}_{original}");
            if occupied.contains(&candidate) {
                candidate = format!("{prefix}_element_{index}_{original}");
            }
            let mut suffix = 2;
            while occupied.contains(&candidate) {
                candidate = format!("{prefix}_element_{index}_{suffix}_{original}");
                suffix += 1;
            }
            *accessor = emitted_ident(&candidate)?;
            occupied.insert(candidate);
        }
        Ok(())
    }

    match body {
        NodeBodyPlan::Struct { elements, .. } => qualify(elements, None, &counts, &mut occupied)?,
        NodeBodyPlan::Enum { variants, .. } => {
            for variant in variants {
                qualify(
                    &mut variant.elements,
                    Some(&variant.ident),
                    &counts,
                    &mut occupied,
                )?;
            }
        }
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &mut pratt.nud {
                qualify(
                    &mut branch.variant.elements,
                    Some(&branch.variant.ident),
                    &counts,
                    &mut occupied,
                )?;
            }
            for branch in &mut pratt.led {
                qualify(
                    &mut branch.variant.elements,
                    Some(&branch.variant.ident),
                    &counts,
                    &mut occupied,
                )?;
            }
        }
    }
    Ok(())
}

/// Collects the recursive Pratt operands already frozen into one branch body.
fn planned_pratt_operands(elements: &[ElementParsePlan]) -> Vec<PrattOperandParsePlan> {
    elements
        .iter()
        .enumerate()
        .filter_map(|(element_index, element)| {
            let ElementParseKind::Semantic {
                projection: semantic,
                ..
            } = &element.kind
            else {
                return None;
            };
            let SemanticParseProjection::Direct { pratt_min_bp, .. } = semantic.as_ref() else {
                return None;
            };
            pratt_min_bp.map(|minimum_binding_power| PrattOperandParsePlan {
                element_index,
                minimum_binding_power,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
/// Freezes Pratt prefix/extender roles, dispatch, guards, and failure evidence.
///
/// # Long-function contract
///
/// Rationale: constructs the executable Pratt plan from frozen nud, led, binding-power, and failure products.
/// Invariants: led recursion is the only recursive path and every branch consumes authoritative analysis.
/// Phases: plans nud alternatives, led variants, operands and failures, then validates coverage.
pub(super) fn plan_pratt_node(
    node: &ResolvedNode,
    application: &MaterializedNodeApplication,
    application_type: crate::TypeId,
    materialized: &[MaterializedPrattVariant],
    resolved_variants: &[crate::resolution::ResolvedNodeVariant],
    semantic_variants: &[crate::PrattVariant],
    lowered: &crate::materialization::LoweredGrammar,
    lookups: &EmissionLookupIndex<'_>,
    input_lifetime: &syn::Lifetime,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<PrattParsePlan, String> {
    let atom_index = lookups.atom_index;
    let kind_count = lookups.kind_count;
    let nud_dispatch = lookups
        .dispatch(application_type, DispatchKind::PrattNud)
        .ok_or_else(|| format!("Pratt Node `{}` lacks its nud dispatch plan", node.ident))?;
    let led_dispatch = lookups.dispatch(application_type, DispatchKind::PrattLed);

    let nud_failure = lookups
        .owner_failure(application_type, FailureEdgeKind::PrattNudDispatch)
        .ok_or_else(|| format!("Pratt Node `{}` lacks its nud failure edge", node.ident))?;
    let nud_failure = planned_expected_recipe(nud_failure.expected(), atom_index, kind_count)?;
    let guard = lookups.owner_failure(application_type, FailureEdgeKind::PrattCallerFollowGuard);
    let guard_branches = match guard.map(|edge| edge.expected()) {
        Some(ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches }) => Some(branches),
        Some(_) => return Err("a Pratt caller-FOLLOW guard has the wrong recipe".to_owned()),
        None => None,
    };

    let mut nud = Vec::new();
    let mut led = Vec::new();
    let mut nud_by_declaration = Vec::new();
    let mut led_by_declaration = Vec::new();
    for (source_index, variant) in materialized.iter().enumerate() {
        let common = match variant {
            MaterializedPrattVariant::Nud(nud) => &nud.common,
            MaterializedPrattVariant::Led(led) => &led.common,
        };
        let resolved = resolved_variants
            .iter()
            .find(|candidate| candidate.id == common.resolved_variant)
            .ok_or_else(|| "materialized Pratt variant lacks source shape".to_owned())?;
        let correlation = lowered
            .lowering_map
            .variants
            .iter()
            .find(|correlation| {
                correlation.application == application.id
                    && correlation.variant == common.resolved_variant
            })
            .ok_or_else(|| "materialized Pratt variant lacks lowering correlation".to_owned())?;
        let semantic = semantic_variants
            .iter()
            .find(|variant| variant.declaration == correlation.lowered)
            .ok_or_else(|| "materialized Pratt variant lacks frozen semantic sites".to_owned())?;
        match variant {
            MaterializedPrattVariant::Nud(materialized_nud) => {
                let branch = nud_dispatch
                    .branches()
                    .iter()
                    .find(|branch| branch.declaration() == correlation.lowered)
                    .ok_or_else(|| "materialized Pratt nud lacks dispatch branch".to_owned())?;
                validate_pratt_position(branch.position(), materialized_nud)?;
                let position = branch
                    .position()
                    .ok_or_else(|| "a frozen Pratt nud branch lacks a position".to_owned())?;
                let operand = match position {
                    PrattPosition::Prefix { bp } => Some((application_type, bp)),
                    PrattPosition::Atom => None,
                    PrattPosition::Infix { .. } | PrattPosition::Postfix { .. } => {
                        return Err("a frozen nud branch has a led position".to_owned());
                    }
                };
                let elements = plan_elements(
                    node,
                    Some(&MaterializedVariant {
                        resolved_variant: common.resolved_variant,
                        name: common.name.clone(),
                        elements: materialized_nud.body.clone(),
                        pretty: common.pretty.clone(),
                    }),
                    &materialized_nud.body,
                    &semantic.sites,
                    lowered,
                    lookups,
                    input_lifetime,
                    operand,
                    None,
                )?;
                let operands = planned_pratt_operands(&elements);
                nud_by_declaration.push((branch.declaration(), nud.len()));
                nud.push(PrattNudParsePlan {
                    source_index,
                    variant: VariantParsePlan {
                        resolved_variant: common.resolved_variant,
                        ident: emitted_ident(&common.name)?,
                        style: resolved.style,
                        elements,
                        capture: resolved.capture.map(PlannedCapturePolicy::from),
                        pretty: common.pretty.clone(),
                    },
                    position,
                    operands,
                });
            }
            MaterializedPrattVariant::Led(materialized_led) => {
                let dispatch = led_dispatch.ok_or_else(|| {
                    format!("Pratt Node `{}` lacks its led dispatch plan", node.ident)
                })?;
                let branch = dispatch
                    .branches()
                    .iter()
                    .find(|branch| branch.declaration() == correlation.lowered)
                    .ok_or_else(|| "materialized Pratt led lacks dispatch branch".to_owned())?;
                validate_pratt_led_position(branch.position(), materialized_led)?;
                let position = branch
                    .position()
                    .ok_or_else(|| "a frozen Pratt led branch lacks a position".to_owned())?;
                let operand = match position {
                    PrattPosition::Infix { rbp, .. } => Some((application_type, rbp)),
                    PrattPosition::Postfix { bp } => Some((application_type, bp)),
                    PrattPosition::Atom | PrattPosition::Prefix { .. } => {
                        return Err("a frozen led branch has a nud position".to_owned());
                    }
                };
                let synthetic_variant = MaterializedVariant {
                    resolved_variant: common.resolved_variant,
                    name: common.name.clone(),
                    elements: materialized_led.tail.clone(),
                    pretty: common.pretty.clone(),
                };
                let elements = plan_elements(
                    node,
                    Some(&synthetic_variant),
                    &materialized_led.tail,
                    &semantic.sites,
                    lowered,
                    lookups,
                    input_lifetime,
                    operand,
                    Some(application_type),
                )?;
                let operands = planned_pratt_operands(&elements);
                led_by_declaration.push((branch.declaration(), led.len()));
                led.push(PrattLedParsePlan {
                    source_index,
                    variant: VariantParsePlan {
                        resolved_variant: common.resolved_variant,
                        ident: emitted_ident(&common.name)?,
                        style: resolved.style,
                        elements,
                        capture: resolved.capture.map(PlannedCapturePolicy::from),
                        pretty: common.pretty.clone(),
                    },
                    lhs: plan_pratt_lhs(node, resolved, materialized_led, lowered, input_lifetime)?,
                    position,
                    operands,
                });
            }
        }
    }

    let nud_dispatch = plan_joined_dispatch(
        nud_dispatch,
        &nud_by_declaration,
        lowered,
        atom_index,
        kind_count,
        instrumentation,
    )?;
    let led_dispatch = led_dispatch
        .map(|dispatch| {
            plan_joined_dispatch(
                dispatch,
                &led_by_declaration,
                lowered,
                atom_index,
                kind_count,
                instrumentation,
            )
        })
        .transpose()?;
    let nud_dispatch_root = &nud_dispatch.states[nud_dispatch.root.index()];
    let mut nud_dispatch_expected = ExpectedSet::empty(kind_count);
    for edge in &nud_dispatch_root.edges {
        nud_dispatch_expected.union_assign(&edge.position.expected);
    }
    let (nud_failure_expected, nud_failure_uses_caller) = match &nud_failure {
        PlannedFollow::Caller => (ExpectedSet::empty(kind_count), true),
        PlannedFollow::Static(expected) => (expected.clone(), false),
        PlannedFollow::StaticAndCaller(expected) | PlannedFollow::CallerAndPrattStops(expected) => {
            (expected.clone(), true)
        }
        PlannedFollow::Accepting { .. } => {
            return Err("a Pratt nud failure edge never carries a greedy acceptance".to_owned());
        }
    };
    if nud_dispatch_expected != nud_failure_expected
        || nud_dispatch.states[nud_dispatch.root.index()]
            .target
            .is_some_and(|target| matches!(target.mode, crate::DispatchTerminalMode::CallerFollow))
            != nud_failure_uses_caller
    {
        return Err("the frozen Pratt nud dispatch and failure products disagree".to_owned());
    }
    let caller_follow_guard = match (guard_branches, &led_dispatch) {
        (Some(branches), Some(dispatch)) => {
            let frozen_led = lookups
                .dispatch(application_type, DispatchKind::PrattLed)
                .expect("the joined led dispatch came from frozen decisions");
            if branches.as_slice() != frozen_led.branches() {
                return Err("the frozen Pratt guard and led dispatch branches disagree".to_owned());
            }
            let _ = dispatch;
            Some(
                branches
                    .iter()
                    .map(|branch| {
                        let position = branch.position().ok_or_else(|| {
                            "a frozen Pratt guard branch lacks a position".to_owned()
                        })?;
                        let first = branch
                            .sequences()
                            .iter()
                            .filter_map(|sequence| sequence.positions().first())
                            .map(|position| FirstPosition {
                                kinds: position.kinds().clone(),
                                text_predicate: None,
                            })
                            .collect::<Vec<_>>();
                        let expected = ExpectedSet::from_positions(&first, atom_index, kind_count)?;
                        if expected.is_empty() {
                            return Err("a frozen Pratt guard branch is empty".to_owned());
                        }
                        Ok(PrattGuardBranchPlan { position, expected })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )
        }
        (None, None) => None,
        _ => return Err("the frozen Pratt led dispatch and guard family disagree".to_owned()),
    };

    let mut nud_selection_follow = ExpectedSet::empty(kind_count);
    for branch in caller_follow_guard.iter().flatten() {
        nud_selection_follow.union_assign(&branch.expected);
    }

    Ok(PrattParsePlan {
        expected_nud_count: semantic_variants
            .iter()
            .filter(|variant| {
                matches!(
                    variant.position,
                    PrattPosition::Atom | PrattPosition::Prefix { .. }
                )
            })
            .count(),
        expected_led_count: semantic_variants
            .iter()
            .filter(|variant| {
                matches!(
                    variant.position,
                    PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
                )
            })
            .count(),
        nud,
        led,
        nud_dispatch,
        led_dispatch,
        nud_failure,
        nud_selection_follow,
        caller_follow_guard,
    })
}

/// Reconstructs and validates a dispatch plan shared by the supplied variants.
pub(super) fn plan_joined_dispatch(
    dispatch: &crate::DispatchPlan,
    variants: &[(crate::DeclId, usize)],
    lowered: &crate::materialization::LoweredGrammar,
    atom_index: &ExpectedAtomIndex,
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<PredictiveDispatchPlan, String> {
    let (root, states) = plan_predictive_dispatch(
        dispatch.trie(),
        variants,
        atom_index,
        kind_count,
        instrumentation,
    )?;
    let frozen_node_count = states.len();
    let balanced = dispatch
        .balanced()
        .map(|balanced| {
            let (after_close_root, after_close_states) = plan_predictive_dispatch(
                balanced.after_close(),
                variants,
                atom_index,
                kind_count,
                instrumentation,
            )?;
            let open = ExpectedSet::from_positions(
                &[FirstPosition {
                    kinds: KindSet::singleton(lowered.grammar.kind_count, balanced.open())
                        .map_err(|error| error.to_string())?,
                    text_predicate: None,
                }],
                atom_index,
                kind_count,
            )?;
            let close = ExpectedSet::from_positions(
                &[FirstPosition {
                    kinds: KindSet::singleton(lowered.grammar.kind_count, balanced.close())
                        .map_err(|error| error.to_string())?,
                    text_predicate: None,
                }],
                atom_index,
                kind_count,
            )?;
            let frozen_node_count = after_close_states.len();
            Ok::<BalancedDispatchPlan, String>(BalancedDispatchPlan {
                open,
                close,
                after_close_root,
                frozen_identity: predictive_dispatch_identity(
                    after_close_root,
                    &after_close_states,
                ),
                after_close_states,
                frozen_node_count,
            })
        })
        .transpose()?;
    Ok(PredictiveDispatchPlan {
        frozen_identity: predictive_dispatch_identity(root, &states),
        root,
        states,
        frozen_sequence_count: dispatch
            .branches()
            .iter()
            .map(|branch| branch.sequences().len())
            .sum(),
        frozen_node_count,
        balanced,
    })
}

/// Checks that a frozen Pratt prefix position is valid for its role.
pub(super) fn validate_pratt_position(
    position: Option<PrattPosition>,
    variant: &MaterializedPrattNud,
) -> Result<(), String> {
    let expected = match variant.position {
        MaterializedPrattNudPosition::Atom => PrattPosition::Atom,
        MaterializedPrattNudPosition::Prefix { bp } => PrattPosition::Prefix { bp },
    };
    if position == Some(expected) {
        Ok(())
    } else {
        Err("the frozen Pratt nud position disagrees with materialized source".to_owned())
    }
}

/// Checks that an extender position and left-operand slot agree.
pub(super) fn validate_pratt_led_position(
    position: Option<PrattPosition>,
    variant: &MaterializedPrattLed,
) -> Result<(), String> {
    let expected = match variant.position {
        MaterializedPrattLedPosition::Infix { lbp, rbp } => PrattPosition::Infix { lbp, rbp },
        MaterializedPrattLedPosition::Postfix { bp } => PrattPosition::Postfix { bp },
    };
    if position == Some(expected) {
        Ok(())
    } else {
        Err("the frozen Pratt led position disagrees with materialized source".to_owned())
    }
}

/// Plans the distinguished left-operand element of a Pratt extender.
pub(super) fn plan_pratt_lhs(
    node: &ResolvedNode,
    resolved_variant: &crate::resolution::ResolvedNodeVariant,
    led: &MaterializedPrattLed,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<ElementParsePlan, String> {
    let resolved_field = node
        .fields
        .iter()
        .find(|candidate| candidate.id == led.lhs.field)
        .ok_or_else(|| "materialized Pratt lhs lacks source syntax".to_owned())?;
    let field_index = resolved_variant
        .fields
        .iter()
        .position(|candidate| *candidate == led.lhs.field)
        .ok_or_else(|| "materialized Pratt lhs is absent from its source variant".to_owned())?;
    let base = resolved_field
        .syntax
        .ident
        .clone()
        .unwrap_or_else(|| format_ident!("field_{field_index}"));
    let variant_name = snake_case(&resolved_variant.ident);
    let accessor = if base.unraw() == variant_name {
        base.clone()
    } else {
        format_ident!("{}_{}", variant_name, base)
    };
    let variant_capture = resolved_variant
        .capture
        .map(PlannedCapturePolicy::from)
        .or_else(|| node.capture.map(PlannedCapturePolicy::from))
        .unwrap_or(PlannedCapturePolicy::Inherited);
    let field_override = resolved_field.capture.map(PlannedCapturePolicy::from);
    let closed = ClosedType::Boxed(Box::new(ClosedType::Node(led.lhs.owner.clone())));
    Ok(ElementParsePlan {
        explorer_source: ExplorerElementSource::PrattLhs(led.lhs.field),
        metadata_name: format!("{}_{}", variant_name, base.unraw()),
        kind: ElementParseKind::Semantic {
            field: FieldPlan {
                ident: resolved_field.syntax.ident.clone(),
                index: field_index,
                accessor: Some(accessor),
            },
            projection: Box::new(SemanticParseProjection::Direct {
                terminal: Box::new(terminal_type(&closed, lowered, input_lifetime)?),
                terminal_node: Some(node.id),
                terminal_application: Some(led.lhs.owner.application),
                explorer_terminal: ExplorerTerminalPlan::Node(led.lhs.owner.application),
                wrappers: vec![FieldWrapper::Boxed],
                container: Box::new(PlannedContainer::Direct {
                    child_follow: PlannedFollow::Caller,
                    required_failure: PlannedFollow::Caller,
                }),
                capture: field_override.unwrap_or(variant_capture),
                child_capture: field_override.unwrap_or(variant_capture),
                pratt_min_bp: None,
                pratt_excluded_led: Vec::new(),
                separator: None,
                separator_spacing: None,
                attachment: None,
                opaque_provenance: false,
                repetition_break: PlannedRepetitionBreak::Soft,
                pretty: led.lhs.pretty.clone(),
            }),
        },
    })
}

#[allow(clippy::too_many_arguments)]
/// Freezes element parsing recipes in authored field order.
///
/// # Long-function contract
///
/// Rationale: plans a source-ordered element sequence as one capture and provenance unit.
/// Invariants: container wrappers, attachments, FOLLOW recipes, and cursor widths remain aligned.
/// Phases: correlates sites, plans semantic/fixed elements, attaches separators, then computes layout.
pub(super) fn plan_elements(
    node: &ResolvedNode,
    variant: Option<&MaterializedVariant>,
    elements: &[MaterializedElement],
    sites: &[SiteId],
    lowered: &crate::materialization::LoweredGrammar,
    lookups: &EmissionLookupIndex<'_>,
    input_lifetime: &syn::Lifetime,
    pratt_operand: Option<(crate::TypeId, u8)>,
    pratt_continuation: Option<crate::TypeId>,
) -> Result<Vec<ElementParsePlan>, String> {
    if elements.len() != sites.len() {
        return Err(format!(
            "Node `{}` has mismatched source elements and normalized parse sites",
            node.ident
        ));
    }
    let node_capture = node
        .capture
        .map(PlannedCapturePolicy::from)
        .unwrap_or(PlannedCapturePolicy::Inherited);
    let variant_override = variant
        .and_then(|variant| match &node.body {
            ResolvedNodeBody::Enum { variants } => variants
                .iter()
                .find(|candidate| candidate.id == variant.resolved_variant)
                .and_then(|variant| variant.capture),
            ResolvedNodeBody::Struct { .. } => None,
        })
        .map(PlannedCapturePolicy::from);
    let variant_capture = variant_override.unwrap_or(node_capture);
    let mut semantic_index = 0;
    let mut planned = elements
        .iter()
        .zip(sites)
        .map(|(element, site)| match element {
            MaterializedElement::FixedToken { token_use } => {
                let source_model = lowered.source_modeled.source_model();
                let token_use_id = *token_use;
                let token_use = &source_model.token_uses[token_use.index()];
                let definition = &source_model.token_definitions[token_use.definition.index()];
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return Err("a fixed element did not reference fixed-token syntax".to_owned());
                };
                let PlannedFollow::Static(expected) = planned_failure_edge(
                    lookups,
                    *site,
                    FailureEdgeKind::RequiredSite,
                )?
                else {
                    return Err("a fixed element has a dynamic failure recipe".to_owned());
                };
                let name = fixed_element_name(node, token_use, definition, fixed);
                let accessor = fixed_accessor_ident(&name)?;
                let capture = fixed_element_field(node, token_use)
                    .and_then(|field| field.capture)
                    .map(PlannedCapturePolicy::from)
                    .unwrap_or(variant_capture);
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::Fixed(token_use_id),
                    metadata_name: name,
                    kind: ElementParseKind::Fixed {
                        kind: explorer_kind_index(lowered, definition.id)?,
                        expected,
                        capture,
                        spelling: fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        ),
                        spacing: fixed.pretty_spacing,
                        accessor,
                    },
                })
            }
            MaterializedElement::OptionalFixedTokenGroup {
                element, tokens, ..
            } => {
                let correlation = lowered
                    .lowering_map
                    .optional_fixed_groups
                    .iter()
                    .find(|correlation| correlation.outer == *site)
                    .ok_or_else(|| {
                        "an optional fixed group lacks its normalized correlation".to_owned()
                    })?;
                if correlation.element != *element
                    || correlation.sequence_sites.len() != tokens.len()
                {
                    return Err("an optional fixed group has mismatched normalized sites".to_owned());
                }
                let machine = lookups
                    .container(*site)
                    .ok_or_else(|| "an optional fixed group lacks its optional machine".to_owned())?;
                if machine.operation() != ContainerOperation::Optional {
                    return Err("an optional fixed group did not normalize as optional".to_owned());
                }
                let source = lowered.source_modeled.source_model();
                let metadata_name = format!(
                    "optional_{}",
                    tokens
                        .iter()
                        .map(|token_use| {
                            let usage = &source.token_uses[token_use.index()];
                            snake_case(&source.token_definitions[usage.definition.index()].name)
                        })
                        .collect::<Vec<_>>()
                        .join("_")
                );
                let tokens = tokens
                    .iter()
                    .zip(&correlation.sequence_sites)
                    .map(|(token_use, token_site)| {
                        let usage = &source.token_uses[token_use.index()];
                        let definition = &source.token_definitions[usage.definition.index()];
                        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                            return Err("an optional group references non-fixed syntax".to_owned());
                        };
                        let PlannedFollow::Static(expected) = planned_failure_edge(
                            lookups,
                            *token_site,
                            FailureEdgeKind::RequiredSite,
                        )? else {
                            return Err("an optional group token has a dynamic failure recipe".to_owned());
                        };
                        Ok(AttachedFixedParsePlan {
                            expected,
                            capture: PlannedCapturePolicy::Mandatory,
                            spelling: fixed_pretty_spelling(
                                &fixed.spelling,
                                lowered.grammar.text_equivalence,
                            ),
                            spacing: fixed.pretty_spacing,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::OptionalFixed(*element),
                    metadata_name: metadata_name.clone(),
                    kind: ElementParseKind::OptionalFixedTokenGroup {
                        tokens,
                        accessor: fixed_accessor_ident(&metadata_name)?,
                    },
                })
            }
            MaterializedElement::SemanticField {
                field,
                projection: MaterializedSemanticFieldProjection::FixedPresence { tokens },
                attachment,
                pretty,
            } => {
                let resolved_field = node
                    .fields
                    .iter()
                    .find(|candidate| candidate.id == *field)
                    .ok_or_else(|| "materialized presence field lacks source syntax".to_owned())?;
                let field_index = variant.map_or_else(
                    || {
                        node.fields
                            .iter()
                            .position(|candidate| candidate.id == *field)
                            .unwrap_or(semantic_index)
                    },
                    |variant| {
                        let resolved_variant = match &node.body {
                            ResolvedNodeBody::Enum { variants } => variants
                                .iter()
                                .find(|candidate| candidate.id == variant.resolved_variant),
                            ResolvedNodeBody::Struct { .. } => None,
                        };
                        resolved_variant
                            .and_then(|variant| {
                                variant.fields.iter().position(|candidate| candidate == field)
                            })
                            .unwrap_or(semantic_index)
                    },
                );
                semantic_index += 1;
                let base = resolved_field
                    .syntax
                    .ident
                    .clone()
                    .unwrap_or_else(|| format_ident!("field_{field_index}"));
                let metadata_name = variant.map_or_else(
                    || base.unraw().to_string(),
                    |variant| format!("{}_{}", snake_case(&variant.name), base.unraw()),
                );
                let accessor = variant.map_or(base.clone(), |variant| {
                    let variant_name = snake_case(&variant.name);
                    if base.unraw() == variant_name {
                        base.clone()
                    } else {
                        format_ident!("{}_{}", variant_name, base)
                    }
                });
                let correlation = lowered
                    .lowering_map
                    .presences
                    .iter()
                    .find(|correlation| correlation.outer == *site)
                    .ok_or_else(|| "a fixed presence lacks its normalized correlation".to_owned())?;
                let before_uses = attachment
                    .as_ref()
                    .map_or(&[][..], |attachment| attachment.before.as_slice());
                let after_uses = attachment
                    .as_ref()
                    .map_or(&[][..], |attachment| attachment.after.as_slice());
                if correlation.presence_index != before_uses.len()
                    || correlation.envelope_sites.len()
                        != before_uses.len() + 1 + after_uses.len()
                    || correlation.sequence_sites.len() != tokens.len()
                {
                    return Err("a fixed presence has mismatched normalized sequence sites".to_owned());
                }
                let capture = resolved_field
                    .capture
                    .map(PlannedCapturePolicy::from)
                    .unwrap_or(variant_capture);
                let plan_fixed = |token_use: crate::lowering::TokenUseId,
                                  site: SiteId|
                 -> Result<AttachedFixedParsePlan, String> {
                    let source = lowered.source_modeled.source_model();
                    let usage = &source.token_uses[token_use.index()];
                    let definition = &source.token_definitions[usage.definition.index()];
                    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                        return Err("a presence sequence references non-fixed syntax".to_owned());
                    };
                    let PlannedFollow::Static(expected) = planned_failure_edge(
                        lookups,
                        site,
                        FailureEdgeKind::RequiredSite,
                    )?
                    else {
                        return Err("a presence token has a dynamic failure recipe".to_owned());
                    };
                    Ok(AttachedFixedParsePlan {
                        expected,
                        capture,
                        spelling: fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        ),
                        spacing: fixed.pretty_spacing,
                    })
                };
                let before = before_uses
                    .iter()
                    .zip(&correlation.envelope_sites[..correlation.presence_index])
                    .map(|(syntax, site)| {
                        plan_materialized_attached_syntax(
                            syntax,
                            *site,
                            lowered,
                            lookups,
                            capture,
                        )
                    })
                    .collect::<Result<_, _>>()?;
                let sequence = tokens
                    .iter()
                    .zip(&correlation.sequence_sites)
                    .map(|(token, site)| plan_fixed(*token, *site))
                    .collect::<Result<_, _>>()?;
                let after = after_uses
                    .iter()
                    .zip(&correlation.envelope_sites[correlation.presence_index + 1..])
                    .map(|(syntax, site)| {
                        plan_materialized_attached_syntax(
                            syntax,
                            *site,
                            lowered,
                            lookups,
                            capture,
                        )
                    })
                    .collect::<Result<_, _>>()?;
                let presence_site = if correlation.outer_is_optional {
                    correlation.outer
                } else {
                    correlation.envelope_sites[correlation.presence_index]
                };
                let machine = lookups
                    .container(presence_site)
                    .ok_or_else(|| "a fixed presence lacks its optional container machine".to_owned())?;
                if machine.operation() != ContainerOperation::Optional {
                    return Err("a fixed presence did not normalize as optional".to_owned());
                }
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::Semantic(*field),
                    metadata_name,
                    kind: ElementParseKind::Semantic {
                        field: FieldPlan {
                            ident: resolved_field.syntax.ident.clone(),
                            index: field_index,
                            accessor: Some(accessor),
                        },
                        projection: Box::new(SemanticParseProjection::FixedPresence {
                            before,
                            sequence,
                            after,
                            pretty: pretty.clone(),
                        }),
                    },
                })
            }
            MaterializedElement::SemanticField {
                field,
                projection:
                    MaterializedSemanticFieldProjection::Direct {
                        ty,
                        separator,
                        pratt_extensions,
                        ..
                    },
                attachment,
                pretty,
            } => {
                let resolved_field = node
                    .fields
                    .iter()
                    .find(|candidate| candidate.id == *field)
                    .ok_or_else(|| "materialized semantic field lacks source syntax".to_owned())?;
                let field_index = variant.map_or_else(
                    || {
                        node.fields
                            .iter()
                            .position(|candidate| candidate.id == *field)
                            .unwrap_or(semantic_index)
                    },
                    |variant| {
                        let resolved_variant = match &node.body {
                            ResolvedNodeBody::Enum { variants } => variants
                                .iter()
                                .find(|candidate| candidate.id == variant.resolved_variant),
                            ResolvedNodeBody::Struct { .. } => None,
                        };
                        resolved_variant
                            .and_then(|variant| {
                                variant
                                    .fields
                                    .iter()
                                    .position(|candidate| candidate == field)
                            })
                            .unwrap_or(semantic_index)
                    },
                );
                semantic_index += 1;
                let field_name = resolved_field
                    .syntax
                    .ident
                    .as_ref()
                    .map(|ident| ident.unraw().to_string())
                    .unwrap_or_else(|| format!("field_{field_index}"));
                let metadata_name = variant.map_or_else(
                    || field_name.clone(),
                    |variant| format!("{}_{}", snake_case(&variant.name), field_name),
                );
                let wrappers = field_wrappers(ty)?;
                let field_override = resolved_field.capture.map(PlannedCapturePolicy::from);
                let capture = field_override.unwrap_or(variant_capture);
                let child_capture = field_override
                    .or(variant_override)
                    .or(terminal_node_capture(ty, lowered).map(PlannedCapturePolicy::from))
                    .unwrap_or(node_capture);
                let machine = lookups
                    .container(*site)
                    .ok_or_else(|| format!("field `{field_name}` lacks its container machine"))?;
                validate_container_wrappers(machine.operation(), &wrappers, &field_name)?;
                let mut container = planned_container(machine, lookups)?;
                let pratt_min_bp = pratt_operand
                    .filter(|(owner, _)| parse_site_targets(&lowered.grammar, *site, *owner))
                    .map(|(_, min_bp)| min_bp);
                let pratt_extenders = if let Some((owner, min_bp)) =
                    pratt_operand.filter(|_| pratt_min_bp.is_some())
                {
                    let (active, inactive) =
                        lookups.pratt_extenders_at_min_bp(owner, min_bp)?;
                    match &mut container {
                        PlannedContainer::Direct { child_follow, .. } => {
                            *child_follow =
                                pratt_operand_follow(child_follow, &active, &inactive);
                        }
                        PlannedContainer::Optional { .. } | PlannedContainer::Repeated { .. } => {
                            return Err(format!(
                                "Pratt operand field `{field_name}` is not a direct frozen container"
                            ));
                        }
                    }
                    Some((active, inactive))
                } else {
                    None
                };
                let separator_behavior = separator
                    .as_ref()
                    .map(|separator| {
                        let source = lowered.source_modeled.source_model();
                        let token_use = &source.token_uses[separator.token_use.index()];
                        let definition = &source.token_definitions[token_use.definition.index()];
                        match &definition.kind {
                            TokenDefinitionKind::Fixed(fixed) => Ok((
                                fixed_pretty_spelling(
                                    &fixed.spelling,
                                    lowered.grammar.text_equivalence,
                                ),
                                fixed.pretty_spacing,
                            )),
                            _ => Err("a repetition separator is not fixed syntax".to_owned()),
                        }
                    })
                    .transpose()?;
                let terminal_application = closed_terminal_application(ty);
                let pratt_excluded_led = planned_pratt_exclusions(
                    lowered,
                    terminal_application,
                    pratt_extensions.as_ref(),
                    &field_name,
                )?;
                let terminal_node = terminal_application.and_then(|application| {
                    lowered
                        .applications
                        .iter()
                        .find(|candidate| candidate.id == application)
                        .map(|candidate| candidate.node)
                });
                let repetition_break = terminal_application
                    .filter(|application| {
                        separator_behavior
                            .as_ref()
                            .is_some_and(|(spelling, _)| spelling == ";")
                            && matches!(container, PlannedContainer::Repeated { .. })
                            && lowered.applications.iter().any(|candidate| {
                                candidate.id == *application
                                    && matches!(candidate.body, MaterializedNodeBody::Enum(_))
                            })
                    })
                    .map(PlannedRepetitionBreak::TopLevelStatement)
                    .unwrap_or(PlannedRepetitionBreak::Soft);
                let mut attachment = attachment
                    .as_ref()
                    .map(|attachment| {
                        plan_fixed_attachment(
                            resolved_field,
                            attachment,
                            *site,
                            lowered,
                            lookups,
                            variant_capture,
                        )
                        .map(Box::new)
                    })
                    .transpose()?;
                if let (Some((active, inactive)), Some(attachment)) =
                    (pratt_extenders.as_ref(), attachment.as_mut())
                {
                    let PlannedContainer::Direct { child_follow, .. } =
                        attachment.semantic.as_mut()
                    else {
                        return Err(format!(
                            "Pratt operand attachment `{field_name}` is not a direct frozen container"
                        ));
                    };
                    *child_follow = pratt_operand_follow(child_follow, active, inactive);
                }
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::Semantic(*field),
                    metadata_name,
                    kind: ElementParseKind::Semantic {
                        field: FieldPlan {
                            ident: resolved_field.syntax.ident.clone(),
                            index: field_index,
                            accessor: Some({
                                let base = resolved_field
                                    .syntax
                                    .ident
                                    .clone()
                                    .unwrap_or_else(|| format_ident!("field_{field_index}"));
                                variant.map_or(base.clone(), |variant| {
                                    let variant_name = snake_case(&variant.name);
                                    if base.unraw() == variant_name {
                                        base
                                    } else {
                                        format_ident!("{}_{}", variant_name, base)
                                    }
                                })
                            }),
                        },
                        projection: Box::new(SemanticParseProjection::Direct {
                            terminal: Box::new(terminal_type(ty, lowered, input_lifetime)?),
                            terminal_node,
                            terminal_application,
                            explorer_terminal: explorer_terminal(ty)?,
                            wrappers,
                            container: Box::new(container),
                            capture,
                            child_capture,
                            pratt_min_bp,
                            pratt_excluded_led,
                            separator: separator_behavior
                                .as_ref()
                                .map(|(spelling, _)| spelling.clone()),
                            separator_spacing: separator_behavior.map(|(_, spacing)| spacing),
                            attachment,
                            opaque_provenance: false,
                            repetition_break,
                            pretty: pretty.clone(),
                        }),
                    },
                })
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    if let (Some(owner), Some(element)) = (pratt_continuation, planned.last_mut()) {
        let extenders = lookups.pratt_extenders(owner)?;
        add_pratt_continuation_follow(element, &extenders);
    }
    Ok(planned)
}

/// Resolves the generated node and application targeted by a parse site.
pub(super) fn parse_site_targets(
    grammar: &crate::GrammarDescription,
    site: SiteId,
    target: crate::TypeId,
) -> bool {
    let site = &grammar.sites[site.index()];
    if site.target == target {
        return true;
    }
    let TypeShape::Sequence { sites } = &grammar.types[site.target.index()].shape else {
        return false;
    };
    sites
        .iter()
        .any(|site| grammar.sites[site.index()].target == target)
}

/// Freezes one required or erased-optional materialized attachment entry.
fn plan_materialized_attached_syntax(
    syntax: &crate::materialization::MaterializedAttachedFixedSyntax,
    outer_site: SiteId,
    lowered: &crate::materialization::LoweredGrammar,
    lookups: &EmissionLookupIndex<'_>,
    capture: PlannedCapturePolicy,
) -> Result<AttachedFixedSyntaxParsePlan, String> {
    let grammar = &lowered.grammar;
    let source = lowered.source_modeled.source_model();
    let plan_fixed = |token_use: crate::lowering::TokenUseId,
                      site: SiteId|
     -> Result<AttachedFixedParsePlan, String> {
        let token_use = &source.token_uses[token_use.index()];
        let definition = &source.token_definitions[token_use.definition.index()];
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            return Err("an attached fixed element did not reference fixed syntax".to_owned());
        };
        let PlannedFollow::Static(expected) =
            planned_failure_edge(lookups, site, FailureEdgeKind::RequiredSite)?
        else {
            return Err("an attached fixed element has a dynamic failure recipe".to_owned());
        };
        Ok(AttachedFixedParsePlan {
            expected,
            capture,
            spelling: fixed_pretty_spelling(&fixed.spelling, grammar.text_equivalence),
            spacing: fixed.pretty_spacing,
        })
    };
    match syntax {
        crate::materialization::MaterializedAttachedFixedSyntax::Required(token) => {
            plan_fixed(*token, outer_site).map(AttachedFixedSyntaxParsePlan::Required)
        }
        crate::materialization::MaterializedAttachedFixedSyntax::Optional { tokens, .. } => {
            let machine = lookups
                .container(outer_site)
                .ok_or_else(|| "an optional attachment lacks its optional machine".to_owned())?;
            if machine.operation() != ContainerOperation::Optional {
                return Err("an optional attachment did not normalize as optional".to_owned());
            }
            let sequence = grammar.sites[outer_site.index()].target;
            let TypeShape::Sequence { sites } = &grammar.types[sequence.index()].shape else {
                return Err("an optional attachment target is not a fixed sequence".to_owned());
            };
            if sites.len() != tokens.len() {
                return Err("an optional attachment has mismatched normalized tokens".to_owned());
            }
            tokens
                .iter()
                .zip(sites)
                .map(|(token, site)| plan_fixed(*token, *site))
                .collect::<Result<Vec<_>, _>>()
                .map(AttachedFixedSyntaxParsePlan::Optional)
        }
    }
}

/// Plans fixed tokens attached around a semantic container.
pub(super) fn plan_fixed_attachment(
    field: &crate::resolution::ResolvedNodeField,
    attachment: &MaterializedFixedAttachment,
    outer_site: SiteId,
    lowered: &crate::materialization::LoweredGrammar,
    lookups: &EmissionLookupIndex<'_>,
    variant_capture: PlannedCapturePolicy,
) -> Result<FixedAttachmentParsePlan, String> {
    let correlation = lowered
        .lowering_map
        .attachments
        .iter()
        .find(|correlation| correlation.outer == outer_site)
        .ok_or_else(|| "a fixed attachment lacks its typed lowering correlation".to_owned())?;
    if correlation.sites.len() != attachment.before.len() + 1 + attachment.after.len() {
        return Err(
            "a fixed attachment has mismatched source and normalized unit elements".to_owned(),
        );
    }
    let capture = field
        .capture
        .map(PlannedCapturePolicy::from)
        .unwrap_or(variant_capture);
    let before = attachment
        .before
        .iter()
        .zip(&correlation.sites)
        .map(|(syntax, site)| {
            plan_materialized_attached_syntax(syntax, *site, lowered, lookups, capture)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let semantic_site = correlation.sites[correlation.semantic_index];
    let machine = lookups.container(semantic_site).ok_or_else(|| {
        "an attachment semantic site lacks its direct container machine".to_owned()
    })?;
    if machine.operation() != ContainerOperation::Direct {
        return Err("an attachment semantic site is not a direct frozen container".to_owned());
    }
    let semantic = Box::new(planned_container(machine, lookups)?);
    let after_start = correlation.semantic_index + 1;
    let after = attachment
        .after
        .iter()
        .zip(&correlation.sites[after_start..])
        .map(|(syntax, site)| {
            plan_materialized_attached_syntax(syntax, *site, lowered, lookups, capture)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FixedAttachmentParsePlan {
        before,
        semantic,
        after,
    })
}

/// Collects materialized Pratt extenders admitted for an application.
fn build_pratt_extenders(
    lookups: &EmissionLookupIndex<'_>,
    owner: crate::TypeId,
    include: impl Fn(PrattPosition) -> bool,
    instrument: bool,
) -> Result<ExpectedSet, String> {
    #[cfg(not(test))]
    let _ = instrument;
    #[cfg(test)]
    if instrument {
        observe_emission_lookup_work(|work| work.pratt_extender_builds += 1);
    }
    let edge = lookups
        .owner_failure(owner, FailureEdgeKind::PrattCallerFollowGuard)
        .ok_or_else(|| "a Pratt operand lacks its frozen extender family".to_owned())?;
    let ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches } = edge.expected() else {
        return Err("a Pratt operand has an invalid frozen extender family".to_owned());
    };
    let first = branches
        .iter()
        .inspect(|_| {
            #[cfg(test)]
            if instrument {
                observe_emission_lookup_work(|work| {
                    work.pratt_extender_branch_candidates += 1;
                });
            }
        })
        .filter(|branch| branch.position().is_some_and(&include))
        .flat_map(|branch| branch.sequences())
        .filter_map(|sequence| sequence.positions().first())
        .map(|position| FirstPosition {
            kinds: position.kinds().clone(),
            text_predicate: None,
        })
        .collect::<Vec<_>>();
    ExpectedSet::from_positions(&first, lookups.atom_index, lookups.kind_count)
}

/// Derives child capture policy from the resolved terminal node.
pub(super) fn terminal_node_capture(
    ty: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
) -> Option<CaptureMode> {
    match ty {
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => terminal_node_capture(inner, lowered),
        ClosedType::Node(node_use) => {
            let node = lowered.applications[node_use.application.index()].node;
            lowered
                .source_modeled
                .resolved()
                .nodes
                .iter()
                .find(|candidate| candidate.id == node)
                .and_then(|node| node.capture)
        }
        ClosedType::ContentToken(_) => None,
    }
}

#[cfg(feature = "diagrams")]
/// Chooses the stable terminal label used by diagram metadata.
pub(super) fn diagram_terminal_name(
    ty: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    match ty {
        ClosedType::Node(node) => lowered
            .applications
            .get(node.application.index())
            .map(|application| application.display_name.clone())
            .ok_or_else(|| "diagram terminal references an unknown Node application".to_owned()),
        ClosedType::ContentToken(token) => lowered
            .source_modeled
            .resolved()
            .content_tokens
            .iter()
            .find(|content| content.id == *token)
            .map(|content| content.ident.clone())
            .ok_or_else(|| "diagram terminal references an unknown content token".to_owned()),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => diagram_terminal_name(inner, lowered),
    }
}

#[cfg(feature = "diagrams")]
/// Resolves a repetition separator to its canonical fixed spelling.
pub(super) fn diagram_separator_spelling(
    separator: Option<&crate::source_model::Separator>,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<Option<String>, String> {
    let Some(separator) = separator else {
        return Ok(None);
    };
    let source_model = lowered.source_modeled.source_model();
    let token_use = source_model
        .token_uses
        .get(separator.token_use.index())
        .ok_or_else(|| "diagram separator references an unknown token use".to_owned())?;
    let definition = source_model
        .token_definitions
        .get(token_use.definition.index())
        .ok_or_else(|| "diagram separator references an unknown token definition".to_owned())?;
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("diagram separator does not reference fixed-token syntax".to_owned());
    };
    Ok(Some(fixed.spelling.clone()))
}

/// Derives the stable metadata name for a fixed element occurrence.
pub(super) fn fixed_element_name(
    node: &ResolvedNode,
    token_use: &crate::source_model::TokenUse,
    definition: &crate::source_model::TokenDefinition,
    fixed: &crate::source_model::FixedTokenDefinition,
) -> String {
    let field = fixed_element_field(node, token_use);
    let base = if definition.name.starts_with('"') {
        snake_case(&fixed.spelling)
    } else {
        snake_case(&definition.name)
    };
    field
        .and_then(|field| field.syntax.ident.as_ref())
        .map_or(base.clone(), |field| {
            format!("{}_{}", snake_case(&field.unraw().to_string()), base)
        })
}

/// Determines the construction slot occupied by a fixed element.
pub(super) fn fixed_element_field<'node>(
    node: &'node ResolvedNode,
    token_use: &crate::source_model::TokenUse,
) -> Option<&'node crate::resolution::ResolvedNodeField> {
    node.fields.iter().find(|field| {
        field.metas.iter().any(|meta| {
            let range = span_range(meta.span());
            range.start <= token_use.site.range.start && range.end >= token_use.site.range.end
        })
    })
}

/// Converts authored type constructors into parser storage wrappers.
pub(super) fn field_wrappers(ty: &ClosedType) -> Result<Vec<FieldWrapper>, String> {
    fn walk(ty: &ClosedType, output: &mut Vec<FieldWrapper>) {
        match ty {
            ClosedType::Node(_) | ClosedType::ContentToken(_) => {}
            ClosedType::Boxed(inner) => {
                output.push(FieldWrapper::Boxed);
                walk(inner, output)
            }
            ClosedType::Optional(inner) => {
                output.push(FieldWrapper::Optional);
                walk(inner, output)
            }
            ClosedType::ZeroOrMore(inner) => {
                output.push(FieldWrapper::ZeroOrMore);
                walk(inner, output)
            }
            ClosedType::OneOrMore(inner) => {
                output.push(FieldWrapper::OneOrMore);
                walk(inner, output)
            }
        }
    }
    let mut wrappers = Vec::new();
    walk(ty, &mut wrappers);
    Ok(wrappers)
}

/// Projects a resolved terminal into Explorer's grammar-erased schema.
pub(super) fn explorer_terminal(ty: &ClosedType) -> Result<ExplorerTerminalPlan, String> {
    match ty {
        ClosedType::Node(node) => Ok(ExplorerTerminalPlan::Node(node.application)),
        ClosedType::ContentToken(token) => Ok(ExplorerTerminalPlan::Content(*token)),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => explorer_terminal(inner),
    }
}

/// Rejects wrapper combinations unsupported by parser container semantics.
pub(super) fn validate_container_wrappers(
    operation: ContainerOperation,
    wrappers: &[FieldWrapper],
    field_name: &str,
) -> Result<(), String> {
    let repetition = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore));
    let optional_before_repeat = repetition.is_some_and(|index| {
        wrappers[..index]
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    });
    let inferred = match repetition.map(|index| wrappers[index]) {
        Some(FieldWrapper::ZeroOrMore) => ContainerOperation::ZeroOrMore,
        Some(FieldWrapper::OneOrMore) if optional_before_repeat => {
            ContainerOperation::OptionalOneOrMore
        }
        Some(FieldWrapper::OneOrMore) => ContainerOperation::OneOrMore,
        Some(FieldWrapper::Boxed | FieldWrapper::Optional) => unreachable!(),
        None if wrappers
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional)) =>
        {
            ContainerOperation::Optional
        }
        None => ContainerOperation::Direct,
    };
    if inferred == operation {
        Ok(())
    } else {
        Err(format!(
            "field `{field_name}` container wrappers disagree with its frozen machine: {inferred:?} vs {operation:?}"
        ))
    }
}

/// Freezes container control flow, selectors, FOLLOW sets, and failures.
pub(super) fn planned_container(
    machine: &crate::ContainerMachine,
    lookups: &EmissionLookupIndex<'_>,
) -> Result<PlannedContainer, String> {
    let atom_index = lookups.atom_index;
    let kind_count = lookups.kind_count;
    let stop = planned_follow(machine.stop(), atom_index, kind_count)?;
    let element_selector =
        ExpectedSet::from_positions(machine.element_first(), atom_index, kind_count)?;
    // A matching `#[greedy(...)]` acceptance applies to the child parsed at this site, so it
    // wraps the child FOLLOW only: the repetition stop and every failure edge are unmarked,
    // and a site without an acceptance is untouched.
    let accept = |follow: PlannedFollow| match lookups.greedy_acceptance(machine.site()) {
        Some(accepted) => PlannedFollow::Accepting {
            inner: Box::new(follow),
            accepted: accepted.clone(),
        },
        None => follow,
    };
    Ok(match machine.operation() {
        ContainerOperation::Direct => PlannedContainer::Direct {
            child_follow: accept(stop),
            required_failure: planned_failure_edge(
                lookups,
                machine.site(),
                FailureEdgeKind::RequiredSite,
            )?,
        },
        ContainerOperation::Optional => PlannedContainer::Optional {
            selector: element_selector,
            decision: machine
                .optional_viability_decision()
                .map(|decision| {
                    plan_optional_viability_decision(decision, atom_index, kind_count).map(Box::new)
                })
                .transpose()?,
            child_follow: accept(stop),
        },
        operation @ (ContainerOperation::ZeroOrMore
        | ContainerOperation::OneOrMore
        | ContainerOperation::OptionalOneOrMore) => {
            let separator_selector = if machine.separator().is_some() {
                Some(ExpectedSet::from_positions(
                    machine.separator_first(),
                    atom_index,
                    kind_count,
                )?)
            } else {
                None
            };
            let continuation = separator_selector
                .as_ref()
                .unwrap_or(&element_selector)
                .clone();
            let child_follow = accept(follow_with_static(&stop, &continuation));
            PlannedContainer::Repeated {
                operation,
                element_selector,
                separator_selector,
                optional_trailing: machine.optional_trailing(),
                stop,
                child_follow,
                minimum_failure: matches!(
                    operation,
                    ContainerOperation::OneOrMore | ContainerOperation::OptionalOneOrMore
                )
                .then(|| {
                    planned_failure_edge(lookups, machine.site(), FailureEdgeKind::ContainerMinimum)
                })
                .transpose()?
                .map(Box::new),
                continuation_failure: Box::new(planned_failure_edge(
                    lookups,
                    machine.site(),
                    FailureEdgeKind::ContainerContinuation,
                )?),
                after_separator_failure: machine
                    .separator()
                    .map(|_| {
                        planned_failure_edge(
                            lookups,
                            machine.site(),
                            FailureEdgeKind::ContainerAfterSeparator,
                        )
                    })
                    .transpose()?
                    .map(Box::new),
            }
        }
    })
}

/// Converts analyzed failure evidence into its emitted FOLLOW representation.
pub(super) fn planned_failure_edge(
    lookups: &EmissionLookupIndex<'_>,
    site: SiteId,
    kind: FailureEdgeKind,
) -> Result<PlannedFollow, String> {
    let edge = lookups.site_failure(site, kind).ok_or_else(|| {
        format!(
            "site {} lacks its frozen {kind:?} failure edge",
            site.index()
        )
    })?;
    planned_expected_recipe(edge.expected(), lookups.atom_index, lookups.kind_count)
}

/// Converts an expected-set recipe to the runtime-width representation.
pub(super) fn planned_expected_recipe(
    expected: &ExpectedRecipe,
    atom_index: &ExpectedAtomIndex,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    match expected {
        ExpectedRecipe::Static(first) => Ok(PlannedFollow::Static(ExpectedSet::from_positions(
            first, atom_index, kind_count,
        )?)),
        ExpectedRecipe::Follow(follow) => planned_follow(follow, atom_index, kind_count),
        ExpectedRecipe::ContinueOrStop { continuation, stop } => {
            let stop = planned_follow(stop, atom_index, kind_count)?;
            let continuation = ExpectedSet::from_positions(continuation, atom_index, kind_count)?;
            Ok(follow_with_static(&stop, &continuation))
        }
        ExpectedRecipe::PrattExtendersAtOrAboveMinBp { .. } => {
            Err("a non-Pratt site referenced a Pratt failure recipe".to_owned())
        }
    }
}

/// Resolves the concrete terminal type for one materialized parse site.
pub(super) fn terminal_type(
    closed: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    match closed {
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => terminal_type(inner, lowered, input_lifetime),
        ClosedType::Node(node_use) => materialized_application_type(
            &lowered.applications[node_use.application.index()],
            &node_use.lifetimes,
            lowered,
            input_lifetime,
        ),
        ClosedType::ContentToken(token) => {
            let mut path: syn::Path = syn::parse_quote!(crate);
            for segment in &lowered
                .source_modeled
                .resolved()
                .build_inputs
                .module_anchor
                .segments
            {
                path.segments
                    .push(syn::PathSegment::from(emitted_ident(segment)?));
            }
            path.segments.push(syn::PathSegment {
                ident: format_ident!("__recursa_content_tokens"),
                arguments: PathArguments::None,
            });
            path.segments.push(syn::PathSegment {
                ident: format_ident!("__RecursaContentToken{}", token.index()),
                arguments: PathArguments::AngleBracketed(syn::parse_quote!(<#input_lifetime>)),
            });
            Ok(Type::Path(syn::TypePath { qself: None, path }))
        }
    }
}

/// Finds the fully bound generated application for a terminal, when one exists.
pub(super) fn closed_terminal_application(
    closed: &ClosedType,
) -> Option<MaterializedApplicationId> {
    match closed {
        ClosedType::Node(node) => Some(node.application),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => closed_terminal_application(inner),
        ClosedType::ContentToken(_) => None,
    }
}

/// Resolves authored Pratt extension exclusions to the target application's LED ordinals.
fn planned_pratt_exclusions(
    lowered: &crate::materialization::LoweredGrammar,
    terminal_application: Option<MaterializedApplicationId>,
    filter: Option<&crate::source_model::PrattExtensionFilter>,
    field_name: &str,
) -> Result<Vec<usize>, String> {
    let Some(filter) = filter else {
        return Ok(Vec::new());
    };
    let application_id = terminal_application.ok_or_else(|| {
        format!("field `{field_name}` applies a Pratt restriction to a non-generated terminal")
    })?;
    let application = lowered
        .applications
        .iter()
        .find(|application| application.id == application_id)
        .ok_or_else(|| {
            format!("field `{field_name}` targets an unknown materialized application")
        })?;
    let MaterializedNodeBody::Pratt(variants) = &application.body else {
        return Err(format!(
            "field `{field_name}` applies a Pratt restriction to non-Pratt Node `{}`",
            application.display_name,
        ));
    };
    filter
        .excluded
        .iter()
        .map(|excluded| {
            let mut led_index = 0;
            for variant in variants {
                match variant {
                    MaterializedPrattVariant::Nud(nud) if nud.common.name == *excluded => {
                        return Err(format!(
                            "field `{field_name}` cannot exclude Pratt NUD variant `{excluded}`; only extensions may be excluded"
                        ));
                    }
                    MaterializedPrattVariant::Nud(_) => {}
                    MaterializedPrattVariant::Led(led) if led.common.name == *excluded => {
                        return Ok(led_index);
                    }
                    MaterializedPrattVariant::Led(_) => led_index += 1,
                }
            }
            Err(format!(
                "field `{field_name}` excludes unknown Pratt extension `{excluded}` from `{}`",
                application.display_name,
            ))
        })
        .collect()
}

/// Renders the concrete Rust type of a materialized node application.
pub(super) fn materialized_application_type(
    application: &MaterializedNodeApplication,
    lifetimes: &[syn::Lifetime],
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    let resolved = lowered.source_modeled.resolved();
    let node = &resolved.nodes[application.node.index()];
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in &node.key.segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    let segment = path
        .segments
        .last_mut()
        .ok_or_else(|| "materialized application path is empty".to_owned())?;
    let lifetime_parameter_count = node.generics.lifetimes().count();
    if lifetimes.len() != lifetime_parameter_count {
        return Err(format!(
            "materialized application `{}` has {} lifetime arguments for {lifetime_parameter_count} parameters",
            application.display_name,
            lifetimes.len(),
        ));
    }
    let mut type_arguments = application.type_arguments.iter();
    let mut arguments = syn::punctuated::Punctuated::new();
    for parameter in &node.generics.params {
        match parameter {
            GenericParam::Lifetime(_) => {
                arguments.push(GenericArgument::Lifetime(input_lifetime.clone()));
            }
            GenericParam::Type(_) => {
                let argument = type_arguments.next().ok_or_else(|| {
                    format!(
                        "materialized application `{}` lacks a closed type argument",
                        application.display_name
                    )
                })?;
                arguments.push(GenericArgument::Type(closed_type(
                    argument,
                    lowered,
                    input_lifetime,
                )?));
            }
            GenericParam::Const(_) => {
                return Err("const-generic materialized applications cannot be emitted".to_owned());
            }
        }
    }
    if type_arguments.next().is_some() {
        return Err(format!(
            "materialized application `{}` has excess generic arguments",
            application.display_name
        ));
    }
    if !arguments.is_empty() {
        segment.arguments = PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: arguments,
            gt_token: Default::default(),
        });
    }
    Ok(Type::Path(syn::TypePath { qself: None, path }))
}

/// Renders a resolved type after requiring every generic to be bound.
pub(super) fn closed_type(
    closed: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    let facade = emitted_ident(&lowered.source_modeled.resolved().build_inputs.facade.ident)?;
    Ok(match closed {
        ClosedType::Node(_) | ClosedType::ContentToken(_) => {
            terminal_type(closed, lowered, input_lifetime)?
        }
        ClosedType::Boxed(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(Box<#inner>)
        }
        ClosedType::Optional(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(Option<#inner>)
        }
        ClosedType::ZeroOrMore(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(Vec<#inner>)
        }
        ClosedType::OneOrMore(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(#facade::Vec1<#inner>)
        }
    })
}

/// Converts a frozen FOLLOW recipe into its emitted representation.
pub(super) fn planned_follow(
    recipe: &FollowRecipe,
    atom_index: &ExpectedAtomIndex,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    Ok(match recipe {
        FollowRecipe::Caller => PlannedFollow::Caller,
        FollowRecipe::Static { first } => {
            PlannedFollow::Static(ExpectedSet::from_positions(first, atom_index, kind_count)?)
        }
        FollowRecipe::StaticAndCaller { first } => PlannedFollow::StaticAndCaller(
            ExpectedSet::from_positions(first, atom_index, kind_count)?,
        ),
    })
}

/// Unions a static expected set into an existing FOLLOW policy.
pub(super) fn follow_with_static(
    follow: &PlannedFollow,
    additional: &ExpectedSet,
) -> PlannedFollow {
    match follow {
        PlannedFollow::Caller => PlannedFollow::StaticAndCaller(additional.clone()),
        PlannedFollow::Static(expected) => {
            let mut expected = expected.clone();
            expected.union_assign(additional);
            PlannedFollow::Static(expected)
        }
        PlannedFollow::StaticAndCaller(expected) => {
            let mut expected = expected.clone();
            expected.union_assign(additional);
            PlannedFollow::StaticAndCaller(expected)
        }
        PlannedFollow::CallerAndPrattStops(expected) => {
            let mut expected = expected.clone();
            expected.union_assign(additional);
            PlannedFollow::CallerAndPrattStops(expected)
        }
        PlannedFollow::Accepting { inner, accepted } => PlannedFollow::Accepting {
            inner: Box::new(follow_with_static(inner, additional)),
            accepted: accepted.clone(),
        },
    }
}

/// Adds generated Pratt stop tokens without replacing an inherited authored boundary.
fn follow_with_pratt_stops(follow: &PlannedFollow, additional: &ExpectedSet) -> PlannedFollow {
    match follow {
        PlannedFollow::Caller => PlannedFollow::CallerAndPrattStops(additional.clone()),
        PlannedFollow::CallerAndPrattStops(expected) => {
            let mut expected = expected.clone();
            expected.union_assign(additional);
            PlannedFollow::CallerAndPrattStops(expected)
        }
        PlannedFollow::Accepting { inner, accepted } => PlannedFollow::Accepting {
            inner: Box::new(follow_with_pratt_stops(inner, additional)),
            accepted: accepted.clone(),
        },
        PlannedFollow::Static(_) | PlannedFollow::StaticAndCaller(_) => {
            follow_with_static(follow, additional)
        }
    }
}

/// Adapts a frozen site FOLLOW for one recursive Pratt binding-power threshold.
///
/// Static FOLLOW analysis can carry extender starters through recursive grammar paths. Active
/// extenders belong to the nested Pratt loop, while inactive extenders are its stopping tokens.
/// Caller-relative membership is retained because it remains the runtime ambiguity boundary.
pub(super) fn pratt_operand_follow(
    follow: &PlannedFollow,
    active: &ExpectedSet,
    inactive: &ExpectedSet,
) -> PlannedFollow {
    let without_active = match follow {
        PlannedFollow::Caller => PlannedFollow::Caller,
        PlannedFollow::Static(expected) => {
            let mut expected = expected.clone();
            expected.difference_assign(active);
            PlannedFollow::Static(expected)
        }
        PlannedFollow::StaticAndCaller(expected) => {
            let mut expected = expected.clone();
            expected.difference_assign(active);
            PlannedFollow::StaticAndCaller(expected)
        }
        PlannedFollow::CallerAndPrattStops(expected) => {
            let mut expected = expected.clone();
            expected.difference_assign(active);
            PlannedFollow::CallerAndPrattStops(expected)
        }
        PlannedFollow::Accepting { inner, accepted } => {
            return PlannedFollow::Accepting {
                inner: Box::new(pratt_operand_follow(inner, active, inactive)),
                accepted: accepted.clone(),
            };
        }
    };
    let mut inactive_only = inactive.clone();
    inactive_only.difference_assign(active);
    follow_with_pratt_stops(&without_active, &inactive_only)
}

/// Adds the next-loop starters to a completed non-recursive Pratt LED tail.
fn add_pratt_continuation_follow(element: &mut ElementParsePlan, extenders: &ExpectedSet) {
    let ElementParseKind::Semantic { projection, .. } = &mut element.kind else {
        return;
    };
    let SemanticParseProjection::Direct {
        container,
        pratt_min_bp,
        attachment,
        ..
    } = projection.as_mut()
    else {
        return;
    };
    if pratt_min_bp.is_some() {
        return;
    }
    add_container_stop(container, extenders);
    if let Some(attachment) = attachment {
        add_container_stop(&mut attachment.semantic, extenders);
    }
}

/// Adds one static legal stop to a semantic container's child and loop boundaries.
fn add_container_stop(container: &mut PlannedContainer, stop: &ExpectedSet) {
    match container {
        PlannedContainer::Direct { child_follow, .. }
        | PlannedContainer::Optional { child_follow, .. } => {
            *child_follow = follow_with_static(child_follow, stop);
        }
        PlannedContainer::Repeated {
            stop: repetition_stop,
            child_follow,
            ..
        } => {
            *repetition_stop = follow_with_static(repetition_stop, stop);
            *child_follow = follow_with_static(child_follow, stop);
        }
    }
}

/// Emits the parser body selected by the frozen node plan.
pub(super) fn render_node_parse_body(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    instrumentation: &mut RenderInstrumentation,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let helper_method_generics = if node.generics.lifetimes().next().is_some() {
        quote!(<A>)
    } else {
        quote!(<'__recursa_input, A>)
    };
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => (
            render_sequence_parse(plan, node_index, None, *style, elements, None, None),
            quote!(),
            quote!(),
        ),
        NodeBodyPlan::Enum { variants, dispatch } => {
            let mut expected_index = 1_000_000;
            render_predictive_dispatch(
                plan,
                node_index,
                variants,
                dispatch,
                &mut expected_index,
                instrumentation,
                &helper_method_generics,
            )
        }
        NodeBodyPlan::Pratt(pratt) => render_pratt_parse(
            plan,
            node_index,
            pratt,
            instrumentation,
            &helper_method_generics,
        ),
    }
}

/// Emits trie traversal and variant selection for predictive dispatch.
///
/// # Long-function contract
///
/// Rationale: emits the validated decision trie without rebuilding lookahead semantics.
/// Invariants: edge order, text predicates, and failure expectations match the frozen plan.
/// Phases: renders lookahead acquisition, trie branching, selected variant, and failure path.
pub(super) fn render_predictive_dispatch(
    plan: &EmissionPlan,
    node_index: usize,
    variants: &[VariantParsePlan],
    dispatch: &PredictiveDispatchPlan,
    expected_index: &mut usize,
    instrumentation: &mut RenderInstrumentation,
    helper_method_generics: &proc_macro2::TokenStream,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    instrumentation.frozen_dispatch_nodes_rendered += dispatch.states.len()
        + dispatch
            .balanced
            .as_ref()
            .map_or(0, |balanced| balanced.after_close_states.len());
    let (ordinary, ordinary_items) = render_predictive_graph_selection(
        plan,
        node_index,
        dispatch.root,
        &dispatch.states,
        &quote!(0usize),
        dispatch.balanced.as_ref().map(|balanced| &balanced.open),
        expected_index,
    );
    let (selection, selection_items) = if let Some(balanced) = dispatch.balanced.as_ref() {
        let open_expected = render_expected_set(plan, &balanced.open);
        let close_expected = render_expected_set(plan, &balanced.close);
        let after_depth = format_ident!("__recursa_balanced_after_depth");
        let (after_close, after_close_items) = render_predictive_graph_selection(
            plan,
            node_index,
            balanced.after_close_root,
            &balanced.after_close_states,
            &quote!(#after_depth),
            None,
            expected_index,
        );
        (
            quote! {
                if context
                    .peek(0)
                    .is_some_and(|record| #open_expected.contains(record.kind))
                {
                    let mut __recursa_balanced_depth = 0usize;
                    let mut __recursa_balanced_nesting = 0usize;
                    loop {
                        let Some(__recursa_balanced_record) =
                            context.peek(__recursa_balanced_depth)
                        else {
                            return Err(context.unexpected_at(
                                __recursa_balanced_depth,
                                &#close_expected,
                            ));
                        };
                        if #open_expected.contains(__recursa_balanced_record.kind) {
                            __recursa_balanced_nesting += 1;
                        } else if #close_expected.contains(__recursa_balanced_record.kind) {
                            __recursa_balanced_nesting -= 1;
                            if __recursa_balanced_nesting == 0 {
                                break;
                            }
                        }
                        __recursa_balanced_depth += 1;
                    }
                    let #after_depth = __recursa_balanced_depth + 1;
                    #after_close
                } else {
                    #ordinary
                }
            },
            quote!(#ordinary_items #after_close_items),
        )
    } else {
        (ordinary, ordinary_items)
    };
    let facade = &plan.facade;
    let mut helpers = Vec::with_capacity(variants.len());
    let mut selections = Vec::with_capacity(variants.len());
    for (variant_index, variant) in variants.iter().enumerate() {
        if let Some(inline) = render_runtime_variant_selection(plan, variant) {
            selections.push(quote!(#variant_index => #inline));
            continue;
        }
        let body = render_sequence_parse(
            plan,
            node_index,
            Some((variant_index, &variant.ident)),
            variant.style,
            &variant.elements,
            variant.capture,
            None,
        );
        let helper = format_ident!("__recursa_parse_variant_{node_index}_{variant_index}");
        helpers.push(quote! {
            fn #helper #helper_method_generics(
                context: &mut #facade::__private::ParseContext<'_, '__recursa_input, A>,
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                #body
            }
        });
        selections.push(quote!(#variant_index => Self::#helper(context)));
    }
    (
        quote! {
            match { #selection } {
                #(#selections,)*
                _ => unreachable!("frozen dispatch selected an unknown branch"),
            }
        },
        selection_items,
        quote!(#(#helpers)*),
    )
}

/// Emits one runtime-helper call for a variant whose shape the runtime parses whole.
///
/// Two shapes qualify: a variant whose only element is one required direct child under
/// the caller FOLLOW, and a variant made only of fixed tokens under the inherited capture
/// policy. Every other shape keeps its typechecked helper function. Returns `None` when
/// the variant does not qualify.
fn render_runtime_variant_selection(
    plan: &EmissionPlan,
    variant: &VariantParsePlan,
) -> Option<proc_macro2::TokenStream> {
    let facade = &plan.facade;
    if variant.capture.is_some() || variant.elements.is_empty() {
        return None;
    }
    let ident = &variant.ident;
    let fixed_only = variant.elements.iter().all(|element| {
        matches!(
            &element.kind,
            ElementParseKind::Fixed {
                capture: PlannedCapturePolicy::Inherited,
                ..
            }
        )
    });
    if fixed_only && variant.style == ResolvedFieldStyle::Unit {
        let expected = variant.elements.iter().map(|element| match &element.kind {
            ElementParseKind::Fixed { expected, .. } => {
                let expected = render_expected_set(plan, expected);
                quote!(&#expected)
            }
            _ => unreachable!("fixed-only variants contain only fixed elements"),
        });
        return Some(quote!(context.parse_fixed_variant([#(#expected),*], Self::#ident)));
    }
    let [element] = variant.elements.as_slice() else {
        return None;
    };
    let ElementParseKind::Semantic { field, projection } = &element.kind else {
        return None;
    };
    let SemanticParseProjection::Direct {
        terminal,
        wrappers,
        container,
        capture,
        child_capture,
        pratt_min_bp: None,
        pratt_excluded_led,
        attachment: None,
        ..
    } = projection.as_ref()
    else {
        return None;
    };
    if !pratt_excluded_led.is_empty() {
        return None;
    }
    let PlannedContainer::Direct {
        child_follow: PlannedFollow::Caller,
        required_failure,
    } = container.as_ref()
    else {
        return None;
    };
    let failure = match required_failure {
        PlannedFollow::Static(expected) => {
            let expected = render_expected_set(plan, expected);
            quote!(&#expected)
        }
        PlannedFollow::Caller => quote!(context.caller_follow()),
        PlannedFollow::StaticAndCaller(_)
        | PlannedFollow::CallerAndPrattStops(_)
        | PlannedFollow::Accepting { .. } => return None,
    };
    let child_capture = child_capture.render(facade);
    let capture = capture.render(facade);
    let wrap = if wrappers.is_empty() && variant.style == ResolvedFieldStyle::Unnamed {
        quote!(Self::#ident)
    } else {
        let value = format_ident!("__recursa_value_0");
        let wrapped = render_wrapped_value(quote!(__recursa_child), wrappers);
        let construction = render_construction(
            Some(ident),
            variant.style,
            std::slice::from_ref(&(field.clone(), value.clone())),
        );
        quote!(|__recursa_child| {
            let #value = #wrapped;
            #construction
        })
    };
    Some(quote! {
        context.parse_wrapping_variant::<Self, #terminal>(
            #child_capture,
            #capture,
            #failure,
            #wrap,
        )
    })
}

#[allow(clippy::too_many_arguments)]
/// Emits one frozen predictive graph from an arbitrary lookahead origin.
pub(super) fn render_predictive_graph_selection(
    plan: &EmissionPlan,
    node_index: usize,
    root: DispatchStateId,
    states: &[DispatchTrieNodePlan],
    initial_depth: &proc_macro2::TokenStream,
    root_failure_extra: Option<&ExpectedSet>,
    expected_index: &mut usize,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let facade = &plan.facade;
    let root_index = root.index();
    let table_index = *expected_index;
    *expected_index += 1;
    let module_anchor = &plan.module_anchor;
    let mut position_registry = BTreeMap::<DispatchPositionPlan, usize>::new();
    let mut positions = Vec::new();
    let mut edge_values = Vec::new();
    let mut state_values = Vec::with_capacity(states.len() * 4);
    for (state_index, node) in states.iter().enumerate() {
        let edge_start = edge_values.len() / 2;
        for edge in &node.edges {
            let position = if let Some(position) = position_registry.get(&edge.position) {
                *position
            } else {
                let position = positions.len();
                let expected_index = plan
                    .expected_sets
                    .id(&edge.position.expected)
                    .expect("validated dispatch expectation belongs to the grammar-wide pool");
                positions.push((edge.position.clone(), expected_index));
                position_registry.insert(edge.position.clone(), position);
                position
            };
            edge_values.extend([position, edge.next.index()]);
        }
        let expected = dispatch_state_expected_set(
            node,
            (state_index == root_index)
                .then_some(root_failure_extra)
                .flatten(),
            plan.kind_count,
        );
        let expected_index = plan
            .expected_sets
            .id(&expected)
            .expect("validated dispatch expectation belongs to the grammar-wide pool");
        let target = node.target.map_or(0, |target| {
            target
                .variant
                .checked_mul(3)
                .and_then(|variant| variant.checked_add(dispatch_terminal_mode_code(target.mode)))
                .and_then(|target| target.checked_add(1))
                .expect("frozen dispatch target encoding fits usize")
        });
        let edge_end = edge_values.len() / 2;
        let edge_count = edge_end - edge_start;
        state_values.extend([target, edge_start, edge_count, expected_index]);
    }
    let encoded_positions = encode_dispatch_positions(
        plan.text_equivalence,
        &positions
            .iter()
            .map(|(position, expected_index)| (*expected_index, position.text_predicate.clone()))
            .collect::<Vec<_>>(),
    );
    let mut packed_values = state_values.clone();
    packed_values.extend(&edge_values);
    packed_values.extend(&encoded_positions.table);
    let (packed_width, _) = packed_table(&packed_values);
    let (_, states_literal) = packed_table_with_width(&state_values, packed_width);
    let (_, edges_literal) = packed_table_with_width(&edge_values, packed_width);
    let (_, positions_literal) = packed_table_with_width(&encoded_positions.table, packed_width);
    let table_name = format_ident!("__RECURSA_DISPATCH_{node_index}_{table_index}");
    let spellings = &encoded_positions.spellings;
    let items = quote! {
        static #table_name: #facade::__private::DispatchTable =
            #facade::__private::dispatch_table(
                #states_literal,
                #edges_literal,
                #positions_literal,
                &[#(#spellings),*],
                #packed_width,
                4usize,
                1usize,
                &#module_anchor::__RECURSA_EXPECTED_SETS,
            );
    };
    let selection = quote!(context.select_predictive(&#table_name, #root_index, #initial_depth)?);
    (selection, items)
}

/// Packs integer table cells into fixed-width URL-safe base64 digits.
fn packed_table(values: &[usize]) -> (usize, proc_macro2::Literal) {
    let max = values.iter().copied().max().unwrap_or(0);
    let significant_bits = (usize::BITS - max.leading_zeros()) as usize;
    let width = significant_bits.div_ceil(6).max(1);
    packed_table_with_width(values, width)
}

/// Packs integer table cells with the caller-selected fixed digit width.
fn packed_table_with_width(values: &[usize], width: usize) -> (usize, proc_macro2::Literal) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = Vec::with_capacity(values.len() * width);
    for value in values {
        for digit in (0..width).rev() {
            encoded.push(ALPHABET[(value >> (digit * 6)) & 0x3f]);
        }
    }
    (width, proc_macro2::Literal::byte_string(&encoded))
}

/// Emits compact table-driven construction of the branch selected by predictive dispatch.
pub(super) fn render_predictive_selection(
    plan: &EmissionPlan,
    node_index: usize,
    dispatch: &PredictiveDispatchPlan,
    caller_follow: &proc_macro2::TokenStream,
    expected_index: &mut usize,
    instrumentation: &mut RenderInstrumentation,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    instrumentation.frozen_dispatch_nodes_rendered += dispatch.states.len();
    let facade = &plan.facade;
    let root = dispatch.root.index();
    let table_index = *expected_index;
    *expected_index += 1;
    let mut position_registry = BTreeMap::<DispatchPositionPlan, usize>::new();
    let mut positions = Vec::new();
    let mut edge_values = Vec::new();
    let mut state_values = Vec::new();
    for node in &dispatch.states {
        let edge_start = edge_values.len() / 2;
        for edge in &node.edges {
            let position = if let Some(position) = position_registry.get(&edge.position) {
                *position
            } else {
                let position = positions.len();
                let expected_index = plan
                    .expected_sets
                    .id(&edge.position.expected)
                    .expect("validated Pratt expectation belongs to the grammar-wide pool");
                positions.push((edge.position.clone(), expected_index));
                position_registry.insert(edge.position.clone(), position);
                position
            };
            edge_values.extend([position, edge.next.index()]);
        }
        let target = node.target.map_or(0, |target| {
            target
                .variant
                .checked_mul(3)
                .and_then(|variant| variant.checked_add(dispatch_terminal_mode_code(target.mode)))
                .and_then(|target| target.checked_add(1))
                .expect("frozen Pratt target encoding fits usize")
        });
        let edge_end = edge_values.len() / 2;
        let edge_count = edge_end - edge_start;
        state_values.extend([target, edge_start, edge_count]);
    }
    let encoded_positions = encode_dispatch_positions(
        plan.text_equivalence,
        &positions
            .iter()
            .map(|(position, expected_index)| (*expected_index, position.text_predicate.clone()))
            .collect::<Vec<_>>(),
    );
    let mut packed_values = state_values.clone();
    packed_values.extend(&edge_values);
    packed_values.extend(&encoded_positions.table);
    let (packed_width, _) = packed_table(&packed_values);
    let (_, states_literal) = packed_table_with_width(&state_values, packed_width);
    let (_, edges_literal) = packed_table_with_width(&edge_values, packed_width);
    let (_, positions_literal) = packed_table_with_width(&encoded_positions.table, packed_width);
    let table_name = format_ident!("__RECURSA_PRATT_{node_index}_{table_index}");
    let spellings = &encoded_positions.spellings;
    let module_anchor = &plan.module_anchor;
    let items = quote! {
        static #table_name: #facade::__private::DispatchTable =
            #facade::__private::dispatch_table(
                #states_literal,
                #edges_literal,
                #positions_literal,
                &[#(#spellings),*],
                #packed_width,
                3usize,
                1usize,
                &#module_anchor::__RECURSA_EXPECTED_SETS,
            );
    };
    let selection = quote!(context.select_pratt(&#table_name, #root, &#caller_follow));
    (selection, items)
}

/// Emits the prefix parse and binding-power extender loop for a Pratt node.
///
/// # Long-function contract
///
/// Rationale: emits Pratt control flow from the complete immutable Pratt plan.
/// Invariants: binding powers, FOLLOW handoff, and typed lhs replacement preserve the analyzed grammar.
/// Phases: parses nud, loops over eligible led branches, then returns the accumulated lhs.
pub(super) fn render_pratt_parse(
    plan: &EmissionPlan,
    node_index: usize,
    pratt: &PrattParsePlan,
    instrumentation: &mut RenderInstrumentation,
    helper_method_generics: &proc_macro2::TokenStream,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let facade = &plan.facade;
    let owner = quote!(Self);
    let expression_start = format_ident!("__recursa_expression_start");
    let mut expected_index = 2_000_000;
    let nud_follow = render_expected_set(plan, &pratt.nud_selection_follow);
    let nud_follow_value = format_ident!("__recursa_pratt_nud_follow");
    let (nud_selection, nud_selection_items) = render_predictive_selection(
        plan,
        node_index,
        &pratt.nud_dispatch,
        &quote!(#nud_follow_value),
        &mut expected_index,
        instrumentation,
    );
    let nud_failure_name = expected_static(node_index, 3_100_000, "pratt_nud_failure");
    let (nud_failure_definition, nud_failure_value) =
        render_follow_value(plan, &nud_failure_name, &pratt.nud_failure);
    let mut helper_methods = Vec::with_capacity(pratt.nud.len() + pratt.led.len());
    let mut nud_branches = Vec::with_capacity(pratt.nud.len());
    for (index, branch) in pratt.nud.iter().enumerate() {
        let variant = &branch.variant;
        let body = render_sequence_parse(
            plan,
            node_index,
            Some((index, &variant.ident)),
            variant.style,
            &variant.elements,
            variant.capture,
            Some(&owner),
        );
        let helper = format_ident!("__recursa_parse_pratt_nud_{node_index}_{index}");
        helper_methods.push(quote! {
            fn #helper #helper_method_generics(
                context: &mut #facade::__private::ParseContext<'_, '__recursa_input, A>,
                __recursa_pratt_excluded: &[usize],
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                #body
            }
        });
        let selected = match branch.position {
            PrattPosition::Atom => quote! {
                context.parse_generated(
                    &#nud_follow_value,
                    context.capture_policy(),
                    |context| Self::#helper(context, __recursa_pratt_excluded),
                )?
            },
            PrattPosition::Prefix { .. } => {
                quote!(Self::#helper(context, __recursa_pratt_excluded)?)
            }
            PrattPosition::Infix { .. } | PrattPosition::Postfix { .. } => {
                unreachable!("validated Pratt NUD plan contains only atom or prefix branches")
            }
        };
        nud_branches.push(quote!((Some(#index), _) => #selected));
    }

    if pratt.led.is_empty() {
        return (
            quote! {
                #nud_failure_definition
                let _ = min_bp;
                let #nud_follow_value = {
                    static __RECURSA_FOLLOW_SITE: #facade::__private::FollowSiteCache =
                        #facade::__private::FollowSiteCache::new();
                    __RECURSA_FOLLOW_SITE
                        .with_caller(&#nud_follow, context.follow())
                        .expect("matching generated Pratt FOLLOW widths")
                };
                let #expression_start = context.cursor();
                let parsed = match { #nud_selection } {
                    #(#nud_branches,)*
                    (None, failure_depth) => {
                        return Err(context.unexpected_at(failure_depth, &#nud_failure_value));
                    }
                    (Some(_), _) => {
                        unreachable!("frozen Pratt dispatch selected an unknown branch")
                    }
                };
                let _ = #expression_start;
                Ok(parsed)
            },
            nud_selection_items,
            quote!(#(#helper_methods)*),
        );
    }

    let (led_selection, led_selection_items) = render_predictive_selection(
        plan,
        node_index,
        pratt
            .led_dispatch
            .as_ref()
            .expect("validated led plan has a frozen dispatch"),
        &quote!(context.follow()),
        &mut expected_index,
        instrumentation,
    );
    let guard = pratt
        .caller_follow_guard
        .as_ref()
        .expect("validated led plan has a caller-FOLLOW guard");
    // The reported guard is the union of every extender still active at `min_bp`, and
    // that union is fixed per distinct threshold, so it is composed during generation
    // and selected here by one comparison chain on the error path only. The Pratt entry
    // path composes nothing.
    let guard_arms = pratt_guard_thresholds(guard, plan.kind_count)
        .into_iter()
        .map(|(threshold, expected)| {
            let expected = render_expected_set(plan, &expected);
            quote! {
                if min_bp <= #threshold {
                    return &#expected;
                }
            }
        })
        .collect::<Vec<_>>();
    let guard_definition = quote! {
        fn __recursa_pratt_guard(min_bp: u8) -> &'static #facade::__private::FollowSet {
            #(#guard_arms)*
            unreachable!("an active extender contributes an expectation")
        }
    };
    let led_offset = pratt.nud.len();
    let mut led_branches = Vec::new();
    for (index, branch) in pratt.led.iter().enumerate() {
        let metadata_index = led_offset + index;
        let lbp = pratt_led_lbp(branch.position);
        let required_prefix = pratt_led_required_prefix(branch)
            .into_iter()
            .enumerate()
            .map(|(depth, expected)| {
                let expected = render_expected_set(plan, expected);
                quote! {
                    context.peek(#depth)
                        .is_some_and(|record| #expected.contains(record.kind))
                }
            })
            .collect::<Vec<_>>();
        let continuation_viable = quote!(true #(&& #required_prefix)*);
        let body = render_pratt_led_parse(
            plan,
            node_index,
            metadata_index,
            &owner,
            branch,
            &expression_start,
        );
        let helper = format_ident!("__recursa_parse_pratt_led_{node_index}_{index}");
        helper_methods.push(quote! {
            fn #helper #helper_method_generics(
                context: &mut #facade::__private::ParseContext<'_, '__recursa_input, A>,
                lhs: #facade::__private::ParsedNode<Self>,
                #expression_start: usize,
                __recursa_pratt_excluded: &[usize],
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                Ok({ #body })
            }
        });
        led_branches.push(quote! {
            (Some(#index), _) => {
                if #lbp < min_bp { break; }
                if __recursa_pratt_excluded.contains(&#index) { break; }
                if context.next_is_pratt_boundary() {
                    if !(#continuation_viable) { break; }
                    if !context.next_is_greedy_extension() {
                        return Err(context.pratt_ambiguity(__recursa_pratt_guard(min_bp)));
                    }
                }
                lhs = Self::#helper(
                    context,
                    lhs,
                    #expression_start,
                    __recursa_pratt_excluded,
                )?;
                continue;
            }
        });
    }

    (
        quote! {
            #nud_failure_definition
            let #nud_follow_value = {
                static __RECURSA_FOLLOW_SITE: #facade::__private::FollowSiteCache =
                    #facade::__private::FollowSiteCache::new();
                __RECURSA_FOLLOW_SITE
                    .with_caller(&#nud_follow, context.follow())
                    .expect("matching generated Pratt FOLLOW widths")
            };
            let #expression_start = context.cursor();
            let mut lhs = match { #nud_selection } {
                #(#nud_branches,)*
                (None, failure_depth) => {
                    return Err(context.unexpected_at(failure_depth, &#nud_failure_value));
                }
                (Some(_), _) => {
                    unreachable!("frozen Pratt dispatch selected an unknown branch")
                }
            };
            #guard_definition
            loop {
                match { #led_selection } {
                    #(#led_branches)*
                    (None, _) => break,
                    (Some(_), _) => {
                        unreachable!("frozen Pratt dispatch selected an unknown branch")
                    }
                }
            }
            Ok(lhs)
        },
        quote!(#nud_selection_items #led_selection_items),
        quote!(#(#helper_methods)*),
    )
}

/// Returns the required fixed prefix that must remain viable before a selected LED reaches its
/// first semantic operand.
fn pratt_led_required_prefix(branch: &PrattLedParsePlan) -> Vec<&ExpectedSet> {
    let mut required = Vec::new();
    for element in &branch.variant.elements {
        match &element.kind {
            ElementParseKind::Fixed { expected, .. } => required.push(expected),
            ElementParseKind::OptionalFixedTokenGroup { .. } => break,
            ElementParseKind::Semantic { projection, .. } => {
                if let SemanticParseProjection::Direct { attachment, .. } = projection.as_ref()
                    && let Some(attachment) = attachment
                {
                    for fixed in &attachment.before {
                        match fixed {
                            AttachedFixedSyntaxParsePlan::Required(fixed) => {
                                required.push(&fixed.expected);
                            }
                            AttachedFixedSyntaxParsePlan::Optional(_) => return required,
                        }
                    }
                }
                break;
            }
        }
    }
    required
}

/// Returns the cumulative caller-FOLLOW guard for every distinct extender threshold.
///
/// The Pratt ambiguity diagnostic reports the union of the expectations of every
/// extender still active at the entry `min_bp`, so its content depends only on which
/// distinct left binding powers are at or above that threshold. Composing the
/// cumulative unions during generation keeps the diagnostic identical while removing
/// every runtime union from the Pratt entry path. Thresholds are returned ascending,
/// so the first threshold at or above `min_bp` names the guard to report.
pub(super) fn pratt_guard_thresholds(
    guard: &[PrattGuardBranchPlan],
    kind_count: usize,
) -> Vec<(u8, ExpectedSet)> {
    let mut thresholds = guard
        .iter()
        .map(|branch| pratt_led_lbp(branch.position))
        .collect::<Vec<_>>();
    thresholds.sort_unstable();
    thresholds.dedup();
    thresholds
        .into_iter()
        .map(|threshold| {
            let mut expected = ExpectedSet::empty(kind_count);
            for branch in guard
                .iter()
                .filter(|branch| pratt_led_lbp(branch.position) >= threshold)
            {
                expected.union_assign(&branch.expected);
            }
            (threshold, expected)
        })
        .collect()
}

/// Returns the left binding power at which one Pratt extender competes.
pub(super) fn pratt_led_lbp(position: PrattPosition) -> u8 {
    match position {
        PrattPosition::Infix { lbp, .. } => lbp,
        PrattPosition::Postfix { bp } => bp,
        PrattPosition::Atom | PrattPosition::Prefix { .. } => {
            unreachable!("a Pratt led plan has a nud position")
        }
    }
}

/// Emits one Pratt extender branch around the existing left operand.
///
/// # Long-function contract
///
/// Rationale: renders one led branch with its authoritative attachment and operand thresholds.
/// Invariants: lhs consumption and recursive operand binding power match the selected variant.
/// Phases: binds lhs, parses attachments and operands, constructs the variant, then advances provenance.
pub(super) fn render_pratt_led_parse(
    plan: &EmissionPlan,
    node_index: usize,
    variant_index: usize,
    owner: &proc_macro2::TokenStream,
    branch: &PrattLedParsePlan,
    expression_start: &Ident,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let variant = &branch.variant;
    let lhs_value = format_ident!("__recursa_lhs_value");
    let lhs_occurrence = format_ident!("__recursa_lhs_occurrence");
    let ElementParseKind::Semantic {
        field: lhs_field,
        projection: lhs_projection,
        ..
    } = &branch.lhs.kind
    else {
        unreachable!("a planned Pratt lhs is semantic")
    };
    let SemanticParseProjection::Direct {
        wrappers: lhs_wrappers,
        capture: lhs_capture,
        ..
    } = lhs_projection.as_ref()
    else {
        unreachable!("a planned Pratt lhs uses direct semantic projection")
    };
    let wrapped_lhs = render_wrapped_value(quote!(parsed_lhs), lhs_wrappers);
    let lhs_capture = lhs_capture.render(facade);
    let mut definitions = vec![quote! {
        let (parsed_lhs, parsed_lhs_occurrence) = lhs.into_parts();
        let parsed_lhs_span = context.consumed_span_from(#expression_start);
        let #lhs_occurrence = context.finish_group(
            #lhs_capture,
            parsed_lhs_span,
            [parsed_lhs_occurrence],
        )?;
        let #lhs_value = #wrapped_lhs;
    }];
    let mut values = vec![(lhs_field.clone(), lhs_value)];
    let mut occurrences = vec![lhs_occurrence];
    for (tail_index, element) in variant.elements.iter().enumerate() {
        let element_index = tail_index + 1;
        let value = format_ident!("__recursa_value_{element_index}");
        let occurrence = format_ident!("__recursa_occurrence_{element_index}");
        occurrences.push(occurrence.clone());
        let unique_index = variant_index * 1000 + element_index;
        let (definition, field) = render_element_parse(
            plan,
            node_index,
            unique_index,
            element,
            &value,
            &occurrence,
            Some(owner),
        );
        definitions.push(definition);
        if let Some(field) = field {
            values.push((field, value));
        }
    }
    let construction = render_construction(Some(&variant.ident), variant.style, &values);
    let finish = variant.capture.map_or_else(
        || quote!(context.finish_with_children(#construction, span, [#(#occurrences),*])),
        |capture| {
            let capture = capture.render(facade);
            quote!(context.finish_with_children_policy(
                #construction,
                span,
                #capture,
                [#(#occurrences),*],
            ))
        },
    );
    quote! {
        #(#definitions)*
        let span = context.consumed_span_from(#expression_start);
        #finish?
    }
}

/// Emits ordered parsing and construction of a structural sequence.
pub(super) fn render_sequence_parse(
    plan: &EmissionPlan,
    node_index: usize,
    variant: Option<(usize, &Ident)>,
    style: ResolvedFieldStyle,
    elements: &[ElementParsePlan],
    capture: Option<PlannedCapturePolicy>,
    pratt_owner: Option<&proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let start = format_ident!("__recursa_start");
    let mut definitions = Vec::new();
    let mut values = Vec::new();
    let mut occurrences = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let value = format_ident!("__recursa_value_{index}");
        let occurrence = format_ident!("__recursa_occurrence_{index}");
        occurrences.push(occurrence.clone());
        let unique_index = variant.map_or(index, |(variant, _)| variant * 1000 + index);
        let (definition, field) = render_element_parse(
            plan,
            node_index,
            unique_index,
            element,
            &value,
            &occurrence,
            pratt_owner,
        );
        definitions.push(definition);
        if let Some(field) = field {
            values.push((field, value));
        }
    }
    let target = variant.map(|(_, ident)| ident);
    let construction = render_construction(target, style, &values);
    let finish = capture.map_or_else(
        || quote!(context.finish_with_children(#construction, span, [#(#occurrences),*])),
        |capture| {
            let capture = capture.render(facade);
            quote!(context.finish_with_children_policy(
                #construction,
                span,
                #capture,
                [#(#occurrences),*],
            ))
        },
    );
    if elements.is_empty() {
        quote! {
            let _ = context;
            compile_error!("nullable empty Node bodies are not emitted by compound ticket #64");
        }
    } else {
        quote! {
            let #start = context.cursor();
            #(#definitions)*
            let span = context.consumed_span_from(#start);
            #finish
        }
    }
}

/// Renders one planned element and returns its construction slot when semantic.
fn render_element_parse(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    element: &ElementParsePlan,
    value: &Ident,
    occurrence: &Ident,
    pratt_owner: Option<&proc_macro2::TokenStream>,
) -> (proc_macro2::TokenStream, Option<FieldPlan>) {
    let facade = &plan.facade;
    match &element.kind {
        ElementParseKind::Fixed {
            expected, capture, ..
        } => {
            let expected = render_expected_set(plan, expected);
            let capture = capture.render(facade);
            (
                quote! {
                    let #occurrence = context.parse_fixed(&#expected, #capture)?;
                },
                None,
            )
        }
        ElementParseKind::OptionalFixedTokenGroup { tokens, .. } => (
            render_optional_fixed_group_parse(
                plan,
                node_index,
                unique_index,
                occurrence,
                tokens,
                "optional_fixed",
            ),
            None,
        ),
        ElementParseKind::Semantic { field, projection } => {
            let rendered = match projection.as_ref() {
                SemanticParseProjection::FixedPresence {
                    before,
                    sequence,
                    after,
                    ..
                } => render_fixed_token_presence_parse(
                    plan,
                    node_index,
                    unique_index,
                    value,
                    occurrence,
                    before,
                    sequence,
                    after,
                ),
                SemanticParseProjection::Direct {
                    terminal,
                    wrappers,
                    container,
                    capture,
                    child_capture,
                    pratt_min_bp,
                    pratt_excluded_led,
                    attachment,
                    ..
                } => render_semantic_parse(
                    plan,
                    node_index,
                    unique_index,
                    value,
                    occurrence,
                    terminal,
                    wrappers,
                    container,
                    *capture,
                    *child_capture,
                    pratt_owner,
                    *pratt_min_bp,
                    pratt_excluded_led,
                    attachment.as_deref(),
                ),
            };
            (rendered, Some(field.clone()))
        }
    }
}

/// Emits one required envelope containing an atomically selected fixed-token presence sequence.
///
/// # Long-function contract
///
/// Rationale: one renderer must keep the semantic boolean, optional token occurrence, and
/// surrounding fixed-syntax envelope in a single generated parser transaction.
/// Invariants: the presence sequence is non-empty and source-validated; its complete expected
/// sequence is peeked before any member is consumed; before and after syntax remains required or
/// independently atomic according to its normalized plan; every occurrence retains source order.
/// Phases: render immutable expected-token recipes, render the surrounding fixed syntax, emit the
/// complete-sequence lookahead and conditional consumption, then finish the optional occurrence
/// and its containing envelope.
#[allow(
    clippy::too_many_arguments,
    reason = "presence rendering consumes one frozen before/sequence/after parser recipe"
)]
pub(super) fn render_fixed_token_presence_parse(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    value: &Ident,
    occurrence: &Ident,
    before: &[AttachedFixedSyntaxParsePlan],
    sequence: &[AttachedFixedParsePlan],
    after: &[AttachedFixedSyntaxParsePlan],
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let group_capture = sequence
        .first()
        .expect("presence sequences are source-validated non-empty")
        .capture
        .render(facade);
    let render_tokens = |role: &str, tokens: &[AttachedFixedParsePlan]| {
        tokens
            .iter()
            .enumerate()
            .map(|(index, fixed)| {
                let expected = render_expected_set(plan, &fixed.expected);
                let occurrence = format_ident!("__recursa_presence_{role}_{index}");
                let capture = fixed.capture.render(facade);
                (
                    expected.clone(),
                    quote!(let #occurrence = context.parse_fixed(&#expected, #capture)?;),
                    occurrence,
                )
            })
            .collect::<Vec<_>>()
    };
    let render_syntax = |role: &str, syntax: &[AttachedFixedSyntaxParsePlan]| {
        syntax
            .iter()
            .enumerate()
            .map(|(index, syntax)| {
                let occurrence = format_ident!("__recursa_presence_{role}_{index}");
                let statement = match syntax {
                    AttachedFixedSyntaxParsePlan::Required(fixed) => {
                        let expected = render_expected_set(plan, &fixed.expected);
                        let capture = fixed.capture.render(facade);
                        quote! {
                            let #occurrence = context.parse_fixed(&#expected, #capture)?;
                        }
                    }
                    AttachedFixedSyntaxParsePlan::Optional(tokens) => {
                        render_optional_fixed_group_parse(
                            plan,
                            node_index,
                            unique_index,
                            &occurrence,
                            tokens,
                            &format!("presence_{role}_{index}"),
                        )
                    }
                };
                (statement, occurrence)
            })
            .collect::<Vec<_>>()
    };
    let before = render_syntax("before", before);
    let sequence = render_tokens("sequence", sequence);
    let after = render_syntax("after", after);
    let before_statements = before.iter().map(|(statement, _)| statement);
    let before_occurrences = before.iter().map(|(_, occurrence)| occurrence);
    let sequence_matches = sequence.iter().enumerate().map(|(index, (expected, _, _))| {
        quote!(context.peek(#index).is_some_and(|record| #expected.contains(record.kind)))
    });
    let sequence_statements = sequence.iter().map(|(_, statement, _)| statement);
    let sequence_occurrences = sequence.iter().map(|(_, _, occurrence)| occurrence);
    let after_statements = after.iter().map(|(statement, _)| statement);
    let after_occurrences = after.iter().map(|(_, occurrence)| occurrence);
    quote! {
        let __recursa_presence_start = context.cursor();
        #(#before_statements)*
        let #value = true #(&& #sequence_matches)*;
        let __recursa_presence_occurrence = if #value {
            let __recursa_optional_start = context.cursor();
            #(#sequence_statements)*
            let __recursa_optional_span = context.consumed_span_from(__recursa_optional_start);
            context.finish_group(
                #group_capture,
                __recursa_optional_span,
                [#(#sequence_occurrences),*],
            )?
        } else {
            context.finish_group(#group_capture, None, ::std::iter::empty())?
        };
        #(#after_statements)*
        let __recursa_presence_span = context.consumed_span_from(__recursa_presence_start);
        let #occurrence = context.finish_group(
            #group_capture,
            __recursa_presence_span,
            [#(#before_occurrences,)* __recursa_presence_occurrence, #(#after_occurrences),*],
        )?;
    }
}

/// Emits one erased fixed-token group only when its complete sequence is present.
pub(super) fn render_optional_fixed_group_parse(
    plan: &EmissionPlan,
    _node_index: usize,
    _unique_index: usize,
    occurrence: &Ident,
    tokens: &[AttachedFixedParsePlan],
    role: &str,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let group_capture = PlannedCapturePolicy::Mandatory.render(facade);
    let tokens = tokens
        .iter()
        .enumerate()
        .map(|(index, fixed)| {
            let expected = render_expected_set(plan, &fixed.expected);
            let child = format_ident!("__recursa_{role}_{index}");
            let capture = fixed.capture.render(facade);
            (
                expected.clone(),
                quote!(let #child = context.parse_fixed(&#expected, #capture)?;),
                child,
            )
        })
        .collect::<Vec<_>>();
    let matches = tokens.iter().enumerate().map(|(index, (expected, _, _))| {
        quote!(context.peek(#index).is_some_and(|record| #expected.contains(record.kind)))
    });
    let statements = tokens.iter().map(|(_, statement, _)| statement);
    let children = tokens.iter().map(|(_, _, child)| child);
    quote! {
        let #occurrence = if true #(&& #matches)* {
            let __recursa_optional_fixed_start = context.cursor();
            #(#statements)*
            let __recursa_optional_fixed_span =
                context.consumed_span_from(__recursa_optional_fixed_start);
            context.finish_group(
                #group_capture,
                __recursa_optional_fixed_span,
                [#(#children),*],
            )?
        } else {
            context.finish_group(#group_capture, None, ::std::iter::empty())?
        };
    }
}

/// Emits compact table-driven traversal of sparse optional consume-vs-skip overrides.
///
/// # Long-function contract
///
/// Rationale: the shared packed width, table offsets, decoder, and traversal must be derived from
/// one canonical registry so emitted cells cannot drift between separately rendered fragments.
/// Invariants: every state has four cells, every edge has two cells, edge predicates are exact and
/// disjoint, traversal never consumes input, a missing action delegates to the root kind selector,
/// and caller FOLLOW is tested only at the current depth after all outgoing edges fail.
/// Phases: interns expected positions, flattens graph tables, selects one shared packed width,
/// renders the position decoder, then emits the bounded consume-or-skip traversal.
fn render_optional_viability_decision_selection(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    decision: &OptionalViabilityDecisionParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let table_name = format_ident!("__RECURSA_OPTIONAL_DECISION_{node_index}_{unique_index}");
    let mut position_registry = BTreeMap::<DispatchPositionPlan, usize>::new();
    let mut positions = Vec::new();
    let mut edge_values = Vec::new();
    let mut state_values = Vec::with_capacity(decision.states.len() * 4);
    let encode_action = |action: crate::OptionalAction| match action {
        crate::OptionalAction::Consume => 1usize,
        crate::OptionalAction::Skip => 2usize,
    };
    for node in &decision.states {
        let mut caller_action = 0usize;
        let mut unconditional_action = 0usize;
        for terminal in &node.terminals {
            if terminal.on_caller_follow {
                caller_action = encode_action(terminal.action);
            } else {
                unconditional_action = encode_action(terminal.action);
            }
        }
        let edge_start = edge_values.len() / 2;
        for edge in &node.edges {
            let position = if let Some(position) = position_registry.get(&edge.position) {
                *position
            } else {
                let position = positions.len();
                let expected_index = plan
                    .expected_sets
                    .id(&edge.position.expected)
                    .expect("validated optional expectation belongs to the grammar-wide pool");
                positions.push((edge.position.clone(), expected_index));
                position_registry.insert(edge.position.clone(), position);
                position
            };
            edge_values.extend([position, edge.next.index()]);
        }
        let edge_end = edge_values.len() / 2;
        state_values.extend([
            caller_action,
            unconditional_action,
            edge_start,
            edge_end - edge_start,
        ]);
    }
    let encoded_positions = encode_dispatch_positions(
        plan.text_equivalence,
        &positions
            .iter()
            .map(|(position, expected_index)| (*expected_index, position.text_predicate.clone()))
            .collect::<Vec<_>>(),
    );
    let mut packed_values = state_values.clone();
    packed_values.extend(&edge_values);
    packed_values.extend(&encoded_positions.table);
    let (packed_width, _) = packed_table(&packed_values);
    let (_, states_literal) = packed_table_with_width(&state_values, packed_width);
    let (_, edges_literal) = packed_table_with_width(&edge_values, packed_width);
    let (_, positions_literal) = packed_table_with_width(&encoded_positions.table, packed_width);
    let root = decision.root.index();
    let spellings = &encoded_positions.spellings;
    quote! {{
        static #table_name: #facade::__private::DispatchTable =
            #facade::__private::dispatch_table(
                #states_literal,
                #edges_literal,
                #positions_literal,
                &[#(#spellings),*],
                #packed_width,
                4usize,
                2usize,
                &#module_anchor::__RECURSA_EXPECTED_SETS,
            );
        context.select_optional(&#table_name, #root)
    }}
}

#[allow(clippy::too_many_arguments)]
/// Emits parsing, capture, and storage for one semantic field.
///
/// # Long-function contract
///
/// Rationale: renders a semantic grammar element and its declared container operations together.
/// Invariants: caller FOLLOW is threaded only where the frozen recipe requires it.
/// Phases: selects terminal parse entry, applies wrappers, captures provenance, then reports the value.
pub(super) fn render_semantic_parse(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    value: &Ident,
    occurrence: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
    container: &PlannedContainer,
    capture: PlannedCapturePolicy,
    child_capture: PlannedCapturePolicy,
    pratt_owner: Option<&proc_macro2::TokenStream>,
    pratt_min_bp: Option<u8>,
    pratt_excluded_led: &[usize],
    attachment: Option<&FixedAttachmentParsePlan>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let capture = capture.render(facade);
    let child_capture = child_capture.render(facade);
    match container {
        PlannedContainer::Direct {
            child_follow,
            required_failure,
        } => {
            let follow_name = expected_static(node_index, unique_index, "child_follow");
            let (follow_definition, follow_value) =
                render_follow_value(plan, &follow_name, child_follow);
            let failure_name = expected_static(node_index, unique_index, "required_failure");
            let (failure_definition, failure_value) =
                render_follow_value(plan, &failure_name, required_failure);
            let wrapped = render_wrapped_value(quote!(parsed_value), wrappers);
            let authored_exclusions = quote!(&[#(#pratt_excluded_led),*]);
            let direct_child = if !pratt_excluded_led.is_empty() {
                let min_bp = pratt_min_bp.unwrap_or(0);
                quote! {
                    context.parse_generated(&#follow_value, #child_capture, |context| {
                        <#terminal>::__recursa_parse_bp(
                            context,
                            #min_bp,
                            #authored_exclusions,
                        )
                    })
                }
            } else if let (Some(owner), Some(min_bp)) = (pratt_owner, pratt_min_bp) {
                quote! {
                    context.parse_generated(&#follow_value, #child_capture, |context| {
                        #owner::__recursa_parse_bp(
                            context,
                            #min_bp,
                            __recursa_pratt_excluded,
                        )
                    })
                }
            } else {
                quote! {
                    context.parse_node::<#terminal>(&#follow_value, #child_capture)
                }
            };
            let (attachment_definitions, parse_child) = attachment.map_or_else(
                || (quote!(), direct_child),
                |attachment| {
                    render_attachment_child(
                        plan,
                        node_index,
                        unique_index,
                        terminal,
                        attachment,
                        &follow_value,
                        &child_capture,
                        pratt_owner,
                        pratt_min_bp,
                        pratt_excluded_led,
                    )
                },
            );
            let plain_child = attachment.is_none()
                && pratt_excluded_led.is_empty()
                && !(pratt_owner.is_some() && pratt_min_bp.is_some());
            if plain_child {
                // The runtime owns the element transaction: start cursor, the
                // no-progress failure rewrite, and the element group.
                return quote! {
                    #follow_definition
                    #failure_definition
                    let (parsed_value, #occurrence) = context.parse_direct_child::<#terminal>(
                        &#follow_value,
                        #child_capture,
                        #capture,
                        &#failure_value,
                    )?;
                    let #value = #wrapped;
                };
            }
            quote! {
                #follow_definition
                #failure_definition
                #attachment_definitions
                let element_start = context.cursor();
                let (parsed_value, child_occurrence) = match #parse_child {
                    Ok(parsed) => parsed.into_parts(),
                    Err(error)
                        if context.cursor() == element_start
                            && context.peek(0).map_or_else(
                                || error.span().start() as usize == context.source().len(),
                                |record| error.span() == record.span,
                            ) =>
                    {
                        return Err(context.unexpected(&#failure_value));
                    }
                    Err(error) => return Err(error),
                };
                let element_span = context.consumed_span_from(element_start);
                let #occurrence = context.finish_group(
                    #capture,
                    element_span,
                    [child_occurrence],
                )?;
                let #value = #wrapped;
            }
        }
        PlannedContainer::Optional {
            selector,
            decision,
            child_follow,
        } => {
            let selector = render_expected_set(plan, selector);
            let decision = decision
                .as_deref()
                .map(|decision| {
                    render_optional_viability_decision_selection(
                        plan,
                        node_index,
                        unique_index,
                        decision,
                    )
                })
                .unwrap_or_else(|| quote!(None));
            let follow_name = expected_static(node_index, unique_index, "child_follow");
            let (follow_definition, follow_value) =
                render_follow_value(plan, &follow_name, child_follow);
            let wrapped = render_wrapped_value(quote!(parsed_value), wrappers);
            let absent = render_absent_value(wrappers);
            let authored_exclusions = quote!(&[#(#pratt_excluded_led),*]);
            let direct_child = if pratt_excluded_led.is_empty() {
                quote!(context.parse_node::<#terminal>(
                    &#follow_value,
                    #child_capture,
                ))
            } else {
                quote! {
                    context.parse_generated(&#follow_value, #child_capture, |context| {
                        <#terminal>::__recursa_parse_bp(context, 0, #authored_exclusions)
                    })
                }
            };
            let (attachment_definitions, parse_child) = attachment.map_or_else(
                || (quote!(), direct_child),
                |attachment| {
                    render_attachment_child(
                        plan,
                        node_index,
                        unique_index,
                        terminal,
                        attachment,
                        &follow_value,
                        &child_capture,
                        pratt_owner,
                        pratt_min_bp,
                        pratt_excluded_led,
                    )
                },
            );
            quote! {
                #follow_definition
                let (#value, #occurrence) = if #decision
                    .unwrap_or_else(|| context.next_is(&#selector))
                {
                    let element_start = context.cursor();
                    #attachment_definitions
                    let (parsed_value, child_occurrence) = #parse_child?.into_parts();
                    let element_span = context.consumed_span_from(element_start);
                    let occurrence = context.finish_group(
                        #capture,
                        element_span,
                        [child_occurrence],
                    )?;
                    (#wrapped, occurrence)
                } else {
                    (#absent, context.finish_group(
                        #capture,
                        None,
                        ::std::iter::empty(),
                    )?)
                };
            }
        }
        PlannedContainer::Repeated {
            operation,
            element_selector,
            separator_selector,
            optional_trailing,
            stop,
            child_follow,
            minimum_failure,
            continuation_failure,
            after_separator_failure,
        } => render_repeated_parse(
            plan,
            node_index,
            unique_index,
            value,
            occurrence,
            terminal,
            wrappers,
            *operation,
            element_selector,
            separator_selector.as_ref(),
            *optional_trailing,
            stop,
            child_follow,
            minimum_failure.as_deref(),
            continuation_failure.as_ref(),
            after_separator_failure.as_deref(),
            capture,
            child_capture,
            pratt_excluded_led,
            attachment,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
/// Emits a semantic child surrounded by its required fixed attachments.
///
/// # Long-function contract
///
/// Rationale: renders fixed attachments adjacent to their owning semantic occurrence.
/// Invariants: attachment order, optional presence, and span accounting match the parse plan.
/// Phases: matches fixed tokens, records provenance, then returns the child expression.
pub(super) fn render_attachment_child(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    terminal: &Type,
    attachment: &FixedAttachmentParsePlan,
    outer_follow: &proc_macro2::TokenStream,
    child_capture: &proc_macro2::TokenStream,
    pratt_owner: Option<&proc_macro2::TokenStream>,
    pratt_min_bp: Option<u8>,
    pratt_excluded_led: &[usize],
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let facade = &plan.facade;
    let PlannedContainer::Direct {
        child_follow,
        required_failure,
    } = attachment.semantic.as_ref()
    else {
        unreachable!("attachment semantic sites are frozen direct containers")
    };
    let semantic_follow_name =
        expected_static(node_index, unique_index, "attachment_semantic_follow");
    let (semantic_follow_definition, semantic_follow_value) =
        render_follow_value(plan, &semantic_follow_name, child_follow);
    let semantic_failure_name =
        expected_static(node_index, unique_index, "attachment_semantic_failure");
    let (semantic_failure_definition, semantic_failure_value) =
        render_follow_value(plan, &semantic_failure_name, required_failure);
    let authored_exclusions = quote!(&[#(#pratt_excluded_led),*]);
    let semantic_parse = if !pratt_excluded_led.is_empty() {
        let min_bp = pratt_min_bp.unwrap_or(0);
        quote! {
            context.parse_generated(&#semantic_follow_value, #child_capture, |context| {
                <#terminal>::__recursa_parse_bp(context, #min_bp, #authored_exclusions)
            })
        }
    } else if let (Some(owner), Some(min_bp)) = (pratt_owner, pratt_min_bp) {
        quote! {
            context.parse_generated(&#semantic_follow_value, #child_capture, |context| {
                #owner::__recursa_parse_bp(context, #min_bp, __recursa_pratt_excluded)
            })
        }
    } else {
        quote! {
            context.parse_node::<#terminal>(&#semantic_follow_value, #child_capture)
        }
    };
    let render_syntax = |syntax: &AttachedFixedSyntaxParsePlan, role: &str, index: usize| {
        let occurrence = format_ident!("__recursa_attachment_{role}_{index}");
        match syntax {
            AttachedFixedSyntaxParsePlan::Required(fixed) => {
                let expected = render_expected_set(plan, &fixed.expected);
                let capture = fixed.capture.render(facade);
                (
                    quote! {
                        let #occurrence = context.parse_fixed(&#expected, #capture)?;
                    },
                    occurrence,
                )
            }
            AttachedFixedSyntaxParsePlan::Optional(tokens) => (
                render_optional_fixed_group_parse(
                    plan,
                    node_index,
                    unique_index,
                    &occurrence,
                    tokens,
                    &format!("attachment_{role}_{index}"),
                ),
                occurrence,
            ),
        }
    };
    let before = attachment
        .before
        .iter()
        .enumerate()
        .map(|(index, syntax)| render_syntax(syntax, "before", index))
        .collect::<Vec<_>>();
    let after = attachment
        .after
        .iter()
        .enumerate()
        .map(|(index, syntax)| render_syntax(syntax, "after", index))
        .collect::<Vec<_>>();
    let before_statements = before.iter().map(|(statement, _)| statement);
    let before_occurrences = before.iter().map(|(_, occurrence)| occurrence);
    let after_statements = after.iter().map(|(statement, _)| statement);
    let after_occurrences = after.iter().map(|(_, occurrence)| occurrence);
    let parse = quote! {
        context.parse_generated(&#outer_follow, #child_capture, |context| {
            #semantic_follow_definition
            #semantic_failure_definition
            let attachment_start = context.cursor();
            #(#before_statements)*
            let semantic_start = context.cursor();
            let (parsed_value, semantic_occurrence) = match #semantic_parse {
                Ok(parsed) => parsed.into_parts(),
                Err(error)
                    if context.cursor() == semantic_start
                        && context.peek(0).map_or_else(
                            || error.span().start() as usize == context.source().len(),
                            |record| error.span() == record.span,
                        ) =>
                {
                    return Err(context.unexpected(&#semantic_failure_value));
                }
                Err(error) => return Err(error),
            };
            #(#after_statements)*
            let attachment_span = context.consumed_span_from(attachment_start);
            context.finish_with_children(
                parsed_value,
                attachment_span,
                [
                    #(#before_occurrences,)*
                    semantic_occurrence,
                    #(#after_occurrences,)*
                ],
            )
        })
    };
    (quote!(), parse)
}

#[allow(clippy::too_many_arguments)]
/// Emits the repetition loop, separator handling, and minimum-count failures.
///
/// # Long-function contract
///
/// Rationale: renders repeated parsing as one loop because separator and FOLLOW state are interdependent.
/// Invariants: minimum cardinality, separator policy, and caller FOLLOW remain explicit.
/// Phases: parses the first item when required, loops over separators/items, then builds the declared container.
pub(super) fn render_repeated_parse(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    value: &Ident,
    occurrence: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
    operation: ContainerOperation,
    element_selector: &ExpectedSet,
    separator_selector: Option<&ExpectedSet>,
    optional_trailing: bool,
    stop: &PlannedFollow,
    child_follow: &PlannedFollow,
    minimum_failure: Option<&PlannedFollow>,
    continuation_failure: &PlannedFollow,
    after_separator_failure: Option<&PlannedFollow>,
    capture: proc_macro2::TokenStream,
    child_capture: proc_macro2::TokenStream,
    pratt_excluded_led: &[usize],
    attachment: Option<&FixedAttachmentParsePlan>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let repeat_index = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore))
        .expect("a repeated container plan has a repetition wrapper");
    let outer = &wrappers[..repeat_index];
    let inner = &wrappers[repeat_index + 1..];
    let repeated_wrapper = wrappers[repeat_index];
    let wrapped_item = render_wrapped_value(quote!(parsed_value), inner);

    let element_expected = render_expected_set(plan, element_selector);
    let stop_name = expected_static(node_index, unique_index, "stop");
    let (stop_definition, stop_value) = render_follow_value(plan, &stop_name, stop);
    let child_follow_name = expected_static(node_index, unique_index, "child_follow");
    let (child_follow_definition, child_follow_value) =
        render_follow_value(plan, &child_follow_name, child_follow);
    let minimum_name = expected_static(node_index, unique_index, "minimum_failure");
    let (minimum_definition, minimum_value) = minimum_failure.map_or_else(
        || (quote!(), quote!()),
        |failure| render_follow_value(plan, &minimum_name, failure),
    );
    let continuation_name = expected_static(node_index, unique_index, "continuation_failure");
    let (continuation_definition, continuation_value) =
        render_follow_value(plan, &continuation_name, continuation_failure);
    let after_separator_name = expected_static(node_index, unique_index, "after_separator_failure");
    let (after_separator_definition, after_separator_value) = after_separator_failure.map_or_else(
        || (quote!(), quote!()),
        |failure| render_follow_value(plan, &after_separator_name, failure),
    );
    let authored_exclusions = quote!(&[#(#pratt_excluded_led),*]);
    let direct_child = if pratt_excluded_led.is_empty() {
        quote!(context.parse_node::<#terminal>(
            &#child_follow_value,
            #child_capture,
        ))
    } else {
        quote! {
            context.parse_generated(&#child_follow_value, #child_capture, |context| {
                <#terminal>::__recursa_parse_bp(context, 0, #authored_exclusions)
            })
        }
    };
    let (attachment_definitions, parse_child) = attachment.map_or_else(
        || (quote!(), direct_child),
        |attachment| {
            render_attachment_child(
                plan,
                node_index,
                unique_index,
                terminal,
                attachment,
                &child_follow_value,
                &child_capture,
                None,
                None,
                pratt_excluded_led,
            )
        },
    );
    let parse_item = quote! {
        let (parsed_value, child_occurrence) = #parse_child?.into_parts();
        parsed_values.push(#wrapped_item);
        child_occurrences.push(child_occurrence);
    };
    let start = if operation == ContainerOperation::OneOrMore {
        quote! {
            if !context.next_is(&#element_expected) {
                return Err(context.unexpected(&#minimum_value));
            }
            #parse_item
        }
    } else if minimum_failure.is_some() {
        quote! {
            if context.next_is(&#element_expected) {
                #parse_item
            } else if !context.is_eof() && !context.next_is(&#stop_value) {
                return Err(context.unexpected(&#minimum_value));
            }
        }
    } else {
        quote! {
            if context.next_is(&#element_expected) {
                #parse_item
            } else if !context.is_eof() && !context.next_is(&#stop_value) {
                return Err(context.unexpected(&#continuation_value));
            }
        }
    };

    let continuation = if let Some(separator_selector) = separator_selector {
        let separator_expected = render_expected_set(plan, separator_selector);
        let trailing = if optional_trailing {
            quote! {
                if context.is_eof() || context.next_is(&#stop_value) {
                    break;
                }
            }
        } else {
            quote! {}
        };
        quote! {
            loop {
                if !context.next_is(&#separator_expected) {
                    if context.is_eof() || context.next_is(&#stop_value) {
                        break;
                    }
                    return Err(context.unexpected(&#continuation_value));
                }
                let separator_occurrence =
                    context.parse_repetition_separator(&#separator_expected)?;
                separator_occurrences.push(separator_occurrence);
                if context.next_is(&#element_expected) {
                    #parse_item
                    continue;
                }
                #trailing
                return Err(context.unexpected(&#after_separator_value));
            }
        }
    } else {
        quote! {
            loop {
                if context.next_is(&#element_expected) {
                    #parse_item
                    continue;
                }
                if context.is_eof() || context.next_is(&#stop_value) {
                    break;
                }
                return Err(context.unexpected(&#continuation_value));
            }
        }
    };

    let collection = match repeated_wrapper {
        FieldWrapper::ZeroOrMore => quote!(parsed_values),
        FieldWrapper::OneOrMore => quote!({
            let mut values = parsed_values.into_iter();
            let first = values
                .next()
                .expect("a present one-or-more container parsed its first value");
            let mut non_empty = #facade::Vec1::new(first);
            for value in values {
                non_empty.push(value);
            }
            non_empty
        }),
        FieldWrapper::Boxed | FieldWrapper::Optional => unreachable!(),
    };
    let present = render_wrapped_value(collection, outer);
    let has_values = if operation == ContainerOperation::OptionalOneOrMore {
        quote!(let has_values = !parsed_values.is_empty();)
    } else {
        quote!()
    };
    let semantic = if operation == ContainerOperation::OptionalOneOrMore {
        let absent = render_absent_value(outer);
        quote! {
            let #value = if !has_values {
                #absent
            } else {
                #present
            };
        }
    } else {
        quote!(let #value = #present;)
    };
    let separator_occurrences = if separator_selector.is_some() {
        quote!(let mut separator_occurrences = Vec::new();)
    } else {
        quote!(let separator_occurrences = Vec::new();)
    };

    quote! {
        #stop_definition
        #child_follow_definition
        #minimum_definition
        #continuation_definition
        #after_separator_definition
        #attachment_definitions
        let element_start = context.cursor();
        let mut parsed_values = Vec::new();
        let mut child_occurrences = #facade::__private::Occurrences::new();
        #separator_occurrences
        #start
        if !parsed_values.is_empty() {
            #continuation
        }
        let element_span = context.consumed_span_from(element_start);
        #has_values
        let #occurrence = context.finish_repeated_group(
            #capture,
            element_span,
            child_occurrences,
            separator_occurrences,
        )?;
        #semantic
    }
}

/// Emits the runtime FOLLOW value represented by a frozen policy.
pub(super) fn render_follow_value(
    plan: &EmissionPlan,
    name: &Ident,
    follow: &PlannedFollow,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let facade = &plan.facade;
    let value = format_ident!("{}_value", name.to_string().to_ascii_lowercase());
    // Every binding is a `&'static FollowSet`. Static pool entries and the caller FOLLOW
    // are borrowed directly; composed sets come from the site cache emitted next to the
    // use, so a repeated caller costs one atomic load and one pointer comparison.
    match follow {
        PlannedFollow::Caller => (
            quote!(let #value = context.caller_follow();),
            quote!(#value),
        ),
        PlannedFollow::Static(expected) => {
            let expected = render_expected_set(plan, expected);
            (quote!(let #value = &#expected;), quote!(#value))
        }
        PlannedFollow::StaticAndCaller(expected) => (
            render_site_composition(
                plan,
                &quote!(let #value =),
                expected,
                SiteComposition::WithCaller,
            ),
            quote!(#value),
        ),
        PlannedFollow::CallerAndPrattStops(expected) => (
            render_site_composition(
                plan,
                &quote!(let #value =),
                expected,
                SiteComposition::Union,
            ),
            quote!(#value),
        ),
        // The acceptance rides the composition the site already performs: the composed
        // FOLLOW, its expectations, and its Pratt boundary are unchanged, and only the
        // accepted kinds are marked, so a Pratt child reached here extends on them
        // instead of reporting `RCA4102`. One site cache still answers one visit.
        PlannedFollow::Accepting { .. } => {
            let (inner, accepted) = flatten_acceptance(follow, plan.kind_count);
            let accepted = render_expected_set(plan, &accepted);
            let composition = match inner {
                // A caller-only FOLLOW is marked by unioning it with nothing.
                PlannedFollow::Caller => {
                    let empty = render_expected_set(plan, &ExpectedSet::empty(plan.kind_count));
                    quote!(.union_accepting(&#empty, context.follow(), &#accepted))
                }
                // A caller-independent FOLLOW composes with itself, which changes no field.
                PlannedFollow::Static(expected) => {
                    let expected = render_expected_set(plan, expected);
                    quote!(.with_caller_accepting(&#expected, &#expected, &#accepted))
                }
                PlannedFollow::StaticAndCaller(expected) => {
                    let expected = render_expected_set(plan, expected);
                    quote!(.with_caller_accepting(&#expected, context.follow(), &#accepted))
                }
                PlannedFollow::CallerAndPrattStops(expected) => {
                    let expected = render_expected_set(plan, expected);
                    quote!(.union_accepting(&#expected, context.follow(), &#accepted))
                }
                PlannedFollow::Accepting { .. } => {
                    unreachable!("flattening removes every nested acceptance")
                }
            };
            (
                quote! {
                    let #value = {
                        static __RECURSA_FOLLOW_SITE: #facade::__private::FollowSiteCache =
                            #facade::__private::FollowSiteCache::new();
                        __RECURSA_FOLLOW_SITE
                            #composition
                            .expect("matching generated greedy acceptance widths")
                    };
                },
                quote!(#value),
            )
        }
    }
}

/// Collapses nested acceptances into the composition they wrap and one union of their kinds.
///
/// Acceptances are attached once per site and the FOLLOW combinators rebuild the wrapped
/// policy in place, so nesting does not arise; unioning the marks keeps the meaning if it
/// ever did, because the mark is a set of kinds the Pratt loop may extend on.
fn flatten_acceptance(follow: &PlannedFollow, kind_count: usize) -> (&PlannedFollow, ExpectedSet) {
    let mut current = follow;
    let mut accepted = ExpectedSet::empty(kind_count);
    while let PlannedFollow::Accepting {
        inner,
        accepted: kinds,
    } = current
    {
        accepted.union_assign(kinds);
        current = inner;
    }
    (current, accepted)
}

#[derive(Clone, Copy)]
/// Which cached composition one generated FOLLOW site performs.
pub(super) enum SiteComposition {
    /// Keeps the nearer Pratt boundary (`FollowSet::with_caller`).
    WithCaller,
    /// Unions both Pratt boundaries (`FollowSet::union`).
    Union,
}

/// Emits one composition site with its own inline cache.
///
/// The cache is a block-scoped `static`, so every use site owns an independent
/// one-entry cache without a grammar-wide naming scheme, and the entry outlives the
/// parse exactly as the interned result does.
pub(super) fn render_site_composition(
    plan: &EmissionPlan,
    binding: &proc_macro2::TokenStream,
    expected: &ExpectedSet,
    composition: SiteComposition,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let expected = render_expected_set(plan, expected);
    let (method, message) = match composition {
        SiteComposition::WithCaller => (
            format_ident!("with_caller"),
            "matching generated FOLLOW widths",
        ),
        SiteComposition::Union => (
            format_ident!("union"),
            "matching generated Pratt stop/FOLLOW widths",
        ),
    };
    quote! {
        #binding {
            static __RECURSA_FOLLOW_SITE: #facade::__private::FollowSiteCache =
                #facade::__private::FollowSiteCache::new();
            __RECURSA_FOLLOW_SITE
                .#method(&#expected, context.follow())
                .expect(#message)
        };
    }
}

/// Emits named, tuple, or unit construction for parsed field values.
pub(super) fn render_construction(
    variant: Option<&Ident>,
    style: ResolvedFieldStyle,
    values: &[(FieldPlan, Ident)],
) -> proc_macro2::TokenStream {
    let target = variant.map_or_else(|| quote!(Self), |variant| quote!(Self::#variant));
    match style {
        ResolvedFieldStyle::Named => {
            let fields = values.iter().map(|(field, value)| {
                let ident = field
                    .ident
                    .as_ref()
                    .expect("named source shape has named fields");
                quote!(#ident: #value)
            });
            quote!(#target { #(#fields),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let mut ordered = values.iter().collect::<Vec<_>>();
            ordered.sort_by_key(|(field, _)| field.index);
            let fields = ordered.into_iter().map(|(_, value)| value);
            quote!(#target(#(#fields),*))
        }
        ResolvedFieldStyle::Unit => quote!(#target),
    }
}

/// Applies authored storage wrappers to a parsed value expression.
pub(super) fn render_wrapped_value(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
) -> proc_macro2::TokenStream {
    wrappers
        .iter()
        .rev()
        .fold(value, |value, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!(Box::new(#value)),
            FieldWrapper::Optional => quote!(Some(#value)),
            FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {
                unreachable!("a repetition wrapper is constructed separately")
            }
        })
}

/// Emits the empty value required by optional and repeated wrappers.
pub(super) fn render_absent_value(wrappers: &[FieldWrapper]) -> proc_macro2::TokenStream {
    let optional = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::Optional))
        .expect("an absent field has an optional wrapper");
    wrappers[..optional]
        .iter()
        .rev()
        .fold(quote!(None), |value, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!(Box::new(#value)),
            FieldWrapper::Optional => unreachable!("the first optional ends the absent value"),
            FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {
                unreachable!("an absent outer wrapper cannot contain a repetition")
            }
        })
}

/// Applies authored storage wrappers to a terminal type.
pub(super) fn render_wrapped_type(
    facade: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
) -> Type {
    wrappers
        .iter()
        .rev()
        .fold(terminal.clone(), |ty, wrapper| match wrapper {
            FieldWrapper::Boxed => syn::parse_quote!(Box<#ty>),
            FieldWrapper::Optional => syn::parse_quote!(Option<#ty>),
            FieldWrapper::ZeroOrMore => syn::parse_quote!(Vec<#ty>),
            FieldWrapper::OneOrMore => syn::parse_quote!(#facade::Vec1<#ty>),
        })
}

/// Emits a reference to a terminal value through its wrapper shape.
pub(super) fn render_terminal_reference(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
    value_is_reference: bool,
) -> proc_macro2::TokenStream {
    let initial = if value_is_reference {
        value
    } else {
        quote!(&#value)
    };
    wrappers
        .iter()
        .fold(initial, |reference, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!((#reference).as_ref()),
            FieldWrapper::Optional => {
                unreachable!("non-optional accessors contain no optional wrapper")
            }
            FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {
                unreachable!("container access stops at the repetition wrapper")
            }
        })
}

/// Emits an optional borrowed view through wrappers that may be absent.
pub(super) fn render_optional_view_reference(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
    value_is_reference: bool,
) -> proc_macro2::TokenStream {
    let mut reference = if value_is_reference {
        value
    } else {
        quote!(&#value)
    };
    let mut optional = false;
    for wrapper in wrappers {
        match (optional, wrapper) {
            (false, FieldWrapper::Boxed) => reference = quote!((#reference).as_ref()),
            (false, FieldWrapper::Optional) => {
                reference = quote!((#reference).as_ref());
                optional = true;
            }
            (true, FieldWrapper::Boxed) => {
                reference = quote!((#reference).map(|value| value.as_ref()));
            }
            (true, FieldWrapper::Optional) => {
                reference = quote!((#reference).and_then(|value| value.as_ref()));
            }
            (_, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore) => {
                unreachable!("container access stops at the repetition wrapper")
            }
        }
    }
    debug_assert!(optional);
    reference
}

/// Emits field access for a named or positional construction slot.
pub(super) fn field_access(
    base: proc_macro2::TokenStream,
    ident: Option<&Ident>,
    index: usize,
) -> proc_macro2::TokenStream {
    ident.map_or_else(
        || {
            let index = syn::Index::from(index);
            quote!(#base.#index)
        },
        |ident| quote!(#base.#ident),
    )
}

/// Emits the binding pattern for a named or positional field.
pub(super) fn field_binding_pattern(
    node: &Ident,
    variant: &Ident,
    style: ResolvedFieldStyle,
    field: Option<&Ident>,
    index: usize,
    binding: &Ident,
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => {
            let field = field.expect("named variant field has an identifier");
            quote!(#node::#variant { #field: #binding, .. })
        }
        ResolvedFieldStyle::Unnamed => {
            let before = (0..index).map(|_| quote!(_));
            quote!(#node::#variant(#(#before,)* #binding, ..))
        }
        ResolvedFieldStyle::Unit => quote!(#node::#variant),
    }
}

/// Emits the destructuring pattern for one enum variant.
pub(super) fn variant_pattern(
    node: &Ident,
    variant: &Ident,
    style: ResolvedFieldStyle,
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => quote!(#node::#variant { .. }),
        ResolvedFieldStyle::Unnamed => quote!(#node::#variant(..)),
        ResolvedFieldStyle::Unit => quote!(#node::#variant),
    }
}

/// Emits one expected-set value in runtime representation.
pub(super) fn render_expected_value(
    expected: &ExpectedSet,
    plan: &EmissionPlan,
    facade: &Ident,
) -> proc_macro2::TokenStream {
    let words = &expected.words;
    let atom_count = expected.atoms.len();
    let mut atom_words = expected
        .atoms
        .last()
        .map_or_else(Vec::new, |atom| vec![0_u64; *atom as usize / 64 + 1]);
    for atom in &expected.atoms {
        atom_words[*atom as usize / 64] |= 1_u64 << (*atom as usize % 64);
    }
    let kind_count = plan.kind_count;
    quote! {
        #facade::__private::follow_set(
            #kind_count as u32,
            &[#(#words),*],
            &#facade::__private::expected_atoms::<#atom_count>(&[#(#atom_words),*]),
        )
    }
}

/// Emits one root-qualified place in the validated grammar-wide expectation pool.
pub(super) fn render_expected_set(
    plan: &EmissionPlan,
    expected: &ExpectedSet,
) -> proc_macro2::TokenStream {
    let module_anchor = &plan.module_anchor;
    let index = plan
        .expected_sets
        .id(expected)
        .expect("validated emitted expectation belongs to the grammar-wide pool");
    quote!(#module_anchor::__RECURSA_EXPECTED_SETS[#index])
}

/// Derives the collision-free identifier for generated metadata storage.
pub(super) fn metadata_static(node: usize, variant: Option<usize>) -> Ident {
    variant.map_or_else(
        || format_ident!("__RECURSA_ELEMENTS_{node}"),
        |variant| format_ident!("__RECURSA_ELEMENTS_{node}_{variant}"),
    )
}

/// Derives a collision-free seed for one generated local FOLLOW value.
pub(super) fn expected_static(node: usize, element: usize, role: &str) -> Ident {
    format_ident!("__RECURSA_{}_{}_{}", role.to_uppercase(), node, element)
}
/// Emits the parser entry point for one materialized node application.
pub(super) fn render_node_parse_application(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime: syn::Lifetime = syn::parse_quote!('__recursa_input);
    let ty = &application.ty;
    let mut recursive_generics = Generics::default();
    let mut helper_generics = Generics::default();
    recursive_generics
        .params
        .push(syn::parse_quote!(#input_lifetime));
    recursive_generics.params.push(syn::parse_quote!(A));
    if node.generics.lifetimes().next().is_some() {
        helper_generics
            .params
            .push(syn::parse_quote!(#input_lifetime));
    }
    let (recursive_impl_generics, _, recursive_where_clause) = recursive_generics.split_for_impl();
    let (helper_impl_generics, _, helper_where_clause) = helper_generics.split_for_impl();
    let node_capture = node.capture.render(facade);
    let mut application_node = node.clone();
    application_node.body = application.body.clone();
    let (planned_parse_body, parse_items, branch_helpers) = render_node_parse_body(
        plan,
        &application_node,
        application.parse_index,
        instrumentation,
    );
    let helper_method_generics = if node.generics.lifetimes().next().is_some() {
        quote!(<A>)
    } else {
        quote!(<#input_lifetime, A>)
    };
    let (parse_body, generated_helpers) = match &application.body {
        NodeBodyPlan::Pratt(_) => (
            quote!(Self::__recursa_parse_bp(context, 0, &[])),
            quote! {
                impl #helper_impl_generics #ty #helper_where_clause {
                    pub(crate) fn __recursa_parse_bp #helper_method_generics(
                        context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
                        min_bp: u8,
                        __recursa_pratt_excluded: &[usize],
                    ) -> Result<
                        #facade::__private::ParsedNode<Self>,
                        #facade::ParseError,
                    > {
                        #planned_parse_body
                    }

                    #branch_helpers
                }
            },
        ),
        NodeBodyPlan::Enum { .. } => (
            planned_parse_body,
            quote! {
                impl #helper_impl_generics #ty #helper_where_clause {
                    #branch_helpers
                }
            },
        ),
        NodeBodyPlan::Struct { .. } => (planned_parse_body, quote!()),
    };

    quote! {
        #parse_items
        #generated_helpers

        impl #recursive_impl_generics
            #facade::__private::Parse<#input_lifetime, A, #facade::__private::RecursiveDescent>
            for #ty #recursive_where_clause
        {
            const CAPTURE_POLICY: #facade::__private::CapturePolicy = #node_capture;
            fn grammar() -> &'static #facade::__private::GrammarInfo {
                &#module_anchor::__RECURSA_GRAMMAR
            }
            fn root_follow() -> &'static #facade::__private::FollowSet {
                &#module_anchor::__RECURSA_EMPTY_FOLLOW
            }
            fn parse(
                context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                #parse_body
            }
        }
    }
}
