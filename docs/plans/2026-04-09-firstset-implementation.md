# FirstSet / Lookahead Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add multi-token lookahead to enum `Parse` dispatch using combined peek regexes built from each variant's terminal prefix.

**Architecture:** Add `IS_TERMINAL` const and `first_patterns()` method to the `Parse` trait. Update all derive macros to generate these. Replace enum sequential peek dispatch with a single combined regex using named capture groups.

**Tech Stack:** Rust, `regex`, `syn`/`quote`/`proc-macro2`, `OnceLock`

**Design doc:** `docs/plans/2026-04-09-firstset-design.md`

---

## Task 1: Add IS_TERMINAL and first_patterns to Parse Trait

Add the two new items to the `Parse` trait and update the blanket impl for `Scan` types.

**Files:**
- Modify: `recursa-core/src/parse.rs`
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Write the failing tests**

Add to the test module in `recursa-core/src/lib.rs`:

```rust
#[test]
fn scan_type_is_terminal() {
    assert!(<TestKeyword as Parse>::IS_TERMINAL);
}

#[test]
fn scan_type_first_patterns() {
    let patterns = <TestKeyword as Parse>::first_patterns();
    assert_eq!(patterns, &["test"]);
}

#[test]
fn scan_ident_first_patterns() {
    let patterns = <TestIdent as Parse>::first_patterns();
    assert_eq!(patterns, &[r"[a-zA-Z_][a-zA-Z0-9_]*"]);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `IS_TERMINAL` and `first_patterns` not defined on `Parse`.

**Step 3: Implement**

Modify `recursa-core/src/parse.rs`:

```rust
pub trait Parse<'input>: Sized {
    type Rules: ParseRules;

    /// Whether this type is a leaf token (Scan type) or a composite production.
    const IS_TERMINAL: bool;

    /// The terminal prefix patterns for this production.
    ///
    /// For Scan types, returns the single token pattern.
    /// For structs, returns consecutive terminal field patterns from the start.
    /// For enums, returns variant prefix patterns used to build combined peek regexes.
    fn first_patterns() -> &'static [&'static str];

    fn peek(input: &Input<'input, Self::Rules>) -> bool;
    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError>;
}

impl<'input, T: Scan<'input>> Parse<'input> for T {
    type Rules = NoRules;
    const IS_TERMINAL: bool = true;

    fn first_patterns() -> &'static [&'static str] {
        // Leak a boxed slice to get a &'static [&'static str].
        // This happens once per type via the blanket impl.
        static PATTERNS: std::sync::OnceLock<&'static [&'static str]> = std::sync::OnceLock::new();
        PATTERNS.get_or_init(|| {
            let patterns = vec![T::PATTERN];
            patterns.leak()
        })
    }

    fn peek(input: &Input<'input, NoRules>) -> bool {
        <T as Scan>::peek(input)
    }

    fn parse(input: &mut Input<'input, NoRules>) -> Result<Self, ParseError> {
        <T as Scan>::parse(input)
    }
}
```

Note: The `OnceLock` + `leak` approach for `first_patterns` in the blanket impl avoids the problem that we can't return `&[Self::PATTERN]` as a `&'static` from a generic context (the static would need to be per-type). An alternative is to use a function-local static per monomorphisation — but `OnceLock` with `leak` is simpler. The derive macros will use a cleaner approach since they know the concrete patterns at codegen time.

Actually, a simpler approach: since `Scan` types always have exactly one pattern, and `PATTERN` is already `&'static str`, we can use a trick with a one-element array. But Rust doesn't allow `&[Self::PATTERN]` as a static reference in a generic context.

The cleanest approach is to have the derive macro implement `first_patterns` directly on each `Scan` type (not through the blanket impl). But that means removing `first_patterns` and `IS_TERMINAL` from the blanket impl and adding them to the derive macros for `Scan` types.

**Revised approach:** Keep `IS_TERMINAL` and `first_patterns` in the blanket impl. For the blanket impl, use a one-element leaked vec. The derive macros will generate their own `first_patterns` anyway for structs/enums, so the blanket impl only handles `Scan` types.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/parse.rs recursa-core/src/lib.rs
git commit -m "Add IS_TERMINAL and first_patterns to Parse trait"
```

---

## Task 2: Update derive(Scan) to Generate IS_TERMINAL and first_patterns

The blanket impl handles these for `Scan` types, but the blanket impl uses a `OnceLock` + `leak` approach. Since the derive macros generate explicit `impl Scan` blocks (not going through the blanket impl for `Parse`), we need to verify the blanket impl picks up the new trait items correctly.

Actually — the blanket impl `impl<T: Scan> Parse for T` provides `IS_TERMINAL` and `first_patterns` for ALL `Scan` types automatically. The derive macros for `Scan` only generate the `Scan` impl, and the `Parse` impl comes from the blanket. So no changes are needed to `scan_derive.rs`.

However, we need to verify this works with the derived `Scan` types. This is a test-only task.

**Files:**
- Modify: `recursa-derive/tests/scan_unit_struct.rs`
- Modify: `recursa-derive/tests/scan_tuple_struct.rs`

**Step 1: Write the tests**

Add to `recursa-derive/tests/scan_unit_struct.rs`:

```rust
use recursa_core::Parse;

#[test]
fn scan_unit_struct_is_terminal() {
    assert!(<LetKeyword as Parse>::IS_TERMINAL);
}

#[test]
fn scan_unit_struct_first_patterns() {
    let patterns = <LetKeyword as Parse>::first_patterns();
    assert_eq!(patterns, &["let"]);
}
```

Add to `recursa-derive/tests/scan_tuple_struct.rs`:

```rust
use recursa_core::Parse;

#[test]
fn scan_tuple_struct_is_terminal() {
    assert!(<Ident as Parse>::IS_TERMINAL);
}

#[test]
fn scan_tuple_struct_first_patterns() {
    let patterns = <Ident as Parse>::first_patterns();
    assert_eq!(patterns, &[r"[a-zA-Z_][a-zA-Z0-9_]*"]);
}
```

**Step 2: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — the blanket impl provides these for all Scan types.

If they fail, the blanket impl needs fixing. If they pass, no code changes needed.

**Step 3: Commit**

```bash
git add recursa-derive/tests/
git commit -m "Add tests for IS_TERMINAL and first_patterns on Scan types"
```

---

## Task 3: Update derive(Parse) for Structs to Generate IS_TERMINAL and first_patterns

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs` (the `derive_parse_struct` function)
- Modify: `recursa-derive/tests/parse_struct.rs`

**Step 1: Write the failing tests**

Add to `recursa-derive/tests/parse_struct.rs`:

```rust
#[test]
fn parse_struct_is_not_terminal() {
    assert!(!<LetBinding as Parse>::IS_TERMINAL);
}

#[test]
fn parse_struct_first_patterns_consecutive_terminals() {
    // LetBinding fields: LetKw, Ident, Eq, IntLit, Semi
    // All are Scan (terminal) types, so first_patterns should include
    // all of them until a non-terminal is hit.
    // Since ALL fields here are terminal, we get all patterns.
    let patterns = <LetBinding as Parse>::first_patterns();
    assert_eq!(patterns, &["let", r"[a-zA-Z_][a-zA-Z0-9_]*", "=", r"[0-9]+", ";"]);
}
```

Also add a test struct with a non-terminal field to verify the walk stops:

```rust
#[derive(Parse)]
#[parse(rules = WsRules)]
struct NestedStmt<'input> {
    let_kw: LetKw,
    binding: LetBinding<'input>,  // non-terminal: IS_TERMINAL = false
}

#[test]
fn parse_struct_first_patterns_stops_at_non_terminal() {
    // NestedStmt fields: LetKw (terminal), LetBinding (non-terminal)
    // Walk stops after LetKw, so only "let" is returned.
    let patterns = <NestedStmt as Parse>::first_patterns();
    assert_eq!(patterns, &["let"]);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — `IS_TERMINAL` and `first_patterns` not generated by derive(Parse) for structs.

**Step 3: Implement**

In `recursa-derive/src/parse_derive.rs`, update `derive_parse_struct` to generate `IS_TERMINAL` and `first_patterns`. Add these inside the generated `impl Parse` block:

```rust
const IS_TERMINAL: bool = false;

fn first_patterns() -> &'static [&'static str] {
    static PATTERNS: ::std::sync::OnceLock<::std::vec::Vec<&'static str>> = ::std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        let mut patterns = ::std::vec::Vec::new();
        // For field 1:
        patterns.extend(<Field1Type as ::recursa_core::Parse>::first_patterns());
        if <Field1Type as ::recursa_core::Parse>::IS_TERMINAL {
            // For field 2:
            patterns.extend(<Field2Type as ::recursa_core::Parse>::first_patterns());
            if <Field2Type as ::recursa_core::Parse>::IS_TERMINAL {
                // ... continue for each field
            }
        }
        patterns
    })
}
```

The derive macro generates this as a nested chain. For each field in order:
1. Extend patterns with the field type's `first_patterns()`
2. If the field type's `IS_TERMINAL` is true, continue to the next field
3. If false, stop

The code generation in `derive_parse_struct` should build this as nested `if` blocks:

```rust
let first_patterns_body = {
    let mut stmts = Vec::new();
    for ty in &field_types {
        stmts.push(quote! {
            patterns.extend(<#ty as ::recursa_core::Parse>::first_patterns());
        });
        // Only continue to next field if this one is terminal
        // We build a nested if chain
    }
    // Build the nested structure
    let mut body = quote! {};
    for ty in field_types.iter().rev() {
        body = quote! {
            patterns.extend(<#ty as ::recursa_core::Parse>::first_patterns());
            if <#ty as ::recursa_core::Parse>::IS_TERMINAL {
                #body
            }
        };
    }
    body
};
```

Generate into the impl block:

```rust
const IS_TERMINAL: bool = false;

fn first_patterns() -> &'static [&'static str] {
    static PATTERNS: ::std::sync::OnceLock<::std::vec::Vec<&'static str>> = ::std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        let mut patterns = ::std::vec::Vec::new();
        #first_patterns_body
        patterns
    })
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-derive/src/parse_derive.rs recursa-derive/tests/parse_struct.rs
git commit -m "Generate IS_TERMINAL and first_patterns for derived Parse structs"
```

---

## Task 4: Update derive(Parse) for Enums to Generate first_patterns and Combined Peek Regex

This is the main payoff — enums get a combined peek regex built from variant prefixes.

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs` (the `derive_parse_enum` function)
- Create: `recursa-derive/tests/parse_lookahead.rs`

**Step 1: Write the failing tests**

Create `recursa-derive/tests/parse_lookahead.rs`:

```rust
#![allow(dead_code)]

use recursa_core::{Input, Parse, ParseRules};
use recursa_derive::{Parse, Scan};

// -- Tokens --

#[derive(Scan, Debug)]
#[scan(pattern = "pub")]
struct PubKw;

#[derive(Scan, Debug)]
#[scan(pattern = "fn")]
struct FnKw;

#[derive(Scan, Debug)]
#[scan(pattern = "struct")]
struct StructKw;

#[derive(Scan, Debug)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);

#[derive(Scan, Debug)]
#[scan(pattern = r"\{")]
struct LBrace;

#[derive(Scan, Debug)]
#[scan(pattern = r"\}")]
struct RBrace;

// -- Rules --

struct WsRules;
impl ParseRules for WsRules {
    const IGNORE: &'static str = r"\s+";
}

// -- AST: two structs that share the same first token (pub) --

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct FnDecl<'input> {
    pub_kw: PubKw,
    fn_kw: FnKw,
    name: Ident<'input>,
    lbrace: LBrace,
    rbrace: RBrace,
}

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
struct StructDecl<'input> {
    pub_kw: PubKw,
    struct_kw: StructKw,
    name: Ident<'input>,
    lbrace: LBrace,
    rbrace: RBrace,
}

// -- Enum with ambiguous first token --

#[derive(Parse, Debug)]
#[parse(rules = WsRules)]
enum Declaration<'input> {
    Fn(FnDecl<'input>),
    Struct(StructDecl<'input>),
}

// -- Tests --

#[test]
fn lookahead_parses_fn_decl() {
    let mut input = Input::<WsRules>::new("pub fn foo {}");
    let decl = Declaration::parse(&mut input).unwrap();
    assert!(matches!(decl, Declaration::Fn(_)));
}

#[test]
fn lookahead_parses_struct_decl() {
    let mut input = Input::<WsRules>::new("pub struct Bar {}");
    let decl = Declaration::parse(&mut input).unwrap();
    assert!(matches!(decl, Declaration::Struct(_)));
}

#[test]
fn lookahead_peek_fn() {
    let input = Input::<WsRules>::new("pub fn foo {}");
    assert!(Declaration::peek(&input));
}

#[test]
fn lookahead_peek_struct() {
    let input = Input::<WsRules>::new("pub struct Bar {}");
    assert!(Declaration::peek(&input));
}

#[test]
fn lookahead_peek_fails() {
    let input = Input::<WsRules>::new("let x = 1;");
    assert!(!Declaration::peek(&input));
}

#[test]
fn lookahead_error_on_mismatch() {
    let mut input = Input::<WsRules>::new("pub let x;");
    let err = Declaration::parse(&mut input);
    assert!(err.is_err());
}

#[test]
fn lookahead_first_patterns_fn_decl() {
    // FnDecl: PubKw, FnKw, Ident, LBrace, RBrace — all terminal
    let patterns = <FnDecl as Parse>::first_patterns();
    assert_eq!(patterns, &["pub", "fn", r"[a-zA-Z_][a-zA-Z0-9_]*", r"\{", r"\}"]);
}

#[test]
fn lookahead_first_patterns_struct_decl() {
    let patterns = <StructDecl as Parse>::first_patterns();
    assert_eq!(patterns, &["pub", "struct", r"[a-zA-Z_][a-zA-Z0-9_]*", r"\{", r"\}"]);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — enum doesn't use combined peek regex and picks wrong variant when both start with `pub`.

**Step 3: Implement**

Update `derive_parse_enum` in `recursa-derive/src/parse_derive.rs`. The key changes:

1. Generate `IS_TERMINAL` and `first_patterns()` for the enum
2. Build a combined peek regex from variant prefixes, with `IGNORE` spliced between patterns and named capture groups per variant
3. Replace the sequential `peek` with a single regex match
4. Replace the sequential `parse` dispatch with regex-guided dispatch

The generated `first_patterns` for an enum collects all variant inner types' first patterns:

```rust
fn first_patterns() -> &'static [&'static str] {
    static PATTERNS: OnceLock<Vec<&'static str>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let mut patterns = Vec::new();
        patterns.extend(<FnDecl as Parse>::first_patterns());
        patterns.extend(<StructDecl as Parse>::first_patterns());
        patterns
    })
}
```

The combined peek regex is built by joining each variant's prefix patterns with the `IGNORE` pattern between tokens:

```rust
static PEEK_REGEX: OnceLock<Regex> = OnceLock::new();
let regex = PEEK_REGEX.get_or_init(|| {
    let ignore = <Rules>::IGNORE;
    let ignore_pat = if ignore.is_empty() { String::new() } else { format!("(?:{})", ignore) };

    let mut variant_patterns = Vec::new();

    // Variant 0: FnDecl
    {
        let prefixes = <FnDecl as Parse>::first_patterns();
        let joined = prefixes.join(&ignore_pat);
        variant_patterns.push(format!("(?P<_0>{})", joined));
    }

    // Variant 1: StructDecl
    {
        let prefixes = <StructDecl as Parse>::first_patterns();
        let joined = prefixes.join(&ignore_pat);
        variant_patterns.push(format!("(?P<_1>{})", joined));
    }

    let combined = format!(r"\A(?:{})", variant_patterns.join("|"));
    Regex::new(&combined).unwrap()
});
```

For `peek`: run the regex, return true if it matches.

For `parse`: run the regex, find which named group matched (longest match, declaration order tiebreaker), then parse that variant's inner type directly using the existing fork/rebind/commit pattern.

The generated code for `parse`:

```rust
fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
    let mut fork = input.fork();
    fork.consume_ignored();

    let regex = /* get or init the PEEK_REGEX */;

    let captures = match regex.captures(fork.remaining()) {
        Some(c) => c,
        None => {
            return Err(ParseError::merge(vec![/* error per variant */]));
        }
    };

    // Find longest match, declaration order tiebreaker
    let mut best_len = 0usize;
    let mut best_index: Option<usize> = None;
    if let Some(m) = captures.name("_0") {
        if m.len() > best_len { best_len = m.len(); best_index = Some(0); }
    }
    if let Some(m) = captures.name("_1") {
        if m.len() > best_len { best_len = m.len(); best_index = Some(1); }
    }

    match best_index {
        Some(0) => {
            let mut rebound = fork.rebind::<<FnDecl as Parse>::Rules>();
            let inner = <FnDecl as Parse>::parse(&mut rebound)?;
            input.commit(rebound.rebind());
            Ok(Declaration::Fn(inner))
        }
        Some(1) => {
            let mut rebound = fork.rebind::<<StructDecl as Parse>::Rules>();
            let inner = <StructDecl as Parse>::parse(&mut rebound)?;
            input.commit(rebound.rebind());
            Ok(Declaration::Struct(inner))
        }
        _ => Err(ParseError::merge(vec![/* errors */]))
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive`
Expected: PASS — including the previously-failing `pub fn` vs `pub struct` lookahead tests.

Also verify all existing tests still pass:

Run: `cargo test --workspace`
Expected: All tests PASS.

**Step 5: Commit**

```bash
git add recursa-derive/src/parse_derive.rs recursa-derive/tests/parse_lookahead.rs
git commit -m "Implement combined peek regex for enum Parse dispatch with multi-token lookahead"
```

---

## Task 5: Update derive(Parse) for Pratt Enums

Pratt enums need `IS_TERMINAL` and `first_patterns` but keep their existing sequential peek.

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs` (the `derive_parse_pratt_enum` function)
- Modify: `recursa-derive/tests/parse_pratt.rs`

**Step 1: Write the failing tests**

Add to `recursa-derive/tests/parse_pratt.rs`:

```rust
#[test]
fn pratt_is_not_terminal() {
    assert!(!<Expr as Parse>::IS_TERMINAL);
}

#[test]
fn pratt_first_patterns_includes_atoms_and_prefix() {
    let patterns = <Expr as Parse>::first_patterns();
    // Should include atom patterns (IntLit, Ident) and prefix operator (Minus)
    // but NOT infix operators (Plus, Star)
    assert!(patterns.contains(&r"[0-9]+"));
    assert!(patterns.contains(&r"[a-zA-Z_][a-zA-Z0-9_]*"));
    assert!(patterns.contains(&r"-"));
    assert!(!patterns.contains(&r"\+"));
    assert!(!patterns.contains(&r"\*"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive`
Expected: FAIL — `IS_TERMINAL` and `first_patterns` not generated for Pratt enums.

**Step 3: Implement**

In `derive_parse_pratt_enum`, add inside the generated `const _: () = { ... }` block, in the `impl Parse` block:

```rust
const IS_TERMINAL: bool = false;

fn first_patterns() -> &'static [&'static str] {
    static PATTERNS: ::std::sync::OnceLock<::std::vec::Vec<&'static str>> = ::std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        let mut patterns = ::std::vec::Vec::new();
        // Atom variants: include their inner type's first_patterns
        #(patterns.extend(<#atom_type as ::recursa_core::Parse>::first_patterns());)*
        // Prefix variants: include the operator's first_patterns
        #(patterns.extend(<#prefix_op_type as ::recursa_core::Parse>::first_patterns());)*
        // Infix variants: NOT included (checked in the led loop, not nud)
        patterns
    })
}
```

The derive macro needs to collect the atom types and prefix operator types into separate lists for the code generation.

**Step 4: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: All tests PASS.

**Step 5: Commit**

```bash
git add recursa-derive/src/parse_derive.rs recursa-derive/tests/parse_pratt.rs
git commit -m "Add IS_TERMINAL and first_patterns to Pratt enum derive"
```

---

## Task 6: Verify Existing Tests and E2E

Run the full workspace test suite and the end-to-end integration test to ensure nothing broke.

**Files:**
- No changes expected — this is a verification task.

**Step 1: Run all tests**

Run: `cargo test --workspace`
Expected: All tests PASS, zero warnings.

**Step 2: Run clippy**

Run: `cargo clippy --workspace`
Expected: No new warnings.

**Step 3: If any tests fail, fix them**

The most likely failures:
- Existing enum tests may now behave differently because the combined peek regex changes dispatch order. The semantics should be the same (longest match, declaration order tiebreaker), but verify.
- The `parse_enum.rs` test for `Statement` should still work because `let` and `return` are unambiguous at the first token level.

**Step 4: Commit if any fixes were needed**

```bash
git commit -m "Fix any test regressions from lookahead changes"
```

---

## Summary

| Task | What it delivers |
|------|-----------------|
| 1 | `IS_TERMINAL` and `first_patterns()` on `Parse` trait + blanket impl |
| 2 | Verification that Scan types get these via blanket impl |
| 3 | Struct derive generates terminal prefix walk |
| 4 | Enum derive generates combined peek regex with multi-token lookahead |
| 5 | Pratt enum generates first_patterns (atoms + prefix only) |
| 6 | Full regression verification |
