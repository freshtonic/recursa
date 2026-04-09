use std::sync::Mutex;

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
    /// Pass the `IGNORE` pattern string from a `ParseRules` implementation.
    /// An empty string is a no-op.
    pub fn consume_ignored(&mut self, ignore: &str) {
        if ignore.is_empty() {
            return;
        }

        // Cache compiled ignore regexes keyed by pattern string.
        // The number of distinct patterns is bounded by the number of
        // ParseRules implementations in the program.
        static PATTERNS: Mutex<Vec<(String, Regex)>> = Mutex::new(Vec::new());

        let ignore_regex = {
            let patterns = PATTERNS.lock().unwrap();
            patterns
                .iter()
                .find(|(p, _)| p == ignore)
                .map(|(_, r)| r.clone())
        };

        let regex = match ignore_regex {
            Some(r) => r,
            None => {
                let anchored = format!(r"\A(?:{})", ignore);
                let r = Regex::new(&anchored).expect("invalid IGNORE pattern");
                PATTERNS
                    .lock()
                    .unwrap()
                    .push((ignore.to_string(), r.clone()));
                r
            }
        };

        if let Some(m) = regex.find(self.remaining()) {
            self.cursor += m.len();
        }
    }
}
