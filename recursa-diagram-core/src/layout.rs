//! Railroad diagram layout primitives. Port of the tabatkins algorithm.

pub(crate) const CHAR_WIDTH: u32 = 8;
pub(crate) const HORIZONTAL_PADDING: u32 = 20; // per side → +40 total
pub(crate) const BOX_HEIGHT: u32 = 22;
pub(crate) const BASELINE_OFFSET: u32 = BOX_HEIGHT / 2; // up = down = 11

/// Horizontal spacer placed between adjacent children of a `Sequence`.
pub(crate) const HORIZONTAL_SPACER: u32 = 10;
/// Combined width of the entry + exit rails wrapping `Choice`/`Optional`/`OneOrMore`.
/// Also used as the body width of an empty `Sequence`.
pub(crate) const CHOICE_RAIL_WIDTH: u32 = 20;
/// Vertical gap between stacked branches in `Choice` and between the
/// child/separator rows of `Optional`/`OneOrMore`.
pub(crate) const VERTICAL_GAP: u32 = 10;
/// Vertical space reserved for the back-edge when `OneOrMore` has no separator.
pub(crate) const RETURN_RAIL_HEIGHT: u32 = 10;
/// Vertical space between adjacent rows of a multi-row (wrapped) `Sequence`.
/// Double `VERTICAL_GAP` so the orthogonal wrap connector has room to breathe
/// between the two baselines it joins.
pub(crate) const WRAP_ROW_GAP: u32 = VERTICAL_GAP * 2;

#[derive(Clone, Debug)]
pub enum Node {
    Terminal(Terminal),
    NonTerminal(NonTerminal),
    Sequence(Sequence),
    Choice(Choice),
    Optional(Optional),
    OneOrMore(OneOrMore),
}

impl Node {
    pub fn width(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.width,
            Node::NonTerminal(n) => n.width,
            Node::Sequence(n) => n.width,
            Node::Choice(n) => n.width,
            Node::Optional(n) => n.width,
            Node::OneOrMore(n) => n.width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.height,
            Node::NonTerminal(n) => n.height,
            Node::Sequence(n) => n.height,
            Node::Choice(n) => n.height,
            Node::Optional(n) => n.height,
            Node::OneOrMore(n) => n.height,
        }
    }

    pub fn up(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.up,
            Node::NonTerminal(n) => n.up,
            Node::Sequence(n) => n.up,
            Node::Choice(n) => n.up,
            Node::Optional(n) => n.up,
            Node::OneOrMore(n) => n.up,
        }
    }

    pub fn down(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.down,
            Node::NonTerminal(n) => n.down,
            Node::Sequence(n) => n.down,
            Node::Choice(n) => n.down,
            Node::Optional(n) => n.down,
            Node::OneOrMore(n) => n.down,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Terminal {
    pub text: String,
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
}

impl Terminal {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let width = text.chars().count() as u32 * CHAR_WIDTH + 2 * HORIZONTAL_PADDING;
        Self {
            text,
            width,
            height: BOX_HEIGHT,
            up: BASELINE_OFFSET,
            down: BASELINE_OFFSET,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NonTerminal {
    pub text: String,
    pub href: Option<String>,
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
}

impl NonTerminal {
    pub fn new(text: impl Into<String>, href: Option<String>) -> Self {
        let text = text.into();
        let width = text.chars().count() as u32 * CHAR_WIDTH + 2 * HORIZONTAL_PADDING;
        Self {
            text,
            href,
            width,
            height: BOX_HEIGHT,
            up: BASELINE_OFFSET,
            down: BASELINE_OFFSET,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sequence {
    pub children: Vec<Node>,
    /// Row-break indices for multi-row (wrapped) layout. Empty means the
    /// sequence is a single row. Each entry is the child index where the next
    /// row begins, so `children = [A,B,C,D,E]` with `rows = [2, 4]` means
    /// rows `[A,B]`, `[C,D]`, `[E]`.
    pub rows: Vec<usize>,
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
}

/// Width of a contiguous slice of children laid out on one row: sum of child
/// widths plus `HORIZONTAL_SPACER` between adjacent children. Returns 0 for
/// an empty slice.
fn row_width(children: &[Node]) -> u32 {
    if children.is_empty() {
        return 0;
    }
    let sum: u32 = children.iter().map(|c| c.width()).sum();
    sum + HORIZONTAL_SPACER * (children.len() as u32 - 1)
}

impl Sequence {
    pub fn new(children: Vec<Node>) -> Self {
        let width = if children.is_empty() {
            CHOICE_RAIL_WIDTH
        } else {
            row_width(&children)
        };
        let height = children
            .iter()
            .map(|c| c.height())
            .max()
            .unwrap_or(BOX_HEIGHT);
        let up = children
            .iter()
            .map(|c| c.up())
            .max()
            .unwrap_or(BASELINE_OFFSET);
        let down = children
            .iter()
            .map(|c| c.down())
            .max()
            .unwrap_or(BASELINE_OFFSET);
        Self {
            children,
            rows: Vec::new(),
            width,
            height,
            up,
            down,
        }
    }

    /// Construct a `Sequence` that greedily wraps its children into multiple
    /// rows so that no single row exceeds `max_width` (sum of child widths
    /// plus inter-child spacers).
    ///
    /// - `max_width == 0` disables wrapping; the result is identical to
    ///   [`Sequence::new`].
    /// - If all children already fit on one row, behaves identically to
    ///   [`Sequence::new`] and leaves `rows` empty.
    /// - A single child wider than `max_width` occupies its own row. The
    ///   overall `width` of such a `Sequence` may exceed `max_width`.
    pub fn wrapped(children: Vec<Node>, max_width: u32) -> Self {
        // Disabled, empty, or already fits: single-row fallback.
        if max_width == 0 || children.is_empty() || row_width(&children) <= max_width {
            return Self::new(children);
        }

        // Greedy packing: start a new row whenever adding the next child would
        // push the current row width past `max_width`. An oversized single
        // child gets its own row (the branch below starts a new row for it
        // and then the next iteration starts another).
        let mut rows: Vec<usize> = Vec::new();
        let mut row_start = 0usize;
        let mut i = 0usize;
        while i < children.len() {
            // Try to extend the current row by one more child.
            let candidate = &children[row_start..=i];
            let w = row_width(candidate);
            if w <= max_width || i == row_start {
                // Fits, or the row is still empty (pathological oversized
                // child must still be admitted to avoid infinite loop).
                i += 1;
            } else {
                // Doesn't fit: close current row at i (exclusive), start a new
                // row beginning at i. `i` is not advanced so the child is
                // retried on the next row.
                rows.push(i);
                row_start = i;
            }
        }

        // Per-row geometry.
        let num_rows = rows.len() + 1;
        let mut row_slices: Vec<&[Node]> = Vec::with_capacity(num_rows);
        let mut prev = 0usize;
        for &brk in &rows {
            row_slices.push(&children[prev..brk]);
            prev = brk;
        }
        row_slices.push(&children[prev..]);

        let max_row_w = row_slices.iter().map(|r| row_width(r)).max().unwrap_or(0);
        // Add CHOICE_RAIL_WIDTH so the wrap back-rail has margin on each side.
        let width = max_row_w + CHOICE_RAIL_WIDTH;

        let row_up: Vec<u32> = row_slices
            .iter()
            .map(|r| r.iter().map(|c| c.up()).max().unwrap_or(BASELINE_OFFSET))
            .collect();
        let row_down: Vec<u32> = row_slices
            .iter()
            .map(|r| r.iter().map(|c| c.down()).max().unwrap_or(BASELINE_OFFSET))
            .collect();
        let strips_sum: u32 = row_up.iter().zip(&row_down).map(|(u, d)| u + d).sum();
        let height = strips_sum + WRAP_ROW_GAP * (num_rows as u32 - 1);

        // Root baseline sits at the baseline of row 0, measured from the top
        // of the drawn area.
        let up = row_up[0];
        let down = height - up;

        Self {
            children,
            rows,
            width,
            height,
            up,
            down,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub children: Vec<Node>,
    pub default_idx: usize,
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
}

impl Choice {
    pub fn new(default_idx: usize, children: Vec<Node>) -> Self {
        assert!(!children.is_empty(), "Choice must have at least one child");
        assert!(
            default_idx < children.len(),
            "Choice default_idx {default_idx} out of bounds (len = {})",
            children.len()
        );
        let width = children.iter().map(|c| c.width()).max().unwrap() + CHOICE_RAIL_WIDTH;
        let height: u32 = children.iter().map(|c| c.height()).sum::<u32>()
            + VERTICAL_GAP * (children.len() as u32 - 1);
        // The default branch sits on the baseline; branches above contribute
        // to `up`, branches below to `down`.
        let default_up = children[default_idx].up();
        let default_down = children[default_idx].down();
        let above: u32 = children[..default_idx]
            .iter()
            .map(|c| c.height() + VERTICAL_GAP)
            .sum();
        let below: u32 = children[default_idx + 1..]
            .iter()
            .map(|c| c.height() + VERTICAL_GAP)
            .sum();
        let up = default_up + above;
        let down = default_down + below;
        Self {
            default_idx,
            children,
            width,
            height,
            up,
            down,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Optional {
    pub child: Box<Node>,
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
}

impl Optional {
    pub fn new(child: Node) -> Self {
        let width = child.width() + CHOICE_RAIL_WIDTH;
        let height = child.height() + BOX_HEIGHT + VERTICAL_GAP;
        // The skip rail sits above the child, adding a full box height + gap to `up`.
        let up = child.up() + BOX_HEIGHT + VERTICAL_GAP;
        let down = child.down();
        Self {
            child: Box::new(child),
            width,
            height,
            up,
            down,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OneOrMore {
    pub child: Box<Node>,
    pub separator: Option<Box<Node>>,
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
}

impl OneOrMore {
    pub fn new(child: Node, separator: Option<Node>) -> Self {
        let sep_w = separator.as_ref().map(|s| s.width()).unwrap_or(0);
        let width = child.width().max(sep_w) + CHOICE_RAIL_WIDTH;
        let sep_h = separator.as_ref().map_or(RETURN_RAIL_HEIGHT, Node::height);
        let height = child.height() + sep_h + VERTICAL_GAP;
        // The child sits on the baseline; the separator (or implicit return rail)
        // sits below it, adding to `down`.
        let up = child.up();
        let down = child.down() + sep_h + VERTICAL_GAP;
        Self {
            child: Box::new(child),
            separator: separator.map(Box::new),
            width,
            height,
            up,
            down,
        }
    }
}

/// Convenience constructor for a zero-or-more repetition.
///
/// Returns `Optional(OneOrMore(child, sep))` so call sites can express
/// zero-or-more without manually nesting layout nodes.
pub fn zero_or_more(child: Node, sep: Option<Node>) -> Node {
    Node::Optional(Optional::new(Node::OneOrMore(OneOrMore::new(child, sep))))
}
