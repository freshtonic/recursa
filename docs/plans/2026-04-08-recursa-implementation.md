# Recursa Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a derive-macro framework for recursive descent parsers where AST types derive `Parse` and `Scan` traits automatically.

**Architecture:** Three-crate workspace — `recursa-core` (traits, `Input`, errors), `recursa-derive` (proc macros), `recursa` (re-exports). Scannerless parsing: lexing is driven by parse context. Zero-copy tokens via `&'input str`. `miette`-based error diagnostics.

**Tech Stack:** Rust 2024 edition, `regex` crate, `miette` + `thiserror` for errors, `syn`/`quote`/`proc-macro2` for derive macros.

**Design doc:** `docs/plans/2026-04-08-recursa-design.md`

---

## Phase 1: Workspace Setup

### Task 1: Convert to Cargo Workspace

Convert the existing single-crate project into a workspace with three member crates.

**Files:**
- Modify: `Cargo.toml` (root — becomes workspace manifest)
- Create: `recursa-core/Cargo.toml`
- Create: `recursa-core/src/lib.rs`
- Create: `recursa-derive/Cargo.toml`
- Create: `recursa-derive/src/lib.rs`
- Modify: `src/lib.rs` (becomes the `recursa` facade crate)

**Step 1: Replace root `Cargo.toml` with workspace manifest**

```toml
[workspace]
members = ["recursa-core", "recursa-derive", "."]
resolver = "3"

[package]
name = "recursa"
version = "0.1.0"
edition = "2024"

[dependencies]
recursa-core = { path = "recursa-core" }
recursa-derive = { path = "recursa-derive" }
```

**Step 2: Create `recursa-core/Cargo.toml`**

```toml
[package]
name = "recursa-core"
version = "0.1.0"
edition = "2024"

[dependencies]
regex = "1"
miette = { version = "7", features = ["fancy"] }
thiserror = "2"
```

**Step 3: Create `recursa-core/src/lib.rs`**

```rust
//! Core traits and types for the recursa parser framework.
```

**Step 4: Create `recursa-derive/Cargo.toml`**

```toml
[package]
name = "recursa-derive"
version = "0.1.0"
edition = "2024"

[lib]
proc-macro = true

[dependencies]
syn = { version = "2", features = ["full"] }
quote = "1"
proc-macro2 = "1"

[dev-dependencies]
recursa-core = { path = "../recursa-core" }
```

**Step 5: Create `recursa-derive/src/lib.rs`**

```rust
//! Derive macros for the recursa parser framework.

use proc_macro::TokenStream;

#[proc_macro_derive(Scan, attributes(scan))]
pub fn derive_scan(input: TokenStream) -> TokenStream {
    TokenStream::new()
}

#[proc_macro_derive(Parse, attributes(parse))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    TokenStream::new()
}
```

**Step 6: Replace `src/lib.rs` with re-exports**

```rust
//! Recursa — derive recursive descent parsers from Rust types.

pub use recursa_core::*;
pub use recursa_derive::*;
```

**Step 7: Build the workspace**

Run: `cargo build`
Expected: Compiles with no errors.

**Step 8: Commit**

```bash
git add -A
git commit -m "Set up cargo workspace with three crates"
```

---

## Phase 2: Core Traits and Types (recursa-core)

### Task 2: ParseRules Trait and NoRules

**Files:**
- Create: `recursa-core/src/rules.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the test**

Add to `recursa-core/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rules_ignore_is_empty() {
        assert_eq!(<NoRules as ParseRules>::IGNORE, "");
    }
}
```

**Step 2: Run the test to verify it fails**

Run: `cargo test -p recursa-core`
Expected: FAIL — `NoRules` and `ParseRules` not defined.

**Step 3: Implement ParseRules and NoRules**

Create `recursa-core/src/rules.rs`:

```rust
/// Configuration for a grammar's ignored content (whitespace, comments, etc.).
///
/// `IGNORE` is a regex pattern matched and skipped between tokens during parsing.
/// It must be a const so derive macros can splice it into lookahead regexes at compile time.
pub trait ParseRules {
    const IGNORE: &'static str;
}

/// No-op rules for `Scan` types that don't skip whitespace.
pub struct NoRules;

impl ParseRules for NoRules {
    const IGNORE: &'static str = "";
}
```

Update `recursa-core/src/lib.rs`:

```rust
mod rules;

pub use rules::{NoRules, ParseRules};
```

**Step 4: Run the test to verify it passes**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/rules.rs recursa-core/src/lib.rs
git commit -m "Add ParseRules trait and NoRules implementation"
```

---

### Task 3: ParseError Type with miette

**Files:**
- Create: `recursa-core/src/error.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the test**

Add to `recursa-core/src/lib.rs` tests:

```rust
use miette::Diagnostic;

#[test]
fn parse_error_is_diagnostic() {
    let err = ParseError::new(
        "let 123 = foo;",
        4..7,
        "identifier",
    );
    // Verify it implements Diagnostic (miette)
    let _: &dyn Diagnostic = &err;
    assert_eq!(err.expected(), "identifier");
}

#[test]
fn parse_error_with_context() {
    let inner = ParseError::new("let 123 = foo;", 4..7, "identifier");
    let outer = inner.with_context("let binding", 0..3);
    // The related errors should contain the context
    let related: Vec<_> = outer.related().into_iter().flatten().collect();
    assert_eq!(related.len(), 1);
}

#[test]
fn parse_error_with_help() {
    let err = ParseError::new("let 123 = foo;", 4..7, "identifier")
        .with_help("variable names must start with a letter or underscore");
    assert!(err.help().is_some());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `ParseError` not defined.

**Step 3: Implement ParseError**

Create `recursa-core/src/error.rs`:

```rust
use miette::{Diagnostic, LabeledSpan, MietteHandler, ReportHandler, SourceCode};
use std::fmt;
use std::ops::Range;

/// A parse error with source location, expected/found info, and optional context chain.
#[derive(Debug, Clone)]
pub struct ParseError {
    src: String,
    span: Range<usize>,
    expected: String,
    found: Option<String>,
    help: Option<String>,
    context: Vec<ContextError>,
}

/// A "while parsing X" breadcrumb attached to a ParseError.
#[derive(Debug, Clone)]
struct ContextError {
    label: String,
    span: Range<usize>,
}

impl ParseError {
    /// Create a new parse error.
    ///
    /// - `src`: the full source text
    /// - `span`: byte range of the problematic input
    /// - `expected`: description of what was expected
    pub fn new(src: impl Into<String>, span: Range<usize>, expected: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            span,
            expected: expected.into(),
            found: None,
            help: None,
            context: Vec::new(),
        }
    }

    /// What was expected at this position.
    pub fn expected(&self) -> &str {
        &self.expected
    }

    /// Set what was actually found.
    pub fn with_found(mut self, found: impl Into<String>) -> Self {
        self.found = Some(found.into());
        self
    }

    /// Add a help message.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Wrap this error with "while parsing <label>" context.
    pub fn with_context(mut self, label: impl Into<String>, span: Range<usize>) -> Self {
        self.context.push(ContextError {
            label: label.into(),
            span,
        });
        self
    }

    /// Merge multiple expected values into one error (for enum dispatch failures).
    pub fn merge(errors: Vec<ParseError>) -> Self {
        assert!(!errors.is_empty(), "cannot merge empty error list");
        let first = &errors[0];
        let src = first.src.clone();
        let span = first.span.clone();

        let expected_items: Vec<&str> = errors.iter().map(|e| e.expected.as_str()).collect();
        let expected = format!("one of: {}", expected_items.join(", "));

        Self {
            src,
            span,
            expected,
            found: first.found.clone(),
            help: None,
            context: Vec::new(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.found {
            Some(found) => write!(f, "expected {} but found {}", self.expected, found),
            None => write!(f, "expected {}", self.expected),
        }
    }
}

impl std::error::Error for ParseError {}

impl Diagnostic for ParseError {
    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.src)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let label = match &self.found {
            Some(found) => format!("found {}", found),
            None => format!("expected {}", self.expected),
        };
        Some(Box::new(std::iter::once(
            LabeledSpan::new(Some(label), self.span.start, self.span.len()),
        )))
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        if self.context.is_empty() {
            return None;
        }
        Some(Box::new(
            self.context.iter().map(|c| c as &dyn Diagnostic),
        ))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.help
            .as_ref()
            .map(|h| Box::new(h.as_str()) as Box<dyn fmt::Display>)
    }
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "while parsing {}", self.label)
    }
}

impl std::error::Error for ContextError {}

impl Diagnostic for ContextError {
    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(std::iter::once(LabeledSpan::new(
            Some(format!("while parsing {}", self.label)),
            self.span.start,
            self.span.len(),
        ))))
    }
}
```

Update `recursa-core/src/lib.rs` to add:

```rust
mod error;

pub use error::ParseError;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/error.rs recursa-core/src/lib.rs
git commit -m "Add ParseError with miette diagnostics"
```

---

### Task 4: Input Type with Fork/Commit

**Files:**
- Create: `recursa-core/src/input.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the tests**

Add to `recursa-core/src/lib.rs` tests:

```rust
#[test]
fn input_starts_at_zero() {
    let input = Input::<NoRules>::new("hello world");
    assert_eq!(input.cursor(), 0);
    assert_eq!(input.remaining(), "hello world");
}

#[test]
fn input_advance() {
    let mut input = Input::<NoRules>::new("hello world");
    input.advance(5);
    assert_eq!(input.cursor(), 5);
    assert_eq!(input.remaining(), " world");
}

#[test]
fn input_fork_does_not_affect_original() {
    let input = Input::<NoRules>::new("hello world");
    let mut fork = input.fork();
    fork.advance(5);
    assert_eq!(input.cursor(), 0);
    assert_eq!(fork.cursor(), 5);
}

#[test]
fn input_fork_commit() {
    let mut input = Input::<NoRules>::new("hello world");
    let mut fork = input.fork();
    fork.advance(5);
    input.commit(fork);
    assert_eq!(input.cursor(), 5);
}

#[test]
fn input_source() {
    let input = Input::<NoRules>::new("hello world");
    assert_eq!(input.source(), "hello world");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `Input` not defined.

**Step 3: Implement Input**

Create `recursa-core/src/input.rs`:

```rust
use std::marker::PhantomData;

use crate::ParseRules;

/// A cursor over source text, parameterised by grammar rules.
///
/// Use `fork()` to create a snapshot before speculative parsing.
/// On success, `commit()` the fork to advance the original.
/// On failure, simply drop the fork — the original is untouched.
pub struct Input<'input, R: ParseRules> {
    source: &'input str,
    cursor: usize,
    _rules: PhantomData<R>,
}

impl<'input, R: ParseRules> Input<'input, R> {
    /// Create a new input from source text.
    pub fn new(source: &'input str) -> Self {
        Self {
            source,
            cursor: 0,
            _rules: PhantomData,
        }
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
            _rules: PhantomData,
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
}
```

Update `recursa-core/src/lib.rs` to add:

```rust
mod input;

pub use input::Input;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/input.rs recursa-core/src/lib.rs
git commit -m "Add Input type with fork/commit model"
```

---

### Task 5: Scan Trait

**Files:**
- Create: `recursa-core/src/scan.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the tests**

Add to `recursa-core/src/lib.rs` tests:

```rust
use regex::Regex;
use std::sync::OnceLock;

struct TestKeyword;

impl Scan<'_> for TestKeyword {
    const PATTERN: &'static str = r"test";

    fn regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| Regex::new(r"\Atest").unwrap())
    }

    fn from_match(_matched: &str) -> Result<Self, ParseError> {
        Ok(TestKeyword)
    }
}

struct TestIdent<'input>(&'input str);

impl<'input> Scan<'input> for TestIdent<'input> {
    const PATTERN: &'static str = r"[a-zA-Z_][a-zA-Z0-9_]*";

    fn regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| Regex::new(r"\A[a-zA-Z_][a-zA-Z0-9_]*").unwrap())
    }

    fn from_match(matched: &'input str) -> Result<Self, ParseError> {
        Ok(TestIdent(matched))
    }
}

#[test]
fn scan_keyword_peek() {
    let input = Input::<NoRules>::new("test foo");
    assert!(TestKeyword::peek(&input));
}

#[test]
fn scan_keyword_peek_fails() {
    let input = Input::<NoRules>::new("foo bar");
    assert!(!TestKeyword::peek(&input));
}

#[test]
fn scan_keyword_parse() {
    let mut input = Input::<NoRules>::new("test foo");
    let _kw = TestKeyword::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 4);
}

#[test]
fn scan_ident_parse_captures() {
    let mut input = Input::<NoRules>::new("hello world");
    let ident = TestIdent::parse(&mut input).unwrap();
    assert_eq!(ident.0, "hello");
    assert_eq!(input.cursor(), 5);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `Scan` trait not defined, no `peek`/`parse` methods.

**Step 3: Implement Scan trait**

Create `recursa-core/src/scan.rs`:

```rust
use regex::Regex;

use crate::error::ParseError;
use crate::input::Input;
use crate::rules::{NoRules, ParseRules};

/// Leaf-level token matching via regex.
///
/// Each token type implements `Scan` with a regex pattern and a constructor.
/// The blanket implementation of `Parse` for `Scan` types handles `peek` and `parse`
/// by matching the regex against the remaining input.
pub trait Scan<'input>: Sized {
    /// The regex pattern that matches this token (without `\A` anchor — added automatically).
    const PATTERN: &'static str;

    /// Returns the compiled, cached regex for this token.
    /// Implementations should use a `static OnceLock<Regex>` for caching.
    fn regex() -> &'static Regex;

    /// Construct this token from the matched text.
    fn from_match(matched: &'input str) -> Result<Self, ParseError>;

    /// Check whether this token can be parsed at the current position without advancing.
    fn peek(input: &Input<'input, NoRules>) -> bool {
        Self::regex().is_match(input.remaining())
    }

    /// Attempt to parse this token, advancing the input on success.
    fn parse(input: &mut Input<'input, NoRules>) -> Result<Self, ParseError> {
        match Self::regex().find(input.remaining()) {
            Some(m) => {
                let matched = &input.source()[input.cursor()..input.cursor() + m.len()];
                let result = Self::from_match(matched)?;
                input.advance(m.len());
                Ok(result)
            }
            None => Err(ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                Self::PATTERN,
            )),
        }
    }
}
```

Update `recursa-core/src/lib.rs` to add:

```rust
mod scan;

pub use scan::Scan;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/scan.rs recursa-core/src/lib.rs
git commit -m "Add Scan trait with peek and parse via regex"
```

---

### Task 6: Parse Trait and Blanket Implementation

**Files:**
- Create: `recursa-core/src/parse.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the tests**

These tests verify that `Scan` types can be used through the `Parse` trait via the blanket impl. Add to `recursa-core/src/lib.rs` tests:

```rust
#[test]
fn scan_type_implements_parse() {
    // TestKeyword implements Scan, so it should also implement Parse
    let mut input = Input::<NoRules>::new("test foo");
    let _kw = <TestKeyword as Parse>::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 4);
}

#[test]
fn scan_type_peek_through_parse() {
    let input = Input::<NoRules>::new("test foo");
    assert!(<TestKeyword as Parse>::peek(&input));
}

#[test]
fn scan_type_peek_through_parse_fails() {
    let input = Input::<NoRules>::new("foo bar");
    assert!(!<TestKeyword as Parse>::peek(&input));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `Parse` trait not defined.

**Step 3: Implement Parse trait with blanket impl**

Create `recursa-core/src/parse.rs`:

```rust
use crate::error::ParseError;
use crate::input::Input;
use crate::rules::{NoRules, ParseRules};
use crate::scan::Scan;

/// Recursive descent parser trait.
///
/// Structs derive `Parse` as a sequence (parse fields in order).
/// Enums derive `Parse` as a choice (peek to select variant).
/// `Scan` types get a blanket implementation automatically.
pub trait Parse<'input>: Sized {
    type Rules: ParseRules;

    /// Check whether this production can start at the current input position.
    fn peek(input: &Input<'input, Self::Rules>) -> bool;

    /// Parse this production, advancing the input on success.
    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError>;
}

/// Blanket implementation: every `Scan` type is also a `Parse` type with `NoRules`.
impl<'input, T: Scan<'input>> Parse<'input> for T {
    type Rules = NoRules;

    fn peek(input: &Input<'input, NoRules>) -> bool {
        <T as Scan>::peek(input)
    }

    fn parse(input: &mut Input<'input, NoRules>) -> Result<Self, ParseError> {
        <T as Scan>::parse(input)
    }
}
```

Update `recursa-core/src/lib.rs` to add:

```rust
mod parse;

pub use parse::Parse;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/parse.rs recursa-core/src/lib.rs
git commit -m "Add Parse trait with blanket implementation for Scan types"
```

---

### Task 7: Input Ignore-Token Consumption

The `Input` type needs a method to consume ignored tokens (whitespace/comments) using the `ParseRules::IGNORE` pattern. This is called by derived `Parse` impls between struct fields.

**Files:**
- Modify: `recursa-core/src/input.rs`
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Write the tests**

```rust
struct WhitespaceRules;

impl ParseRules for WhitespaceRules {
    const IGNORE: &'static str = r"\s+";
}

#[test]
fn input_consume_ignored_skips_whitespace() {
    let mut input = Input::<WhitespaceRules>::new("   hello");
    input.consume_ignored();
    assert_eq!(input.remaining(), "hello");
}

#[test]
fn input_consume_ignored_noop_when_no_whitespace() {
    let mut input = Input::<WhitespaceRules>::new("hello");
    input.consume_ignored();
    assert_eq!(input.remaining(), "hello");
}

#[test]
fn input_consume_ignored_noop_for_no_rules() {
    let mut input = Input::<NoRules>::new("   hello");
    input.consume_ignored();
    assert_eq!(input.remaining(), "   hello");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `consume_ignored` method not defined.

**Step 3: Implement consume_ignored**

Add to `recursa-core/src/input.rs`:

```rust
use regex::Regex;
use std::sync::OnceLock;

// Add a helper to get the cached ignore regex. Since the IGNORE pattern is a const,
// we need a per-type cache. We use a function-local OnceLock parameterised by the
// pattern string, but since we can't have generic statics, we match on the pattern
// at runtime (the OnceLock ensures this only happens once).
```

Add this method to `impl<'input, R: ParseRules> Input<'input, R>`:

```rust
    /// Skip any ignored content (whitespace, comments) at the current position.
    /// Uses the `IGNORE` pattern from the associated `ParseRules`.
    pub fn consume_ignored(&mut self) {
        if R::IGNORE.is_empty() {
            return;
        }
        // Build the anchored pattern on first use
        static PATTERNS: std::sync::Mutex<Vec<(&'static str, Regex)>> =
            std::sync::Mutex::new(Vec::new());

        // This is a simple approach; a more efficient approach using TypeId-keyed
        // storage can be implemented later if needed.
        let ignore_regex = {
            let patterns = PATTERNS.lock().unwrap();
            patterns
                .iter()
                .find(|(p, _)| *p == R::IGNORE)
                .map(|(_, r)| r.clone())
        };

        let regex = match ignore_regex {
            Some(r) => r,
            None => {
                let anchored = format!(r"\A(?:{})", R::IGNORE);
                let r = Regex::new(&anchored).expect("invalid IGNORE pattern");
                PATTERNS.lock().unwrap().push((R::IGNORE, r.clone()));
                r
            }
        };

        if let Some(m) = regex.find(self.remaining()) {
            self.cursor += m.len();
        }
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/input.rs recursa-core/src/lib.rs
git commit -m "Add consume_ignored method to Input using ParseRules::IGNORE"
```

---

## Phase 3: Derive Macros (recursa-derive)

### Task 8: derive(Scan) for Unit Structs

Derive `Scan` for simple unit structs like `struct LetKeyword;`.

**Files:**
- Create: `recursa-derive/src/scan_derive.rs`
- Modify: `recursa-derive/src/lib.rs`

**Step 1: Write the test**

Create `recursa-derive/tests/scan_unit_struct.rs`:

```rust
use recursa_core::{Input, NoRules, Scan};
use recursa_derive::Scan;

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKeyword;

#[test]
fn scan_unit_struct_peek() {
    let input = Input::<NoRules>::new("let x = 1");
    assert!(LetKeyword::peek(&input));
}

#[test]
fn scan_unit_struct_peek_fails() {
    let input = Input::<NoRules>::new("var x = 1");
    assert!(!LetKeyword::peek(&input));
}

#[test]
fn scan_unit_struct_parse() {
    let mut input = Input::<NoRules>::new("let x = 1");
    let _kw = LetKeyword::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_unit_struct_parse_fails() {
    let mut input = Input::<NoRules>::new("var x = 1");
    let err = LetKeyword::parse(&mut input);
    assert!(err.is_err());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — derive macro produces empty output.

**Step 3: Implement derive(Scan) for unit structs**

Create `recursa-derive/src/scan_derive.rs`:

```rust
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr};

pub fn derive_scan(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let pattern = get_scan_pattern(&input)?;

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unit => derive_scan_unit_struct(name, &pattern, impl_generics, ty_generics, where_clause),
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                derive_scan_tuple_struct(name, &pattern, generics)
            }
            _ => Err(syn::Error::new_spanned(
                name,
                "Scan can only be derived for unit structs or single-field tuple structs",
            )),
        },
        _ => Err(syn::Error::new_spanned(
            name,
            "Scan can only be derived for structs (enum Scan support is separate)",
        )),
    }
}

fn get_scan_pattern(input: &DeriveInput) -> syn::Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("scan") {
            let mut pattern = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("pattern") {
                    let value: LitStr = meta.value()?.parse()?;
                    pattern = Some(value.value());
                    Ok(())
                } else {
                    Err(meta.error("expected `pattern`"))
                }
            })?;
            return pattern.ok_or_else(|| syn::Error::new_spanned(attr, "missing `pattern` in #[scan(...)]"));
        }
    }
    Err(syn::Error::new_spanned(&input.ident, "missing #[scan(pattern = \"...\")] attribute"))
}

fn derive_scan_unit_struct(
    name: &syn::Ident,
    pattern: &str,
    impl_generics: syn::ImplGenerics,
    ty_generics: syn::TypeGenerics,
    where_clause: Option<&syn::WhereClause>,
) -> syn::Result<TokenStream> {
    let anchored_pattern = format!(r"\A(?:{})", pattern);

    Ok(quote! {
        impl #impl_generics ::recursa_core::Scan<'_> for #name #ty_generics #where_clause {
            const PATTERN: &'static str = #pattern;

            fn regex() -> &'static ::regex::Regex {
                static REGEX: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
                REGEX.get_or_init(|| ::regex::Regex::new(#anchored_pattern).unwrap())
            }

            fn from_match(_matched: &str) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                Ok(#name)
            }
        }
    })
}

fn derive_scan_tuple_struct(
    _name: &syn::Ident,
    _pattern: &str,
    _generics: &syn::Generics,
) -> syn::Result<TokenStream> {
    // Placeholder — implemented in Task 9
    todo!()
}
```

Update `recursa-derive/src/lib.rs`:

```rust
mod scan_derive;

use proc_macro::TokenStream;

#[proc_macro_derive(Scan, attributes(scan))]
pub fn derive_scan(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match scan_derive::derive_scan(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(Parse, attributes(parse))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    TokenStream::new()
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-derive/src/scan_derive.rs recursa-derive/src/lib.rs recursa-derive/tests/
git commit -m "Implement derive(Scan) for unit structs"
```

---

### Task 9: derive(Scan) for Tuple Structs (Capture)

Derive `Scan` for tuple structs like `struct Ident<'input>(&'input str);`.

**Files:**
- Modify: `recursa-derive/src/scan_derive.rs`

**Step 1: Write the test**

Create `recursa-derive/tests/scan_tuple_struct.rs`:

```rust
use recursa_core::{Input, NoRules, Scan};
use recursa_derive::Scan;

#[derive(Scan)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan)]
#[scan(pattern = r"[0-9]+")]
struct IntLiteral<'input>(&'input str);

#[test]
fn scan_tuple_struct_parse_captures() {
    let mut input = Input::<NoRules>::new("hello world");
    let ident = Ident::parse(&mut input).unwrap();
    assert_eq!(ident.0, "hello");
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_tuple_struct_int_literal() {
    let mut input = Input::<NoRules>::new("42 + 1");
    let lit = IntLiteral::parse(&mut input).unwrap();
    assert_eq!(lit.0, "42");
    assert_eq!(input.cursor(), 2);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — `derive_scan_tuple_struct` hits `todo!()`.

**Step 3: Implement derive_scan_tuple_struct**

Replace the `derive_scan_tuple_struct` function in `recursa-derive/src/scan_derive.rs`:

```rust
fn derive_scan_tuple_struct(
    name: &syn::Ident,
    pattern: &str,
    generics: &syn::Generics,
) -> syn::Result<TokenStream> {
    let anchored_pattern = format!(r"\A(?:{})", pattern);

    // Extract the lifetime parameter (should be 'input or similar)
    let lifetime = generics
        .lifetimes()
        .next()
        .ok_or_else(|| syn::Error::new_spanned(name, "tuple Scan structs must have a lifetime parameter"))?;
    let lt = &lifetime.lifetime;

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::recursa_core::Scan<#lt> for #name #ty_generics #where_clause {
            const PATTERN: &'static str = #pattern;

            fn regex() -> &'static ::regex::Regex {
                static REGEX: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
                REGEX.get_or_init(|| ::regex::Regex::new(#anchored_pattern).unwrap())
            }

            fn from_match(matched: &#lt str) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                Ok(#name(matched))
            }
        }
    })
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-derive/src/scan_derive.rs recursa-derive/tests/
git commit -m "Implement derive(Scan) for tuple structs with capture"
```

---

### Task 10: derive(Parse) for Structs (Sequence)

Derive `Parse` for structs where each field is parsed in order.

**Files:**
- Create: `recursa-derive/src/parse_derive.rs`
- Modify: `recursa-derive/src/lib.rs`

**Step 1: Write the test**

Create `recursa-derive/tests/parse_struct.rs`:

```rust
use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan)]
#[scan(pattern = "=")]
struct Eq;

#[derive(Scan)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

#[derive(Parse)]
#[parse(rules = WsRules)]
struct LetBinding<'input> {
    let_kw: LetKw,
    name: Ident<'input>,
    eq: Eq,
    value: IntLit<'input>,
    semi: Semi,
}

#[test]
fn parse_struct_sequence() {
    let mut input = Input::<WsRules>::new("let x = 42;");
    let binding = LetBinding::parse(&mut input).unwrap();
    assert_eq!(binding.name.0, "x");
    assert_eq!(binding.value.0, "42");
    assert_eq!(input.cursor(), 11);
}

#[test]
fn parse_struct_peek() {
    let input = Input::<WsRules>::new("let x = 42;");
    assert!(LetBinding::peek(&input));
}

#[test]
fn parse_struct_peek_fails() {
    let input = Input::<WsRules>::new("var x = 42;");
    assert!(!LetBinding::peek(&input));
}

#[test]
fn parse_struct_error_on_bad_field() {
    let mut input = Input::<WsRules>::new("let 123 = 42;");
    let err = LetBinding::parse(&mut input);
    assert!(err.is_err());
    // Cursor should NOT have advanced (fork was not committed)
    assert_eq!(input.cursor(), 0);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — derive macro produces empty output.

**Step 3: Implement derive(Parse) for structs**

The key challenge: struct fields implement `Parse<'input>` with `type Rules = NoRules`, but the struct's `Parse` impl uses a different `Rules` type. The derived code needs to:
1. Call `input.consume_ignored()` (which uses the struct's `Rules`)
2. Convert the `Input<Rules>` to `Input<NoRules>` for scanning each field
3. Commit the field's fork back

This means `Input` needs a method to temporarily rebind its rules type. Add to `recursa-core/src/input.rs`:

```rust
    /// Create a view of this input with different rules.
    /// Used internally when a struct's Parse impl (with Rules) needs to
    /// call a field's Parse impl (with NoRules for Scan types).
    pub fn rebind<R2: ParseRules>(&self) -> Input<'input, R2> {
        Input {
            source: self.source,
            cursor: self.cursor,
            _rules: PhantomData,
        }
    }
```

Create `recursa-derive/src/parse_derive.rs`:

```rust
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type};

pub fn derive_parse(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    let rules_type = get_rules_type(&input)?;

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => derive_parse_struct(name, &input.generics, &rules_type, fields),
            _ => Err(syn::Error::new_spanned(
                name,
                "Parse can only be derived for structs with named fields",
            )),
        },
        Data::Enum(data) => {
            derive_parse_enum(name, &input.generics, &rules_type, data, &input.attrs)
        }
        _ => Err(syn::Error::new_spanned(name, "Parse can only be derived for structs and enums")),
    }
}

fn get_rules_type(input: &DeriveInput) -> syn::Result<Type> {
    for attr in &input.attrs {
        if attr.path().is_ident("parse") {
            let mut rules = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rules") {
                    let value = meta.value()?;
                    let ty: Type = value.parse()?;
                    rules = Some(ty);
                    Ok(())
                } else if meta.path.is_ident("pratt") {
                    // Handled separately
                    Ok(())
                } else {
                    Err(meta.error("expected `rules` or `pratt`"))
                }
            })?;
            if let Some(rules) = rules {
                return Ok(rules);
            }
        }
    }
    Err(syn::Error::new_spanned(
        &input.ident,
        "missing #[parse(rules = ...)] attribute",
    ))
}

fn derive_parse_struct(
    name: &syn::Ident,
    generics: &syn::Generics,
    rules_type: &Type,
    fields: &syn::FieldsNamed,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Determine the lifetime to use for Parse<'input>
    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let field_names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();
    let first_field_type = field_types.first().ok_or_else(|| {
        syn::Error::new_spanned(name, "Parse struct must have at least one field")
    })?;

    // Generate the parse body: consume_ignored + rebind + parse each field
    let parse_fields = field_names.iter().zip(field_types.iter()).map(|(name, ty)| {
        quote! {
            fork.consume_ignored();
            let #name = {
                let mut rebound = fork.rebind::<<<#ty as ::recursa_core::Parse>::Rules>();
                let result = <#ty as ::recursa_core::Parse>::parse(&mut rebound)?;
                fork.commit(rebound.rebind());
                result
            };
        }
    });

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            type Rules = #rules_type;

            fn peek(input: &::recursa_core::Input<#lt, Self::Rules>) -> bool {
                let rebound = input.rebind::<<<#first_field_type as ::recursa_core::Parse>::Rules>();
                <#first_field_type as ::recursa_core::Parse>::peek(&rebound)
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, Self::Rules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let mut fork = input.fork();
                #(#parse_fields)*
                input.commit(fork);
                Ok(Self { #(#field_names),* })
            }
        }
    })
}

fn derive_parse_enum(
    _name: &syn::Ident,
    _generics: &syn::Generics,
    _rules_type: &Type,
    _data: &syn::DataEnum,
    _attrs: &[syn::Attribute],
) -> syn::Result<TokenStream> {
    // Placeholder — implemented in Task 11
    todo!()
}
```

Update `recursa-derive/src/lib.rs`:

```rust
mod parse_derive;
mod scan_derive;

use proc_macro::TokenStream;

#[proc_macro_derive(Scan, attributes(scan))]
pub fn derive_scan(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match scan_derive::derive_scan(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(Parse, attributes(parse))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match parse_derive::derive_parse(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

Note: The `rebind` approach for converting between `Input<R1>` and `Input<R2>` may need iteration during implementation. The core idea is sound but the exact generated code may need adjustment once the compiler sees it. The implementer should feel free to adjust the `rebind` call pattern to satisfy the type checker.

**Step 5: Commit**

```bash
git add recursa-derive/src/ recursa-derive/tests/ recursa-core/src/input.rs
git commit -m "Implement derive(Parse) for structs with sequence parsing"
```

---

### Task 11: derive(Parse) for Enums (Choice)

Derive `Parse` for enums where each variant is a single-field newtype.

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs`

**Step 1: Write the test**

Create `recursa-derive/tests/parse_enum.rs`:

```rust
use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

#[derive(Scan)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan)]
#[scan(pattern = "return")]
struct ReturnKw;

#[derive(Scan)]
#[scan(pattern = "=")]
struct Eq;

#[derive(Scan)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

#[derive(Parse)]
#[parse(rules = WsRules)]
struct LetBinding<'input> {
    let_kw: LetKw,
    name: Ident<'input>,
    eq: Eq,
    value: IntLit<'input>,
    semi: Semi,
}

#[derive(Parse)]
#[parse(rules = WsRules)]
struct ReturnStmt<'input> {
    return_kw: ReturnKw,
    value: IntLit<'input>,
    semi: Semi,
}

#[derive(Parse)]
#[parse(rules = WsRules)]
enum Statement<'input> {
    Let(LetBinding<'input>),
    Return(ReturnStmt<'input>),
}

#[test]
fn parse_enum_let_variant() {
    let mut input = Input::<WsRules>::new("let x = 42;");
    let stmt = Statement::parse(&mut input).unwrap();
    assert!(matches!(stmt, Statement::Let(_)));
}

#[test]
fn parse_enum_return_variant() {
    let mut input = Input::<WsRules>::new("return 42;");
    let stmt = Statement::parse(&mut input).unwrap();
    assert!(matches!(stmt, Statement::Return(_)));
}

#[test]
fn parse_enum_peek() {
    let input = Input::<WsRules>::new("let x = 42;");
    assert!(Statement::peek(&input));

    let input2 = Input::<WsRules>::new("return 42;");
    assert!(Statement::peek(&input2));
}

#[test]
fn parse_enum_peek_fails() {
    let input = Input::<WsRules>::new("if true {}");
    assert!(!Statement::peek(&input));
}

#[test]
fn parse_enum_error_reports_all_variants() {
    let mut input = Input::<WsRules>::new("if true {}");
    let err = Statement::parse(&mut input).unwrap_err();
    let msg = format!("{}", err);
    // Error should mention both expected alternatives
    assert!(msg.contains("let") || msg.contains("return") || msg.contains("one of"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — `derive_parse_enum` hits `todo!()`.

**Step 3: Implement derive_parse_enum**

Replace `derive_parse_enum` in `recursa-derive/src/parse_derive.rs`:

```rust
fn derive_parse_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    rules_type: &Type,
    data: &syn::DataEnum,
    attrs: &[syn::Attribute],
) -> syn::Result<TokenStream> {
    // Check for #[parse(pratt)] attribute
    let is_pratt = attrs.iter().any(|attr| {
        if !attr.path().is_ident("parse") {
            return false;
        }
        let mut found_pratt = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("pratt") {
                found_pratt = true;
            }
            Ok(())
        });
        found_pratt
    });

    if is_pratt {
        return derive_parse_pratt_enum(name, generics, rules_type, data);
    }

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    // Each variant must be a single-field newtype: Variant(InnerType)
    let mut peek_arms = Vec::new();
    let mut parse_arms = Vec::new();
    let mut error_arms = Vec::new();

    for variant in &data.variants {
        let variant_name = &variant.ident;
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => {
                return Err(syn::Error::new_spanned(
                    variant_name,
                    "Parse enum variants must be single-field newtypes, e.g. Variant(InnerType)",
                ))
            }
        };

        peek_arms.push(quote! {
            if <#inner_type as ::recursa_core::Parse>::peek(&rebound) {
                return true;
            }
        });

        parse_arms.push(quote! {
            {
                let rebound = fork.rebind::<<#inner_type as ::recursa_core::Parse>::Rules>();
                if <#inner_type as ::recursa_core::Parse>::peek(&rebound) {
                    let mut rebound = fork.rebind::<<#inner_type as ::recursa_core::Parse>::Rules>();
                    match <#inner_type as ::recursa_core::Parse>::parse(&mut rebound) {
                        Ok(inner) => {
                            input.commit(rebound.rebind());
                            return Ok(#name::#variant_name(inner));
                        }
                        Err(e) => errors.push(e),
                    }
                }
            }
        });

        error_arms.push(quote! {
            errors.push(::recursa_core::ParseError::new(
                fork.source().to_string(),
                fork.cursor()..fork.cursor(),
                stringify!(#variant_name),
            ));
        });
    }

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            type Rules = #rules_type;

            fn peek(input: &::recursa_core::Input<#lt, Self::Rules>) -> bool {
                let rebound = input.rebind::<<Self as ::recursa_core::Parse>::Rules>();
                #(#peek_arms)*
                false
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, Self::Rules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let fork = input.fork();
                let mut errors = Vec::new();
                #(#parse_arms)*
                // None matched — merge all errors
                if errors.is_empty() {
                    #(#error_arms)*
                }
                Err(::recursa_core::ParseError::merge(errors))
            }
        }
    })
}

fn derive_parse_pratt_enum(
    _name: &syn::Ident,
    _generics: &syn::Generics,
    _rules_type: &Type,
    _data: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    // Placeholder — implemented in Task 13
    todo!()
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

Note: The `rebind` call pattern in the generated code may need adjustment. The key insight is that `peek` needs to check each variant's inner type using that type's `Rules`, and `parse` does the same but commits on success. The implementer should adjust the rebind chain as needed.

**Step 5: Commit**

```bash
git add recursa-derive/src/parse_derive.rs recursa-derive/tests/
git commit -m "Implement derive(Parse) for enums with peek-based dispatch"
```

---

### Task 12: derive(Scan) for Enums (Combined Regex)

Derive `Scan` for enums where all variants implement `Scan`, generating a single combined regex.

**Files:**
- Modify: `recursa-derive/src/scan_derive.rs`

**Step 1: Write the test**

Create `recursa-derive/tests/scan_enum.rs`:

```rust
use recursa_core::{Input, NoRules, Scan};
use recursa_derive::Scan;

#[derive(Scan, Debug)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan, Debug)]
#[scan(pattern = "if")]
struct IfKw;

#[derive(Scan, Debug)]
#[scan(pattern = "while")]
struct WhileKw;

#[derive(Scan, Debug)]
enum Keyword {
    Let(LetKw),
    If(IfKw),
    While(WhileKw),
}

#[test]
fn scan_enum_let() {
    let mut input = Input::<NoRules>::new("let x");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::Let(_)));
    assert_eq!(input.cursor(), 3);
}

#[test]
fn scan_enum_if() {
    let mut input = Input::<NoRules>::new("if true");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::If(_)));
    assert_eq!(input.cursor(), 2);
}

#[test]
fn scan_enum_while() {
    let mut input = Input::<NoRules>::new("while true");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::While(_)));
    assert_eq!(input.cursor(), 5);
}

#[test]
fn scan_enum_longest_match() {
    // If we had "iffy" as a keyword and "if" as another,
    // longest match should win. Here we test with identifiers.

    #[derive(Scan, Debug)]
    #[scan(pattern = r"[a-zA-Z]+")]
    struct Word<'input>(&'input str);

    #[derive(Scan, Debug)]
    #[scan(pattern = r"[0-9]+")]
    struct Num<'input>(&'input str);

    #[derive(Scan, Debug)]
    enum Token<'input> {
        Word(Word<'input>),
        Num(Num<'input>),
    }

    let mut input = Input::<NoRules>::new("hello123");
    let tok = Token::parse(&mut input).unwrap();
    assert!(matches!(tok, Token::Word(w) if w.0 == "hello"));
}

#[test]
fn scan_enum_peek() {
    let input = Input::<NoRules>::new("let x");
    assert!(Keyword::peek(&input));
}

#[test]
fn scan_enum_peek_fails() {
    let input = Input::<NoRules>::new("123");
    assert!(!Keyword::peek(&input));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — enum Scan not implemented.

**Step 3: Implement derive(Scan) for enums**

Add to `recursa-derive/src/scan_derive.rs`, update the `derive_scan` function to handle enums:

```rust
pub fn derive_scan(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    match &input.data {
        Data::Struct(data) => {
            let pattern = get_scan_pattern(&input)?;
            let generics = &input.generics;
            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
            match &data.fields {
                Fields::Unit => derive_scan_unit_struct(name, &pattern, impl_generics, ty_generics, where_clause),
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    derive_scan_tuple_struct(name, &pattern, generics)
                }
                _ => Err(syn::Error::new_spanned(
                    name,
                    "Scan can only be derived for unit structs or single-field tuple structs",
                )),
            }
        }
        Data::Enum(data) => derive_scan_enum(name, &input.generics, data),
        _ => Err(syn::Error::new_spanned(name, "Scan cannot be derived for unions")),
    }
}
```

Add the `derive_scan_enum` function:

```rust
fn derive_scan_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Each variant must be a single-field newtype where the inner type implements Scan
    let mut variant_names = Vec::new();
    let mut variant_types = Vec::new();

    for variant in &data.variants {
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => {
                return Err(syn::Error::new_spanned(
                    &variant.ident,
                    "Scan enum variants must be single-field newtypes",
                ))
            }
        };
        variant_names.push(&variant.ident);
        variant_types.push(inner_type);
    }

    // Build combined pattern: join all variant patterns with |
    // Each is wrapped in a named capture group for identification
    // We use indexed groups: (?P<_0>pattern0)|(?P<_1>pattern1)|...
    let variant_indices: Vec<_> = (0..variant_names.len())
        .map(|i| syn::Ident::new(&format!("_{}", i), proc_macro2::Span::call_site()))
        .collect();

    let variant_count = variant_names.len();

    // Generate match arms for from_match dispatch
    let match_arms = variant_names
        .iter()
        .zip(variant_types.iter())
        .enumerate()
        .map(|(i, (vname, vtype))| {
            let idx = &variant_indices[i];
            quote! {
                if let Some(m) = captures.name(stringify!(#idx)) {
                    let matched_str = &input.source()[input.cursor()..input.cursor() + m.len()];
                    return Ok(#name::#vname(<#vtype as ::recursa_core::Scan>::from_match(matched_str)?));
                }
            }
        });

    let match_arms_for_len = variant_names
        .iter()
        .zip(variant_types.iter())
        .enumerate()
        .map(|(i, (vname, vtype))| {
            let idx = &variant_indices[i];
            quote! {
                if let Some(m) = captures.name(stringify!(#idx)) {
                    if m.len() > best_len {
                        best_len = m.len();
                        best_index = Some(#i);
                    }
                }
            }
        });

    let dispatch_arms = variant_names
        .iter()
        .zip(variant_types.iter())
        .enumerate()
        .map(|(i, (vname, vtype))| {
            let idx = &variant_indices[i];
            quote! {
                #i => {
                    let m = captures.name(stringify!(#idx)).unwrap();
                    let matched_str = &input.source()[input.cursor()..input.cursor() + m.len()];
                    let result = <#vtype as ::recursa_core::Scan>::from_match(matched_str)?;
                    input.advance(m.len());
                    Ok(#name::#vname(result))
                }
            }
        });

    // Build the combined pattern string at compile time using concat
    // We need to build it at runtime since we reference associated consts
    let pattern_parts = variant_types.iter().zip(variant_indices.iter()).map(|(vtype, idx)| {
        quote! {
            parts.push(format!("(?P<{}>{})", stringify!(#idx), <#vtype as ::recursa_core::Scan>::PATTERN));
        }
    });

    Ok(quote! {
        impl #impl_generics ::recursa_core::Scan<#lt> for #name #ty_generics #where_clause {
            const PATTERN: &'static str = ""; // Combined pattern is built at runtime

            fn regex() -> &'static ::regex::Regex {
                static REGEX: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
                REGEX.get_or_init(|| {
                    let mut parts = Vec::new();
                    #(#pattern_parts)*
                    let combined = format!(r"\A(?:{})", parts.join("|"));
                    ::regex::Regex::new(&combined).unwrap()
                })
            }

            fn from_match(_matched: &#lt str) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                // Not used directly — parse handles dispatch via named captures
                unimplemented!("use parse() for enum Scan types")
            }

            fn peek(input: &::recursa_core::Input<#lt, ::recursa_core::NoRules>) -> bool {
                Self::regex().is_match(input.remaining())
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, ::recursa_core::NoRules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let captures = match Self::regex().captures(input.remaining()) {
                    Some(c) => c,
                    None => {
                        return Err(::recursa_core::ParseError::new(
                            input.source().to_string(),
                            input.cursor()..input.cursor(),
                            stringify!(#name),
                        ));
                    }
                };

                // Find longest match (maximal munch), declaration order as tiebreaker
                let mut best_len = 0usize;
                let mut best_index: Option<usize> = None;
                #(#match_arms_for_len)*

                match best_index {
                    #(#dispatch_arms)*
                    _ => Err(::recursa_core::ParseError::new(
                        input.source().to_string(),
                        input.cursor()..input.cursor(),
                        stringify!(#name),
                    )),
                }
            }
        }
    })
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

Note: The longest-match logic with named capture groups may need refinement. The `regex` crate's `captures()` returns the first match of the overall alternation, so all alternatives are tested. Named groups let us identify which alternative matched. If multiple groups could match, we pick the longest. The implementer should verify this matches the maximal munch semantics we want.

**Step 5: Commit**

```bash
git add recursa-derive/src/scan_derive.rs recursa-derive/tests/
git commit -m "Implement derive(Scan) for enums with combined regex"
```

---

## Phase 4: Pratt Parsing

### Task 13: derive(Parse) for Pratt Enums

Derive `Parse` with Pratt parsing for expression enums.

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs`

**Step 1: Write the test**

Create `recursa-derive/tests/parse_pratt.rs`:

```rust
use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

#[derive(Scan, Debug)]
#[scan(pattern = r"\+")]
struct Plus;

#[derive(Scan, Debug)]
#[scan(pattern = r"\*")]
struct Star;

#[derive(Scan, Debug)]
#[scan(pattern = r"-")]
struct Minus;

#[derive(Scan, Debug)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

#[derive(Scan, Debug)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules, pratt)]
enum Expr<'input> {
    #[parse(prefix, bp = 9)]
    Neg(Minus, Box<Expr<'input>>),

    #[parse(infix, bp = 5)]
    Add(Box<Expr<'input>>, Plus, Box<Expr<'input>>),

    #[parse(infix, bp = 6)]
    Mul(Box<Expr<'input>>, Star, Box<Expr<'input>>),

    #[parse(atom)]
    Lit(IntLit<'input>),

    #[parse(atom)]
    Name(Ident<'input>),
}

#[test]
fn pratt_atom() {
    let mut input = Input::<WsRules>::new("42");
    let expr = Expr::parse(&mut input).unwrap();
    assert!(matches!(expr, Expr::Lit(_)));
}

#[test]
fn pratt_simple_add() {
    let mut input = Input::<WsRules>::new("1 + 2");
    let expr = Expr::parse(&mut input).unwrap();
    assert!(matches!(expr, Expr::Add(_, _, _)));
}

#[test]
fn pratt_precedence_mul_over_add() {
    // 1 + 2 * 3 should parse as 1 + (2 * 3)
    let mut input = Input::<WsRules>::new("1 + 2 * 3");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Lit(_)));
            assert!(matches!(*right, Expr::Mul(_, _, _)));
        }
        _ => panic!("expected Add at top level"),
    }
}

#[test]
fn pratt_left_associativity() {
    // 1 + 2 + 3 should parse as (1 + 2) + 3
    let mut input = Input::<WsRules>::new("1 + 2 + 3");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Add(_, _, _)));
            assert!(matches!(*right, Expr::Lit(_)));
        }
        _ => panic!("expected Add at top level"),
    }
}

#[test]
fn pratt_prefix_neg() {
    let mut input = Input::<WsRules>::new("-42");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Neg(_, inner) => assert!(matches!(*inner, Expr::Lit(_))),
        _ => panic!("expected Neg"),
    }
}

#[test]
fn pratt_prefix_in_expression() {
    // -1 + 2 should parse as (-1) + 2 because prefix bp=9 > infix bp=5
    let mut input = Input::<WsRules>::new("-1 + 2");
    let expr = Expr::parse(&mut input).unwrap();
    match expr {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Neg(_, _)));
            assert!(matches!(*right, Expr::Lit(_)));
        }
        _ => panic!("expected Add at top level"),
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — `derive_parse_pratt_enum` hits `todo!()`.

**Step 3: Implement derive_parse_pratt_enum**

This generates a standard Pratt parser. The algorithm:
1. Parse a prefix expression (atom or prefix operator + recursive call at prefix bp)
2. Loop: check for infix operator, if its bp > minimum bp, consume it and parse right side

Replace `derive_parse_pratt_enum` in `recursa-derive/src/parse_derive.rs`:

```rust
fn derive_parse_pratt_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    rules_type: &Type,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let mut atom_variants = Vec::new();
    let mut prefix_variants = Vec::new();
    let mut infix_variants = Vec::new();

    for variant in &data.variants {
        let vname = &variant.ident;
        let kind = parse_pratt_attrs(&variant.attrs)?;
        let fields: Vec<_> = match &variant.fields {
            Fields::Unnamed(f) => f.unnamed.iter().collect(),
            _ => return Err(syn::Error::new_spanned(vname, "Pratt variants must use tuple fields")),
        };

        match kind {
            PrattKind::Atom => {
                if fields.len() != 1 {
                    return Err(syn::Error::new_spanned(vname, "atom variants must have exactly one field"));
                }
                atom_variants.push((vname.clone(), fields[0].ty.clone()));
            }
            PrattKind::Prefix { bp } => {
                if fields.len() != 2 {
                    return Err(syn::Error::new_spanned(vname, "prefix variants must have exactly two fields (operator, operand)"));
                }
                prefix_variants.push((vname.clone(), fields[0].ty.clone(), bp));
            }
            PrattKind::Infix { bp, right_assoc } => {
                if fields.len() != 3 {
                    return Err(syn::Error::new_spanned(vname, "infix variants must have exactly three fields (left, operator, right)"));
                }
                infix_variants.push((vname.clone(), fields[1].ty.clone(), bp, right_assoc));
            }
        }
    }

    // Generate atom peek/parse arms
    let atom_peek_arms = atom_variants.iter().map(|(vname, ty)| {
        quote! {
            {
                let rebound = input.rebind::<<#ty as ::recursa_core::Parse>::Rules>();
                if <#ty as ::recursa_core::Parse>::peek(&rebound) {
                    return true;
                }
            }
        }
    });

    let atom_parse_arms = atom_variants.iter().map(|(vname, ty)| {
        quote! {
            {
                let rebound = input.rebind::<<#ty as ::recursa_core::Parse>::Rules>();
                if <#ty as ::recursa_core::Parse>::peek(&rebound) {
                    let mut rebound = input.rebind::<<#ty as ::recursa_core::Parse>::Rules>();
                    let inner = <#ty as ::recursa_core::Parse>::parse(&mut rebound)?;
                    input.commit(rebound.rebind());
                    return Ok(#name::#vname(inner));
                }
            }
        }
    });

    // Generate prefix parse arms
    let prefix_parse_arms = prefix_variants.iter().map(|(vname, op_ty, bp)| {
        quote! {
            {
                let rebound = input.rebind::<<#op_ty as ::recursa_core::Parse>::Rules>();
                if <#op_ty as ::recursa_core::Parse>::peek(&rebound) {
                    let mut rebound = input.rebind::<<#op_ty as ::recursa_core::Parse>::Rules>();
                    let op = <#op_ty as ::recursa_core::Parse>::parse(&mut rebound)?;
                    input.commit(rebound.rebind());
                    let rhs = parse_expr(input, #bp)?;
                    return Ok(#name::#vname(op, Box::new(rhs)));
                }
            }
        }
    });

    let prefix_peek_arms = prefix_variants.iter().map(|(_, op_ty, _)| {
        quote! {
            {
                let rebound = input.rebind::<<#op_ty as ::recursa_core::Parse>::Rules>();
                if <#op_ty as ::recursa_core::Parse>::peek(&rebound) {
                    return true;
                }
            }
        }
    });

    // Generate infix check/parse arms
    let infix_arms = infix_variants.iter().map(|(vname, op_ty, bp, right_assoc)| {
        // For left-assoc, right side parses at bp+1; for right-assoc, at bp
        let right_bp = if *right_assoc { *bp } else { bp + 1 };
        quote! {
            {
                input.consume_ignored();
                let rebound = input.rebind::<<#op_ty as ::recursa_core::Parse>::Rules>();
                if <#op_ty as ::recursa_core::Parse>::peek(&rebound) && #bp > min_bp {
                    let mut rebound = input.rebind::<<#op_ty as ::recursa_core::Parse>::Rules>();
                    let op = <#op_ty as ::recursa_core::Parse>::parse(&mut rebound)?;
                    input.commit(rebound.rebind());
                    let rhs = parse_expr(input, #right_bp)?;
                    lhs = #name::#vname(Box::new(lhs), op, Box::new(rhs));
                    continue;
                }
            }
        }
    });

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            type Rules = #rules_type;

            fn peek(input: &::recursa_core::Input<#lt, Self::Rules>) -> bool {
                #(#atom_peek_arms)*
                #(#prefix_peek_arms)*
                false
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, Self::Rules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                parse_expr(input, 0)
            }
        }

        fn parse_expr<#lt>(
            input: &mut ::recursa_core::Input<#lt, #rules_type>,
            min_bp: u32,
        ) -> ::std::result::Result<#name #ty_generics, ::recursa_core::ParseError> {
            input.consume_ignored();

            // Parse prefix or atom
            let mut lhs;

            // Try prefix operators first
            #(#prefix_parse_arms)*

            // Try atoms
            #(#atom_parse_arms)*

            return Err(::recursa_core::ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                stringify!(#name),
            ));

            // Infix loop
            parsed:
            loop {
                #(#infix_arms)*
                break;
            }

            Ok(lhs)
        }
    })
}

enum PrattKind {
    Atom,
    Prefix { bp: u32 },
    Infix { bp: u32, right_assoc: bool },
}

fn parse_pratt_attrs(attrs: &[syn::Attribute]) -> syn::Result<PrattKind> {
    for attr in attrs {
        if attr.path().is_ident("parse") {
            let mut kind = None;
            let mut bp = None;
            let mut right_assoc = false;

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("atom") {
                    kind = Some("atom");
                } else if meta.path.is_ident("prefix") {
                    kind = Some("prefix");
                } else if meta.path.is_ident("infix") {
                    kind = Some("infix");
                } else if meta.path.is_ident("bp") {
                    let value = meta.value()?;
                    let lit: syn::LitInt = value.parse()?;
                    bp = Some(lit.base10_parse::<u32>()?);
                } else if meta.path.is_ident("assoc") {
                    let value = meta.value()?;
                    let lit: syn::LitStr = value.parse()?;
                    if lit.value() == "right" {
                        right_assoc = true;
                    }
                }
                Ok(())
            })?;

            return match kind {
                Some("atom") => Ok(PrattKind::Atom),
                Some("prefix") => Ok(PrattKind::Prefix {
                    bp: bp.ok_or_else(|| syn::Error::new_spanned(attr, "prefix requires bp"))?,
                }),
                Some("infix") => Ok(PrattKind::Infix {
                    bp: bp.ok_or_else(|| syn::Error::new_spanned(attr, "infix requires bp"))?,
                    right_assoc,
                }),
                _ => Err(syn::Error::new_spanned(attr, "expected atom, prefix, or infix")),
            };
        }
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        "pratt enum variant missing #[parse(atom|prefix|infix, ...)] attribute",
    ))
}
```

Note: The generated `parse_expr` function uses a free function scoped alongside the impl. This may need to be placed inside a module or use a unique name to avoid collisions if multiple pratt enums exist. The implementer should use a name-mangled identifier like `parse_expr_{TypeName}` or place it in an anonymous const block.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-derive/src/parse_derive.rs recursa-derive/tests/
git commit -m "Implement derive(Parse) with Pratt parsing for expression enums"
```

---

## Phase 5: Bulk Declaration Macros

### Task 14: keywords!, punctuation!, and literals! Macros

These are declarative macros in `recursa-core` (re-exported via `recursa`).

**Files:**
- Create: `recursa-core/src/macros.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the test**

Create `recursa-derive/tests/bulk_macros.rs`:

```rust
use recursa_core::{Input, NoRules, Scan};

// These macros are defined in recursa-core
recursa_core::keywords! {
    Let    => "let",
    If     => "if",
    While  => "while",
}

recursa_core::punctuation! {
    Plus   => "+",
    Minus  => "-",
    LParen => "(",
}

recursa_core::literals! {
    IntLit  => r"[0-9]+",
    IdentLit => r"[a-zA-Z_][a-zA-Z0-9_]*",
}

#[test]
fn keyword_macro_creates_types() {
    let mut input = Input::<NoRules>::new("let x");
    let _kw = Let::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 3);
}

#[test]
fn keyword_macro_creates_enum() {
    let mut input = Input::<NoRules>::new("if x");
    let kw = Keyword::parse(&mut input).unwrap();
    assert!(matches!(kw, Keyword::If(_)));
}

#[test]
fn punctuation_macro_escapes_pattern() {
    let mut input = Input::<NoRules>::new("+ 1");
    let _p = Plus::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 1);
}

#[test]
fn punctuation_macro_creates_enum() {
    let mut input = Input::<NoRules>::new("(");
    let p = Punctuation::parse(&mut input).unwrap();
    assert!(matches!(p, Punctuation::LParen(_)));
}

#[test]
fn literals_macro_captures() {
    let mut input = Input::<NoRules>::new("42 hello");
    let lit = IntLit::parse(&mut input).unwrap();
    assert_eq!(lit.0, "42");
}

#[test]
fn literals_macro_creates_enum() {
    let mut input = Input::<NoRules>::new("hello");
    let lit = Literal::parse(&mut input).unwrap();
    assert!(matches!(lit, Literal::IdentLit(_)));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — macros not defined.

**Step 3: Implement the macros**

Create `recursa-core/src/macros.rs`:

```rust
/// Declare keyword token types and a combined `Keyword` enum.
///
/// ```ignore
/// keywords! {
///     Let   => "let",
///     While => "while",
///     If    => "if",
/// }
/// ```
///
/// Expands to a unit struct with `#[derive(Scan)]` for each entry,
/// plus an enum `Keyword` with all variants.
#[macro_export]
macro_rules! keywords {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, Debug, Clone)]
            #[scan(pattern = $pattern)]
            pub struct $name;
        )*

        #[derive(::recursa_derive::Scan, Debug, Clone)]
        pub enum Keyword {
            $($name($name)),*
        }
    };
}

/// Declare punctuation token types and a combined `Punctuation` enum.
///
/// Patterns are automatically regex-escaped.
///
/// ```ignore
/// punctuation! {
///     Plus   => "+",
///     LParen => "(",
/// }
/// ```
#[macro_export]
macro_rules! punctuation {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, Debug, Clone)]
            #[scan(pattern = $pattern)]
            pub struct $name;
        )*

        #[derive(::recursa_derive::Scan, Debug, Clone)]
        pub enum Punctuation {
            $($name($name)),*
        }
    };
}

/// Declare literal/capturing token types and a combined `Literal` enum.
///
/// Each type is a tuple struct capturing `&'input str`.
///
/// ```ignore
/// literals! {
///     IntLiteral => r"[0-9]+",
///     Ident      => r"[a-zA-Z_][a-zA-Z0-9_]*",
/// }
/// ```
#[macro_export]
macro_rules! literals {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, Debug, Clone)]
            #[scan(pattern = $pattern)]
            pub struct $name<'input>(pub &'input str);
        )*

        #[derive(::recursa_derive::Scan, Debug, Clone)]
        pub enum Literal<'input> {
            $($name($name<'input>)),*
        }
    };
}
```

Update `recursa-core/src/lib.rs` to add:

```rust
mod macros;
```

Note: The `punctuation!` macro needs the `#[scan(pattern = ...)]` patterns to be regex-escaped. There are two approaches:
1. The derive macro for `Scan` handles escaping via an attribute like `#[scan(pattern = "+", literal)]`
2. The user provides already-escaped patterns like `r"\+"`

For simplicity, start with approach 2 — users write `r"\+"` in the punctuation macro. If this proves painful, add a `literal` flag to the `Scan` derive later. Update the test accordingly.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/macros.rs recursa-core/src/lib.rs recursa-derive/tests/
git commit -m "Add keywords!, punctuation!, and literals! bulk declaration macros"
```

---

## Phase 6: Re-export Crate

### Task 15: Wire Up the recursa Facade Crate

**Files:**
- Modify: `src/lib.rs`

**Step 1: Write the test**

Create `tests/integration.rs` (in the root `recursa` crate):

```rust
use recursa::{Input, Parse, ParseRules, Scan};
use recursa::{Parse as ParseDerive, Scan as ScanDerive};

// Verify that everything is accessible through the recursa crate
#[derive(Scan)]
#[scan(pattern = "hello")]
struct Hello;

struct MyRules;
impl ParseRules for MyRules {
    const IGNORE: &'static str = r"\s+";
}

#[test]
fn recursa_reexports_work() {
    let mut input = Input::<recursa::NoRules>::new("hello");
    let _h = Hello::parse(&mut input).unwrap();
    assert_eq!(input.cursor(), 5);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p recursa`
Expected: FAIL — re-exports not wired up.

**Step 3: Update `src/lib.rs`**

```rust
//! Recursa — derive recursive descent parsers from Rust types.
//!
//! This crate re-exports everything from `recursa-core` (traits, types)
//! and `recursa-derive` (proc macros).

pub use recursa_core::*;
pub use recursa_derive::*;
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p recursa`
Expected: PASS

**Step 5: Commit**

```bash
git add src/lib.rs tests/
git commit -m "Wire up recursa facade crate with re-exports"
```

---

## Phase 7: End-to-End Integration Test

### Task 16: Integration Test — Mini Language Parser

Write an end-to-end test that parses a small language to verify all components work together.

**Files:**
- Create: `tests/mini_language.rs`

**Step 1: Write and run the test**

```rust
//! End-to-end test: parse a tiny language with let bindings and expressions.

use recursa::{Input, Parse, ParseRules, Scan};

// -- Token types --

#[derive(Scan, Debug)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan, Debug)]
#[scan(pattern = "=")]
struct Eq;

#[derive(Scan, Debug)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan, Debug)]
#[scan(pattern = r"\+")]
struct Plus;

#[derive(Scan, Debug)]
#[scan(pattern = r"\*")]
struct Star;

#[derive(Scan, Debug)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

// -- Grammar rules --

struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
}

// -- AST --

#[derive(Parse, Debug)]
#[parse(rules = Lang, pratt)]
enum Expr<'input> {
    #[parse(infix, bp = 5)]
    Add(Box<Expr<'input>>, Plus, Box<Expr<'input>>),

    #[parse(infix, bp = 6)]
    Mul(Box<Expr<'input>>, Star, Box<Expr<'input>>),

    #[parse(atom)]
    Lit(IntLit<'input>),

    #[parse(atom)]
    Var(Ident<'input>),
}

#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct LetStmt<'input> {
    let_kw: LetKw,
    name: Ident<'input>,
    eq: Eq,
    value: Expr<'input>,
    semi: Semi,
}

// -- Tests --

#[test]
fn parse_let_with_expression() {
    let mut input = Input::<Lang>::new("let x = 1 + 2 * 3;");
    let stmt = LetStmt::parse(&mut input).unwrap();
    assert_eq!(stmt.name.0, "x");
    // value should be Add(Lit(1), Mul(Lit(2), Lit(3)))
    match stmt.value {
        Expr::Add(left, _, right) => {
            assert!(matches!(*left, Expr::Lit(l) if l.0 == "1"));
            assert!(matches!(*right, Expr::Mul(_, _, _)));
        }
        _ => panic!("expected Add"),
    }
    assert!(input.is_empty());
}

#[test]
fn parse_error_has_span() {
    use miette::Diagnostic;
    let mut input = Input::<Lang>::new("let 123 = 1;");
    let err = LetStmt::parse(&mut input).unwrap_err();
    // Error should have labels (spans)
    assert!(err.labels().is_some());
}
```

Run: `cargo test -p recursa`
Expected: PASS — all components working together.

**Step 2: Commit**

```bash
git add tests/mini_language.rs
git commit -m "Add end-to-end integration test with mini language parser"
```

---

## Summary

| Phase | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Task 1 | Workspace structure |
| 2 | Tasks 2-7 | Core traits: `ParseRules`, `ParseError`, `Input`, `Scan`, `Parse`, `consume_ignored` |
| 3 | Tasks 8-12 | Derive macros: `Scan` (unit/tuple/enum), `Parse` (struct/enum) |
| 4 | Task 13 | Pratt parsing for expressions |
| 5 | Task 14 | Bulk declaration macros |
| 6 | Task 15 | Facade crate re-exports |
| 7 | Task 16 | End-to-end integration test |

**Deferred to future work:**
- `FirstSet` type representation and compile-time ambiguity detection
- `Option<T>` and `Vec<T>` blanket Parse impls
- Error recovery strategies
- `#[scan(literal)]` for auto-escaping in punctuation
