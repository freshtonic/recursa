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
    #[test]
    fn explorer_generation_supports_semantic_targets_sharing_a_base_kind() {
        test_support::explorer_generation_supports_semantic_targets_sharing_a_base_kind();
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
    /// Every content-token identity lexed as this kind, empty for fixed syntax.
    ///
    /// Multiple declared semantic targets may share one content base kind; they all
    /// resolve to this kind-level schema entry.
    pub(super) semantics: Vec<ContentTokenId>,
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
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
            let semantics = lowered
                .lexical_plan
                .parser_targets
                .iter()
                .filter(|target| target.base_kind == kind.id)
                .map(|target| target.semantic)
                .collect::<Vec<_>>();
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
                // A merged kind is never lexed, so the explorer never sees one in a token
                // stream; it is planned as the keyword it spells so the kind table stays dense.
                TokenKindIdentity::Merged(merged) => (
                    ExplorerTokenKindPlan::Fixed,
                    Some(fixed_pretty_spelling(
                        &merged.spelling,
                        lowered.grammar.text_equivalence,
                    )),
                    ExplorerHighlightClassPlan::Keyword,
                    ExplorerDisplayPlan::Fixed(fixed_pretty_spelling(
                        &merged.spelling,
                        lowered.grammar.text_equivalence,
                    )),
                ),
                TokenKindIdentity::ContentBase(_) => (
                    ExplorerTokenKindPlan::Content,
                    None,
                    ExplorerHighlightClassPlan::Content,
                    ExplorerDisplayPlan::MatchedOrStored,
                ),
            };
            Ok(ExplorerTokenPlan {
                kind_index,
                semantics,
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
            TokenKindIdentity::Merged(merged) => merged.name.clone(),
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

/// Rejects an authored `View` item or import in the grammar module, where the generic view lives.
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

    let anchor = resolved.configuration.module;
    if let Some(origin) = occupied.get(&(anchor, "View".to_owned())) {
        return Err(format!(
            "generated view type `View` collides with {origin} in the grammar module"
        ));
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
    reserve(root_module, explorer_grammar_tables_static().to_string())?;
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
            ExplorerNodeBodyPlan::Struct(_) => vec![None],
            ExplorerNodeBodyPlan::Enum(variants) | ExplorerNodeBodyPlan::Pratt(variants) => {
                (0..variants.len()).map(Some).collect()
            }
        };
        for variant in variants {
            if let Some(variant) = variant {
                reserve(
                    root_module,
                    explorer_schema_static(parse_index, Some(variant)).to_string(),
                )?;
            }
            reserve(
                root_module,
                explorer_elements_static(parse_index, variant).to_string(),
            )?;
        }
    }
    let mut checked_root_nodes = BTreeSet::new();
    for root in &explorer.roots {
        let (node, _) = explorer_application_parse_plan(nodes, root.application);
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
        .find(|candidate| candidate.semantics.contains(&token.semantic))
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

                fn __recursa_explorer_field(
                    &self,
                    _element: usize,
                ) -> #facade::__private::ExplorerField<'_> {
                    #facade::__private::ExplorerField::None
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
    let (node_body, field_body) = match (&application.body, &explorer_node.body) {
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
            let field = render_explorer_variant_field(facade, explorer, elements, &bindings);
            (
                quote!(#facade::__private::explorer_node(self, &#module_anchor::#schema, None, cursor)),
                field,
            )
        }
        (NodeBodyPlan::Enum { variants, .. }, ExplorerNodeBodyPlan::Enum(explorer_variants)) => {
            let base = explorer_schema_static(node_index, None);
            let mut node_arms = Vec::new();
            let mut field_arms = Vec::new();
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
                let field =
                    render_explorer_variant_field(facade, explorer, &variant.elements, &bindings);
                node_arms.push(quote!(#pattern => #facade::__private::explorer_node(self, &#module_anchor::#base, Some(&#module_anchor::#active), cursor),));
                field_arms.push(quote!(#pattern => #field,));
            }
            (
                quote!(match self { #(#node_arms)* }),
                quote!(match self { #(#field_arms)* }),
            )
        }
        (NodeBodyPlan::Pratt(pratt), ExplorerNodeBodyPlan::Pratt(explorer_variants)) => {
            let base = explorer_schema_static(node_index, None);
            let mut node_arms = Vec::new();
            let mut field_arms = Vec::new();
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
                let field =
                    render_explorer_variant_field(facade, explorer, &variant.elements, &bindings);
                node_arms.push(quote!(#pattern => #facade::__private::explorer_node(self, &#module_anchor::#base, Some(&#module_anchor::#active), cursor),));
                field_arms.push(quote!(#pattern => #field,));
            }
            (
                quote!(match self { #(#node_arms)* }),
                quote!(match self { #(#field_arms)* }),
            )
        }
        _ => unreachable!("typed explorer and parse Node bodies have identical shapes"),
    };

    let root = explorer_plan
        .roots
        .iter()
        .find(|root| root.node == node.node && root.application == application.application);
    let entry = root.map(|root| {
        let root_schema = explorer_schema_static(node_index, None);
        let grammar_tables = explorer_grammar_tables_static();
        let explorer_lex = explorer_lex_function();
        let raw_lex = explorer_raw_lex_function();
        let pretty = root.capabilities.pretty;
        let pretty_method = pretty.then(|| {
            quote! {
                fn __recursa_explorer_pretty(
                    parsed: &#facade::Parsed<'_, Self>,
                    config: &#facade::PrettyConfig,
                ) -> Option<#facade::explorer::PrettyOutput> {
                    #facade::__private::explorer_pretty_root(parsed, config)
                }
            }
        });
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
                    /// Partitions exact source into strict typed islands, or rejects
                    /// the document at its first failing island.
                    pub fn frame #method_generics(
                        source: &'__recursa_input str,
                    ) -> Result<
                        #facade::framing::CompleteFrame<'__recursa_input, Self>,
                        #facade::framing::FrameFailure<'__recursa_input>,
                    > {
                        #facade::__private::frame_document::<Self>(
                            source,
                            #module_anchor::__recursa_lexical::__recursa_frame_lex,
                            &#module_anchor::__RECURSA_FRAMING_PLAN,
                        )
                    }
                }
            });
        // The sample gate expands to `(false, None)` unless the facade enables both
        // `explorer` and `arbitrary`, so the generic sample path is named only when
        // the runtime provides it.
        let sample_binding = if arbitrary {
            quote! {
                let (arbitrary, sample): (bool, Option<#facade::__private::ExplorerSampleFn>) =
                    __recursa_explorer_sample!(
                        #facade::__private::explorer_sample_root::<#static_ty>
                    );
            }
        } else {
            quote! {
                let (arbitrary, sample): (bool, Option<#facade::__private::ExplorerSampleFn>) =
                    (false, None);
            }
        };
        quote! {
            unsafe impl #entry_impl_generics #facade::__private::ExplorerRoot
                for #ty #entry_where_clause
            {
                #pretty_method
            }

            unsafe impl #facade::__private::ExplorerRootFamily for #static_ty {
                type Root<'__recursa_input> = #ty;
            }

            impl #entry_impl_generics #ty #entry_where_clause {
                /// Returns this source-visible Node family's opaque explorer adapter.
                ///
                /// Explorer occurrence projection is independent of the optional `Visit`
                /// traversal implementation.
                pub fn explorer() -> #facade::explorer::Adapter {
                    #sample_binding
                    unsafe {
                        #facade::__private::explorer_adapter(
                            &#module_anchor::#grammar_tables,
                            &#module_anchor::#root_schema,
                            #pretty,
                            arbitrary,
                            #module_anchor::#explorer_lex,
                            #module_anchor::__recursa_lexical::#raw_lex,
                            #facade::__private::explorer_parse_root::<#static_ty>,
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

                fn __recursa_explorer_field(
                    &self,
                    element: usize,
                ) -> #facade::__private::ExplorerField<'_> {
                    #field_body
                }
            }

            #entry
        }
    }
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

/// Names the single grammar-local lexer handed to the shared strict root parse.
pub(super) fn explorer_raw_lex_function() -> Ident {
    format_ident!("__recursa_explorer_raw_lex")
}

/// Names the static schema for one token kind.
pub(super) fn explorer_token_schema_static(kind_index: u16) -> Ident {
    format_ident!("__RECURSA_EXPLORER_TOKEN_SCHEMA_{kind_index}")
}

/// Names the single static grammar-wide schema tables shared by every root adapter.
pub(super) fn explorer_grammar_tables_static() -> Ident {
    format_ident!("__RECURSA_EXPLORER_GRAMMAR")
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

/// Renders the complete static grammar schema shared by adapter callbacks.
pub(super) fn render_explorer_static_schema(
    plan: &EmissionPlan,
    explorer: &ExplorerPlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let tokens = render_explorer_token_schemas(plan, explorer);
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
    let grammar_tables = explorer_grammar_tables_static();
    quote! {
        #tokens
        #(#nodes)*
        pub(crate) static #node_schemas_static:
            [&#facade::__private::ExplorerNodeSchemaData; #node_schema_count] =
            [#(#all_nodes),*];
        pub(crate) static #token_schemas_static:
            [&#facade::__private::ExplorerTokenSchemaData; #token_schema_count] =
            [#(#all_tokens),*];
        pub(crate) static #grammar_tables: #facade::__private::ExplorerGrammarTablesData =
            #facade::__private::explorer_grammar_tables(
                #grammar_name,
                &#node_schemas_static,
                &#token_schemas_static,
            );
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
///
/// The resolution is kind-level: every semantic target sharing one content base kind
/// reaches the same kind schema static, so the lookup is total by construction.
pub(super) fn explorer_content_token_schema_static(
    explorer: &ExplorerPlan,
    token: ContentTokenId,
) -> Ident {
    let kind_index = explorer
        .tokens
        .iter()
        .find(|candidate| candidate.semantics.contains(&token))
        .expect("typed explorer target retains its content token")
        .kind_index;
    explorer_token_schema_static(kind_index)
}

/// Names the element-descriptor table of one Node shape or variant.
pub(super) fn explorer_elements_static(node_index: usize, variant: Option<usize>) -> Ident {
    match variant {
        Some(variant) => format_ident!("__RECURSA_EXPLORER_ELEMENTS_{node_index}_{variant}"),
        None => format_ident!("__RECURSA_EXPLORER_ELEMENTS_{node_index}"),
    }
}

/// Renders one shape's element descriptors as a single static table.
pub(super) fn render_explorer_schema_entries(
    plan: &EmissionPlan,
    node_index: usize,
    variant: Option<usize>,
    elements: &[ExplorerElementPlan],
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let table = explorer_elements_static(node_index, variant);
    let count = elements.len();
    let definitions = elements.iter().map(|element| {
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
        let attachment_child = element
            .attachment_semantic_child
            .map_or_else(|| quote!(None), |child| quote!(Some(#child)));
        quote! {
            #facade::__private::explorer_element_schema(
                #name,
                #role,
                #cardinality,
                #target,
                #token,
                #attachment_child,
            ),
        }
    });
    quote! {
        pub(crate) static #table: [#facade::__private::ExplorerElementSchemaData; #count] =
            [#(#definitions)*];
    }
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
            let elements = explorer_elements_static(node_index, None);
            quote! {
                #definitions
                pub(crate) static #schema: #facade::__private::ExplorerNodeSchemaData =
                    #facade::__private::explorer_node_schema(
                        #node_name,
                        #qualified_name,
                        &#elements,
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
                let elements = explorer_elements_static(node_index, Some(variant_index));
                quote! {
                    #entries
                    pub(crate) static #schema: #facade::__private::ExplorerVariantSchemaData =
                        #facade::__private::explorer_variant_schema(
                            #variant_name,
                            #variant_qualified_name,
                            &#elements,
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

/// Renders the erased semantic field behind one source-order element.
///
/// The runtime derives child counts and child cursors from this field and the
/// static element schema, so only the wrapper shape is emitted per element.
pub(super) fn render_explorer_field(
    facade: &Ident,
    element: &ExplorerElementPlan,
    field_value: Option<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let ExplorerOccurrencePlan::Children { wrappers, .. } = &element.occurrence else {
        return quote!(#facade::__private::ExplorerField::None);
    };
    let field_value = field_value.expect("semantic explorer elements have field values");
    let has_optional = |wrappers: &[FieldWrapper]| {
        wrappers
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    };
    if let Some(repeat_index) = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore))
    {
        let outer = &wrappers[..repeat_index];
        if has_optional(outer) {
            let optional = render_optional_view_reference(field_value, outer, true);
            quote!(#facade::__private::explorer_field_optional_many(#optional))
        } else {
            let collection = render_terminal_reference(field_value, outer, true);
            quote!(#facade::__private::explorer_field_many(#collection))
        }
    } else if has_optional(wrappers) {
        let optional = render_optional_view_reference(field_value, wrappers, true);
        quote!(#facade::__private::explorer_field_optional(#optional))
    } else {
        let value = render_terminal_reference(field_value, wrappers, true);
        quote!(#facade::__private::explorer_field_one(#value))
    }
}

/// Renders erased-field dispatch for one active variant shape.
pub(super) fn render_explorer_variant_field(
    facade: &Ident,
    explorer: &ExplorerVariantPlan,
    parse_elements: &[ElementParsePlan],
    bindings: &[Option<proc_macro2::TokenStream>],
) -> proc_macro2::TokenStream {
    let arms = explorer
        .elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            matches!(element.occurrence, ExplorerOccurrencePlan::Children { .. })
        })
        .map(|(element_index, element)| {
            let field = parse_elements
                .iter()
                .position(|parse| parse.explorer_source == element.source)
                .and_then(|index| bindings[index].clone());
            let field = render_explorer_field(facade, element, field);
            quote!(#element_index => #field,)
        });
    quote!(match element { #(#arms)* _ => #facade::__private::ExplorerField::None })
}
