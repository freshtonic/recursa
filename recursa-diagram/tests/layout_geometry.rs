use recursa_diagram::layout::{Node, NonTerminal, Terminal};

#[test]
fn terminal_geometry_is_nonzero() {
    let t = Node::Terminal(Terminal::new("SELECT"));
    assert!(t.width() > 0);
    assert!(t.height() > 0);
    let _ = t.up();
    let _ = t.down();
}

#[test]
fn non_terminal_width_scales_with_text() {
    let short = Node::NonTerminal(NonTerminal::new("Expr", None));
    let long = Node::NonTerminal(NonTerminal::new("VeryLongTypeName", None));
    assert!(long.width() > short.width());
}

#[test]
fn non_terminal_preserves_href() {
    let nt = NonTerminal::new("Expr", Some("Expr.html".into()));
    assert_eq!(nt.href.as_deref(), Some("Expr.html"));
}
