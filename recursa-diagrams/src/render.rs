//! Deterministic Rustdoc rendering of planned diagram topology.
//!
//! Every renderer here is pure: the same plan renders the same bytes in any
//! process, so the doc-time facade and the generator's debug bundle agree.

use crate::{DiagramNode, DiagramPlan, NodeDiagramPlan, TokenDiagramPlan, ascent_to_anchor};

/// Wraps one rendered diagram in a horizontally scrolling Rustdoc container.
///
/// The `<style>` escape frees the page from Rustdoc's width limiter so wide
/// railroads scroll inside their own container instead of clipping.
fn scrollable_diagram(svg: &str) -> String {
    format!(
        "<style>.width-limiter:has(.recursa-railroad) {{ max-width: none; }}</style>\
         <div class=\"recursa-railroad\" style=\"overflow-x: auto\">{svg}</div>"
    )
}

/// Renders the aggregate grammar Rustdoc containing every Node and token diagram.
pub fn render_aggregate_document(plan: &DiagramPlan) -> String {
    let mut document = format!(
        "# Generated grammar diagrams\n\n<!-- RECURSA_GRAMMAR_DIAGRAM -->\n\nGrammar `{}` is rendered from its frozen semantic Generation plan.\n\n",
        plan.grammar_name
    );
    for node in &plan.nodes {
        document.push_str(&scrollable_diagram(&render_svg(
            &node.production,
            &node.semantic_key,
            "../",
        )));
        document.push_str("\n\n");
    }
    document.push_str("## Content tokens\n\n");
    for token in &plan.tokens {
        document.push_str(&scrollable_diagram(&render_svg(
            &token.production,
            &token.semantic_key,
            "../",
        )));
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
///
/// Returns the Rustdoc document and the standalone SVG it embeds.
pub fn render_node_document(node: &NodeDiagramPlan, anchor_depth: usize) -> (String, String) {
    let svg = render_svg(
        &node.production,
        &node.semantic_key,
        &ascent_to_anchor(&node.semantic_key, anchor_depth),
    );
    let document = format!(
        "<!-- RECURSA_NODE_DIAGRAM: {} -->\n{}\n\nFixed-token terminals are syntax-only and do not add AST fields. Boxed subdiagrams expand a referenced production in place; repeated references stay opaque.\n",
        node.ident,
        scrollable_diagram(&svg)
    );
    (document, svg)
}

/// Renders declaration-local Rustdoc for one generated content-token type.
///
/// Returns the Rustdoc document and the standalone SVG it embeds.
pub fn render_token_document(token: &TokenDiagramPlan, anchor_depth: usize) -> (String, String) {
    let svg = render_svg(
        &token.production,
        &token.semantic_key,
        &ascent_to_anchor(&token.semantic_key, anchor_depth),
    );
    let document = format!(
        "<!-- RECURSA_TOKEN_DIAGRAM: {} -->\n{}\n\nThe label `{}` represents the content-token regex `{}`; parsed values retain exact matched text.\n",
        token.ident,
        scrollable_diagram(&svg),
        token.label,
        token.pattern
    );
    (document, svg)
}

/// Type-erased railroad node used while projecting the owned semantic topology.
type RailroadNode = Box<dyn railroad::Node>;

/// Caches railroad measurements so nested container layout stays linear.
///
/// Railroad containers re-measure children on every geometry query and again
/// while drawing. Over deep inline trees those repeated queries compound
/// multiplicatively, which made rendering a large grammar effectively hang;
/// caching each node's three measurements restores one computation per node.
struct CachedMeasure {
    /// Wrapped railroad node supplying geometry and drawing.
    inner: RailroadNode,
    /// Entry height computed once on first query.
    entry_height: std::cell::Cell<Option<i64>>,
    /// Total height computed once on first query.
    height: std::cell::Cell<Option<i64>>,
    /// Total width computed once on first query.
    width: std::cell::Cell<Option<i64>>,
}

impl CachedMeasure {
    /// Wraps one railroad node so each measurement computes at most once.
    fn wrap(inner: RailroadNode) -> RailroadNode {
        Box::new(Self {
            inner,
            entry_height: std::cell::Cell::new(None),
            height: std::cell::Cell::new(None),
            width: std::cell::Cell::new(None),
        })
    }

    /// Returns the cached measurement, computing and storing it on first use.
    fn measure(cell: &std::cell::Cell<Option<i64>>, compute: impl FnOnce() -> i64) -> i64 {
        match cell.get() {
            Some(value) => value,
            None => {
                let value = compute();
                cell.set(Some(value));
                value
            }
        }
    }
}

impl railroad::Node for CachedMeasure {
    fn entry_height(&self) -> i64 {
        Self::measure(&self.entry_height, || self.inner.entry_height())
    }

    fn height(&self) -> i64 {
        Self::measure(&self.height, || self.inner.height())
    }

    fn width(&self) -> i64 {
        Self::measure(&self.width, || self.inner.width())
    }

    fn draw(&self, x: i64, y: i64, h_dir: railroad::svg::HDir) -> railroad::svg::Element {
        self.inner.draw(x, y, h_dir)
    }
}

/// Projects semantic topology into railroad layout primitives.
fn to_railroad(node: &DiagramNode, link_prefix: &str) -> RailroadNode {
    let node = to_railroad_uncached(node, link_prefix);
    CachedMeasure::wrap(node)
}

/// Builds the railroad node for one topology node with cached children.
fn to_railroad_uncached(node: &DiagramNode, link_prefix: &str) -> RailroadNode {
    match node {
        DiagramNode::Empty => Box::new(railroad::Empty),
        DiagramNode::Terminal(label) => Box::new(railroad::Terminal::new(label.clone())),
        DiagramNode::NonTerminal { label, link, .. } => {
            let node: RailroadNode = Box::new(railroad::NonTerminal::new(label.clone()));
            match link {
                Some(href) => Box::new(railroad::Link::new(node, format!("{link_prefix}{href}"))),
                None => node,
            }
        }
        DiagramNode::Inline { label, link, body } => {
            let name: RailroadNode = Box::new(railroad::Comment::new(label.clone()));
            let name: RailroadNode = match link {
                Some(href) => Box::new(railroad::Link::new(name, format!("{link_prefix}{href}"))),
                None => name,
            };
            Box::new(railroad::LabeledBox::new(
                to_railroad(body, link_prefix),
                name,
            ))
        }
        DiagramNode::Comment(label) => Box::new(railroad::Comment::new(label.clone())),
        DiagramNode::LinkedComment { label, link } => {
            let comment: RailroadNode = Box::new(railroad::Comment::new(label.clone()));
            match link {
                Some(href) => {
                    Box::new(railroad::Link::new(comment, format!("{link_prefix}{href}")))
                }
                None => comment,
            }
        }
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
///
/// `link_prefix` rebases every Rustdoc-relative link for the page that embeds
/// the diagram: `../` for the aggregate page, or the declaration's ascent to
/// the grammar anchor for declaration-local pages.
pub fn render_svg(production: &DiagramNode, label: &str, link_prefix: &str) -> String {
    use railroad::Node as _;

    /// Diagram padding railroad adds around the root on every side.
    const DIAGRAM_PADDING: i64 = 10;

    let root = railroad::Sequence::new(vec![
        Box::new(railroad::Start) as RailroadNode,
        to_railroad(production, link_prefix),
        Box::new(railroad::End),
    ]);
    let width = root.width() + 2 * DIAGRAM_PADDING;
    let height = root.height() + 2 * DIAGRAM_PADDING;
    let mut diagram = railroad::Diagram::with_default_css(root);
    diagram.add_css(
        "svg.railroad a.link text { text-decoration: underline; } \
         svg.railroad a.link:hover text { text-decoration-thickness: 2px; }",
    );
    // Rustdoc still recognizes Markdown links inside raw HTML text. Entity-encode
    // brackets after railroad's XML escaping so regex labels remain literal.
    let raw = diagram
        .to_string()
        .replace('[', "&#91;")
        .replace(']', "&#93;");
    let title = escape_xml(&format!("{label} grammar diagram"));
    let description = escape_xml(&format!("Railroad topology for {label}"));
    let identifier = diagram_identifier(label);
    let opening_end = raw
        .find('>')
        .expect("railroad emits an opening SVG element");
    let mut svg = String::with_capacity(raw.len() + title.len() + description.len() + 220);
    svg.push_str(&raw[..opening_end]);
    svg.push_str(&format!(
        " width=\"{width}\" height=\"{height}\" \
         style=\"display: block; max-width: none\" \
         role=\"img\" aria-labelledby=\"{identifier}-title {identifier}-description\">"
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

    fn diagram_svg(production: &DiagramNode, label: &str) -> String {
        render_svg(production, label, "")
    }

    #[test]
    fn railroad_svg_is_accessible_deterministic_escaped_and_topological() {
        let production = DiagramNode::Choice(vec![
            DiagramNode::Sequence(vec![
                DiagramNode::Terminal("<&[x]".into()),
                DiagramNode::Optional(Box::new(DiagramNode::NonTerminal {
                    label: "Public".into(),
                    link: Some("struct.Public.html?x=1&y=2".into()),
                    reference: None,
                })),
            ]),
            DiagramNode::Repeat {
                item: Box::new(DiagramNode::NonTerminal {
                    label: "Private".into(),
                    link: None,
                    reference: None,
                }),
                separator: Box::new(DiagramNode::Terminal(",".into())),
            },
        ]);
        let svg = diagram_svg(&production, "Root<&");
        assert_eq!(svg, diagram_svg(&production, "Root<&"));
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("role=\"img\" aria-labelledby=\"recursa-diagram-"));
        assert!(svg.contains(" width=\""));
        assert!(svg.contains(" height=\""));
        assert!(svg.contains("style=\"display: block; max-width: none\""));
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

    #[test]
    fn declaration_local_documents_ascend_to_the_anchor_before_descending() {
        let node = NodeDiagramPlan {
            semantic_key: "ast::utility::explain::ExplainStmt".to_owned(),
            ident: "ExplainStmt".to_owned(),
            public_path: Some("ast/utility/explain/struct.ExplainStmt.html".to_owned()),
            production: DiagramNode::NonTerminal {
                label: "ExplainInput".to_owned(),
                link: Some("ast/utility/explain/struct.ExplainInput.html".to_owned()),
                reference: None,
            },
        };
        let (document, svg) = render_node_document(&node, 0);
        assert!(
            document
                .contains("xlink:href=\"../../../ast/utility/explain/struct.ExplainInput.html\""),
            "a page three modules below the anchor ascends three times:\n{document}",
        );
        assert!(
            document.contains(&svg),
            "the document embeds the standalone SVG"
        );
        assert!(document.starts_with("<!-- RECURSA_NODE_DIAGRAM: ExplainStmt -->"));

        let anchored = NodeDiagramPlan {
            semantic_key: "grammar::Record".to_owned(),
            ident: "Record".to_owned(),
            public_path: Some("struct.Record.html".to_owned()),
            production: DiagramNode::NonTerminal {
                label: "Label".to_owned(),
                link: Some("struct.Label.html".to_owned()),
                reference: None,
            },
        };
        let (document, _) = render_node_document(&anchored, 1);
        assert!(
            document.contains("xlink:href=\"struct.Label.html\""),
            "a page at the anchor links its siblings directly:\n{document}",
        );
    }

    #[test]
    fn token_documents_explain_the_label_and_pattern() {
        let token = TokenDiagramPlan {
            semantic_key: "grammar::Number".to_owned(),
            ident: "Number".to_owned(),
            pattern: "[0-9]+".to_owned(),
            label: "digits".to_owned(),
            public_path: Some("type.Number.html".to_owned()),
            production: DiagramNode::Terminal("digits".to_owned()),
        };
        let (document, svg) = render_token_document(&token, 1);
        assert!(document.starts_with("<!-- RECURSA_TOKEN_DIAGRAM: Number -->"));
        assert!(
            document.contains("The label `digits` represents the content-token regex `[0-9]+`")
        );
        assert!(document.contains(&svg));
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn aggregate_documents_list_every_public_type_and_fixed_token() {
        let plan = DiagramPlan {
            grammar_name: "Demo".to_owned(),
            anchor_depth: 1,
            nodes: vec![NodeDiagramPlan {
                semantic_key: "grammar::Record".to_owned(),
                ident: "Record".to_owned(),
                public_path: Some("struct.Record.html".to_owned()),
                production: DiagramNode::Terminal("record".to_owned()),
            }],
            tokens: vec![TokenDiagramPlan {
                semantic_key: "grammar::Number".to_owned(),
                ident: "Number".to_owned(),
                pattern: "[0-9]+".to_owned(),
                label: "[0-9]+".to_owned(),
                public_path: None,
                production: DiagramNode::Terminal("[0-9]+".to_owned()),
            }],
            fixed_tokens: vec![("Comma".to_owned(), ",".to_owned())],
        };
        let document = render_aggregate_document(&plan);
        assert!(document.starts_with("# Generated grammar diagrams"));
        assert!(document.contains("RECURSA_GRAMMAR_DIAGRAM"));
        assert!(document.contains("Grammar `Demo`"));
        assert!(document.contains("## Content tokens"));
        assert!(document.contains("- [`grammar::Record`](../struct.Record.html)"));
        assert!(!document.contains("grammar::Number`]("));
        assert!(document.contains("- `Comma`: `,`"));
        assert_eq!(document.matches("<svg").count(), 2);
    }

    #[test]
    fn variant_labels_render_as_anchored_links_when_public() {
        let production = DiagramNode::Choice(vec![DiagramNode::Sequence(vec![
            DiagramNode::LinkedComment {
                label: "prefix AtHashAtPrefix".to_owned(),
                link: Some("ast/shared/expr/enum.Expr.html#variant.AtHashAtPrefix".to_owned()),
            },
            DiagramNode::Terminal("@#@".to_owned()),
        ])]);
        let svg = render_svg(&production, "Expr", "../");
        assert!(
            svg.contains("href=\"../ast/shared/expr/enum.Expr.html#variant.AtHashAtPrefix\""),
            "svg:\n{svg}"
        );
        assert!(svg.contains(">prefix AtHashAtPrefix</text>"), "svg:\n{svg}");

        let private = DiagramNode::LinkedComment {
            label: "prefix Hidden".to_owned(),
            link: None,
        };
        let svg = render_svg(&private, "Hidden", "");
        assert!(
            !svg.contains("href="),
            "a private owner stays unlinked:\n{svg}"
        );
    }

    #[test]
    fn inline_expansions_render_as_linked_labeled_boxes() {
        let production = DiagramNode::Inline {
            label: "Item".into(),
            link: Some("struct.Item.html".into()),
            body: Box::new(DiagramNode::Terminal("ITEM".into())),
        };
        let svg = diagram_svg(&production, "Root");
        assert!(svg.contains("class=\"labeledbox\""), "svg:\n{svg}");
        assert!(svg.contains("href=\"struct.Item.html\""), "svg:\n{svg}");
        assert!(svg.contains(">Item</text>"), "svg:\n{svg}");
        assert!(svg.contains(">ITEM</text>"), "svg:\n{svg}");
        roxmltree::Document::parse(&svg).expect("labeled boxes render valid XML");
    }
}
