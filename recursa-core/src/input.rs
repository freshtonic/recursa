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
            capture_spans: false,
        }
    }
}

/// A cursor over a borrowed lexing result plus parse-session state.
pub struct Input<'i, A = ()> {
    source: &'i str,
    records: &'i [TokenRecord],
    cursor: usize,
    grammar: &'static GrammarInfo,
    builder: A,
    capture_spans: bool,
}

impl<'i, A> Input<'i, A> {
    pub fn source(&self) -> &'i str {
        self.source
    }

    pub fn grammar(&self) -> &'static GrammarInfo {
        self.grammar
    }

    pub fn builder(&mut self) -> &mut A {
        &mut self.builder
    }

    /// Returns a record at arbitrary lookahead in constant time.
    pub fn peek(&self, distance: usize) -> Option<TokenRecord> {
        self.cursor
            .checked_add(distance)
            .and_then(|index| self.records.get(index))
            .copied()
    }

    pub fn advance(&mut self) -> Option<TokenRecord> {
        let record = self.peek(0)?;
        self.cursor += 1;
        Some(record)
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn byte_position(&self) -> usize {
        self.peek(0)
            .map_or(self.source.len(), |record| record.span.start() as usize)
    }

    pub fn is_eof(&self) -> bool {
        self.cursor >= self.records.len()
    }

    pub(crate) fn checkpoint(&self) -> usize {
        self.cursor
    }

    pub(crate) fn rewind(&mut self, cursor: usize) {
        assert!(cursor <= self.records.len(), "checkpoint is outside input");
        self.cursor = cursor;
    }

    pub(crate) fn captures_spans(&self) -> bool {
        self.capture_spans
    }

    pub(crate) fn consumed_end(&self) -> usize {
        if self.cursor == 0 {
            return self.byte_position();
        }
        self.records[self.cursor - 1].span.end() as usize
    }

    pub(crate) fn with_span_capture<R>(&mut self, operation: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.capture_spans;
        self.capture_spans = true;
        let result = operation(self);
        self.capture_spans = previous;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static GRAMMAR: GrammarInfo = GrammarInfo {
        name: "capture-test",
        kind_names: &[],
        atom_names: &[],
        lexical_error_kind: 0,
    };

    #[test]
    fn span_capture_is_nested_and_restored_after_success_or_error() {
        let lexed = LexResult::new("", vec![]);
        let mut input = lexed.input(&GRAMMAR);
        assert!(!input.captures_spans());

        let success: Result<(), ()> = input.with_span_capture(|input| {
            assert!(input.captures_spans());
            input.with_span_capture(|nested| assert!(nested.captures_spans()));
            assert!(input.captures_spans());
            Ok(())
        });
        assert_eq!(success, Ok(()));
        assert!(!input.captures_spans());

        let failure: Result<(), ()> = input.with_span_capture(|input| {
            assert!(input.captures_spans());
            Err(())
        });
        assert_eq!(failure, Err(()));
        assert!(!input.captures_spans());
    }
}
