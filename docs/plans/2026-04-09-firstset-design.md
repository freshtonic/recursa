# FirstSet / Lookahead Design

Adds multi-token lookahead to enum `Parse` dispatch using combined peek regexes built from each variant's terminal prefix.

## Problem

Currently, enum `Parse` dispatch tries each variant's `peek` sequentially. If two variants share a first token (e.g., `pub fn` vs `pub struct`), the first declared variant wins — which is wrong. Multi-token lookahead is needed to disambiguate.

## Approach

Runtime construction via trait methods. No complex type-level encoding. Combined peek regexes are cached in `OnceLock` for zero-cost after first use.

Each `Parse` type exposes a `first_pattern()` method returning a single `&'static str` — a self-contained regex fragment representing the terminal prefix of that production. The fragment composes correctly when embedded in larger patterns because enums wrap their variant patterns in groups to preserve alternation boundaries.

## Trait Changes

Add two items to `Parse`:

```rust
pub trait Parse<'input>: Sized {
    type Rules: ParseRules;

    /// Whether this type is a leaf token (Scan type) or a composite production.
    const IS_TERMINAL: bool;

    /// A regex fragment representing the terminal prefix of this production.
    ///
    /// For Scan types: the token's pattern (e.g., `"let"`).
    /// For structs: consecutive terminal field patterns joined with IGNORE
    ///   (e.g., `"pub(?:\\s+)fn"`).
    /// For enums: an alternation of variant patterns wrapped in groups
    ///   (e.g., `"(pub(?:\\s+)fn)|(pub(?:\\s+)struct)"`).
    ///
    /// The returned string is a regex fragment, not a complete regex —
    /// it has no `\A` anchor. Callers are responsible for anchoring.
    fn first_pattern() -> &'static str;

    fn peek(input: &Input<'input, Self::Rules>) -> bool;
    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError>;
}
```

## Behaviour by Type

### Scan (via blanket impl)

```rust
const IS_TERMINAL: bool = true;

fn first_pattern() -> &'static str {
    Self::PATTERN  // e.g., "let", "[0-9]+"
}
```

No `OnceLock` needed — `PATTERN` is already `&'static str`.

### Struct

```rust
const IS_TERMINAL: bool = false;

fn first_pattern() -> &'static str {
    static PATTERN: OnceLock<String> = OnceLock::new();
    PATTERN.get_or_init(|| {
        let ignore = <Rules>::IGNORE;
        let sep = if ignore.is_empty() {
            String::new()
        } else {
            format!("(?:{})?", ignore)
        };

        let mut parts = Vec::new();

        // Walk consecutive terminal fields:
        parts.push(<Field1Type as Parse>::first_pattern().to_string());
        if <Field1Type as Parse>::IS_TERMINAL {
            parts.push(<Field2Type as Parse>::first_pattern().to_string());
            if <Field2Type as Parse>::IS_TERMINAL {
                // ... continue
            }
        }

        parts.join(&sep)
    })
}
```

Walks consecutive terminal fields from the start, joining their patterns with `IGNORE` between them. The result is a regex fragment like `"pub(?:\s+)?fn(?:\s+)?[a-zA-Z_][a-zA-Z0-9_]*"`.

Stops at the first non-terminal field, but includes that field's `first_pattern()` (which may itself be an alternation) because the non-terminal's prefix is still part of this struct's lookahead.

### Enum (non-Pratt)

```rust
const IS_TERMINAL: bool = false;

fn first_pattern() -> &'static str {
    static PATTERN: OnceLock<String> = OnceLock::new();
    PATTERN.get_or_init(|| {
        let mut parts = Vec::new();
        parts.push(format!("({})", <Variant1Type as Parse>::first_pattern()));
        parts.push(format!("({})", <Variant2Type as Parse>::first_pattern()));
        parts.join("|")
    })
}
```

Each variant's pattern is wrapped in a group `(...)` to preserve alternation boundaries when nested. The result is a regex fragment like `"(pub(?:\s+)?fn)|(pub(?:\s+)?struct)"`.

**Why groups are necessary:**

Without groups, nesting produces incorrect regexes. If variant 1's pattern is `"a|b"` and variant 2's is `"c"`, a naive join produces `"a|b|c"` — which loses the variant boundaries. With groups: `"(a|b)|(c)"` — correct.

**Combined peek regex for dispatch:**

The enum's own `peek` and `parse` use a separate combined regex with *named* capture groups for variant identification:

```
\A(?:(?P<_0>pub(?:\s+)?fn)|(?P<_1>pub(?:\s+)?struct))
```

Built from each variant's `first_pattern()` wrapped in `(?P<_N>...)`.

**Peek:** run combined regex against `input.remaining()`. Return true if any group matches.

**Parse:** run combined regex, identify which named group matched (longest match, declaration order tiebreaker), parse only that variant.

### Enum (Pratt)

```rust
const IS_TERMINAL: bool = false;
```

`first_pattern()` returns an alternation of atom and prefix operator patterns only (not infix, which are checked in the Pratt infix loop). Keeps existing sequential peek.

## Composability Example

Given:

```rust
struct FnDecl { pub_kw: PubKw, fn_kw: FnKw, name: Ident, ... }
struct StructDecl { pub_kw: PubKw, struct_kw: StructKw, name: Ident, ... }
enum Declaration { Fn(FnDecl), Struct(StructDecl) }
struct Module { decl: Declaration, semi: Semi }
```

The `first_pattern()` chain:

- `PubKw`: `"pub"`
- `FnKw`: `"fn"`
- `FnDecl`: `"pub(?:\s+)?fn(?:\s+)?[a-zA-Z_][a-zA-Z0-9_]*(?:\s+)?\\{(?:\s+)?\\}"`
- `StructDecl`: `"pub(?:\s+)?struct(?:\s+)?[a-zA-Z_][a-zA-Z0-9_]*(?:\s+)?\\{(?:\s+)?\\}"`
- `Declaration`: `"(pub(?:\s+)?fn(?:\s+)?...)|(pub(?:\s+)?struct(?:\s+)?...)"`
- `Module`: `Declaration`'s pattern (since `Declaration` is non-terminal, the walk stops after it)

At every level, the regex fragment composes correctly.

## Matching Semantics

Same as `Scan` enums:
1. Longest match wins (maximal munch)
2. Declaration order as tiebreaker for equal-length matches

## Future Work

- Compile-time ambiguity detection: check for overlapping variant prefixes at runtime during `OnceLock` init, emit warnings or errors
- The original `type FirstSet` associated type from the design doc is replaced by this simpler runtime approach
