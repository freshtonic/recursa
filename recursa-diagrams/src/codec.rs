//! Compact, dependency-free text codec for published diagram plans.
//!
//! The generator writes one encoded plan next to the generated source; the
//! facade decodes it while `cargo doc` expands generated attributes. The
//! format is a private matching-version detail: a version header line, then a
//! whitespace-separated token stream of keywords, integers, quoted strings,
//! and parenthesized productions. Quoted strings escape `\\`, `"`, and the
//! control characters newline, carriage return, and tab; every other scalar is
//! written as UTF-8.

use std::fmt;

use crate::{DiagramNode, DiagramPlan, NodeDiagramPlan, TokenDiagramPlan};

/// Version header that opens every encoded plan.
const HEADER: &str = "recursa-diagram-plan-v1";

/// Why an encoded plan could not be decoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodeError {
    /// Human-readable description naming the offending token or structure.
    message: String,
}

impl DecodeError {
    /// Builds one error from a description.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid diagram plan: {}", self.message)
    }
}

impl std::error::Error for DecodeError {}

impl DiagramPlan {
    /// Encodes the plan into the private text format read by [`Self::decode`].
    pub fn encode(&self) -> String {
        let mut writer = Writer::default();
        writer.line(HEADER);
        writer.word("plan");
        writer.string(&self.grammar_name);
        writer.number(self.anchor_depth);
        writer.newline();
        writer.word("nodes");
        writer.number(self.nodes.len());
        writer.newline();
        for node in &self.nodes {
            writer.word("node");
            writer.string(&node.semantic_key);
            writer.string(&node.ident);
            writer.optional_string(node.public_path.as_deref());
            writer.newline();
            writer.production(&node.production);
            writer.newline();
        }
        writer.word("tokens");
        writer.number(self.tokens.len());
        writer.newline();
        for token in &self.tokens {
            writer.word("token");
            writer.string(&token.semantic_key);
            writer.string(&token.ident);
            writer.string(&token.pattern);
            writer.string(&token.label);
            writer.optional_string(token.public_path.as_deref());
            writer.newline();
            writer.production(&token.production);
            writer.newline();
        }
        writer.word("fixed");
        writer.number(self.fixed_tokens.len());
        writer.newline();
        for (name, spelling) in &self.fixed_tokens {
            writer.word("fixed_token");
            writer.string(name);
            writer.string(spelling);
            writer.newline();
        }
        writer.word("end");
        writer.newline();
        writer.output
    }

    /// Decodes a plan written by [`Self::encode`].
    pub fn decode(encoded: &str) -> Result<Self, DecodeError> {
        let (header, body) = encoded
            .split_once('\n')
            .ok_or_else(|| DecodeError::new("missing version header"))?;
        if header.trim_end_matches('\r') != HEADER {
            return Err(DecodeError::new(format!(
                "unsupported version header `{header}`"
            )));
        }
        let tokens = tokenize(body)?;
        let mut reader = Reader { tokens, index: 0 };
        reader.keyword("plan")?;
        let grammar_name = reader.string()?;
        let anchor_depth = reader.number()?;
        reader.keyword("nodes")?;
        let node_count = reader.number()?;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            reader.keyword("node")?;
            nodes.push(NodeDiagramPlan {
                semantic_key: reader.string()?,
                ident: reader.string()?,
                public_path: reader.optional_string()?,
                production: reader.production()?,
            });
        }
        reader.keyword("tokens")?;
        let token_count = reader.number()?;
        let mut tokens = Vec::with_capacity(token_count);
        for _ in 0..token_count {
            reader.keyword("token")?;
            tokens.push(TokenDiagramPlan {
                semantic_key: reader.string()?,
                ident: reader.string()?,
                pattern: reader.string()?,
                label: reader.string()?,
                public_path: reader.optional_string()?,
                production: reader.production()?,
            });
        }
        reader.keyword("fixed")?;
        let fixed_count = reader.number()?;
        let mut fixed_tokens = Vec::with_capacity(fixed_count);
        for _ in 0..fixed_count {
            reader.keyword("fixed_token")?;
            let name = reader.string()?;
            let spelling = reader.string()?;
            fixed_tokens.push((name, spelling));
        }
        reader.keyword("end")?;
        if reader.index != reader.tokens.len() {
            return Err(DecodeError::new("trailing tokens after `end`"));
        }
        Ok(Self {
            grammar_name,
            anchor_depth,
            nodes,
            tokens,
            fixed_tokens,
        })
    }
}

/// Accumulates encoded text with single-space token separation.
#[derive(Default)]
struct Writer {
    /// Encoded text produced so far.
    output: String,
    /// Whether the current line already holds a token needing a separator.
    mid_line: bool,
}

impl Writer {
    /// Appends one raw line, terminating any open line first.
    fn line(&mut self, text: &str) {
        self.newline();
        self.output.push_str(text);
        self.output.push('\n');
    }

    /// Ends the current line when one is open.
    fn newline(&mut self) {
        if self.mid_line {
            self.output.push('\n');
            self.mid_line = false;
        }
    }

    /// Appends one bare keyword or punctuation token.
    fn word(&mut self, word: &str) {
        if self.mid_line {
            self.output.push(' ');
        }
        self.output.push_str(word);
        self.mid_line = true;
    }

    /// Appends one decimal integer token.
    fn number(&mut self, value: usize) {
        self.word(&value.to_string());
    }

    /// Appends one quoted, escaped string token.
    fn string(&mut self, value: &str) {
        let mut quoted = String::with_capacity(value.len() + 2);
        quoted.push('"');
        for scalar in value.chars() {
            match scalar {
                '\\' => quoted.push_str("\\\\"),
                '"' => quoted.push_str("\\\""),
                '\n' => quoted.push_str("\\n"),
                '\r' => quoted.push_str("\\r"),
                '\t' => quoted.push_str("\\t"),
                other => quoted.push(other),
            }
        }
        quoted.push('"');
        self.word(&quoted);
    }

    /// Appends a quoted string, or `-` for `None`.
    fn optional_string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => self.string(value),
            None => self.word("-"),
        }
    }

    /// Appends one production as a parenthesized tree.
    fn production(&mut self, node: &DiagramNode) {
        match node {
            DiagramNode::Empty => self.word("empty"),
            DiagramNode::Terminal(text) => {
                self.word("(t");
                self.string(text);
                self.word(")");
            }
            DiagramNode::NonTerminal {
                label,
                link,
                reference,
            } => {
                self.word("(nt");
                self.string(label);
                self.optional_string(link.as_deref());
                self.optional_string(reference.as_deref());
                self.word(")");
            }
            DiagramNode::Inline { label, link, body } => {
                self.word("(inline");
                self.string(label);
                self.optional_string(link.as_deref());
                self.production(body);
                self.word(")");
            }
            DiagramNode::Comment(text) => {
                self.word("(comment");
                self.string(text);
                self.word(")");
            }
            DiagramNode::LinkedComment { label, link } => {
                self.word("(lcomment");
                self.string(label);
                self.optional_string(link.as_deref());
                self.word(")");
            }
            DiagramNode::Sequence(nodes) => {
                self.word("(seq");
                for node in nodes {
                    self.production(node);
                }
                self.word(")");
            }
            DiagramNode::Choice(nodes) => {
                self.word("(choice");
                for node in nodes {
                    self.production(node);
                }
                self.word(")");
            }
            DiagramNode::Optional(node) => {
                self.word("(opt");
                self.production(node);
                self.word(")");
            }
            DiagramNode::Repeat { item, separator } => {
                self.word("(rep");
                self.production(item);
                self.production(separator);
                self.word(")");
            }
        }
    }
}

/// One lexical unit of the encoded body.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    /// A bare keyword, integer, `-`, or `(`-prefixed constructor name.
    Word(String),
    /// A decoded quoted string.
    Text(String),
    /// A closing parenthesis.
    Close,
}

/// Splits the encoded body into tokens, decoding string escapes.
fn tokenize(body: &str) -> Result<Vec<Token>, DecodeError> {
    let mut tokens = Vec::new();
    let mut scalars = body.chars().peekable();
    while let Some(scalar) = scalars.next() {
        match scalar {
            scalar if scalar.is_whitespace() => {}
            ')' => tokens.push(Token::Close),
            '"' => {
                let mut text = String::new();
                loop {
                    match scalars.next() {
                        None => return Err(DecodeError::new("unterminated string")),
                        Some('"') => break,
                        Some('\\') => match scalars.next() {
                            Some('\\') => text.push('\\'),
                            Some('"') => text.push('"'),
                            Some('n') => text.push('\n'),
                            Some('r') => text.push('\r'),
                            Some('t') => text.push('\t'),
                            other => {
                                return Err(DecodeError::new(format!(
                                    "unknown string escape {other:?}"
                                )));
                            }
                        },
                        Some(other) => text.push(other),
                    }
                }
                tokens.push(Token::Text(text));
            }
            first => {
                let mut word = String::from(first);
                while let Some(next) = scalars.peek() {
                    if next.is_whitespace() || *next == ')' || *next == '"' {
                        break;
                    }
                    word.push(*next);
                    scalars.next();
                }
                tokens.push(Token::Word(word));
            }
        }
    }
    Ok(tokens)
}

/// Cursor over the token stream with structural expectations.
struct Reader {
    /// Complete token stream of the encoded body.
    tokens: Vec<Token>,
    /// Index of the next unread token.
    index: usize,
}

impl Reader {
    /// Consumes the next token, failing at end of input.
    fn next(&mut self) -> Result<Token, DecodeError> {
        let token = self
            .tokens
            .get(self.index)
            .cloned()
            .ok_or_else(|| DecodeError::new("unexpected end of plan"))?;
        self.index += 1;
        Ok(token)
    }

    /// Consumes one exact keyword.
    fn keyword(&mut self, expected: &str) -> Result<(), DecodeError> {
        match self.next()? {
            Token::Word(word) if word == expected => Ok(()),
            other => Err(DecodeError::new(format!(
                "expected `{expected}`, found {other:?}"
            ))),
        }
    }

    /// Consumes one decimal integer.
    fn number(&mut self) -> Result<usize, DecodeError> {
        match self.next()? {
            Token::Word(word) => word
                .parse()
                .map_err(|_| DecodeError::new(format!("expected an integer, found `{word}`"))),
            other => Err(DecodeError::new(format!(
                "expected an integer, found {other:?}"
            ))),
        }
    }

    /// Consumes one quoted string.
    fn string(&mut self) -> Result<String, DecodeError> {
        match self.next()? {
            Token::Text(text) => Ok(text),
            other => Err(DecodeError::new(format!(
                "expected a string, found {other:?}"
            ))),
        }
    }

    /// Consumes one quoted string or the `-` placeholder.
    fn optional_string(&mut self) -> Result<Option<String>, DecodeError> {
        match self.next()? {
            Token::Text(text) => Ok(Some(text)),
            Token::Word(word) if word == "-" => Ok(None),
            other => Err(DecodeError::new(format!(
                "expected a string or `-`, found {other:?}"
            ))),
        }
    }

    /// Consumes the closing parenthesis of one constructor.
    fn close(&mut self) -> Result<(), DecodeError> {
        match self.next()? {
            Token::Close => Ok(()),
            other => Err(DecodeError::new(format!("expected `)`, found {other:?}"))),
        }
    }

    /// Consumes productions until the constructor's closing parenthesis.
    fn productions_until_close(&mut self) -> Result<Vec<DiagramNode>, DecodeError> {
        let mut nodes = Vec::new();
        while self.tokens.get(self.index) != Some(&Token::Close) {
            nodes.push(self.production()?);
        }
        self.close()?;
        Ok(nodes)
    }

    /// Consumes one production tree.
    fn production(&mut self) -> Result<DiagramNode, DecodeError> {
        let constructor = match self.next()? {
            Token::Word(word) => word,
            other => Err(DecodeError::new(format!(
                "expected a production, found {other:?}"
            )))?,
        };
        let node = match constructor.as_str() {
            "empty" => return Ok(DiagramNode::Empty),
            "(t" => DiagramNode::Terminal(self.string()?),
            "(nt" => DiagramNode::NonTerminal {
                label: self.string()?,
                link: self.optional_string()?,
                reference: self.optional_string()?,
            },
            "(inline" => DiagramNode::Inline {
                label: self.string()?,
                link: self.optional_string()?,
                body: Box::new(self.production()?),
            },
            "(comment" => DiagramNode::Comment(self.string()?),
            "(lcomment" => DiagramNode::LinkedComment {
                label: self.string()?,
                link: self.optional_string()?,
            },
            "(seq" => return Ok(DiagramNode::Sequence(self.productions_until_close()?)),
            "(choice" => return Ok(DiagramNode::Choice(self.productions_until_close()?)),
            "(opt" => DiagramNode::Optional(Box::new(self.production()?)),
            "(rep" => DiagramNode::Repeat {
                item: Box::new(self.production()?),
                separator: Box::new(self.production()?),
            },
            other => {
                return Err(DecodeError::new(format!(
                    "unknown production constructor `{other}`"
                )));
            }
        };
        self.close()?;
        Ok(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> DiagramPlan {
        DiagramPlan {
            grammar_name: "Demo \"quoted\"".to_owned(),
            anchor_depth: 2,
            nodes: vec![
                NodeDiagramPlan {
                    semantic_key: "grammar::Record".to_owned(),
                    ident: "Record".to_owned(),
                    public_path: Some("struct.Record.html".to_owned()),
                    production: DiagramNode::Sequence(vec![
                        DiagramNode::Terminal("(\\ \"tab\t\" )".to_owned()),
                        DiagramNode::Inline {
                            label: "Pair".to_owned(),
                            link: None,
                            body: Box::new(DiagramNode::Choice(vec![
                                DiagramNode::Empty,
                                DiagramNode::Optional(Box::new(DiagramNode::Comment(
                                    "new\nline".to_owned(),
                                ))),
                            ])),
                        },
                        DiagramNode::Repeat {
                            item: Box::new(DiagramNode::NonTerminal {
                                label: "Item".to_owned(),
                                link: Some("struct.Item.html".to_owned()),
                                reference: Some("grammar::Item<T>".to_owned()),
                            }),
                            separator: Box::new(DiagramNode::Empty),
                        },
                        DiagramNode::LinkedComment {
                            label: "prefix Neg".to_owned(),
                            link: Some("enum.Expr.html#variant.Neg".to_owned()),
                        },
                    ]),
                },
                NodeDiagramPlan {
                    semantic_key: "grammar::Hidden".to_owned(),
                    ident: "Hidden".to_owned(),
                    public_path: None,
                    production: DiagramNode::Empty,
                },
            ],
            tokens: vec![TokenDiagramPlan {
                semantic_key: "grammar::Number".to_owned(),
                ident: "Number".to_owned(),
                pattern: "[0-9]+\\r".to_owned(),
                label: "digits ✓".to_owned(),
                public_path: None,
                production: DiagramNode::Terminal("digits ✓".to_owned()),
            }],
            fixed_tokens: vec![
                ("Comma".to_owned(), ",".to_owned()),
                ("Select".to_owned(), "SELECT".to_owned()),
            ],
        }
    }

    #[test]
    fn plans_round_trip_through_the_text_codec() {
        let plan = sample_plan();
        let encoded = plan.encode();
        assert!(encoded.starts_with("recursa-diagram-plan-v1\n"));
        assert_eq!(encoded, plan.encode(), "encoding is deterministic");
        assert_eq!(DiagramPlan::decode(&encoded).expect("round trip"), plan);
    }

    #[test]
    fn empty_plans_round_trip() {
        let plan = DiagramPlan {
            grammar_name: String::new(),
            anchor_depth: 0,
            nodes: Vec::new(),
            tokens: Vec::new(),
            fixed_tokens: Vec::new(),
        };
        assert_eq!(
            DiagramPlan::decode(&plan.encode()).expect("round trip"),
            plan
        );
    }

    #[test]
    fn malformed_plans_are_rejected_with_a_reason() {
        let plan = sample_plan();
        let encoded = plan.encode();

        let error = DiagramPlan::decode("").expect_err("missing header");
        assert!(error.to_string().contains("missing version header"));

        let wrong = encoded.replacen("v1", "v0", 1);
        let error = DiagramPlan::decode(&wrong).expect_err("wrong header");
        assert!(error.to_string().contains("unsupported version header"));

        let truncated = &encoded[..encoded.len() - 4];
        let error = DiagramPlan::decode(truncated).expect_err("truncated");
        assert!(error.to_string().contains("unexpected end of plan"));

        let misplaced = encoded.replacen("fixed_token", "end", 1);
        let error = DiagramPlan::decode(&misplaced).expect_err("misplaced end");
        assert!(error.to_string().contains("expected `fixed_token`"));

        let trailing = format!("{encoded}extra");
        let error = DiagramPlan::decode(&trailing).expect_err("trailing");
        assert!(error.to_string().contains("trailing tokens"));

        let unknown = encoded.replacen("(rep", "(loop", 1);
        let error = DiagramPlan::decode(&unknown).expect_err("unknown constructor");
        assert!(error.to_string().contains("unknown production constructor"));

        let bad_escape = encoded.replacen("\\t", "\\q", 1);
        let error = DiagramPlan::decode(&bad_escape).expect_err("bad escape");
        assert!(error.to_string().contains("unknown string escape"));
    }
}
