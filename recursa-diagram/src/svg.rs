//! SVG serialization for railroad layout trees.

use crate::layout::{
    BASELINE_OFFSET, BOX_HEIGHT, Choice, HORIZONTAL_SPACER, Node, NonTerminal, OneOrMore, Optional,
    Sequence, Terminal,
};

/// Outer padding around the rendered diagram, in SVG user units.
pub(crate) const SVG_OUTER_PADDING: u32 = 10;

pub fn render(root: &Node) -> String {
    let mut out = String::new();
    let pad = SVG_OUTER_PADDING;
    let total_w = root.width() + pad * 2;
    let total_h = root.height() + pad * 2;

    // TODO(phase-5): verify rustdoc preserves <style> or move to inline attrs
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><!-- railroad --><style>.railroad rect{{fill:#fff;stroke:#333;stroke-width:1}} .railroad text{{font-family:monospace;font-size:12px;fill:#000}} .railroad path{{stroke:#333;stroke-width:1;fill:none}}</style><g class="railroad">"#,
        w = total_w,
        h = total_h,
    ));

    let x = pad as i32;
    let y = (pad + root.up()) as i32;
    render_node(root, x, y, &mut out);

    out.push_str("</g></svg>");
    out
}

fn render_node(node: &Node, x: i32, y: i32, out: &mut String) {
    match node {
        Node::Terminal(t) => render_terminal(t, x, y, out),
        Node::NonTerminal(nt) => render_non_terminal(nt, x, y, out),
        Node::Sequence(s) => render_sequence(s, x, y, out),
        Node::Choice(c) => render_choice(c, x, y, out),
        Node::Optional(o) => render_optional(o, x, y, out),
        Node::OneOrMore(om) => render_one_or_more(om, x, y, out),
    }
}

fn render_terminal(t: &Terminal, x: i32, y: i32, out: &mut String) {
    let w = t.width as i32;
    let h = BOX_HEIGHT as i32;
    let half = BASELINE_OFFSET as i32;
    out.push_str(&format!(
        r##"<rect x="{x}" y="{ry}" width="{w}" height="{h}" rx="{half}" ry="{half}"/><text x="{tx}" y="{ty}" text-anchor="middle">{text}</text>"##,
        ry = y - half,
        tx = x + w / 2,
        ty = y + 4,
        text = escape(&t.text),
    ));
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// Stubs for the other variants — real impls come in later tasks.
fn render_non_terminal(nt: &NonTerminal, x: i32, y: i32, out: &mut String) {
    if let Some(href) = &nt.href {
        out.push_str(&format!(r#"<a href="{h}">"#, h = escape(href)));
    }
    let w = nt.width as i32;
    let h = BOX_HEIGHT as i32;
    let half = BASELINE_OFFSET as i32;
    out.push_str(&format!(
        r##"<rect x="{x}" y="{ry}" width="{w}" height="{h}"/><text x="{tx}" y="{ty}" text-anchor="middle">{text}</text>"##,
        ry = y - half,
        tx = x + w / 2,
        ty = y + 4,
        text = escape(&nt.text),
    ));
    if nt.href.is_some() {
        out.push_str("</a>");
    }
}
fn render_sequence(s: &Sequence, mut x: i32, y: i32, out: &mut String) {
    let spacer = HORIZONTAL_SPACER as i32;
    for (i, child) in s.children.iter().enumerate() {
        if i > 0 {
            // Connector path between previous child's exit and this child's entry,
            // drawn at the shared baseline `y`.
            out.push_str(&format!(
                r#"<path d="M{x1} {y} h{spacer}"/>"#,
                x1 = x - spacer,
            ));
        }
        render_node(child, x, y, out);
        x += child.width() as i32 + spacer;
    }
}
fn render_choice(_: &Choice, _: i32, _: i32, _: &mut String) {
    todo!("render_choice not yet implemented");
}
fn render_optional(_: &Optional, _: i32, _: i32, _: &mut String) {
    todo!("render_optional not yet implemented");
}
fn render_one_or_more(_: &OneOrMore, _: i32, _: i32, _: &mut String) {
    todo!("render_one_or_more not yet implemented");
}
