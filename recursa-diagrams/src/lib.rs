//! Semantic railroad-diagram plans and deterministic Rustdoc SVG rendering.
//!
//! Diagram work happens in three phases owned by different processes:
//!
//! 1. `recursa-codegen` derives a [`DiagramPlan`] from a frozen Generation and
//!    publishes it with [`DiagramPlan::encode`] next to the generated source.
//! 2. Generated items carry `cfg(doc)`-gated facade attributes that name their
//!    diagram. Ordinary builds strip those attributes before expansion, so no
//!    diagram markup is ever compiled.
//! 3. While `cargo doc` expands those attributes, the facade decodes the plan
//!    with [`DiagramPlan::decode`], renders the requested diagram, writes the
//!    standalone SVG file, and attaches the Rustdoc document.
//!
//! Rendering follows the strategy proven by pg-proto's protocol railroads:
//! reference labels drop their module qualifiers so boxes stay compact, and the
//! first reference to another same-grammar Node application is expanded inline
//! as a labeled box, while every later or recursive reference stays an opaque
//! linked box. Both choices only change presentation; the plan remains derived
//! from the frozen generation inputs.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

mod codec;
mod render;

pub use codec::DecodeError;
pub use render::{
    render_aggregate_document, render_node_document, render_svg, render_token_document,
};

/// Generation-relative directory holding the published plan and rendered SVG files.
pub const DIAGRAM_DIRECTORY: &str = "diagrams";
/// File name of the encoded [`DiagramPlan`] inside [`DIAGRAM_DIRECTORY`].
pub const PLAN_FILE_NAME: &str = "diagrams.plan";

/// Returns the standalone SVG file name for the Node at one canonical index.
pub fn node_svg_file_name(index: usize) -> String {
    format!("node-{index}.svg")
}

/// Returns the standalone SVG file name for the content token at one canonical index.
pub fn token_svg_file_name(index: usize) -> String {
    format!("token-{index}.svg")
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar topology independent of railroad layout geometry.
pub enum DiagramNode {
    /// An epsilon path.
    Empty,
    /// Exact fixed syntax.
    Terminal(String),
    /// A semantic reference, linked only when publicly addressable.
    NonTerminal {
        /// Shortened text displayed for the referenced semantic type.
        label: String,
        /// Rustdoc-relative public target, or `None` for private targets.
        link: Option<String>,
        /// Full application display name when the reference targets a Node
        /// application in this grammar, or `None` for content tokens.
        reference: Option<String>,
    },
    /// A referenced Node application expanded in place as a labeled box.
    Inline {
        /// Shortened text labeling the expanded production.
        label: String,
        /// Rustdoc-relative public target, or `None` for private targets.
        link: Option<String>,
        /// The referenced application's production expanded in place.
        body: Box<Self>,
    },
    /// A non-consuming branch or cardinality annotation.
    Comment(String),
    /// A production carrying explanatory hover text instead of inline syntax.
    ///
    /// The note never occupies diagram space: renderers attach it to the drawn
    /// group so a reader sees it only while pointing at the production.
    Annotated {
        /// Hover text describing why the production is annotated.
        note: String,
        /// The annotated production, drawn unchanged.
        body: Box<Self>,
    },
    /// A branch annotation linking to a Rustdoc anchor when its owner is public.
    LinkedComment {
        /// Annotation text.
        label: String,
        /// Rustdoc-relative anchor target, or `None` for private owners.
        link: Option<String>,
    },
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

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar-wide diagram projection in canonical node order.
pub struct DiagramPlan {
    /// Display name labeling the aggregate grammar diagram.
    pub grammar_name: String,
    /// Crate-relative module depth of the grammar anchor owning the output.
    pub anchor_depth: usize,
    /// Node productions in canonical parse-plan order.
    pub nodes: Vec<NodeDiagramPlan>,
    /// Typed content-token diagrams in canonical token order.
    pub tokens: Vec<TokenDiagramPlan>,
    /// Fixed-token names and canonical spellings in kind order.
    pub fixed_tokens: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Diagram identity and railroad body for one node.
pub struct NodeDiagramPlan {
    /// Stable Node identity used to correlate documentation artifacts.
    pub semantic_key: String,
    /// Generated Rust identifier labeling the production.
    pub ident: String,
    /// Rustdoc-relative link target when the Node is publicly reachable.
    pub public_path: Option<String>,
    /// Semantic production derived from the frozen node application plans.
    pub production: DiagramNode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Frozen diagram plan for one typed content-token identity.
pub struct TokenDiagramPlan {
    /// Stable semantic key used for aggregate documentation and debug artifacts.
    pub semantic_key: String,
    /// Generated public token name used by declaration-local Rustdoc.
    pub ident: String,
    /// Authored lexical pattern retained for explanatory documentation.
    pub pattern: String,
    /// Authored diagram label, or the lexical-pattern fallback.
    pub label: String,
    /// Rustdoc-relative target when the token binding is publicly reachable.
    pub public_path: Option<String>,
    /// Semantic token production rendered by railroad.
    pub production: DiagramNode,
}

/// Converts a semantic key into a Rustdoc path relative to the module anchor.
///
/// Semantic keys are crate-relative; the anchor owns the generated output
/// (including the aggregate `grammar_docs` page), so exactly `anchor_depth`
/// leading module segments are stripped. An anchor of `crate` strips none.
pub fn rustdoc_path(semantic_key: &str, kind: &str, anchor_depth: usize) -> String {
    let mut segments = semantic_key.split("::").collect::<Vec<_>>();
    let name = segments.pop().expect("semantic keys are nonempty");
    let segments = &segments[anchor_depth.min(segments.len())..];
    let page = format!("{kind}.{name}.html");
    if segments.is_empty() {
        page
    } else {
        format!("{}/{}", segments.join("/"), page)
    }
}

/// Returns the `../` ascent from a declaration's module up to the anchor.
///
/// Declaration-local diagrams embed into the Rustdoc page of their own
/// module; anchor-relative link targets therefore first ascend one level per
/// module below the anchor.
pub fn ascent_to_anchor(semantic_key: &str, anchor_depth: usize) -> String {
    let module_depth = semantic_key.split("::").count().saturating_sub(1);
    "../".repeat(module_depth.saturating_sub(anchor_depth))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustdoc_paths_strip_exactly_the_anchor_depth() {
        // Anchor `crate` (depth 0): every crate-relative module segment survives.
        assert_eq!(
            rustdoc_path("ast::utility::explain::ExplainStmt", "struct", 0),
            "ast/utility/explain/struct.ExplainStmt.html",
        );
        // Anchor `crate::grammar` (depth 1): the anchor segment is stripped.
        assert_eq!(
            rustdoc_path("grammar::Record", "struct", 1),
            "struct.Record.html",
        );
        assert_eq!(
            rustdoc_path("grammar::nested::Leaf", "struct", 1),
            "nested/struct.Leaf.html",
        );
    }

    #[test]
    fn standalone_file_names_are_indexed_and_stable() {
        assert_eq!(node_svg_file_name(0), "node-0.svg");
        assert_eq!(token_svg_file_name(12), "token-12.svg");
        assert_eq!(PLAN_FILE_NAME, "diagrams.plan");
        assert_eq!(DIAGRAM_DIRECTORY, "diagrams");
    }
}
