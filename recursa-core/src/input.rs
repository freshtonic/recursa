use crate::{GrammarInfo, Span, TokenKind};

/// One lexed token: an erased kind and its byte span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenRecord {
    pub kind: TokenKind,
    pub span: Span,
}

/// A lexing result that borrows source text and owns compact token records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexResult<'src> {
    source: &'src str,
    records: Vec<TokenRecord>,
}

impl<'src> LexResult<'src> {
    pub fn new(source: &'src str, records: Vec<TokenRecord>) -> Self {
        Self { source, records }
    }

    pub fn input<'i>(&'i self, grammar: &'static GrammarInfo) -> Input<'i>
    where
        'src: 'i,
    {
        self.input_with_builder(grammar, ())
    }

    pub fn input_with_builder<'i, A>(
        &'i self,
        grammar: &'static GrammarInfo,
        builder: A,
    ) -> Input<'i, A>
    where
        'src: 'i,
    {
        Input {
            source: self.source,
            records: &self.records,
            cursor: 0,
            grammar,
            builder,
        }
    }

    pub(crate) fn source(&self) -> &'src str {
        self.source
    }

    pub(crate) fn records(&self) -> &[TokenRecord] {
        &self.records
    }
}

/// A cursor over a borrowed lexing result plus parse-session state.
pub struct Input<'i, A = ()> {
    source: &'i str,
    records: &'i [TokenRecord],
    cursor: usize,
    grammar: &'static GrammarInfo,
    builder: A,
}

impl<'i, A> Input<'i, A> {
    pub fn source(&self) -> &'i str {
        self.source
    }

    pub(crate) fn grammar(&self) -> &'static GrammarInfo {
        self.grammar
    }

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

    pub(crate) fn advance(&mut self) -> Option<TokenRecord> {
        let record = self.peek(0)?;
        self.cursor += 1;
        Some(record)
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn byte_position(&self) -> usize {
        self.peek(0)
            .map_or(self.source.len(), |record| record.span.start() as usize)
    }

    pub fn is_eof(&self) -> bool {
        self.cursor >= self.records.len()
    }
}
