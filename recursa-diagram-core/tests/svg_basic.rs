//! Smoke tests for the rendered SVG.
//!
//! These tests no longer pin the exact pixel geometry: we delegate layout
//! to the `railroad` crate, so the output shape (coordinates, connector
//! paths, CSS classes) is that crate's concern. What we still own is the
//! tree-to-tree translation from our `Node` IR into railroad's IR, so the
//! tests focus on: (1) basic well-formedness, (2) every expected label
//! appearing in the output, (3) relative ordering where it follows from
//! our IR choices.

use recursa_diagram_core::{
    layout::{Choice, Node, NonTerminal, OneOrMore, Optional, Sequence, Terminal},
    render,
};

/// Cheap well-formedness check: every `<tag>` open must be balanced by a
/// matching `</tag>` or self-close `/>`. Counts-based; does NOT catch
/// tag-name mismatches. Use as a smoke signal only — real structural
/// correctness is covered by the snapshot test.
fn assert_balanced_tags(svg: &str) {
    // Strip XML comments first so `<!--` is not confused with an open tag.
    let mut stripped = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find("<!--") {
        stripped.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let end = after.find("-->").expect("unterminated comment");
        rest = &after[end + 3..];
    }
    stripped.push_str(rest);
    let s = stripped.as_str();

    let opens = s.matches('<').count();
    let self_close = s.matches("/>").count();
    let close = s.matches("</").count();
    assert_eq!(
        opens,
        self_close + 2 * close,
        "unbalanced tags: opens={opens} self_close={self_close} close={close}"
    );
}

fn find_label(svg: &str, label: &str) -> usize {
    svg.find(label)
        .unwrap_or_else(|| panic!("label {label} not found in svg:\n{svg}"))
}

#[test]
fn terminal_svg_contains_text() {
    let svg = render(&Node::Terminal(Terminal::new("SELECT")));
    assert!(svg.starts_with("<svg"), "should be an svg: {svg}");
    assert!(svg.contains("SELECT"), "should contain the literal: {svg}");
    assert!(svg.trim_end().ends_with("</svg>"));
    assert_balanced_tags(&svg);
}

#[test]
fn non_terminal_svg_without_href() {
    let svg = render(&Node::NonTerminal(NonTerminal::new("Expr", None)));
    assert!(svg.contains("Expr"));
    assert!(!svg.contains("<a "));
    assert_balanced_tags(&svg);
}

#[test]
fn non_terminal_svg_with_href_wraps_in_anchor() {
    let svg = render(&Node::NonTerminal(NonTerminal::new(
        "Expr",
        Some("Expr.html".into()),
    )));
    assert!(svg.contains("<a "), "expected anchor tag: {svg}");
    assert!(
        svg.contains("Expr.html"),
        "expected href target in svg: {svg}"
    );
    assert!(svg.contains("Expr"));
    assert_balanced_tags(&svg);
}

#[test]
fn sequence_renders_children_in_order() {
    let seq = Node::Sequence(Sequence::new(vec![
        Node::Terminal(Terminal::new("SELECT")),
        Node::NonTerminal(NonTerminal::new("Column", None)),
    ]));
    let svg = render(&seq);
    assert_balanced_tags(&svg);
    let i_select = find_label(&svg, "SELECT");
    let i_column = find_label(&svg, "Column");
    assert!(i_select < i_column, "SELECT should appear before Column");
}

#[test]
fn terminal_text_is_xml_escaped() {
    // We don't pin the exact escape form (`&apos;` vs `&#x27;` vary by
    // library); we just require that the raw `<` and `&` are not present
    // as active markup in the body.
    let svg = render(&Node::Terminal(Terminal::new("a<b&c\"'")));
    assert!(
        !svg.contains("a<b"),
        "raw < leaked into text content: {svg}"
    );
    assert!(svg.contains("&lt;"), "expected escaped <: {svg}");
    assert!(svg.contains("&amp;"), "expected escaped &: {svg}");
    assert_balanced_tags(&svg);
}

#[test]
fn non_terminal_href_is_escaped() {
    let svg = render(&Node::NonTerminal(NonTerminal::new(
        "X",
        Some("a&b.html".into()),
    )));
    assert!(svg.contains("a&amp;b.html"), "expected escaped href: {svg}");
    assert_balanced_tags(&svg);
}

#[test]
fn choice_emits_text_for_every_branch() {
    // Default-branch handling: we move `default_idx` to the front when
    // converting to the railroad crate's Choice, so the inline main-path
    // branch is the one we declared as default. Verify that every branch's
    // label still shows up in the output and that the default branch is
    // emitted before the other branches in source order.
    let ch = Node::Choice(Choice::new(
        1,
        vec![
            Node::Terminal(Terminal::new("AAA")),
            Node::Terminal(Terminal::new("BBB")),
            Node::Terminal(Terminal::new("CCC")),
        ],
    ));
    let svg = render(&ch);
    assert_balanced_tags(&svg);
    let i_a = find_label(&svg, "AAA");
    let i_b = find_label(&svg, "BBB");
    let i_c = find_label(&svg, "CCC");
    // Default (BBB) first, then the rest in original order (AAA, CCC).
    assert!(
        i_b < i_a && i_b < i_c,
        "default branch BBB should be emitted first: a={i_a} b={i_b} c={i_c}"
    );
    assert!(i_a < i_c, "non-default branches preserve source order");
}

#[test]
fn optional_renders_child_label() {
    let opt = Node::Optional(Optional::new(Node::Terminal(Terminal::new("MAYBE"))));
    let svg = render(&opt);
    assert_balanced_tags(&svg);
    assert!(svg.contains("MAYBE"));
}

#[test]
fn one_or_more_without_separator_renders_child() {
    let om = Node::OneOrMore(OneOrMore::new(Node::Terminal(Terminal::new("ITEM")), None));
    let svg = render(&om);
    assert_balanced_tags(&svg);
    assert!(svg.contains("ITEM"));
}

#[test]
fn one_or_more_with_separator_emits_both_labels() {
    let om = Node::OneOrMore(OneOrMore::new(
        Node::Terminal(Terminal::new("COL")),
        Some(Node::Terminal(Terminal::new("COMMA"))),
    ));
    let svg = render(&om);
    assert_balanced_tags(&svg);
    assert!(svg.contains("COL"));
    assert!(svg.contains("COMMA"));
}

#[test]
fn wrapped_sequence_emits_every_child() {
    // Wrapping is translated to a Stack-of-Sequences in the railroad crate;
    // we only assert that every child label survives the conversion.
    let children: Vec<Node> = (0..5)
        .map(|i| Node::Terminal(Terminal::new(format!("X{i}"))))
        .collect();
    let seq = Node::Sequence(Sequence::wrapped(children, 160));
    let svg = render(&seq);
    assert_balanced_tags(&svg);
    for i in 0..5 {
        let label = format!("X{i}");
        assert!(svg.contains(&label), "missing {label}: {svg}");
    }
}

#[test]
fn empty_sequence_renders_valid_wrapper() {
    let svg = render(&Node::Sequence(Sequence::new(vec![])));
    assert!(svg.starts_with("<svg"), "not an svg: {svg}");
    assert!(svg.trim_end().ends_with("</svg>"), "unclosed svg: {svg}");
    assert_balanced_tags(&svg);
}

#[test]
fn empty_wrapped_sequence_matches_empty_new_sequence() {
    // Documents the empty-input contract: Sequence::wrapped(vec![], _)
    // short-circuits to Sequence::new(vec![]), which must render identically.
    let from_new = render(&Node::Sequence(Sequence::new(vec![])));
    let from_wrapped = render(&Node::Sequence(Sequence::wrapped(vec![], 1200)));
    assert_eq!(from_wrapped, from_new);
}
