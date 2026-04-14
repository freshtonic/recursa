use recursa_diagram::layout::{
    Choice, Node, NonTerminal, OneOrMore, Optional, Sequence, Terminal, zero_or_more,
};

fn assert_baseline_invariant(n: &Node) {
    assert_eq!(
        n.up() + n.down(),
        n.height(),
        "baseline invariant violated: up({}) + down({}) != height({})",
        n.up(),
        n.down(),
        n.height()
    );
}

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
    assert_baseline_invariant(&seq);
}

#[test]
fn empty_sequence_has_zero_body_width() {
    let seq = Node::Sequence(Sequence::new(vec![]));
    // Entry/exit stubs: 20 px total.
    assert_eq!(seq.width(), 20);
    assert_baseline_invariant(&seq);
}

#[test]
fn choice_width_is_max_child_plus_rails() {
    let a = Node::Terminal(Terminal::new("A"));
    let b = Node::Terminal(Terminal::new("LONGER_OPTION"));
    let wb = b.width();
    let ch = Node::Choice(Choice::new(0, vec![a, b]));
    // 20 px for entry/exit rails.
    assert_eq!(ch.width(), wb + 20);
    assert_baseline_invariant(&ch);
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
    assert_baseline_invariant(&ch);
}

#[test]
fn optional_adds_skip_branch() {
    let child = Node::Terminal(Terminal::new("X"));
    let cw = child.width();
    let opt = Node::Optional(Optional::new(child));
    // skip rail adds 20 px of rails; height grows by BOX_HEIGHT + VERTICAL_GAP.
    assert_eq!(opt.width(), cw + 20);
    // Terminal("X"): height 22. Optional: 22 + 22 + 10 = 54.
    assert_eq!(opt.height(), 54);
    assert_baseline_invariant(&opt);
}

#[test]
fn one_or_more_with_separator() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let sep = Node::Terminal(Terminal::new(","));
    let max_w = child.width().max(sep.width());
    let oom = Node::OneOrMore(OneOrMore::new(child, Some(sep)));
    assert_eq!(oom.width(), max_w + 20);
    assert_baseline_invariant(&oom);
}

#[test]
fn one_or_more_without_separator() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let cw = child.width();
    let oom = Node::OneOrMore(OneOrMore::new(child, None));
    assert_eq!(oom.width(), cw + 20);
    assert_baseline_invariant(&oom);
}

#[test]
fn zero_or_more_wraps_one_or_more_in_optional() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let cw = child.width();
    let z = zero_or_more(child, None);
    // OneOrMore adds 20 px of rails, Optional wraps it and adds another 20 px.
    assert_eq!(z.width(), cw + 20 + 20);
    assert!(matches!(z, Node::Optional(_)));
    assert_baseline_invariant(&z);
}

#[test]
fn nested_optional_one_or_more_pins_geometry() {
    let child = Node::Terminal(Terminal::new("X"));
    let oom = Node::OneOrMore(OneOrMore::new(child, None));
    let opt = Node::Optional(Optional::new(oom));

    // Pin exact values so future refactors are deliberate.
    // Terminal("X"): width = 8 + 40 = 48, height = 22, up = 11, down = 11
    // OneOrMore(.., None): width = 48 + 20 = 68, height = 22 + 10 + 10 = 42,
    //   up = 11, down = 11 + 10 + 10 = 31
    // Optional: width = 68 + 20 = 88, height = 42 + 22 + 10 = 74,
    //   up = 11 + 22 + 10 = 43, down = 31
    assert_eq!(opt.width(), 88);
    assert_eq!(opt.height(), 74);
    assert_eq!(opt.up(), 43);
    assert_eq!(opt.down(), 31);
    assert_baseline_invariant(&opt);
}

#[test]
fn choice_with_nonzero_default_idx_pins_up_down() {
    let a = Node::Terminal(Terminal::new("A")); // height 22, up/down 11
    let b = Node::Terminal(Terminal::new("B")); // height 22, up/down 11
    let c = Node::Terminal(Terminal::new("C")); // height 22, up/down 11
    let ch = Node::Choice(Choice::new(1, vec![a, b, c]));

    // default is middle branch (b). "A" is above → contributes to up;
    // "C" is below → contributes to down.
    // height = 22*3 + 10*2 = 86
    // up = b.up() + (a.height() + VERTICAL_GAP) = 11 + 32 = 43
    // down = b.down() + (c.height() + VERTICAL_GAP) = 11 + 32 = 43
    assert_eq!(ch.height(), 86);
    assert_eq!(ch.up(), 43);
    assert_eq!(ch.down(), 43);
    assert_baseline_invariant(&ch);
}
