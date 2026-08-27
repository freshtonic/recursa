//! Semantic railroad-diagram planning and deterministic Rustdoc rendering.
//!
//! The owned topology is derived from the frozen parser plan, then rendered as
//! Rustdoc-safe SVG. Keeping those phases here prevents visual grammar semantics
//! from drifting away from the authoritative generation plan.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar topology independent of railroad layout geometry.
pub(super) enum DiagramNode {
    /// An epsilon path.
    Empty,
    /// Exact fixed syntax.
    Terminal(String),
    /// A semantic reference, linked only when publicly addressable.
    NonTerminal {
        /// Text displayed for the referenced semantic type.
        label: String,
        /// Rustdoc-relative public target, or `None` for private targets.
        link: Option<String>,
    },
    /// A non-consuming branch or cardinality annotation.
    Comment(String),
    /// Elements consumed in source order.
    Sequence(Vec<Self>),
    /// Ordered alternative productions.
    Choice(Vec<Self>),
    /// A production that may be absent.
    Optional(Box<Self>),
    /// A one-or-more production with its separator path.
    Repeat {
        /// Repeated semantic production.
        item: Box<Self>,
        /// Syntax between successive items, or [`DiagramNode::Empty`].
        separator: Box<Self>,
    },
}

#[derive(Clone, Debug)]
/// Grammar-wide diagram projection in canonical node order.
pub(super) struct DiagramPlan {
    /// Display name labeling the aggregate grammar diagram.
    pub(super) grammar_name: String,
    /// Node productions in canonical parse-plan order.
    pub(super) nodes: Vec<NodeDiagramPlan>,
    /// Typed content-token diagrams in canonical token order.
    pub(super) tokens: Vec<TokenDiagramPlan>,
    /// Fixed-token names and canonical spellings in kind order.
    pub(super) fixed_tokens: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
/// Diagram identity and railroad body for one node.
pub(super) struct NodeDiagramPlan {
    /// Stable Node identity used to correlate documentation artifacts.
    pub(super) semantic_key: String,
    /// Generated Rust identifier labeling the production.
    pub(super) ident: String,
    /// Rustdoc-relative link target when the Node is publicly reachable.
    pub(super) public_path: Option<String>,
    /// Semantic production derived from the frozen node application plans.
    pub(super) production: DiagramNode,
}

#[derive(Clone, Debug)]
/// Frozen diagram plan for one typed content-token identity.
pub(super) struct TokenDiagramPlan {
    /// Stable semantic key used for aggregate documentation and debug artifacts.
    pub(super) semantic_key: String,
    /// Generated public token name used by declaration-local Rustdoc.
    pub(super) ident: String,
    /// Authored lexical pattern retained for explanatory documentation.
    pub(super) pattern: String,
    /// Authored diagram label, or the lexical-pattern fallback.
    pub(super) label: String,
    /// Rustdoc-relative target when the token binding is publicly reachable.
    pub(super) public_path: Option<String>,
    /// Semantic token production rendered by railroad.
    pub(super) production: DiagramNode,
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
    let mut links = BTreeMap::<String, Option<String>>::new();
    for node in nodes {
        let source = resolved
            .nodes
            .iter()
            .find(|source| source.key.segments.join("::") == node.semantic_key)
            .ok_or_else(|| format!("diagram plan cannot correlate Node `{}`", node.semantic_key))?;
        let link = node_is_publicly_linkable(resolved, source)
            .then(|| rustdoc_path(&node.semantic_key, "struct"));
        for application in lowered
            .applications
            .iter()
            .filter(|item| item.node == source.id)
        {
            merge_link(&mut links, &application.display_name, link.clone());
        }
        merge_link(&mut links, &node.ident.unraw().to_string(), link);
    }

    let nodes = nodes
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
                .then(|| rustdoc_path(&node.semantic_key, "struct"));
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
                        diagram_production(&planned.body, materialized, lowered, &links)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            productions.sort_by(|left, right| left.0.cmp(&right.0));
            let production = if productions.len() == 1 {
                productions.pop().expect("one production").1
            } else {
                DiagramNode::Choice(
                    productions
                        .into_iter()
                        .map(|(name, body)| sequence(vec![DiagramNode::Comment(name), body]))
                        .collect(),
                )
            };
            Ok(NodeDiagramPlan {
                semantic_key: node.semantic_key.clone(),
                ident: node.ident.unraw().to_string(),
                public_path,
                production,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

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
                .map(|binding| rustdoc_path(&binding.semantic_key, "type"));
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

/// Converts a semantic key into a path relative to the generated grammar module.
fn rustdoc_path(semantic_key: &str, kind: &str) -> String {
    let mut segments = semantic_key.split("::").collect::<Vec<_>>();
    let name = segments.pop().expect("semantic keys are nonempty");
    if !segments.is_empty() {
        segments.remove(0);
    }
    let page = format!("{kind}.{name}.html");
    if segments.is_empty() {
        page
    } else {
        format!("{}/{}", segments.join("/"), page)
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
                    Ok(sequence(vec![
                        DiagramNode::Comment(variant.ident.unraw().to_string()),
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
        (NodeBodyPlan::Pratt(pratt), MaterializedNodeBody::Pratt(materialized)) => {
            diagram_pratt_production(pratt, materialized, lowered, links)
        }
        _ => Err("diagram and materialized Node shapes disagree".to_owned()),
    }
}

/// Projects ordered Pratt branches, roles, and binding powers into a choice.
fn diagram_pratt_production(
    pratt: &PrattParsePlan,
    materialized: &[MaterializedPrattVariant],
    lowered: &crate::materialization::LoweredGrammar,
    links: &BTreeMap<String, Option<String>>,
) -> Result<DiagramNode, String> {
    if pratt.expected_nud_count + pratt.expected_led_count != materialized.len() {
        return Err("Pratt diagram lost a materialized variant".to_owned());
    }
    materialized
        .iter()
        .enumerate()
        .map(|(source_index, materialized)| {
            let (role, name, body) = match materialized {
                MaterializedPrattVariant::Nud(materialized) => {
                    let branch = unique_pratt_nud(pratt, source_index)?;
                    validate_pratt_position(Some(branch.position), materialized)?;
                    (
                        diagram_pratt_role(branch.position),
                        branch.variant.ident.unraw().to_string(),
                        diagram_sequence(
                            &branch.variant.elements,
                            &materialized.body,
                            lowered,
                            links,
                        )?,
                    )
                }
                MaterializedPrattVariant::Led(materialized) => {
                    let branch = unique_pratt_led(pratt, source_index)?;
                    validate_pratt_led_position(Some(branch.position), materialized)?;
                    let lhs = diagram_pratt_lhs(&branch.lhs, materialized, lowered, links)?;
                    let tail = diagram_sequence(
                        &branch.variant.elements,
                        &materialized.tail,
                        lowered,
                        links,
                    )?;
                    (
                        diagram_pratt_role(branch.position),
                        branch.variant.ident.unraw().to_string(),
                        sequence(vec![lhs, tail]),
                    )
                }
            };
            Ok(sequence(vec![
                DiagramNode::Comment(format!("{role} {name}")),
                body,
            ]))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(DiagramNode::Choice)
}

/// Finds the sole prefix/atom parse branch for a materialized source variant.
fn unique_pratt_nud(
    pratt: &PrattParsePlan,
    source_index: usize,
) -> Result<&PrattNudParsePlan, String> {
    let mut matching = pratt
        .nud
        .iter()
        .filter(|branch| branch.source_index == source_index);
    let branch = matching
        .next()
        .ok_or_else(|| "Pratt diagram lacks its nud branch".to_owned())?;
    if matching.next().is_some() {
        return Err("Pratt diagram has duplicate nud branches".to_owned());
    }
    Ok(branch)
}

/// Finds the sole infix/postfix parse branch for a materialized source variant.
fn unique_pratt_led(
    pratt: &PrattParsePlan,
    source_index: usize,
) -> Result<&PrattLedParsePlan, String> {
    let mut matching = pratt
        .led
        .iter()
        .filter(|branch| branch.source_index == source_index);
    let branch = matching
        .next()
        .ok_or_else(|| "Pratt diagram lacks its led branch".to_owned())?;
    if matching.next().is_some() {
        return Err("Pratt diagram has duplicate led branches".to_owned());
    }
    Ok(branch)
}

/// Reconstructs the implicit left operand of an infix or postfix branch.
fn diagram_pratt_lhs(
    lhs: &ElementParsePlan,
    materialized: &MaterializedPrattLed,
    lowered: &crate::materialization::LoweredGrammar,
    links: &BTreeMap<String, Option<String>>,
) -> Result<DiagramNode, String> {
    let ElementParseKind::Semantic {
        container,
        pratt_min_bp,
        ..
    } = &lhs.kind
    else {
        return Err("Pratt diagram lhs is not semantic".to_owned());
    };
    if pratt_min_bp.is_some() {
        return Err("Pratt diagram lhs has a minimum binding power".to_owned());
    }
    let label = diagram_terminal_name(&ClosedType::Node(materialized.lhs.owner.clone()), lowered)?;
    diagram_semantic_element(non_terminal(label, links), None, container)
}

/// Formats a Pratt role without discarding its binding-power thresholds.
fn diagram_pratt_role(position: PrattPosition) -> String {
    match position {
        PrattPosition::Atom => "atom".to_owned(),
        PrattPosition::Prefix { bp } => format!("prefix bp={bp}"),
        PrattPosition::Infix { lbp, rbp } => format!("infix lbp={lbp} rbp={rbp}"),
        PrattPosition::Postfix { bp } => format!("postfix bp={bp}"),
    }
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
                    ElementParseKind::Semantic {
                        container,
                        attachment: planned,
                        ..
                    },
                    MaterializedElement::SemanticField {
                        ty,
                        attachment,
                        separator,
                        ..
                    },
                ) => {
                    if planned.is_some() != attachment.is_some() {
                        return Err("diagram attachment diverges from parse plan".to_owned());
                    }
                    let label = diagram_terminal_name(ty, lowered)?;
                    let node = if let Some(value) = attachment {
                        diagram_attachment_unit(non_terminal(label, links), value, lowered)?
                    } else {
                        non_terminal(label, links)
                    };
                    let separator = diagram_separator_spelling(separator.as_ref(), lowered)?
                        .map(DiagramNode::Terminal);
                    diagram_semantic_element(node, separator, container)
                }
                _ => Err("diagram element and materialization disagree".to_owned()),
            },
        )
        .collect::<Result<Vec<_>, String>>()
        .map(sequence)
}

/// Constructs a semantic reference and retains a unique public link when known.
fn non_terminal(label: String, links: &BTreeMap<String, Option<String>>) -> DiagramNode {
    DiagramNode::NonTerminal {
        link: links.get(&label).cloned().flatten(),
        label,
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
    attachment
        .before
        .iter()
        .map(|id| terminal(*id))
        .chain(std::iter::once(Ok(node)))
        .chain(attachment.after.iter().map(|id| terminal(*id)))
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
        PlannedContainer::Direct { .. } if separator.is_none() => Ok(node),
        PlannedContainer::Optional { .. } if separator.is_none() => {
            Ok(DiagramNode::Optional(Box::new(node)))
        }
        PlannedContainer::Repeated {
            operation,
            separator_selector,
            optional_trailing,
            ..
        } => {
            if separator.is_some() != separator_selector.is_some() {
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

/// Renders the aggregate grammar Rustdoc containing every Node and token diagram.
pub(super) fn render_aggregate_diagram_document(plan: &DiagramPlan) -> String {
    let mut document = format!(
        "# Generated grammar diagrams\n\nRECURSA_GRAMMAR_DIAGRAM\n\nGrammar `{}` is rendered from its frozen semantic Generation plan.\n\n",
        plan.grammar_name
    );
    for node in &plan.nodes {
        document.push_str(&diagram_svg_with_prefix(
            &node.production,
            &node.semantic_key,
            "../",
        ));
        document.push_str("\n\n");
    }
    document.push_str("## Content tokens\n\n");
    for token in &plan.tokens {
        document.push_str(&diagram_svg_with_prefix(
            &token.production,
            &token.semantic_key,
            "../",
        ));
        document.push_str("\n\n");
    }
    document.push_str("## Public grammar types\n\n");
    for (name, path) in plan
        .nodes
        .iter()
        .filter_map(|node| {
            node.public_path
                .as_ref()
                .map(|path| (&node.semantic_key, path))
        })
        .chain(plan.tokens.iter().filter_map(|token| {
            token
                .public_path
                .as_ref()
                .map(|path| (&token.semantic_key, path))
        }))
    {
        document.push_str(&format!("- [`{name}`](../{path})\n"));
    }
    document.push_str(
        "\n## Fixed tokens\n\nFixed tokens are syntax-only and retain no matched text.\n\n",
    );
    for (name, spelling) in &plan.fixed_tokens {
        document.push_str(&format!("- `{name}`: `{spelling}`\n"));
    }
    document
}

/// Renders declaration-local Rustdoc for one generated Node.
pub(super) fn render_node_diagram_document(node: &NodeDiagramPlan) -> String {
    format!(
        "## Generated grammar diagram\n\nRECURSA_NODE_DIAGRAM: {}\n\n{}\n\nFixed-token terminals are syntax-only and do not add AST fields.\n",
        node.ident,
        diagram_svg(&node.production, &node.semantic_key)
    )
}

/// Renders declaration-local Rustdoc for one generated content-token type.
pub(super) fn render_token_diagram_document(token: &TokenDiagramPlan) -> String {
    format!(
        "## Generated token diagram\n\nRECURSA_TOKEN_DIAGRAM: {}\n\n{}\n\nThe label `{}` represents the content-token regex `{}`; parsed values retain exact matched text.\n",
        token.ident,
        diagram_svg(&token.production, &token.semantic_key),
        token.label,
        token.pattern
    )
}

/// Type-erased railroad node used while projecting the owned semantic topology.
type RailroadNode = Box<dyn railroad::Node>;

/// Projects semantic topology into railroad layout primitives.
fn to_railroad(node: &DiagramNode, link_prefix: &str) -> RailroadNode {
    match node {
        DiagramNode::Empty => Box::new(railroad::Empty),
        DiagramNode::Terminal(label) => Box::new(railroad::Terminal::new(label.clone())),
        DiagramNode::NonTerminal { label, link } => {
            let node: RailroadNode = Box::new(railroad::NonTerminal::new(label.clone()));
            match link {
                Some(href) => Box::new(railroad::Link::new(node, format!("{link_prefix}{href}"))),
                None => node,
            }
        }
        DiagramNode::Comment(label) => Box::new(railroad::Comment::new(label.clone())),
        DiagramNode::Sequence(nodes) => Box::new(railroad::Sequence::new(
            nodes
                .iter()
                .map(|node| to_railroad(node, link_prefix))
                .collect(),
        )),
        DiagramNode::Choice(nodes) => Box::new(railroad::Choice::new(
            nodes
                .iter()
                .map(|node| to_railroad(node, link_prefix))
                .collect(),
        )),
        DiagramNode::Optional(node) => {
            Box::new(railroad::Optional::new(to_railroad(node, link_prefix)))
        }
        DiagramNode::Repeat { item, separator } => Box::new(railroad::Repeat::new(
            to_railroad(item, link_prefix),
            to_railroad(separator, link_prefix),
        )),
    }
}

/// Renders one standalone, deterministic, accessible SVG document.
pub(super) fn diagram_svg(production: &DiagramNode, label: &str) -> String {
    diagram_svg_with_prefix(production, label, "")
}

/// Renders SVG while rebasing links for aggregate or declaration-local Rustdoc.
fn diagram_svg_with_prefix(production: &DiagramNode, label: &str, link_prefix: &str) -> String {
    let root = railroad::Sequence::new(vec![
        Box::new(railroad::Start) as RailroadNode,
        to_railroad(production, link_prefix),
        Box::new(railroad::End),
    ]);
    // Rustdoc still recognizes Markdown links inside raw HTML text. Entity-encode
    // brackets after railroad's XML escaping so regex labels remain literal.
    let raw = railroad::Diagram::with_default_css(root)
        .to_string()
        .replace('[', "&#91;")
        .replace(']', "&#93;");
    let title = escape_xml(&format!("{label} grammar diagram"));
    let description = escape_xml(&format!("Railroad topology for {label}"));
    let identifier = diagram_identifier(label);
    let opening_end = raw
        .find('>')
        .expect("railroad emits an opening SVG element");
    let mut svg = String::with_capacity(raw.len() + title.len() + description.len() + 120);
    svg.push_str(&raw[..opening_end]);
    svg.push_str(&format!(
        " role=\"img\" aria-labelledby=\"{identifier}-title {identifier}-description\">"
    ));
    svg.push_str(&format!("<title id=\"{identifier}-title\">{title}</title><desc id=\"{identifier}-description\">{description}</desc>"));
    svg.push_str(&raw[opening_end + 1..]);
    svg.lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<String>()
}

/// Derives a deterministic collision-resistant accessibility identifier.
fn diagram_identifier(label: &str) -> String {
    let hash = label
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("recursa-diagram-{hash:016x}")
}

/// Escapes text inserted into accessibility elements outside railroad's renderer.
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_diagrams_cover_pratt_nodes_content_tokens_and_local_docs() {
        super::super::test_support::semantic_diagrams_cover_pratt_nodes_content_tokens_and_local_docs();
    }

    #[test]
    fn semantic_diagrams_preserve_generic_applications_and_repetition_topology() {
        super::super::test_support::semantic_diagrams_preserve_generic_applications_and_repetition_topology();
    }

    #[test]
    fn railroad_svg_is_accessible_deterministic_escaped_and_topological() {
        let production = DiagramNode::Choice(vec![
            sequence(vec![
                DiagramNode::Terminal("<&[x]".into()),
                DiagramNode::Optional(Box::new(DiagramNode::NonTerminal {
                    label: "Public".into(),
                    link: Some("struct.Public.html?x=1&y=2".into()),
                })),
            ]),
            DiagramNode::Repeat {
                item: Box::new(DiagramNode::NonTerminal {
                    label: "Private".into(),
                    link: None,
                }),
                separator: Box::new(DiagramNode::Terminal(",".into())),
            },
        ]);
        let svg = diagram_svg(&production, "Root<&");
        assert_eq!(svg, diagram_svg(&production, "Root<&"));
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("role=\"img\" aria-labelledby=\"recursa-diagram-"));
        assert!(svg.contains("Root&lt;&amp; grammar diagram</title>"));
        assert!(svg.contains("class=\"choice\""));
        assert!(svg.contains("class=\"optional\""));
        assert!(svg.contains("class=\"repeat\""));
        assert!(svg.contains("&lt;&amp;"));
        assert!(svg.contains("&#91;x&#93;"));
        assert!(svg.contains("href=\"struct.Public.html?x=1&amp;y=2\""));
        assert!(!svg.contains("href=\"Private"));
        assert!(!svg.lines().any(|line| line.trim().is_empty()));
        roxmltree::Document::parse(&svg).expect("railroad output is valid XML");
        assert_ne!(diagram_identifier("Root<&"), diagram_identifier("Other"));
    }
}
