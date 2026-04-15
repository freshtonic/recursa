//! SVG serialization for railroad layout trees.
//!
//! Converts our `Node` IR into the `railroad` crate's node tree and renders
//! with the crate's default stylesheet. The crate produces the final SVG
//! and handles its own layout geometry.

use railroad::Node as RrNode;

use crate::layout::{Choice, Node, NonTerminal, OneOrMore, Optional, Sequence, Terminal};

type RrBox = Box<dyn RrNode>;

/// Serialize a layout tree rooted at `root` into a self-contained SVG
/// document produced by the `railroad` crate with its default stylesheet.
pub fn render(root: &Node) -> String {
    let rr = to_railroad(root);
    railroad::Diagram::with_default_css(rr).to_string()
}

fn to_railroad(node: &Node) -> RrBox {
    match node {
        Node::Terminal(Terminal { text, .. }) => Box::new(railroad::Terminal::new(text.clone())),
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
