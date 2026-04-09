# Container Types Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add blanket `Parse` impls for `Option<T>`, `Box<T>`, and a type-level configurable `Seq<T, S, Trailing, Empty>` for separated lists.

**Architecture:** `Option` and `Box` are blanket impls in `recursa-core/src/parse.rs`. `Seq` is a new concrete type in `recursa-core` with marker types for trailing/emptiness configuration. Parse impls for `Seq` use a sealed `SeqParse` trait to dispatch parse logic by marker type.

**Tech Stack:** Rust, `vec1` crate (for `NonEmpty` deref)

**Design doc:** `docs/plans/2026-04-09-container-types-design.md`

---

## Task 1: Box\<T\> blanket Parse impl

The simplest container — pure delegation. Needed for recursive types (already used in Pratt parsing, but currently the Pratt derive generates its own `Box::new` calls rather than going through `Parse`).

**Files:**
- Modify: `recursa-core/src/parse.rs`
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Write the failing test**

Add to the test module in `recursa-core/src/lib.rs`:

```rust
#[test]
fn box_parse_delegates_to_inner() {
    let mut input = Input::<NoRules>::new("test foo");
    let boxed = <Box<TestKeyword> as Parse>::parse(&mut input).unwrap();
    let _: Box<TestKeyword> = boxed;
    assert_eq!(input.cursor(), 4);
}

#[test]
fn box_peek_delegates_to_inner() {
    let input = Input::<NoRules>::new("test foo");
    assert!(<Box<TestKeyword> as Parse>::peek(&input));
}

#[test]
fn box_is_terminal_delegates() {
    assert!(<Box<TestKeyword> as Parse>::IS_TERMINAL);
}

#[test]
fn box_first_pattern_delegates() {
    assert_eq!(
        <Box<TestKeyword> as Parse>::first_pattern(),
        <TestKeyword as Parse>::first_pattern()
    );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — no `Parse` impl for `Box<TestKeyword>`.

**Step 3: Implement**

Add to `recursa-core/src/parse.rs`, after the `Scan` blanket impl:

```rust
/// Blanket implementation: `Box<T>` delegates to `T`.
/// Needed for recursive types like `Box<Expr>` in Pratt parsing.
impl<'input, T: Parse<'input>> Parse<'input> for Box<T> {
    type Rules = T::Rules;
    const IS_TERMINAL: bool = T::IS_TERMINAL;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(input: &Input<'input, Self::Rules>) -> bool {
        T::peek(input)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        Ok(Box::new(T::parse(input)?))
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/parse.rs recursa-core/src/lib.rs
git commit -m "Add blanket Parse impl for Box<T>"
```

---

## Task 2: Option\<T\> blanket Parse impl

Peek-based: if `T::peek` succeeds, parse and return `Some`. Otherwise return `None`.

**Files:**
- Modify: `recursa-core/src/parse.rs`
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Write the failing test**

Add to the test module in `recursa-core/src/lib.rs`:

```rust
#[test]
fn option_parse_some_when_peek_matches() {
    let mut input = Input::<NoRules>::new("test foo");
    let result = <Option<TestKeyword> as Parse>::parse(&mut input).unwrap();
    assert!(result.is_some());
    assert_eq!(input.cursor(), 4);
}

#[test]
fn option_parse_none_when_peek_fails() {
    let mut input = Input::<NoRules>::new("foo bar");
    let result = <Option<TestKeyword> as Parse>::parse(&mut input).unwrap();
    assert!(result.is_none());
    assert_eq!(input.cursor(), 0); // no input consumed
}

#[test]
fn option_peek_delegates() {
    let input = Input::<NoRules>::new("test foo");
    assert!(<Option<TestKeyword> as Parse>::peek(&input));

    let input2 = Input::<NoRules>::new("foo bar");
    assert!(!<Option<TestKeyword> as Parse>::peek(&input2));
}

#[test]
fn option_first_pattern_delegates() {
    assert_eq!(
        <Option<TestKeyword> as Parse>::first_pattern(),
        <TestKeyword as Parse>::first_pattern()
    );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — no `Parse` impl for `Option<TestKeyword>`.

**Step 3: Implement**

Add to `recursa-core/src/parse.rs`:

```rust
/// Blanket implementation: `Option<T>` is peek-based.
/// Returns `Some(T)` if `T::peek` succeeds, `None` otherwise.
/// If peek succeeds but parse fails, the error propagates.
impl<'input, T: Parse<'input>> Parse<'input> for Option<T> {
    type Rules = T::Rules;
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(input: &Input<'input, Self::Rules>) -> bool {
        T::peek(input)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        if T::peek(input) {
            Ok(Some(T::parse(input)?))
        } else {
            Ok(None)
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/parse.rs recursa-core/src/lib.rs
git commit -m "Add blanket Parse impl for Option<T>"
```

---

## Task 3: Seq Type, Marker Types, and Basic Structure

Create the `Seq` type with marker types. No `Parse` impl yet — just the data structure, constructors, accessors, and `Deref`.

**Files:**
- Create: `recursa-core/src/seq.rs`
- Modify: `recursa-core/src/lib.rs`
- Modify: `recursa-core/Cargo.toml` (add `vec1` dependency)

**Step 1: Write the failing tests**

Add to `recursa-core/src/lib.rs` test module:

```rust
use crate::seq::{Seq, NoTrailing, AllowEmpty, NonEmpty};

#[test]
fn seq_empty() {
    let seq: Seq<i32, ()> = Seq::empty();
    assert_eq!(seq.len(), 0);
    assert!(seq.is_empty());
    let elements: &Vec<i32> = &seq;
    assert!(elements.is_empty());
}

#[test]
fn seq_from_pairs() {
    let pairs = vec![
        (1, Some(())),
        (2, Some(())),
        (3, None),
    ];
    let seq: Seq<i32, ()> = Seq::from_pairs(pairs);
    assert_eq!(seq.len(), 3);
    let elements: &Vec<i32> = &seq;
    assert_eq!(elements, &[1, 2, 3]);
}

#[test]
fn seq_pairs_accessible() {
    let pairs = vec![
        (1, Some(',')),
        (2, None),
    ];
    let seq: Seq<i32, char> = Seq::from_pairs(pairs);
    let raw_pairs = seq.pairs();
    assert_eq!(raw_pairs.len(), 2);
    assert_eq!(raw_pairs[0], (1, Some(',')));
    assert_eq!(raw_pairs[1], (2, None));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `seq` module doesn't exist.

**Step 3: Implement**

Add `vec1` to `recursa-core/Cargo.toml`:

```toml
[dependencies]
regex = "1"
miette = { version = "7", features = ["fancy"] }
thiserror = "2"
vec1 = "1"
```

Create `recursa-core/src/seq.rs`:

```rust
use std::marker::PhantomData;
use std::ops::Deref;

// -- Marker types --

/// No trailing separator allowed. Last element has no separator.
pub struct NoTrailing;

/// Trailing separator is required. Every element must be followed by a separator.
pub struct RequiredTrailing;

/// Trailing separator is optional. Last element may or may not have a separator.
pub struct OptionalTrailing;

/// Sequence may be empty (zero elements).
pub struct AllowEmpty;

/// Sequence must have at least one element.
pub struct NonEmpty;

// -- Seq type --

/// A separated list of elements with type-level configuration.
///
/// - `T`: element type
/// - `S`: separator type
/// - `Trailing`: trailing separator policy (`NoTrailing`, `RequiredTrailing`, `OptionalTrailing`)
/// - `Empty`: emptiness policy (`AllowEmpty`, `NonEmpty`)
pub struct Seq<T, S, Trailing = NoTrailing, Empty = AllowEmpty> {
    pairs: Vec<(T, Option<S>)>,
    elements: Vec<T>,
    _phantom: PhantomData<(Trailing, Empty)>,
}

impl<T: Clone, S, Trailing, Empty> Seq<T, S, Trailing, Empty> {
    /// Create a Seq from raw element-separator pairs.
    pub fn from_pairs(pairs: Vec<(T, Option<S>)>) -> Self {
        let elements = pairs.iter().map(|(t, _)| t.clone()).collect();
        Self {
            pairs,
            elements,
            _phantom: PhantomData,
        }
    }

    /// Access the raw element-separator pairs.
    pub fn pairs(&self) -> &[(T, Option<S>)] {
        &self.pairs
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Whether the sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

impl<T: Clone, S> Seq<T, S, NoTrailing, AllowEmpty> {
    /// Create an empty Seq (only available for AllowEmpty variants).
    pub fn empty() -> Self {
        Self {
            pairs: Vec::new(),
            elements: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

// Deref to Vec<T> for AllowEmpty variants
impl<T: Clone, S, Trailing> Deref for Seq<T, S, Trailing, AllowEmpty> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

// Deref to Vec1<T> for NonEmpty variants is added in a later task
// once we verify vec1 is available and the Parse impl guarantees non-emptiness.
```

Update `recursa-core/src/lib.rs` to add:

```rust
pub mod seq;
```

Note: We store a separate `elements: Vec<T>` alongside `pairs` to support the `Deref` to `&Vec<T>`. This requires `T: Clone`. An alternative is to compute elements on the fly, but `Deref` must return a reference to an owned value. The `Clone` bound is acceptable since AST nodes are typically small or borrow from the input.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/seq.rs recursa-core/src/lib.rs recursa-core/Cargo.toml
git commit -m "Add Seq type with marker types and basic structure"
```

---

## Task 4: Seq Parse Impl — NoTrailing + AllowEmpty

Implement `Parse` for the simplest `Seq` variant: zero-or-more elements, no trailing separator.

**Files:**
- Modify: `recursa-core/src/seq.rs`
- Create: `recursa-derive/tests/seq_parse.rs`

**Step 1: Write the failing test**

Create `recursa-derive/tests/seq_parse.rs`:

```rust
#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_core::seq::{Seq, NoTrailing, AllowEmpty};
use recursa_derive::{Parse, Scan};

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ",")]
struct Comma;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\(")]
struct LParen;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\)")]
struct RParen;

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct ArgList<'input> {
    lparen: LParen,
    args: Seq<Ident<'input>, Comma>,
    rparen: RParen,
}

#[test]
fn seq_parse_no_trailing_allow_empty_with_elements() {
    let mut input = Input::<WsRules>::new("(a, b, c)");
    let arglist = ArgList::parse(&mut input).unwrap();
    let args: &Vec<Ident> = &arglist.args;
    assert_eq!(args.len(), 3);
    assert_eq!(args[0].0, "a");
    assert_eq!(args[1].0, "b");
    assert_eq!(args[2].0, "c");
}

#[test]
fn seq_parse_no_trailing_allow_empty_empty() {
    let mut input = Input::<WsRules>::new("()");
    let arglist = ArgList::parse(&mut input).unwrap();
    assert!(arglist.args.is_empty());
}

#[test]
fn seq_parse_no_trailing_single_element() {
    let mut input = Input::<WsRules>::new("(x)");
    let arglist = ArgList::parse(&mut input).unwrap();
    let args: &Vec<Ident> = &arglist.args;
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].0, "x");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — no `Parse` impl for `Seq`.

**Step 3: Implement**

The key challenge: `Seq<T, S>` needs a `Parse` impl, but `T` and `S` have potentially different `Rules` types. The struct's parse context handles rule rebinding, so the `Seq` parse needs to work with the element and separator types' own rules.

Since `Seq`'s element and separator types are typically `Scan` types (with `Rules = NoRules`), and the surrounding struct's derive handles `consume_ignored` + `rebind`, the simplest approach is: `Seq` has `type Rules = NoRules`, and the struct's generated code rebinds to `NoRules` before parsing the `Seq` field (just like it does for any `Scan` field).

But `T` might not be a `Scan` type — it could be a `Parse` struct like `Expr`. In that case `T::Rules` might be `WsRules`. So `Seq`'s `Rules` should match `T::Rules`.

The cleanest solution: `Seq` doesn't implement `Parse` directly. Instead, it has a `parse_with_rules<R: ParseRules>` method that takes an `Input<R>` and handles the element/separator rebinding internally. The `Parse` impl uses `T::Rules`.

Add to `recursa-core/src/seq.rs`:

```rust
use crate::error::ParseError;
use crate::input::Input;
use crate::parse::Parse;
use crate::rules::ParseRules;

impl<'input, T, S> Parse<'input> for Seq<T, S, NoTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
{
    type Rules = T::Rules;
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(_input: &Input<'input, Self::Rules>) -> bool {
        true // AllowEmpty: always valid (might parse zero elements)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        let mut pairs = Vec::new();

        // Peek for first element
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Ok(Self::from_pairs(pairs));
        }

        loop {
            // Parse element
            let mut rebound = input.rebind::<<T as Parse>::Rules>();
            let element = <T as Parse>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            // Peek for separator
            input.consume_ignored();
            let rebound = input.rebind::<::recursa_core::NoRules>();
            if !<S as Scan>::peek(&rebound) {
                // No separator — this is the last element
                pairs.push((element, None));
                break;
            }

            // Parse separator
            let mut rebound = input.rebind::<::recursa_core::NoRules>();
            let sep = <S as Scan>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            pairs.push((element, Some(sep)));

            input.consume_ignored();
        }

        Ok(Self::from_pairs(pairs))
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/seq.rs recursa-derive/tests/seq_parse.rs
git commit -m "Add Parse impl for Seq<T, S, NoTrailing, AllowEmpty>"
```

---

## Task 5: Seq Parse Impl — OptionalTrailing + AllowEmpty

**Files:**
- Modify: `recursa-core/src/seq.rs`
- Modify: `recursa-derive/tests/seq_parse.rs`

**Step 1: Write the failing tests**

Add to `recursa-derive/tests/seq_parse.rs`:

```rust
use recursa_core::seq::OptionalTrailing;

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct ArrayLit<'input> {
    lparen: LParen,
    elements: Seq<Ident<'input>, Comma, OptionalTrailing>,
    rparen: RParen,
}

#[test]
fn seq_optional_trailing_no_trailing() {
    let mut input = Input::<WsRules>::new("(a, b, c)");
    let arr = ArrayLit::parse(&mut input).unwrap();
    assert_eq!(arr.elements.len(), 3);
}

#[test]
fn seq_optional_trailing_with_trailing() {
    let mut input = Input::<WsRules>::new("(a, b, c,)");
    let arr = ArrayLit::parse(&mut input).unwrap();
    assert_eq!(arr.elements.len(), 3);
    // Last element should have Some separator (trailing comma)
    let pairs = arr.elements.pairs();
    assert!(pairs[2].1.is_some());
}

#[test]
fn seq_optional_trailing_empty() {
    let mut input = Input::<WsRules>::new("()");
    let arr = ArrayLit::parse(&mut input).unwrap();
    assert!(arr.elements.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — no `Parse` impl for `Seq<_, _, OptionalTrailing, AllowEmpty>`.

**Step 3: Implement**

Add to `recursa-core/src/seq.rs`:

```rust
impl<'input, T, S> Parse<'input> for Seq<T, S, OptionalTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
{
    type Rules = T::Rules;
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(_input: &Input<'input, Self::Rules>) -> bool {
        true // AllowEmpty
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        let mut pairs = Vec::new();

        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Ok(Self::from_pairs(pairs));
        }

        loop {
            // Parse element
            let mut rebound = input.rebind::<<T as Parse>::Rules>();
            let element = <T as Parse>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            // Peek for separator
            input.consume_ignored();
            let rebound = input.rebind::<::recursa_core::NoRules>();
            if !<S as Scan>::peek(&rebound) {
                pairs.push((element, None));
                break;
            }

            // Parse separator
            let mut rebound = input.rebind::<::recursa_core::NoRules>();
            let sep = <S as Scan>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            // Peek for next element — if absent, this was a trailing separator
            input.consume_ignored();
            let rebound = input.rebind::<<T as Parse>::Rules>();
            if !<T as Parse>::peek(&rebound) {
                pairs.push((element, Some(sep)));
                break;
            }

            pairs.push((element, Some(sep)));
        }

        Ok(Self::from_pairs(pairs))
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/seq.rs recursa-derive/tests/seq_parse.rs
git commit -m "Add Parse impl for Seq<T, S, OptionalTrailing, AllowEmpty>"
```

---

## Task 6: Seq Parse Impl — RequiredTrailing + AllowEmpty

**Files:**
- Modify: `recursa-core/src/seq.rs`
- Modify: `recursa-derive/tests/seq_parse.rs`

**Step 1: Write the failing tests**

Add to `recursa-derive/tests/seq_parse.rs`:

```rust
use recursa_core::seq::RequiredTrailing;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct StmtBlock<'input> {
    lparen: LParen,
    stmts: Seq<Ident<'input>, Semi, RequiredTrailing>,
    rparen: RParen,
}

#[test]
fn seq_required_trailing_with_elements() {
    let mut input = Input::<WsRules>::new("(a; b; c;)");
    let block = StmtBlock::parse(&mut input).unwrap();
    assert_eq!(block.stmts.len(), 3);
    // All elements should have Some separator
    for (_, sep) in block.stmts.pairs() {
        assert!(sep.is_some());
    }
}

#[test]
fn seq_required_trailing_empty() {
    let mut input = Input::<WsRules>::new("()");
    let block = StmtBlock::parse(&mut input).unwrap();
    assert!(block.stmts.is_empty());
}

#[test]
fn seq_required_trailing_error_on_missing_sep() {
    let mut input = Input::<WsRules>::new("(a; b)");
    let result = StmtBlock::parse(&mut input);
    // "b" has no trailing semicolon — should error
    assert!(result.is_err());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL.

**Step 3: Implement**

Add to `recursa-core/src/seq.rs`:

```rust
impl<'input, T, S> Parse<'input> for Seq<T, S, RequiredTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
{
    type Rules = T::Rules;
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(_input: &Input<'input, Self::Rules>) -> bool {
        true // AllowEmpty
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        let mut pairs = Vec::new();

        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Ok(Self::from_pairs(pairs));
        }

        loop {
            // Parse element
            let mut rebound = input.rebind::<<T as Parse>::Rules>();
            let element = <T as Parse>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            // Parse separator (required — error if missing)
            input.consume_ignored();
            let mut rebound = input.rebind::<::recursa_core::NoRules>();
            let sep = <S as Scan>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            pairs.push((element, Some(sep)));

            // Peek for next element — if absent, we're done
            input.consume_ignored();
            let rebound = input.rebind::<<T as Parse>::Rules>();
            if !<T as Parse>::peek(&rebound) {
                break;
            }
        }

        Ok(Self::from_pairs(pairs))
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/seq.rs recursa-derive/tests/seq_parse.rs
git commit -m "Add Parse impl for Seq<T, S, RequiredTrailing, AllowEmpty>"
```

---

## Task 7: NonEmpty Seq Variants

Add `Parse` impls for all three trailing policies with `NonEmpty`. These error if no elements are parsed. Also add `Deref` to `Vec1<T>`.

**Files:**
- Modify: `recursa-core/src/seq.rs`
- Modify: `recursa-derive/tests/seq_parse.rs`

**Step 1: Write the failing tests**

Add to `recursa-derive/tests/seq_parse.rs`:

```rust
use recursa_core::seq::NonEmpty;

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct NonEmptyArgList<'input> {
    lparen: LParen,
    args: Seq<Ident<'input>, Comma, NoTrailing, NonEmpty>,
    rparen: RParen,
}

#[test]
fn seq_non_empty_parses_elements() {
    let mut input = Input::<WsRules>::new("(a, b)");
    let arglist = NonEmptyArgList::parse(&mut input).unwrap();
    assert_eq!(arglist.args.len(), 2);
}

#[test]
fn seq_non_empty_errors_when_empty() {
    let mut input = Input::<WsRules>::new("()");
    let result = NonEmptyArgList::parse(&mut input);
    assert!(result.is_err());
}

#[test]
fn seq_non_empty_peek_delegates_to_element() {
    // NonEmpty peek delegates to T::peek
    let input = Input::<WsRules>::new("(a)");
    // We can't directly test Seq's peek here since it's used inside a struct,
    // but we can verify the struct parses correctly
    let mut input = Input::<WsRules>::new("(a)");
    let arglist = NonEmptyArgList::parse(&mut input).unwrap();
    assert_eq!(arglist.args.len(), 1);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — no `Parse` impl for `Seq<_, _, NoTrailing, NonEmpty>`.

**Step 3: Implement**

Add `NonEmpty` variants for all three trailing policies. The parse logic is identical to the `AllowEmpty` versions except:
- `peek` delegates to `T::peek` instead of returning `true`
- `parse` errors if the first element peek fails

Also add `Deref` to `Vec1<T>` for `NonEmpty` variants and an `empty()` method for all `AllowEmpty` trailing variants.

The `Deref` to `Vec1` requires that the `Seq` is non-empty. Since the `Parse` impl guarantees at least one element, this is safe. The `Vec1` is constructed in `from_pairs` with an assertion.

Add `from_pairs` and `Deref` specializations for `NonEmpty`:

```rust
impl<T: Clone, S, Trailing> Seq<T, S, Trailing, NonEmpty> {
    pub fn from_pairs(pairs: Vec<(T, Option<S>)>) -> Self {
        assert!(!pairs.is_empty(), "NonEmpty Seq must have at least one element");
        let elements = pairs.iter().map(|(t, _)| t.clone()).collect();
        Self {
            pairs,
            elements,
            _phantom: PhantomData,
        }
    }
}

impl<T: Clone, S, Trailing> Deref for Seq<T, S, Trailing, NonEmpty> {
    type Target = vec1::Vec1<T>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Parse impl guarantees non-empty; we assert in from_pairs
        // vec1::Vec1 can be constructed from a non-empty Vec
        // We need to store Vec1 instead of Vec for NonEmpty variants
        todo!("need to store Vec1 internally for NonEmpty")
    }
}
```

Actually, the `Deref` approach is tricky because we need to store different internal types for `AllowEmpty` vs `NonEmpty`. The simplest approach: store `Vec<T>` always, and for `NonEmpty` the `Deref` converts on the fly... but `Deref` returns a reference, so we can't create a `Vec1` on the fly.

**Revised approach:** Use two separate element storage fields gated by the `Empty` type parameter. Or simpler: store `Vec<T>` always, and for `NonEmpty` provide a method `as_vec1() -> &Vec1<T>` that transmutes (since `Vec1` is a newtype around `Vec` with the invariant that it's non-empty — check the `vec1` crate layout).

Actually, the cleanest approach: for `NonEmpty`, store a `Vec1<T>` in the `elements` field using an enum or by making `Seq` generic over the internal storage. This is getting complex.

**Simplest pragmatic approach:** Don't use `Deref`. Instead provide:
- `elements(&self) -> &[T]` on all `Seq` variants
- `elements_vec1(&self) -> &Vec1<T>` on `NonEmpty` variants (constructs `Vec1` from the internal `Vec`, panicking if empty — but the `Parse` impl guarantees non-empty)

Actually, let's just store `Vec1<T>` for `NonEmpty` by making the struct generic internally. The easiest way: `NonEmpty` variants use a separate `SeqNonEmpty` type that wraps `Vec1`. But this defeats the unified type.

**Final approach:** Keep `Vec<T>` internal storage. `Deref` for `AllowEmpty` targets `Vec<T>`. For `NonEmpty`, `Deref` targets `[T]` (a slice, which both `Vec` and `Vec1` deref to). Users who need `Vec1` can call `.to_vec1()` which does the conversion. This avoids the storage problem entirely.

Actually, even simpler: both `AllowEmpty` and `NonEmpty` deref to `[T]`. This is the most natural target since it's what both `Vec` and `Vec1` provide. Users get slice access by default, and can convert to `Vec1` when needed.

Implement this revised approach.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/seq.rs recursa-derive/tests/seq_parse.rs
git commit -m "Add NonEmpty Seq variants with Parse impls"
```

---

## Task 8: Re-export Container Types and Integration Test

Wire up re-exports and write an end-to-end integration test that uses `Option`, `Box`, and `Seq` together.

**Files:**
- Modify: `recursa-core/src/lib.rs` (re-exports)
- Modify: `src/lib.rs` (facade re-exports)
- Create: `tests/container_types.rs`

**Step 1: Write the integration test**

Create `tests/container_types.rs`:

```rust
#![allow(dead_code)]

use recursa::{Input, Parse, ParseRules, Scan};
use recursa::seq::{Seq, OptionalTrailing, NonEmpty};

#[derive(Scan, Debug, Clone)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = "=")]
struct Eq;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = ",")]
struct Comma;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\[")]
struct LBracket;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"\]")]
struct RBracket;

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug, Clone)]
#[scan(pattern = r"[0-9]+")]
struct IntLit<'input>(&'input str);

struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
}

// Array literal with optional trailing comma
#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct ArrayLit<'input> {
    lbracket: LBracket,
    elements: Seq<IntLit<'input>, Comma, OptionalTrailing>,
    rbracket: RBracket,
}

// Let binding with optional type annotation
#[derive(Parse, Debug)]
#[parse(rules = Lang)]
struct LetStmt<'input> {
    let_kw: LetKw,
    name: Ident<'input>,
    eq: Eq,
    value: ArrayLit<'input>,
    semi: Semi,
}

#[test]
fn integration_array_with_trailing_comma() {
    let mut input = Input::<Lang>::new("let x = [1, 2, 3,];");
    let stmt = LetStmt::parse(&mut input).unwrap();
    assert_eq!(stmt.name.0, "x");
    assert_eq!(stmt.value.elements.len(), 3);
    assert!(input.is_empty());
}

#[test]
fn integration_array_without_trailing_comma() {
    let mut input = Input::<Lang>::new("let x = [1, 2, 3];");
    let stmt = LetStmt::parse(&mut input).unwrap();
    assert_eq!(stmt.value.elements.len(), 3);
}

#[test]
fn integration_empty_array() {
    let mut input = Input::<Lang>::new("let x = [];");
    let stmt = LetStmt::parse(&mut input).unwrap();
    assert!(stmt.value.elements.is_empty());
}
```

**Step 2: Ensure re-exports are in place**

Update `recursa-core/src/lib.rs` to export the `seq` module as public.
Update `src/lib.rs` if needed (the `pub use recursa_core::*` should pick it up).

**Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All tests PASS.

**Step 4: Commit**

```bash
git add tests/container_types.rs recursa-core/src/lib.rs src/lib.rs
git commit -m "Add container types integration test with Option, Box, and Seq"
```

---

## Summary

| Task | What it delivers |
|------|-----------------|
| 1 | `Box<T>` blanket Parse impl |
| 2 | `Option<T>` blanket Parse impl |
| 3 | `Seq` type, marker types, structure, Deref |
| 4 | `Seq<T, S, NoTrailing, AllowEmpty>` Parse impl |
| 5 | `Seq<T, S, OptionalTrailing, AllowEmpty>` Parse impl |
| 6 | `Seq<T, S, RequiredTrailing, AllowEmpty>` Parse impl |
| 7 | `NonEmpty` variants for all three trailing policies |
| 8 | Re-exports and end-to-end integration test |
