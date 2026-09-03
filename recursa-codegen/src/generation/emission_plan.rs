//! Construction and cross-capability validation of the source-aware emission plan.
//!
//! This coordinator joins source analysis to capability-owned plans in a fixed order, freezes
//! every Debug-derived family identity, and validates those identities before rendering.

use super::*;

#[cfg(test)]
std::thread_local! {
    /// Per-test observation of complete emission-plan validation passes.
    static EMISSION_PLAN_VALIDATION_COUNT: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

#[cfg(test)]
/// Resets the current test thread's complete emission-plan validation count.
pub(super) fn reset_emission_plan_validation_count() {
    EMISSION_PLAN_VALIDATION_COUNT.set(0);
}

#[cfg(test)]
/// Returns the current test thread's complete emission-plan validation count.
pub(super) fn emission_plan_validation_count() -> usize {
    EMISSION_PLAN_VALIDATION_COUNT.get()
}

#[derive(Clone)]
/// Immutable source-aware plan consumed by generated-code rendering.
///
/// Field order is part of the Debug-derived generation identity. Construction joins authored
/// structure and correlations to already-frozen semantic decisions; validation must succeed
/// before rendering, debug staging, or publication begins.
pub(super) struct EmissionPlan {
    /// Generation identity over canonical lowered content and emitter protocol.
    pub(super) identity: String,
    /// Authored facade mapping used by all generated Rust paths.
    pub(super) facade: Ident,
    /// Selected-root module path anchoring generated loaders and exports.
    pub(super) module_anchor: syn::Path,
    /// Display name retained from the grammar configuration.
    pub(super) grammar_name: String,
    /// Grammar-wide fixed-token comparison policy.
    pub(super) text_equivalence: TextEquivalence,
    /// Default maximum rendered width for generated `Pretty` implementations.
    pub(super) pretty_max_width: usize,
    /// Default indentation width for generated `Pretty` implementations.
    pub(super) pretty_indent: usize,
    /// Total grammar-local token-kind cardinality, including lexical error.
    pub(super) kind_count: usize,
    /// Private numeric kind reserved for rejected lexer ranges.
    pub(super) lexical_error_kind: u16,
    /// Generated typed kind variants in numeric kind order.
    pub(super) kind_variants: Vec<Ident>,
    /// Canonical fixed spellings positionally aligned with kind variants.
    pub(super) kind_static_texts: Vec<Option<String>>,
    /// Diagnostic display names positionally aligned with kind variants.
    pub(super) kind_names: Vec<String>,
    /// Expected-atom descriptions in semantic analysis order.
    pub(super) atom_names: Vec<String>,
    /// Canonical grammar-wide storage for every emitted static expectation.
    pub(super) expected_sets: ExpectedSetPool,
    /// Frozen Node parse plans in canonical materialization order.
    pub(super) nodes: Vec<NodeParsePlan>,
    /// Frozen content-token plans in semantic token order.
    pub(super) tokens: Vec<TokenParsePlan>,
    /// Source-visible generated token bindings and their visibility.
    pub(super) token_bindings: Vec<TokenBindingPlan>,
    /// Backend-independent lexer rules in precedence order.
    pub(super) lexical_rules: Vec<LexRulePlan>,
    /// Generated typed trivia classifications in numeric kind order.
    pub(super) trivia_kinds: Vec<Ident>,
    /// Keyword-carrier reclassification plans in declaration order.
    pub(super) word_classifiers: Vec<WordClassifierPlan>,
    /// Kind set eligible for word-shaped fixed-token classification.
    pub(super) word_candidate_kinds: Vec<u16>,
    /// Grammar-default optional trait recipes for generated token types.
    pub(super) root_configured_traits: Vec<ConfiguredTraitRecipe>,
    /// Release and protocol handshake embedded into structural loaders.
    pub(super) private_abi: PrivateAbiPlan,
    /// Accepted facade capability combinations for this Generation.
    pub(super) capabilities: Vec<FacadeCapability>,
    /// Frozen grammar-structured rendering plan.
    pub(super) pretty: PrettyEmissionPlan,
    #[cfg(feature = "diagrams")]
    /// Optional semantic railroad topology when diagram generation is enabled.
    pub(super) diagrams: Option<DiagramPlan>,
    /// Optional grammar-erased schema and callback plan.
    pub(super) explorer: Option<ExtensionEmissionPlan>,
    /// Optional generated exact-source document framing plan.
    pub(super) framing: Option<FramingEmissionPlan>,
    /// Validation counters proving frozen dispatch consumption.
    pub(super) instrumentation: EmissionInstrumentation,
    /// Independent identities used to detect capability-plan drift.
    pub(super) plan_families: EmissionPlanFamilies,
}

/// Opaque proof that an immutable emission plan passed its complete invariant check.
pub(super) struct ValidatedEmissionPlan(EmissionPlan);

impl std::ops::Deref for ValidatedEmissionPlan {
    type Target = EmissionPlan;

    /// Borrows the immutable plan certified by this validation proof.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug)]
/// Resolved compile-time document framing identities consumed by generated code.
pub(super) struct FramingEmissionPlan {
    /// Index of the Node family used for typed islands.
    pub(super) node_index: usize,
    /// Index of the exact materialized application selected by typed lowering.
    pub(super) application_index: usize,
    /// Grammar-local fixed-token kind separating adjacent islands.
    pub(super) boundary_kind: u16,
    /// Closed physical-line token kinds owned atomically outside island parsing.
    pub(super) line_kinds: Vec<u16>,
    /// Closed delimited token kinds owned atomically outside island parsing.
    pub(super) delimited_kinds: Vec<u16>,
    /// Optional complete variant identity that enters payload mode.
    pub(super) payload: Option<FramingPayloadPlan>,
}

#[derive(Clone, Debug)]
/// Resolved payload transition in one framing declaration.
pub(super) struct FramingPayloadPlan {
    /// Source-order variant index under the framing island Node.
    pub(super) variant_index: usize,
    /// Exact nonempty standalone physical-line terminator.
    pub(super) terminator: String,
}

#[derive(Clone, Debug)]
/// Frozen identities for capability-owned subplans independently checked before rendering.
pub(super) struct EmissionPlanFamilies {
    /// Identity reconstructed from authored recipes and intrinsic content-token value traits.
    pub(super) configured_traits: u64,
    /// Authoritative matching-version loader handshake.
    pub(super) private_abi: u64,
    /// Authoritative facade capability combinations.
    pub(super) capabilities: u64,
    /// Authoritative Pretty family identity.
    pub(super) pretty: Option<u64>,
    /// Authoritative parse-plan identity.
    pub(super) parse: u64,
    /// Authoritative diagram family identity when enabled.
    pub(super) diagrams: Option<u64>,
    /// Authoritative Explorer family identity when enabled.
    pub(super) explorer: Option<u64>,
}

#[derive(Clone, Default)]
/// Counters proving plan construction consumed the complete frozen semantic decision product.
pub(super) struct EmissionInstrumentation {
    /// Number of Node dispatch tries joined into the frozen plan.
    pub(super) frozen_dispatch_nodes_consumed: usize,
}

#[derive(Default)]
/// Counters proving rendering consumed exactly the plan structure validated earlier.
pub(super) struct RenderInstrumentation {
    /// Number of frozen Node dispatch tries consumed during rendering.
    pub(super) frozen_dispatch_nodes_rendered: usize,
}

/// Resolves one authored framing declaration to generated Node, token, and variant identities.
///
/// # Long-function contract
///
/// Rationale: framing is a cross-product of typed lowering, lexical ownership, payload dispatch,
/// and generated parser identity; freezing those joins together prevents rendering from repeating
/// source resolution. Invariants: the island uses exactly one materialized application correlation,
/// payload uses its exact resolved variant, and boundary/atomic token ownership is disjoint and
/// matcher-validated. Phases: consume typed application correlations, validate fixed and atomic
/// lexical kinds, correlate the optional payload variant with that application, then publish the
/// immutable framing plan.
fn plan_framing(
    syntax: &crate::source_syntax::FramingSyntax,
    lowered: &crate::materialization::LoweredGrammar,
    nodes: &[NodeParsePlan],
) -> Result<FramingEmissionPlan, String> {
    let correlation = lowered
        .lowering_map
        .framing
        .ok_or_else(|| "framing declaration lacks its typed lowering correlation".to_owned())?;
    let (node_index, application_index) = nodes
        .iter()
        .enumerate()
        .find_map(|(node_index, node)| {
            node.parse_applications
                .iter()
                .position(|application| application.application == correlation.application)
                .map(|application_index| (node_index, application_index))
        })
        .ok_or_else(|| "framing island lacks its exact generated application".to_owned())?;
    if nodes[node_index].node != correlation.node {
        return Err("framing application diverges from its resolved Node identity".to_owned());
    }

    let definitions = &lowered.source_modeled.source_model().token_definitions;
    let resolve_token =
        |ident: &syn::Ident| -> Result<&crate::source_model::TokenDefinition, String> {
            let name = ident.unraw().to_string();
            let mut matches = definitions
                .iter()
                .filter(|definition| definition.name == name);
            let definition = matches
                .next()
                .ok_or_else(|| format!("framing token `{name}` is not defined"))?;
            if matches.next().is_some() {
                return Err(format!("framing token `{name}` is ambiguous"));
            }
            Ok(definition)
        };
    let boundary = resolve_token(&syntax.boundary)?;
    if !matches!(boundary.kind, TokenDefinitionKind::Fixed(_)) {
        return Err("framing boundary must name one fixed token".to_owned());
    }
    let boundary_kind = explorer_kind_index(lowered, boundary.id)?;

    let resolve_atomic = |ident: &syn::Ident, physical_line: bool| -> Result<u16, String> {
        let definition = resolve_token(ident)?;
        let matcher = match &definition.kind {
            TokenDefinitionKind::Content(content) => content.closed_matcher.as_ref(),
            TokenDefinitionKind::LexicalBase(base) => base.closed_matcher.as_ref(),
            TokenDefinitionKind::Fixed(_) => None,
        };
        let accepted = if physical_line {
            matches!(matcher, Some(ClosedMatcher::PhysicalLine { .. }))
        } else {
            matches!(
                matcher,
                Some(ClosedMatcher::SameDelimiter { .. } | ClosedMatcher::Nested { .. })
            )
        };
        if !accepted {
            let class = if physical_line {
                "physical-line"
            } else {
                "same- or nested-delimiter"
            };
            return Err(format!(
                "framing token `{}` must use a closed {class} matcher",
                ident.unraw()
            ));
        }
        explorer_kind_index(lowered, definition.id)
    };
    let line_kinds = syntax
        .line
        .iter()
        .map(|ident| resolve_atomic(ident, true))
        .collect::<Result<Vec<_>, _>>()?;
    let delimited_kinds = syntax
        .delimited
        .iter()
        .map(|ident| resolve_atomic(ident, false))
        .collect::<Result<Vec<_>, _>>()?;
    let mut atomic = line_kinds
        .iter()
        .chain(&delimited_kinds)
        .copied()
        .collect::<Vec<_>>();
    atomic.sort_unstable();
    if atomic.windows(2).any(|pair| pair[0] == pair[1]) || atomic.contains(&boundary_kind) {
        return Err(
            "framing boundary, line, and delimited token ownership must be disjoint".to_owned(),
        );
    }

    let payload = syntax
        .payload
        .as_ref()
        .map(|payload| -> Result<FramingPayloadPlan, String> {
            let resolved_variant = correlation.payload_variant.ok_or_else(|| {
                "payload declaration lacks its resolved variant correlation".to_owned()
            })?;
            let variant_index = match &nodes[node_index].parse_applications[application_index].body
            {
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter()
                    .position(|variant| variant.resolved_variant == resolved_variant),
                NodeBodyPlan::Pratt(pratt) => pratt
                    .nud
                    .iter()
                    .map(|branch| &branch.variant)
                    .chain(pratt.led.iter().map(|branch| &branch.variant))
                    .position(|variant| variant.resolved_variant == resolved_variant),
                NodeBodyPlan::Struct { .. } => None,
            }
            .ok_or_else(|| {
                "payload `after` variant lacks its generated framing identity".to_owned()
            })?;
            Ok(FramingPayloadPlan {
                variant_index,
                terminator: payload.terminator.value(),
            })
        })
        .transpose()?;

    Ok(FramingEmissionPlan {
        node_index,
        application_index,
        boundary_kind,
        line_kinds,
        delimited_kinds,
        payload,
    })
}

/// Reports whether every module segment and the declaration itself are public.
pub(super) fn publicly_reachable(
    tree: &crate::discovery::DiscoveredModuleTree,
    module: ModuleId,
    visibility: &Visibility,
) -> bool {
    if !is_public(visibility) {
        return false;
    }
    let mut current = module;
    loop {
        let Some(inclusion) = tree.inclusions.iter().find(|entry| entry.id == current) else {
            return false;
        };
        let Some(parent) = inclusion.parent else {
            return true;
        };
        if !is_public(&inclusion.visibility) {
            return false;
        }
        current = parent;
    }
}

impl EmissionPlan {
    /// Joins frozen semantic products into a validated emission plan.
    pub(super) fn join(
        analyzed: &crate::source_analysis::AnalyzedGrammar,
        cfg: &CargoCfgSnapshot,
    ) -> Result<ValidatedEmissionPlan, String> {
        Self::build(analyzed, cfg).map(ValidatedEmissionPlan)
    }

    #[cfg(test)]
    /// Joins a mutable raw plan for corruption and checked-rendering tests.
    pub(super) fn join_for_test(
        analyzed: &crate::source_analysis::AnalyzedGrammar,
        cfg: &CargoCfgSnapshot,
    ) -> Result<Self, String> {
        Self::build(analyzed, cfg)
    }

    /// Builds and validates the immutable plan wrapped by the production join seam.
    ///
    /// # Long-function contract
    ///
    /// Rationale: keeps the analyzed grammar and all capability plans in one immutable generation snapshot.
    /// Invariants: source order, Debug field order, and family-identity inputs remain canonical.
    /// Phases: joins source facts, constructs capability plans, computes identities, then validates.
    fn build(
        analyzed: &crate::source_analysis::AnalyzedGrammar,
        cfg: &CargoCfgSnapshot,
    ) -> Result<Self, String> {
        let lowered = &analyzed.lowered;
        let resolved = lowered.source_modeled.resolved();
        let output = analyzed.analysis.output.as_ref().ok_or_else(|| {
            "successful source analysis output lacks semantic analysis decisions".to_owned()
        })?;
        let decisions = output.decisions();
        let module_anchor = emitted_crate_path(&resolved.build_inputs.module_anchor.segments)?;
        let facade = emitted_ident(&resolved.build_inputs.facade.ident)?;
        let root_configured_traits = resolved
            .configuration
            .syntax
            .derives
            .iter()
            .filter_map(|path| {
                crate::source_model::configured_trait_recipe(
                    path,
                    &resolved.build_inputs.facade.ident,
                )
            })
            .collect::<Vec<_>>();
        let kind_count = lowered.grammar.kind_count + 1;
        let lexical_error_kind = u16::try_from(lowered.grammar.kind_count)
            .map_err(|_| "generated lexical-error kind exceeds runtime width".to_owned())?;
        let kind_variants = planned_kind_variants(lowered)?;
        let kind_static_texts = lowered
            .lexical_plan
            .token_kinds
            .iter()
            .map(|kind| match &kind.identity {
                TokenKindIdentity::Fixed(text) => Some(text.clone()),
                TokenKindIdentity::ContentBase(_) => None,
            })
            .collect::<Vec<_>>();
        let mut kind_names = lowered
            .grammar
            .kinds
            .iter()
            .map(|kind| kind.name.clone())
            .collect::<Vec<_>>();
        kind_names.push("lexical error".to_owned());
        let atom_names = lowered
            .grammar
            .atoms
            .iter()
            .map(|atom| atom.name.clone())
            .collect::<Vec<_>>();

        validate_view_collisions(resolved, cfg)?;

        let mut tokens = Vec::new();
        for (index, content) in resolved.content_tokens.iter().enumerate() {
            let target = lowered
                .lexical_plan
                .parser_targets
                .iter()
                .find(|target| target.semantic == content.id)
                .ok_or_else(|| {
                    format!("content token `{}` lacks a parser target", content.ident)
                })?;
            let content_type = lowered
                .lowering_map
                .content_targets
                .iter()
                .find(|correlation| correlation.semantic == content.id)
                .ok_or_else(|| {
                    format!("content token `{}` lacks a normalized type", content.ident)
                })?
                .lowered;
            let TypeShape::Token { atom, .. } = &lowered.grammar.types[content_type.index()].shape
            else {
                return Err(format!(
                    "content token `{}` did not lower to a normalized token type",
                    content.ident
                ));
            };
            let kind = u16::try_from(target.base_kind.index())
                .map_err(|_| "generated token kind exceeds runtime width".to_owned())?;
            let accepted_kinds = target.accepted_kinds();
            let mut expected = ExpectedSet::from_kinds(&accepted_kinds, kind_count);
            expected.atoms.push(atom.index().try_into().map_err(|_| {
                "generated content-token expectation exceeds runtime width".to_owned()
            })?);
            tokens.push(TokenParsePlan {
                semantic: content.id,
                ident: format_ident!("__RecursaContentToken{index}"),
                kind,
                kind_variant: kind_variants[target.base_kind.index()].clone(),
                accepted_kind_variants: accepted_kinds
                    .iter()
                    .map(|kind| kind_variants[kind.index()].clone())
                    .collect(),
                expected,
                capture: resolved.build_inputs.spans.into(),
                arbitrary_spelling: lexical_pattern_witness(
                    &lowered
                        .lexical_plan
                        .content_bases
                        .iter()
                        .find(|base| base.kind == target.base_kind)
                        .ok_or_else(|| {
                            format!("content token `{}` lacks a lexical base", content.ident)
                        })?
                        .pattern,
                )?,
                configured_traits: lowered
                    .source_modeled
                    .source_model()
                    .token_definitions
                    .iter()
                    .find_map(|definition| match &definition.kind {
                        TokenDefinitionKind::Content(token) if token.semantic == content.id => {
                            Some(
                                token
                                    .configured_traits
                                    .effective
                                    .iter()
                                    .map(|selection| selection.recipe)
                                    .collect(),
                            )
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        format!(
                            "content token `{}` lacks a configured trait plan",
                            content.ident
                        )
                    })?,
            });
        }
        let token_bindings = resolved
            .token_bindings
            .iter()
            .map(|binding| {
                Ok(TokenBindingPlan {
                    module: binding.module,
                    semantic_key: resolved
                        .tree
                        .inclusions
                        .iter()
                        .find(|inclusion| inclusion.id == binding.module)
                        .map(|inclusion| {
                            inclusion
                                .logical_path
                                .iter()
                                .chain(std::iter::once(&binding.ident))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("::")
                        })
                        .ok_or_else(|| {
                            format!("token binding `{}` has no selected module", binding.ident)
                        })?,
                    ident: emitted_ident(&binding.ident)?,
                    visibility: binding.visibility.clone(),
                    publicly_materialized: publicly_reachable(
                        &resolved.tree,
                        binding.module,
                        &binding.visibility,
                    ),
                    token: binding.target.index(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut nodes = Vec::new();
        let mut instrumentation = EmissionInstrumentation::default();
        let expected_atom_index = ExpectedAtomIndex::from_grammar(&lowered.grammar)?;
        let emission_lookups = EmissionLookupIndex::new(
            decisions,
            &expected_atom_index,
            lowered.grammar.types.len(),
            lowered.grammar.sites.len(),
            kind_count,
        );
        for node in &resolved.nodes {
            let applications = lowered
                .applications
                .iter()
                .filter(|application| application.node == node.id)
                .collect::<Vec<_>>();
            if applications.is_empty() {
                return Err(format!(
                    "Node `{}` lacks a materialized application before compound emission",
                    node.ident
                ));
            }
            let source_node = lowered
                .source_modeled
                .source_model()
                .nodes
                .iter()
                .find(|source_node| source_node.resolved_node == node.id)
                .ok_or_else(|| format!("Node `{}` lacks a configured trait plan", node.ident))?;
            let configured_traits = source_node
                .configured_traits
                .effective
                .iter()
                .map(|selection| selection.recipe)
                .collect();
            let explorer_visit_configured = source_node
                .configured_traits
                .effective
                .iter()
                .any(|selection| selection.recipe == ConfiguredTraitRecipe::Visit)
                || source_node
                    .configured_traits
                    .omissions
                    .iter()
                    .any(|omission| omission.recipe == ConfiguredTraitRecipe::Visit);
            let authored_lifetimes = node
                .generics
                .lifetimes()
                .map(|parameter| parameter.lifetime.clone())
                .collect::<Vec<_>>();
            let mut application_plans = Vec::new();
            let mut source_plan = None;
            for application in applications {
                let planned = plan_node(
                    node,
                    application,
                    lowered,
                    &emission_lookups,
                    &mut instrumentation,
                )?;
                application_plans.push(NodeApplicationParsePlan {
                    application: application.id,
                    node: application.node,
                    parse_index: 0,
                    ty: materialized_application_type(
                        application,
                        &authored_lifetimes,
                        lowered,
                        &syn::parse_quote!('__recursa_input),
                    )?,
                    static_ty: materialized_application_type(
                        application,
                        &authored_lifetimes,
                        lowered,
                        &syn::parse_quote!('static),
                    )?,
                    body: planned.body.clone(),
                });
                if source_plan.is_none() {
                    source_plan = Some(planned);
                }
            }
            let mut planned = source_plan
                .ok_or_else(|| format!("Node `{}` lacks an application plan", node.ident))?;
            planned.body = structural_body(node, source_node, lowered, kind_count)?;
            planned.parse_applications.append(&mut application_plans);
            planned.configured_traits = configured_traits;
            planned.explorer_visit_configured = explorer_visit_configured;
            nodes.push(planned);
        }
        let mut next_parse_index = 0;
        for node in &mut nodes {
            for application in &mut node.parse_applications {
                application.parse_index = next_parse_index;
                next_parse_index += 1;
            }
        }
        let expected_sets = ExpectedSetPool::from_plans(&nodes, &tokens, kind_count);
        let frozen_dispatch_sequences = nodes
            .iter()
            .flat_map(|node| &node.parse_applications)
            .filter_map(|application| match &application.body {
                NodeBodyPlan::Enum { dispatch, .. } => Some(dispatch.frozen_sequence_count),
                NodeBodyPlan::Struct { .. } => None,
                NodeBodyPlan::Pratt(pratt) => Some(
                    pratt.nud_dispatch.frozen_sequence_count
                        + pratt
                            .led_dispatch
                            .as_ref()
                            .map_or(0, |dispatch| dispatch.frozen_sequence_count),
                ),
            })
            .sum::<usize>();
        let frozen_parse_identity = parse_plan_identity(&nodes);
        // source analysis has already decided whether each fixed token remains a standalone
        // rule or is recognized through one or more proven carriers. Preserve
        // that decision here instead of reconstructing fixed rules from source.
        let mut lexical_rules: Vec<LexRulePlan> = Vec::new();
        for rule in &lowered.lexical_plan.rules {
            let pattern = match &rule.matcher {
                LexicalMatcher::Regex(pattern) => pattern.clone(),
                LexicalMatcher::Fixed(text) => {
                    fixed_lexical_pattern(text, resolved.build_inputs.text_equivalence)
                }
            };
            let planned = LexRulePlan {
                pattern,
                closed_matcher: rule.closed_matcher.clone(),
                kind: rule
                    .kind
                    .index()
                    .try_into()
                    .map_err(|_| "generated token kind exceeds runtime width".to_owned())?,
                skip: false,
                trivia_kind: None,
                fixed: matches!(rule.priority, LexicalPriority::Fixed),
                priority: match rule.priority {
                    LexicalPriority::Fixed => 0,
                    LexicalPriority::Pattern { authored } => authored.unwrap_or(0),
                },
            };
            if !lexical_rules.iter().any(|existing| {
                existing.pattern == planned.pattern
                    && existing.closed_matcher == planned.closed_matcher
                    && existing.kind == planned.kind
            }) {
                lexical_rules.push(planned);
            }
        }
        let trivia_names = lowered
            .lexical_plan
            .non_token_rules
            .iter()
            .filter_map(|rule| match &rule.outcome {
                NonTokenLexicalOutcome::NonEmitting => None,
                NonTokenLexicalOutcome::RetainedTrivia { name } => Some(name.clone()),
            })
            .collect::<BTreeSet<_>>();
        if trivia_names.len() > u16::MAX as usize {
            return Err("generated trivia-kind plan exceeds runtime width".to_owned());
        }
        let trivia_kind_indexes = trivia_names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                u16::try_from(index)
                    .map(|index| (name.clone(), index))
                    .map_err(|_| "generated trivia kind exceeds runtime width".to_owned())
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let trivia_kinds = trivia_names
            .iter()
            .map(|name| emitted_ident(name))
            .collect::<Result<Vec<_>, _>>()?;
        for rule in &lowered.lexical_plan.non_token_rules {
            let pattern = match &rule.matcher {
                LexicalMatcher::Regex(pattern) => pattern.clone(),
                LexicalMatcher::Fixed(text) => {
                    fixed_lexical_pattern(text, resolved.build_inputs.text_equivalence)
                }
            };
            let trivia_kind = match &rule.outcome {
                NonTokenLexicalOutcome::NonEmitting => None,
                NonTokenLexicalOutcome::RetainedTrivia { name } => {
                    Some(*trivia_kind_indexes.get(name).ok_or_else(|| {
                        "retained trivia rule lacks a generated classification".to_owned()
                    })?)
                }
            };
            lexical_rules.push(LexRulePlan {
                pattern,
                closed_matcher: rule.closed_matcher.clone(),
                kind: 0,
                skip: true,
                trivia_kind,
                fixed: false,
                priority: match rule.priority {
                    LexicalPriority::Fixed => 0,
                    LexicalPriority::Pattern { authored } => authored.unwrap_or(0),
                },
            });
        }
        let keyword_classifiers =
            lowered
                .lexical_plan
                .keywords
                .iter()
                .filter_map(|keyword| {
                    let KeywordRecognition::CombinedCarriers {
                        carrier,
                        additional_carriers,
                    } = &keyword.recognition
                    else {
                        return None;
                    };
                    let carriers = std::iter::once(*carrier)
                        .chain(additional_carriers.iter().copied())
                        .map(|kind| {
                            kind.index().try_into().map_err(|_| {
                                "generated token kind exceeds runtime width".to_owned()
                            })
                        })
                        .collect::<Result<Vec<_>, _>>();
                    Some(carriers.and_then(|carriers| {
                        Ok(KeywordClassifierPlan {
                            canonical_spelling: keyword.canonical_spelling.clone(),
                            kind: keyword.kind.index().try_into().map_err(|_| {
                                "generated token kind exceeds runtime width".to_owned()
                            })?,
                            carriers,
                            ascii_insensitive: resolved.build_inputs.text_equivalence
                                == TextEquivalence::AsciiInsensitive,
                        })
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
        let word_classifiers = lowered
            .lexical_plan
            .keywords
            .iter()
            .map(|keyword| {
                Ok(WordClassifierPlan {
                    canonical_spelling: keyword.canonical_spelling.clone(),
                    kind: keyword
                        .kind
                        .index()
                        .try_into()
                        .map_err(|_| "generated token kind exceeds runtime width".to_owned())?,
                    ascii_insensitive: resolved.build_inputs.text_equivalence
                        == TextEquivalence::AsciiInsensitive,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let word_candidate_kinds = lowered
            .lexical_plan
            .content_bases
            .iter()
            .map(|base| {
                base.kind
                    .index()
                    .try_into()
                    .map_err(|_| "generated token kind exceeds runtime width".to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?;
        let framing = resolved
            .configuration
            .syntax
            .framing
            .as_ref()
            .map(|syntax| plan_framing(syntax, lowered, &nodes))
            .transpose()?;
        #[cfg(feature = "diagrams")]
        let diagrams = resolved
            .build_inputs
            .diagrams
            .then(|| {
                plan_diagrams(
                    lowered,
                    resolved,
                    &nodes,
                    &tokens,
                    &token_bindings,
                    &kind_names,
                    &kind_static_texts,
                )
            })
            .transpose()?;
        #[cfg(feature = "diagrams")]
        let diagram_enabled = diagrams.is_some();
        #[cfg(not(feature = "diagrams"))]
        let diagram_enabled = false;
        #[cfg(feature = "diagrams")]
        let frozen_diagrams = diagrams.as_ref().map(plan_family_identity);
        #[cfg(not(feature = "diagrams"))]
        let frozen_diagrams = None;
        let private_abi = emitted_private_abi_plan();
        #[cfg(feature = "explorer")]
        let explorer = emitted_explorer_plan(
            &nodes,
            lowered,
            &kind_variants,
            framing.as_ref().map(|framing| {
                nodes[framing.node_index].parse_applications[framing.application_index].application
            }),
        )?
        .map(|explorer| ExtensionEmissionPlan {
            artifacts: Vec::new(),
            explorer,
        });
        #[cfg(not(feature = "explorer"))]
        let explorer: Option<ExtensionEmissionPlan> = None;
        if let Some(explorer) = explorer.as_ref() {
            validate_explorer_collisions(resolved, cfg, &nodes, &explorer.explorer)?;
        }
        if framing.is_some() && explorer.is_none() {
            return Err(
                "document framing requires the Recursa `explorer` generation feature".to_owned(),
            );
        }
        let pretty = emitted_pretty_plan(&nodes, &tokens);
        let capabilities =
            emitted_capability_plan(&nodes, &tokens, diagram_enabled, explorer.is_some());
        #[cfg(feature = "explorer")]
        let authoritative_explorer =
            authoritative_explorer_plan(lowered)?.map(|explorer| ExtensionEmissionPlan {
                artifacts: Vec::new(),
                explorer,
            });
        #[cfg(not(feature = "explorer"))]
        let authoritative_explorer: Option<ExtensionEmissionPlan> = None;
        let plan_families = EmissionPlanFamilies {
            configured_traits: authoritative_configured_trait_identity(lowered, resolved),
            private_abi: plan_family_identity(&authoritative_private_abi_plan()),
            capabilities: plan_family_identity(&authoritative_capability_plan(
                lowered,
                resolved.build_inputs.diagrams,
                authoritative_explorer.is_some(),
            )),
            pretty: authoritative_pretty_plan(lowered, resolved)?
                .as_ref()
                .map(plan_family_identity),
            parse: frozen_parse_identity,
            diagrams: frozen_diagrams,
            explorer: authoritative_explorer.as_ref().map(plan_family_identity),
        };

        let mut canonical = format!(
            "{};facade={facade};anchor={};kinds={kind_count};edition={:?};cfg={:?};",
            generation_protocol_identity(),
            resolved.build_inputs.module_anchor.segments.join("::"),
            resolved.build_inputs.discovery.edition,
            resolved.build_inputs.discovery.cfg_snapshot,
        );
        // Raw source text stays out of the identity: hashing it relocates the whole
        // immutable Generation on semantically neutral edits (comments, inline test
        // modules), which defeats downstream incremental compilation of the include
        // tree. The span-stripped family identities and explicit plan facts below
        // cover every semantic input rendering consumes; publication still verifies
        // byte-for-byte payload equality whenever an identity is reused.
        canonical.push_str(&format!(
            ";families=configured_traits:{:016x},private_abi:{:016x},capabilities:{:016x},pretty:{:?},diagrams:{:?},explorer:{:?}",
            plan_families.configured_traits,
            plan_families.private_abi,
            plan_families.capabilities,
            plan_families.pretty,
            plan_families.diagrams,
            plan_families.explorer,
        ));
        canonical.push_str(&format!(
            ";grammar={:?}:{:?};pretty_defaults={}x{};lexical_error_kind={lexical_error_kind}",
            lowered.grammar.name,
            lowered.grammar.text_equivalence,
            resolved.build_inputs.pretty.max_width,
            resolved.build_inputs.pretty.indent,
        ));
        canonical.push_str(&format!(
            ";kind_tables={:016x};atom_names={:016x}",
            plan_family_identity(&(&kind_variants, &kind_static_texts, &kind_names)),
            plan_family_identity(&atom_names),
        ));
        for node in &nodes {
            canonical.push_str(&format!(
                ";node={}:{}:{}:{}",
                node.semantic_key,
                node.loader_anchor,
                node.ident,
                node.generics.to_token_stream()
            ));
        }
        for token in &tokens {
            let accepted_kind_variants = token
                .accepted_kind_variants
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            canonical.push_str(&format!(
                ";token={}:{}:{}:{:?}:{}:{}",
                token.ident,
                token.kind,
                token.capture.canonical_name(),
                accepted_kind_variants,
                token.kind_variant,
                token.arbitrary_spelling,
            ));
        }
        for binding in &token_bindings {
            canonical.push_str(&format!(
                ";binding={}:{}:{}:{}",
                binding.module.index(),
                binding.ident,
                binding.visibility.to_token_stream(),
                binding.token
            ));
        }
        for rule in &lexical_rules {
            canonical.push_str(&format!(
                ";rule={:?}:{:?}:{}:{}:{:?}:{}:{}",
                rule.pattern,
                rule.closed_matcher,
                rule.kind,
                rule.skip,
                rule.trivia_kind,
                rule.fixed,
                rule.priority,
            ));
        }
        for classifier in &keyword_classifiers {
            canonical.push_str(&format!(
                ";classify={:?}:{}:{:?}:{}",
                classifier.canonical_spelling,
                classifier.kind,
                classifier.carriers,
                classifier.ascii_insensitive,
            ));
        }
        for classifier in &word_classifiers {
            canonical.push_str(&format!(
                ";classify_word={:?}:{}:{}",
                classifier.canonical_spelling, classifier.kind, classifier.ascii_insensitive,
            ));
        }
        canonical.push_str(&format!(";word_candidates={word_candidate_kinds:?}"));
        canonical.push_str(&format!(
            ";trivia_kinds={:?}",
            trivia_kinds
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ));
        canonical.push_str(&format!(";framing={framing:?}"));
        #[cfg(feature = "diagrams")]
        canonical.push_str(if diagrams.is_some() {
            ";capabilities=core,diagrams"
        } else {
            ";capabilities=core"
        });
        #[cfg(not(feature = "diagrams"))]
        canonical.push_str(";capabilities=core");
        canonical.push_str(&format!(
            ";frozen_dispatch_sequences={frozen_dispatch_sequences};frozen_dispatch_nodes={};emitter_dispatch_decisions=0;frozen_parse_identity={frozen_parse_identity:016x}",
            instrumentation.frozen_dispatch_nodes_consumed,
        ));
        let identity = format!("g_{:016x}", stable_hash(canonical.as_bytes()));
        let plan = Self {
            identity,
            facade,
            module_anchor,
            grammar_name: lowered.grammar.name.clone(),
            text_equivalence: lowered.grammar.text_equivalence,
            pretty_max_width: resolved.build_inputs.pretty.max_width,
            pretty_indent: resolved.build_inputs.pretty.indent,
            kind_count,
            lexical_error_kind,
            kind_variants,
            kind_static_texts,
            kind_names,
            atom_names,
            expected_sets,
            nodes,
            tokens,
            token_bindings,
            lexical_rules,
            trivia_kinds,
            word_classifiers,
            word_candidate_kinds,
            root_configured_traits,
            private_abi,
            capabilities,
            pretty,
            #[cfg(feature = "diagrams")]
            diagrams,
            explorer,
            framing,
            instrumentation,
            plan_families,
        };
        plan.validate()?;
        Ok(plan)
    }

    /// Validates cross-capability identities before any rendering begins.
    ///
    /// # Long-function contract
    ///
    /// Rationale: checks the complete cross-capability contract before any bytes or files become observable.
    /// Invariants: semantic decisions are consumed rather than recomputed and every family identity matches its frozen plan.
    /// Phases: validates ABI and capabilities, plan families, names, then feature-selected payloads.
    pub(super) fn validate(&self) -> Result<(), String> {
        #[cfg(test)]
        EMISSION_PLAN_VALIDATION_COUNT.set(EMISSION_PLAN_VALIDATION_COUNT.get() + 1);
        if self.identity.is_empty() || self.nodes.is_empty() {
            return Err("the Emission plan lacks a Generation identity or Nodes".to_owned());
        }
        if self.kind_variants.len() + 1 != self.kind_count
            || self.kind_static_texts.len() + 1 != self.kind_count
            || self.kind_names.len() != self.kind_count
        {
            return Err("the generated token-kind plan has inconsistent width".to_owned());
        }
        self.expected_sets.validate(
            &self.nodes,
            &self.tokens,
            self.kind_count,
            self.atom_names.len(),
        )?;
        if self.lexical_rules.is_empty() {
            return Err("the generated lexical plan has no rules".to_owned());
        }
        if let Some(framing) = &self.framing {
            if framing.node_index >= self.nodes.len()
                || self.nodes[framing.node_index]
                    .parse_applications
                    .get(framing.application_index)
                    .is_none()
                || framing
                    .line_kinds
                    .iter()
                    .chain(&framing.delimited_kinds)
                    .any(|kind| usize::from(*kind) >= self.kind_count)
                || usize::from(framing.boundary_kind) >= self.kind_count
            {
                return Err(
                    "the document framing plan contains an invalid generated identity".to_owned(),
                );
            }
            if framing.payload.as_ref().is_some_and(|payload| {
                match &self.nodes[framing.node_index].parse_applications[framing.application_index]
                    .body
                {
                    NodeBodyPlan::Enum { variants, .. } => payload.variant_index >= variants.len(),
                    NodeBodyPlan::Pratt(pratt) => {
                        payload.variant_index >= pratt.nud.len() + pratt.led.len()
                    }
                    NodeBodyPlan::Struct { .. } => true,
                }
            }) {
                return Err("the document framing payload variant identity is invalid".to_owned());
            }
        }
        if self.lexical_rules.iter().any(|rule| {
            rule.closed_matcher
                .as_ref()
                .is_some_and(|matcher| matcher.starter_pattern() != rule.pattern)
        }) {
            return Err(
                "the generated closed-matcher plan diverges from its validated starter".to_owned(),
            );
        }
        #[cfg(feature = "diagrams")]
        if self.diagrams.as_ref().is_some_and(|diagrams| {
            diagrams.nodes.len() != self.nodes.len() || diagrams.tokens.len() != self.tokens.len()
        }) {
            return Err(
                "the diagram plan does not cover every frozen Node and content token".to_owned(),
            );
        }
        self.validate_plan_families()?;
        let mut frozen_nodes = 0;
        let mut applications = BTreeSet::new();
        for node in &self.nodes {
            for application in &node.parse_applications {
                if !applications.insert(application.application) {
                    return Err(
                        "the Pretty plan contains a duplicate materialized application".to_owned(),
                    );
                }
                match &application.body {
                    NodeBodyPlan::Enum { variants, dispatch } => {
                        if dispatch.frozen_sequence_count == 0 && dispatch.balanced.is_none() {
                            return Err(
                                "the emitted predictive plan has no frozen sequences".to_owned()
                            );
                        }
                        frozen_nodes += validate_predictive_dispatch(dispatch, variants.len())?;
                    }
                    NodeBodyPlan::Pratt(pratt) => {
                        pratt.validate()?;
                        frozen_nodes += pratt.nud_dispatch.frozen_node_count;
                        frozen_nodes += pratt
                            .led_dispatch
                            .as_ref()
                            .map_or(0, |dispatch| dispatch.frozen_node_count);
                    }
                    NodeBodyPlan::Struct { .. } => {}
                }
                let pretty_element_sets = match &application.body {
                    NodeBodyPlan::Struct { elements, .. } => vec![elements.as_slice()],
                    NodeBodyPlan::Enum { variants, .. } => variants
                        .iter()
                        .map(|variant| variant.elements.as_slice())
                        .collect(),
                    NodeBodyPlan::Pratt(_) => Vec::new(),
                };
                for elements in pretty_element_sets {
                    for element in elements {
                        let ElementParseKind::Semantic {
                            projection: semantic,
                            ..
                        } = &element.kind
                        else {
                            continue;
                        };
                        let SemanticParseProjection::Direct {
                            terminal_application,
                            repetition_break,
                            ..
                        } = semantic.as_ref()
                        else {
                            continue;
                        };
                        if let PlannedRepetitionBreak::TopLevelStatement(application) =
                            repetition_break
                            && (*terminal_application != Some(*application)
                                || !self.nodes.iter().any(|candidate| {
                                    candidate.parse_applications.iter().any(|candidate| {
                                        candidate.application == *application
                                            && matches!(candidate.body, NodeBodyPlan::Enum { .. })
                                    })
                                }))
                        {
                            return Err(
                                "the frozen Pretty statement-break family lacks its exact enum application"
                                .to_owned(),
                            );
                        }
                        if matches!(
                            repetition_break,
                            PlannedRepetitionBreak::TopLevelStatementFamily(_)
                        ) {
                            return Err(
                                "a closed Pretty application contains a declaration-family break"
                                    .to_owned(),
                            );
                        }
                    }
                }
            }
            let declaration_elements = match &node.body {
                NodeBodyPlan::Struct { elements, .. } => vec![elements.as_slice()],
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter()
                    .map(|variant| variant.elements.as_slice())
                    .collect(),
                NodeBodyPlan::Pratt(_) => Vec::new(),
            };
            for elements in declaration_elements {
                for element in elements {
                    let ElementParseKind::Semantic {
                        projection: semantic,
                        ..
                    } = &element.kind
                    else {
                        continue;
                    };
                    let SemanticParseProjection::Direct {
                        terminal_application,
                        repetition_break,
                        ..
                    } = semantic.as_ref()
                    else {
                        continue;
                    };
                    match repetition_break {
                        PlannedRepetitionBreak::Soft => {}
                        PlannedRepetitionBreak::TopLevelStatement(_) => {
                            return Err(
                                "a source declaration contains an application-specific Pretty break"
                                    .to_owned(),
                            );
                        }
                        PlannedRepetitionBreak::TopLevelStatementFamily(family) => {
                            let applications = self
                                .nodes
                                .iter()
                                .flat_map(|candidate| &candidate.parse_applications)
                                .filter(|candidate| candidate.node == *family)
                                .collect::<Vec<_>>();
                            if terminal_application.is_some()
                                || applications.is_empty()
                                || applications.iter().any(|candidate| {
                                    !matches!(candidate.body, NodeBodyPlan::Enum { .. })
                                })
                            {
                                return Err(
                                    "the source-derived Pretty statement break lacks its typed enum family"
                                        .to_owned(),
                                );
                            }
                        }
                    }
                }
            }
        }
        if self.instrumentation.frozen_dispatch_nodes_consumed != frozen_nodes
            || self.plan_families.parse != parse_plan_identity(&self.nodes)
        {
            return Err(
                "the emitted predictive plan diverges from frozen semantic analysis decisions"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// Ensures every capability plan belongs to the same frozen grammar family.
    fn validate_plan_families(&self) -> Result<(), String> {
        if self.plan_families.configured_traits
            != configured_trait_plan_identity(&self.nodes, &self.tokens)
        {
            return Err(
                "the generated trait-behavior family diverges from its frozen identity".to_owned(),
            );
        }
        if self.plan_families.private_abi != plan_family_identity(&self.private_abi) {
            return Err("the private ABI plan family diverges from its frozen identity".to_owned());
        }
        let pretty = pretty_family_identity(&self.pretty);
        if self.plan_families.pretty != pretty {
            return Err("the Pretty plan family diverges from its frozen identity".to_owned());
        }
        #[cfg(feature = "diagrams")]
        if self.plan_families.diagrams != self.diagrams.as_ref().map(plan_family_identity) {
            return Err("the diagram plan family diverges from its frozen identity".to_owned());
        }
        #[cfg(not(feature = "diagrams"))]
        if self.plan_families.diagrams.is_some() {
            return Err("the diagram plan family diverges from its frozen identity".to_owned());
        }
        if self.plan_families.explorer != self.explorer.as_ref().map(plan_family_identity) {
            return Err("the explorer plan family diverges from its frozen identity".to_owned());
        }
        if self.plan_families.capabilities != plan_family_identity(&self.capabilities) {
            return Err("the capability plan family diverges from its frozen identity".to_owned());
        }
        Ok(())
    }
}
