use regex::Regex;

/// A cursor over source text for parsing.
///
/// Use `fork()` to create a snapshot before speculative parsing.
/// On success, `commit()` the fork to advance the original.
/// On failure, simply drop the fork -- the original is untouched.
pub struct Input<'input> {
    source: &'input str,
    cursor: usize,
}

impl<'input> Input<'input> {
    /// Create a new input from source text.
    pub fn new(source: &'input str) -> Self {
        Self { source, cursor: 0 }
    }

    /// The full source text.
    pub fn source(&self) -> &'input str {
        self.source
    }

    /// Current byte offset in the source.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The remaining unparsed text from the cursor onwards.
    pub fn remaining(&self) -> &'input str {
        &self.source[self.cursor..]
    }

    /// Advance the cursor by `n` bytes.
    pub fn advance(&mut self, n: usize) {
        self.cursor += n;
    }

    /// Create a fork (snapshot) at the current cursor position.
    pub fn fork(&self) -> Self {
        Self {
            source: self.source,
            cursor: self.cursor,
        }
    }

    /// Commit a fork's position back to this input.
    pub fn commit(&mut self, fork: Self) {
        self.cursor = fork.cursor;
    }

    /// Whether the cursor is at the end of the source.
    pub fn is_empty(&self) -> bool {
        self.cursor >= self.source.len()
    }

    /// Skip ignored content (whitespace, comments) at the current position.
    ///
    /// Pass the pre-compiled ignore regex from `ParseRules::ignore_regex()`.
    /// `None` is a no-op (used by `NoRules`).
    pub fn consume_ignored(&mut self, ignore: Option<&Regex>) {
        if let Some(regex) = ignore
            && let Some(m) = regex.find(self.remaining())
        {
            self.cursor += m.len();
        }
    }
}
