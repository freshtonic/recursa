//! SVG serialization for railroad layout trees.

use crate::layout::{
    BASELINE_OFFSET, BOX_HEIGHT, CHOICE_RAIL_WIDTH, Choice, HORIZONTAL_SPACER, Node, NonTerminal,
    OneOrMore, Optional, Sequence, Terminal, VERTICAL_GAP,
};

/// Outer padding around the rendered diagram, in SVG user units.
pub(crate) const SVG_OUTER_PADDING: u32 = 10;

/// Vertical nudge from the box midline to the font's visual baseline,
/// tuned for the 12px monospace font used in the <style> block below.
const TEXT_BASELINE_NUDGE: i32 = 4;

pub fn render(root: &Node) -> String {
    let mut out = String::new();
    let pad = SVG_OUTER_PADDING;
    let total_w = root.width() + pad * 2;
    let total_h = root.height() + pad * 2;

    // TODO(phase-5): rustdoc runs ammonia on doc HTML; its default allowlist
    // excludes svg/rect/path/text/g/style and <a> inside <svg>. We will likely
    // need to either embed via <img src="data:image/svg+xml;..."/>, reference
    // an external .svg file written next to the generated docs, or extend the
    // ammonia allowlist via a crate attribute. Verify empirically in Phase 5.
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

/// Horizontally centre a child of width `child_w` within a composite of width
/// `total_w`, leaving `CHOICE_RAIL_WIDTH` total for entry+exit rails.
///
/// Integer division rounds toward zero, so when the residual width is odd the
/// child sits 1px left of true geometric centre. Invisible for 12px monospace.
fn centered_child_x(x: i32, total_w: i32, child_w: i32) -> i32 {
    let rail = CHOICE_RAIL_WIDTH as i32 / 2;
    x + rail + (total_w - CHOICE_RAIL_WIDTH as i32 - child_w) / 2
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

// Terminals render as rounded rects (vs. non-terminals' square boxes).
fn render_terminal(t: &Terminal, x: i32, y: i32, out: &mut String) {
    let w = t.width as i32;
    let h = BOX_HEIGHT as i32;
    let half = BASELINE_OFFSET as i32;
    out.push_str(&format!(
        r##"<rect x="{x}" y="{ry}" width="{w}" height="{h}" rx="{half}" ry="{half}"/><text x="{tx}" y="{ty}" text-anchor="middle">{text}</text>"##,
        ry = y - half,
        tx = x + w / 2,
        ty = y + TEXT_BASELINE_NUDGE,
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

// Non-terminals render as square boxes (vs. terminals' rounded ends).
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
        ty = y + TEXT_BASELINE_NUDGE,
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
// First-pass Choice renderer. The default branch is drawn on the enclosing
// baseline `y`; other branches are stacked above/below with simple quadratic
// entry/exit rails. Visual polish (proper arcs) is deferred; see the design
// doc Phase 3.
fn render_choice(c: &Choice, x: i32, y: i32, out: &mut String) {
    let rail = CHOICE_RAIL_WIDTH as i32 / 2;
    let total_w = c.width as i32;

    // Compute each branch's baseline relative to `y`. The default branch is at
    // `y`; branches above accumulate negative offsets, branches below positive.
    let mut branch_ys = vec![0i32; c.children.len()];
    // Walk up from default.
    let mut acc: i32 = 0;
    for i in (0..c.default_idx).rev() {
        // Distance from branch i's baseline to branch i+1's baseline:
        // i.down + gap + (i+1).up.
        let delta =
            c.children[i].down() as i32 + VERTICAL_GAP as i32 + c.children[i + 1].up() as i32;
        acc -= delta;
        branch_ys[i] = acc;
    }
    // Walk down from default.
    acc = 0;
    for (i, slot) in branch_ys.iter_mut().enumerate().skip(c.default_idx + 1) {
        let delta =
            c.children[i - 1].down() as i32 + VERTICAL_GAP as i32 + c.children[i].up() as i32;
        acc += delta;
        *slot = acc;
    }

    for (i, child) in c.children.iter().enumerate() {
        let child_y = y + branch_ys[i];
        let child_w = child.width() as i32;
        let child_x = centered_child_x(x, total_w, child_w);
        let exit_x = child_x + child_w;
        let end_x = x + total_w;

        if child_y == y {
            // Default branch: straight line through baseline, no degenerate curves.
            out.push_str(&format!(r#"<path d="M{x} {y} L{child_x} {y}"/>"#));
            render_node(child, child_x, y, out);
            out.push_str(&format!(r#"<path d="M{exit_x} {y} L{end_x} {y}"/>"#));
        } else {
            // Off-baseline branch: quadratic entry + straight + quadratic exit.
            out.push_str(&format!(
                r#"<path d="M{x} {y} Q{cx} {y} {cx} {child_y} L{child_x} {child_y}"/>"#,
                cx = x + rail,
            ));
            render_node(child, child_x, child_y, out);
            out.push_str(&format!(
                r#"<path d="M{exit_x} {child_y} L{rx} {child_y} Q{end_x} {child_y} {end_x} {y}"/>"#,
                rx = end_x - rail,
            ));
        }
    }
}

// First-pass Optional renderer. The child is drawn on the enclosing baseline;
// a skip rail arcs above the child (by BOX_HEIGHT + VERTICAL_GAP, matching the
// layout geometry) bypassing it.
fn render_optional(o: &Optional, x: i32, y: i32, out: &mut String) {
    let rail = CHOICE_RAIL_WIDTH as i32 / 2;
    let total_w = o.width as i32;
    let skip_dy = (BOX_HEIGHT + VERTICAL_GAP) as i32;
    let skip_y = y - skip_dy;

    let child = &*o.child;
    let child_w = child.width() as i32;
    let child_x = centered_child_x(x, total_w, child_w);

    // Straight-through: entry stub, child, exit stub, all at baseline y.
    out.push_str(&format!(r#"<path d="M{x} {y} L{child_x} {y}"/>"#,));
    render_node(child, child_x, y, out);
    let exit_x = child_x + child_w;
    let end_x = x + total_w;
    out.push_str(&format!(r#"<path d="M{exit_x} {y} L{end_x} {y}"/>"#,));
    // Skip rail: from (x, y) up to skip_y, across, back down to (end_x, y).
    out.push_str(&format!(
        r#"<path d="M{x} {y} Q{rx} {y} {rx} {skip_y} L{lx} {skip_y} Q{end_x} {skip_y} {end_x} {y}"/>"#,
        rx = x + rail,
        lx = end_x - rail,
    ));
}

// First-pass OneOrMore renderer. The child is drawn on the enclosing baseline;
// the separator (if present) or an implicit return rail is drawn below, with
// loop-back paths on each side.
fn render_one_or_more(o: &OneOrMore, x: i32, y: i32, out: &mut String) {
    let total_w = o.width as i32;

    let child = &*o.child;
    let child_w = child.width() as i32;
    let child_x = centered_child_x(x, total_w, child_w);

    // Straight-through path at baseline (entry stub + exit stub).
    out.push_str(&format!(r#"<path d="M{x} {y} L{child_x} {y}"/>"#,));
    render_node(child, child_x, y, out);
    let exit_x = child_x + child_w;
    let end_x = x + total_w;
    out.push_str(&format!(r#"<path d="M{exit_x} {y} L{end_x} {y}"/>"#,));

    // Loop-back row: separator (if any) rendered below the child, with rails
    // connecting child exit -> row -> child entry.
    if let Some(sep) = o.separator.as_deref() {
        let sep_w = sep.width() as i32;
        let sep_x = centered_child_x(x, total_w, sep_w);
        let sep_y = y + child.down() as i32 + VERTICAL_GAP as i32 + sep.up() as i32;
        // Rails: down from exit to sep row, across sep, back up to entry.
        out.push_str(&format!(
            r#"<path d="M{exit_x} {y} Q{end_x} {y} {end_x} {sep_y} L{sep_right} {sep_y}"/>"#,
            sep_right = sep_x + sep_w,
        ));
        render_node(sep, sep_x, sep_y, out);
        out.push_str(&format!(
            r#"<path d="M{sep_x} {sep_y} L{x} {sep_y} Q{x} {y} {child_x} {y}"/>"#,
        ));
    } else {
        // Implicit return rail: simple loop underneath.
        let rail_y = y + child.down() as i32 + VERTICAL_GAP as i32;
        out.push_str(&format!(
            r#"<path d="M{exit_x} {y} Q{end_x} {y} {end_x} {rail_y} L{x} {rail_y} Q{x} {y} {child_x} {y}"/>"#,
        ));
    }
}
