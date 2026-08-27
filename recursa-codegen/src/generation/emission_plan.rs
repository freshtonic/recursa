//! Construction and cross-capability validation of the source-aware emission plan.
//!
//! This coordinator joins source analysis to capability-owned plans in a fixed order, freezes
//! every Debug-derived family identity, and validates those identities before rendering.

use super::*;

#[derive(Clone)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct EmissionPlan {
    pub(super) identity: String,
    pub(super) facade: Ident,
    pub(super) module_anchor: syn::Path,
    pub(super) grammar_name: String,
    pub(super) text_equivalence: TextEquivalence,
    pub(super) pretty_max_width: usize,
    pub(super) pretty_indent: usize,
    pub(super) kind_count: usize,
    pub(super) lexical_error_kind: u16,
    pub(super) kind_variants: Vec<Ident>,
    pub(super) kind_static_texts: Vec<Option<String>>,
    pub(super) kind_names: Vec<String>,
    pub(super) atom_names: Vec<String>,
    pub(super) nodes: Vec<NodeParsePlan>,
    pub(super) tokens: Vec<TokenParsePlan>,
    pub(super) token_bindings: Vec<TokenBindingPlan>,
    pub(super) lexical_rules: Vec<LexRulePlan>,
    pub(super) word_classifiers: Vec<WordClassifierPlan>,
    pub(super) word_candidate_kinds: Vec<u16>,
    pub(super) post_lex: Option<syn::Path>,
    pub(super) root_configured_traits: Vec<ConfiguredTraitRecipe>,
    pub(super) private_abi: PrivateAbiPlan,
    pub(super) capabilities: Vec<FacadeCapability>,
    pub(super) pretty: PrettyEmissionPlan,
    #[cfg(feature = "diagrams")]
    pub(super) diagrams: Option<DiagramPlan>,
    pub(super) explorer: Option<ExtensionEmissionPlan>,
    pub(super) instrumentation: EmissionInstrumentation,
    pub(super) plan_families: EmissionPlanFamilies,
}

#[derive(Clone, Debug)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct EmissionPlanFamilies {
    pub(super) configured_traits: u64,
    pub(super) private_abi: u64,
    pub(super) capabilities: u64,
    pub(super) pretty: Option<u64>,
    pub(super) parse: u64,
    pub(super) diagrams: Option<u64>,
    pub(super) explorer: Option<u64>,
}

#[derive(Clone, Default)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct EmissionInstrumentation {
    pub(super) frozen_dispatch_nodes_consumed: usize,
}

#[derive(Default)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct RenderInstrumentation {
    pub(super) frozen_dispatch_nodes_rendered: usize,
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
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
    pub(super) fn join(
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
                    decisions,
                    kind_count,
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
                kind: rule
                    .kind
                    .index()
                    .try_into()
                    .map_err(|_| "generated token kind exceeds runtime width".to_owned())?,
                skip: false,
                callback: match &lowered.lexical_plan.token_kinds[rule.kind.index()].identity {
                    TokenKindIdentity::ContentBase(base) => base.callback.clone(),
                    TokenKindIdentity::Fixed(_) => None,
                },
                fixed: matches!(rule.priority, LexicalPriority::Fixed),
                priority: match rule.priority {
                    LexicalPriority::Fixed => 0,
                    LexicalPriority::Pattern { authored } => authored.unwrap_or(0),
                },
            };
            if !lexical_rules.iter().any(|existing| {
                existing.pattern == planned.pattern && existing.kind == planned.kind
            }) {
                lexical_rules.push(planned);
            }
        }
        for rule in &lowered.lexical_plan.non_token_rules {
            let pattern = match &rule.matcher {
                LexicalMatcher::Regex(pattern) => pattern.clone(),
                LexicalMatcher::Fixed(text) => {
                    fixed_lexical_pattern(text, resolved.build_inputs.text_equivalence)
                }
            };
            lexical_rules.push(LexRulePlan {
                pattern,
                kind: 0,
                skip: true,
                callback: None,
                fixed: false,
                priority: 0,
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
        #[cfg(feature = "diagrams")]
        let authoritative_diagrams = resolved
            .build_inputs
            .diagrams
            .then(|| plan_diagrams(lowered, resolved, &nodes, &kind_names, &kind_static_texts))
            .transpose()?;
        #[cfg(feature = "diagrams")]
        let diagrams = resolved
            .build_inputs
            .diagrams
            .then(|| plan_diagrams(lowered, resolved, &nodes, &kind_names, &kind_static_texts))
            .transpose()?;
        #[cfg(feature = "diagrams")]
        let diagram_enabled = diagrams.is_some();
        #[cfg(not(feature = "diagrams"))]
        let diagram_enabled = false;
        #[cfg(feature = "diagrams")]
        let frozen_diagrams = authoritative_diagrams.as_ref().map(plan_family_identity);
        #[cfg(not(feature = "diagrams"))]
        let frozen_diagrams = None;
        let private_abi = emitted_private_abi_plan();
        #[cfg(feature = "explorer")]
        let explorer = emitted_explorer_plan(&nodes, lowered, &kind_variants)?.map(|explorer| {
            ExtensionEmissionPlan {
                artifacts: Vec::new(),
                explorer,
            }
        });
        #[cfg(not(feature = "explorer"))]
        let explorer: Option<ExtensionEmissionPlan> = None;
        if let Some(explorer) = explorer.as_ref() {
            validate_explorer_collisions(resolved, cfg, &nodes, &explorer.explorer)?;
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
            "{EMITTER_IDENTITY};release={};protocol={PRIVATE_PROTOCOL};facade={facade};anchor={};kinds={kind_count};edition={:?};cfg={:?};",
            env!("CARGO_PKG_VERSION"),
            resolved.build_inputs.module_anchor.segments.join("::"),
            resolved.build_inputs.discovery.edition,
            resolved.build_inputs.discovery.cfg_snapshot,
        );
        for source in &resolved.tree.files {
            canonical.push_str(&format!(
                ";source={}:{}:{:016x}",
                source.id.index(),
                source.path.display(),
                stable_hash(source.text.as_bytes())
            ));
        }
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
            canonical.push_str(&format!(
                ";token={}:{}:{}:{:?}",
                token.ident,
                token.kind,
                token.capture.canonical_name(),
                token.accepted_kind_variants,
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
                ";rule={:?}:{}:{}:{}:{}:{}",
                rule.pattern,
                rule.kind,
                rule.skip,
                rule.callback
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), crate::source_syntax::path_text,),
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
        if let Some(post_lex) = &lowered.source_modeled.source_model().post_lex {
            canonical.push_str(&format!(
                ";post_lex={}",
                crate::source_syntax::path_text(post_lex),
            ));
        }
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
            nodes,
            tokens,
            token_bindings,
            lexical_rules,
            word_classifiers,
            word_candidate_kinds,
            post_lex: lowered.source_modeled.source_model().post_lex.clone(),
            root_configured_traits,
            private_abi,
            capabilities,
            pretty,
            #[cfg(feature = "diagrams")]
            diagrams,
            explorer,
            instrumentation,
            plan_families,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.identity.is_empty() || self.nodes.is_empty() {
            return Err("the Emission plan lacks a Generation identity or Nodes".to_owned());
        }
        if self.kind_variants.len() + 1 != self.kind_count
            || self.kind_static_texts.len() + 1 != self.kind_count
            || self.kind_names.len() != self.kind_count
        {
            return Err("the generated token-kind plan has inconsistent width".to_owned());
        }
        if self.lexical_rules.is_empty() {
            return Err("the generated lexical plan has no rules".to_owned());
        }
        #[cfg(feature = "diagrams")]
        if self
            .diagrams
            .as_ref()
            .is_some_and(|diagrams| diagrams.nodes.len() != self.nodes.len())
        {
            return Err("the diagram plan does not cover every frozen Node".to_owned());
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
                        if dispatch.frozen_sequence_count == 0 {
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
                            terminal_application,
                            repetition_break,
                            ..
                        } = &element.kind
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
                        terminal_application,
                        repetition_break,
                        ..
                    } = &element.kind
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

    fn validate_plan_families(&self) -> Result<(), String> {
        if self.plan_families.configured_traits
            != configured_trait_plan_identity(&self.nodes, &self.tokens)
        {
            return Err(
                "the configured-trait plan family diverges from its frozen identity".to_owned(),
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
