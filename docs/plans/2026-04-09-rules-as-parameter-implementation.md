# Rules-as-Parameter Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove `type Rules` from `Parse` and `R` from `Input`, replacing them with a `rules` parameter on `peek` and `parse` methods, so that rules flow from parent to child at runtime.

**Architecture:** `Input<'input>` becomes unparameterized. `Parse::peek` and `Parse::parse` gain a generic `R: ParseRules` parameter. Derived structs with `#[parse(rules = X)]` ignore the passed-in rules and use their own. Scan types and container types (`Seq`, `Option`, `Box`) use the passed-in rules. This eliminates `rebind` entirely and solves the `Seq` rules problem.

**Tech Stack:** Rust, `syn`/`quote`/`proc-macro2`

---

## Overview of Changes

**What's removed:**
- `type Rules: ParseRules` from `Parse` trait
- `R: ParseRules` type parameter from `Input`
- `PhantomData<R>` from `Input`
- `rebind()` method from `Input`
- `impl_parse_for_scan!` macro (blanket impl can be restored since no `type Rules` to conflict)
- `R: ParseRules` parameter from `Seq` struct

**What's changed:**
- `Input<'input, R>` → `Input<'input>`
- `Parse::peek(input: &Input<R>)` → `Parse::peek<R: ParseRules>(input: &Input, rules: &R)`
- `Parse::parse(input: &mut Input<R>)` → `Parse::parse<R: ParseRules>(input: &mut Input, rules: &R)`
- `Input::consume_ignored()` → `Input::consume_ignored(rules: &impl ParseRules)` (or `Input::consume_ignored(ignore: &str)`)
- `Scan::peek(input: &Input<NoRules>)` → `Scan::peek(input: &Input)` (no rules needed at Scan level)
- `Scan::parse(input: &mut Input<NoRules>)` → `Scan::parse(input: &mut Input)` (no rules needed)

**Rules flow:**
- Derived struct with `#[parse(rules = WsRules)]`: ignores `rules` param, uses `WsRules` internally
- Scan types (blanket impl): `peek` doesn't use rules (just regex), `parse` uses rules to consume_ignored before matching
- `Box<T>`: passes `rules` through to `T`
- `Option<T>`: passes `rules` through to `T`
- `Seq<T, S>`: uses `rules` for whitespace between elements

**Note on blanket impl restoration:** With `type Rules` removed from `Parse`, there's no longer a coherence conflict between `impl<T: Scan> Parse for T` and `impl<T: Parse> Parse for Box<T>` / `Option<T>`. The blanket impl can be restored, eliminating the need for `impl_parse_for_scan!` and the duplicate Parse impl generation in `derive(Scan)`.

---

## Task 1: Refactor Input — Remove R Parameter

Remove the `R: ParseRules` type parameter from `Input`. Change `consume_ignored` to take a `&str` pattern parameter (the raw IGNORE string). Remove `rebind`.

**Files:**
- Modify: `recursa-core/src/input.rs`
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Update Input struct and methods**

```rust
pub struct Input<'input> {
    source: &'input str,
    cursor: usize,
}

impl<'input> Input<'input> {
    pub fn new(source: &'input str) -> Self {
        Self { source, cursor: 0 }
    }

    pub fn source(&self) -> &'input str { self.source }
    pub fn cursor(&self) -> usize { self.cursor }
    pub fn remaining(&self) -> &'input str { &self.source[self.cursor..] }
    pub fn advance(&mut self, n: usize) { self.cursor += n; }

    pub fn fork(&self) -> Self {
        Self { source: self.source, cursor: self.cursor }
    }

    pub fn commit(&mut self, fork: Self) {
        self.cursor = fork.cursor;
    }

    pub fn is_empty(&self) -> bool {
        self.cursor >= self.source.len()
    }

    /// Skip ignored content using the given IGNORE pattern.
    /// Pass an empty string for no-op.
    pub fn consume_ignored(&mut self, ignore: &str) {
        if ignore.is_empty() {
            return;
        }
        // Same Mutex-based regex caching as before, but keyed on the pattern string
        // ...
    }
}
```

Note: `rebind` is removed entirely.

**Step 2: Update tests in `recursa-core/src/lib.rs`**

All `Input::<NoRules>::new(...)` becomes `Input::new(...)`.
All `Input::<WsRules>::new(...)` becomes `Input::new(...)`.
All `input.consume_ignored()` becomes `input.consume_ignored(WsRules::IGNORE)` or `input.consume_ignored(NoRules::IGNORE)`.
Remove `rebind` tests.

**Step 3: Build recursa-core only (other crates will fail — that's expected)**

Run: `cargo test -p recursa-core --lib`

**Step 4: Commit**

```bash
git commit -m "Refactor Input: remove R type parameter, consume_ignored takes pattern string"
```

---

## Task 2: Refactor Parse Trait — Rules as Parameter

Remove `type Rules` from `Parse`. Add `R: ParseRules` parameter to `peek` and `parse`. Restore the blanket `impl<T: Scan> Parse for T`. Remove `impl_parse_for_scan!`.

**Files:**
- Modify: `recursa-core/src/parse.rs`
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Update Parse trait**

```rust
pub trait Parse<'input>: Sized {
    const IS_TERMINAL: bool;

    fn first_pattern() -> &'static str;

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool;

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError>;
}
```

**Step 2: Restore blanket impl for Scan**

```rust
impl<'input, T: Scan<'input>> Parse<'input> for T {
    const IS_TERMINAL: bool = true;

    fn first_pattern() -> &'static str {
        T::PATTERN
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        let mut fork = input.fork();
        fork.consume_ignored(R::IGNORE);
        T::regex().is_match(fork.remaining())
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        input.consume_ignored(R::IGNORE);
        <T as Scan>::parse(input)
    }
}
```

Note: The Scan blanket impl now handles `consume_ignored` using the passed-in rules. Each Scan type consumes leading whitespace before matching its token. This means the struct derive no longer needs to call `consume_ignored` before each field — each field's `parse` handles its own whitespace.

Wait — that changes the semantics. Currently struct derive calls `consume_ignored` before each field. If each Scan `parse` also calls `consume_ignored`, we'd double-consume. Let me reconsider.

**Actually:** The cleaner approach is: `Scan::parse` does NOT consume whitespace (it just matches its token). Whitespace consumption is the parent's responsibility. The `rules` parameter is passed through to children so that container types like `Seq` can consume whitespace between elements.

So the Scan blanket impl becomes:

```rust
fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
    T::regex().is_match(input.remaining())
}

fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
    <T as Scan>::parse(input)
}
```

And the struct derive generates:

```rust
fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
    let rules = WsRules;  // use own rules
    let mut fork = input.fork();
    fork.consume_ignored(WsRules::IGNORE);
    let field1 = <Field1 as Parse>::parse(&mut fork, &rules)?;
    fork.consume_ignored(WsRules::IGNORE);
    let field2 = <Field2 as Parse>::parse(&mut fork, &rules)?;
    // ...
    input.commit(fork);
    Ok(Self { field1, field2, ... })
}
```

**Step 3: Update Box and Option impls**

```rust
impl<'input, T: Parse<'input>> Parse<'input> for Box<T> {
    const IS_TERMINAL: bool = T::IS_TERMINAL;
    fn first_pattern() -> &'static str { T::first_pattern() }
    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool { T::peek(input, rules) }
    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        Ok(Box::new(T::parse(input, rules)?))
    }
}

impl<'input, T: Parse<'input>> Parse<'input> for Option<T> {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str { T::first_pattern() }
    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool { T::peek(input, rules) }
    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        if T::peek(input, rules) {
            Ok(Some(T::parse(input, rules)?))
        } else {
            Ok(None)
        }
    }
}
```

**Step 4: Remove `impl_parse_for_scan!` macro**

No longer needed — the blanket impl handles Scan → Parse.

**Step 5: Update tests**

All `<T as Parse>::parse(&mut input)` becomes `<T as Parse>::parse(&mut input, &NoRules)` or `<T as Parse>::parse(&mut input, &WsRules)` depending on context.

**Step 6: Build and test recursa-core**

Run: `cargo test -p recursa-core --lib`

**Step 7: Commit**

```bash
git commit -m "Refactor Parse trait: remove type Rules, add rules parameter to peek/parse"
```

---

## Task 3: Refactor Scan Trait — Remove NoRules from Input

Update `Scan::peek` and `Scan::parse` default impls to use `Input<'input>` instead of `Input<'input, NoRules>`.

**Files:**
- Modify: `recursa-core/src/scan.rs`
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Update Scan trait**

```rust
pub trait Scan<'input>: Sized {
    const PATTERN: &'static str;
    fn regex() -> &'static Regex;
    fn from_match(matched: &'input str) -> Result<Self, ParseError>;

    fn peek(input: &Input<'input>) -> bool {
        Self::regex().is_match(input.remaining())
    }

    fn parse(input: &mut Input<'input>) -> Result<Self, ParseError> {
        match Self::regex().find(input.remaining()) {
            Some(m) if m.start() == 0 => {
                let matched = &input.source()[input.cursor()..input.cursor() + m.len()];
                let result = Self::from_match(matched)?;
                input.advance(m.len());
                Ok(result)
            }
            Some(_) | None => Err(ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                Self::PATTERN,
            )),
        }
    }
}
```

**Step 2: Update tests**

Tests that explicitly call `<T as Scan>::peek` or `<T as Scan>::parse` now pass `Input::new(...)` without type parameter.

**Step 3: Build and test**

Run: `cargo test -p recursa-core`

**Step 4: Commit**

```bash
git commit -m "Refactor Scan trait: Input no longer parameterized by rules"
```

---

## Task 4: Refactor derive(Scan) — Remove Parse Impl Generation

Since the blanket `impl<T: Scan> Parse for T` is restored, `derive(Scan)` no longer needs to generate a `Parse` impl. It only generates the `Scan` impl.

**Files:**
- Modify: `recursa-derive/src/scan_derive.rs`
- Modify: `recursa-derive/tests/scan_unit_struct.rs`
- Modify: `recursa-derive/tests/scan_tuple_struct.rs`
- Modify: `recursa-derive/tests/scan_enum.rs`

**Step 1: Remove `generate_parse_for_scan` from scan_derive.rs**

The `derive_scan_unit_struct`, `derive_scan_tuple_struct`, and `derive_scan_enum` functions should only generate `Scan` impls, not `Parse` impls. Remove any `Parse` impl generation code.

**Step 2: Update tests**

Tests that use `<T as Parse>::parse(...)` now pass a rules parameter.
Tests that use `<T as Parse>::peek(...)` now pass a rules parameter.
`Input::<NoRules>::new(...)` becomes `Input::new(...)`.

**Step 3: Build and test**

Run: `cargo test -p recursa-derive`

**Step 4: Commit**

```bash
git commit -m "Simplify derive(Scan): blanket impl handles Parse, remove generated Parse impls"
```

---

## Task 5: Refactor derive(Parse) for Structs

Update the struct derive to pass rules as a parameter instead of using `type Rules` and `rebind`.

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs` (the `derive_parse_struct` function)
- Modify: `recursa-derive/tests/parse_struct.rs`

**Step 1: Update generated code**

The generated `Parse` impl for a struct with `#[parse(rules = WsRules)]`:

```rust
impl<'input> Parse<'input> for LetBinding<'input> {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // ... same as before, uses WsRules::IGNORE for separator
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        fork.consume_ignored(WsRules::IGNORE);
        <FirstFieldType as Parse>::peek(&fork, &WsRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        let rules = WsRules;
        let mut fork = input.fork();
        fork.consume_ignored(WsRules::IGNORE);
        let field1 = <Field1 as Parse>::parse(&mut fork, &rules)?;
        fork.consume_ignored(WsRules::IGNORE);
        let field2 = <Field2 as Parse>::parse(&mut fork, &rules)?;
        // ...
        input.commit(fork);
        Ok(Self { field1, field2, ... })
    }
}
```

Key differences from before:
- No `type Rules`
- No `rebind` calls
- `consume_ignored` takes `WsRules::IGNORE` string
- Field parse calls pass `&rules` (the struct's own rules)
- The `_rules` parameter is ignored in favour of the struct's own rules

**Step 2: Update tests**

`Input::<WsRules>::new(...)` → `Input::new(...)`
`LetBinding::parse(&mut input)` → `LetBinding::parse(&mut input, &WsRules)`
`LetBinding::peek(&input)` → `LetBinding::peek(&input, &WsRules)`

**Step 3: Build and test**

Run: `cargo test -p recursa-derive -- parse_struct`

**Step 4: Commit**

```bash
git commit -m "Refactor derive(Parse) for structs: rules as parameter, remove rebind"
```

---

## Task 6: Refactor derive(Parse) for Enums

Update the enum derive. The combined peek regex and dispatch logic stay the same, but `Input` and `Parse` signatures change.

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs` (the `derive_parse_enum` function)
- Modify: `recursa-derive/tests/parse_enum.rs`
- Modify: `recursa-derive/tests/parse_lookahead.rs`

**Step 1: Update generated code**

The enum with `#[parse(rules = WsRules)]`:

```rust
fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
    let mut peek_input = input.fork();
    peek_input.consume_ignored(WsRules::IGNORE);
    peek_regex().is_match(peek_input.remaining())
}

fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
    let rules = WsRules;
    let regex = peek_regex();
    let mut fork = input.fork();
    fork.consume_ignored(WsRules::IGNORE);
    // ... regex capture matching same as before ...
    // dispatch: <InnerType as Parse>::parse(&mut fork, &rules)?;
}
```

Key: no `rebind`, variant parse calls pass `&rules`.

**Step 2: Update tests**

Same pattern: remove `Input::<WsRules>`, add `&WsRules` to parse/peek calls.

**Step 3: Build and test**

Run: `cargo test -p recursa-derive -- parse_enum parse_lookahead`

**Step 4: Commit**

```bash
git commit -m "Refactor derive(Parse) for enums: rules as parameter, remove rebind"
```

---

## Task 7: Refactor derive(Parse) for Pratt Enums

Update the Pratt enum derive. The Pratt `parse_expr` helper function gains a `rules` parameter.

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs` (the `derive_parse_pratt_enum` function)
- Modify: `recursa-derive/tests/parse_pratt.rs`

**Step 1: Update generated code**

```rust
fn parse_expr<'input, R: ParseRules>(
    input: &mut Input<'input>,
    rules: &R,
    min_bp: u32,
) -> Result<Expr<'input>, ParseError> {
    input.consume_ignored(WsRules::IGNORE);
    // nud: peek/parse atoms and prefix operators using &rules
    // led loop: peek/parse infix operators using &rules
}
```

**Step 2: Update tests**

Same pattern.

**Step 3: Build and test**

Run: `cargo test -p recursa-derive -- parse_pratt`

**Step 4: Commit**

```bash
git commit -m "Refactor derive(Parse) for Pratt enums: rules as parameter"
```

---

## Task 8: Refactor Seq — Remove R Parameter

Remove the `R: ParseRules` type parameter from `Seq`. The Parse impls use the passed-in `rules` parameter for whitespace handling.

**Files:**
- Modify: `recursa-core/src/seq.rs`
- Modify: `recursa-derive/tests/seq_parse.rs`

**Step 1: Update Seq struct**

```rust
pub struct Seq<T, S, Trailing = NoTrailing, Empty = AllowEmpty> {
    pairs: Vec<(T, Option<S>)>,
    elements: Vec<T>,
    _phantom: PhantomData<(Trailing, Empty)>,
}
```

**Step 2: Update Parse impls**

The `rules` parameter flows through to element parsing and `consume_ignored`:

```rust
impl<'input, T, S> Parse<'input> for Seq<T, S, NoTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
{
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str { T::first_pattern() }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool { true }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        if !<T as Parse>::peek(input, rules) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_no_trailing::<T, S, R>(input, rules)?;
        Ok(Self::from_pairs(pairs))
    }
}
```

Helper functions:

```rust
fn parse_no_trailing<'input, T, S, R>(
    input: &mut Input<'input>,
    rules: &R,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let element = <T as Parse>::parse(input, rules)?;

        input.consume_ignored(R::IGNORE);
        if !<S as Scan>::peek(input) {
            pairs.push((element, None));
            break;
        }

        let sep = <S as Scan>::parse(input)?;
        pairs.push((element, Some(sep)));
        input.consume_ignored(R::IGNORE);
    }
    Ok(pairs)
}
```

Note how much simpler this is — no `rebind`, just pass `rules` through. Scan types are called directly without rules.

**Step 3: Update tests**

Remove `WsRules` from `Seq` type parameters:
`Seq<Ident<'input>, Comma, WsRules>` → `Seq<Ident<'input>, Comma>`
`Seq<Ident<'input>, Comma, WsRules, OptionalTrailing>` → `Seq<Ident<'input>, Comma, OptionalTrailing>`

Parse/peek calls add `&WsRules` parameter.

**Step 4: Build and test**

Run: `cargo test -p recursa-derive -- seq`

**Step 5: Commit**

```bash
git commit -m "Refactor Seq: remove R parameter, use passed-in rules for whitespace"
```

---

## Task 9: Update Integration Tests and Facade

Update all remaining test files and the facade crate.

**Files:**
- Modify: `tests/mini_language.rs`
- Modify: `tests/container_types.rs`
- Modify: `tests/bulk_macros.rs`
- Modify: `tests/integration.rs`
- Modify: `src/lib.rs`
- Modify: `recursa-core/src/lib.rs` (re-exports)

**Step 1: Update all test files**

Same mechanical changes: remove `Input::<Rules>` type parameter, add `rules` to parse/peek calls.

**Step 2: Update re-exports if needed**

`NoRules` may no longer need to be exported (or may still be useful). Check.

**Step 3: Full workspace test**

Run: `cargo test --workspace`
Expected: All tests pass, zero warnings.

**Step 4: Commit**

```bash
git commit -m "Update integration tests and facade for rules-as-parameter architecture"
```

---

## Task 10: Cleanup and Verification

Final cleanup pass.

**Step 1: Remove dead code**

- Check for any remaining references to `rebind`
- Check for any remaining `Input<'input, R>` usage
- Check for any remaining `type Rules`
- Remove `NoRules` if it's no longer needed (it may still be needed for `ParseRules::IGNORE` empty string, and as a default rules type)

**Step 2: Run clippy**

Run: `cargo clippy --workspace`

**Step 3: Run doc generation**

Run: `cargo doc`

**Step 4: Commit if any cleanup was needed**

```bash
git commit -m "Cleanup: remove dead code from rules-as-parameter refactor"
```

---

## Summary

| Task | What it changes |
|------|----------------|
| 1 | `Input` — remove `R` parameter, `consume_ignored` takes pattern string |
| 2 | `Parse` trait — remove `type Rules`, add `rules` parameter, restore Scan blanket impl |
| 3 | `Scan` trait — update signatures for unparameterized `Input` |
| 4 | `derive(Scan)` — remove generated Parse impls (blanket impl handles it) |
| 5 | `derive(Parse)` for structs — rules as parameter, remove rebind |
| 6 | `derive(Parse)` for enums — rules as parameter, remove rebind |
| 7 | `derive(Parse)` for Pratt — rules as parameter |
| 8 | `Seq` — remove `R` parameter, use passed-in rules |
| 9 | Integration tests and facade updates |
| 10 | Cleanup and verification |

**Migration strategy:** Tasks 1-3 update the core crate (other crates will temporarily fail to compile). Tasks 4-8 update the derive macros and Seq. Task 9 updates tests. Task 10 cleans up. Each task should be committed even if intermediate states don't compile across the full workspace — `cargo test -p recursa-core` should pass after tasks 1-3, and `cargo test --workspace` should pass after task 9.
