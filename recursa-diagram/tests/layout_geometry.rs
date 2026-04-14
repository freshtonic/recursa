use recursa_diagram::layout::{Node, NonTerminal, Terminal};

#[test]
fn terminal_geometry_pins_constants() {
    let t = Node::Terminal(Terminal::new("SELECT"));
    assert_eq!(t.width(), 6 * 8 + 40); // 88
    assert_eq!(t.height(), 22);
    assert_eq!(t.up(), 11);
    assert_eq!(t.down(), 11);
    assert_eq!(Terminal::new("SELECT").text, "SELECT");
}

#[test]
fn non_terminal_width_scales_with_text() {
    let short = Node::NonTerminal(NonTerminal::new("Expr", None));
    let long = Node::NonTerminal(NonTerminal::new("VeryLongTypeName", None));
    assert!(long.width() > short.width());
    assert_eq!(short.width(), 4 * 8 + 40); // "Expr" → 72
}

#[test]
fn non_terminal_preserves_href() {
    let nt = NonTerminal::new("Expr", Some("Expr.html".into()));
    assert_eq!(nt.href.as_deref(), Some("Expr.html"));
}
