use recursa_diagram_core::{
    layout::{Choice, Node, NonTerminal, OneOrMore, Optional, Sequence, Terminal},
    render,
};

/// Cheap well-formedness check: every `<tag>` opens must be balanced by a
/// matching `</tag>` or self-close `/>`. This catches accidental unbalanced
/// markup in the first-pass renderers without pulling in a full XML parser.
///
/// **Limitation:** counts-based — does NOT detect tag-name mismatches like
/// `<a></b>`. Use for smoke-testing emission quantity only; rely on the
/// snapshot fixture for structural correctness.
fn assert_balanced_tags(svg: &str) {
    // Strip XML comments `<!-- ... -->` so they don't confuse the counter.
    let mut stripped = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find("<!--") {
        stripped.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let end = after.find("-->").expect("unterminated comment");
        rest = &after[end + 3..];
    }
    stripped.push_str(rest);
    let s = stripped.as_str();

    // Every `<` is either an opening tag, a self-closing tag, or a closing tag.
    //   opens_total  = open_tags + close_tags
    //   open_tags    = self_close + paired_opens
    //   paired_opens = close_tags
    // So opens_total == self_close + 2 * close_tags.
    let opens = s.matches('<').count();
    let self_close = s.matches("/>").count();
    let close = s.matches("</").count();
    assert_eq!(
        opens,
        self_close + 2 * close,
        "unbalanced tags: opens={opens} self_close={self_close} close={close} svg={svg}"
    );
}

/// Extract the `y="..."` attribute of the `<rect>` that immediately precedes
/// the given label text. Returns the integer y value of the rect's top edge.
///
/// Searches for `>{label}<` so href attributes like `href="Foo.html"` don't
/// match before the actual `<text>Foo</text>` element.
fn rect_y_before_label(svg: &str, label: &str) -> i32 {
    let needle = format!(">{label}<");
    let label_pos = svg
        .find(&needle)
        .unwrap_or_else(|| panic!("label {label} not found (as >{label}<)"));
    let prefix = &svg[..label_pos];
    let rect_pos = prefix.rfind("<rect").expect("rect before label");
    let rect_rest = &svg[rect_pos..];
    // Find y=" then parse integer
    let y_start = rect_rest.find(r#"y=""#).expect("y attr") + 3;
    let y_rest = &rect_rest[y_start..];
    let y_end = y_rest.find('"').expect("y close");
    y_rest[..y_end].parse::<i32>().expect("y int")
}

#[test]
fn terminal_svg_contains_text() {
    let svg = render(&Node::Terminal(Terminal::new("SELECT")));
    assert!(svg.starts_with("<svg"), "should be an svg: {svg}");
    assert!(svg.contains("SELECT"), "should contain the literal: {svg}");
    assert!(svg.ends_with("</svg>"));
    assert!(svg.contains("<!-- railroad -->"));
}

#[test]
fn non_terminal_svg_without_href() {
    let svg = render(&Node::NonTerminal(NonTerminal::new("Expr", None)));
    assert!(svg.contains("Expr"));
    assert!(!svg.contains("<a "));
}

#[test]
fn non_terminal_svg_with_href_wraps_in_anchor() {
    let svg = render(&Node::NonTerminal(NonTerminal::new(
        "Expr",
        Some("Expr.html".into()),
    )));
    assert!(svg.contains(r#"<a xlink:href="Expr.html""#) || svg.contains(r#"<a href="Expr.html""#));
    assert!(svg.contains("Expr"));
}

#[test]
fn sequence_renders_children_in_order() {
    let seq = Node::Sequence(Sequence::new(vec![
        Node::Terminal(Terminal::new("SELECT")),
        Node::NonTerminal(NonTerminal::new("Column", None)),
    ]));
    let svg = render(&seq);
    let i_select = svg.find("SELECT").expect("SELECT present");
    let i_column = svg.find("Column").expect("Column present");
    assert!(i_select < i_column, "SELECT should appear before Column");
    assert_eq!(
        svg.matches("<path").count(),
        1,
        "expected exactly one connector between two children: {svg}"
    );
}

#[test]
fn terminal_text_is_xml_escaped() {
    let svg = render(&Node::Terminal(Terminal::new("a<b&c\"'")));
    assert!(
        svg.contains("a&lt;b&amp;c&quot;&apos;"),
        "expected escaped text: {svg}"
    );
    assert!(!svg.contains("a<b"), "raw < leaked through: {svg}");
}

#[test]
fn non_terminal_href_is_escaped() {
    let svg = render(&Node::NonTerminal(NonTerminal::new(
        "X",
        Some("a&b.html".into()),
    )));
    assert!(
        svg.contains(r#"href="a&amp;b.html""#),
        "expected escaped href: {svg}"
    );
}

#[test]
fn choice_default_branch_sits_on_baseline() {
    // 3-branch choice with middle (index 1) as the default. The default branch
    // must be rendered at the enclosing baseline; the other two must sit above
    // and below it.
    let branch_a = Node::Terminal(Terminal::new("AAA"));
    let branch_b = Node::Terminal(Terminal::new("BBB"));
    let branch_c = Node::Terminal(Terminal::new("CCC"));
    let ch = Node::Choice(Choice::new(1, vec![branch_a, branch_b, branch_c]));
    let svg = render(&ch);
    assert_balanced_tags(&svg);

    let y_a = rect_y_before_label(&svg, "AAA");
    let y_b = rect_y_before_label(&svg, "BBB");
    let y_c = rect_y_before_label(&svg, "CCC");

    // Baseline of the diagram = SVG_OUTER_PADDING (10) + root.up().
    // With the middle branch on the baseline, its rect top edge is baseline - 11.
    // The top branch sits above (smaller y); the bottom branch sits below.
    assert!(
        y_a < y_b,
        "top branch must be above default: y_a={y_a} y_b={y_b}"
    );
    assert!(
        y_c > y_b,
        "bottom branch must be below default: y_c={y_c} y_b={y_b}"
    );

    // The default branch's rect top must be exactly `pad + (root.up() - 11)` =
    // 10 + (root.up() - 11). root.up() is `default.up() + (above sum)`.
    // We can compute it via the layout without re-deriving: for a single-level
    // Choice with three identical-height branches and default_idx=1, the default
    // branch's rect top is pad + default_up_offset - half = 10 + above - 0? Let's
    // just assert the difference: (y_b - y_a) should equal the branch stride
    // (branch height + vertical gap) = 22 + 10 = 32.
    assert_eq!(y_b - y_a, 32, "branch stride should be box+gap");
    assert_eq!(y_c - y_b, 32, "branch stride should be box+gap");
}

#[test]
fn choice_emits_text_for_every_branch() {
    let ch = Node::Choice(Choice::new(
        0,
        vec![
            Node::Terminal(Terminal::new("ONE")),
            Node::Terminal(Terminal::new("TWO")),
        ],
    ));
    let svg = render(&ch);
    assert_balanced_tags(&svg);
    assert!(svg.contains("ONE"));
    assert!(svg.contains("TWO"));
}

#[test]
fn optional_renders_child_on_baseline_with_skip_rail_above() {
    let opt = Node::Optional(Optional::new(Node::Terminal(Terminal::new("MAYBE"))));
    let svg = render(&opt);
    assert_balanced_tags(&svg);
    assert!(svg.contains("MAYBE"));

    // The child's rect top is at (pad + root.up()) - 11. The skip rail path is
    // emitted as a <path> element. At minimum we should see at least one path
    // and the child's text on the main baseline row.
    assert!(
        svg.matches("<path").count() >= 1,
        "expected skip-rail path(s): {svg}"
    );

    // Child rect y == pad + root.up() - 11. The main baseline equals
    // pad + root.up(), i.e. 10 + (11 + 22 + 10) = 53, so rect top is 42.
    let child_y = rect_y_before_label(&svg, "MAYBE");
    assert_eq!(
        child_y, 42,
        "optional child should sit on main baseline, got rect y={child_y}: {svg}"
    );
}

#[test]
fn one_or_more_without_separator_has_loopback_path() {
    let om = Node::OneOrMore(OneOrMore::new(Node::Terminal(Terminal::new("ITEM")), None));
    let svg = render(&om);
    assert_balanced_tags(&svg);
    assert!(svg.contains("ITEM"));
    assert!(
        svg.matches("<path").count() >= 1,
        "expected loop-back path: {svg}"
    );
}

#[test]
fn one_or_more_with_separator_places_sep_below_child() {
    let om = Node::OneOrMore(OneOrMore::new(
        Node::Terminal(Terminal::new("COL")),
        Some(Node::Terminal(Terminal::new("COMMA"))),
    ));
    let svg = render(&om);
    assert_balanced_tags(&svg);
    assert!(svg.contains("COL"));
    assert!(svg.contains("COMMA"));

    let y_col = rect_y_before_label(&svg, "COL");
    let y_comma = rect_y_before_label(&svg, "COMMA");
    assert!(
        y_comma > y_col,
        "separator must render below child: y_col={y_col} y_comma={y_comma}"
    );
}

#[test]
fn empty_sequence_renders_valid_wrapper() {
    // Empty sequence: body width = CHOICE_RAIL_WIDTH (20, entry+exit stubs),
    // body height = BOX_HEIGHT (22), up = down = BASELINE_OFFSET (11).
    // With SVG_OUTER_PADDING (10) on each side:
    //   total width  = 20 + 20 = 40
    //   total height = 22 + 20 = 42
    let svg = render(&Node::Sequence(Sequence::new(vec![])));
    assert!(svg.starts_with("<svg"), "not an svg: {svg}");
    assert!(svg.ends_with("</svg>"), "unclosed svg: {svg}");
    assert!(svg.contains(r#"width="40""#), "unexpected width: {svg}");
    assert!(svg.contains(r#"height="42""#), "unexpected height: {svg}");
    assert_eq!(
        svg.matches("<path").count(),
        0,
        "empty sequence should emit no connectors: {svg}"
    );
}
