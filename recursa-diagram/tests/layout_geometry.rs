use recursa_diagram::layout::{
    Choice, Node, NonTerminal, OneOrMore, Optional, Sequence, Terminal, zero_or_more,
};

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

#[test]
fn optional_adds_skip_branch() {
    let child = Node::Terminal(Terminal::new("X"));
    let cw = child.width();
    let opt = Node::Optional(Optional::new(child));
    // skip rail adds 20 px of rails; height grows by 20 px (skip line + gap).
    assert_eq!(opt.width(), cw + 20);
    assert!(opt.height() > 22);
}

#[test]
fn one_or_more_with_separator() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let sep = Node::Terminal(Terminal::new(","));
    let max_w = child.width().max(sep.width());
    let oom = Node::OneOrMore(OneOrMore::new(child, Some(sep)));
    assert_eq!(oom.width(), max_w + 20);
}

#[test]
fn one_or_more_without_separator() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let cw = child.width();
    let oom = Node::OneOrMore(OneOrMore::new(child, None));
    assert_eq!(oom.width(), cw + 20);
}

#[test]
fn zero_or_more_wraps_one_or_more_in_optional() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let cw = child.width();
    let z = zero_or_more(child, None);
    // OneOrMore adds 20 px of rails, Optional wraps it and adds another 20 px.
    assert_eq!(z.width(), cw + 20 + 20);
    assert!(matches!(z, Node::Optional(_)));
}
