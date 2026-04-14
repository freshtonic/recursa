use recursa_diagram::{
    layout::{Node, NonTerminal, Sequence, Terminal},
    render,
};

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
