use recursa_diagram::{
    layout::{Node, Terminal},
    railroad, render,
};

#[test]
fn facade_reexports_layout_and_render() {
    let svg = render(&Node::Terminal(Terminal::new("SELECT")));
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("SELECT"));
}

#[railroad(label = "SELECT")]
pub struct SelectKw;

#[test]
fn facade_reexports_railroad() {
    // Compiled successfully; nothing to assert at runtime.
    let _ = SelectKw;
}
