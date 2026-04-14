//! Railroad diagram layout primitives. Port of the tabatkins algorithm.

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
        // Character width ~8 px + horizontal padding 20 px each side.
        let width = (text.chars().count() as u32) * 8 + 40;
        Self {
            text,
            width,
            height: 22,
            up: 11,
            down: 11,
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
        let width = (text.chars().count() as u32) * 8 + 40;
        Self {
            text,
            href,
            width,
            height: 22,
            up: 11,
            down: 11,
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
