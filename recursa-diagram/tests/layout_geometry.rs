use recursa_diagram::layout::{Node, Terminal};

#[test]
fn terminal_geometry_is_nonzero() {
    let t = Node::Terminal(Terminal::new("SELECT"));
    assert!(t.width() > 0);
    assert!(t.height() > 0);
    let _ = t.up();
    let _ = t.down();
}
