//! Railroad-diagram planning and deterministic Rustdoc rendering.
//!
//! Plan construction and SVG/document rendering stay together so diagram semantics
//! cannot drift between authoritative validation and emitted documentation.

use super::*;

#[cfg(feature = "diagrams")]
#[derive(Clone, Debug)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct DiagramPlan {
    pub(super) grammar_name: String,
    pub(super) nodes: Vec<NodeDiagramPlan>,
    pub(super) fixed_tokens: Vec<(String, String)>,
}

#[cfg(feature = "diagrams")]
#[derive(Clone, Debug)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct NodeDiagramPlan {
    pub(super) semantic_key: String,
    pub(super) ident: String,
    pub(super) public_path: Option<String>,
    pub(super) production: String,
}
#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn plan_diagrams(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
    nodes: &[NodeParsePlan],
    kind_names: &[String],
    kind_static_texts: &[Option<String>],
) -> Result<DiagramPlan, String> {
    let nodes = nodes
        .iter()
        .map(|node| {
            let source = resolved
                .nodes
                .iter()
                .find(|source| source.key.segments.join("::") == node.semantic_key)
                .ok_or_else(|| {
                    format!(
                        "diagram plan cannot correlate frozen Node `{}`",
                        node.semantic_key
                    )
                })?;
            let public_path = node_is_publicly_linkable(resolved, source)
                .then(|| format!("crate::{}", node.semantic_key));
            let applications = lowered
                .applications
                .iter()
                .filter(|application| application.node == source.id)
                .collect::<Vec<_>>();
            if applications.len() != node.parse_applications.len() {
                return Err(format!(
                    "diagram plan cannot correlate every materialized application for `{}`",
                    node.semantic_key
                ));
            }
            let mut productions = applications
                .into_iter()
                .zip(&node.parse_applications)
                .map(|(materialized, planned)| {
                    Ok((
                        materialized.display_name.clone(),
                        diagram_production(
                            &node.ident,
                            &node.semantic_key,
                            &planned.body,
                            materialized,
                            lowered,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            productions.sort_by(|left, right| left.0.cmp(&right.0));
            let production = if productions.len() == 1 {
                productions.pop().expect("one diagram production").1
            } else {
                productions
                    .into_iter()
                    .map(|(application, production)| format!("{application} => {production}"))
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            Ok(NodeDiagramPlan {
                semantic_key: node.semantic_key.clone(),
                ident: node.ident.unraw().to_string(),
                public_path,
                production,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let fixed_tokens = kind_names
        .iter()
        .zip(kind_static_texts)
        .filter_map(|(name, spelling)| {
            spelling
                .as_ref()
                .map(|spelling| (name.clone(), spelling.clone()))
        })
        .collect();
    Ok(DiagramPlan {
        grammar_name: resolved.build_inputs.module_anchor.segments.join("::"),
        nodes,
        fixed_tokens,
    })
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn node_is_publicly_linkable(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    node: &ResolvedNode,
) -> bool {
    if !matches!(node.visibility, Visibility::Public(_)) {
        return false;
    }
    let mut module = Some(node.module);
    while let Some(current) = module {
        let inclusion = &resolved.tree.inclusions[current.index()];
        if inclusion.parent.is_some() && !matches!(inclusion.visibility, Visibility::Public(_)) {
            return false;
        }
        module = inclusion.parent;
    }
    true
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_production(
    node_ident: &Ident,
    semantic_key: &str,
    body: &NodeBodyPlan,
    application: &MaterializedNodeApplication,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    let right = match (body, &application.body) {
        (
            NodeBodyPlan::Struct { elements, .. },
            MaterializedNodeBody::Struct(materialized)
            | MaterializedNodeBody::Presence(materialized),
        ) => diagram_sequence(elements, materialized, lowered)?,
        (
            NodeBodyPlan::Enum { variants, .. },
            MaterializedNodeBody::Enum(materialized_variants),
        ) => {
            if variants.len() != materialized_variants.len() {
                return Err("a diagram enum lost a frozen materialized variant".to_owned());
            }
            let alternatives = variants
                .iter()
                .zip(materialized_variants)
                .map(|(variant, materialized)| {
                    let sequence =
                        diagram_sequence(&variant.elements, &materialized.elements, lowered)?;
                    Ok(if sequence == "ε" {
                        variant.ident.unraw().to_string()
                    } else {
                        format!("{}({sequence})", variant.ident.unraw())
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            alternatives.join(" | ")
        }
        (NodeBodyPlan::Pratt(pratt), MaterializedNodeBody::Pratt(materialized)) => {
            diagram_pratt_production(pratt, materialized, lowered)?
        }
        _ => {
            return Err(format!(
                "Node `{}` diagram and materialized shapes disagree",
                semantic_key
            ));
        }
    };
    Ok(format!("{} ::= {right}", node_ident.unraw()))
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_pratt_production(
    pratt: &PrattParsePlan,
    materialized: &[MaterializedPrattVariant],
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    if pratt.expected_nud_count + pratt.expected_led_count != materialized.len() {
        return Err("a Pratt diagram lost a frozen materialized variant".to_owned());
    }
    let mut alternatives = Vec::with_capacity(materialized.len());
    for (source_index, materialized) in materialized.iter().enumerate() {
        let (position, variant, sequence) = match materialized {
            MaterializedPrattVariant::Nud(materialized) => {
                let branch = unique_pratt_nud(pratt, source_index)?;
                validate_pratt_position(Some(branch.position), materialized)?;
                validate_diagram_variant_name(&branch.variant, &materialized.common.name)?;
                let sequence =
                    diagram_sequence(&branch.variant.elements, &materialized.body, lowered)?;
                (branch.position, &branch.variant, sequence)
            }
            MaterializedPrattVariant::Led(materialized) => {
                let branch = unique_pratt_led(pratt, source_index)?;
                validate_pratt_led_position(Some(branch.position), materialized)?;
                validate_diagram_variant_name(&branch.variant, &materialized.common.name)?;
                let lhs = diagram_pratt_lhs(&branch.lhs, materialized, lowered)?;
                let tail = diagram_sequence(&branch.variant.elements, &materialized.tail, lowered)?;
                let sequence = if tail == "ε" {
                    lhs
                } else {
                    format!("{lhs} {tail}")
                };
                (branch.position, &branch.variant, sequence)
            }
        };
        let role = diagram_pratt_role(position);
        let name = variant.ident.unraw();
        alternatives.push(if sequence == "ε" {
            format!("{role} {name}")
        } else {
            format!("{role} {name}({sequence})")
        });
    }
    Ok(alternatives.join(" | "))
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn unique_pratt_nud(
    pratt: &PrattParsePlan,
    source_index: usize,
) -> Result<&PrattNudParsePlan, String> {
    let mut matching = pratt
        .nud
        .iter()
        .filter(|branch| branch.source_index == source_index);
    let branch = matching
        .next()
        .ok_or_else(|| "a Pratt diagram lacks its frozen nud branch".to_owned())?;
    if matching.next().is_some() {
        return Err("a Pratt diagram has duplicate frozen nud branches".to_owned());
    }
    Ok(branch)
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn unique_pratt_led(
    pratt: &PrattParsePlan,
    source_index: usize,
) -> Result<&PrattLedParsePlan, String> {
    let mut matching = pratt
        .led
        .iter()
        .filter(|branch| branch.source_index == source_index);
    let branch = matching
        .next()
        .ok_or_else(|| "a Pratt diagram lacks its frozen led branch".to_owned())?;
    if matching.next().is_some() {
        return Err("a Pratt diagram has duplicate frozen led branches".to_owned());
    }
    Ok(branch)
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn validate_diagram_variant_name(
    variant: &VariantParsePlan,
    materialized_name: &str,
) -> Result<(), String> {
    if variant.ident.unraw() == materialized_name {
        Ok(())
    } else {
        Err("a Pratt diagram variant name diverges from materialized source".to_owned())
    }
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_pratt_lhs(
    lhs: &ElementParsePlan,
    materialized: &MaterializedPrattLed,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    let ElementParseKind::Semantic {
        container,
        pratt_min_bp,
        ..
    } = &lhs.kind
    else {
        return Err("a Pratt diagram lhs is not a frozen semantic element".to_owned());
    };
    if pratt_min_bp.is_some() {
        return Err("a Pratt diagram lhs unexpectedly has a minimum binding power".to_owned());
    }
    let terminal =
        diagram_terminal_name(&ClosedType::Node(materialized.lhs.owner.clone()), lowered)?;
    diagram_semantic_element(&terminal, None, container)
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_pratt_role(position: PrattPosition) -> String {
    match position {
        PrattPosition::Atom => "atom".to_owned(),
        PrattPosition::Prefix { bp } => format!("prefix(bp={bp})"),
        PrattPosition::Infix { lbp, rbp } => format!("infix(lbp={lbp}, rbp={rbp})"),
        PrattPosition::Postfix { bp } => format!("postfix(bp={bp})"),
    }
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_sequence(
    elements: &[ElementParsePlan],
    materialized: &[MaterializedElement],
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    if elements.len() != materialized.len() {
        return Err("a diagram sequence lost a frozen materialized element".to_owned());
    }
    if elements.is_empty() {
        return Ok("ε".to_owned());
    }
    elements
        .iter()
        .zip(materialized)
        .map(
            |(element, materialized)| match (&element.kind, materialized) {
                (
                    ElementParseKind::Fixed { spelling, .. },
                    MaterializedElement::FixedToken { .. },
                ) => Ok(format!("{spelling:?}")),
                (
                    ElementParseKind::Semantic {
                        container,
                        attachment: planned_attachment,
                        ..
                    },
                    MaterializedElement::SemanticField {
                        ty,
                        attachment,
                        separator,
                        ..
                    },
                ) => {
                    if planned_attachment.is_some() != attachment.is_some() {
                        return Err(
                            "a diagram attachment diverges from the frozen parse plan".to_owned()
                        );
                    }
                    let terminal = diagram_terminal_name(ty, lowered)?;
                    let terminal = attachment.as_ref().map_or_else(
                        || Ok(terminal.clone()),
                        |attachment| diagram_attachment_unit(&terminal, attachment, lowered),
                    )?;
                    let separator = diagram_separator_spelling(separator.as_ref(), lowered)?;
                    diagram_semantic_element(&terminal, separator.as_deref(), container)
                }
                _ => Err("a diagram element and its frozen materialization disagree".to_owned()),
            },
        )
        .collect::<Result<Vec<_>, String>>()
        .map(|elements| elements.join(" "))
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_attachment_unit(
    terminal: &str,
    attachment: &MaterializedFixedAttachment,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    let source = lowered.source_modeled.source_model();
    let spelling = |token_use: crate::lowering::TokenUseId| {
        let usage = &source.token_uses[token_use.index()];
        let definition = &source.token_definitions[usage.definition.index()];
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            return Err("a diagram attachment is not fixed syntax".to_owned());
        };
        Ok(format!("{:?}", fixed.spelling))
    };
    attachment
        .before
        .iter()
        .map(|token_use| spelling(*token_use))
        .chain(std::iter::once(Ok(terminal.to_owned())))
        .chain(
            attachment
                .after
                .iter()
                .map(|token_use| spelling(*token_use)),
        )
        .collect::<Result<Vec<_>, _>>()
        .map(|elements| elements.join(" "))
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_semantic_element(
    terminal: &str,
    separator: Option<&str>,
    container: &PlannedContainer,
) -> Result<String, String> {
    match container {
        PlannedContainer::Direct { .. } => {
            if separator.is_some() {
                return Err("a direct diagram element unexpectedly has a separator".to_owned());
            }
            Ok(terminal.to_owned())
        }
        PlannedContainer::Optional { .. } => {
            if separator.is_some() {
                return Err("an optional diagram element unexpectedly has a separator".to_owned());
            }
            Ok(format!("Option<{terminal}>"))
        }
        PlannedContainer::Repeated {
            operation,
            separator_selector,
            optional_trailing,
            ..
        } => {
            if separator.is_some() != separator_selector.is_some() {
                return Err(
                    "a diagram separator diverges from the frozen container machine".to_owned(),
                );
            }
            if *optional_trailing && separator.is_none() {
                return Err("a diagram has optional trailing syntax without a separator".to_owned());
            }
            let mut rendered = match operation {
                ContainerOperation::ZeroOrMore => format!("Vec<{terminal}>"),
                ContainerOperation::OneOrMore => format!("Vec1<{terminal}>"),
                ContainerOperation::OptionalOneOrMore => {
                    format!("Option<Vec1<{terminal}>>")
                }
                ContainerOperation::Direct | ContainerOperation::Optional => {
                    return Err("a repeated diagram element has a scalar operation".to_owned());
                }
            };
            if let Some(separator) = separator {
                rendered.push_str(&format!(" separated by {separator:?}"));
                if *optional_trailing {
                    rendered.push_str(&format!(" (optional trailing {separator:?})"));
                }
            }
            Ok(rendered)
        }
    }
}
#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_aggregate_diagram_document(plan: &DiagramPlan) -> String {
    let mut document = format!(
        "# Generated grammar diagram\n\nRECURSA_GRAMMAR_DIAGRAM\n\nGrammar `{}` is rendered from the frozen semantic Generation plan.\n\n",
        plan.grammar_name
    );
    for node in &plan.nodes {
        document.push_str(&diagram_svg(&node.production, &node.semantic_key));
        document.push_str("\n\n");
    }
    document.push_str("## Public Nodes\n\n");
    for node in plan.nodes.iter().filter(|node| node.public_path.is_some()) {
        let path = node.public_path.as_deref().expect("filtered public path");
        document.push_str(&format!("- [`{}`]({path})\n", node.semantic_key));
    }
    document.push_str(
        "\n## Fixed tokens\n\nFixed tokens are syntax-only: they shape accepted input but do not create AST fields or retain matched text. Their canonical spellings are:\n\n",
    );
    for (name, spelling) in &plan.fixed_tokens {
        document.push_str(&format!("- `{name}`: `{spelling}`\n"));
    }
    document
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_node_diagram_document(node: &NodeDiagramPlan) -> String {
    format!(
        "## Generated grammar diagram\n\nRECURSA_NODE_DIAGRAM: {}\n\n{}\n\nFixed tokens shown as quoted terminals are syntax-only. They shape parsing but do not add fields to the authored Node or retain matched text.\n",
        node.ident,
        diagram_svg(&node.production, &node.semantic_key),
    )
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn diagram_svg(production: &str, label: &str) -> String {
    let width = 40_usize
        .saturating_add(production.chars().count().saturating_mul(9))
        .clamp(240, 1400);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"52\" role=\"img\" aria-label=\"{} grammar diagram\"><rect x=\"1\" y=\"1\" width=\"{}\" height=\"50\" rx=\"5\" fill=\"none\" stroke=\"currentColor\"/><text x=\"16\" y=\"32\" font-family=\"monospace\" fill=\"currentColor\">{}</text></svg>",
        escape_html(label),
        width.saturating_sub(2),
        escape_html(production),
    )
}

#[cfg(feature = "diagrams")]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
