//! SVG serialization for railroad layout trees.

use crate::layout::{
    BASELINE_OFFSET, BOX_HEIGHT, CHOICE_RAIL_WIDTH, Choice, HORIZONTAL_SPACER, Node, NonTerminal,
    OneOrMore, Optional, Sequence, Terminal, VERTICAL_GAP, WRAP_ROW_GAP,
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

    // Verified in Phase 5 (Task 19) that rustdoc's ammonia sanitizer preserves
    // <svg> and its standard children intact, so inline embedding works.
    //
    // Colors use `currentColor` so the diagram adapts to rustdoc's light and
    // dark themes automatically (currentColor resolves to the inherited text
    // colour of the enclosing doc page). Rect interiors are transparent so
    // the page background shows through; only stroke and text are coloured.
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><!-- railroad --><style>.railroad rect{{fill:none;stroke:currentColor;stroke-width:1}} .railroad text{{font-family:monospace;font-size:12px;fill:currentColor}} .railroad path{{stroke:currentColor;stroke-width:1;fill:none}}</style><g class="railroad">"#,
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

fn render_sequence(s: &Sequence, x: i32, y: i32, out: &mut String) {
    if s.rows.is_empty() {
        render_sequence_row(&s.children, x, y, out);
        return;
    }
    render_wrapped_sequence(s, x, y, out);
}

/// Render a single horizontal row of children starting at `(x, y)`. Emits
/// inline connector stubs between adjacent siblings. Used both for the
/// non-wrapped path and for each row of a wrapped `Sequence`.
fn render_sequence_row(children: &[Node], start_x: i32, y: i32, out: &mut String) {
    let spacer = HORIZONTAL_SPACER as i32;
    let mut x = start_x;
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            out.push_str(&format!(
                r#"<path d="M{x1} {y} h{spacer}"/>"#,
                x1 = x - spacer,
            ));
        }
        render_node(child, x, y, out);
        x += child.width() as i32 + spacer;
    }
}

/// Render a multi-row wrapped `Sequence`. Each row is drawn with its own
/// independent baseline computed from the row's children. Rows are joined by
/// orthogonal wrap connectors: from the exit of the last child in row N to
/// the right margin, down to row N+1's baseline, then left to the first
/// child's entry.
fn render_wrapped_sequence(s: &Sequence, x: i32, y: i32, out: &mut String) {
    // Slice children into rows.
    let mut row_slices: Vec<&[Node]> = Vec::with_capacity(s.rows.len() + 1);
    let mut prev = 0usize;
    for &brk in &s.rows {
        row_slices.push(&s.children[prev..brk]);
        prev = brk;
    }
    row_slices.push(&s.children[prev..]);

    // Per-row up/down so we can compute each row's baseline y.
    let row_up: Vec<i32> = row_slices
        .iter()
        .map(|r| r.iter().map(|c| c.up()).max().unwrap_or(BASELINE_OFFSET) as i32)
        .collect();
    let row_down: Vec<i32> = row_slices
        .iter()
        .map(|r| r.iter().map(|c| c.down()).max().unwrap_or(BASELINE_OFFSET) as i32)
        .collect();

    // Row 0's baseline coincides with the caller's `y`. Subsequent rows sit
    // below by (prev_row.down + WRAP_ROW_GAP + this_row.up).
    let mut row_y: Vec<i32> = Vec::with_capacity(row_slices.len());
    row_y.push(y);
    for i in 1..row_slices.len() {
        let next_y = row_y[i - 1] + row_down[i - 1] + WRAP_ROW_GAP as i32 + row_up[i];
        row_y.push(next_y);
    }

    // Total drawn width (of the rect containing rows, excluding the back-rail
    // margin allocated in CHOICE_RAIL_WIDTH): max of row widths. We use the
    // Sequence's own `width` minus CHOICE_RAIL_WIDTH, which equals that max.
    let inner_w = s.width as i32 - CHOICE_RAIL_WIDTH as i32;
    let back_rail_x = x + inner_w + (CHOICE_RAIL_WIDTH as i32 / 2);

    // Render each row and its outbound wrap connector.
    let spacer = HORIZONTAL_SPACER as i32;
    for (i, row) in row_slices.iter().enumerate() {
        let ry = row_y[i];
        // Align rows to the left edge `x` (no centering; keeps left rail
        // vertical and connector geometry straightforward).
        render_sequence_row(row, x, ry, out);

        if i + 1 < row_slices.len() {
            // Exit x of this row = start_x + sum(child widths) + spacers.
            let row_w: i32 = row.iter().map(|c| c.width() as i32).sum::<i32>()
                + spacer * (row.len() as i32 - 1).max(0);
            let exit_x = x + row_w;
            let next_ry = row_y[i + 1];
            // Orthogonal wrap rail: right, down, left.
            out.push_str(&format!(
                r#"<path d="M{exit_x} {ry} L{back_rail_x} {ry} L{back_rail_x} {next_ry} L{x} {next_ry}"/>"#,
            ));
        }
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
