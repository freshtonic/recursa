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
}
