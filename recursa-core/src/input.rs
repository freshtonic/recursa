use std::borrow::Cow;

use crate::{GrammarInfo, Span, TokenKind};

/// One lexed token: an erased kind and its byte span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenRecord {
    /// Grammar-local classification assigned by generated lexing.
    pub kind: TokenKind,
    /// Exact matched source extent.
    pub span: Span,
    /// Structured lexical failure attached to this record, when its kind is the grammar error kind.
    pub diagnostic: Option<crate::lex::LexDiagnostic>,
}

/// One classified trivia range retained outside the significant-token stream.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriviaRecord {
    /// Grammar-local trivia classification assigned by generated lexing.
    pub kind: u16,
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
    /// Classified trivia records, present only for an opted-in grammar.
    trivia: Option<Vec<TriviaRecord>>,
}

impl<'src> LexResult<'src> {
    /// Builds a grammar-bound lexing result from validated records.
    pub fn new(
        source: &'src str,
        records: Vec<TokenRecord>,
        trivia: Option<Vec<TriviaRecord>>,
    ) -> Self {
        Self {
            source,
            records,
            trivia,
        }
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
            trivia: self
                .trivia
                .as_ref()
                .map(|records| Cow::Owned(records.clone())),
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

    /// Returns classified trivia retained by an opted-in grammar.
    pub(crate) fn trivia(&self) -> Option<&[TriviaRecord]> {
        self.trivia.as_deref()
    }

    /// Projects immutable beginning, inter-record, and end gaps.
    pub fn source_gaps(&self) -> Option<SourceGaps<'_>> {
        SourceGaps::new(self.source, &self.records, self.trivia.as_deref()?)
    }
}

/// A source-borrowing cursor with owned token records and parse-session state.
pub struct Input<'i, A = ()> {
    /// Complete borrowed source for token text and diagnostics.
    source: &'i str,
    /// Grammar-bound finalized records in source order.
    records: Cow<'i, [TokenRecord]>,
    /// Classified trivia retained by the grammar without entering the parser cursor.
    trivia: Option<Cow<'i, [TriviaRecord]>>,
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

    /// Clones opted-in document provenance for a successfully parsed value.
    pub(crate) fn retained_document(&self) -> Option<RetainedDocument> {
        self.trivia.as_ref().map(|trivia| RetainedDocument {
            records: self.records.to_vec().into_boxed_slice(),
            trivia: trivia.to_vec().into_boxed_slice(),
        })
    }
}

/// Owned document records retained only when classified trivia is enabled.
pub(crate) struct RetainedDocument {
    /// Significant and rejected lexical records delimiting document gaps.
    pub(crate) records: Box<[TokenRecord]>,
    /// Classified trivia records ordered within the gaps.
    pub(crate) trivia: Box<[TriviaRecord]>,
}

/// Iterator over immutable source gaps around non-trivia lexical records.
#[doc(hidden)]
pub struct SourceGaps<'view> {
    /// Complete source containing every projected range.
    source: &'view str,
    /// Significant or rejected lexical records delimiting gaps.
    records: &'view [TokenRecord],
    /// Classified trivia records ordered within those gaps.
    trivia: &'view [TriviaRecord],
    /// Next gap ordinal.
    index: usize,
    /// First trivia record not assigned to an earlier gap.
    trivia_index: usize,
}

impl<'view> SourceGaps<'view> {
    /// Constructs the complete gap iterator after validating its terminal span.
    pub(crate) fn new(
        source: &'view str,
        records: &'view [TokenRecord],
        trivia: &'view [TriviaRecord],
    ) -> Option<Self> {
        Span::new(0, source.len()).ok()?;
        Some(Self {
            source,
            records,
            trivia,
            index: 0,
            trivia_index: 0,
        })
    }
}

impl<'view> Iterator for SourceGaps<'view> {
    type Item = SourceGap<'view>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index > self.records.len() {
            return None;
        }
        let start = self
            .index
            .checked_sub(1)
            .and_then(|index| self.records.get(index))
            .map_or(0, |record| record.span.end() as usize);
        let end = self
            .records
            .get(self.index)
            .map_or(self.source.len(), |record| record.span.start() as usize);
        let trivia_start = self.trivia_index;
        while self
            .trivia
            .get(self.trivia_index)
            .is_some_and(|record| record.span.end() as usize <= end)
        {
            self.trivia_index += 1;
        }
        self.index += 1;
        Some(SourceGap {
            source: self.source,
            span: Span::new(start, end).expect("lexical gaps use checked source offsets"),
            trivia: &self.trivia[trivia_start..self.trivia_index],
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.records.len() + 1 - self.index.min(self.records.len() + 1);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for SourceGaps<'_> {}
impl std::iter::FusedIterator for SourceGaps<'_> {}

/// One exact source gap and its explicitly classified trivia records.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct SourceGap<'view> {
    /// Complete source borrowed by the gap.
    source: &'view str,
    /// Exact range between adjacent non-trivia records.
    span: Span,
    /// Ordered classified trivia records inside the gap.
    trivia: &'view [TriviaRecord],
}

impl<'view> SourceGap<'view> {
    /// Returns the checked exact gap extent.
    pub const fn span(self) -> Span {
        self.span
    }

    /// Returns the exact gap source text.
    pub fn text(self) -> &'view str {
        self.source
            .get(self.span.range())
            .expect("a source gap is validated when constructed")
    }

    /// Iterates classified trivia in source order.
    pub fn trivia(self) -> TriviaRecords<'view> {
        TriviaRecords {
            source: self.source,
            records: self.trivia.iter(),
        }
    }
}

/// Exact-size iterator over classified trivia in one source gap.
#[doc(hidden)]
pub struct TriviaRecords<'view> {
    /// Complete source borrowed by every record.
    source: &'view str,
    /// Remaining ordered trivia records.
    records: std::slice::Iter<'view, TriviaRecord>,
}

impl<'view> Iterator for TriviaRecords<'view> {
    type Item = TriviaRecordView<'view>;

    fn next(&mut self) -> Option<Self::Item> {
        self.records.next().copied().map(|record| TriviaRecordView {
            source: self.source,
            record,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.records.size_hint()
    }
}

impl ExactSizeIterator for TriviaRecords<'_> {}
impl std::iter::FusedIterator for TriviaRecords<'_> {}

/// One exact classified trivia occurrence.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct TriviaRecordView<'view> {
    /// Complete source borrowed by this record.
    source: &'view str,
    /// Compact retained record.
    record: TriviaRecord,
}

impl<'view> TriviaRecordView<'view> {
    /// Returns the private grammar-local classification index.
    pub const fn raw_kind(self) -> u16 {
        self.record.kind
    }

    /// Returns the checked exact trivia extent.
    pub const fn span(self) -> Span {
        self.record.span
    }

    /// Returns exact matched trivia text.
    pub fn text(self) -> &'view str {
        self.source
            .get(self.record.span.range())
            .expect("retained trivia spans are validated against their source")
    }
}
