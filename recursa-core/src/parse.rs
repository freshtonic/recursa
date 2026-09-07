use std::fmt;

use crate::lex::LexDiagnosticCode;
use crate::{ExpectedAtomId, FollowSet, GrammarInfo, Input, Span, TokenKind};

/// A low-level parse failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    /// Stable failure classification.
    kind: ParseErrorKind,
    /// Lexical mechanism code retained only for a classified lexical failure.
    lexical_code: Option<LexDiagnosticCode>,
    /// Concrete or zero-width source location.
    span: Span,
    /// Primary source location for the construct that caused the failure.
    anchor: Span,
    /// Grammar-local found kind, absent at EOF.
    found: Option<TokenKind>,
    /// Symbolic expectations retained in analysis order.
    expected: Vec<ExpectedAtomId>,
    /// Static metadata used to render kind and expectation names.
    grammar: &'static GrammarInfo,
}

/// Classification of a low-level parse failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// A concrete token cannot begin any valid continuation.
    UnexpectedToken,
    /// Required syntax remains after the final token.
    UnexpectedEof,
    /// The generated lexer emitted its distinguished error kind.
    LexicalError,
    /// A token record refers to an invalid source range.
    InvalidTokenSpan,
    /// Parser input was produced by different grammar metadata.
    GrammarMismatch,
}

impl ParseErrorKind {
    /// Returns the stable runtime code for a non-lexical failure or the lexical fallback code.
    const fn default_code(self) -> &'static str {
        match self {
            Self::UnexpectedToken => "RCA4100",
            Self::UnexpectedEof => "RCA4101",
            Self::LexicalError => "RCA4000",
            Self::InvalidTokenSpan => "RCA4103",
            Self::GrammarMismatch => "RCA4104",
        }
    }
}

impl ParseError {
    /// Builds an unexpected-input error at the current cursor.
    pub(crate) fn unexpected<A>(input: &Input<'_, A>, expected: &FollowSet) -> Self {
        Self::unexpected_at(input, expected, 0)
    }

    /// Builds an unexpected-input error at an arbitrary lookahead distance.
    pub(crate) fn unexpected_at<A>(
        input: &Input<'_, A>,
        expected: &FollowSet,
        distance: usize,
    ) -> Self {
        let (kind, lexical_code, span, anchor, found) = match input.peek(distance) {
            Some(record) if record.kind == input.grammar().lexical_error_kind => {
                let diagnostic = record.diagnostic;
                let kind = ParseErrorKind::LexicalError;
                (
                    kind,
                    diagnostic.map(|diagnostic| diagnostic.code),
                    record.span,
                    diagnostic.map_or(record.span, |diagnostic| diagnostic.anchor),
                    Some(record.kind),
                )
            }
            Some(record) => {
                let kind = ParseErrorKind::UnexpectedToken;
                (kind, None, record.span, record.span, Some(record.kind))
            }
            None => {
                let position = input.byte_position_at(distance);
                let span = Span::new(position, position)
                    .expect("lexing input must fit Recursa's checked span range");
                let kind = ParseErrorKind::UnexpectedEof;
                (kind, None, span, span, None)
            }
        };

        Self {
            kind,
            lexical_code,
            span,
            anchor,
            found,
            expected: expected.expected().to_vec(),
            grammar: input.grammar(),
        }
    }

    /// Reports a token record whose span cannot safely address the source.
    pub(crate) fn invalid_token_span<A>(
        input: &Input<'_, A>,
        span: Span,
        found: Option<TokenKind>,
    ) -> Self {
        let kind = ParseErrorKind::InvalidTokenSpan;
        Self {
            kind,
            lexical_code: None,
            span,
            anchor: span,
            found,
            expected: Vec::new(),
            grammar: input.grammar(),
        }
    }

    /// Reports parser input bound to different static grammar metadata.
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
        let kind = ParseErrorKind::GrammarMismatch;
        Self {
            kind,
            lexical_code: None,
            span,
            anchor: span,
            found,
            expected: Vec::new(),
            grammar: input.grammar(),
        }
    }

    /// Returns the stable classification of this parse failure.
    pub fn kind(&self) -> ParseErrorKind {
        self.kind
    }

    /// Returns the append-only machine-readable code for this runtime parse failure.
    pub const fn code(&self) -> &'static str {
        match self.lexical_code {
            Some(code) => code.as_str(),
            None => self.kind.default_code(),
        }
    }

    /// Returns the concrete or zero-width failure location.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the primary source location for the construct that caused this failure.
    pub const fn anchor(&self) -> Span {
        self.anchor
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
