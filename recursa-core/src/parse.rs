use std::fmt;

use crate::{ExpectedAtomId, FollowSet, GrammarInfo, Input, Span, TokenKind};

/// A low-level parse failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    kind: ParseErrorKind,
    span: Span,
    found: Option<TokenKind>,
    expected: Vec<ExpectedAtomId>,
    grammar: &'static GrammarInfo,
}

/// Classification of a low-level parse failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    UnexpectedToken,
    UnexpectedEof,
    PrattAmbiguity,
    LexicalError,
    InvalidTokenSpan,
    GrammarMismatch,
}

impl ParseError {
    pub(crate) fn unexpected<A>(input: &Input<'_, A>, expected: &FollowSet) -> Self {
        Self::unexpected_at(input, expected, 0)
    }

    pub(crate) fn unexpected_at<A>(
        input: &Input<'_, A>,
        expected: &FollowSet,
        distance: usize,
    ) -> Self {
        let (kind, span, found) = match input.peek(distance) {
            Some(record) if record.kind == input.grammar().lexical_error_kind => {
                (ParseErrorKind::LexicalError, record.span, Some(record.kind))
            }
            Some(record) => (
                ParseErrorKind::UnexpectedToken,
                record.span,
                Some(record.kind),
            ),
            None => {
                let position = input.byte_position_at(distance);
                let span = Span::new(position, position)
                    .expect("lexing input must fit Recursa's checked span range");
                (ParseErrorKind::UnexpectedEof, span, None)
            }
        };

        Self {
            kind,
            span,
            found,
            expected: expected.expected().to_vec(),
            grammar: input.grammar(),
        }
    }

    pub(crate) fn invalid_token_span<A>(
        input: &Input<'_, A>,
        span: Span,
        found: Option<TokenKind>,
    ) -> Self {
        Self {
            kind: ParseErrorKind::InvalidTokenSpan,
            span,
            found,
            expected: Vec::new(),
            grammar: input.grammar(),
        }
    }

    pub(crate) fn pratt_ambiguity<A>(input: &Input<'_, A>, expected: &FollowSet) -> Self {
        let record = input
            .peek(0)
            .expect("a Pratt ambiguity is reported only for a matched extender");
        Self {
            kind: ParseErrorKind::PrattAmbiguity,
            span: record.span,
            found: Some(record.kind),
            expected: expected.expected().to_vec(),
            grammar: input.grammar(),
        }
    }

    pub(crate) fn grammar_mismatch<A>(input: &Input<'_, A>) -> Self {
        let (span, found) = input.peek(0).map_or_else(
            || {
                let position = input.byte_position();
                (
                    Span::new(position, position)
                        .expect("lexing input must fit Recursa's checked span range"),
                    None,
                )
            },
            |record| (record.span, Some(record.kind)),
        );
        Self {
            kind: ParseErrorKind::GrammarMismatch,
            span,
            found,
            expected: Vec::new(),
            grammar: input.grammar(),
        }
    }

    pub fn kind(&self) -> ParseErrorKind {
        self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }

    /// The grammar-local display name of the found token, when present.
    pub fn found(&self) -> Option<&'static str> {
        self.found.and_then(|kind| self.grammar.kind_name(kind))
    }

    /// Grammar-local display names for the symbolic expectations.
    pub fn expected(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.expected
            .iter()
            .filter_map(|atom| self.grammar.atom_name(*atom))
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ParseErrorKind::UnexpectedEof => formatter.write_str("unexpected end of input"),
            ParseErrorKind::PrattAmbiguity => {
                formatter.write_str("Pratt extender overlaps caller FOLLOW")
            }
            ParseErrorKind::LexicalError => formatter.write_str("lexical error"),
            ParseErrorKind::InvalidTokenSpan => formatter.write_str("invalid token span"),
            ParseErrorKind::GrammarMismatch => {
                formatter.write_str("input belongs to a different grammar")
            }
            ParseErrorKind::UnexpectedToken => {
                let found = self
                    .found
                    .and_then(|kind| self.grammar.kind_name(kind))
                    .unwrap_or("unknown token");
                write!(formatter, "unexpected {found}")
            }
        }
    }
}

impl std::error::Error for ParseError {}
