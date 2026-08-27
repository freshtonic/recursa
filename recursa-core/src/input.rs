use std::borrow::Cow;

use crate::{GrammarInfo, Span, TokenKind};

/// One lexed token: an erased kind and its byte span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenRecord {
    /// Grammar-local classification assigned by generated lexing.
    pub kind: TokenKind,
    /// Exact matched source extent.
    pub span: Span,
}

/// A lexing result that borrows source text and owns compact token records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexResult<'src> {
    /// Complete source whose byte offsets are used by every record.
    source: &'src str,
    /// Finalized records in source order.
    records: Vec<TokenRecord>,
}

impl<'src> LexResult<'src> {
    /// Builds a grammar-bound lexing result from validated records.
    pub fn new(source: &'src str, records: Vec<TokenRecord>) -> Self {
        Self { source, records }
    }

    /// Creates parser input with unit session state.
    pub fn input(&self, grammar: &'static GrammarInfo) -> Input<'src> {
        self.input_with_builder(grammar, ())
    }

    /// Creates parser input carrying caller-owned construction state.
    pub fn input_with_builder<A>(
        &self,
        grammar: &'static GrammarInfo,
        builder: A,
    ) -> Input<'src, A> {
        Input {
            source: self.source,
            records: Cow::Owned(self.records.clone()),
            cursor: 0,
            grammar,
            builder,
        }
    }

    /// Returns the exact source retained by this result.
    pub(crate) fn source(&self) -> &'src str {
        self.source
    }

    /// Returns the finalized payload-free token records.
    pub(crate) fn records(&self) -> &[TokenRecord] {
        &self.records
    }
}

/// A source-borrowing cursor with owned token records and parse-session state.
pub struct Input<'i, A = ()> {
    /// Complete borrowed source for token text and diagnostics.
    source: &'i str,
    /// Grammar-bound finalized records in source order.
    records: Cow<'i, [TokenRecord]>,
    /// Absolute index of the next record.
    cursor: usize,
    /// Static metadata that assigns meaning to record kinds.
    grammar: &'static GrammarInfo,
    /// Caller-owned state used while constructing semantic values.
    builder: A,
}

impl<'i, A> Input<'i, A> {
    /// Returns the complete source borrowed by this parse session.
    pub fn source(&self) -> &'i str {
        self.source
    }

    /// Returns static metadata for the grammar that produced these records.
    pub(crate) fn grammar(&self) -> &'static GrammarInfo {
        self.grammar
    }

    /// Returns mutable caller-owned construction state.
    pub fn builder(&mut self) -> &mut A {
        &mut self.builder
    }

    /// Returns a record at arbitrary lookahead in constant time.
    pub(crate) fn peek(&self, distance: usize) -> Option<TokenRecord> {
        self.cursor
            .checked_add(distance)
            .and_then(|index| self.records.get(index))
            .copied()
    }

    /// Consumes and returns the next token record, if present.
    pub(crate) fn advance(&mut self) -> Option<TokenRecord> {
        let record = self.peek(0)?;
        self.cursor += 1;
        Some(record)
    }

    /// Returns a record by absolute token index without moving the cursor.
    pub(crate) fn record(&self, index: usize) -> Option<TokenRecord> {
        self.records.get(index).copied()
    }

    /// Returns the current token-record cursor.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Restores a previously captured cursor after transactional parsing.
    pub(crate) fn restore(&mut self, cursor: usize) {
        debug_assert!(cursor <= self.records.len());
        self.cursor = cursor;
    }

    /// Returns the current token start, or the source end at EOF.
    pub(crate) fn byte_position(&self) -> usize {
        self.byte_position_at(0)
    }

    /// Returns a lookahead token start, or the source end beyond available input.
    pub(crate) fn byte_position_at(&self, distance: usize) -> usize {
        self.peek(distance)
            .map_or(self.source.len(), |record| record.span.start() as usize)
    }

    /// Reports whether every finalized token record has been consumed.
    pub fn is_eof(&self) -> bool {
        self.cursor >= self.records.len()
    }
}
