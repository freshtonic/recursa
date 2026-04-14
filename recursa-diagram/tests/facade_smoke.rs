use recursa_diagram::{
    layout::{Node, Terminal},
    render,
};

#[test]
fn facade_reexports_layout_and_render() {
    let svg = render(&Node::Terminal(Terminal::new("SELECT")));
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("SELECT"));
}
