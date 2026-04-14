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
            Node::Choice(_) => unimplemented!("layout geometry not yet implemented"),
            Node::Optional(_) => unimplemented!("layout geometry not yet implemented"),
            Node::OneOrMore(_) => unimplemented!("layout geometry not yet implemented"),
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.height,
            Node::NonTerminal(n) => n.height,
            Node::Sequence(n) => n.height,
            Node::Choice(_) => unimplemented!("layout geometry not yet implemented"),
            Node::Optional(_) => unimplemented!("layout geometry not yet implemented"),
            Node::OneOrMore(_) => unimplemented!("layout geometry not yet implemented"),
        }
    }

    pub fn up(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.up,
            Node::NonTerminal(n) => n.up,
            Node::Sequence(n) => n.up,
            Node::Choice(_) => unimplemented!("layout geometry not yet implemented"),
            Node::Optional(_) => unimplemented!("layout geometry not yet implemented"),
            Node::OneOrMore(_) => unimplemented!("layout geometry not yet implemented"),
        }
    }

    pub fn down(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.down,
            Node::NonTerminal(n) => n.down,
            Node::Sequence(n) => n.down,
            Node::Choice(_) => unimplemented!("layout geometry not yet implemented"),
            Node::Optional(_) => unimplemented!("layout geometry not yet implemented"),
            Node::OneOrMore(_) => unimplemented!("layout geometry not yet implemented"),
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
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
}

impl Sequence {
    pub fn new(children: Vec<Node>) -> Self {
        let width = if children.is_empty() {
            CHOICE_RAIL_WIDTH
        } else {
            let child_sum: u32 = children.iter().map(|c| c.width()).sum();
            child_sum + HORIZONTAL_SPACER * (children.len() as u32 - 1)
        };
        let height = children
            .iter()
            .map(|c| c.height())
            .max()
            .unwrap_or(BOX_HEIGHT);
        // TODO(phase-3): verify up/down against rendered svg
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

#[derive(Clone, Debug)]
pub struct Optional {
    pub child: Box<Node>,
    pub width: u32,
    pub height: u32,
    pub up: u32,
    pub down: u32,
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
