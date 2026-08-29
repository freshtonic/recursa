//! Parse-plan construction, validation, dispatch, Pratt, container, and FOLLOW rendering.
//!
//! This module keeps source-aware construction beside executable parser emission. Frozen
//! semantic decisions are consumed here without recomputing analysis during rendering.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    macro_rules! focused_case {
        ($name:ident) => {
            #[test]
            fn $name() {
                test_support::$name();
            }
        };
    }
    focused_case!(predictive_compilation_rejects_missing_and_conflicting_frozen_products);
    focused_case!(corrupted_joined_dispatch_fails_before_publication_with_rca9105);
    focused_case!(corrupted_pratt_branch_follow_and_failure_identities_fail_before_publication);
    focused_case!(corrupted_real_plan_family_content_fails_before_publication);
}

#[derive(Clone, Debug)]
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
    /// Visibility of the generated node declaration.
    pub(super) visibility: Visibility,
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

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
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
        dispatch: PredictiveDispatchPlan,
    },
    /// Parses an expression family using frozen Pratt roles.
    Pratt(Box<PrattParsePlan>),
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
/// Prefix branch paired with its authored identity and binding position.
pub(super) struct PrattNudParsePlan {
    /// Authored variant index retained for diagnostics and identity.
    pub(super) source_index: usize,
    /// Frozen prefix variant body.
    pub(super) variant: VariantParsePlan,
    /// Binding-power position assigned by semantic analysis.
    pub(super) position: PrattPosition,
}

#[derive(Clone, Debug)]
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
}

#[derive(Clone, Debug)]
/// Caller-FOLLOW guard for one Pratt binding position.
pub(super) struct PrattGuardBranchPlan {
    /// Pratt position at which the guard applies.
    pub(super) position: PrattPosition,
    /// Caller-FOLLOW set admitted at that position.
    pub(super) expected: ExpectedSet,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
/// Predictive trie plus the semantic identity used to validate it.
pub(super) struct PredictiveDispatchPlan {
    /// Root of the frozen predictive trie.
    pub(super) root: DispatchTrieNodePlan,
    /// Sequence count used to detect stale semantic products.
    pub(super) frozen_sequence_count: usize,
    /// Trie-node count used to detect incomplete reconstruction.
    pub(super) frozen_node_count: usize,
    /// Deterministic identity of the analyzed dispatch family.
    pub(super) frozen_identity: u64,
}

#[derive(Clone, Debug, Default)]
/// One selectable state in the frozen predictive trie.
pub(super) struct DispatchTrieNodePlan {
    /// Selection made when the consumed prefix ends here.
    pub(super) target: Option<DispatchTargetPlan>,
    /// Outgoing positions in canonical predictive order.
    pub(super) edges: Vec<DispatchTrieEdgePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Variant selection reached at a predictive trie state.
pub(super) struct DispatchTargetPlan {
    /// Zero-based branch selected by the trie.
    pub(super) variant: usize,
    /// Whether selection depends on the caller-supplied FOLLOW set.
    pub(super) on_caller_follow: bool,
}

#[derive(Clone, Debug)]
/// Ordered transition from one predictive trie state to another.
pub(super) struct DispatchTrieEdgePlan {
    /// Token/text condition labeling this trie edge.
    pub(super) position: DispatchPositionPlan,
    /// Trie state reached after the position matches.
    pub(super) next: DispatchTrieNodePlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Token expectation and spelling refinement labeling a dispatch edge.
pub(super) struct DispatchPositionPlan {
    /// Token-kind expectation tested at this position.
    pub(super) expected: ExpectedSet,
    /// Optional spelling refinement for atom-like token kinds.
    pub(super) text_predicate: Option<PlannedTextPredicate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug)]
/// Schema provenance and parser recipe for one authored element.
pub(super) struct ElementParsePlan {
    /// Schema provenance exposed for Explorer occurrences.
    pub(super) explorer_source: ExplorerElementSource,
    /// Stable element name used by generated metadata.
    pub(super) metadata_name: String,
    /// Fixed or semantic parsing recipe.
    pub(super) kind: ElementParseKind,
}

#[derive(Clone, Debug)]
/// Fixed-token envelope around a semantic container.
pub(super) struct FixedAttachmentParsePlan {
    /// Fixed syntax consumed before the semantic child.
    pub(super) before: Vec<AttachedFixedSyntaxParsePlan>,
    /// Container operation that parses the semantic child.
    pub(super) semantic: Box<PlannedContainer>,
    /// Fixed syntax consumed after the semantic child.
    pub(super) after: Vec<AttachedFixedSyntaxParsePlan>,
}

#[derive(Clone, Debug)]
/// Classifies required and erased-optional syntax in a generated attachment.
pub(super) enum AttachedFixedSyntaxParsePlan {
    /// Consumes one required fixed token.
    Required(AttachedFixedParsePlan),
    /// Consumes a fixed sequence atomically when its full spelling is present.
    Optional(Vec<AttachedFixedParsePlan>),
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
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
        /// Canonical repetition separator spelling, if any.
        separator: Option<String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Pretty break behavior retained for a repeated parse field.
pub(super) enum PlannedRepetitionBreak {
    /// Allows the formatter to choose a repetition break.
    Soft,
    /// Breaks between occurrences of one materialized statement type.
    TopLevelStatement(MaterializedApplicationId),
    /// Breaks between occurrences from one statement node family.
    TopLevelStatementFamily(NodeId),
}

#[derive(Clone, Debug)]
/// Construction position and optional named binding for a parsed field.
pub(super) struct FieldPlan {
    /// Named-field binding, absent for tuple and unit positions.
    pub(super) ident: Option<Ident>,
    /// Zero-based construction position in authored field order.
    pub(super) index: usize,
    /// Generated accessor when the field is addressable by name.
    pub(super) accessor: Option<Ident>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug)]
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
        /// FIRST set that decides whether the child begins.
        selector: ExpectedSet,
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

#[derive(Clone, Debug, Eq, PartialEq)]
/// Static and caller-dependent FOLLOW policy used by emitted parsers.
pub(super) enum PlannedFollow {
    /// Uses only the caller-supplied FOLLOW set.
    Caller,
    /// Uses a frozen FOLLOW set independent of the caller.
    Static(ExpectedSet),
    /// Unions a frozen FOLLOW set with the caller context.
    StaticAndCaller(ExpectedSet),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Runtime-width token expectations with optional atom refinements.
pub(super) struct ExpectedSet {
    /// Dense token-kind bitset in runtime word order.
    pub(super) words: Vec<u64>,
    /// Sorted, deduplicated atom indices refining the kind set.
    pub(super) atoms: Vec<u32>,
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
        grammar: &crate::GrammarDescription,
        kind_count: usize,
    ) -> Result<Self, String> {
        let mut value = Self::empty(kind_count);
        for position in positions {
            for (left, right) in value.words.iter_mut().zip(position.kinds.words()) {
                *left |= *right;
            }
            for kind in position.kinds.iter() {
                if let Some(atom) = grammar.atoms.iter().position(|atom| {
                    matches!(atom.expression, AtomExpression::Kind(candidate) if candidate == kind)
                }) {
                    value.atoms.push(
                        atom.try_into()
                            .map_err(|_| "generated expectation atom exceeds runtime width")?,
                    );
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
            let recursive_bp = branch.variant.elements.iter().find_map(|element| {
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
                *pratt_min_bp
            });
            match (branch.position, recursive_bp) {
                (PrattPosition::Infix { rbp, .. }, Some(actual)) if rbp == actual => {}
                (PrattPosition::Postfix { .. }, None) => {}
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
            if let PrattPosition::Prefix { bp } = branch.position {
                let recursive_bp = branch.variant.elements.iter().find_map(|element| {
                    let ElementParseKind::Semantic {
                        projection: semantic,
                        ..
                    } = &element.kind
                    else {
                        return None;
                    };
                    let SemanticParseProjection::Direct { pratt_min_bp, .. } = semantic.as_ref()
                    else {
                        return None;
                    };
                    *pratt_min_bp
                });
                if recursive_bp != Some(bp) {
                    return Err(
                        "the frozen Pratt prefix binding-power family is corrupt".to_owned()
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    position: &FirstPosition,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<DispatchPositionPlan, String> {
    let text_predicate = position
        .text_predicate
        .map(|predicate| {
            grammar
                .text_predicates
                .get(predicate.index())
                .ok_or_else(|| "frozen dispatch references an unknown text predicate".to_owned())
                .map(|predicate| match predicate {
                    TextPredicate::Any => PlannedTextPredicate::Any,
                    TextPredicate::Exact(text) => PlannedTextPredicate::Exact(text.clone()),
                    TextPredicate::OneOf(options) => PlannedTextPredicate::OneOf(options.clone()),
                    TextPredicate::NotIn(options) => PlannedTextPredicate::NotIn(options.clone()),
                })
        })
        .transpose()?;
    Ok(DispatchPositionPlan {
        expected: ExpectedSet::from_positions(std::slice::from_ref(position), grammar, kind_count)?,
        text_predicate,
    })
}

/// Reconstructs a frozen predictive subtree while preserving edge order and identity.
pub(super) fn plan_predictive_dispatch_node(
    frozen: &crate::DispatchTrieNode,
    variants: &[(crate::DeclId, usize)],
    grammar: &crate::GrammarDescription,
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<DispatchTrieNodePlan, String> {
    instrumentation.frozen_dispatch_nodes_consumed += 1;
    let target = frozen
        .terminal()
        .map(|terminal| {
            variants
                .iter()
                .find(|(declaration, _)| *declaration == terminal.declaration())
                .map(|(_, variant)| DispatchTargetPlan {
                    variant: *variant,
                    on_caller_follow: terminal.on_caller_follow(),
                })
                .ok_or_else(|| "frozen dispatch terminal lacks a source variant".to_owned())
        })
        .transpose()?;
    let edges = frozen
        .edges()
        .iter()
        .map(|edge| {
            Ok(DispatchTrieEdgePlan {
                position: planned_dispatch_position(edge.position(), grammar, kind_count)?,
                next: plan_predictive_dispatch_node(
                    edge.next(),
                    variants,
                    grammar,
                    kind_count,
                    instrumentation,
                )?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(DispatchTrieNodePlan { target, edges })
}

/// Computes the deterministic identity used to validate a predictive trie.
pub(super) fn predictive_dispatch_identity(root: &DispatchTrieNodePlan) -> u64 {
    stable_hash(format!("{root:?}").as_bytes())
}

/// Rejects dispatch plans whose shape or identity differs from semantic analysis.
pub(super) fn validate_predictive_dispatch(
    dispatch: &PredictiveDispatchPlan,
    variants: usize,
) -> Result<usize, String> {
    fn validate_node(node: &DispatchTrieNodePlan, variants: usize) -> Result<usize, String> {
        if node.target.is_none() && node.edges.is_empty() {
            return Err("frozen predictive dispatch contains an empty node".to_owned());
        }
        if node.target.is_some_and(|target| target.variant >= variants) {
            return Err("frozen predictive dispatch references an unknown variant".to_owned());
        }
        if node
            .target
            .is_some_and(|target| !target.on_caller_follow && !node.edges.is_empty())
        {
            return Err("frozen predictive dispatch extends an unconditional terminal".to_owned());
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
            count += validate_node(&edge.next, variants)?;
        }
        Ok(count)
    }

    let count = validate_node(&dispatch.root, variants)?;
    if count != dispatch.frozen_node_count
        || predictive_dispatch_identity(&dispatch.root) != dispatch.frozen_identity
    {
        return Err("the joined predictive plan diverges from its frozen identity".to_owned());
    }
    Ok(count)
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
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<NodeParsePlan, String> {
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
                decisions,
                kind_count,
                &input_lifetime,
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
            let dispatch = decisions
                .dispatches()
                .iter()
                .find(|dispatch| {
                    dispatch.owner() == application_type && dispatch.kind() == DispatchKind::Enum
                })
                .ok_or_else(|| format!("enum Node `{}` lacks its dispatch plan", node.ident))?;
            let failure_expected = decisions
                .failure_edges()
                .iter()
                .find(|edge| {
                    edge.owner() == application_type
                        && edge.site().is_none()
                        && edge.kind() == FailureEdgeKind::EnumDispatch
                })
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
                ExpectedSet::from_positions(failure_expected, &lowered.grammar, kind_count)?;
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
                if branch.sequences().is_empty() {
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
                        decisions,
                        kind_count,
                        &input_lifetime,
                        None,
                    )?,
                    capture: resolved_variant.capture.map(PlannedCapturePolicy::from),
                    pretty: variant.pretty.clone(),
                });
            }
            let frozen_branch_count = dispatch.branches().len();
            let frozen_sequence_count = dispatch
                .branches()
                .iter()
                .map(|branch| branch.sequences().len())
                .sum();
            let nodes_before = instrumentation.frozen_dispatch_nodes_consumed;
            let root = plan_predictive_dispatch_node(
                dispatch.trie(),
                &variants_by_declaration,
                &lowered.grammar,
                kind_count,
                instrumentation,
            )?;
            let frozen_node_count = instrumentation.frozen_dispatch_nodes_consumed - nodes_before;
            let dispatch = PredictiveDispatchPlan {
                frozen_identity: predictive_dispatch_identity(&root),
                root,
                frozen_sequence_count,
                frozen_node_count,
            };
            let mut root_expected = ExpectedSet::empty(kind_count);
            for edge in &dispatch.root.edges {
                root_expected.union_assign(&edge.position.expected);
            }
            if root_expected != failure_expected {
                return Err(format!(
                    "enum Node `{}` dispatch and failure products disagree",
                    node.ident
                ));
            }
            let root_uses_caller = dispatch
                .root
                .target
                .is_some_and(|target| target.on_caller_follow);
            if root_uses_caller != failure_uses_caller {
                return Err(format!(
                    "enum Node `{}` dispatch and caller-FOLLOW failure products disagree",
                    node.ident
                ));
            }
            if variants_by_declaration.len() != frozen_branch_count
                || dispatch.frozen_sequence_count == 0
            {
                return Err(format!(
                    "enum Node `{}` lost a frozen dispatch sequence",
                    node.ident
                ));
            }
            NodeBodyPlan::Enum {
                variants: planned,
                dispatch,
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
            decisions,
            kind_count,
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
        visibility: node.visibility.clone(),
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
                dispatch: empty_declaration_dispatch(),
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

/// Creates the canonical dispatch plan for an enum with no selectable variants.
pub(super) fn empty_declaration_dispatch() -> PredictiveDispatchPlan {
    let root = DispatchTrieNodePlan::default();
    PredictiveDispatchPlan {
        frozen_identity: predictive_dispatch_identity(&root),
        root,
        frozen_sequence_count: 0,
        frozen_node_count: 0,
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
                        let separator_spelling = separator
                            .as_ref()
                            .map(|separator| {
                                let usage = &source.token_uses[separator.token_use.index()];
                                let definition =
                                    &source.token_definitions[usage.definition.index()];
                                match &definition.kind {
                                    TokenDefinitionKind::Fixed(fixed) => Ok(fixed_pretty_spelling(
                                        &fixed.spelling,
                                        lowered.grammar.text_equivalence,
                                    )),
                                    _ => Err("an authored separator references a non-fixed token"
                                        .to_owned()),
                                }
                            })
                            .transpose()?;
                        let repetition_break = authored
                            .terminal_node
                            .filter(|terminal_node| {
                                separator_spelling.as_deref() == Some(";")
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
                            separator: separator_spelling,
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
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    input_lifetime: &syn::Lifetime,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<PrattParsePlan, String> {
    let nud_dispatch = decisions
        .dispatches()
        .iter()
        .find(|dispatch| {
            dispatch.owner() == application_type && dispatch.kind() == DispatchKind::PrattNud
        })
        .ok_or_else(|| format!("Pratt Node `{}` lacks its nud dispatch plan", node.ident))?;
    let led_dispatch = decisions.dispatches().iter().find(|dispatch| {
        dispatch.owner() == application_type && dispatch.kind() == DispatchKind::PrattLed
    });

    let nud_failure = decisions
        .failure_edges()
        .iter()
        .find(|edge| {
            edge.owner() == application_type && edge.kind() == FailureEdgeKind::PrattNudDispatch
        })
        .ok_or_else(|| format!("Pratt Node `{}` lacks its nud failure edge", node.ident))?;
    let nud_failure =
        planned_expected_recipe(nud_failure.expected(), &lowered.grammar, kind_count)?;
    let guard = decisions.failure_edges().iter().find(|edge| {
        edge.owner() == application_type && edge.kind() == FailureEdgeKind::PrattCallerFollowGuard
    });
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
                nud_by_declaration.push((branch.declaration(), nud.len()));
                nud.push(PrattNudParsePlan {
                    source_index,
                    variant: VariantParsePlan {
                        resolved_variant: common.resolved_variant,
                        ident: emitted_ident(&common.name)?,
                        style: resolved.style,
                        elements: plan_elements(
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
                            decisions,
                            kind_count,
                            input_lifetime,
                            operand,
                        )?,
                        capture: resolved.capture.map(PlannedCapturePolicy::from),
                        pretty: common.pretty.clone(),
                    },
                    position,
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
                    PrattPosition::Postfix { .. } => None,
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
                led_by_declaration.push((branch.declaration(), led.len()));
                led.push(PrattLedParsePlan {
                    source_index,
                    variant: VariantParsePlan {
                        resolved_variant: common.resolved_variant,
                        ident: emitted_ident(&common.name)?,
                        style: resolved.style,
                        elements: plan_elements(
                            node,
                            Some(&synthetic_variant),
                            &materialized_led.tail,
                            &semantic.sites,
                            lowered,
                            decisions,
                            kind_count,
                            input_lifetime,
                            operand,
                        )?,
                        capture: resolved.capture.map(PlannedCapturePolicy::from),
                        pretty: common.pretty.clone(),
                    },
                    lhs: plan_pratt_lhs(node, resolved, materialized_led, lowered, input_lifetime)?,
                    position,
                });
            }
        }
    }

    let nud_dispatch = plan_joined_dispatch(
        nud_dispatch,
        &nud_by_declaration,
        lowered,
        kind_count,
        instrumentation,
    )?;
    let led_dispatch = led_dispatch
        .map(|dispatch| {
            plan_joined_dispatch(
                dispatch,
                &led_by_declaration,
                lowered,
                kind_count,
                instrumentation,
            )
        })
        .transpose()?;
    let mut nud_dispatch_expected = ExpectedSet::empty(kind_count);
    for edge in &nud_dispatch.root.edges {
        nud_dispatch_expected.union_assign(&edge.position.expected);
    }
    let (nud_failure_expected, nud_failure_uses_caller) = match &nud_failure {
        PlannedFollow::Caller => (ExpectedSet::empty(kind_count), true),
        PlannedFollow::Static(expected) => (expected.clone(), false),
        PlannedFollow::StaticAndCaller(expected) => (expected.clone(), true),
    };
    if nud_dispatch_expected != nud_failure_expected
        || nud_dispatch
            .root
            .target
            .is_some_and(|target| target.on_caller_follow)
            != nud_failure_uses_caller
    {
        return Err("the frozen Pratt nud dispatch and failure products disagree".to_owned());
    }
    let caller_follow_guard = match (guard_branches, &led_dispatch) {
        (Some(branches), Some(dispatch)) => {
            let frozen_led = decisions
                .dispatches()
                .iter()
                .find(|candidate| {
                    candidate.owner() == application_type
                        && candidate.kind() == DispatchKind::PrattLed
                })
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
                            .filter_map(|sequence| sequence.positions().first().cloned())
                            .collect::<Vec<_>>();
                        let expected =
                            ExpectedSet::from_positions(&first, &lowered.grammar, kind_count)?;
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
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<PredictiveDispatchPlan, String> {
    let nodes_before = instrumentation.frozen_dispatch_nodes_consumed;
    let root = plan_predictive_dispatch_node(
        dispatch.trie(),
        variants,
        &lowered.grammar,
        kind_count,
        instrumentation,
    )?;
    Ok(PredictiveDispatchPlan {
        frozen_identity: predictive_dispatch_identity(&root),
        root,
        frozen_sequence_count: dispatch
            .branches()
            .iter()
            .map(|branch| branch.sequences().len())
            .sum(),
        frozen_node_count: instrumentation.frozen_dispatch_nodes_consumed - nodes_before,
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
                separator: None,
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
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    input_lifetime: &syn::Lifetime,
    pratt_operand: Option<(crate::TypeId, u8)>,
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
    elements
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
                    decisions,
                    *site,
                    FailureEdgeKind::RequiredSite,
                    &lowered.grammar,
                    kind_count,
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
                let machine = decisions
                    .container_machines()
                    .iter()
                    .find(|machine| machine.site() == *site)
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
                            decisions,
                            *token_site,
                            FailureEdgeKind::RequiredSite,
                            &lowered.grammar,
                            kind_count,
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
                        decisions,
                        site,
                        FailureEdgeKind::RequiredSite,
                        &lowered.grammar,
                        kind_count,
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
                            decisions,
                            kind_count,
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
                            decisions,
                            kind_count,
                            capture,
                        )
                    })
                    .collect::<Result<_, _>>()?;
                let presence_site = if correlation.outer_is_optional {
                    correlation.outer
                } else {
                    correlation.envelope_sites[correlation.presence_index]
                };
                let machine = decisions
                    .container_machines()
                    .iter()
                    .find(|machine| machine.site() == presence_site)
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
                let machine = decisions
                    .container_machines()
                    .iter()
                    .find(|machine| machine.site() == *site)
                    .ok_or_else(|| format!("field `{field_name}` lacks its container machine"))?;
                validate_container_wrappers(machine.operation(), &wrappers, &field_name)?;
                let mut container = planned_container(
                    machine,
                    decisions,
                    &lowered.grammar,
                    kind_count,
                )?;
                let pratt_min_bp = pratt_operand
                    .filter(|(owner, _)| parse_site_targets(&lowered.grammar, *site, *owner))
                    .map(|(_, min_bp)| min_bp);
                if let Some((owner, min_bp)) = pratt_operand.filter(|_| pratt_min_bp.is_some()) {
                    let inactive = planned_pratt_extenders(
                        decisions,
                        owner,
                        &lowered.grammar,
                        kind_count,
                        |position| pratt_led_lbp(position) < min_bp,
                    )?;
                    match &mut container {
                        PlannedContainer::Direct { child_follow, .. } => {
                            *child_follow = follow_with_static(child_follow, &inactive);
                        }
                        PlannedContainer::Optional { .. } | PlannedContainer::Repeated { .. } => {
                            return Err(format!(
                                "Pratt operand field `{field_name}` is not a direct frozen container"
                            ));
                        }
                    }
                }
                let separator = separator
                    .as_ref()
                    .map(|separator| {
                        let source = lowered.source_modeled.source_model();
                        let token_use = &source.token_uses[separator.token_use.index()];
                        let definition = &source.token_definitions[token_use.definition.index()];
                        match &definition.kind {
                            TokenDefinitionKind::Fixed(fixed) => Ok(fixed_pretty_spelling(
                                &fixed.spelling,
                                lowered.grammar.text_equivalence,
                            )),
                            _ => Err("a repetition separator is not fixed syntax".to_owned()),
                        }
                    })
                    .transpose()?;
                let terminal_application = closed_terminal_application(ty);
                let terminal_node = terminal_application.and_then(|application| {
                    lowered
                        .applications
                        .iter()
                        .find(|candidate| candidate.id == application)
                        .map(|candidate| candidate.node)
                });
                let repetition_break = terminal_application
                    .filter(|application| {
                        separator.as_deref() == Some(";")
                            && matches!(container, PlannedContainer::Repeated { .. })
                            && lowered.applications.iter().any(|candidate| {
                                candidate.id == *application
                                    && matches!(candidate.body, MaterializedNodeBody::Enum(_))
                            })
                    })
                    .map(PlannedRepetitionBreak::TopLevelStatement)
                    .unwrap_or(PlannedRepetitionBreak::Soft);
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
                            separator,
                            attachment: attachment
                                .as_ref()
                                .map(|attachment| {
                                    plan_fixed_attachment(
                                        resolved_field,
                                        attachment,
                                        *site,
                                        lowered,
                                        decisions,
                                        kind_count,
                                        variant_capture,
                                    )
                                    .map(Box::new)
                                })
                                .transpose()?,
                            opaque_provenance: false,
                            repetition_break,
                            pretty: pretty.clone(),
                        }),
                    },
                })
            }
        })
        .collect()
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
    decisions: &crate::DecisionProducts,
    kind_count: usize,
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
        let PlannedFollow::Static(expected) = planned_failure_edge(
            decisions,
            site,
            FailureEdgeKind::RequiredSite,
            grammar,
            kind_count,
        )?
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
            let machine = decisions
                .container_machines()
                .iter()
                .find(|machine| machine.site() == outer_site)
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

#[allow(clippy::too_many_arguments)]
/// Plans fixed tokens attached around a semantic container.
pub(super) fn plan_fixed_attachment(
    field: &crate::resolution::ResolvedNodeField,
    attachment: &MaterializedFixedAttachment,
    outer_site: SiteId,
    lowered: &crate::materialization::LoweredGrammar,
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    variant_capture: PlannedCapturePolicy,
) -> Result<FixedAttachmentParsePlan, String> {
    let grammar = &lowered.grammar;
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
            plan_materialized_attached_syntax(
                syntax, *site, lowered, decisions, kind_count, capture,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let semantic_site = correlation.sites[correlation.semantic_index];
    let machine = decisions
        .container_machines()
        .iter()
        .find(|machine| machine.site() == semantic_site)
        .ok_or_else(|| {
            "an attachment semantic site lacks its direct container machine".to_owned()
        })?;
    if machine.operation() != ContainerOperation::Direct {
        return Err("an attachment semantic site is not a direct frozen container".to_owned());
    }
    let semantic = Box::new(planned_container(machine, decisions, grammar, kind_count)?);
    let after_start = correlation.semantic_index + 1;
    let after = attachment
        .after
        .iter()
        .zip(&correlation.sites[after_start..])
        .map(|(syntax, site)| {
            plan_materialized_attached_syntax(
                syntax, *site, lowered, decisions, kind_count, capture,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FixedAttachmentParsePlan {
        before,
        semantic,
        after,
    })
}

/// Collects materialized Pratt extenders admitted for an application.
pub(super) fn planned_pratt_extenders(
    decisions: &crate::DecisionProducts,
    owner: crate::TypeId,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
    include: impl Fn(PrattPosition) -> bool,
) -> Result<ExpectedSet, String> {
    let edge = decisions
        .failure_edges()
        .iter()
        .find(|edge| {
            edge.owner() == owner && edge.kind() == FailureEdgeKind::PrattCallerFollowGuard
        })
        .ok_or_else(|| "a Pratt operand lacks its frozen extender family".to_owned())?;
    let ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches } = edge.expected() else {
        return Err("a Pratt operand has an invalid frozen extender family".to_owned());
    };
    let first = branches
        .iter()
        .filter(|branch| branch.position().is_some_and(&include))
        .flat_map(|branch| branch.sequences())
        .filter_map(|sequence| sequence.positions().first().cloned())
        .collect::<Vec<_>>();
    ExpectedSet::from_positions(&first, grammar, kind_count)
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
    decisions: &crate::DecisionProducts,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedContainer, String> {
    let stop = planned_follow(machine.stop(), grammar, kind_count)?;
    let element_selector =
        ExpectedSet::from_positions(machine.element_first(), grammar, kind_count)?;
    Ok(match machine.operation() {
        ContainerOperation::Direct => PlannedContainer::Direct {
            child_follow: stop,
            required_failure: planned_failure_edge(
                decisions,
                machine.site(),
                FailureEdgeKind::RequiredSite,
                grammar,
                kind_count,
            )?,
        },
        ContainerOperation::Optional => PlannedContainer::Optional {
            selector: element_selector,
            child_follow: stop,
        },
        operation @ (ContainerOperation::ZeroOrMore
        | ContainerOperation::OneOrMore
        | ContainerOperation::OptionalOneOrMore) => {
            let separator_selector = if machine.separator().is_some() {
                Some(ExpectedSet::from_positions(
                    machine.separator_first(),
                    grammar,
                    kind_count,
                )?)
            } else {
                None
            };
            let continuation = separator_selector
                .as_ref()
                .unwrap_or(&element_selector)
                .clone();
            let child_follow = follow_with_static(&stop, &continuation);
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
                    planned_failure_edge(
                        decisions,
                        machine.site(),
                        FailureEdgeKind::ContainerMinimum,
                        grammar,
                        kind_count,
                    )
                })
                .transpose()?
                .map(Box::new),
                continuation_failure: Box::new(planned_failure_edge(
                    decisions,
                    machine.site(),
                    FailureEdgeKind::ContainerContinuation,
                    grammar,
                    kind_count,
                )?),
                after_separator_failure: machine
                    .separator()
                    .map(|_| {
                        planned_failure_edge(
                            decisions,
                            machine.site(),
                            FailureEdgeKind::ContainerAfterSeparator,
                            grammar,
                            kind_count,
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
    decisions: &crate::DecisionProducts,
    site: SiteId,
    kind: FailureEdgeKind,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    let edge = decisions
        .failure_edges()
        .iter()
        .find(|edge| edge.site() == Some(site) && edge.kind() == kind)
        .ok_or_else(|| {
            format!(
                "site {} lacks its frozen {kind:?} failure edge",
                site.index()
            )
        })?;
    planned_expected_recipe(edge.expected(), grammar, kind_count)
}

/// Converts an expected-set recipe to the runtime-width representation.
pub(super) fn planned_expected_recipe(
    expected: &ExpectedRecipe,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    match expected {
        ExpectedRecipe::Static(first) => Ok(PlannedFollow::Static(ExpectedSet::from_positions(
            first, grammar, kind_count,
        )?)),
        ExpectedRecipe::Follow(follow) => planned_follow(follow, grammar, kind_count),
        ExpectedRecipe::ContinueOrStop { continuation, stop } => {
            let stop = planned_follow(stop, grammar, kind_count)?;
            let continuation = ExpectedSet::from_positions(continuation, grammar, kind_count)?;
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
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    Ok(match recipe {
        FollowRecipe::Caller => PlannedFollow::Caller,
        FollowRecipe::Static { first } => {
            PlannedFollow::Static(ExpectedSet::from_positions(first, grammar, kind_count)?)
        }
        FollowRecipe::StaticAndCaller { first } => {
            PlannedFollow::StaticAndCaller(ExpectedSet::from_positions(first, grammar, kind_count)?)
        }
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
    }
}

/// Emits the parser body selected by the frozen node plan.
pub(super) fn render_node_parse_body(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            render_sequence_parse(plan, node_index, None, *style, elements, None, None)
        }
        NodeBodyPlan::Enum { variants, dispatch } => {
            let mut expected_index = 1_000_000;
            render_predictive_dispatch(
                plan,
                node_index,
                variants,
                &dispatch.root,
                0,
                &mut expected_index,
                instrumentation,
            )
        }
        NodeBodyPlan::Pratt(pratt) => render_pratt_parse(plan, node_index, pratt, instrumentation),
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
    node: &DispatchTrieNodePlan,
    depth: usize,
    expected_index: &mut usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    instrumentation.frozen_dispatch_nodes_rendered += 1;
    if let Some(target) = node.target.filter(|target| !target.on_caller_follow) {
        debug_assert!(node.edges.is_empty());
        let variant = &variants[target.variant];
        return render_sequence_parse(
            plan,
            node_index,
            Some((target.variant, &variant.ident)),
            variant.style,
            &variant.elements,
            variant.capture,
            None,
        );
    }
    let facade = &plan.facade;
    let mut expected = ExpectedSet::empty(plan.kind_count);
    for edge in &node.edges {
        expected.union_assign(&edge.position.expected);
    }
    let expected_name = expected_static(node_index, *expected_index, "dispatch");
    *expected_index += 1;
    let expected_definition = render_expected_static(&expected_name, &expected, plan, facade);

    let target = node.target.map(|target| {
        let variant = &variants[target.variant];
        let body = render_sequence_parse(
            plan,
            node_index,
            Some((target.variant, &variant.ident)),
            variant.style,
            &variant.elements,
            variant.capture,
            None,
        );
        debug_assert!(target.on_caller_follow);
        quote! {
            if context.peek(#depth).is_none()
                || context
                    .peek(#depth)
                    .is_some_and(|record| context.follow().contains(record.kind))
            {
                return { #body };
            }
        }
    });

    let edges = node.edges.iter().map(|edge| {
        let position_name = expected_static(node_index, *expected_index, "position");
        *expected_index += 1;
        let position_definition =
            render_expected_static(&position_name, &edge.position.expected, plan, facade);
        let matches = render_dispatch_position_match(
            plan,
            &position_name,
            &edge.position.text_predicate,
            depth,
        );
        let next = render_predictive_dispatch(
            plan,
            node_index,
            variants,
            &edge.next,
            depth + 1,
            expected_index,
            instrumentation,
        );
        quote! {
            #position_definition
            if #matches {
                return { #next };
            }
        }
    });

    let failure = if node.target.is_some_and(|target| target.on_caller_follow) {
        quote! {
            let __recursa_dispatch_expected = #expected_name
                .union(context.follow())
                .expect("matching generated dispatch/FOLLOW widths");
            Err(context.unexpected_at(#depth, &__recursa_dispatch_expected))
        }
    } else {
        quote!(Err(context.unexpected_at(#depth, &#expected_name)))
    };

    quote! {
        #expected_definition
        #target
        #(#edges)*
        #failure
    }
}

/// Emits the token-kind and optional spelling test for one trie edge.
pub(super) fn render_dispatch_position_match(
    plan: &EmissionPlan,
    expected: &Ident,
    predicate: &Option<PlannedTextPredicate>,
    depth: usize,
) -> proc_macro2::TokenStream {
    let predicate = predicate.as_ref().map(|predicate| {
        let matches = match predicate {
            PlannedTextPredicate::Any => quote!(true),
            PlannedTextPredicate::Exact(exact) => match plan.text_equivalence {
                TextEquivalence::Sensitive => quote!(text == #exact),
                TextEquivalence::AsciiInsensitive => quote!(text.eq_ignore_ascii_case(#exact)),
            },
            PlannedTextPredicate::OneOf(options) => match plan.text_equivalence {
                TextEquivalence::Sensitive => quote!([#(#options),*].contains(&text)),
                TextEquivalence::AsciiInsensitive => {
                    quote!([#(#options),*].iter().any(|option| text.eq_ignore_ascii_case(option)))
                }
            },
            PlannedTextPredicate::NotIn(options) => match plan.text_equivalence {
                TextEquivalence::Sensitive => quote!(![#(#options),*].contains(&text)),
                TextEquivalence::AsciiInsensitive => {
                    quote!(![#(#options),*].iter().any(|option| text.eq_ignore_ascii_case(option)))
                }
            },
        };
        quote! {
            && context
                .source()
                .get(record.span.range())
                .is_some_and(|text| #matches)
        }
    });
    quote! {
        context.peek(#depth).is_some_and(|record| {
            #expected.contains(record.kind) #predicate
        })
    }
}

/// Emits construction of the branch selected by predictive dispatch.
pub(super) fn render_predictive_selection(
    plan: &EmissionPlan,
    node_index: usize,
    node: &DispatchTrieNodePlan,
    depth: usize,
    caller_follow: &proc_macro2::TokenStream,
    expected_index: &mut usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    instrumentation.frozen_dispatch_nodes_rendered += 1;
    let fallback = node.target.map_or_else(
        || quote!(None),
        |target| {
            let variant = target.variant;
            if target.on_caller_follow {
                quote! {
                    if context.peek(#depth).is_none()
                        || context
                            .peek(#depth)
                            .is_some_and(|record| #caller_follow.contains(record.kind))
                    {
                        Some(#variant)
                    } else {
                        None
                    }
                }
            } else {
                debug_assert!(node.edges.is_empty());
                quote!(Some(#variant))
            }
        },
    );
    node.edges.iter().rev().fold(fallback, |otherwise, edge| {
        let position_name = expected_static(node_index, *expected_index, "pratt_position");
        *expected_index += 1;
        let position_definition =
            render_expected_static(&position_name, &edge.position.expected, plan, &plan.facade);
        let matches = render_dispatch_position_match(
            plan,
            &position_name,
            &edge.position.text_predicate,
            depth,
        );
        let selected = render_predictive_selection(
            plan,
            node_index,
            &edge.next,
            depth + 1,
            caller_follow,
            expected_index,
            instrumentation,
        );
        quote! {{
            #position_definition
            if #matches { #selected } else { #otherwise }
        }}
    })
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
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let owner = quote!(Self);
    let expression_start = format_ident!("__recursa_expression_start");
    let mut expected_index = 2_000_000;
    let nud_follow_name = expected_static(node_index, 3_200_000, "pratt_nud_follow");
    let nud_follow_definition =
        render_expected_static(&nud_follow_name, &pratt.nud_selection_follow, plan, facade);
    let nud_follow_value = format_ident!("__recursa_pratt_nud_follow");
    let nud_selection = render_predictive_selection(
        plan,
        node_index,
        &pratt.nud_dispatch.root,
        0,
        &quote!(#nud_follow_value),
        &mut expected_index,
        instrumentation,
    );
    let nud_failure_name = expected_static(node_index, 3_100_000, "pratt_nud_failure");
    let (nud_failure_definition, nud_failure_value) =
        render_follow_value(plan, &nud_failure_name, &pratt.nud_failure);
    let nud_branches = pratt.nud.iter().enumerate().map(|(index, branch)| {
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
        quote!(Some(#index) => { #body? })
    });

    if pratt.led.is_empty() {
        return quote! {
            #nud_failure_definition
            #nud_follow_definition
            let _ = min_bp;
            let #nud_follow_value = #nud_follow_name
                .union(context.follow())
                .expect("matching generated Pratt FOLLOW widths");
            let #expression_start = context.cursor();
            let parsed = match { #nud_selection } {
                #(#nud_branches,)*
                None => return Err(context.unexpected(&#nud_failure_value)),
                Some(_) => unreachable!("frozen Pratt dispatch selected an unknown branch"),
            };
            let _ = #expression_start;
            Ok(parsed)
        };
    }

    let led_selection = render_predictive_selection(
        plan,
        node_index,
        &pratt
            .led_dispatch
            .as_ref()
            .expect("validated led plan has a frozen dispatch")
            .root,
        0,
        &quote!(context.follow()),
        &mut expected_index,
        instrumentation,
    );
    let guard = pratt
        .caller_follow_guard
        .as_ref()
        .expect("validated led plan has a caller-FOLLOW guard");
    let guard_definitions = guard.iter().enumerate().map(|(index, branch)| {
        let name = expected_static(node_index, 3_000_000 + index, "pratt_guard");
        render_expected_static(&name, &branch.expected, plan, &plan.facade)
    });
    let guard_additions = guard.iter().enumerate().map(|(index, branch)| {
        let name = expected_static(node_index, 3_000_000 + index, "pratt_guard");
        let bp = pratt_led_lbp(branch.position);
        quote! {
            if #bp >= min_bp {
                __recursa_pratt_guard = Some(match __recursa_pratt_guard {
                    Some(expected) => expected
                        .union(&#name)
                        .expect("matching generated Pratt expectation widths"),
                    None => #name.clone(),
                });
            }
        }
    });
    let led_offset = pratt.nud.len();
    let mut led_branches = Vec::new();
    for (index, branch) in pratt.led.iter().enumerate() {
        let metadata_index = led_offset + index;
        let lbp = pratt_led_lbp(branch.position);
        let body = render_pratt_led_parse(
            plan,
            node_index,
            metadata_index,
            &owner,
            branch,
            &expression_start,
        );
        led_branches.push(quote! {
            Some(#index) => {
                if #lbp < min_bp { break; }
                if context.next_is(context.follow()) {
                    return Err(context.pratt_ambiguity(
                        __recursa_pratt_guard
                            .as_ref()
                            .expect("an active extender contributes an expectation"),
                    ));
                }
                lhs = { #body };
                continue;
            }
        });
    }

    quote! {
        #nud_failure_definition
        #nud_follow_definition
        #(#guard_definitions)*
        let #nud_follow_value = #nud_follow_name
            .union(context.follow())
            .expect("matching generated Pratt FOLLOW widths");
        let #expression_start = context.cursor();
        let mut lhs = match { #nud_selection } {
            #(#nud_branches,)*
            None => return Err(context.unexpected(&#nud_failure_value)),
            Some(_) => unreachable!("frozen Pratt dispatch selected an unknown branch"),
        };
        let mut __recursa_pratt_guard: Option<#facade::__private::FollowSet> = None;
        #(#guard_additions)*
        loop {
            match { #led_selection } {
                #(#led_branches)*
                None => break,
                Some(_) => unreachable!("frozen Pratt dispatch selected an unknown branch"),
            }
        }
        Ok(lhs)
    }
}

/// Returns the left binding power encoded by an extender position.
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
            let expected_name = expected_static(node_index, unique_index, "fixed");
            let expected_definition =
                render_expected_static(&expected_name, expected, plan, facade);
            let capture = capture.render(facade);
            (
                quote! {
                    #expected_definition
                    let #occurrence = context.parse_fixed(&#expected_name, #capture)?;
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
                let expected = expected_static(
                    node_index,
                    unique_index,
                    &format!("presence_{role}_{index}"),
                );
                let definition = render_expected_static(&expected, &fixed.expected, plan, facade);
                let occurrence = format_ident!("__recursa_presence_{role}_{index}");
                let capture = fixed.capture.render(facade);
                (
                    expected.clone(),
                    definition,
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
                        let expected = expected_static(
                            node_index,
                            unique_index,
                            &format!("presence_{role}_{index}"),
                        );
                        let definition =
                            render_expected_static(&expected, &fixed.expected, plan, facade);
                        let capture = fixed.capture.render(facade);
                        quote! {
                            #definition
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
    let sequence_definitions = sequence.iter().map(|(_, definition, _, _)| definition);
    let sequence_matches = sequence.iter().enumerate().map(|(index, (expected, _, _, _))| {
        quote!(context.peek(#index).is_some_and(|record| #expected.contains(record.kind)))
    });
    let sequence_statements = sequence.iter().map(|(_, _, statement, _)| statement);
    let sequence_occurrences = sequence.iter().map(|(_, _, _, occurrence)| occurrence);
    let after_statements = after.iter().map(|(statement, _)| statement);
    let after_occurrences = after.iter().map(|(_, occurrence)| occurrence);
    quote! {
        let __recursa_presence_start = context.cursor();
        #(#before_statements)*
        #(#sequence_definitions)*
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
    node_index: usize,
    unique_index: usize,
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
            let expected = expected_static(node_index, unique_index, &format!("{role}_{index}"));
            let definition = render_expected_static(&expected, &fixed.expected, plan, facade);
            let child = format_ident!("__recursa_{role}_{index}");
            let capture = fixed.capture.render(facade);
            (
                expected.clone(),
                definition,
                quote!(let #child = context.parse_fixed(&#expected, #capture)?;),
                child,
            )
        })
        .collect::<Vec<_>>();
    let definitions = tokens.iter().map(|(_, definition, _, _)| definition);
    let matches = tokens.iter().enumerate().map(|(index, (expected, _, _, _))| {
        quote!(context.peek(#index).is_some_and(|record| #expected.contains(record.kind)))
    });
    let statements = tokens.iter().map(|(_, _, statement, _)| statement);
    let children = tokens.iter().map(|(_, _, _, child)| child);
    quote! {
        #(#definitions)*
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
            let direct_child = match (pratt_owner, pratt_min_bp) {
                (Some(owner), Some(min_bp)) => quote! {
                    context.parse_generated(&#follow_value, #child_capture, |context| {
                        #owner::__recursa_parse_bp(context, #min_bp)
                    })
                },
                _ => quote! {
                    context.parse_node::<#terminal>(&#follow_value, #child_capture)
                },
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
                    )
                },
            );
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
            child_follow,
        } => {
            let selector_name = expected_static(node_index, unique_index, "optional");
            let selector_definition =
                render_expected_static(&selector_name, selector, plan, facade);
            let follow_name = expected_static(node_index, unique_index, "child_follow");
            let (follow_definition, follow_value) =
                render_follow_value(plan, &follow_name, child_follow);
            let wrapped = render_wrapped_value(quote!(parsed_value), wrappers);
            let absent = render_absent_value(wrappers);
            let direct_child = quote!(context.parse_node::<#terminal>(
                &#follow_value,
                #child_capture,
            ));
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
                    )
                },
            );
            quote! {
                #selector_definition
                #follow_definition
                let (#value, #occurrence) = if context.next_is(&#selector_name) {
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
    let semantic_parse = match (pratt_owner, pratt_min_bp) {
        (Some(owner), Some(min_bp)) => quote! {
            context.parse_generated(&#semantic_follow_value, #child_capture, |context| {
                #owner::__recursa_parse_bp(context, #min_bp)
            })
        },
        _ => quote! {
            context.parse_node::<#terminal>(&#semantic_follow_value, #child_capture)
        },
    };
    let render_syntax = |syntax: &AttachedFixedSyntaxParsePlan, role: &str, index: usize| {
        let occurrence = format_ident!("__recursa_attachment_{role}_{index}");
        match syntax {
            AttachedFixedSyntaxParsePlan::Required(fixed) => {
                let name = expected_static(
                    node_index,
                    unique_index,
                    &format!("attachment_{role}_{index}"),
                );
                let definition = render_expected_static(&name, &fixed.expected, plan, facade);
                let capture = fixed.capture.render(facade);
                (
                    quote! {
                        #definition
                        let #occurrence = context.parse_fixed(&#name, #capture)?;
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

    let element_name = expected_static(node_index, unique_index, "element");
    let element_definition = render_expected_static(&element_name, element_selector, plan, facade);
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
    let direct_child = quote!(context.parse_node::<#terminal>(
        &#child_follow_value,
        #child_capture,
    ));
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
            if !context.next_is(&#element_name) {
                return Err(context.unexpected(&#minimum_value));
            }
            #parse_item
        }
    } else if minimum_failure.is_some() {
        quote! {
            if context.next_is(&#element_name) {
                #parse_item
            } else if !context.is_eof() && !context.next_is(&#stop_value) {
                return Err(context.unexpected(&#minimum_value));
            }
        }
    } else {
        quote! {
            if context.next_is(&#element_name) {
                #parse_item
            } else if !context.is_eof() && !context.next_is(&#stop_value) {
                return Err(context.unexpected(&#continuation_value));
            }
        }
    };

    let continuation = if let Some(separator_selector) = separator_selector {
        let separator_name = expected_static(node_index, unique_index, "separator");
        let separator_definition =
            render_expected_static(&separator_name, separator_selector, plan, facade);
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
            #separator_definition
            loop {
                if !context.next_is(&#separator_name) {
                    if context.is_eof() || context.next_is(&#stop_value) {
                        break;
                    }
                    return Err(context.unexpected(&#continuation_value));
                }
                let separator_occurrence =
                    context.parse_repetition_separator(&#separator_name)?;
                separator_occurrences.push(separator_occurrence);
                if context.next_is(&#element_name) {
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
                if context.next_is(&#element_name) {
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
        #element_definition
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
    match follow {
        PlannedFollow::Caller => (
            quote!(let #value = context.follow().clone();),
            quote!(#value),
        ),
        PlannedFollow::Static(expected) => {
            let definition = render_expected_static(name, expected, plan, facade);
            (
                quote!(#definition let #value = #name.clone();),
                quote!(#value),
            )
        }
        PlannedFollow::StaticAndCaller(expected) => {
            let definition = render_expected_static(name, expected, plan, facade);
            (
                quote! {
                    #definition
                    let #value = #name
                        .union(context.follow())
                        .expect("matching generated FOLLOW widths");
                },
                quote!(#value),
            )
        }
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

/// Emits a stable static expected set in runtime representation.
pub(super) fn render_expected_static(
    name: &Ident,
    expected: &ExpectedSet,
    plan: &EmissionPlan,
    facade: &Ident,
) -> proc_macro2::TokenStream {
    let words = &expected.words;
    let atoms = expected
        .atoms
        .iter()
        .map(|atom| quote!(#facade::__private::ExpectedAtomId(#atom)));
    let kind_count = plan.kind_count;
    quote! {
        static #name: #facade::__private::FollowSet = #facade::__private::follow_set(
            #kind_count as u32,
            &[#(#words),*],
            &[#(#atoms),*],
        );
    }
}

/// Derives the collision-free identifier for generated metadata storage.
pub(super) fn metadata_static(node: usize, variant: Option<usize>) -> Ident {
    variant.map_or_else(
        || format_ident!("__RECURSA_ELEMENTS_{node}"),
        |variant| format_ident!("__RECURSA_ELEMENTS_{node}_{variant}"),
    )
}

/// Derives the collision-free identifier for a generated expected set.
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
    let mut parse_generics = Generics::default();
    let mut helper_generics = Generics::default();
    parse_generics
        .params
        .push(syn::parse_quote!(#input_lifetime));
    helper_generics
        .params
        .push(syn::parse_quote!(#input_lifetime));
    parse_generics.params.push(syn::parse_quote!(A));
    let (parse_impl_generics, _, parse_where_clause) = parse_generics.split_for_impl();
    let (helper_impl_generics, _, helper_where_clause) = helper_generics.split_for_impl();
    let node_capture = node.capture.render(facade);
    let mut application_node = node.clone();
    application_node.body = application.body.clone();
    let planned_parse_body = render_node_parse_body(
        plan,
        &application_node,
        application.parse_index,
        instrumentation,
    );
    let (parse_body, pratt_helper) = if matches!(&application.body, NodeBodyPlan::Pratt(_)) {
        (
            quote!(Self::__recursa_parse_bp(context, 0)),
            quote! {
                impl #helper_impl_generics #ty #helper_where_clause {
                    fn __recursa_parse_bp<A>(
                        context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
                        min_bp: u8,
                    ) -> Result<
                        #facade::__private::ParsedNode<Self>,
                        #facade::ParseError,
                    > {
                        #planned_parse_body
                    }
                }
            },
        )
    } else {
        (planned_parse_body, quote!())
    };

    quote! {
        #pratt_helper

        impl #parse_impl_generics #facade::__private::Parse<#input_lifetime, A>
            for #ty #parse_where_clause
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

        impl #parse_impl_generics
            #facade::__private::ParseSealed<#module_anchor::Input<#input_lifetime, A>>
            for #ty #parse_where_clause
        {}

        impl #parse_impl_generics #facade::Parse<#module_anchor::Input<#input_lifetime, A>>
            for #ty #parse_where_clause
        {
            type Output = #facade::Parsed<#input_lifetime, Self>;
            fn parse(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<Self::Output, #facade::ParseError> {
                #facade::__private::parse(&mut input.inner)
            }
        }
    }
}
