//! Explorer adapter planning, collision validation, schema, and occurrence rendering.
//!
//! Explorer remains an optional grammar-erased capability, but its plan is always validated
//! consistently so feature-off output cannot leak schema or adapter payloads.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(all(test, feature = "explorer"))]
mod tests {
    use super::super::test_support;
    #[test]
    fn explorer_plan_freezes_the_complete_typed_static_schema() {
        test_support::explorer_plan_freezes_the_complete_typed_static_schema();
    }
    #[test]
    fn explorer_rendering_consumes_its_frozen_schema_plan() {
        test_support::explorer_rendering_consumes_its_frozen_schema_plan();
    }
    #[test]
    fn explorer_output_omits_only_redundant_lifetimes() {
        test_support::explorer_output_omits_only_redundant_lifetimes();
    }
    #[test]
    fn view_collisions_are_rejected_at_the_emission_plan_seam() {
        test_support::view_collisions_are_rejected_at_the_emission_plan_seam();
    }
    #[test]
    fn explorer_value_collisions_are_rejected_at_the_emission_plan_seam() {
        test_support::explorer_value_collisions_are_rejected_at_the_emission_plan_seam();
    }
    #[test]
    fn explorer_method_collisions_are_rejected_at_the_emission_plan_seam() {
        test_support::explorer_method_collisions_are_rejected_at_the_emission_plan_seam();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Explorer schema plus extension artifacts prepared for publication.
pub(super) struct ExtensionEmissionPlan {
    /// Extension artifacts emitted alongside the Explorer plan.
    pub(super) artifacts: Vec<ExtensionArtifactPlan>,
    /// Frozen grammar-erased schema and callback plan.
    pub(super) explorer: ExplorerPlan,
}

impl std::ops::Deref for ExtensionEmissionPlan {
    type Target = ExplorerPlan;

    fn deref(&self) -> &Self::Target {
        &self.explorer
    }
}

impl std::ops::DerefMut for ExtensionEmissionPlan {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.explorer
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Named generated artifact owned by the extension publication stage.
pub(super) struct ExtensionArtifactPlan {
    /// Generation-relative artifact identity.
    pub(super) logical_name: String,
    /// Rendered artifact body retained for publication.
    pub(super) content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete grammar-erased schema and callback projection.
pub(super) struct ExplorerPlan {
    /// Grammar-level Explorer display metadata.
    pub(super) grammar: ExplorerGrammarPlan,
    /// Source-visible root applications eligible for adapters.
    pub(super) roots: Vec<ExplorerRootPlan>,
    /// Occurrence schemas in materialized application order.
    pub(super) nodes: Vec<ExplorerNodePlan>,
    /// Token schemas in grammar-local kind order.
    pub(super) tokens: Vec<ExplorerTokenPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar-level identity exposed by an Explorer adapter.
pub(super) struct ExplorerGrammarPlan {
    /// Grammar display name exposed through the adapter schema.
    pub(super) name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Adapter-eligible root application and its optional capabilities.
pub(super) struct ExplorerRootPlan {
    /// Root Node declaration identity.
    pub(super) node: NodeId,
    /// Materialized root application selected for callbacks.
    pub(super) application: MaterializedApplicationId,
    /// Optional operations valid for this root application.
    pub(super) capabilities: ExplorerCapabilityPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Operations generated for one concrete root application.
pub(super) struct ExplorerCapabilityPlan {
    /// Whether the root can render structured Pretty output.
    pub(super) pretty: bool,
    /// Whether the root can generate deterministic samples.
    pub(super) arbitrary: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar-erased identity, classification, and display metadata for a token.
pub(super) struct ExplorerTokenPlan {
    /// Grammar-local numeric kind used only to align static metadata.
    pub(super) kind_index: u16,
    /// Content-token identity, or `None` for fixed syntax.
    pub(super) semantic: Option<ContentTokenId>,
    /// Grammar-local token display name.
    pub(super) name: String,
    /// Collision-free schema identity within the selected grammar.
    pub(super) qualified_name: String,
    /// Generated typed kind-variant name.
    pub(super) kind_name: String,
    /// Fixed or semantic content-token classification.
    pub(super) kind: ExplorerTokenKindPlan,
    /// Stable fixed spelling, absent for content tokens.
    pub(super) canonical_text: Option<String>,
    /// Consumer-facing syntax highlight category.
    pub(super) highlight: ExplorerHighlightClassPlan,
    /// Deterministic label used in dynamic schema views.
    pub(super) display: ExplorerDisplayPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(feature = "explorer"), allow(dead_code))]
/// Distinguishes fixed syntax from value-carrying tokens.
pub(super) enum ExplorerTokenKindPlan {
    /// Token has a canonical authored spelling.
    Fixed,
    /// Token stores or matches source text dynamically.
    Content,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(feature = "explorer"), allow(dead_code))]
/// Consumer-facing syntax category for token highlighting.
pub(super) enum ExplorerHighlightClassPlan {
    /// Word-like fixed syntax.
    Keyword,
    /// Non-word fixed syntax.
    Punctuation,
    /// Value-carrying token content.
    Content,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar-erased schema for one materialized node application.
pub(super) struct ExplorerNodePlan {
    /// Source Node declaration represented by this occurrence schema.
    pub(super) node: NodeId,
    /// Concrete materialized type application.
    pub(super) application: MaterializedApplicationId,
    /// Unqualified Node display name.
    pub(super) name: String,
    /// Grammar-unique Node schema identity.
    pub(super) qualified_name: String,
    /// Human-readable type spelling for Explorer consumers.
    pub(super) display: ExplorerDisplayPlan,
    /// Struct, enum, or Pratt occurrence shape.
    pub(super) body: ExplorerNodeBodyPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Occurrence shapes available for a node application.
pub(super) enum ExplorerNodeBodyPlan {
    /// Single structural shape.
    Struct(ExplorerVariantPlan),
    /// One of the authored enum variants.
    Enum(Vec<ExplorerVariantPlan>),
    /// One of the analyzed Pratt variant roles.
    Pratt(Vec<ExplorerVariantPlan>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Named occurrence shape and its source-ordered elements.
pub(super) struct ExplorerVariantPlan {
    /// Resolved variant identity, absent for a struct body.
    pub(super) variant: Option<ResolvedVariantId>,
    /// Variant display name, absent for a struct body.
    pub(super) name: Option<String>,
    /// Grammar-unique variant schema identity.
    pub(super) qualified_name: Option<String>,
    /// Source-order grammar elements in this active shape.
    pub(super) elements: Vec<ExplorerElementPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Schema and runtime provenance for one grammar element.
pub(super) struct ExplorerElementPlan {
    /// Parse-plan origin of the grammar element.
    pub(super) source: ExplorerElementSource,
    /// Stable source-facing element label.
    pub(super) name: String,
    /// Fixed-token, semantic-field, or Pratt-left-hand role.
    pub(super) role: ExplorerElementRolePlan,
    /// Occurrence cardinality after supported-container lowering.
    pub(super) cardinality: ExplorerCardinalityPlan,
    /// Fixed/content-token classification when the element consumes a token.
    pub(super) token_classification: Option<ExplorerTokenClassificationPlan>,
    /// Child Node or content-token schema target, when semantic.
    pub(super) target: Option<ExplorerElementTargetPlan>,
    /// Grammar-local kind used by fixed syntax.
    pub(super) token_kind: Option<u16>,
    /// Semantic attachment child whose occurrence owns the token.
    pub(super) attachment_semantic_child: Option<usize>,
    /// Consumer-facing element spelling.
    pub(super) display: ExplorerDisplayPlan,
    /// Provenance recipe for locating runtime child occurrences.
    pub(super) occurrence: ExplorerOccurrencePlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Grammar-erased schema reached by a semantic element.
pub(super) enum ExplorerElementTargetPlan {
    /// References a materialized generated-node application.
    Node(MaterializedApplicationId),
    /// References an authored content-token declaration.
    ContentToken(ContentTokenId),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Parse-plan identity from which an Explorer element originates.
pub(super) enum ExplorerElementSource {
    /// Originates from an authored fixed-token use.
    Fixed(TokenUseId),
    /// Originates from an erased optional fixed-token group.
    OptionalFixed(crate::lowering::ElementId),
    /// Originates from an ordinary semantic field.
    Semantic(ResolvedFieldId),
    /// Originates from a Pratt extender's left operand.
    PrattLhs(ResolvedFieldId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Consumer-visible role of an element in its occurrence shape.
pub(super) enum ExplorerElementRolePlan {
    /// Element represents fixed syntax.
    FixedToken,
    /// Element represents a stored semantic field.
    SemanticField,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Supported occurrence count after container lowering.
pub(super) enum ExplorerCardinalityPlan {
    /// Exactly one occurrence.
    One,
    /// Zero or one occurrence.
    Optional,
    /// Any number of occurrences.
    ZeroOrMore,
    /// At least one occurrence.
    OneOrMore,
    /// Optional collection whose present value is non-empty.
    OptionalOneOrMore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token storage class attached to an element schema.
pub(super) enum ExplorerTokenClassificationPlan {
    /// Element consumes canonical fixed syntax.
    Fixed,
    /// Element consumes a value-carrying token.
    Content,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(feature = "explorer"), allow(dead_code))]
/// Rule for deriving text shown for a schema or occurrence.
pub(super) enum ExplorerDisplayPlan {
    /// No display text is available.
    None,
    /// Always displays the stored canonical text.
    Fixed(String),
    /// Displays source text when matched and stored text otherwise.
    MatchedOrStored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Runtime recipe for projecting an element occurrence.
pub(super) enum ExplorerOccurrencePlan {
    /// Occurrence has no traversable semantic children.
    Leaf,
    /// Traverses a wrapped node or content-token value.
    Children {
        /// Supported container layers surrounding the terminal application.
        wrappers: Vec<FieldWrapper>,
        /// Node or content-token reached after removing wrappers.
        terminal: ExplorerTerminalPlan,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Traversable terminal reached after storage wrappers are removed.
pub(super) enum ExplorerTerminalPlan {
    /// Terminal is a materialized generated-node application.
    Node(MaterializedApplicationId),
    /// Terminal is an authored content token.
    Content(ContentTokenId),
}
/// Projects the frozen emission plan into runtime Explorer schemas and eligible roots.
#[cfg(feature = "explorer")]
pub(super) fn emitted_explorer_plan(
    nodes: &[NodeParsePlan],
    lowered: &crate::materialization::LoweredGrammar,
    kind_variants: &[Ident],
    framing_root: Option<MaterializedApplicationId>,
) -> Result<Option<ExplorerPlan>, String> {
    let mut roots = nodes
        .iter()
        .flat_map(|node| {
            node.parse_applications
                .iter()
                .filter(move |application| {
                    explorer_emitted_application_is_eligible(node)
                        || framing_root == Some(application.application)
                })
                .map(move |application| ExplorerRootPlan {
                    node: node.node,
                    application: application.application,
                    capabilities: ExplorerCapabilityPlan {
                        pretty: node
                            .configured_traits
                            .contains(&ConfiguredTraitRecipe::Pretty),
                        arbitrary: node
                            .configured_traits
                            .contains(&ConfiguredTraitRecipe::Arbitrary),
                    },
                })
        })
        .collect::<Vec<_>>();
    roots.sort_by_key(|root| root.application.index());
    let mut planned_nodes = nodes
        .iter()
        .flat_map(|node| {
            node.parse_applications
                .iter()
                .map(move |application| emitted_explorer_node(node, application))
        })
        .collect::<Vec<_>>();
    planned_nodes.sort_by_key(|node| node.application.index());
    if roots.is_empty() {
        return Ok(None);
    }
    let tokens = explorer_token_plans(lowered, kind_variants)?;
    Ok(Some(ExplorerPlan {
        grammar: ExplorerGrammarPlan {
            name: lowered.grammar.name.clone(),
        },
        roots,
        nodes: planned_nodes,
        tokens,
    }))
}

#[cfg(feature = "explorer")]
/// Reconstructs Explorer semantics directly from lowered authored source for validation.
pub(super) fn authoritative_explorer_plan(
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<Option<ExplorerPlan>, String> {
    let resolved = lowered.source_modeled.resolved();
    let source = lowered.source_modeled.source_model();
    let mut roots = Vec::new();
    for application in &lowered.applications {
        let node = resolved
            .nodes
            .iter()
            .find(|node| node.id == application.node)
            .ok_or_else(|| "an explorer application lacks its source Node".to_owned())?;
        let configured = source
            .nodes
            .iter()
            .find(|source_node| source_node.resolved_node == node.id)
            .ok_or_else(|| format!("Node `{}` lacks configured explorer traits", node.ident))?;
        let framing_root = lowered
            .lowering_map
            .framing
            .is_some_and(|framing| framing.application == application.id);
        if explorer_source_application_is_eligible(resolved, node, application, configured)
            || framing_root
        {
            roots.push(ExplorerRootPlan {
                node: node.id,
                application: application.id,
                capabilities: ExplorerCapabilityPlan {
                    pretty: configured
                        .configured_traits
                        .effective
                        .iter()
                        .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty),
                    arbitrary: configured
                        .configured_traits
                        .effective
                        .iter()
                        .any(|selection| selection.recipe == ConfiguredTraitRecipe::Arbitrary),
                },
            });
        }
    }
    roots.sort_by_key(|root| root.application.index());
    if roots.is_empty() {
        return Ok(None);
    }

    let kind_variants = planned_kind_variants(lowered)?;
    let tokens = explorer_token_plans(lowered, &kind_variants)?;
    let nodes = lowered
        .applications
        .iter()
        .map(|application| authoritative_explorer_node(lowered, application))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Some(ExplorerPlan {
        grammar: ExplorerGrammarPlan {
            name: lowered.grammar.name.clone(),
        },
        roots,
        nodes,
        tokens,
    }))
}

#[cfg(feature = "explorer")]
/// Joins token metadata and kind tables into schema entries in kind order.
pub(super) fn explorer_token_plans(
    lowered: &crate::materialization::LoweredGrammar,
    kind_variants: &[Ident],
) -> Result<Vec<ExplorerTokenPlan>, String> {
    lowered
        .lexical_plan
        .token_kinds
        .iter()
        .zip(kind_variants)
        .map(|(kind, variant)| {
            let kind_index = u16::try_from(kind.id.index())
                .map_err(|_| "an explorer token kind exceeds runtime width".to_owned())?;
            let name = variant.unraw().to_string();
            let qualified_name = format!("{}::{name}", lowered.grammar.name);
            let semantic = lowered
                .lexical_plan
                .parser_targets
                .iter()
                .find(|target| target.base_kind == kind.id)
                .map(|target| target.semantic);
            let (token_kind, canonical_text, highlight, display) = match &kind.identity {
                TokenKindIdentity::Fixed(spelling) => {
                    let highlight = if lowered
                        .lexical_plan
                        .keywords
                        .iter()
                        .any(|keyword| keyword.kind == kind.id)
                    {
                        ExplorerHighlightClassPlan::Keyword
                    } else {
                        ExplorerHighlightClassPlan::Punctuation
                    };
                    (
                        ExplorerTokenKindPlan::Fixed,
                        Some(fixed_pretty_spelling(
                            spelling,
                            lowered.grammar.text_equivalence,
                        )),
                        highlight,
                        ExplorerDisplayPlan::Fixed(fixed_pretty_spelling(
                            spelling,
                            lowered.grammar.text_equivalence,
                        )),
                    )
                }
                TokenKindIdentity::ContentBase(_) => (
                    ExplorerTokenKindPlan::Content,
                    None,
                    ExplorerHighlightClassPlan::Content,
                    ExplorerDisplayPlan::MatchedOrStored,
                ),
            };
            Ok(ExplorerTokenPlan {
                kind_index,
                semantic,
                name,
                qualified_name,
                kind_name: lowered.grammar.kinds[kind.id.index()].name.clone(),
                kind: token_kind,
                canonical_text,
                highlight,
                display,
            })
        })
        .collect()
}

/// Resolves a planned token variant to its grammar-local kind index.
pub(super) fn explorer_kind_index(
    lowered: &crate::materialization::LoweredGrammar,
    definition: TokenDefinitionId,
) -> Result<u16, String> {
    let kind = lowered
        .lexical_plan
        .lowering_map
        .token_definitions
        .iter()
        .find(|correlation| correlation.source == definition)
        .ok_or_else(|| "an explorer fixed token lacks a lexical kind".to_owned())?
        .lowered;
    u16::try_from(kind.index())
        .map_err(|_| "an explorer fixed token kind exceeds runtime width".to_owned())
}

#[cfg(feature = "explorer")]
/// Reports whether a materialized emitted application may expose a root adapter.
pub(super) fn explorer_emitted_application_is_eligible(node: &NodeParsePlan) -> bool {
    node.publicly_materialized && node.explorer_visit_configured
}

#[cfg(feature = "explorer")]
/// Applies the same root-eligibility rules to authoritative source applications.
pub(super) fn explorer_source_application_is_eligible(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    node: &ResolvedNode,
    _application: &MaterializedNodeApplication,
    configured: &crate::source_model::Node,
) -> bool {
    publicly_reachable(&resolved.tree, node.module, &node.visibility)
        && (configured
            .configured_traits
            .effective
            .iter()
            .any(|selection| selection.recipe == ConfiguredTraitRecipe::Visit)
            || configured
                .configured_traits
                .omissions
                .iter()
                .any(|omission| omission.recipe == ConfiguredTraitRecipe::Visit))
}

#[cfg(feature = "explorer")]
/// Projects one materialized parse plan into an occurrence schema.
pub(super) fn emitted_explorer_node(
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> ExplorerNodePlan {
    let qualified_name = node.semantic_key.clone();
    let body = match &application.body {
        NodeBodyPlan::Struct { elements, .. } => ExplorerNodeBodyPlan::Struct(
            emitted_explorer_variant(None, None, &qualified_name, elements.iter()),
        ),
        NodeBodyPlan::Enum { variants, .. } => ExplorerNodeBodyPlan::Enum(
            variants
                .iter()
                .map(|variant| {
                    emitted_explorer_variant(
                        Some(variant.resolved_variant),
                        Some(variant.ident.unraw().to_string()),
                        &qualified_name,
                        variant.elements.iter(),
                    )
                })
                .collect(),
        ),
        NodeBodyPlan::Pratt(pratt) => ExplorerNodeBodyPlan::Pratt(
            pratt
                .nud
                .iter()
                .map(|branch| {
                    emitted_explorer_variant(
                        Some(branch.variant.resolved_variant),
                        Some(branch.variant.ident.unraw().to_string()),
                        &qualified_name,
                        branch.variant.elements.iter(),
                    )
                })
                .chain(pratt.led.iter().map(|branch| {
                    emitted_explorer_variant(
                        Some(branch.variant.resolved_variant),
                        Some(branch.variant.ident.unraw().to_string()),
                        &qualified_name,
                        std::iter::once(&branch.lhs).chain(branch.variant.elements.iter()),
                    )
                }))
                .collect(),
        ),
    };
    ExplorerNodePlan {
        node: node.node,
        application: application.application,
        name: node.ident.unraw().to_string(),
        qualified_name,
        display: ExplorerDisplayPlan::None,
        body,
    }
}

#[cfg(feature = "explorer")]
/// Projects one emitted Node shape into source-order Explorer elements.
pub(super) fn emitted_explorer_variant<'a>(
    variant: Option<ResolvedVariantId>,
    name: Option<String>,
    node_qualified_name: &str,
    elements: impl IntoIterator<Item = &'a ElementParsePlan>,
) -> ExplorerVariantPlan {
    let qualified_name = name
        .as_ref()
        .map(|name| format!("{node_qualified_name}::{name}"));
    ExplorerVariantPlan {
        variant,
        name,
        qualified_name,
        elements: elements.into_iter().map(emitted_explorer_element).collect(),
    }
}

#[cfg(feature = "explorer")]
/// Joins one parse element with its schema target and provenance recipe.
pub(super) fn emitted_explorer_element(element: &ElementParsePlan) -> ExplorerElementPlan {
    match &element.kind {
        ElementParseKind::Fixed { kind, spelling, .. } => ExplorerElementPlan {
            source: element.explorer_source,
            name: element.metadata_name.clone(),
            role: ExplorerElementRolePlan::FixedToken,
            cardinality: ExplorerCardinalityPlan::One,
            token_classification: Some(ExplorerTokenClassificationPlan::Fixed),
            target: None,
            token_kind: Some(*kind),
            attachment_semantic_child: None,
            display: ExplorerDisplayPlan::Fixed(spelling.clone()),
            occurrence: ExplorerOccurrencePlan::Leaf,
        },
        ElementParseKind::OptionalFixedTokenGroup { .. } => ExplorerElementPlan {
            source: element.explorer_source,
            name: element.metadata_name.clone(),
            role: ExplorerElementRolePlan::FixedToken,
            cardinality: ExplorerCardinalityPlan::Optional,
            token_classification: Some(ExplorerTokenClassificationPlan::Fixed),
            target: None,
            token_kind: None,
            attachment_semantic_child: None,
            display: ExplorerDisplayPlan::None,
            occurrence: ExplorerOccurrencePlan::Leaf,
        },
        ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
            SemanticParseProjection::FixedPresence { .. } => ExplorerElementPlan {
                source: element.explorer_source,
                name: element.metadata_name.clone(),
                role: ExplorerElementRolePlan::SemanticField,
                cardinality: ExplorerCardinalityPlan::One,
                token_classification: None,
                target: None,
                token_kind: None,
                attachment_semantic_child: None,
                display: ExplorerDisplayPlan::None,
                occurrence: ExplorerOccurrencePlan::Leaf,
            },
            SemanticParseProjection::Direct {
                explorer_terminal,
                wrappers,
                attachment,
                ..
            } => ExplorerElementPlan {
                source: element.explorer_source,
                name: element.metadata_name.clone(),
                role: ExplorerElementRolePlan::SemanticField,
                cardinality: explorer_cardinality_plan(wrappers),
                token_classification: matches!(explorer_terminal, ExplorerTerminalPlan::Content(_))
                    .then_some(ExplorerTokenClassificationPlan::Content),
                target: Some(match explorer_terminal {
                    ExplorerTerminalPlan::Node(application) => {
                        ExplorerElementTargetPlan::Node(*application)
                    }
                    ExplorerTerminalPlan::Content(token) => {
                        ExplorerElementTargetPlan::ContentToken(*token)
                    }
                }),
                token_kind: None,
                attachment_semantic_child: attachment
                    .as_ref()
                    .map(|attachment| attachment.before.len()),
                display: ExplorerDisplayPlan::None,
                occurrence: ExplorerOccurrencePlan::Children {
                    wrappers: wrappers.clone(),
                    terminal: *explorer_terminal,
                },
            },
        },
    }
}

/// Reconstructs one occurrence schema from lowered source and semantic correlations.
///
/// # Long-function contract
///
/// Rationale: freezes one erased Explorer node from source-owned application and view facts.
/// Invariants: only source-visible eligible applications enter the schema and declaration order remains stable.
/// Phases: resolves the application, projects schema elements, then records callable capabilities.
pub(super) fn authoritative_explorer_node(
    lowered: &crate::materialization::LoweredGrammar,
    application: &MaterializedNodeApplication,
) -> Result<ExplorerNodePlan, String> {
    let resolved = lowered.source_modeled.resolved();
    let node = resolved
        .nodes
        .iter()
        .find(|node| node.id == application.node)
        .ok_or_else(|| "an explorer application lacks its source Node".to_owned())?;
    let body = match &application.body {
        MaterializedNodeBody::Struct(elements) | MaterializedNodeBody::Presence(elements) => {
            ExplorerNodeBodyPlan::Struct(authoritative_explorer_variant(
                lowered, node, None, None, elements,
            )?)
        }
        MaterializedNodeBody::Enum(variants) => ExplorerNodeBodyPlan::Enum(
            variants
                .iter()
                .map(|variant| {
                    authoritative_explorer_variant(
                        lowered,
                        node,
                        Some(variant.resolved_variant),
                        Some(&variant.name),
                        &variant.elements,
                    )
                })
                .collect::<Result<Vec<_>, String>>()?,
        ),
        MaterializedNodeBody::Pratt(variants) => {
            let mut planned = variants
                .iter()
                .map(|variant| match variant {
                    MaterializedPrattVariant::Nud(nud) => authoritative_explorer_variant(
                        lowered,
                        node,
                        Some(nud.common.resolved_variant),
                        Some(&nud.common.name),
                        &nud.body,
                    ),
                    MaterializedPrattVariant::Led(led) => {
                        let mut planned = authoritative_explorer_variant(
                            lowered,
                            node,
                            Some(led.common.resolved_variant),
                            Some(&led.common.name),
                            &led.tail,
                        )?;
                        let resolved_variant = resolved
                            .nodes
                            .iter()
                            .find(|candidate| candidate.id == node.id)
                            .and_then(|candidate| match &candidate.body {
                                ResolvedNodeBody::Enum { variants } => variants
                                    .iter()
                                    .find(|variant| variant.id == led.common.resolved_variant),
                                ResolvedNodeBody::Struct { .. } => None,
                            })
                            .ok_or_else(|| {
                                "a Pratt explorer variant lacks source syntax".to_owned()
                            })?;
                        let field = node
                            .fields
                            .iter()
                            .find(|field| field.id == led.lhs.field)
                            .ok_or_else(|| "a Pratt explorer lhs lacks source syntax".to_owned())?;
                        let index = resolved_variant
                            .fields
                            .iter()
                            .position(|candidate| *candidate == led.lhs.field)
                            .ok_or_else(|| {
                                "a Pratt explorer lhs lacks its variant slot".to_owned()
                            })?;
                        let base = field.syntax.ident.as_ref().map_or_else(
                            || format!("field_{index}"),
                            |ident| ident.unraw().to_string(),
                        );
                        planned.elements.insert(
                            0,
                            ExplorerElementPlan {
                                source: ExplorerElementSource::PrattLhs(led.lhs.field),
                                name: format!("{}_{}", snake_case(&led.common.name), base),
                                role: ExplorerElementRolePlan::SemanticField,
                                cardinality: ExplorerCardinalityPlan::One,
                                token_classification: None,
                                target: Some(ExplorerElementTargetPlan::Node(
                                    led.lhs.owner.application,
                                )),
                                token_kind: None,
                                attachment_semantic_child: None,
                                display: ExplorerDisplayPlan::None,
                                occurrence: ExplorerOccurrencePlan::Children {
                                    wrappers: vec![FieldWrapper::Boxed],
                                    terminal: ExplorerTerminalPlan::Node(led.lhs.owner.application),
                                },
                            },
                        );
                        Ok(planned)
                    }
                })
                .collect::<Result<Vec<_>, String>>()?;
            planned.sort_by_key(|planned| {
                variants
                    .iter()
                    .find(|variant| match variant {
                        MaterializedPrattVariant::Nud(nud) => {
                            Some(nud.common.resolved_variant) == planned.variant
                        }
                        MaterializedPrattVariant::Led(led) => {
                            Some(led.common.resolved_variant) == planned.variant
                        }
                    })
                    .is_some_and(|variant| matches!(variant, MaterializedPrattVariant::Led(_)))
            });
            ExplorerNodeBodyPlan::Pratt(planned)
        }
    };
    Ok(ExplorerNodePlan {
        node: node.id,
        application: application.id,
        name: node.ident.clone(),
        qualified_name: node.key.segments.join("::"),
        display: ExplorerDisplayPlan::None,
        body,
    })
}

/// Reconstructs one authoritative Node shape and its ordered elements.
pub(super) fn authoritative_explorer_variant(
    lowered: &crate::materialization::LoweredGrammar,
    node: &ResolvedNode,
    variant: Option<ResolvedVariantId>,
    name: Option<&str>,
    elements: &[MaterializedElement],
) -> Result<ExplorerVariantPlan, String> {
    let qualified_name = name.map(|name| format!("{}::{name}", node.key.segments.join("::")));
    Ok(ExplorerVariantPlan {
        variant,
        name: name.map(str::to_owned),
        qualified_name,
        elements: elements
            .iter()
            .map(|element| authoritative_explorer_element(lowered, node, variant, name, element))
            .collect::<Result<Vec<_>, String>>()?,
    })
}

/// Reconstructs one element contract without consulting emitted Explorer data.
pub(super) fn authoritative_explorer_element(
    lowered: &crate::materialization::LoweredGrammar,
    node: &ResolvedNode,
    variant: Option<ResolvedVariantId>,
    variant_name: Option<&str>,
    element: &MaterializedElement,
) -> Result<ExplorerElementPlan, String> {
    match element {
        MaterializedElement::FixedToken { token_use } => {
            let source = lowered.source_modeled.source_model();
            let usage = &source.token_uses[token_use.index()];
            let definition = &source.token_definitions[usage.definition.index()];
            let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                return Err("an explorer fixed occurrence lacks fixed-token syntax".to_owned());
            };
            Ok(ExplorerElementPlan {
                source: ExplorerElementSource::Fixed(*token_use),
                name: fixed_element_name(node, usage, definition, fixed),
                role: ExplorerElementRolePlan::FixedToken,
                cardinality: ExplorerCardinalityPlan::One,
                token_classification: Some(ExplorerTokenClassificationPlan::Fixed),
                target: None,
                token_kind: Some(explorer_kind_index(lowered, definition.id)?),
                attachment_semantic_child: None,
                display: ExplorerDisplayPlan::Fixed(fixed_pretty_spelling(
                    &fixed.spelling,
                    lowered.grammar.text_equivalence,
                )),
                occurrence: ExplorerOccurrencePlan::Leaf,
            })
        }
        MaterializedElement::OptionalFixedTokenGroup {
            element, tokens, ..
        } => {
            let source = lowered.source_modeled.source_model();
            let name = format!(
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
            Ok(ExplorerElementPlan {
                source: ExplorerElementSource::OptionalFixed(*element),
                name,
                role: ExplorerElementRolePlan::FixedToken,
                cardinality: ExplorerCardinalityPlan::Optional,
                token_classification: Some(ExplorerTokenClassificationPlan::Fixed),
                target: None,
                token_kind: None,
                attachment_semantic_child: None,
                display: ExplorerDisplayPlan::None,
                occurrence: ExplorerOccurrencePlan::Leaf,
            })
        }
        MaterializedElement::SemanticField {
            field,
            projection: MaterializedSemanticFieldProjection::FixedPresence { .. },
            ..
        } => {
            let resolved_field = node
                .fields
                .iter()
                .find(|candidate| candidate.id == *field)
                .ok_or_else(|| "an explorer presence occurrence lacks source syntax".to_owned())?;
            let field_index = variant
                .map_or_else(
                    || {
                        node.fields
                            .iter()
                            .position(|candidate| candidate.id == *field)
                    },
                    |variant| match &node.body {
                        ResolvedNodeBody::Enum { variants } => variants
                            .iter()
                            .find(|candidate| candidate.id == variant)
                            .and_then(|variant| {
                                variant
                                    .fields
                                    .iter()
                                    .position(|candidate| candidate == field)
                            }),
                        ResolvedNodeBody::Struct { .. } => None,
                    },
                )
                .ok_or_else(|| "an explorer presence occurrence lacks its field slot".to_owned())?;
            let field_name = resolved_field
                .syntax
                .ident
                .as_ref()
                .map(|ident| ident.unraw().to_string())
                .unwrap_or_else(|| format!("field_{field_index}"));
            Ok(ExplorerElementPlan {
                source: ExplorerElementSource::Semantic(*field),
                name: variant_name.map_or_else(
                    || field_name.clone(),
                    |variant| format!("{}_{}", snake_case(variant), field_name),
                ),
                role: ExplorerElementRolePlan::SemanticField,
                cardinality: ExplorerCardinalityPlan::One,
                token_classification: None,
                target: None,
                token_kind: None,
                attachment_semantic_child: None,
                display: ExplorerDisplayPlan::None,
                occurrence: ExplorerOccurrencePlan::Leaf,
            })
        }
        MaterializedElement::SemanticField {
            field,
            projection: MaterializedSemanticFieldProjection::Direct { ty, .. },
            attachment,
            ..
        } => {
            let resolved_field = node
                .fields
                .iter()
                .find(|candidate| candidate.id == *field)
                .ok_or_else(|| "an explorer semantic occurrence lacks source syntax".to_owned())?;
            let field_index = variant
                .map_or_else(
                    || {
                        node.fields
                            .iter()
                            .position(|candidate| candidate.id == *field)
                    },
                    |variant| match &node.body {
                        ResolvedNodeBody::Enum { variants } => variants
                            .iter()
                            .find(|candidate| candidate.id == variant)
                            .and_then(|variant| {
                                variant
                                    .fields
                                    .iter()
                                    .position(|candidate| candidate == field)
                            }),
                        ResolvedNodeBody::Struct { .. } => None,
                    },
                )
                .ok_or_else(|| "an explorer semantic occurrence lacks its field slot".to_owned())?;
            let field_name = resolved_field
                .syntax
                .ident
                .as_ref()
                .map(|ident| ident.unraw().to_string())
                .unwrap_or_else(|| format!("field_{field_index}"));
            let name = variant_name.map_or_else(
                || field_name.clone(),
                |variant| format!("{}_{}", snake_case(variant), field_name),
            );
            let wrappers = field_wrappers(ty)?;
            let terminal = explorer_terminal(ty)?;
            Ok(ExplorerElementPlan {
                source: ExplorerElementSource::Semantic(*field),
                name,
                role: ExplorerElementRolePlan::SemanticField,
                cardinality: explorer_cardinality_plan(&wrappers),
                token_classification: matches!(terminal, ExplorerTerminalPlan::Content(_))
                    .then_some(ExplorerTokenClassificationPlan::Content),
                target: Some(match terminal {
                    ExplorerTerminalPlan::Node(application) => {
                        ExplorerElementTargetPlan::Node(application)
                    }
                    ExplorerTerminalPlan::Content(token) => {
                        ExplorerElementTargetPlan::ContentToken(token)
                    }
                }),
                token_kind: None,
                attachment_semantic_child: attachment
                    .as_ref()
                    .map(|attachment| attachment.before.len()),
                display: ExplorerDisplayPlan::None,
                occurrence: ExplorerOccurrencePlan::Children { wrappers, terminal },
            })
        }
    }
}

/// Maps a supported container shape to its dynamic occurrence cardinality.
pub(super) fn explorer_cardinality_plan(wrappers: &[FieldWrapper]) -> ExplorerCardinalityPlan {
    let optional = wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));
    let one_or_more = wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::OneOrMore));
    if optional && one_or_more {
        ExplorerCardinalityPlan::OptionalOneOrMore
    } else if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore))
    {
        ExplorerCardinalityPlan::ZeroOrMore
    } else if one_or_more {
        ExplorerCardinalityPlan::OneOrMore
    } else if optional {
        ExplorerCardinalityPlan::Optional
    } else {
        ExplorerCardinalityPlan::One
    }
}

/// Derives typed kind-variant names aligned with token metadata.
pub(super) fn planned_kind_variants(
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<Vec<Ident>, String> {
    let mut used = BTreeSet::new();
    let mut variants = Vec::new();
    for kind in &lowered.lexical_plan.token_kinds {
        let candidate = match &kind.identity {
            TokenKindIdentity::Fixed(_) => lowered
                .lexical_plan
                .lowering_map
                .token_definitions
                .iter()
                .find(|correlation| correlation.lowered == kind.id)
                .and_then(|correlation| {
                    lowered
                        .source_modeled
                        .source_model()
                        .token_definitions
                        .get(correlation.source.index())
                })
                .map_or_else(
                    || format!("Fixed{}", kind.id.index()),
                    |definition| definition.name.clone(),
                ),
            TokenKindIdentity::ContentBase(_) => lowered
                .lexical_plan
                .parser_targets
                .iter()
                .find(|target| target.base_kind == kind.id)
                .map_or_else(
                    || format!("Content{}", kind.id.index()),
                    |target| target.name.clone(),
                ),
        };
        let mut candidate = candidate;
        if !used.insert(candidate.clone()) {
            candidate.push_str(&format!("Kind{}", kind.id.index()));
            used.insert(candidate.clone());
        }
        variants.push(emitted_ident(&candidate)?);
    }
    Ok(variants)
}

/// Rejects authored members that collide with generated Node-view names.
pub(super) fn validate_view_collisions(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    cfg: &CargoCfgSnapshot,
) -> Result<(), String> {
    let mut occupied = BTreeMap::<(ModuleId, String), &'static str>::new();
    for inclusion in &resolved.tree.inclusions {
        for name in inclusion.items.iter().filter_map(|item| {
            crate::source_syntax::configure_attributes(
                crate::source_syntax::item_attributes(item),
                cfg,
            )
            .ok()
            .flatten()
            .and_then(|_| item_ident(item))
        }) {
            occupied.insert((inclusion.id, name), "an authored item");
        }
    }
    for import in &resolved.imports {
        if matches!(
            import.target,
            Some(crate::resolution::ImportTarget::External)
        ) {
            continue;
        }
        if let Some(name) = &import.local_name {
            occupied.insert((import.module, name.clone()), "an authored import");
        }
    }
    for node in &resolved.nodes {
        occupied.insert((node.module, node.ident.clone()), "an authored Node");
    }
    for binding in &resolved.token_bindings {
        occupied.insert(
            (binding.module, binding.ident.clone()),
            "a generated content-token binding",
        );
    }

    for node in &resolved.nodes {
        let view_name = format!("{}View", node.ident);
        if let Some(origin) = occupied.get(&(node.module, view_name.clone())) {
            return Err(format!(
                "generated concrete view `{view_name}` collides with {origin} beside Node `{}`",
                node.ident
            ));
        }
        occupied.insert((node.module, view_name), "another generated concrete view");
    }
    for binding in &resolved.token_bindings {
        let view_name = format!("{}View", binding.ident);
        if let Some(origin) = occupied.get(&(binding.module, view_name.clone())) {
            return Err(format!(
                "generated concrete view `{view_name}` collides with {origin} beside content token `{}`",
                binding.ident
            ));
        }
        occupied.insert(
            (binding.module, view_name),
            "another generated concrete view",
        );
    }
    Ok(())
}

/// Rejects authored inherent items that would collide with generated `explorer` entry points.
///
/// # Long-function contract
///
/// Rationale: checks the entire generated Explorer namespace before rendering any adapter.
/// Invariants: every generated type, method, value, and schema name is unique under Rust lookup.
/// Phases: collects authored and generated names, reports collisions, then validates artifacts.
pub(super) fn validate_explorer_collisions(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    cfg: &CargoCfgSnapshot,
    nodes: &[NodeParsePlan],
    explorer: &ExplorerPlan,
) -> Result<(), String> {
    let mut occupied = BTreeSet::<(ModuleId, String)>::new();
    for inclusion in &resolved.tree.inclusions {
        for item in &inclusion.items {
            if crate::source_syntax::configure_attributes(
                crate::source_syntax::item_attributes(item),
                cfg,
            )
            .ok()
            .flatten()
            .is_none()
            {
                continue;
            }
            if let Some(name) = value_item_ident(item) {
                occupied.insert((inclusion.id, name));
            }
        }
    }
    for import in &resolved.imports {
        if let Some(name) = &import.local_name {
            occupied.insert((import.module, name.clone()));
        }
    }

    let root_module = resolved
        .tree
        .inclusions
        .iter()
        .find(|inclusion| inclusion.parent.is_none())
        .map(|inclusion| inclusion.id)
        .ok_or_else(|| "the explorer plan lacks a selected root module".to_owned())?;
    let mut reserve = |module: ModuleId, name: String| -> Result<(), String> {
        if occupied.contains(&(module, name.clone())) {
            return Err(format!(
                "generated explorer item `{name}` collides with an authored value in its module"
            ));
        }
        if !occupied.insert((module, name.clone())) {
            return Err(format!(
                "generated explorer item `{name}` collides with another generated explorer item"
            ));
        }
        Ok(())
    };

    reserve(root_module, explorer_lex_function().to_string())?;
    reserve(root_module, explorer_node_schemas_static().to_string())?;
    reserve(root_module, explorer_token_schemas_static().to_string())?;
    for token in &explorer.tokens {
        reserve(
            root_module,
            explorer_token_schema_static(token.kind_index).to_string(),
        )?;
    }
    for explorer_node in &explorer.nodes {
        let (_, application) = explorer_application_parse_plan(nodes, explorer_node.application);
        let parse_index = application.parse_index;
        reserve(
            root_module,
            explorer_schema_static(parse_index, None).to_string(),
        )?;
        let variants = match &explorer_node.body {
            ExplorerNodeBodyPlan::Struct(variant) => vec![(None, variant.elements.len())],
            ExplorerNodeBodyPlan::Enum(variants) | ExplorerNodeBodyPlan::Pratt(variants) => {
                variants
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| (Some(index), variant.elements.len()))
                    .collect()
            }
        };
        for (variant, element_count) in variants {
            if let Some(variant) = variant {
                reserve(
                    root_module,
                    explorer_schema_static(parse_index, Some(variant)).to_string(),
                )?;
            }
            for element in 0..element_count {
                reserve(
                    root_module,
                    explorer_element_schema_static(parse_index, variant, element).to_string(),
                )?;
            }
        }
    }
    let mut checked_root_nodes = BTreeSet::new();
    for root in &explorer.roots {
        let (node, application) = explorer_application_parse_plan(nodes, root.application);
        reserve(
            root_module,
            explorer_grammar_schema_static(application.parse_index).to_string(),
        )?;
        if root.capabilities.arbitrary {
            reserve(
                node.module,
                explorer_sample_function(application.parse_index).to_string(),
            )?;
        }
        reserve(
            node.module,
            explorer_parse_function(application.parse_index).to_string(),
        )?;
        reserve(
            node.module,
            explorer_explore_function(application.parse_index).to_string(),
        )?;
        if checked_root_nodes.insert(root.node)
            && authored_inherent_method(resolved, cfg, node.module, &node.ident, "explorer")
        {
            return Err(format!(
                "generated explorer entry method `{}::explorer` collides with an authored inherent method",
                node.ident
            ));
        }
    }
    Ok(())
}

/// Resolves an Explorer application identity to its Node and parse plan.
pub(super) fn explorer_application_parse_plan(
    nodes: &[NodeParsePlan],
    application: MaterializedApplicationId,
) -> (&NodeParsePlan, &NodeApplicationParsePlan) {
    nodes
        .iter()
        .find_map(|node| {
            node.parse_applications
                .iter()
                .find(|candidate| candidate.application == application)
                .map(|application| (node, application))
        })
        .expect("typed explorer plan retains its materialized application")
}

/// Returns the identifier declared by a value-namespace source item.
pub(super) fn value_item_ident(item: &syn::Item) -> Option<String> {
    let ident = match item {
        syn::Item::Const(item) => &item.ident,
        syn::Item::Fn(item) => &item.sig.ident,
        syn::Item::Static(item) => &item.ident,
        syn::Item::Struct(item) if !matches!(item.fields, syn::Fields::Named(_)) => &item.ident,
        _ => return None,
    };
    Some(ident.unraw().to_string())
}

/// Extracts the name and location of an authored inherent method.
pub(super) fn authored_inherent_method(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    cfg: &CargoCfgSnapshot,
    module: ModuleId,
    node: &Ident,
    method: &str,
) -> bool {
    resolved.tree.inclusions.iter().any(|inclusion| {
        inclusion.items.iter().any(|item| {
            authored_item_has_inherent_method(
                resolved,
                cfg,
                inclusion.id,
                item,
                module,
                node,
                method,
            )
        })
    })
}

/// Recursively checks an authored item for a colliding inherent method.
pub(super) fn authored_item_has_inherent_method(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    cfg: &CargoCfgSnapshot,
    impl_module: ModuleId,
    item: &syn::Item,
    node_module: ModuleId,
    node: &Ident,
    method: &str,
) -> bool {
    match item {
        syn::Item::Impl(item)
            if crate::source_syntax::configure_attributes(&item.attrs, cfg)
                .ok()
                .flatten()
                .is_some() =>
        {
            let Type::Path(self_ty) = item.self_ty.as_ref() else {
                return false;
            };
            (inherent_self_targets_node(
                resolved,
                impl_module,
                &self_ty.path,
                node_module,
                node,
            ) || self_ty
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == *node))
                && item.items.iter().any(|item| {
                    matches!(item, syn::ImplItem::Fn(function) if function.sig.ident == method)
                })
        }
        syn::Item::Mod(item) => item.content.as_ref().is_some_and(|(_, items)| {
            items.iter().any(|item| {
                authored_item_has_inherent_method(
                    resolved,
                    cfg,
                    impl_module,
                    item,
                    node_module,
                    node,
                    method,
                )
            })
        }),
        _ => false,
    }
}

/// Reports whether an inherent `impl` self type resolves to the target Node.
pub(super) fn inherent_self_targets_node(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    impl_module: ModuleId,
    path: &syn::Path,
    node_module: ModuleId,
    node: &Ident,
) -> bool {
    let target = resolved
        .nodes
        .iter()
        .find(|candidate| candidate.module == node_module && node.unraw() == candidate.ident);
    let Some(target) = target else {
        return false;
    };
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>();
    if segments.len() == 1
        && target.module == impl_module
        && segments
            .first()
            .is_some_and(|segment| segment == &target.ident)
    {
        return true;
    }
    if segments.len() == 1
        && resolved.imports.iter().any(|import| {
            import.module == impl_module
                && import.local_name.as_deref() == segments.first().map(String::as_str)
                && import.target == Some(crate::resolution::ImportTarget::Node(target.id))
        })
    {
        return true;
    }

    let inclusion = resolved
        .tree
        .inclusions
        .iter()
        .find(|inclusion| inclusion.id == impl_module)
        .expect("an authored impl belongs to a selected module");
    let mut resolved_path = inclusion.logical_path.clone();
    let mut remaining = segments.as_slice();
    if remaining.first().is_some_and(|segment| segment == "crate") {
        resolved_path.clear();
        remaining = &remaining[1..];
        let mut anchor = resolved.build_inputs.module_anchor.segments.as_slice();
        if anchor.first().is_some_and(|segment| segment == "crate") {
            anchor = &anchor[1..];
        }
        if remaining.starts_with(anchor) {
            remaining = &remaining[anchor.len()..];
        }
    } else if remaining.first().is_some_and(|segment| segment == "self") {
        remaining = &remaining[1..];
    } else {
        while remaining.first().is_some_and(|segment| segment == "super") {
            resolved_path.pop();
            remaining = &remaining[1..];
        }
    }
    resolved_path.extend(remaining.iter().cloned());
    let root_path = resolved
        .tree
        .inclusions
        .iter()
        .find(|inclusion| inclusion.parent.is_none())
        .map(|inclusion| inclusion.logical_path.as_slice())
        .unwrap_or_default();
    if resolved_path.starts_with(root_path) {
        resolved_path.drain(..root_path.len());
    }
    resolved_path == target.key.segments
}

/// Returns the declared identifier of a source item when it occupies a named namespace.
pub(super) fn item_ident(item: &syn::Item) -> Option<String> {
    let ident = match item {
        syn::Item::Enum(item) => &item.ident,
        syn::Item::ExternCrate(item) => &item.ident,
        syn::Item::Mod(item) => &item.ident,
        syn::Item::Struct(item) => &item.ident,
        syn::Item::Trait(item) => &item.ident,
        syn::Item::TraitAlias(item) => &item.ident,
        syn::Item::Type(item) => &item.ident,
        syn::Item::Union(item) => &item.ident,
        _ => return None,
    };
    Some(ident.unraw().to_string())
}

/// Renders the grammar-erased lexer callback and exact token/error projections.
pub(super) fn render_explorer_lex(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let explorer_lex = explorer_lex_function();
    let records = plan
        .kind_variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let schema = explorer_token_schema_static(
                u16::try_from(index).expect("generated token kind fits explorer width"),
            );
            quote!(TokenKind::#variant => &#schema,)
        });
    quote! {
        __recursa_if_explorer! {
            fn #explorer_lex(
                source: &str,
            ) -> #facade::explorer::LexResult<'_> {
                let lexed = lex(source);
                let tokens = lexed.tokens().map(|token| {
                    let schema = match token.kind() {
                        #(#records)*
                        TokenKind::LexicalError => unreachable!(
                            "lexical errors are not explorer token records"
                        ),
                    };
                    #facade::__private::explorer_lex_record(
                        schema,
                        token.span(),
                    )
                });
                let errors = lexed.errors().map(|error| error.span());
                #facade::__private::explorer_lex_result(source, tokens, errors)
            }
        }
    }
}

/// Renders dynamic occurrence support for one generated content-token type.
pub(super) fn render_token_explorer(
    plan: &EmissionPlan,
    token: &TokenParsePlan,
) -> proc_macro2::TokenStream {
    let Some(explorer_plan) = &plan.explorer else {
        return quote!();
    };
    let facade = &plan.facade;
    let ident = &token.ident;
    let explorer = explorer_plan
        .tokens
        .iter()
        .find(|candidate| candidate.semantic == Some(token.semantic))
        .expect("typed explorer plan retains every content token");
    let schema = explorer_token_schema_static(explorer.kind_index);
    match explorer.display {
        ExplorerDisplayPlan::MatchedOrStored => {}
        ExplorerDisplayPlan::None | ExplorerDisplayPlan::Fixed(_) => {
            unreachable!("content-token explorer display is frozen as matched-or-stored")
        }
    }
    quote! {
        __recursa_if_explorer! {
            unsafe impl<'input> #facade::__private::ExplorerNode for #ident<'input> {
                fn __recursa_explorer_node<'document>(
                    &'document self,
                    cursor: #facade::__private::ViewCursor<'document>,
                ) -> #facade::__private::ExplorerNodeData<'document> {
                    #facade::__private::explorer_token_node(
                        self,
                        &super::#schema,
                        cursor,
                        self.text(),
                    )
                }

                fn __recursa_explorer_child_count(&self, _element: usize) -> usize {
                    0
                }

                fn __recursa_explorer_child<'document>(
                    &'document self,
                    _cursor: #facade::__private::ViewCursor<'document>,
                    _element: usize,
                    _child: usize,
                ) -> Option<#facade::__private::ExplorerNodeData<'document>> {
                    None
                }
            }
        }
    }
}

/// Prepares Explorer implementation generics without introducing recursive solver bounds.
pub(super) fn explorer_impl_generics(
    _plan: &EmissionPlan,
    application: &NodeApplicationParsePlan,
) -> Generics {
    let mut generics = Generics::default();
    let mut input_lifetime = InputLifetimeUse::default();
    syn::visit::Visit::visit_type(&mut input_lifetime, &application.ty);
    if input_lifetime.found {
        generics.params.push(syn::parse_quote!('__recursa_input));
    }
    generics
}

/// Records whether a materialized application type refers to the generated input lifetime.
#[derive(Default)]
struct InputLifetimeUse {
    /// Whether `__recursa_input` occurs in the visited type.
    found: bool,
}

impl<'ast> syn::visit::Visit<'ast> for InputLifetimeUse {
    fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
        if lifetime.ident == "__recursa_input" {
            self.found = true;
        }
    }
}

/// Binds semantic fields needed to enumerate one runtime variant's occurrences.
pub(super) fn explorer_variant_pattern_and_bindings(
    node_ident: &Ident,
    variant: &VariantParsePlan,
) -> (proc_macro2::TokenStream, Vec<Option<Ident>>) {
    let mut bindings = vec![None; variant.elements.len()];
    let semantic = variant
        .elements
        .iter()
        .enumerate()
        .filter_map(|(element_index, element)| match &element.kind {
            ElementParseKind::Semantic { field, .. } => {
                let binding = format_ident!("__recursa_explorer_field_{}", field.index);
                bindings[element_index] = Some(binding.clone());
                Some((field, binding))
            }
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .collect::<Vec<_>>();
    let variant_ident = &variant.ident;
    let pattern = match variant.style {
        ResolvedFieldStyle::Named => {
            let fields = semantic.iter().map(|(field, binding)| {
                let ident = field.ident.as_ref().expect("named fields have identifiers");
                quote!(#ident: #binding)
            });
            quote!(#node_ident::#variant_ident { #(#fields),*, .. })
        }
        ResolvedFieldStyle::Unnamed => {
            let count = semantic
                .iter()
                .map(|(field, _)| field.index + 1)
                .max()
                .unwrap_or(0);
            let mut fields = (0..count).map(|_| quote!(_)).collect::<Vec<_>>();
            for (field, binding) in &semantic {
                fields[field.index] = quote!(#binding);
            }
            quote!(#node_ident::#variant_ident(#(#fields),*))
        }
        ResolvedFieldStyle::Unit => quote!(#node_ident::#variant_ident),
    };
    (pattern, bindings)
}

/// Renders occurrence access and eligible root adapters for one Node.
pub(super) fn render_node_explorer(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    _node_index: usize,
) -> proc_macro2::TokenStream {
    let Some(explorer_plan) = &plan.explorer else {
        return quote!();
    };
    let applications = node.parse_applications.iter().map(|application| {
        render_node_application_explorer(plan, explorer_plan, node, application)
    });
    quote!(#(#applications)*)
}

/// Renders one materialized application's schema, parse, Pretty, and sample callbacks.
///
/// # Long-function contract
///
/// Rationale: keeps an application adapter's schema, occurrence traversal, parsing, Pretty, and sampling in one emitted impl.
/// Invariants: the erased API never exposes semantic Rust types or outlives its input.
/// Phases: renders identity and schema, occurrence access, parse entry, then optional capabilities.
pub(super) fn render_node_application_explorer(
    plan: &EmissionPlan,
    explorer_plan: &ExplorerPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let node_ident = &node.ident;
    let node_index = application.parse_index;
    let ty = &application.ty;
    let static_ty = &application.static_ty;
    let explorer_node = explorer_plan
        .nodes
        .iter()
        .find(|candidate| {
            candidate.node == node.node && candidate.application == application.application
        })
        .expect("typed explorer plan retains every materialized Node application");
    let explorer_generics = explorer_impl_generics(plan, application);
    let (impl_generics, _, where_clause) = explorer_generics.split_for_impl();
    let (entry_impl_generics, _, entry_where_clause) = explorer_generics.split_for_impl();
    let (node_body, child_count_body, child_body) = match (&application.body, &explorer_node.body) {
        (NodeBodyPlan::Struct { elements, .. }, ExplorerNodeBodyPlan::Struct(explorer)) => {
            let schema = explorer_schema_static(node_index, None);
            let bindings = elements
                .iter()
                .map(|element| match &element.kind {
                    ElementParseKind::Fixed { .. } => None,
                    ElementParseKind::OptionalFixedTokenGroup { .. } => None,
                    ElementParseKind::Semantic { field, .. } => Some(field_access(
                        quote!(&self),
                        field.ident.as_ref(),
                        field.index,
                    )),
                })
                .collect::<Vec<_>>();
            let count = render_explorer_variant_child_count(explorer, elements, &bindings);
            let child = render_explorer_variant_child(facade, explorer, elements, &bindings);
            (
                quote!(#facade::__private::explorer_node(self, &#module_anchor::#schema, None, cursor)),
                count,
                child,
            )
        }
        (NodeBodyPlan::Enum { variants, .. }, ExplorerNodeBodyPlan::Enum(explorer_variants)) => {
            let base = explorer_schema_static(node_index, None);
            let mut node_arms = Vec::new();
            let mut count_arms = Vec::new();
            let mut child_arms = Vec::new();
            for (variant_index, explorer) in explorer_variants.iter().enumerate() {
                let variant = variants
                    .iter()
                    .find(|variant| Some(variant.resolved_variant) == explorer.variant)
                    .expect("typed explorer enum variant retains its parse shape");
                let (pattern, bindings) =
                    explorer_variant_pattern_and_bindings(node_ident, variant);
                let bindings = bindings
                    .into_iter()
                    .map(|binding| binding.map(|binding| quote!(#binding)))
                    .collect::<Vec<_>>();
                let active = explorer_schema_static(node_index, Some(variant_index));
                let count =
                    render_explorer_variant_child_count(explorer, &variant.elements, &bindings);
                let child =
                    render_explorer_variant_child(facade, explorer, &variant.elements, &bindings);
                node_arms.push(quote!(#pattern => #facade::__private::explorer_node(self, &#module_anchor::#base, Some(&#module_anchor::#active), cursor),));
                count_arms.push(quote!(#pattern => #count,));
                child_arms.push(quote!(#pattern => #child,));
            }
            (
                quote!(match self { #(#node_arms)* }),
                quote!(match self { #(#count_arms)* }),
                quote!(match self { #(#child_arms)* }),
            )
        }
        (NodeBodyPlan::Pratt(pratt), ExplorerNodeBodyPlan::Pratt(explorer_variants)) => {
            let base = explorer_schema_static(node_index, None);
            let mut node_arms = Vec::new();
            let mut count_arms = Vec::new();
            let mut child_arms = Vec::new();
            for (variant_index, explorer) in explorer_variants.iter().enumerate() {
                let variant = if variant_index < pratt.nud.len() {
                    pratt
                        .nud
                        .iter()
                        .map(|branch| branch.variant.clone())
                        .find(|variant| Some(variant.resolved_variant) == explorer.variant)
                        .expect("typed explorer Pratt nud retains its parse shape")
                } else {
                    let branch = pratt
                        .led
                        .iter()
                        .find(|branch| Some(branch.variant.resolved_variant) == explorer.variant)
                        .expect("typed explorer Pratt led retains its parse shape");
                    let mut variant = branch.variant.clone();
                    variant.elements.insert(0, branch.lhs.clone());
                    variant
                };
                let (pattern, bindings) =
                    explorer_variant_pattern_and_bindings(node_ident, &variant);
                let bindings = bindings
                    .into_iter()
                    .map(|binding| binding.map(|binding| quote!(#binding)))
                    .collect::<Vec<_>>();
                let active = explorer_schema_static(node_index, Some(variant_index));
                let count =
                    render_explorer_variant_child_count(explorer, &variant.elements, &bindings);
                let child =
                    render_explorer_variant_child(facade, explorer, &variant.elements, &bindings);
                node_arms.push(quote!(#pattern => #facade::__private::explorer_node(self, &#module_anchor::#base, Some(&#module_anchor::#active), cursor),));
                count_arms.push(quote!(#pattern => #count,));
                child_arms.push(quote!(#pattern => #child,));
            }
            (
                quote!(match self { #(#node_arms)* }),
                quote!(match self { #(#count_arms)* }),
                quote!(match self { #(#child_arms)* }),
            )
        }
        _ => unreachable!("typed explorer and parse Node bodies have identical shapes"),
    };

    let root = explorer_plan
        .roots
        .iter()
        .find(|root| root.node == node.node && root.application == application.application);
    let entry = root.map(|root| {
        let grammar_schema = explorer_grammar_schema_static(node_index);
        let sample = explorer_sample_function(node_index);
        let parse = explorer_parse_function(node_index);
        let explore = explorer_explore_function(node_index);
        let recovery_callback = if explorer_recovery_available(plan, application.application) {
            quote!(Some(#explore))
        } else {
            quote!(None)
        };
        let recovery_plan = explorer_recovery_plan_static(node_index);
        let explorer_lex = explorer_lex_function();
        let pretty = root.capabilities.pretty;
        let pretty_body = if pretty {
            quote! {
                Some(#facade::__private::explorer_pretty_output(
                    #facade::Pretty::render_mapped_with(parsed, *config),
                ))
            }
        } else {
            quote! {
                let _ = (parsed, config);
                None
            }
        };
        let arbitrary = root.capabilities.arbitrary;
        let framing_method = plan
            .framing
            .as_ref()
            .filter(|framing| {
                plan.nodes[framing.node_index].node == node.node
                    && plan.nodes[framing.node_index].parse_applications[framing.application_index]
                        .application
                        == application.application
            })
            .map(|_| {
                let method_generics = if explorer_generics.params.is_empty() {
                    quote!(<'__recursa_input>)
                } else {
                    quote!()
                };
                quote! {
                    /// Partitions exact source into strict typed islands or one
                    /// wholly construction-free recovery projection.
                    pub fn frame #method_generics(
                        source: &'__recursa_input str,
                    ) -> Result<
                        #facade::framing::FrameResult<'__recursa_input, Self>,
                        #facade::framing::FrameError<'__recursa_input>,
                    > {
                        #facade::__private::frame_document::<Self>(
                            source,
                            #module_anchor::__recursa_lexical::__recursa_frame_lex,
                            &#module_anchor::__RECURSA_FRAMING_PLAN,
                        )
                    }
                }
            });
        let sample_function = arbitrary.then(|| {
            quote! {
                __recursa_if_arbitrary! {
                    fn #sample(
                        bytes: &[u8],
                    ) -> Result<
                        #facade::explorer::ExplorerDocument<'static>,
                        #facade::explorer::SampleError,
                    > {
                        let mut input = #facade::__private::arbitrary::Unstructured::new(&bytes);
                        let value = <#static_ty as #facade::Arbitrary>::arbitrary(&mut input)
                            .map_err(|error| #facade::__private::explorer_sample_error(
                                error.to_string(),
                            ))?;
                        let parsed = #facade::Parsed::synthetic(value);
                        Ok(#facade::explorer::ExplorerDocument::from_parsed(parsed))
                    }
                }
            }
        });
        let arbitrary_binding = if arbitrary {
            quote! {
                type __RecursaExplorerSampleCallback = fn(&[u8]) -> Result<
                    #facade::explorer::ExplorerDocument<'static>,
                    #facade::explorer::SampleError,
                >;
                let (arbitrary, sample): (
                    bool,
                    Option<__RecursaExplorerSampleCallback>,
                ) = __recursa_explorer_sample!(#sample);
            }
        } else {
            quote! {
                let (arbitrary, sample) = (false, None);
            }
        };
        quote! {
            fn #parse<'__recursa_input>(
                source: &'__recursa_input str,
            ) -> Result<
                #facade::explorer::ExplorerDocument<'__recursa_input>,
                #facade::explorer::ParseFailure<'__recursa_input>,
            > {
                let lexed = #module_anchor::lex(source);
                let lexical_errors = lexed.errors().map(|error| error.span());
                let mut input = lexed.input();
                #facade::__private::explorer_parse_document::<#ty, _>(
                    &mut input.inner,
                    lexical_errors,
                )
            }

            fn #explore<'__recursa_input>(
                source: &'__recursa_input str,
            ) -> Result<
                #facade::explorer::Exploration<'__recursa_input>,
                #facade::explorer::ExplorationError<'__recursa_input>,
            > {
                let lexed = #module_anchor::lex(source);
                let lexical_errors = lexed.errors().map(|error| error.span()).collect::<Vec<_>>();
                let mut strict_input = lexed.input();
                match #facade::__private::explorer_parse_document::<#ty, _>(
                    &mut strict_input.inner,
                    lexical_errors,
                ) {
                    Ok(document) => Ok(#facade::explorer::Exploration::Complete(document)),
                    Err(failure) => {
                        let mut recovery_input = lexed.input();
                        #facade::__private::explorer_recover_document(
                            &mut recovery_input.inner,
                            failure,
                            &#module_anchor::#recovery_plan,
                        )
                        .map(#facade::explorer::Exploration::Recovered)
                    }
                }
            }

            #sample_function

            unsafe impl #entry_impl_generics #facade::__private::ExplorerRoot
                for #ty #entry_where_clause
            {
                fn __recursa_explorer_schema(
                ) -> &'static #facade::__private::ExplorerGrammarSchemaData {
                    &#module_anchor::#grammar_schema
                }

                fn __recursa_explorer_pretty(
                    parsed: &#facade::Parsed<'_, Self>,
                    config: &#facade::PrettyConfig,
                ) -> Option<#facade::explorer::PrettyOutput> {
                    #pretty_body
                }
            }

            impl #entry_impl_generics #ty #entry_where_clause {
                /// Returns this source-visible Node family's opaque explorer adapter.
                ///
                /// Explorer occurrence projection is independent of the optional `Visit`
                /// traversal implementation.
                pub fn explorer() -> #facade::explorer::Adapter {
                    #arbitrary_binding
                    unsafe {
                        #facade::__private::explorer_adapter(
                            &#module_anchor::#grammar_schema,
                            #pretty,
                            arbitrary,
                            #module_anchor::#explorer_lex,
                            #parse,
                            #recovery_callback,
                            sample,
                        )
                    }
                }

                #framing_method
            }
        }
    });

    quote! {
        __recursa_if_explorer! {
            unsafe impl #impl_generics #facade::__private::ExplorerNode
                for #ty #where_clause
            {
                fn __recursa_explorer_node<'document>(
                    &'document self,
                    cursor: #facade::__private::ViewCursor<'document>,
                ) -> #facade::__private::ExplorerNodeData<'document> {
                    #node_body
                }

                fn __recursa_explorer_child_count(&self, element: usize) -> usize {
                    #child_count_body
                }

                fn __recursa_explorer_child<'document>(
                    &'document self,
                    cursor: #facade::__private::ViewCursor<'document>,
                    element: usize,
                    child: usize,
                ) -> Option<#facade::__private::ExplorerNodeData<'document>> {
                    let _ = (&cursor, child);
                    #child_body
                }
            }

            #entry
        }
    }
}

/// Reports whether every Node reachable from one root has a generated recovery body.
fn explorer_recovery_available(plan: &EmissionPlan, root: MaterializedApplicationId) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(application) = pending.pop() {
        if !visited.insert(application) {
            continue;
        }
        let (_, application) = explorer_application_parse_plan(&plan.nodes, application);
        let element_groups = match &application.body {
            NodeBodyPlan::Struct { elements, .. } => vec![elements.as_slice()],
            NodeBodyPlan::Enum { variants, dispatch } => {
                // A balanced-delimiter cut selects structurally rather than
                // through the bounded trie; its recovery interpreter is not
                // part of this slice.
                if dispatch.balanced.is_some() {
                    return false;
                }
                variants
                    .iter()
                    .map(|variant| variant.elements.as_slice())
                    .collect()
            }
            NodeBodyPlan::Pratt(pratt) => {
                // Pratt recovery interprets the bounded trie only; a balanced
                // cut inside either selector keeps the same exclusion.
                if pratt.nud_dispatch.balanced.is_some()
                    || pratt
                        .led_dispatch
                        .as_ref()
                        .is_some_and(|dispatch| dispatch.balanced.is_some())
                {
                    return false;
                }
                pratt
                    .nud
                    .iter()
                    .map(|variant| variant.variant.elements.as_slice())
                    .chain(
                        pratt
                            .led
                            .iter()
                            .map(|variant| variant.variant.elements.as_slice()),
                    )
                    .collect()
            }
        };
        for element in element_groups.into_iter().flatten() {
            let ElementParseKind::Semantic { projection, .. } = &element.kind else {
                continue;
            };
            let SemanticParseProjection::Direct {
                explorer_terminal,
                container,
                attachment,
                ..
            } = projection.as_ref()
            else {
                continue;
            };
            // A sparse consume-vs-skip viability decision is a frozen product
            // this slice cannot interpret yet; recovery must not fall back to
            // the raw selector where the frozen plan refines the choice.
            let container = attachment
                .as_ref()
                .map_or(container.as_ref(), |attachment| {
                    attachment.semantic.as_ref()
                });
            if matches!(
                container,
                PlannedContainer::Optional {
                    decision: Some(_),
                    ..
                }
            ) {
                return false;
            }
            if let ExplorerTerminalPlan::Node(child) = explorer_terminal {
                pending.push(*child);
            }
        }
    }
    true
}

/// Names the static schema emitted for one Node application and optional variant.
pub(super) fn explorer_schema_static(node_index: usize, variant: Option<usize>) -> Ident {
    match variant {
        Some(variant) => {
            format_ident!("__RECURSA_EXPLORER_SCHEMA_{node_index}_{variant}")
        }
        None => format_ident!("__RECURSA_EXPLORER_SCHEMA_{node_index}"),
    }
}

/// Names the single grammar-erased lexer callback.
pub(super) fn explorer_lex_function() -> Ident {
    format_ident!("__recursa_explorer_lex")
}

/// Returns the private parse callback name for one materialized root.
pub(super) fn explorer_parse_function(node_index: usize) -> Ident {
    format_ident!("__recursa_explorer_parse_{node_index}")
}

/// Returns the generated construction-free exploration callback name.
pub(super) fn explorer_explore_function(node_index: usize) -> Ident {
    format_ident!("__recursa_explorer_explore_{node_index}")
}

/// Names a root application's deterministic sample callback.
pub(super) fn explorer_sample_function(node_index: usize) -> Ident {
    format_ident!("__recursa_explorer_sample_{node_index}")
}

/// Names the static schema for one token kind.
pub(super) fn explorer_token_schema_static(kind_index: u16) -> Ident {
    format_ident!("__RECURSA_EXPLORER_TOKEN_SCHEMA_{kind_index}")
}

/// Names the aggregate static grammar schema for a root application.
pub(super) fn explorer_grammar_schema_static(node_index: usize) -> Ident {
    format_ident!("__RECURSA_EXPLORER_GRAMMAR_SCHEMA_{node_index}")
}

/// Names the ordered static Node-schema table.
pub(super) fn explorer_node_schemas_static() -> Ident {
    format_ident!("__RECURSA_EXPLORER_NODE_SCHEMAS")
}

/// Names the ordered static token-schema table.
pub(super) fn explorer_token_schemas_static() -> Ident {
    format_ident!("__RECURSA_EXPLORER_TOKEN_SCHEMAS")
}

/// Renders token descriptors in grammar-local kind order.
pub(super) fn render_explorer_token_schemas(
    plan: &EmissionPlan,
    explorer: &ExplorerPlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let definitions = explorer.tokens.iter().map(|token| {
        let schema = explorer_token_schema_static(token.kind_index);
        let name = &token.name;
        let qualified_name = &token.qualified_name;
        let kind_name = &token.kind_name;
        let canonical_text = token
            .canonical_text
            .as_ref()
            .map_or_else(|| quote!(None), |text| quote!(Some(#text)));
        let highlight = match token.highlight {
            ExplorerHighlightClassPlan::Keyword => {
                quote!(#facade::explorer::HighlightClass::Keyword)
            }
            ExplorerHighlightClassPlan::Punctuation => {
                quote!(#facade::explorer::HighlightClass::Punctuation)
            }
            ExplorerHighlightClassPlan::Content => {
                quote!(#facade::explorer::HighlightClass::Content)
            }
        };
        quote! {
            pub(crate) static #schema: #facade::__private::ExplorerTokenSchemaData =
                #facade::__private::explorer_token_schema(
                    #name,
                    #qualified_name,
                    #kind_name,
                    #canonical_text,
                    #highlight,
                );
        }
    });
    quote!(#(#definitions)*)
}

/// Names one generated construction-free recovery Node recipe.
pub(super) fn explorer_recovery_node_static(node_index: usize) -> Ident {
    format_ident!("__RECURSA_EXPLORER_RECOVERY_NODE_{node_index}")
}

/// Names one generated root recovery plan.
pub(super) fn explorer_recovery_plan_static(node_index: usize) -> Ident {
    format_ident!("__RECURSA_EXPLORER_RECOVERY_PLAN_{node_index}")
}

/// Emits one pooled `&'static FollowSet` place for construction-free recovery data.
///
/// Every recovery expectation reuses the validated grammar-wide expectation
/// pool; recovery introduces no expectation storage of its own. The only
/// unpooled shape is the canonical empty expectation of a variant-free enum,
/// which reuses the shared empty FOLLOW static.
fn recovery_expected_ref(plan: &EmissionPlan, expected: &ExpectedSet) -> proc_macro2::TokenStream {
    let module_anchor = &plan.module_anchor;
    if let Some(index) = plan.expected_sets.id(expected) {
        quote!(&#module_anchor::__RECURSA_EXPECTED_SETS[#index])
    } else {
        assert!(
            expected.is_empty(),
            "a non-empty recovery expectation is missing from the grammar-wide pool",
        );
        quote!(&#module_anchor::__RECURSA_EMPTY_FOLLOW)
    }
}

/// Renders one frozen FOLLOW policy for the construction-free interpreter.
///
/// Pratt stop tokens carried by a caller-relative FOLLOW are static stop
/// evidence for recovery synchronization, exactly as they are for strict
/// failure reporting.
fn render_recovery_follow(plan: &EmissionPlan, follow: &PlannedFollow) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    match follow {
        PlannedFollow::Caller => {
            quote!(#facade::__private::explorer_recovery_caller_follow())
        }
        PlannedFollow::Static(expected) => {
            let expected = recovery_expected_ref(plan, expected);
            quote!(#facade::__private::explorer_recovery_static_follow(#expected))
        }
        PlannedFollow::StaticAndCaller(expected) | PlannedFollow::CallerAndPrattStops(expected) => {
            let expected = recovery_expected_ref(plan, expected);
            quote!(#facade::__private::explorer_recovery_static_and_caller_follow(#expected))
        }
    }
}

/// Renders one immutable fixed-token sequence from frozen expected sets.
fn render_recovery_fixed_sequence(
    plan: &EmissionPlan,
    prefix: &str,
    tokens: &[AttachedFixedParsePlan],
) -> (proc_macro2::TokenStream, Ident) {
    let facade = &plan.facade;
    let values = tokens
        .iter()
        .map(|token| {
            let expected = recovery_expected_ref(plan, &token.expected);
            quote!(#facade::__private::explorer_recovery_fixed_plan(#expected))
        })
        .collect::<Vec<_>>();
    let sequence = format_ident!("{prefix}_SEQUENCE");
    let count = values.len();
    let definition = quote! {
        static #sequence: [#facade::__private::ExplorerRecoveryFixedPlanData; #count] =
            [#(#values),*];
    };
    (definition, sequence)
}

/// Renders required and atomic-optional syntax surrounding one presence site.
fn render_recovery_fixed_syntax(
    plan: &EmissionPlan,
    prefix: &str,
    syntax: &[AttachedFixedSyntaxParsePlan],
) -> (proc_macro2::TokenStream, Ident) {
    let facade = &plan.facade;
    let mut definitions = Vec::new();
    let mut values = Vec::new();
    for (index, operation) in syntax.iter().enumerate() {
        match operation {
            AttachedFixedSyntaxParsePlan::Required(token) => {
                let expected = recovery_expected_ref(plan, &token.expected);
                values.push(quote!(
                    #facade::__private::explorer_recovery_required_fixed_syntax(#expected)
                ));
            }
            AttachedFixedSyntaxParsePlan::Optional(tokens) => {
                let nested_prefix = format!("{prefix}_OPTIONAL_{index}");
                let (nested_definitions, sequence) =
                    render_recovery_fixed_sequence(plan, &nested_prefix, tokens);
                definitions.push(nested_definitions);
                values.push(quote!(
                    #facade::__private::explorer_recovery_optional_fixed_syntax(&#sequence)
                ));
            }
        }
    }
    let operations = format_ident!("{prefix}_OPERATIONS");
    let count = values.len();
    definitions.push(quote! {
        static #operations:
            [#facade::__private::ExplorerRecoveryFixedSyntaxPlanData; #count] =
            [#(#values),*];
    });
    (quote!(#(#definitions)*), operations)
}

/// Renders one construction-free element recipe from already-frozen parse products.
fn render_recovery_element_plan(
    plan: &EmissionPlan,
    explorer: &ExplorerPlan,
    node_index: usize,
    variant: Option<usize>,
    element_index: usize,
    element: &ElementParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let schema = explorer_element_schema_static(node_index, variant, element_index);
    let stop = render_recovery_follow(
        plan,
        element
            .recovery_stop
            .as_ref()
            .expect("materialized recovery elements retain their frozen FOLLOW"),
    );
    let (kind_definitions, kind) = match &element.kind {
        ElementParseKind::Fixed { expected, .. } => {
            let expected = recovery_expected_ref(plan, expected);
            (
                quote!(),
                quote!(#facade::__private::explorer_recovery_fixed_kind(#expected)),
            )
        }
        ElementParseKind::OptionalFixedTokenGroup { tokens, .. } => {
            let prefix = format!(
                "__RECURSA_EXPLORER_RECOVERY_OPTIONAL_FIXED_{node_index}_{}_{}",
                variant.map_or_else(|| "S".to_owned(), |variant| variant.to_string()),
                element_index,
            );
            let (definitions, sequence) = render_recovery_fixed_sequence(plan, &prefix, tokens);
            (
                definitions,
                quote!(#facade::__private::explorer_recovery_optional_fixed_kind(&#sequence)),
            )
        }
        ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
            SemanticParseProjection::FixedPresence {
                before,
                sequence,
                after,
                ..
            } => {
                let prefix = format!(
                    "__RECURSA_EXPLORER_RECOVERY_PRESENCE_{node_index}_{}_{}",
                    variant.map_or_else(|| "S".to_owned(), |variant| variant.to_string()),
                    element_index,
                );
                let (before_definitions, before) =
                    render_recovery_fixed_syntax(plan, &format!("{prefix}_BEFORE"), before);
                let (sequence_definitions, sequence) =
                    render_recovery_fixed_sequence(plan, &format!("{prefix}_VALUE"), sequence);
                let (after_definitions, after) =
                    render_recovery_fixed_syntax(plan, &format!("{prefix}_AFTER"), after);
                (
                    quote!(#before_definitions #sequence_definitions #after_definitions),
                    quote!(
                    #facade::__private::explorer_recovery_fixed_presence_kind(
                        &#before,
                        &#sequence,
                        &#after,
                    )
                    ),
                )
            }
            SemanticParseProjection::Direct {
                explorer_terminal,
                container,
                attachment,
                ..
            } => {
                let terminal = match explorer_terminal {
                    ExplorerTerminalPlan::Node(application) => {
                        let target = explorer_application_parse_plan(&plan.nodes, *application)
                            .1
                            .parse_index;
                        let target = explorer_recovery_node_static(target);
                        quote!(#facade::__private::explorer_recovery_node_terminal(&#target))
                    }
                    ExplorerTerminalPlan::Content(token) => {
                        let target = explorer_content_token_schema_static(explorer, *token);
                        quote!(#facade::__private::explorer_recovery_content_terminal(&#target))
                    }
                };
                let attachment_prefix = format!(
                    "__RECURSA_EXPLORER_RECOVERY_ATTACHMENT_{node_index}_{}_{}",
                    variant.map_or_else(|| "S".to_owned(), |variant| variant.to_string()),
                    element_index,
                );
                let (attachment_definitions, before, after) = attachment.as_ref().map_or_else(
                    || (quote!(), quote!(&[]), quote!(&[])),
                    |attachment| {
                        let (before_definitions, before) = render_recovery_fixed_syntax(
                            plan,
                            &format!("{attachment_prefix}_BEFORE"),
                            &attachment.before,
                        );
                        let (after_definitions, after) = render_recovery_fixed_syntax(
                            plan,
                            &format!("{attachment_prefix}_AFTER"),
                            &attachment.after,
                        );
                        (
                            quote!(#before_definitions #after_definitions),
                            quote!(&#before),
                            quote!(&#after),
                        )
                    },
                );
                let container_plan = attachment
                    .as_ref()
                    .map_or(container.as_ref(), |attachment| {
                        attachment.semantic.as_ref()
                    });
                let (container_definitions, container) = match container_plan {
                    PlannedContainer::Direct {
                        child_follow,
                        required_failure,
                    } => {
                        let restart = render_recovery_follow(plan, required_failure);
                        let child = render_recovery_follow(plan, child_follow);
                        (
                            quote!(),
                            quote!(#facade::__private::explorer_recovery_direct_container(
                                #restart,
                                #child,
                            )),
                        )
                    }
                    // A sparse viability decision keeps the owning root
                    // recovery-unavailable; the rendered fallback selector is
                    // inert until the decision graph gains a recovery
                    // interpreter.
                    PlannedContainer::Optional {
                        selector,
                        decision: _,
                        child_follow,
                    } => {
                        let restart = recovery_expected_ref(plan, selector);
                        let child = render_recovery_follow(plan, child_follow);
                        (
                            quote!(),
                            quote!(#facade::__private::explorer_recovery_optional_container(
                                #restart,
                                #child,
                            )),
                        )
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
                    } => {
                        let element_selector = recovery_expected_ref(plan, element_selector);
                        let separator = separator_selector.as_ref().map_or_else(
                            || quote!(None),
                            |selector| {
                                let selector = recovery_expected_ref(plan, selector);
                                quote!(Some(#selector))
                            },
                        );
                        let repeat_stop = render_recovery_follow(plan, stop);
                        let child = render_recovery_follow(plan, child_follow);
                        let minimum_failure = minimum_failure.as_deref().map_or_else(
                            || quote!(None),
                            |failure| {
                                let value = render_recovery_follow(plan, failure);
                                quote!(Some(#value))
                            },
                        );
                        let continuation = render_recovery_follow(plan, continuation_failure);
                        let after_separator_failure =
                            after_separator_failure.as_deref().map_or_else(
                                || quote!(None),
                                |failure| {
                                    let value = render_recovery_follow(plan, failure);
                                    quote!(Some(#value))
                                },
                            );
                        let (minimum, presence) = match operation {
                            ContainerOperation::ZeroOrMore => (
                                quote!(#facade::__private::ExplorerRecoveryMinimumData::Zero),
                                quote!(#facade::__private::ExplorerRecoveryRepetitionPresenceData::Required),
                            ),
                            ContainerOperation::OneOrMore => (
                                quote!(#facade::__private::ExplorerRecoveryMinimumData::One),
                                quote!(#facade::__private::ExplorerRecoveryRepetitionPresenceData::Required),
                            ),
                            ContainerOperation::OptionalOneOrMore => (
                                quote!(#facade::__private::ExplorerRecoveryMinimumData::One),
                                quote!(#facade::__private::ExplorerRecoveryRepetitionPresenceData::Optional),
                            ),
                            ContainerOperation::Direct | ContainerOperation::Optional => {
                                unreachable!("repeated plan retains a repeated operation")
                            }
                        };
                        let trailing = if *optional_trailing {
                            quote!(#facade::__private::ExplorerRecoveryTrailingSeparatorData::Allowed)
                        } else {
                            quote!(#facade::__private::ExplorerRecoveryTrailingSeparatorData::Forbidden)
                        };
                        (
                            quote!(),
                            quote!(#facade::__private::explorer_recovery_repeated_container(
                                #minimum,
                                #presence,
                                #element_selector,
                                #separator,
                                #trailing,
                                #repeat_stop,
                                #child,
                                #minimum_failure,
                                #continuation,
                                #after_separator_failure,
                            )),
                        )
                    }
                };
                (
                    quote!(#attachment_definitions #container_definitions),
                    quote!(#facade::__private::explorer_recovery_semantic_kind(
                            #terminal,
                            #container,
                            #before,
                            #after,
                    )),
                )
            }
        },
    };
    quote! {{
        #kind_definitions
        #facade::__private::explorer_recovery_element_plan(
            &#schema,
            #stop,
            #kind,
        )
    }}
}

/// Renders one frozen predictive graph into immutable runtime dispatch statics.
///
/// The joined plan stores canonical states in one arena; shared suffix states
/// are emitted once and referenced by every incoming edge, preserving the
/// frozen graph shape without recomputing dispatch meaning.
fn render_recovery_dispatch_graph(
    plan: &EmissionPlan,
    node_index: usize,
    stem: &str,
    dispatch: &PredictiveDispatchPlan,
) -> (proc_macro2::TokenStream, Ident) {
    let facade = &plan.facade;
    let mut definitions = Vec::new();
    for (index, state) in dispatch.states.iter().enumerate() {
        let mut edges = Vec::new();
        for edge in &state.edges {
            let child = format_ident!(
                "__RECURSA_EXPLORER_RECOVERY_{stem}_{node_index}_{}",
                edge.next.index()
            );
            let expected = recovery_expected_ref(plan, &edge.position.expected);
            let text = match &edge.position.text_predicate {
                None | Some(PlannedTextPredicate::Any) => {
                    quote!(#facade::__private::explorer_recovery_text_any())
                }
                Some(PlannedTextPredicate::Exact(text)) => {
                    quote!(#facade::__private::explorer_recovery_text_exact(#text))
                }
                Some(PlannedTextPredicate::OneOf(options)) => {
                    quote!(#facade::__private::explorer_recovery_text_one_of(&[#(#options),*]))
                }
                Some(PlannedTextPredicate::NotIn(options)) => {
                    quote!(#facade::__private::explorer_recovery_text_not_in(&[#(#options),*]))
                }
            };
            edges.push(quote! {
                #facade::__private::explorer_recovery_dispatch_edge(
                    #expected,
                    #text,
                    &#child,
                )
            });
        }
        let edge_count = edges.len();
        let edges_name =
            format_ident!("__RECURSA_EXPLORER_RECOVERY_{stem}_EDGES_{node_index}_{index}");
        let node_name = format_ident!("__RECURSA_EXPLORER_RECOVERY_{stem}_{node_index}_{index}");
        // The runtime selector tries exact edges before terminals, so both
        // immediate and fallback-after-edges terminals select unconditionally
        // once no edge matches; only caller-FOLLOW terminals stay conditional.
        let target = state.target.map_or_else(
            || quote!(None),
            |target| {
                let variant = target.variant;
                let condition = if matches!(target.mode, crate::DispatchTerminalMode::CallerFollow) {
                    quote!(#facade::__private::ExplorerRecoveryDispatchTerminalConditionData::CallerFollow)
                } else {
                    quote!(#facade::__private::ExplorerRecoveryDispatchTerminalConditionData::Unconditional)
                };
                quote!(Some(#facade::__private::explorer_recovery_dispatch_target(
                    #facade::__private::explorer_recovery_variant_index(#variant),
                    #condition,
                )))
            },
        );
        let commit = state.recovery_commit.map_or_else(
            || quote!(None),
            |variant| quote!(Some(#facade::__private::explorer_recovery_variant_index(#variant))),
        );
        definitions.push(quote! {
            static #edges_name:
                [#facade::__private::ExplorerRecoveryDispatchEdgeData; #edge_count] =
                [#(#edges),*];
            static #node_name: #facade::__private::ExplorerRecoveryDispatchNodeData =
                #facade::__private::explorer_recovery_dispatch_node(
                    #target,
                    #commit,
                    &#edges_name,
                );
        });
    }
    let root = format_ident!(
        "__RECURSA_EXPLORER_RECOVERY_{stem}_{node_index}_{}",
        dispatch.root.index()
    );
    (quote!(#(#definitions)*), root)
}

/// Renders one branch's frozen recursive-operand associations as a static slice.
fn render_recovery_pratt_operands(
    facade: &Ident,
    name: &Ident,
    operands: &[PrattOperandParsePlan],
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    if operands.is_empty() {
        return (quote!(), quote!(&[]));
    }
    let values = operands
        .iter()
        .map(|operand| {
            let element_index = operand.element_index;
            let minimum_binding_power = operand.minimum_binding_power;
            quote! {
                #facade::__private::explorer_recovery_pratt_operand(
                    #facade::__private::explorer_recovery_element_index(#element_index),
                    #minimum_binding_power,
                )
            }
        })
        .collect::<Vec<_>>();
    let count = values.len();
    let definition = quote! {
        static #name:
            [#facade::__private::ExplorerRecoveryPrattOperandPlanData; #count] =
            [#(#values),*];
    };
    (definition, quote!(&#name))
}

/// Renders the immutable grammar-erased recovery payload shared by root callbacks.
///
/// # Long-function contract
///
/// Rationale: one traversal must preserve the frozen parse-application indices shared by
/// Explorer schemas, recovery elements, dispatch tries, and root callbacks. Splitting that
/// traversal would invite a second authority for static names, variant order, or element
/// positions. The function only serializes validated plans; it must not rediscover syntax,
/// operands, binding powers, or FOLLOW policy while rendering.
///
/// Its major phases emit each struct, predictive enum, or Pratt application; emit the dense
/// token-schema table; and finally bind each eligible root to its application plan. The Pratt
/// phase keeps NUD then LED source order, emits dispatch and synchronization sets before their
/// users, and carries each recursive operand's typed element index beside its binding power.
/// Every generated reference therefore resolves to one earlier static definition from the same
/// frozen application.
fn render_recovery_static_plan(
    plan: &EmissionPlan,
    explorer: &ExplorerPlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let nodes = plan.nodes.iter().flat_map(|node| {
        node.parse_applications.iter().map(move |application| {
            let node_index = application.parse_index;
            let schema = explorer_schema_static(node_index, None);
            let recovery_node = explorer_recovery_node_static(node_index);
            match &application.body {
                NodeBodyPlan::Struct { elements, .. } => {
                    let elements_name = format_ident!(
                        "__RECURSA_EXPLORER_RECOVERY_ELEMENTS_{node_index}"
                    );
                    let elements = elements
                        .iter()
                        .enumerate()
                        .map(|(index, element)| {
                            render_recovery_element_plan(
                                plan,
                                explorer,
                                node_index,
                                None,
                                index,
                                element,
                            )
                        })
                        .collect::<Vec<_>>();
                    let count = elements.len();
                    let restart = recovery_expected_ref(plan, &application.recovery_restart);
                    quote! {
                        static #elements_name:
                            [#facade::__private::ExplorerRecoveryElementPlanData; #count] =
                            [#(#elements),*];
                        static #recovery_node: #facade::__private::ExplorerRecoveryNodePlanData =
                            #facade::__private::explorer_recovery_struct_plan(
                                &#schema,
                                &#elements_name,
                                #restart,
                            );
                    }
                }
                NodeBodyPlan::Enum { variants, dispatch } => {
                    let (dispatch_definitions, dispatch_name) =
                        render_recovery_dispatch_graph(plan, node_index, "DISPATCH", dispatch);
                    let restart = recovery_expected_ref(plan, &dispatch.restart);
                    let variant_definitions = variants
                        .iter()
                        .enumerate()
                        .map(|(variant_index, variant)| {
                            let elements_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_ELEMENTS_{node_index}_{variant_index}"
                            );
                            let elements = variant
                                .elements
                                .iter()
                                .enumerate()
                                .map(|(element_index, element)| {
                                    render_recovery_element_plan(
                                        plan,
                                        explorer,
                                        node_index,
                                        Some(variant_index),
                                        element_index,
                                        element,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let element_count = elements.len();
                            let schema =
                                explorer_schema_static(node_index, Some(variant_index));
                            let variant_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_VARIANT_{node_index}_{variant_index}"
                            );
                            quote! {
                                static #elements_name:
                                    [#facade::__private::ExplorerRecoveryElementPlanData; #element_count] =
                                    [#(#elements),*];
                                static #variant_name:
                                    #facade::__private::ExplorerRecoveryVariantPlanData =
                                    #facade::__private::explorer_recovery_variant_plan(
                                        &#schema,
                                        &#elements_name,
                                    );
                            }
                        })
                        .collect::<Vec<_>>();
                    let variant_names = (0..variants.len()).map(|variant_index| {
                        format_ident!(
                            "__RECURSA_EXPLORER_RECOVERY_VARIANT_{node_index}_{variant_index}"
                        )
                    });
                    let variants_name =
                        format_ident!("__RECURSA_EXPLORER_RECOVERY_VARIANTS_{node_index}");
                    let variant_count = variants.len();
                    quote! {
                        #dispatch_definitions
                        #(#variant_definitions)*
                        static #variants_name:
                            [#facade::__private::ExplorerRecoveryVariantPlanData; #variant_count] =
                            [#(#variant_names),*];
                        static #recovery_node: #facade::__private::ExplorerRecoveryNodePlanData =
                            #facade::__private::explorer_recovery_enum_plan(
                                &#schema,
                                &#variants_name,
                                &#dispatch_name,
                                #restart,
                            );
                    }
                }
                NodeBodyPlan::Pratt(pratt) => {
                    let (nud_dispatch_definitions, nud_dispatch) = render_recovery_dispatch_graph(
                        plan,
                        node_index,
                        "PRATT_NUD_DISPATCH",
                        &pratt.nud_dispatch,
                    );
                    let (led_dispatch_definitions, led_dispatch) = pratt
                        .led_dispatch
                        .as_ref()
                        .map_or_else(
                            || (quote!(), quote!(None)),
                            |dispatch| {
                                let (definitions, name) = render_recovery_dispatch_graph(
                                    plan,
                                    node_index,
                                    "PRATT_LED_DISPATCH",
                                    dispatch,
                                );
                                (definitions, quote!(Some(&#name)))
                            },
                        );
                    let nud_restart = recovery_expected_ref(plan, &pratt.nud_dispatch.restart);
                    let nud_failure = render_recovery_follow(plan, &pratt.nud_failure);
                    let nud_selection_follow =
                        recovery_expected_ref(plan, &pratt.nud_selection_follow);

                    let nud_definitions = pratt
                        .nud
                        .iter()
                        .enumerate()
                        .map(|(variant_index, branch)| {
                            let elements_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_NUD_ELEMENTS_{node_index}_{variant_index}"
                            );
                            let elements = branch
                                .variant
                                .elements
                                .iter()
                                .enumerate()
                                .map(|(element_index, element)| {
                                    render_recovery_element_plan(
                                        plan,
                                        explorer,
                                        node_index,
                                        Some(variant_index),
                                        element_index,
                                        element,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let element_count = elements.len();
                            let variant_schema = explorer_schema_static(node_index, Some(variant_index));
                            let variant_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_NUD_VARIANT_{node_index}_{variant_index}"
                            );
                            let plan_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_NUD_{node_index}_{variant_index}"
                            );
                            let operands_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_NUD_OPERANDS_{node_index}_{variant_index}"
                            );
                            let (operands_definition, operands) = render_recovery_pratt_operands(
                                facade,
                                &operands_name,
                                &branch.operands,
                            );
                            let constructor = match branch.position {
                                PrattPosition::Atom => quote!(
                                    #facade::__private::explorer_recovery_pratt_atom(#variant_name)
                                ),
                                PrattPosition::Prefix { .. } => quote!(
                                    #facade::__private::explorer_recovery_pratt_prefix(
                                        #variant_name,
                                        #operands,
                                    )
                                ),
                                PrattPosition::Infix { .. } | PrattPosition::Postfix { .. } => {
                                    unreachable!("validated Pratt nud plan has a nud position")
                                }
                            };
                            quote! {
                                static #elements_name:
                                    [#facade::__private::ExplorerRecoveryElementPlanData; #element_count] =
                                    [#(#elements),*];
                                static #variant_name: #facade::__private::ExplorerRecoveryVariantPlanData =
                                    #facade::__private::explorer_recovery_variant_plan(
                                        &#variant_schema,
                                        &#elements_name,
                                    );
                                #operands_definition
                                static #plan_name: #facade::__private::ExplorerRecoveryPrattNudPlanData =
                                    #constructor;
                            }
                        })
                        .collect::<Vec<_>>();
                    let nud_names = (0..pratt.nud.len()).map(|variant_index| {
                        format_ident!(
                            "__RECURSA_EXPLORER_RECOVERY_PRATT_NUD_{node_index}_{variant_index}"
                        )
                    });
                    let nud_name =
                        format_ident!("__RECURSA_EXPLORER_RECOVERY_PRATT_NUD_{node_index}");
                    let nud_count = pratt.nud.len();

                    let led_definitions = pratt
                        .led
                        .iter()
                        .enumerate()
                        .map(|(led_index, branch)| {
                            let variant_index = nud_count + led_index;
                            let elements_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_LED_ELEMENTS_{node_index}_{led_index}"
                            );
                            let elements = branch
                                .variant
                                .elements
                                .iter()
                                .enumerate()
                                .map(|(tail_index, element)| {
                                    render_recovery_element_plan(
                                        plan,
                                        explorer,
                                        node_index,
                                        Some(variant_index),
                                        tail_index + 1,
                                        element,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let element_count = elements.len();
                            let variant_schema = explorer_schema_static(node_index, Some(variant_index));
                            let lhs_schema =
                                explorer_element_schema_static(node_index, Some(variant_index), 0);
                            let variant_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_LED_VARIANT_{node_index}_{led_index}"
                            );
                            let plan_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_LED_{node_index}_{led_index}"
                            );
                            let operands_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_LED_OPERANDS_{node_index}_{led_index}"
                            );
                            let (operands_definition, operands) = render_recovery_pratt_operands(
                                facade,
                                &operands_name,
                                &branch.operands,
                            );
                            let constructor = match branch.position {
                                PrattPosition::Infix { lbp, .. } => quote!(
                                    #facade::__private::explorer_recovery_pratt_infix(
                                        #variant_name,
                                        &#lhs_schema,
                                        #lbp,
                                        #operands,
                                    )
                                ),
                                PrattPosition::Postfix { bp } => quote!(
                                    #facade::__private::explorer_recovery_pratt_postfix(
                                        #variant_name,
                                        &#lhs_schema,
                                        #bp,
                                        #operands,
                                    )
                                ),
                                PrattPosition::Atom | PrattPosition::Prefix { .. } => {
                                    unreachable!("validated Pratt led plan has a led position")
                                }
                            };
                            quote! {
                                static #elements_name:
                                    [#facade::__private::ExplorerRecoveryElementPlanData; #element_count] =
                                    [#(#elements),*];
                                static #variant_name: #facade::__private::ExplorerRecoveryVariantPlanData =
                                    #facade::__private::explorer_recovery_variant_plan(
                                        &#variant_schema,
                                        &#elements_name,
                                    );
                                #operands_definition
                                static #plan_name: #facade::__private::ExplorerRecoveryPrattLedPlanData =
                                    #constructor;
                            }
                        })
                        .collect::<Vec<_>>();
                    let led_names = (0..pratt.led.len()).map(|led_index| {
                        format_ident!(
                            "__RECURSA_EXPLORER_RECOVERY_PRATT_LED_{node_index}_{led_index}"
                        )
                    });
                    let led_name =
                        format_ident!("__RECURSA_EXPLORER_RECOVERY_PRATT_LED_{node_index}");
                    let led_count = pratt.led.len();

                    let guards = pratt.caller_follow_guard.as_deref().unwrap_or(&[]);
                    let guard_definitions = guards
                        .iter()
                        .enumerate()
                        .map(|(guard_index, guard)| {
                            let guard_name = format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_GUARD_{node_index}_{guard_index}"
                            );
                            let expected = recovery_expected_ref(plan, &guard.expected);
                            let left_binding_power = pratt_led_lbp(guard.position);
                            quote! {
                                static #guard_name: #facade::__private::ExplorerRecoveryPrattGuardPlanData =
                                    #facade::__private::explorer_recovery_pratt_guard(
                                        #left_binding_power,
                                        #expected,
                                    );
                            }
                        })
                        .collect::<Vec<_>>();
                    let guard_names = (0..guards.len())
                        .map(|guard_index| {
                            format_ident!(
                                "__RECURSA_EXPLORER_RECOVERY_PRATT_GUARD_{node_index}_{guard_index}"
                            )
                        });
                    let guard_name =
                        format_ident!("__RECURSA_EXPLORER_RECOVERY_PRATT_GUARDS_{node_index}");
                    let guard_count = guards.len();

                    quote! {
                        #nud_dispatch_definitions
                        #led_dispatch_definitions
                        #(#nud_definitions)*
                        static #nud_name:
                            [#facade::__private::ExplorerRecoveryPrattNudPlanData; #nud_count] =
                            [#(#nud_names),*];
                        #(#led_definitions)*
                        static #led_name:
                            [#facade::__private::ExplorerRecoveryPrattLedPlanData; #led_count] =
                            [#(#led_names),*];
                        #(#guard_definitions)*
                        static #guard_name:
                            [#facade::__private::ExplorerRecoveryPrattGuardPlanData; #guard_count] =
                            [#(#guard_names),*];
                        static #recovery_node: #facade::__private::ExplorerRecoveryNodePlanData =
                            #facade::__private::explorer_recovery_pratt_plan(
                                &#schema,
                                &#nud_name,
                                &#led_name,
                                &#nud_dispatch,
                                #nud_restart,
                                #nud_failure,
                                #nud_selection_follow,
                                #led_dispatch,
                                &#guard_name,
                            );
                    }
                }
            }
        })
    });
    let token_schemas = (0..plan.kind_count).map(|kind| {
        explorer
            .tokens
            .iter()
            .find(|token| usize::from(token.kind_index) == kind)
            .map_or_else(
                || quote!(None),
                |token| {
                    let schema = explorer_token_schema_static(token.kind_index);
                    quote!(Some(&#schema))
                },
            )
    });
    let token_schema_count = plan.kind_count;
    let token_schemas_name = format_ident!("__RECURSA_EXPLORER_RECOVERY_TOKEN_SCHEMAS");
    let roots = explorer.roots.iter().map(|root| {
        let node_index = explorer_application_parse_plan(&plan.nodes, root.application)
            .1
            .parse_index;
        let recovery_node = explorer_recovery_node_static(node_index);
        let recovery_plan = explorer_recovery_plan_static(node_index);
        let text_equivalence = if matches!(plan.text_equivalence, TextEquivalence::AsciiInsensitive)
        {
            quote!(#facade::__private::ExplorerRecoveryTextEquivalenceData::AsciiInsensitive)
        } else {
            quote!(#facade::__private::ExplorerRecoveryTextEquivalenceData::Sensitive)
        };
        quote! {
            static #recovery_plan: #facade::__private::ExplorerRecoveryPlanData =
                #facade::__private::explorer_recovery_plan(
                    &#module_anchor::__RECURSA_GRAMMAR,
                    &#recovery_node,
                    &#module_anchor::__RECURSA_EMPTY_FOLLOW,
                    &#token_schemas_name,
                    #text_equivalence,
                );
        }
    });
    quote! {
        #(#nodes)*
        static #token_schemas_name:
            [Option<&#facade::__private::ExplorerTokenSchemaData>; #token_schema_count] =
            [#(#token_schemas),*];
        #(#roots)*
    }
}

/// Renders the complete static grammar schema shared by adapter callbacks.
pub(super) fn render_explorer_static_schema(
    plan: &EmissionPlan,
    explorer: &ExplorerPlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let tokens = render_explorer_token_schemas(plan, explorer);
    let recovery = render_recovery_static_plan(plan, explorer);
    let nodes = explorer.nodes.iter().map(|node| {
        let node_index = explorer_application_parse_plan(&plan.nodes, node.application)
            .1
            .parse_index;
        render_explorer_schema(plan, node, node_index)
    });
    let grammar_name = &explorer.grammar.name;
    let all_nodes = explorer
        .nodes
        .iter()
        .map(|node| {
            let schema = explorer_application_schema_static(plan, node.application);
            quote!(&#schema)
        })
        .collect::<Vec<_>>();
    let all_tokens = explorer
        .tokens
        .iter()
        .map(|token| {
            let schema = explorer_token_schema_static(token.kind_index);
            quote!(&#schema)
        })
        .collect::<Vec<_>>();
    let node_schema_count = all_nodes.len();
    let token_schema_count = all_tokens.len();
    let node_schemas_static = explorer_node_schemas_static();
    let token_schemas_static = explorer_token_schemas_static();
    let grammars = explorer.roots.iter().map(|root| {
        let root_schema = explorer_application_schema_static(plan, root.application);
        let node_index = explorer_application_parse_plan(&plan.nodes, root.application)
            .1
            .parse_index;
        let grammar_schema = explorer_grammar_schema_static(node_index);
        quote! {
            pub(crate) static #grammar_schema: #facade::__private::ExplorerGrammarSchemaData =
                #facade::__private::explorer_grammar_schema(
                    #grammar_name,
                    &#root_schema,
                    &#node_schemas_static,
                    &#token_schemas_static,
                );
        }
    });
    quote! {
        #tokens
        #(#nodes)*
        pub(crate) static #node_schemas_static:
            [&#facade::__private::ExplorerNodeSchemaData; #node_schema_count] =
            [#(#all_nodes),*];
        pub(crate) static #token_schemas_static:
            [&#facade::__private::ExplorerTokenSchemaData; #token_schema_count] =
            [#(#all_tokens),*];
        #(#grammars)*
        #recovery
    }
}

/// Resolves the static schema name for a materialized Node application.
pub(super) fn explorer_application_schema_static(
    plan: &EmissionPlan,
    application: MaterializedApplicationId,
) -> Ident {
    let parse_index = explorer_application_parse_plan(&plan.nodes, application)
        .1
        .parse_index;
    explorer_schema_static(parse_index, None)
}

/// Resolves the static schema name for a semantic content token.
pub(super) fn explorer_content_token_schema_static(
    explorer: &ExplorerPlan,
    token: ContentTokenId,
) -> Ident {
    let kind_index = explorer
        .tokens
        .iter()
        .find(|candidate| candidate.semantic == Some(token))
        .expect("typed explorer target retains its content token")
        .kind_index;
    explorer_token_schema_static(kind_index)
}

/// Names one element descriptor by Node, variant, and source-order index.
pub(super) fn explorer_element_schema_static(
    node_index: usize,
    variant: Option<usize>,
    element: usize,
) -> Ident {
    match variant {
        Some(variant) => {
            format_ident!("__RECURSA_EXPLORER_ELEMENT_SCHEMA_{node_index}_{variant}_{element}")
        }
        None => format_ident!("__RECURSA_EXPLORER_ELEMENT_SCHEMA_{node_index}_{element}"),
    }
}

/// Renders element descriptors and their optional semantic targets.
pub(super) fn render_explorer_schema_entries(
    plan: &EmissionPlan,
    node_index: usize,
    variant: Option<usize>,
    elements: &[ExplorerElementPlan],
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let definitions = elements.iter().enumerate().map(|(index, element)| {
        let schema = explorer_element_schema_static(node_index, variant, index);
        let name = &element.name;
        let role = match element.role {
            ExplorerElementRolePlan::FixedToken => quote!(#facade::ElementRole::FixedToken),
            ExplorerElementRolePlan::SemanticField => quote!(#facade::ElementRole::SemanticField),
        };
        let cardinality = match element.cardinality {
            ExplorerCardinalityPlan::One => quote!(#facade::explorer::Cardinality::One),
            ExplorerCardinalityPlan::Optional => quote!(#facade::explorer::Cardinality::Optional),
            ExplorerCardinalityPlan::ZeroOrMore => {
                quote!(#facade::explorer::Cardinality::ZeroOrMore)
            }
            ExplorerCardinalityPlan::OneOrMore => {
                quote!(#facade::explorer::Cardinality::OneOrMore)
            }
            ExplorerCardinalityPlan::OptionalOneOrMore => {
                quote!(#facade::explorer::Cardinality::OptionalOneOrMore)
            }
        };
        let target = match element.target {
            Some(ExplorerElementTargetPlan::Node(application)) => {
                let target = explorer_application_schema_static(plan, application);
                quote!(Some(#facade::__private::explorer_node_target(&#target)))
            }
            Some(ExplorerElementTargetPlan::ContentToken(token)) => {
                let target = explorer_content_token_schema_static(
                    plan.explorer
                        .as_ref()
                        .expect("explorer schema rendering has an explorer plan"),
                    token,
                );
                quote!(Some(#facade::__private::explorer_content_token_target(&#target)))
            }
            None => quote!(None),
        };
        let token = match (element.token_kind, element.target) {
            (Some(kind), _) => {
                let token = explorer_token_schema_static(kind);
                quote!(Some(&#token))
            }
            (None, Some(ExplorerElementTargetPlan::ContentToken(token))) => {
                let token = explorer_content_token_schema_static(
                    plan.explorer
                        .as_ref()
                        .expect("explorer schema rendering has an explorer plan"),
                    token,
                );
                quote!(Some(&#token))
            }
            (None, Some(ExplorerElementTargetPlan::Node(_)) | None) => quote!(None),
        };
        quote! {
            pub(crate) static #schema: #facade::__private::ExplorerElementSchemaData =
                #facade::__private::explorer_element_schema(
                    #name,
                    #role,
                    #cardinality,
                    #target,
                    #token,
                );
        }
    });
    quote!(#(#definitions)*)
}

/// Renders one Node application's struct, enum, or Pratt schema topology.
pub(super) fn render_explorer_schema(
    plan: &EmissionPlan,
    node: &ExplorerNodePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_name = &node.name;
    let qualified_name = &node.qualified_name;
    match &node.body {
        ExplorerNodeBodyPlan::Struct(variant) => {
            let definitions =
                render_explorer_schema_entries(plan, node_index, None, &variant.elements);
            let schema = explorer_schema_static(node_index, None);
            let element_schemas = (0..variant.elements.len())
                .map(|index| explorer_element_schema_static(node_index, None, index));
            quote! {
                #definitions
                pub(crate) static #schema: #facade::__private::ExplorerNodeSchemaData =
                    #facade::__private::explorer_node_schema(
                        #node_name,
                        #qualified_name,
                        &[#(#element_schemas),*],
                        &[],
                    );
            }
        }
        ExplorerNodeBodyPlan::Enum(variants) | ExplorerNodeBodyPlan::Pratt(variants) => {
            let base = explorer_schema_static(node_index, None);
            let definitions = variants.iter().enumerate().map(|(variant_index, variant)| {
                let entries = render_explorer_schema_entries(
                    plan,
                    node_index,
                    Some(variant_index),
                    &variant.elements,
                );
                let schema = explorer_schema_static(node_index, Some(variant_index));
                let variant_name = variant
                    .name
                    .as_deref()
                    .expect("enum and Pratt explorer variants retain their names");
                let variant_qualified_name = variant
                    .qualified_name
                    .as_deref()
                    .expect("enum and Pratt variants retain qualified names");
                let element_schemas = (0..variant.elements.len()).map(|element_index| {
                    explorer_element_schema_static(node_index, Some(variant_index), element_index)
                });
                quote! {
                    #entries
                    pub(crate) static #schema: #facade::__private::ExplorerVariantSchemaData =
                        #facade::__private::explorer_variant_schema(
                            #variant_name,
                            #variant_qualified_name,
                            &[#(#element_schemas),*],
                        );
                }
            });
            let variant_schemas = (0..variants.len()).map(|index| {
                let schema = explorer_schema_static(node_index, Some(index));
                quote!(&#schema)
            });
            quote! {
                pub(crate) static #base: #facade::__private::ExplorerNodeSchemaData =
                    #facade::__private::explorer_node_schema(
                        #node_name,
                        #qualified_name,
                        &[],
                        &[#(#variant_schemas),*],
                    );
                #(#definitions)*
            }
        }
    }
}

/// Renders occurrence child counts for each source-order element.
pub(super) fn render_explorer_child_count(
    element: &ExplorerElementPlan,
    field_value: Option<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let ExplorerOccurrencePlan::Children { wrappers, .. } = &element.occurrence else {
        return quote!(0);
    };
    let field_value = field_value.expect("semantic explorer elements have field values");
    if let Some(repeat_index) = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore))
    {
        let outer = &wrappers[..repeat_index];
        if outer
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
        {
            let optional = render_optional_view_reference(field_value, outer, true);
            quote!((#optional).map_or(0, |values| values.len()))
        } else {
            let collection = render_terminal_reference(field_value, outer, true);
            quote!((#collection).len())
        }
    } else if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    {
        let optional = render_optional_view_reference(field_value, wrappers, true);
        quote!(usize::from((#optional).is_some()))
    } else {
        quote!(1)
    }
}

/// Renders indexed child lookup while preserving parsed provenance.
pub(super) fn render_explorer_child(
    facade: &Ident,
    element_index: usize,
    element: &ExplorerElementPlan,
    field_value: Option<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let ExplorerOccurrencePlan::Children { wrappers, .. } = &element.occurrence else {
        return quote!(None);
    };
    let field_value = field_value.expect("semantic explorer elements have field values");
    let child_cursor = element.attachment_semantic_child.map_or_else(
        || quote!(cursor.child(#element_index).child(child)),
        |semantic_child| quote!(cursor.child(#element_index).child(child).child(#semantic_child)),
    );
    if let Some(repeat_index) = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore))
    {
        let outer = &wrappers[..repeat_index];
        let inner = &wrappers[repeat_index + 1..];
        let selected = if outer
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
        {
            let optional = render_optional_view_reference(field_value, outer, true);
            quote!((#optional).and_then(|values| values.get(child)))
        } else {
            let collection = render_terminal_reference(field_value, outer, true);
            quote!((#collection).get(child))
        };
        let value = render_terminal_reference(quote!(value), inner, true);
        quote! {
            (#selected).map(|value| {
                #facade::__private::ExplorerNode::__recursa_explorer_node(
                    #value,
                    #child_cursor,
                )
            })
        }
    } else if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    {
        let optional = render_optional_view_reference(field_value, wrappers, true);
        quote! {
            if child == 0 {
                (#optional).map(|value| {
                    #facade::__private::ExplorerNode::__recursa_explorer_node(
                        value,
                        #child_cursor,
                    )
                })
            } else {
                None
            }
        }
    } else {
        let value = render_terminal_reference(field_value, wrappers, true);
        quote! {
            (child == 0).then(|| {
                #facade::__private::ExplorerNode::__recursa_explorer_node(
                    #value,
                    #child_cursor,
                )
            })
        }
    }
}

/// Renders child-count dispatch for one active variant shape.
pub(super) fn render_explorer_variant_child_count(
    explorer: &ExplorerVariantPlan,
    parse_elements: &[ElementParsePlan],
    bindings: &[Option<proc_macro2::TokenStream>],
) -> proc_macro2::TokenStream {
    let arms = explorer
        .elements
        .iter()
        .enumerate()
        .map(|(element_index, element)| {
            let field = parse_elements
                .iter()
                .position(|parse| parse.explorer_source == element.source)
                .and_then(|index| bindings[index].clone());
            let count = render_explorer_child_count(element, field);
            quote!(#element_index => #count,)
        });
    quote!(match element { #(#arms)* _ => 0 })
}

/// Renders child-occurrence lookup for one active variant shape.
pub(super) fn render_explorer_variant_child(
    facade: &Ident,
    explorer: &ExplorerVariantPlan,
    parse_elements: &[ElementParsePlan],
    bindings: &[Option<proc_macro2::TokenStream>],
) -> proc_macro2::TokenStream {
    let arms = explorer
        .elements
        .iter()
        .enumerate()
        .map(|(element_index, element)| {
            let field = parse_elements
                .iter()
                .position(|parse| parse.explorer_source == element.source)
                .and_then(|index| bindings[index].clone());
            let child = render_explorer_child(facade, element_index, element, field);
            quote!(#element_index => #child,)
        });
    quote!(match element { #(#arms)* _ => None })
}
