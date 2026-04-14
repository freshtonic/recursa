use recursa_diagram::layout::{Choice, Node, NonTerminal, Sequence, Terminal};

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

#[test]
fn sequence_width_sums_children_plus_spacing() {
    let a = Node::Terminal(Terminal::new("A"));
    let b = Node::Terminal(Terminal::new("B"));
    let wa = a.width();
    let wb = b.width();
    let seq = Node::Sequence(Sequence::new(vec![a, b]));
    // 10 px spacer between adjacent children.
    assert_eq!(seq.width(), wa + wb + 10);
}

#[test]
fn empty_sequence_has_zero_body_width() {
    let seq = Node::Sequence(Sequence::new(vec![]));
    // Entry/exit stubs: 20 px total.
    assert_eq!(seq.width(), 20);
}

#[test]
fn choice_width_is_max_child_plus_rails() {
    let a = Node::Terminal(Terminal::new("A"));
    let b = Node::Terminal(Terminal::new("LONGER_OPTION"));
    let wb = b.width();
    let ch = Node::Choice(Choice::new(0, vec![a, b]));
    // 20 px for entry/exit rails.
    assert_eq!(ch.width(), wb + 20);
}

#[test]
fn choice_height_sums_children_plus_vertical_gap() {
    let a = Node::Terminal(Terminal::new("A"));
    let b = Node::Terminal(Terminal::new("B"));
    let ha = a.height();
    let hb = b.height();
    let ch = Node::Choice(Choice::new(0, vec![a, b]));
    // 10 px vertical gap between branches.
    assert_eq!(ch.height(), ha + hb + 10);
}
