//! Semantic railroad-diagram planning from the frozen generation plan.
//!
//! The owned topology is derived from the frozen parser plan and published as
//! an encoded [`DiagramPlan`]; `recursa-diagrams` renders it to Rustdoc-safe
//! SVG only while `cargo doc` expands generated attributes. Keeping planning
//! here prevents visual grammar semantics from drifting away from the
//! authoritative generation plan.
//!
//! Planning follows the strategy proven by pg-proto's protocol railroads:
//! reference labels drop their module qualifiers so boxes stay compact, and the
//! first reference to another same-grammar Node application is expanded inline
//! as a labeled box, while every later or recursive reference stays an opaque
//! linked box. Both choices only change presentation; the diagram plan remains
//! derived from the frozen generation inputs.

use super::*;

pub(super) use recursa_diagrams::{
    DiagramNode, DiagramPlan, NodeDiagramPlan, TokenDiagramPlan, rustdoc_path,
};

#[derive(Clone, Debug)]
/// One Node's identity and per-application productions before inline expansion.
struct PlannedNodeProductions {
    /// Stable Node identity used to correlate documentation artifacts.
    semantic_key: String,
    /// Generated Rust identifier labeling the production.
    ident: String,
    /// Rustdoc-relative link target when the Node is publicly reachable.
    public_path: Option<String>,
    /// Application display names with their shallow productions in name order.
    productions: Vec<(String, DiagramNode)>,
}

#[allow(clippy::too_many_arguments)]
/// Derives complete Node and content-token topology from frozen generation inputs.
pub(super) fn plan_diagrams(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
    nodes: &[NodeParsePlan],
    tokens: &[TokenParsePlan],
    token_bindings: &[TokenBindingPlan],
    kind_names: &[String],
    kind_static_texts: &[Option<String>],
) -> Result<DiagramPlan, String> {
    let anchor_depth = resolved.build_inputs.module_anchor.segments.len();
    let mut links = BTreeMap::<String, Option<String>>::new();
    for node in nodes {
        let source = resolved
            .nodes
            .iter()
            .find(|source| source.key.segments.join("::") == node.semantic_key)
            .ok_or_else(|| format!("diagram plan cannot correlate Node `{}`", node.semantic_key))?;
        let link = node_is_publicly_linkable(resolved, source)
            .then(|| rustdoc_path(&node.semantic_key, node_rustdoc_kind(node), anchor_depth));
        for application in lowered
            .applications
            .iter()
            .filter(|item| item.node == source.id)
        {
            merge_link(&mut links, &application.display_name, link.clone());
        }
        merge_link(&mut links, &node.ident.unraw().to_string(), link);
    }

    let planned_nodes = nodes
        .iter()
        .map(|node| {
            let source = resolved
                .nodes
                .iter()
                .find(|source| source.key.segments.join("::") == node.semantic_key)
                .ok_or_else(|| {
                    format!("diagram plan cannot correlate Node `{}`", node.semantic_key)
                })?;
            let public_path = node_is_publicly_linkable(resolved, source)
                .then(|| rustdoc_path(&node.semantic_key, node_rustdoc_kind(node), anchor_depth));
            let applications = lowered
                .applications
                .iter()
                .filter(|item| item.node == source.id)
                .collect::<Vec<_>>();
            if applications.len() != node.parse_applications.len() {
                return Err(format!(
                    "diagram plan cannot correlate every application for `{}`",
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
                            &planned.body,
                            materialized,
                            lowered,
                            &links,
                            public_path.as_deref(),
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            productions.sort_by(|left, right| left.0.cmp(&right.0));
            Ok(PlannedNodeProductions {
                semantic_key: node.semantic_key.clone(),
                ident: node.ident.unraw().to_string(),
                public_path,
                productions,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut application_bodies = BTreeMap::new();
    for planned in &planned_nodes {
        for (name, body) in &planned.productions {
            application_bodies.insert(name.clone(), body.clone());
        }
    }
    let nodes = planned_nodes
        .into_iter()
        .map(|planned| {
            let PlannedNodeProductions {
                semantic_key,
                ident,
                public_path,
                mut productions,
            } = planned;
            let mut expanded = productions
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<BTreeSet<_>>();
            let production = if productions.len() == 1 {
                productions.pop().expect("one production").1
            } else {
                DiagramNode::Choice(
                    productions
                        .into_iter()
                        .map(|(name, body)| {
                            sequence(vec![DiagramNode::Comment(shorten_label(&name)), body])
                        })
                        .collect(),
                )
            };
            let mut budget = INLINE_EXPANSION_BUDGET;
            let production =
                inline_references(&production, &application_bodies, &mut expanded, &mut budget);
            NodeDiagramPlan {
                semantic_key,
                ident,
                public_path,
                production,
            }
        })
        .collect::<Vec<_>>();

    let source = lowered.source_modeled.source_model();
    let tokens = tokens
        .iter()
        .enumerate()
        .map(|(token_index, token)| {
            let (definition, content) = source
                .token_definitions
                .iter()
                .find_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(content) if content.semantic == token.semantic => {
                        Some((definition, content))
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    format!(
                        "diagram plan cannot correlate content token {}",
                        token.ident
                    )
                })?;
            let label = content
                .diagram_label
                .clone()
                .unwrap_or_else(|| content.pattern.clone());
            let binding = token_bindings
                .iter()
                .find(|binding| binding.token == token_index);
            let public_path = token_bindings
                .iter()
                .find(|binding| binding.token == token_index && binding.publicly_materialized)
                .map(|binding| rustdoc_path(&binding.semantic_key, "type", anchor_depth));
            let ident = binding.map_or_else(
                || definition.name.clone(),
                |binding| binding.ident.unraw().to_string(),
            );
            let semantic_key = binding.map_or_else(
                || definition.name.clone(),
                |binding| binding.semantic_key.clone(),
            );
            Ok(TokenDiagramPlan {
                semantic_key,
                ident,
                pattern: content.pattern.clone(),
                label: label.clone(),
                public_path: public_path.clone(),
                production: DiagramNode::NonTerminal {
                    label,
                    link: public_path,
                    reference: None,
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let fixed_tokens = kind_names
        .iter()
        .zip(kind_static_texts)
        .filter_map(|(name, spelling)| spelling.as_ref().map(|value| (name.clone(), value.clone())))
        .collect();
    Ok(DiagramPlan {
        grammar_name: resolved.build_inputs.module_anchor.segments.join("::"),
        anchor_depth,
        nodes,
        tokens,
        fixed_tokens,
    })
}

/// Adds a label-to-link mapping and suppresses ambiguous public targets.
fn merge_link(links: &mut BTreeMap<String, Option<String>>, label: &str, link: Option<String>) {
    match links.entry(label.to_owned()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(link);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) if entry.get() != &link => {
            entry.insert(None);
        }
        std::collections::btree_map::Entry::Occupied(_) => {}
    }
}

/// Returns the Rustdoc item kind for one generated Node's Rust type.
///
/// Enum parse bodies generate Rust enums; struct bodies generate structs. Links must name the
/// kind Rustdoc uses in the page filename.
fn node_rustdoc_kind(node: &NodeParsePlan) -> &'static str {
    match node
        .parse_applications
        .first()
        .map(|application| &application.body)
    {
        Some(NodeBodyPlan::Struct { .. }) => "struct",
        _ => "enum",
    }
}

/// Reports whether a Node and every containing module are publicly reachable.
fn node_is_publicly_linkable(
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

/// Projects one materialized Node application into semantic diagram topology.
fn diagram_production(
    body: &NodeBodyPlan,
    application: &MaterializedNodeApplication,
    lowered: &crate::materialization::LoweredGrammar,
    links: &BTreeMap<String, Option<String>>,
    variant_anchor: Option<&str>,
) -> Result<DiagramNode, String> {
    match (body, &application.body) {
        (
            NodeBodyPlan::Struct { elements, .. },
            MaterializedNodeBody::Struct(materialized)
            | MaterializedNodeBody::Presence(materialized),
        ) => diagram_sequence(elements, materialized, lowered, links),
        (NodeBodyPlan::Enum { variants, .. }, MaterializedNodeBody::Enum(materialized)) => {
            if variants.len() != materialized.len() {
                return Err("diagram enum lost a materialized variant".to_owned());
            }
            variants
                .iter()
                .zip(materialized)
                .map(|(variant, materialized)| {
                    let ident = variant.ident.unraw().to_string();
                    Ok(sequence(vec![
                        DiagramNode::LinkedComment {
                            link: variant_anchor.map(|base| format!("{base}#variant.{ident}")),
                            label: ident,
                        },
                        diagram_sequence(
                            &variant.elements,
                            &materialized.elements,
                            lowered,
                            links,
                        )?,
                    ]))
                })
                .collect::<Result<Vec<_>, String>>()
                .map(DiagramNode::Choice)
        }
        _ => Err("diagram and materialized Node shapes disagree".to_owned()),
    }
}

/// Returns a semantic diagram element unchanged.
fn annotate_field(
    element: DiagramNode,
    _field: crate::resolution::ResolvedFieldId,
    _lowered: &crate::materialization::LoweredGrammar,
) -> DiagramNode {
    element
}

/// Projects fixed and semantic elements in their frozen parse order.
fn diagram_sequence(
    elements: &[ElementParsePlan],
    materialized: &[MaterializedElement],
    lowered: &crate::materialization::LoweredGrammar,
    links: &BTreeMap<String, Option<String>>,
) -> Result<DiagramNode, String> {
    if elements.len() != materialized.len() {
        return Err("diagram sequence lost an element".to_owned());
    }
    elements
        .iter()
        .zip(materialized)
        .map(
            |(element, materialized)| match (&element.kind, materialized) {
                (
                    ElementParseKind::Fixed { spelling, .. },
                    MaterializedElement::FixedToken { .. },
                ) => Ok(DiagramNode::Terminal(spelling.clone())),
                (
                    ElementParseKind::OptionalFixedTokenGroup { .. },
                    MaterializedElement::OptionalFixedTokenGroup { tokens, .. },
                ) => Ok(DiagramNode::Optional(Box::new(diagram_fixed_sequence(
                    tokens, lowered,
                )?))),
                (
                    ElementParseKind::Semantic {
                        projection: semantic,
                        ..
                    },
                    MaterializedElement::SemanticField {
                        field,
                        projection:
                            MaterializedSemanticFieldProjection::Direct { ty, separator, .. },
                        attachment,
                        ..
                    },
                ) => {
                    let SemanticParseProjection::Direct {
                        container,
                        attachment: planned,
                        ..
                    } = semantic.as_ref()
                    else {
                        return Err("diagram semantic projections disagree".to_owned());
                    };
                    if planned.is_some() != attachment.is_some() {
                        return Err("diagram attachment diverges from parse plan".to_owned());
                    }
                    let label = diagram_terminal_name(ty, lowered)?;
                    let reference = closed_type_targets_node(ty);
                    let node = if let Some(value) = attachment {
                        diagram_attachment_unit(
                            non_terminal(label, reference, links),
                            value,
                            lowered,
                        )?
                    } else {
                        non_terminal(label, reference, links)
                    };
                    let separator = diagram_separator_spelling(separator.as_ref(), lowered)?
                        .map(DiagramNode::Terminal);
                    diagram_semantic_element(node, separator, container)
                        .map(|element| annotate_field(element, *field, lowered))
                }
                (
                    ElementParseKind::Semantic {
                        projection: semantic,
                        ..
                    },
                    MaterializedElement::SemanticField {
                        projection: MaterializedSemanticFieldProjection::FixedPresence { tokens },
                        attachment,
                        ..
                    },
                ) => {
                    if !matches!(
                        semantic.as_ref(),
                        SemanticParseProjection::FixedPresence { .. }
                    ) {
                        return Err("diagram presence projections disagree".to_owned());
                    }
                    let optional =
                        DiagramNode::Optional(Box::new(diagram_fixed_sequence(tokens, lowered)?));
                    if let Some(attachment) = attachment {
                        diagram_attachment_unit(optional, attachment, lowered)
                    } else {
                        Ok(optional)
                    }
                }
                _ => Err("diagram element and materialization disagree".to_owned()),
            },
        )
        .collect::<Result<Vec<_>, String>>()
        .map(sequence)
}

/// Renders one fixed-token sequence from materialized token uses.
fn diagram_fixed_sequence(
    tokens: &[TokenUseId],
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<DiagramNode, String> {
    let source = lowered.source_modeled.source_model();
    tokens
        .iter()
        .map(|token| {
            let use_ = &source.token_uses[token.index()];
            let TokenDefinitionKind::Fixed(fixed) =
                &source.token_definitions[use_.definition.index()].kind
            else {
                return Err("diagram sequence is not fixed syntax".to_owned());
            };
            Ok(DiagramNode::Terminal(fixed.spelling.clone()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(sequence)
}

/// Constructs a semantic reference and retains a unique public link when known.
///
/// `reference` carries the full application display name for Node targets so
/// inline expansion can correlate the reference; content tokens pass `None`.
fn non_terminal(
    name: String,
    reference: bool,
    links: &BTreeMap<String, Option<String>>,
) -> DiagramNode {
    DiagramNode::NonTerminal {
        link: links.get(&name).cloned().flatten(),
        label: shorten_label(&name),
        reference: reference.then_some(name),
    }
}

/// Reports whether a closed type ultimately references a Node application.
fn closed_type_targets_node(ty: &ClosedType) -> bool {
    match ty {
        ClosedType::Node(_) => true,
        ClosedType::ContentToken(_) => false,
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => closed_type_targets_node(inner),
    }
}

/// Drops module qualifiers from every path inside a display name.
///
/// `grammar::Wrap<Vec<grammar::Name>>` becomes `Wrap<Vec<Name>>`; links keep
/// carrying the precise identity, so equal short labels stay unambiguous.
fn shorten_label(label: &str) -> String {
    let mut output = String::with_capacity(label.len());
    let mut segment = String::new();
    let mut characters = label.chars().peekable();
    while let Some(character) = characters.next() {
        if character.is_alphanumeric() || character == '_' {
            segment.push(character);
        } else if character == ':' && characters.peek() == Some(&':') && !segment.is_empty() {
            characters.next();
            segment.clear();
        } else {
            output.push_str(&segment);
            segment.clear();
            output.push(character);
        }
    }
    output.push_str(&segment);
    output
}

/// Most inline expansions one diagram may perform before references stay linked.
///
/// First-use inlining alone bounds each diagram to one expansion per
/// application, but on a large grammar that is still the whole reachable
/// subgraph: every diagram re-expands it, and railroad layout over thousands
/// of inlined applications per diagram made generation effectively hang.
/// The budget keeps diagrams readable and generation linear; small fixture
/// grammars stay below it, so their rendering is unchanged.
const INLINE_EXPANSION_BUDGET: usize = 16;

/// Largest shallow body, counted in leaves, that still expands inline.
///
/// pg-proto's protocol states are all small, so its strategy never states this
/// bound; grammar productions are not. A large production (an expression enum
/// with hundreds of alternatives) inlined into every referencing diagram is
/// unreadable and multiplies railroad's per-render layout cost, so references
/// to bodies above this size stay opaque linked boxes.
const INLINE_BODY_LEAF_LIMIT: usize = 12;

/// Counts presentation leaves to decide whether a body is small enough to inline.
fn diagram_leaf_count(node: &DiagramNode) -> usize {
    match node {
        DiagramNode::Empty
        | DiagramNode::Terminal(_)
        | DiagramNode::NonTerminal { .. }
        | DiagramNode::Comment(_)
        | DiagramNode::LinkedComment { .. } => 1,
        DiagramNode::Inline { body, .. } | DiagramNode::Annotated { body, .. } => {
            diagram_leaf_count(body)
        }
        DiagramNode::Sequence(nodes) | DiagramNode::Choice(nodes) => {
            nodes.iter().map(diagram_leaf_count).sum()
        }
        DiagramNode::Optional(node) => diagram_leaf_count(node),
        DiagramNode::Repeat { item, separator } => {
            diagram_leaf_count(item) + diagram_leaf_count(separator)
        }
    }
}

/// Expands each first same-grammar Node reference in place, linking later ones.
///
/// This ports pg-proto's railroad strategy: `expanded` starts with the diagram's
/// own applications, every newly seen reference is inserted before its body is
/// expanded, and any reference already in the set stays an opaque linked box.
/// The set therefore bounds each diagram to one expansion per application and
/// terminates recursive grammars; `budget` additionally caps the total number
/// of expansions per diagram in deterministic traversal order, and only bodies
/// within [`INLINE_BODY_LEAF_LIMIT`] expand at all.
fn inline_references(
    node: &DiagramNode,
    bodies: &BTreeMap<String, DiagramNode>,
    expanded: &mut BTreeSet<String>,
    budget: &mut usize,
) -> DiagramNode {
    let planned = plan_inline_depths(node, bodies, expanded, budget);
    rewrite_planned_references(node, bodies, &planned, expanded, 0)
}

/// Chooses which references inline and at which depth, spending breadth-first.
///
/// Depth-ordered spending guarantees a diagram shows its own structure one
/// level down before any grandchild expands: every admissible depth-one
/// reference is admitted before the first depth-two candidate is considered,
/// so deep sub-trees cannot starve a node's own top-level clauses. Within one
/// depth, candidates are admitted in traversal order.
///
/// Depth zero — every production's own direct references, which for a
/// multi-variant Node means every variant's own immediate structure — is
/// never budget-limited. `root` for such a Node is the single `Choice` of
/// all its variants, so a shared, order-sensitive budget applied here would
/// let whichever variants happen to enumerate their references first exhaust
/// it, leaving every later variant a bare opaque link with no expansion at
/// all; a variant's own first level of detail must not depend on how many
/// unrelated sibling variants precede it. `budget` still bounds every deeper
/// level exactly as before, which is what actually protects generation time
/// against runaway recursive drill-down.
fn plan_inline_depths(
    root: &DiagramNode,
    bodies: &BTreeMap<String, DiagramNode>,
    seeded: &BTreeSet<String>,
    budget: &mut usize,
) -> BTreeMap<String, usize> {
    let mut planned = BTreeMap::new();
    let mut level = vec![root];
    let mut depth = 0usize;
    loop {
        if level.is_empty() || (depth > 0 && *budget == 0) {
            break;
        }
        let mut candidates = Vec::new();
        for node in level {
            collect_body_references(node, &mut candidates);
        }
        let mut admitted = Vec::new();
        for reference in candidates {
            if depth > 0 && *budget == 0 {
                break;
            }
            if seeded.contains(&reference) || planned.contains_key(&reference) {
                continue;
            }
            let Some(body) = bodies.get(&reference) else {
                continue;
            };
            if diagram_leaf_count(body) > INLINE_BODY_LEAF_LIMIT {
                continue;
            }
            planned.insert(reference, depth);
            if depth > 0 {
                *budget -= 1;
            }
            admitted.push(body);
        }
        level = admitted;
        depth += 1;
    }
    planned
}

/// Collects same-grammar reference names from one shallow body in traversal order.
fn collect_body_references(node: &DiagramNode, references: &mut Vec<String>) {
    match node {
        DiagramNode::NonTerminal {
            reference: Some(reference),
            ..
        } => references.push(reference.clone()),
        DiagramNode::Empty
        | DiagramNode::Terminal(_)
        | DiagramNode::NonTerminal { .. }
        | DiagramNode::Comment(_)
        | DiagramNode::LinkedComment { .. } => {}
        DiagramNode::Inline { body, .. } | DiagramNode::Annotated { body, .. } => {
            collect_body_references(body, references);
        }
        DiagramNode::Sequence(nodes) | DiagramNode::Choice(nodes) => {
            for node in nodes {
                collect_body_references(node, references);
            }
        }
        DiagramNode::Optional(node) => collect_body_references(node, references),
        DiagramNode::Repeat { item, separator } => {
            collect_body_references(item, references);
            collect_body_references(separator, references);
        }
    }
}

/// Expands each planned reference at its planned depth, linking every other one.
///
/// The rewrite walks the same tree the planner walked, so a reference planned
/// at a depth always has an occurrence there; occurrences at other depths, and
/// later occurrences at the same depth, stay opaque linked boxes.
fn rewrite_planned_references(
    node: &DiagramNode,
    bodies: &BTreeMap<String, DiagramNode>,
    planned: &BTreeMap<String, usize>,
    expanded: &mut BTreeSet<String>,
    depth: usize,
) -> DiagramNode {
    match node {
        DiagramNode::NonTerminal {
            label,
            link,
            reference: Some(reference),
        } if planned.get(reference) == Some(&depth) && expanded.insert(reference.clone()) => {
            DiagramNode::Inline {
                label: label.clone(),
                link: link.clone(),
                body: Box::new(rewrite_planned_references(
                    &bodies[reference],
                    bodies,
                    planned,
                    expanded,
                    depth + 1,
                )),
            }
        }
        DiagramNode::Empty
        | DiagramNode::Terminal(_)
        | DiagramNode::NonTerminal { .. }
        | DiagramNode::Comment(_)
        | DiagramNode::LinkedComment { .. } => node.clone(),
        DiagramNode::Inline { label, link, body } => DiagramNode::Inline {
            label: label.clone(),
            link: link.clone(),
            body: Box::new(rewrite_planned_references(
                body,
                bodies,
                planned,
                expanded,
                depth + 1,
            )),
        },
        DiagramNode::Annotated { note, body } => DiagramNode::Annotated {
            note: note.clone(),
            body: Box::new(rewrite_planned_references(
                body, bodies, planned, expanded, depth,
            )),
        },
        DiagramNode::Sequence(nodes) => DiagramNode::Sequence(
            nodes
                .iter()
                .map(|node| rewrite_planned_references(node, bodies, planned, expanded, depth))
                .collect(),
        ),
        DiagramNode::Choice(nodes) => DiagramNode::Choice(
            nodes
                .iter()
                .map(|node| rewrite_planned_references(node, bodies, planned, expanded, depth))
                .collect(),
        ),
        DiagramNode::Optional(node) => DiagramNode::Optional(Box::new(rewrite_planned_references(
            node, bodies, planned, expanded, depth,
        ))),
        DiagramNode::Repeat { item, separator } => DiagramNode::Repeat {
            item: Box::new(rewrite_planned_references(
                item, bodies, planned, expanded, depth,
            )),
            separator: Box::new(rewrite_planned_references(
                separator, bodies, planned, expanded, depth,
            )),
        },
    }
}

/// Wraps a semantic production with its exact before/after fixed-token syntax.
fn diagram_attachment_unit(
    node: DiagramNode,
    attachment: &MaterializedFixedAttachment,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<DiagramNode, String> {
    let source = lowered.source_modeled.source_model();
    let terminal = |token_use: TokenUseId| {
        let usage = &source.token_uses[token_use.index()];
        let TokenDefinitionKind::Fixed(fixed) =
            &source.token_definitions[usage.definition.index()].kind
        else {
            return Err("diagram attachment is not fixed syntax".to_owned());
        };
        Ok(DiagramNode::Terminal(fixed.spelling.clone()))
    };
    let syntax = |syntax: &crate::materialization::MaterializedAttachedFixedSyntax| match syntax {
        crate::materialization::MaterializedAttachedFixedSyntax::Required(token) => {
            terminal(*token)
        }
        crate::materialization::MaterializedAttachedFixedSyntax::Optional { tokens, .. } => tokens
            .iter()
            .map(|token| terminal(*token))
            .collect::<Result<Vec<_>, _>>()
            .map(sequence)
            .map(Box::new)
            .map(DiagramNode::Optional),
    };
    attachment
        .before
        .iter()
        .map(syntax)
        .chain(std::iter::once(Ok(node)))
        .chain(attachment.after.iter().map(syntax))
        .collect::<Result<Vec<_>, _>>()
        .map(sequence)
}

/// Applies optional or repeated container cardinality and separator topology.
fn diagram_semantic_element(
    node: DiagramNode,
    separator: Option<DiagramNode>,
    container: &PlannedContainer,
) -> Result<DiagramNode, String> {
    match container {
        PlannedContainer::Direct if separator.is_none() => Ok(node),
        PlannedContainer::Optional if separator.is_none() => {
            Ok(DiagramNode::Optional(Box::new(node)))
        }
        PlannedContainer::Repeated {
            operation,
            has_separator,
            optional_trailing,
            ..
        } => {
            if separator.is_some() != *has_separator {
                return Err("diagram separator diverges from container".to_owned());
            }
            if *optional_trailing && separator.is_none() {
                return Err("optional trailing syntax lacks a separator".to_owned());
            }
            let separator = separator.unwrap_or(DiagramNode::Empty);
            let repeated = DiagramNode::Repeat {
                item: Box::new(node),
                separator: Box::new(separator.clone()),
            };
            let repeated = match operation {
                ContainerOperation::ZeroOrMore => sequence(vec![
                    DiagramNode::Comment("0+".to_owned()),
                    DiagramNode::Optional(Box::new(repeated)),
                ]),
                ContainerOperation::OneOrMore => {
                    sequence(vec![DiagramNode::Comment("1+".to_owned()), repeated])
                }
                ContainerOperation::OptionalOneOrMore => sequence(vec![
                    DiagramNode::Comment("optional 1+".to_owned()),
                    DiagramNode::Optional(Box::new(repeated)),
                ]),
                _ => return Err("repeated diagram element has a scalar operation".to_owned()),
            };
            Ok(if *optional_trailing {
                sequence(vec![repeated, DiagramNode::Optional(Box::new(separator))])
            } else {
                repeated
            })
        }
        _ => Err("scalar diagram element unexpectedly has a separator".to_owned()),
    }
}
/// Flattens empty and singleton sequences without changing consumption order.
fn sequence(nodes: Vec<DiagramNode>) -> DiagramNode {
    let nodes = nodes
        .into_iter()
        .filter(|node| node != &DiagramNode::Empty)
        .collect::<Vec<_>>();
    match nodes.len() {
        0 => DiagramNode::Empty,
        1 => nodes.into_iter().next().expect("one node"),
        _ => DiagramNode::Sequence(nodes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_diagrams_cover_expression_nodes_content_tokens_and_local_docs() {
        super::super::test_support::semantic_diagrams_cover_expression_nodes_content_tokens_and_local_docs();
    }

    #[test]
    fn semantic_diagrams_preserve_generic_applications_and_repetition_topology() {
        super::super::test_support::semantic_diagrams_preserve_generic_applications_and_repetition_topology();
    }

    #[test]
    fn labels_drop_module_qualifiers_but_keep_type_argument_structure() {
        assert_eq!(shorten_label("grammar::Pair"), "Pair");
        assert_eq!(
            shorten_label("grammar::Wrap<Vec<grammar::Name>>"),
            "Wrap<Vec<Name>>"
        );
        assert_eq!(
            shorten_label("grammar::nested::Deep<a::B, c::D>"),
            "Deep<B, D>"
        );
        assert_eq!(shorten_label("Plain"), "Plain");
    }

    #[test]
    fn first_references_expand_inline_and_later_references_stay_linked() {
        /// Shorthand for a Node-application reference in this test.
        fn reference(name: &str) -> DiagramNode {
            DiagramNode::NonTerminal {
                label: shorten_label(name),
                link: Some(format!("struct.{}.html", shorten_label(name))),
                reference: Some(name.to_owned()),
            }
        }

        let mut bodies = BTreeMap::new();
        bodies.insert(
            "grammar::Root".to_owned(),
            sequence(vec![
                DiagramNode::Terminal("ROOT".into()),
                reference("grammar::Item"),
                reference("grammar::Item"),
                reference("grammar::Root"),
            ]),
        );
        bodies.insert(
            "grammar::Item".to_owned(),
            sequence(vec![
                DiagramNode::Terminal("ITEM".into()),
                reference("grammar::Root"),
            ]),
        );

        let mut expanded = BTreeSet::from(["grammar::Root".to_owned()]);
        let mut budget = INLINE_EXPANSION_BUDGET;
        let inlined = inline_references(
            &bodies["grammar::Root"],
            &bodies,
            &mut expanded,
            &mut budget,
        );
        let DiagramNode::Sequence(elements) = &inlined else {
            panic!("inline expansion preserves the sequence: {inlined:?}");
        };
        let [_, first, second, recursive] = elements.as_slice() else {
            panic!("inline expansion preserves arity: {elements:?}");
        };
        let DiagramNode::Inline { label, link, body } = first else {
            panic!("the first reference expands inline: {first:?}");
        };
        assert_eq!(label, "Item");
        assert_eq!(link.as_deref(), Some("struct.Item.html"));
        let DiagramNode::Sequence(inner) = body.as_ref() else {
            panic!("the inlined body is the referenced production: {body:?}");
        };
        assert!(
            matches!(&inner[1], DiagramNode::NonTerminal { .. }),
            "recursion inside an expansion stays a reference: {inner:?}"
        );
        assert!(
            matches!(second, DiagramNode::NonTerminal { .. }),
            "a later reference stays opaque: {second:?}"
        );
        assert!(
            matches!(recursive, DiagramNode::NonTerminal { .. }),
            "a self-reference stays opaque: {recursive:?}"
        );
    }

    #[test]
    fn an_exhausted_inline_budget_keeps_references_linked() {
        let mut bodies = BTreeMap::new();
        let chain = (0..INLINE_EXPANSION_BUDGET + 4)
            .map(|index| format!("grammar::Chain{index}"))
            .collect::<Vec<_>>();
        for (index, name) in chain.iter().enumerate() {
            let tail = match chain.get(index + 1) {
                Some(next) => DiagramNode::NonTerminal {
                    label: shorten_label(next),
                    link: None,
                    reference: Some(next.clone()),
                },
                None => DiagramNode::Empty,
            };
            bodies.insert(
                name.clone(),
                sequence(vec![DiagramNode::Terminal("T".into()), tail]),
            );
        }

        let mut expanded = BTreeSet::from([chain[0].clone()]);
        let mut budget = INLINE_EXPANSION_BUDGET;
        let inlined = inline_references(&bodies[&chain[0]], &bodies, &mut expanded, &mut budget);
        let mut depth = 0usize;
        let mut node = &inlined;
        loop {
            let DiagramNode::Sequence(elements) = node else {
                panic!("chain bodies stay sequences: {node:?}");
            };
            match &elements[1] {
                DiagramNode::Inline { body, .. } => {
                    depth += 1;
                    node = body;
                }
                DiagramNode::NonTerminal { .. } => break,
                DiagramNode::Empty => panic!("the chain outlives the budget"),
                other => panic!("unexpected chain tail: {other:?}"),
            }
        }
        assert_eq!(
            depth,
            INLINE_EXPANSION_BUDGET + 1,
            "the first (depth-zero) expansion is free, then the rest stop exactly at the budget",
        );
        assert_eq!(budget, 0);
    }

    #[test]
    fn budget_spends_breadth_first_so_shallow_references_inline_before_grandchildren() {
        fn reference(name: &str) -> DiagramNode {
            DiagramNode::NonTerminal {
                label: shorten_label(name),
                link: None,
                reference: Some(name.to_owned()),
            }
        }

        let mut bodies = BTreeMap::new();
        for deep in ["grammar::DeepA", "grammar::DeepB"] {
            let children = (0..8)
                .map(|index| format!("{deep}Child{index}"))
                .collect::<Vec<_>>();
            for child in &children {
                bodies.insert(child.clone(), DiagramNode::Terminal("C".into()));
            }
            bodies.insert(
                deep.to_owned(),
                DiagramNode::Sequence(children.iter().map(|name| reference(name)).collect()),
            );
        }
        bodies.insert(
            "grammar::WhereClause".to_owned(),
            sequence(vec![DiagramNode::Terminal("WHERE".into())]),
        );
        bodies.insert(
            "grammar::Root".to_owned(),
            sequence(vec![
                reference("grammar::DeepA"),
                reference("grammar::DeepB"),
                reference("grammar::WhereClause"),
            ]),
        );

        let mut expanded = BTreeSet::from(["grammar::Root".to_owned()]);
        let mut budget = INLINE_EXPANSION_BUDGET;
        let inlined = inline_references(
            &bodies["grammar::Root"],
            &bodies,
            &mut expanded,
            &mut budget,
        );
        let DiagramNode::Sequence(elements) = &inlined else {
            panic!("the root stays a sequence: {inlined:?}");
        };
        assert!(
            matches!(&elements[0], DiagramNode::Inline { .. }),
            "the first depth-one reference expands: {:?}",
            elements[0],
        );
        assert!(
            matches!(&elements[2], DiagramNode::Inline { label, .. } if label == "WhereClause"),
            "a later depth-one reference expands before any grandchild starves it: {:?}",
            elements[2],
        );
    }

    #[test]
    fn every_variants_own_first_level_expands_regardless_of_variant_count() {
        fn reference(name: &str) -> DiagramNode {
            DiagramNode::NonTerminal {
                label: shorten_label(name),
                link: None,
                reference: Some(name.to_owned()),
            }
        }

        // More variants than the budget, each with its own small referenced production — the
        // real-world shape that starved every variant after the budget ran out.
        let variant_count = INLINE_EXPANSION_BUDGET * 3;
        let mut bodies = BTreeMap::new();
        for index in 0..variant_count {
            bodies.insert(
                format!("grammar::Detail{index}"),
                sequence(vec![DiagramNode::Terminal(format!("D{index}"))]),
            );
        }
        let root = DiagramNode::Choice(
            (0..variant_count)
                .map(|index| {
                    sequence(vec![
                        DiagramNode::Comment(format!("Variant{index}")),
                        reference(&format!("grammar::Detail{index}")),
                    ])
                })
                .collect(),
        );

        let mut expanded = BTreeSet::from(["grammar::Root".to_owned()]);
        let mut budget = INLINE_EXPANSION_BUDGET;
        let inlined = inline_references(&root, &bodies, &mut expanded, &mut budget);
        let DiagramNode::Choice(variants) = &inlined else {
            panic!("the root stays a choice: {inlined:?}");
        };
        for (index, variant) in variants.iter().enumerate() {
            let DiagramNode::Sequence(elements) = variant else {
                panic!("each variant stays a sequence: {variant:?}");
            };
            assert!(
                matches!(&elements[1], DiagramNode::Inline { .. }),
                "variant {index} of {variant_count} shows its own detail, not just the earliest \
                 variants: {:?}",
                elements[1],
            );
        }
        assert_eq!(
            budget, INLINE_EXPANSION_BUDGET,
            "depth-zero expansion — every variant's own first level — never spends the budget",
        );
    }
}
