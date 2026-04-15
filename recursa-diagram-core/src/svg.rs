//! SVG serialization for railroad layout trees.
//!
//! Converts our `Node` IR into the `railroad` crate's node tree and renders
//! with the crate's default stylesheet. The crate produces the final SVG
//! and handles its own layout geometry.

use railroad::Node as RrNode;

use crate::layout::{Choice, Node, NonTerminal, OneOrMore, Optional, Sequence, Terminal, Token};

type RrBox = Box<dyn RrNode>;

/// Custom CSS appended after the railroad crate's default stylesheet.
/// Adds colour distinctions for keyword vs token vs production:
///
/// - `.terminal` (keyword): default railroad colouring.
/// - `.terminal.token`: punctuation/operator tokens — orange fill so
///   they're visually distinct from keyword terminals.
/// - `.non-terminal`: default railroad colouring (productions).
const EXTRA_CSS: &str = r#"
svg.railroad g.terminal.token rect { fill: #ffe7c2; stroke: #a0522d; }
svg.railroad g.terminal.token text { fill: #5a2a00; }
"#;

/// Serialize a layout tree rooted at `root` into a self-contained SVG
/// document produced by the `railroad` crate with its default stylesheet
/// plus our token-class overrides appended.
pub fn render(root: &Node) -> String {
    let rr = to_railroad(root);
    let mut dia = railroad::Diagram::with_default_css(rr);
    dia.add_css(EXTRA_CSS);
    dia.to_string()
}

fn to_railroad(node: &Node) -> RrBox {
    match node {
        Node::Terminal(Terminal { text, .. }) => Box::new(railroad::Terminal::new(text.clone())),
        Node::Token(Token { text, .. }) => {
            // Render as a railroad Terminal (rounded-rect) but extend the
            // CSS class list so EXTRA_CSS in `render()` can target it. The
            // upstream Terminal::new always inserts `class = "terminal"`;
            // we overwrite it with `terminal token` so both classes apply.
            let mut t = railroad::Terminal::new(text.clone());
            t.attr("class".to_owned())
                .and_modify(|v| *v = "terminal token".to_owned())
                .or_insert_with(|| "terminal token".to_owned());
            Box::new(t)
        }
        Node::NonTerminal(NonTerminal { text, href, .. }) => {
            let nt = railroad::NonTerminal::new(text.clone());
            match href {
                Some(h) => Box::new(railroad::Link::new(boxed(nt), h.clone())),
                None => Box::new(nt),
            }
        }
        Node::Sequence(Sequence { children, rows, .. }) => {
            if rows.is_empty() {
                Box::new(railroad::Sequence::new(
                    children.iter().map(to_railroad).collect::<Vec<RrBox>>(),
                ))
            } else {
                // Wrapped sequence: group children into per-row Sequences and
                // emit them as a Stack (the railroad crate's vertical layout
                // primitive). The row break indices come from our greedy
                // wrap pass in `layout::Sequence::wrapped`.
                let mut stack_rows: Vec<RrBox> = Vec::with_capacity(rows.len() + 1);
                let mut prev = 0usize;
                for &brk in rows {
                    stack_rows.push(row_sequence(&children[prev..brk]));
                    prev = brk;
                }
                stack_rows.push(row_sequence(&children[prev..]));
                Box::new(railroad::Stack::new(stack_rows))
            }
        }
        Node::Choice(Choice {
            children,
            default_idx,
            ..
        }) => {
            // The railroad crate draws the first child inline on the main
            // path. Move `default_idx` to the front so our "default branch"
            // semantics survive the conversion.
            let mut ordered: Vec<RrBox> = Vec::with_capacity(children.len());
            ordered.push(to_railroad(&children[*default_idx]));
            for (i, c) in children.iter().enumerate() {
                if i != *default_idx {
                    ordered.push(to_railroad(c));
                }
            }
            Box::new(railroad::Choice::new(ordered))
        }
        Node::Optional(Optional { child, .. }) => {
            Box::new(railroad::Optional::new(to_railroad(child)))
        }
        Node::OneOrMore(OneOrMore {
            child, separator, ..
        }) => {
            let inner = to_railroad(child);
            let sep: RrBox = match separator {
                Some(s) => to_railroad(s),
                None => Box::new(railroad::Empty),
            };
            Box::new(railroad::Repeat::new(inner, sep))
        }
    }
}

fn row_sequence(children: &[Node]) -> RrBox {
    Box::new(railroad::Sequence::new(
        children.iter().map(to_railroad).collect::<Vec<RrBox>>(),
    ))
}

fn boxed<N: RrNode + 'static>(n: N) -> RrBox {
    Box::new(n)
}
