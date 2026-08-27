//! Pretty planning, authoritative validation, and structured rendering.
//!
//! Pretty plan construction and emission are intentionally co-located: spacing, provenance,
//! Pratt parenthesization, repetition, and attachment layout form one behavioral contract.

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
    focused_case!(authored_pretty_authority_rejects_emitted_declaration_corruption);
    focused_case!(pretty_pratt_recursion_uses_typed_node_identity_not_the_final_path_segment);
    focused_case!(every_typed_pretty_pratt_decision_is_part_of_family_authority);
    focused_case!(recursive_concrete_pretty_impls_do_not_publish_cyclic_child_bounds);
    focused_case!(every_local_pretty_hint_dimension_is_part_of_family_authority);
    focused_case!(ascii_insensitive_word_separators_share_canonical_pretty_family_authority);
    focused_case!(generic_top_level_breaks_keep_typed_family_and_closed_application_authority);
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyEmissionPlan {
    pub(super) tokens: Vec<Option<PrettyTokenBehavior>>,
    pub(super) nodes: Vec<Option<PrettyNodeBehavior>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyTokenBehavior {
    MatchedOrStoredText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyNodeBehavior {
    pub(super) node: NodeId,
    pub(super) applications: Vec<MaterializedApplicationId>,
    pub(super) pretty: PrettyHints,
    pub(super) variant_pretty: Vec<PrettyHints>,
    pub(super) variant_layouts: Vec<PrettySequenceLayout>,
    pub(super) variants: Vec<Vec<PrettyElementBehavior>>,
    pub(super) pratt: Option<PrettyPrattBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyPrattBehavior {
    pub(super) grouping: Option<PrettyPrattGrouping>,
    pub(super) variants: Vec<PrettyPrattVariantBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyPrattGrouping {
    pub(super) before: Vec<PrettyAttachedFixedBehavior>,
    pub(super) after: Vec<PrettyAttachedFixedBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyPrattVariantBehavior {
    pub(super) parenthesization: PrettyPrattParenthesization,
    pub(super) operands: Vec<PrettyPrattOperandBehavior>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyPrattParenthesization {
    pub(super) entry_lbp: Option<u16>,
    pub(super) exit_rbp: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyPrattOperandBehavior {
    Ordinary,
    Recursive {
        owner: NodeId,
        applications: Vec<MaterializedApplicationId>,
        minimum: PrettyPrattMinimum,
        next: PrettyPrattNext,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyPrattMinimum {
    Zero,
    Inner,
    Binding(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyPrattNext {
    None,
    Inner,
    Binding(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettySequenceLayout {
    Linear,
    Delimited,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyElementBehavior {
    Fixed {
        spelling: String,
        spacing: PrettySpacing,
        rendering: PrettyFixedRendering,
        soft_break_before: bool,
        soft_break_after: bool,
    },
    Semantic {
        repetition: Option<PrettyRepetitionBehavior>,
        attachment: Option<PrettyAttachmentBehavior>,
        provenance: PrettyProvenanceBehavior,
        pretty: PrettyHints,
        presence: PrettyPresence,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyRepetitionBehavior {
    pub(super) separator: Option<String>,
    pub(super) trailing_separator: bool,
    pub(super) hard_break: bool,
    pub(super) leading_separator: bool,
    pub(super) space_before_separator: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyPresence {
    Always,
    Optional,
    NonEmpty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyProvenanceBehavior {
    Structured,
    Opaque,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyAttachmentBehavior {
    pub(super) layout: PrettySequenceLayout,
    pub(super) before: Vec<PrettyAttachedFixedBehavior>,
    pub(super) after: Vec<PrettyAttachedFixedBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct PrettyAttachedFixedBehavior {
    pub(super) spelling: String,
    pub(super) spacing: PrettySpacing,
    pub(super) rendering: PrettyFixedRendering,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PrettyFixedRendering {
    TightSequence,
    Before,
    After,
    Around,
    Opening,
    Word,
    Punctuation,
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn emitted_pretty_plan(
    nodes: &[NodeParsePlan],
    tokens: &[TokenParsePlan],
) -> PrettyEmissionPlan {
    PrettyEmissionPlan {
        tokens: tokens
            .iter()
            .map(|token| {
                token
                    .configured_traits
                    .contains(&ConfiguredTraitRecipe::Pretty)
                    .then_some(PrettyTokenBehavior::MatchedOrStoredText)
            })
            .collect(),
        nodes: nodes
            .iter()
            .map(|node| {
                node.configured_traits
                    .contains(&ConfiguredTraitRecipe::Pretty)
                    .then(|| emitted_pretty_node_behavior(node))
            })
            .collect(),
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn emitted_pretty_node_behavior(node: &NodeParsePlan) -> PrettyNodeBehavior {
    let variant_pretty = match &node.body {
        NodeBodyPlan::Struct { .. } => vec![PrettyHints::default()],
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
        NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
    };
    let mut variants = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => vec![emitted_pretty_elements(elements)],
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .map(|variant| emitted_pretty_elements(&variant.elements))
            .collect(),
        NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .iter()
            .map(|variant| emitted_pretty_elements(&variant.elements))
            .collect(),
    };
    let variant_layouts = variants
        .iter()
        .map(|elements| pretty_sequence_layout(elements))
        .collect();
    let applications = node
        .parse_applications
        .iter()
        .map(|application| application.application)
        .collect::<Vec<_>>();
    let pratt = match &node.body {
        NodeBodyPlan::Pratt(pratt) => Some(emitted_pretty_pratt_behavior(
            node,
            pratt,
            &applications,
            &mut variants,
        )),
        NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
    };
    PrettyNodeBehavior {
        node: node.node,
        applications,
        pretty: node.pretty.clone(),
        variant_pretty,
        variant_layouts,
        variants,
        pratt,
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn emitted_pretty_pratt_behavior(
    node: &NodeParsePlan,
    pratt: &PrattParsePlan,
    applications: &[MaterializedApplicationId],
    variants: &mut [Vec<PrettyElementBehavior>],
) -> PrettyPrattBehavior {
    let declaration_variants = trait_variants(&node.body);
    let mut positions = vec![None; declaration_variants.len()];
    for branch in &pratt.nud {
        positions[branch.source_index] = Some(branch.position);
    }
    for branch in &pratt.led {
        positions[branch.source_index] = Some(branch.position);
    }
    let variant_behaviors = declaration_variants
        .iter()
        .zip(variants.iter_mut())
        .zip(positions.iter().copied())
        .map(|((variant, elements), position)| {
            let position = position.expect("every Pratt source variant has a typed position");
            apply_pretty_pratt_fixed_layout(position, &variant.elements, elements);
            let mut semantic_index = 0;
            let operands = variant
                .elements
                .iter()
                .filter_map(|element| {
                    let ElementParseKind::Semantic { terminal_node, .. } = &element.kind else {
                        return None;
                    };
                    let current = semantic_index;
                    semantic_index += 1;
                    Some(if *terminal_node == Some(node.node) {
                        pretty_recursive_operand(node.node, applications, position, current)
                    } else {
                        PrettyPrattOperandBehavior::Ordinary
                    })
                })
                .collect();
            PrettyPrattVariantBehavior {
                parenthesization: pretty_pratt_parenthesization(position),
                operands,
            }
        })
        .collect();
    PrettyPrattBehavior {
        grouping: pretty_pratt_grouping(node.node, &declaration_variants, variants, &positions),
        variants: variant_behaviors,
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_pratt_parenthesization(
    position: PrattPosition,
) -> PrettyPrattParenthesization {
    let (entry_lbp, exit_rbp) = match position {
        PrattPosition::Atom => (None, 256),
        PrattPosition::Prefix { bp } => (None, u16::from(bp)),
        PrattPosition::Infix { lbp, rbp } => (Some(u16::from(lbp)), u16::from(rbp)),
        PrattPosition::Postfix { bp } => (Some(u16::from(bp)), 256),
    };
    PrettyPrattParenthesization {
        entry_lbp,
        exit_rbp,
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_recursive_operand(
    owner: NodeId,
    applications: &[MaterializedApplicationId],
    position: PrattPosition,
    semantic_index: usize,
) -> PrettyPrattOperandBehavior {
    let (minimum, next) = match position {
        PrattPosition::Atom => (PrettyPrattMinimum::Zero, PrettyPrattNext::None),
        PrattPosition::Prefix { bp } => (
            PrettyPrattMinimum::Binding(u16::from(bp)),
            PrettyPrattNext::Inner,
        ),
        PrattPosition::Postfix { bp } => (
            PrettyPrattMinimum::Inner,
            PrettyPrattNext::Binding(u16::from(bp)),
        ),
        PrattPosition::Infix { lbp, .. } if semantic_index == 0 => (
            PrettyPrattMinimum::Inner,
            PrettyPrattNext::Binding(u16::from(lbp)),
        ),
        PrattPosition::Infix { rbp, .. } => (
            PrettyPrattMinimum::Binding(u16::from(rbp)),
            PrettyPrattNext::Inner,
        ),
    };
    PrettyPrattOperandBehavior::Recursive {
        owner,
        applications: applications.to_vec(),
        minimum,
        next,
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn apply_pretty_pratt_fixed_layout(
    position: PrattPosition,
    elements: &[ElementParsePlan],
    behaviors: &mut [PrettyElementBehavior],
) {
    for (index, behavior) in behaviors.iter_mut().enumerate() {
        let PrettyElementBehavior::Fixed {
            spacing,
            rendering,
            soft_break_before,
            soft_break_after,
            ..
        } = behavior
        else {
            continue;
        };
        let run_start =
            index == 0 || matches!(elements[index - 1].kind, ElementParseKind::Semantic { .. });
        let run_end = index + 1 == elements.len()
            || matches!(elements[index + 1].kind, ElementParseKind::Semantic { .. });
        if matches!(position, PrattPosition::Prefix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto
            && *rendering == PrettyFixedRendering::Punctuation
        {
            *rendering = PrettyFixedRendering::Opening;
        }
        *soft_break_before = matches!(position, PrattPosition::Infix { .. })
            && run_start
            && *spacing == PrettySpacing::Auto;
        *soft_break_after = matches!(position, PrattPosition::Infix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto;
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_pratt_grouping(
    owner: NodeId,
    declaration_variants: &[VariantParsePlan],
    behaviors: &[Vec<PrettyElementBehavior>],
    positions: &[Option<PrattPosition>],
) -> Option<PrettyPrattGrouping> {
    declaration_variants
        .iter()
        .zip(behaviors)
        .zip(positions)
        .find_map(|((variant, behavior), position)| {
            if !matches!(position, Some(PrattPosition::Atom)) {
                return None;
            }
            match (variant.elements.as_slice(), behavior.as_slice()) {
                ([element], [PrettyElementBehavior::Semantic { attachment: Some(attachment), .. }])
                    if matches!(&element.kind, ElementParseKind::Semantic { terminal_node: Some(node), .. } if *node == owner) =>
                {
                    Some(PrettyPrattGrouping {
                        before: attachment.before.clone(),
                        after: attachment.after.clone(),
                    })
                }
                ([open, semantic, close], [PrettyElementBehavior::Fixed { spelling: open_spelling, spacing: open_spacing, rendering: open_rendering, .. }, PrettyElementBehavior::Semantic { .. }, PrettyElementBehavior::Fixed { spelling: close_spelling, spacing: close_spacing, rendering: close_rendering, .. }])
                    if matches!(&semantic.kind, ElementParseKind::Semantic { terminal_node: Some(node), .. } if *node == owner)
                        && matches!(open.kind, ElementParseKind::Fixed { .. })
                        && matches!(close.kind, ElementParseKind::Fixed { .. }) =>
                {
                    Some(PrettyPrattGrouping {
                        before: vec![PrettyAttachedFixedBehavior {
                            spelling: open_spelling.clone(),
                            spacing: *open_spacing,
                            rendering: *open_rendering,
                        }],
                        after: vec![PrettyAttachedFixedBehavior {
                            spelling: close_spelling.clone(),
                            spacing: *close_spacing,
                            rendering: *close_rendering,
                        }],
                    })
                }
                _ => None,
            }
        })
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn emitted_pretty_elements(elements: &[ElementParsePlan]) -> Vec<PrettyElementBehavior> {
    elements
        .iter()
        .map(|element| match &element.kind {
            ElementParseKind::Fixed {
                spelling, spacing, ..
            } => PrettyElementBehavior::Fixed {
                spelling: spelling.clone(),
                spacing: *spacing,
                rendering: pretty_fixed_rendering(spelling, *spacing),
                soft_break_before: false,
                soft_break_after: false,
            },
            ElementParseKind::Semantic {
                container,
                wrappers,
                attachment,
                separator,
                opaque_provenance,
                repetition_break,
                pretty,
                ..
            } => PrettyElementBehavior::Semantic {
                repetition: pretty_repetition_behavior(
                    matches!(container.as_ref(), PlannedContainer::Repeated { .. }),
                    separator.clone(),
                    matches!(
                        repetition_break,
                        PlannedRepetitionBreak::TopLevelStatement(_)
                            | PlannedRepetitionBreak::TopLevelStatementFamily(_)
                    ),
                    pretty,
                ),
                attachment: attachment.as_ref().map(|attachment| {
                    let before = attachment
                        .before
                        .iter()
                        .map(|fixed| PrettyAttachedFixedBehavior {
                            spelling: fixed.spelling.clone(),
                            spacing: fixed.spacing,
                            rendering: pretty_fixed_rendering(&fixed.spelling, fixed.spacing),
                        })
                        .collect::<Vec<_>>();
                    let after = attachment
                        .after
                        .iter()
                        .map(|fixed| PrettyAttachedFixedBehavior {
                            spelling: fixed.spelling.clone(),
                            spacing: fixed.spacing,
                            rendering: pretty_fixed_rendering(&fixed.spelling, fixed.spacing),
                        })
                        .collect::<Vec<_>>();
                    PrettyAttachmentBehavior {
                        layout: pretty_attachment_layout(&before, &after),
                        before,
                        after,
                    }
                }),
                provenance: if *opaque_provenance {
                    PrettyProvenanceBehavior::Opaque
                } else {
                    PrettyProvenanceBehavior::Structured
                },
                pretty: pretty.clone(),
                presence: pretty_presence(wrappers),
            },
        })
        .collect()
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_repetition_behavior(
    repeated: bool,
    separator: Option<String>,
    hard_break: bool,
    pretty: &PrettyHints,
) -> Option<PrettyRepetitionBehavior> {
    repeated.then(|| PrettyRepetitionBehavior {
        space_before_separator: separator.as_deref().is_some_and(is_word_shaped),
        separator,
        trailing_separator: matches!(
            pretty.trailing_separator,
            Some(PrettyTrailingSeparator::Always)
        ),
        hard_break,
        leading_separator: matches!(pretty.separator, Some(PrettySeparatorPosition::Leading)),
    })
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_fixed_rendering(
    spelling: &str,
    spacing: PrettySpacing,
) -> PrettyFixedRendering {
    match spacing {
        PrettySpacing::Tight => PrettyFixedRendering::TightSequence,
        PrettySpacing::Before => PrettyFixedRendering::Before,
        PrettySpacing::After => PrettyFixedRendering::After,
        PrettySpacing::Around => PrettyFixedRendering::Around,
        PrettySpacing::Auto if matches!(spelling, "(" | "[" | "{") => PrettyFixedRendering::Opening,
        PrettySpacing::Auto if is_word_shaped(spelling) => PrettyFixedRendering::Word,
        PrettySpacing::Auto => PrettyFixedRendering::Punctuation,
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_sequence_layout(elements: &[PrettyElementBehavior]) -> PrettySequenceLayout {
    let delimited = elements.first().and_then(|first| {
        let PrettyElementBehavior::Fixed { spelling, .. } = first else {
            return None;
        };
        delimiter_close(spelling).filter(|close| {
            elements.last().is_some_and(|last| {
                matches!(last, PrettyElementBehavior::Fixed { spelling, .. } if spelling == close)
            })
        })
    });
    if delimited.is_some() {
        PrettySequenceLayout::Delimited
    } else {
        PrettySequenceLayout::Linear
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_attachment_layout(
    before: &[PrettyAttachedFixedBehavior],
    after: &[PrettyAttachedFixedBehavior],
) -> PrettySequenceLayout {
    let delimited = before
        .first()
        .and_then(|open| delimiter_close(&open.spelling))
        .is_some_and(|close| after.last().is_some_and(|fixed| fixed.spelling == close));
    if delimited {
        PrettySequenceLayout::Delimited
    } else {
        PrettySequenceLayout::Linear
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_presence(wrappers: &[FieldWrapper]) -> PrettyPresence {
    if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    {
        PrettyPresence::Optional
    } else if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore))
    {
        PrettyPresence::NonEmpty
    } else {
        PrettyPresence::Always
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn authoritative_pretty_plan(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
) -> Result<Option<PrettyEmissionPlan>, String> {
    let source = lowered.source_modeled.source_model();
    let tokens = resolved
        .content_tokens
        .iter()
        .map(|content| {
            source
                .token_definitions
                .iter()
                .find_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(token) if token.semantic == content.id => {
                        Some(token)
                    }
                    TokenDefinitionKind::Content(_)
                    | TokenDefinitionKind::Fixed(_)
                    | TokenDefinitionKind::LexicalBase(_) => None,
                })
                .is_some_and(|token| {
                    token
                        .configured_traits
                        .effective
                        .iter()
                        .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty)
                })
                .then_some(PrettyTokenBehavior::MatchedOrStoredText)
        })
        .collect::<Vec<_>>();
    let nodes = resolved
        .nodes
        .iter()
        .map(|resolved_node| {
            let source_node = source
                .nodes
                .iter()
                .find(|node| node.resolved_node == resolved_node.id)
                .ok_or_else(|| {
                    format!(
                        "Node `{}` lacks an authoritative Pretty plan",
                        resolved_node.ident
                    )
                })?;
            if !source_node
                .configured_traits
                .effective
                .iter()
                .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty)
            {
                return Ok(None);
            }
            authoritative_pretty_node_behavior(lowered, resolved_node, source_node).map(Some)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let plan = PrettyEmissionPlan { tokens, nodes };
    Ok(pretty_family_identity(&plan).map(|_| plan))
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn authoritative_pretty_node_behavior(
    lowered: &crate::materialization::LoweredGrammar,
    resolved_node: &ResolvedNode,
    source_node: &SourceNode,
) -> Result<PrettyNodeBehavior, String> {
    let variant_pretty = match &source_node.body {
        SourceNodeBody::Struct(_) | SourceNodeBody::Presence(_) => vec![PrettyHints::default()],
        SourceNodeBody::Enum(variants) => variants
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
        SourceNodeBody::Pratt(variants) => variants
            .iter()
            .map(|variant| variant.common.pretty.clone())
            .collect(),
    };
    let mut variants = match &source_node.body {
        SourceNodeBody::Struct(elements) | SourceNodeBody::Presence(elements) => {
            vec![authoritative_pretty_elements(
                lowered,
                resolved_node,
                elements,
            )?]
        }
        SourceNodeBody::Enum(variants) => variants
            .iter()
            .map(|variant| authoritative_pretty_elements(lowered, resolved_node, &variant.elements))
            .collect::<Result<Vec<_>, String>>()?,
        SourceNodeBody::Pratt(variants) => variants
            .iter()
            .map(|variant| {
                authoritative_pretty_elements(lowered, resolved_node, &variant.common.elements)
            })
            .collect::<Result<Vec<_>, String>>()?,
    };
    let variant_layouts = variants
        .iter()
        .map(|elements| pretty_sequence_layout(elements))
        .collect();
    let applications = lowered
        .applications
        .iter()
        .filter(|application| application.node == resolved_node.id)
        .map(|application| application.id)
        .collect::<Vec<_>>();
    let pratt = match &source_node.body {
        SourceNodeBody::Pratt(source_variants) => Some(authoritative_pretty_pratt_behavior(
            lowered,
            resolved_node,
            source_variants,
            &applications,
            &mut variants,
        )?),
        SourceNodeBody::Struct(_) | SourceNodeBody::Presence(_) | SourceNodeBody::Enum(_) => None,
    };
    Ok(PrettyNodeBehavior {
        node: resolved_node.id,
        applications,
        pretty: source_node.pretty.clone(),
        variant_pretty,
        variant_layouts,
        variants,
        pratt,
    })
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn authoritative_pretty_pratt_behavior(
    lowered: &crate::materialization::LoweredGrammar,
    resolved_node: &ResolvedNode,
    source_variants: &[crate::source_model::SourcePrattVariant],
    applications: &[MaterializedApplicationId],
    variants: &mut [Vec<PrettyElementBehavior>],
) -> Result<PrettyPrattBehavior, String> {
    let resolved = lowered.source_modeled.resolved();
    let positions = source_variants
        .iter()
        .map(|variant| match variant.position {
            SourcePrattPosition::Atom => PrattPosition::Atom,
            SourcePrattPosition::Prefix { bp } => PrattPosition::Prefix { bp },
            SourcePrattPosition::Infix { lbp, rbp } => PrattPosition::Infix { lbp, rbp },
            SourcePrattPosition::Postfix { bp } => PrattPosition::Postfix { bp },
        })
        .collect::<Vec<_>>();
    let mut planned = Vec::with_capacity(source_variants.len());
    for ((variant, behavior), position) in source_variants
        .iter()
        .zip(variants.iter_mut())
        .zip(positions.iter().copied())
    {
        apply_authoritative_pretty_pratt_fixed_layout(position, &variant.common.elements, behavior);
        let mut semantic_index = 0;
        let mut operands = Vec::new();
        for element in &variant.common.elements {
            let GrammarElement::SemanticField { resolved_field, .. } = element else {
                continue;
            };
            let field = resolved_node
                .fields
                .iter()
                .find(|field| field.id == *resolved_field)
                .ok_or_else(|| "a Pratt Pretty operand lacks resolved syntax".to_owned())?;
            let projection = authored_projection(&field.ty, resolved)?;
            operands.push(if projection.terminal_node == Some(resolved_node.id) {
                pretty_recursive_operand(resolved_node.id, applications, position, semantic_index)
            } else {
                PrettyPrattOperandBehavior::Ordinary
            });
            semantic_index += 1;
        }
        planned.push(PrettyPrattVariantBehavior {
            parenthesization: pretty_pratt_parenthesization(position),
            operands,
        });
    }
    Ok(PrettyPrattBehavior {
        grouping: authoritative_pretty_pratt_grouping(
            resolved_node,
            source_variants,
            variants,
            &positions,
            resolved,
        )?,
        variants: planned,
    })
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn apply_authoritative_pretty_pratt_fixed_layout(
    position: PrattPosition,
    elements: &[GrammarElement],
    behaviors: &mut [PrettyElementBehavior],
) {
    for (index, behavior) in behaviors.iter_mut().enumerate() {
        let PrettyElementBehavior::Fixed {
            spacing,
            rendering,
            soft_break_before,
            soft_break_after,
            ..
        } = behavior
        else {
            continue;
        };
        let semantic =
            |element: &GrammarElement| matches!(element, GrammarElement::SemanticField { .. });
        let run_start = index == 0 || semantic(&elements[index - 1]);
        let run_end = index + 1 == elements.len() || semantic(&elements[index + 1]);
        if matches!(position, PrattPosition::Prefix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto
            && *rendering == PrettyFixedRendering::Punctuation
        {
            *rendering = PrettyFixedRendering::Opening;
        }
        *soft_break_before = matches!(position, PrattPosition::Infix { .. })
            && run_start
            && *spacing == PrettySpacing::Auto;
        *soft_break_after = matches!(position, PrattPosition::Infix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto;
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn authoritative_pretty_pratt_grouping(
    resolved_node: &ResolvedNode,
    source_variants: &[crate::source_model::SourcePrattVariant],
    behaviors: &[Vec<PrettyElementBehavior>],
    positions: &[PrattPosition],
    resolved: &ResolvedSourceGrammar,
) -> Result<Option<PrettyPrattGrouping>, String> {
    for ((variant, behavior), position) in source_variants.iter().zip(behaviors).zip(positions) {
        if !matches!(position, PrattPosition::Atom) {
            continue;
        }
        let is_recursive = |element: &GrammarElement| -> Result<bool, String> {
            let GrammarElement::SemanticField { resolved_field, .. } = element else {
                return Ok(false);
            };
            let field = resolved_node
                .fields
                .iter()
                .find(|field| field.id == *resolved_field)
                .ok_or_else(|| "a Pratt grouping operand lacks resolved syntax".to_owned())?;
            Ok(authored_projection(&field.ty, resolved)?.terminal_node == Some(resolved_node.id))
        };
        match (variant.common.elements.as_slice(), behavior.as_slice()) {
            (
                [semantic],
                [
                    PrettyElementBehavior::Semantic {
                        attachment: Some(attachment),
                        ..
                    },
                ],
            ) if is_recursive(semantic)? => {
                return Ok(Some(PrettyPrattGrouping {
                    before: attachment.before.clone(),
                    after: attachment.after.clone(),
                }));
            }
            (
                [
                    GrammarElement::FixedToken { .. },
                    semantic,
                    GrammarElement::FixedToken { .. },
                ],
                [
                    PrettyElementBehavior::Fixed {
                        spelling: open_spelling,
                        spacing: open_spacing,
                        rendering: open_rendering,
                        ..
                    },
                    PrettyElementBehavior::Semantic { .. },
                    PrettyElementBehavior::Fixed {
                        spelling: close_spelling,
                        spacing: close_spacing,
                        rendering: close_rendering,
                        ..
                    },
                ],
            ) if is_recursive(semantic)? => {
                return Ok(Some(PrettyPrattGrouping {
                    before: vec![PrettyAttachedFixedBehavior {
                        spelling: open_spelling.clone(),
                        spacing: *open_spacing,
                        rendering: *open_rendering,
                    }],
                    after: vec![PrettyAttachedFixedBehavior {
                        spelling: close_spelling.clone(),
                        spacing: *close_spacing,
                        rendering: *close_rendering,
                    }],
                }));
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn authoritative_pretty_elements(
    lowered: &crate::materialization::LoweredGrammar,
    resolved_node: &ResolvedNode,
    elements: &[GrammarElement],
) -> Result<Vec<PrettyElementBehavior>, String> {
    let source = lowered.source_modeled.source_model();
    let resolved = lowered.source_modeled.resolved();
    elements
        .iter()
        .map(|element| match element {
            GrammarElement::FixedToken { token_use, .. } => {
                let usage = &source.token_uses[token_use.index()];
                let definition = &source.token_definitions[usage.definition.index()];
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return Err("an authored Pretty fixed element is not fixed syntax".to_owned());
                };
                let spelling =
                    fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence);
                Ok(PrettyElementBehavior::Fixed {
                    rendering: pretty_fixed_rendering(&spelling, fixed.pretty_spacing),
                    spelling,
                    spacing: fixed.pretty_spacing,
                    soft_break_before: false,
                    soft_break_after: false,
                })
            }
            GrammarElement::SemanticField {
                resolved_field,
                attachment,
                separator,
                pretty,
                ..
            } => {
                let field = resolved_node
                    .fields
                    .iter()
                    .find(|field| field.id == *resolved_field)
                    .ok_or_else(|| "an authored Pretty field lacks resolved syntax".to_owned())?;
                let projection = authored_projection(&field.ty, resolved)?;
                let repeated = matches!(
                    declaration_container_operation(&projection.wrappers),
                    ContainerOperation::ZeroOrMore
                        | ContainerOperation::OneOrMore
                        | ContainerOperation::OptionalOneOrMore
                );
                let separator_spelling =
                    authoritative_pretty_separator(lowered, separator.as_ref())?;
                let top_level_break = separator.as_ref().is_some_and(|separator| {
                    let usage = &source.token_uses[separator.token_use.index()];
                    let definition = &source.token_definitions[usage.definition.index()];
                    matches!(
                        &definition.kind,
                        TokenDefinitionKind::Fixed(fixed) if fixed.spelling == ";"
                    )
                }) && repeated
                    && projection.terminal_node.is_some_and(|node_id| {
                        matches!(
                            resolved.nodes[node_id.index()].body,
                            ResolvedNodeBody::Enum { .. }
                        )
                    });
                Ok(PrettyElementBehavior::Semantic {
                    repetition: pretty_repetition_behavior(
                        repeated,
                        separator_spelling,
                        top_level_break,
                        pretty,
                    ),
                    attachment: attachment
                        .as_ref()
                        .map(|attachment| authoritative_pretty_attachment(lowered, attachment))
                        .transpose()?,
                    provenance: if authored_type_depends_on_node_generics(
                        &field.ty,
                        resolved_node,
                        resolved,
                    ) {
                        PrettyProvenanceBehavior::Opaque
                    } else {
                        PrettyProvenanceBehavior::Structured
                    },
                    pretty: pretty.clone(),
                    presence: pretty_presence(&projection.wrappers),
                })
            }
        })
        .collect()
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn authoritative_pretty_separator(
    lowered: &crate::materialization::LoweredGrammar,
    separator: Option<&crate::source_model::Separator>,
) -> Result<Option<String>, String> {
    let Some(separator) = separator else {
        return Ok(None);
    };
    let source = lowered.source_modeled.source_model();
    let usage = &source.token_uses[separator.token_use.index()];
    let definition = &source.token_definitions[usage.definition.index()];
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("a Pretty separator is not fixed syntax".to_owned());
    };
    Ok(Some(fixed_pretty_spelling(
        &fixed.spelling,
        lowered.grammar.text_equivalence,
    )))
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn authoritative_pretty_attachment(
    lowered: &crate::materialization::LoweredGrammar,
    attachment: &crate::source_model::FixedTokenAttachment,
) -> Result<PrettyAttachmentBehavior, String> {
    let source = lowered.source_modeled.source_model();
    let fixed = |token_use: crate::lowering::TokenUseId| {
        let usage = &source.token_uses[token_use.index()];
        let definition = &source.token_definitions[usage.definition.index()];
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            return Err("an attached Pretty element is not fixed syntax".to_owned());
        };
        let spelling = fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence);
        Ok(PrettyAttachedFixedBehavior {
            rendering: pretty_fixed_rendering(&spelling, fixed.pretty_spacing),
            spelling,
            spacing: fixed.pretty_spacing,
        })
    };
    let before = attachment
        .before
        .iter()
        .map(|attached| fixed(attached.token_use))
        .collect::<Result<Vec<_>, _>>()?;
    let after = attachment
        .after
        .iter()
        .map(|attached| fixed(attached.token_use))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PrettyAttachmentBehavior {
        layout: pretty_attachment_layout(&before, &after),
        before,
        after,
    })
}
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_family_identity(plan: &PrettyEmissionPlan) -> Option<u64> {
    (plan.tokens.iter().any(Option::is_some) || plan.nodes.iter().any(Option::is_some))
        .then(|| plan_family_identity(plan))
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pratt_node_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    _pratt: &PrattParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Pratt behavior");
    let pratt_behavior = behavior
        .pratt
        .as_ref()
        .expect("validated typed Pretty Pratt behavior");
    let (impl_generics, type_generics, where_clause) = pretty_impl_parts(facade, node);
    let variants = trait_variants(&node.body);
    let arms = variants
        .iter()
        .zip(&behavior.variant_layouts)
        .zip(&behavior.variants)
        .zip(&behavior.variant_pretty)
        .zip(&pratt_behavior.variants)
        .map(|((((variant, layout), behavior), variant_pretty), pratt_variant)| {
            let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "pretty_pratt");
            let mut binding_index = 0_usize;
            let mut semantic_index = 0_usize;
            let rendered = variant
            .elements
            .iter()
            .zip(behavior)
            .enumerate()
            .map(|(index, (element, behavior))| match (&element.kind, behavior) {
                (
                    ElementParseKind::Fixed { .. },
                    PrettyElementBehavior::Fixed {
                        spelling,
                        rendering,
                        soft_break_before,
                        soft_break_after,
                        ..
                    },
                ) => {
                    let fixed = render_pretty_fixed(plan, index, spelling, *rendering);
                    let before = soft_break_before
                        .then(|| quote!(context.soft_break(" ")?;));
                    let after = soft_break_after.then(|| quote!(context.soft_break(" ")?;));
                    quote!(#before #fixed #after)
                }
                (
                    ElementParseKind::Semantic {
                        wrappers,
                        container,
                        ..
                    },
                    PrettyElementBehavior::Semantic {
                        attachment,
                        pretty,
                        presence,
                        provenance,
                        ..
                    },
                ) => {
                    assert!(
                        matches!(container.as_ref(), PlannedContainer::Direct { .. })
                            && wrappers
                                .iter()
                                .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed)),
                        "validated Pratt operands are direct and may only use Box storage wrappers",
                    );
                    let binding = &bindings[binding_index];
                    binding_index += 1;
                    let operand = &pratt_variant.operands[semantic_index];
                    semantic_index += 1;
                    let semantic = if let PrettyPrattOperandBehavior::Recursive {
                        minimum,
                        next,
                        ..
                    } = operand
                    {
                        let minimum = render_pretty_pratt_minimum(*minimum);
                        let next = render_pretty_pratt_next(*next);
                        quote!((#binding).__recursa_pretty_pratt(context, #minimum, #next))
                    } else {
                        quote!(#facade::Pretty::pretty(#binding, context))
                    };
                    let rendered = if let Some(attachment) = attachment {
                        let unit = render_pretty_attachment_unit(
                            attachment,
                            *provenance,
                            semantic,
                        );
                        quote! {
                            context.with_child(#index, |context| {
                                context.with_child(0, |context| { #unit Ok(()) })
                            })?;
                        }
                    } else {
                        let child = pretty_provenance_child_method(*provenance);
                        quote! {
                            context.with_child(#index, |context| {
                                context.#child(0, |context| { #semantic })
                            })?;
                        }
                    };
                    render_pretty_presence(
                        pretty,
                        *presence,
                        wrappers,
                        quote!(#binding),
                        rendered,
                    )
                }
                (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
                | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. }) => {
                    unreachable!("validated Pretty Pratt element behavior")
                }
            })
            .collect::<Vec<_>>();
            let rendered = render_pretty_sequence(*layout, &rendered);
            let rendered = render_pretty_hints(variant_pretty, rendered);
            let entry_check = pratt_variant
                .parenthesization
                .entry_lbp
                .map_or_else(|| quote!(false), |entry| quote!(#entry < min_bp));
            let exit = pratt_variant.parenthesization.exit_rbp;
            let group_open = pratt_behavior.grouping.as_ref().map(|grouping| {
                grouping
                    .before
                    .iter()
                    .map(|fixed| {
                        render_pretty_synthetic_fixed(plan, &fixed.spelling, fixed.rendering)
                    })
                    .collect::<proc_macro2::TokenStream>()
            });
            let group_close = pratt_behavior.grouping.as_ref().map(|grouping| {
                grouping
                    .after
                    .iter()
                    .map(|fixed| {
                        render_pretty_synthetic_fixed(plan, &fixed.spelling, fixed.rendering)
                    })
                    .collect::<proc_macro2::TokenStream>()
            });
            quote! {
                #pattern => {
                    let __recursa_parenthesize = #entry_check
                        || next_lbp.is_some_and(|next| next >= #exit);
                    if __recursa_parenthesize {
                        #group_open
                    }
                    let __recursa_inner_min = if __recursa_parenthesize { 0 } else { min_bp };
                    let __recursa_inner_next = if __recursa_parenthesize { None } else { next_lbp };
                    #rendered
                    if __recursa_parenthesize {
                        #group_close
                    }
                    Ok(())
                }
            }
        });
    let rendered_node = render_pretty_hints(
        &behavior.pretty,
        quote!(self.__recursa_pretty_pratt(context, 0, None)?;),
    );
    quote! {
        impl #impl_generics #node_ident #type_generics #where_clause {
            fn __recursa_pretty_pratt(
                &self,
                context: &mut #facade::PrettyContext,
                min_bp: u16,
                next_lbp: Option<u16>,
            ) -> ::core::fmt::Result {
                match self { #(#arms),* }
            }
        }

        impl #impl_generics #facade::Pretty for #node_ident #type_generics #where_clause {
            fn pretty(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.configure_defaults(#max_width, #indent);
                context.baseline_group(|context| { #rendered_node Ok(()) })
            }
        }
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_pratt_minimum(minimum: PrettyPrattMinimum) -> proc_macro2::TokenStream {
    match minimum {
        PrettyPrattMinimum::Zero => quote!(0u16),
        PrettyPrattMinimum::Inner => quote!(__recursa_inner_min),
        PrettyPrattMinimum::Binding(binding) => quote!(#binding),
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_pratt_next(next: PrettyPrattNext) -> proc_macro2::TokenStream {
    match next {
        PrettyPrattNext::None => quote!(None),
        PrettyPrattNext::Inner => quote!(__recursa_inner_next),
        PrettyPrattNext::Binding(binding) => quote!(Some(#binding)),
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn terminal_path_ident(terminal: &Type) -> Option<&Ident> {
    let Type::Path(path) = terminal else {
        return None;
    };
    path.path.segments.last().map(|segment| &segment.ident)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_node_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Node behavior");
    let (impl_generics, type_generics, where_clause) = pretty_impl_parts(facade, node);
    let elements = |elements: &[ElementParsePlan],
                    behavior: &[PrettyElementBehavior],
                    layout: PrettySequenceLayout,
                    base: proc_macro2::TokenStream| {
        let rendered = elements
            .iter()
            .zip(behavior)
            .enumerate()
            .map(
                |(index, (element, behavior))| match (&element.kind, behavior) {
                    (
                        ElementParseKind::Fixed { .. },
                        PrettyElementBehavior::Fixed {
                            spelling,
                            rendering,
                            ..
                        },
                    ) => render_pretty_fixed(plan, index, spelling, *rendering),
                    (
                        ElementParseKind::Semantic {
                            field, wrappers, ..
                        },
                        PrettyElementBehavior::Semantic {
                            repetition,
                            attachment,
                            pretty,
                            presence,
                            provenance,
                            ..
                        },
                    ) => {
                        let value = field_access(base.clone(), field.ident.as_ref(), field.index);
                        let rendered = if let Some(attachment) = attachment {
                            render_pretty_attached_semantic(
                                facade,
                                index,
                                value.clone(),
                                false,
                                wrappers,
                                attachment,
                                repetition.as_ref(),
                                *presence,
                                *provenance,
                            )
                        } else {
                            render_pretty_semantic(
                                facade,
                                index,
                                quote!(&#value),
                                repetition.as_ref(),
                                *provenance,
                            )
                        };
                        render_pretty_presence(
                            pretty,
                            *presence,
                            wrappers,
                            quote!(&#value),
                            rendered,
                        )
                    }
                    (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
                    | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. }) => {
                        unreachable!("validated Pretty element behavior")
                    }
                },
            )
            .collect::<Vec<_>>();
        render_pretty_sequence(layout, &rendered)
    };
    let body = match &node.body {
        NodeBodyPlan::Struct {
            elements: fields, ..
        } => {
            let rendered = elements(
                fields,
                &behavior.variants[0],
                behavior.variant_layouts[0],
                quote!(self),
            );
            quote!({ #rendered Ok(()) })
        }
        NodeBodyPlan::Pratt(pratt) => {
            return render_pratt_node_pretty(plan, node, node_index, pratt);
        }
        NodeBodyPlan::Enum { .. } => {
            let variants = trait_variants(&node.body);
            let arms = variants
                .iter()
                .zip(&behavior.variant_layouts)
                .zip(&behavior.variants)
                .zip(&behavior.variant_pretty)
                .map(|(((variant, layout), behavior), variant_pretty)| {
                    let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "pretty");
                    let mut binding_index = 0;
                    let rendered = variant
                        .elements
                        .iter()
                        .zip(behavior)
                        .enumerate()
                        .map(
                            |(index, (element, behavior))| match (&element.kind, behavior) {
                                (
                                    ElementParseKind::Fixed { .. },
                                    PrettyElementBehavior::Fixed {
                                        spelling,
                                        rendering,
                                        ..
                                    },
                                ) => render_pretty_fixed(plan, index, spelling, *rendering),
                                (
                                    ElementParseKind::Semantic { wrappers, .. },
                                    PrettyElementBehavior::Semantic {
                                        repetition,
                                        attachment,
                                        pretty,
                                        presence,
                                        provenance,
                                        ..
                                    },
                                ) => {
                                    let binding = &bindings[binding_index];
                                    binding_index += 1;
                                    let rendered = if let Some(attachment) = attachment {
                                        render_pretty_attached_semantic(
                                            facade,
                                            index,
                                            quote!(#binding),
                                            true,
                                            wrappers,
                                            attachment,
                                            repetition.as_ref(),
                                            *presence,
                                            *provenance,
                                        )
                                    } else {
                                        render_pretty_semantic(
                                            facade,
                                            index,
                                            quote!(#binding),
                                            repetition.as_ref(),
                                            *provenance,
                                        )
                                    };
                                    render_pretty_presence(
                                        pretty,
                                        *presence,
                                        wrappers,
                                        quote!(#binding),
                                        rendered,
                                    )
                                }
                                (
                                    ElementParseKind::Fixed { .. },
                                    PrettyElementBehavior::Semantic { .. },
                                )
                                | (
                                    ElementParseKind::Semantic { .. },
                                    PrettyElementBehavior::Fixed { .. },
                                ) => unreachable!("validated Pretty element behavior"),
                            },
                        )
                        .collect::<Vec<_>>();
                    let rendered = render_pretty_sequence(*layout, &rendered);
                    let rendered = render_pretty_hints(variant_pretty, rendered);
                    quote!(#pattern => { #rendered Ok(()) })
                });
            quote!(match self { #(#arms),* })
        }
    };
    let body = render_pretty_hints(&behavior.pretty, quote!(#body?;));
    quote! {
        impl #impl_generics #facade::Pretty for #node_ident #type_generics #where_clause {
            fn pretty(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.configure_defaults(#max_width, #indent);
                context.baseline_group(|context| { #body Ok(()) })
            }
        }
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_semantic(
    facade: &Ident,
    index: usize,
    value: proc_macro2::TokenStream,
    repetition: Option<&PrettyRepetitionBehavior>,
    provenance: PrettyProvenanceBehavior,
) -> proc_macro2::TokenStream {
    let render_value = if let Some(repetition) = repetition {
        let separator = repetition
            .separator
            .as_deref()
            .map_or_else(|| quote!(None), |separator| quote!(Some(#separator)));
        let trailing = repetition.trailing_separator;
        let hard_break = repetition.hard_break;
        let leading = repetition.leading_separator;
        let space_before_separator = repetition.space_before_separator;
        quote! {
            context.with_repetition_style(#separator, #trailing, #hard_break, #leading, #space_before_separator, |context| {
                #facade::Pretty::pretty(#value, context)
            })
        }
    } else {
        quote!(#facade::Pretty::pretty(#value, context))
    };
    if repetition.is_some() {
        quote! {
            context.with_child(#index, |context| #render_value)?;
        }
    } else {
        let child = pretty_provenance_child_method(provenance);
        quote! {
            context.with_child(#index, |context| {
                context.#child(0, |context| #render_value)
            })?;
        }
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_hints(
    hints: &PrettyHints,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let before = match hints.break_before {
        Some(PrettyBreak::Soft) => quote!(context.soft_break(" ")?;),
        Some(PrettyBreak::Hard) => quote!(context.hard_break()?;),
        None => quote!(),
    };
    let after = match hints.break_after {
        Some(PrettyBreak::Soft) => quote!(context.soft_break(" ")?;),
        Some(PrettyBreak::Hard) => quote!(context.hard_break()?;),
        None => quote!(),
    };
    let body = if hints.indent {
        quote!(context.indented(|context| { #body Ok(()) })?;)
    } else {
        body
    };
    let body = match hints.group {
        Some(PrettyGroup::Consistent) => quote!(context.group(|context| { #body Ok(()) })?;),
        Some(PrettyGroup::Inconsistent) => {
            quote!(context.inconsistent_group(|context| { #body Ok(()) })?;)
        }
        None => body,
    };
    quote!(#before #body #after)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_presence(
    hints: &PrettyHints,
    presence: PrettyPresence,
    wrappers: &[FieldWrapper],
    value: proc_macro2::TokenStream,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let body = render_pretty_hints(hints, body);
    if hints.break_before.is_none()
        && hints.break_after.is_none()
        && !hints.indent
        && hints.group.is_none()
    {
        return body;
    }
    if presence == PrettyPresence::Always {
        return body;
    }
    let mut reference = value;
    for wrapper in wrappers {
        match wrapper {
            FieldWrapper::Boxed => reference = quote!((#reference).as_ref()),
            FieldWrapper::Optional if presence == PrettyPresence::Optional => {
                return quote!(if (#reference).is_some() { #body });
            }
            FieldWrapper::ZeroOrMore if presence == PrettyPresence::NonEmpty => {
                return quote!(if !(#reference).is_empty() { #body });
            }
            FieldWrapper::Optional | FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {}
        }
    }
    unreachable!("frozen Pretty presence has no matching storage wrapper")
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_provenance_child_method(provenance: PrettyProvenanceBehavior) -> Ident {
    match provenance {
        PrettyProvenanceBehavior::Structured => format_ident!("with_child"),
        PrettyProvenanceBehavior::Opaque => format_ident!("with_opaque_child"),
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_fixed(
    plan: &EmissionPlan,
    index: usize,
    spelling: &str,
    rendering: PrettyFixedRendering,
) -> proc_macro2::TokenStream {
    render_pretty_fixed_with_method(plan, index, spelling, rendering)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_fixed_with_method(
    plan: &EmissionPlan,
    index: usize,
    spelling: &str,
    rendering: PrettyFixedRendering,
) -> proc_macro2::TokenStream {
    let method = pretty_fixed_method(rendering);
    let fallback = pretty_fixed_fallback(plan, spelling);
    quote! {
        context.with_child(#index, |context| {
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#fallback))
        })?;
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_synthetic_fixed(
    plan: &EmissionPlan,
    spelling: &str,
    rendering: PrettyFixedRendering,
) -> proc_macro2::TokenStream {
    let method = pretty_fixed_method(rendering);
    let fallback = pretty_fixed_fallback(plan, spelling);
    quote!(context.without_provenance(|context| context.#method(#fallback))?;)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_fixed_method(rendering: PrettyFixedRendering) -> syn::Ident {
    match rendering {
        PrettyFixedRendering::TightSequence => format_ident!("tight_sequence_token"),
        PrettyFixedRendering::Before => format_ident!("before_token"),
        PrettyFixedRendering::After => format_ident!("after_token"),
        PrettyFixedRendering::Around => format_ident!("around_token"),
        PrettyFixedRendering::Opening => format_ident!("opening_token"),
        PrettyFixedRendering::Word => format_ident!("token"),
        PrettyFixedRendering::Punctuation => format_ident!("punctuation_token"),
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn pretty_fixed_fallback(_plan: &EmissionPlan, spelling: &str) -> String {
    spelling.to_owned()
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_sequence(
    layout: PrettySequenceLayout,
    rendered: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    if layout == PrettySequenceLayout::Delimited && rendered.len() >= 2 {
        let open = &rendered[0];
        let middle = &rendered[1..rendered.len() - 1];
        let close = &rendered[rendered.len() - 1];
        quote! {
            #open
            context.indented(|context| {
                context.soft_break("")?;
                #(#middle)*
                Ok(())
            })?;
            context.soft_break("")?;
            #close
            context.close_delimiter();
        }
    } else {
        quote!(#(#rendered)*)
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_attached_fixed(
    index: usize,
    fixed: &PrettyAttachedFixedBehavior,
) -> proc_macro2::TokenStream {
    let method = pretty_fixed_method(fixed.rendering);
    let spelling = &fixed.spelling;
    quote! {
        context.with_child(#index, |context| {
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#spelling))
        })?;
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_attachment_unit(
    attachment: &PrettyAttachmentBehavior,
    provenance: PrettyProvenanceBehavior,
    semantic_body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let before = attachment
        .before
        .iter()
        .enumerate()
        .map(|(index, fixed)| render_pretty_attached_fixed(index, fixed))
        .collect::<Vec<_>>();
    let semantic_index = before.len();
    let child = pretty_provenance_child_method(provenance);
    let semantic = quote! {
        context.#child(#semantic_index, |context| { #semantic_body })?;
    };
    let after = attachment
        .after
        .iter()
        .enumerate()
        .map(|(index, fixed)| render_pretty_attached_fixed(semantic_index + 1 + index, fixed))
        .collect::<Vec<_>>();
    if attachment.layout == PrettySequenceLayout::Delimited {
        let open = &before[0];
        let leading = &before[1..];
        let close = after
            .last()
            .expect("delimited attachment has a close token");
        let trailing = &after[..after.len() - 1];
        quote! {
            #open
            context.indented(|context| {
                context.soft_break("")?;
                #(#leading)*
                #semantic
                #(#trailing)*
                Ok(())
            })?;
            context.soft_break("")?;
            #close
            context.close_delimiter();
        }
    } else {
        quote!(#(#before)* #semantic #(#after)*)
    }
}

#[allow(clippy::too_many_arguments)]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_pretty_attached_semantic(
    facade: &Ident,
    element_index: usize,
    value: proc_macro2::TokenStream,
    value_is_reference: bool,
    wrappers: &[FieldWrapper],
    attachment: &PrettyAttachmentBehavior,
    repetition: Option<&PrettyRepetitionBehavior>,
    presence: PrettyPresence,
    provenance: PrettyProvenanceBehavior,
) -> proc_macro2::TokenStream {
    let render_unit = |terminal: proc_macro2::TokenStream| {
        let semantic = quote!(#facade::Pretty::pretty(#terminal, context));
        render_pretty_attachment_unit(attachment, provenance, semantic)
    };

    if let Some(repetition) = repetition {
        let repeat_index = wrappers
            .iter()
            .position(|wrapper| {
                matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
            })
            .expect("repeated attached field has a repetition wrapper");
        let outer = &wrappers[..repeat_index];
        let inner = &wrappers[repeat_index + 1..];
        let item = render_terminal_reference(quote!(item), inner, true);
        let unit = render_unit(item);
        let separator = repetition
            .separator
            .as_deref()
            .map_or_else(|| quote!(None), |separator| quote!(Some(#separator)));
        let trailing = repetition.trailing_separator;
        let hard_break = repetition.hard_break;
        let leading = repetition.leading_separator;
        let space_before_separator = repetition.space_before_separator;
        let render_collection = |collection: proc_macro2::TokenStream| {
            quote! {
                context.with_repetition_style(
                    #separator,
                    #trailing,
                    #hard_break,
                    #leading,
                    #space_before_separator,
                    |context| {
                        context.render_repeated_by(
                            (#collection).iter(),
                            |context, item| { #unit Ok(()) },
                        )
                    },
                )?;
            }
        };
        let outer_optional = presence == PrettyPresence::Optional;
        let body = if outer_optional {
            let collection = render_optional_view_reference(value, outer, value_is_reference);
            let rendered = render_collection(quote!(collection));
            quote! {
                if let Some(collection) = #collection {
                    #rendered
                }
            }
        } else {
            let collection = render_terminal_reference(value, outer, value_is_reference);
            render_collection(collection)
        };
        quote! {
            context.with_child(#element_index, |context| {
                #body
                Ok(())
            })?;
        }
    } else if presence == PrettyPresence::Optional {
        let terminal = render_optional_view_reference(value, wrappers, value_is_reference);
        let unit = render_unit(quote!(terminal));
        quote! {
            if let Some(terminal) = #terminal {
                context.with_child(#element_index, |context| {
                    context.with_child(0, |context| { #unit Ok(()) })
                })?;
            }
        }
    } else {
        let terminal = render_terminal_reference(value, wrappers, value_is_reference);
        let unit = render_unit(terminal);
        quote! {
            context.with_child(#element_index, |context| {
                context.with_child(0, |context| { #unit Ok(()) })
            })?;
        }
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn delimiter_close(open: &str) -> Option<&'static str> {
    match open {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        _ => None,
    }
}
