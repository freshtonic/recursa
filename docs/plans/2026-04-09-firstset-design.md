# FirstSet / Lookahead Design

Adds multi-token lookahead to enum `Parse` dispatch using combined peek regexes built from each variant's terminal prefix.

## Problem

Currently, enum `Parse` dispatch tries each variant's `peek` sequentially. If two variants share a first token (e.g., `pub fn` vs `pub struct`), the first declared variant wins — which is wrong. Multi-token lookahead is needed to disambiguate.

## Approach

Runtime construction via trait methods. No complex type-level encoding. Combined peek regexes are cached in `OnceLock` for zero-cost after first use.

## Trait Changes

Add two items to `Parse`:

```rust
pub trait Parse<'input>: Sized {
    type Rules: ParseRules;

    /// Whether this type is a leaf token (Scan type) or a composite production.
    const IS_TERMINAL: bool;

    /// The terminal prefix patterns for this production.
    /// Scan types return their single pattern. Structs return
    /// consecutive terminal field patterns from the start.
    fn first_patterns() -> &'static [&'static str];

    fn peek(input: &Input<'input, Self::Rules>) -> bool;
    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError>;
}
```

## Behaviour by Type

### Scan (via blanket impl)

```rust
const IS_TERMINAL: bool = true;

fn first_patterns() -> &'static [&'static str] {
    &[Self::PATTERN]
}
```

No `OnceLock` needed — returns a const slice.

### Struct

```rust
const IS_TERMINAL: bool = false;

fn first_patterns() -> &'static [&'static str] {
    static PATTERNS: OnceLock<Vec<&'static str>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let mut patterns = Vec::new();
        // For each field from the start, while IS_TERMINAL is true:
        patterns.extend(<Field1Type as Parse>::first_patterns());
        // if <Field1Type as Parse>::IS_TERMINAL:
        patterns.extend(<Field2Type as Parse>::first_patterns());
        // if <Field2Type as Parse>::IS_TERMINAL:
        patterns.extend(<Field3Type as Parse>::first_patterns());
        // ... stop at first non-terminal field
        patterns
    })
}
```

Walks consecutive terminal fields, collecting their patterns into a flat list representing the longest terminal prefix of the struct.

### Enum (non-Pratt)

```rust
const IS_TERMINAL: bool = false;
```

`first_patterns()` collects each variant's inner type's `first_patterns()`. The more important change is to `peek` and `parse`.

**Combined peek regex construction:**

For each variant:
1. Call `<InnerType as Parse>::first_patterns()` to get the terminal prefix (e.g., `["pub", "fn"]`)
2. Join patterns with the `IGNORE` regex between them: `pub(?:\s+)fn`
3. Wrap in a named capture group: `(?P<_0>pub(?:\s+)fn)`

Join all variant prefixes with `|`, anchor with `\A`, cache in `OnceLock<Regex>`.

Example for an enum with `FnDecl` (starts `pub fn`) and `StructDecl` (starts `pub struct`):
```
\A(?:(?P<_0>pub(?:\s+)fn)|(?P<_1>pub(?:\s+)struct))
```

**Peek:** run combined regex against `input.remaining()`. Return true if any group matches.

**Parse:** run combined regex, identify which named group matched (longest match, declaration order tiebreaker), parse only that variant. No need to try others.

### Enum (Pratt)

```rust
const IS_TERMINAL: bool = false;
```

`first_patterns()` returns patterns from atom and prefix variants only (not infix, which are checked in the Pratt infix loop). Keeps existing sequential peek — atoms and prefix operators are single tokens, so the combined regex provides less benefit.

## Matching Semantics

Same as `Scan` enums:
1. Longest match wins (maximal munch)
2. Declaration order as tiebreaker for equal-length matches

## Future Work

- Compile-time ambiguity detection: check for overlapping variant prefixes at runtime during `OnceLock` init, emit warnings or errors
- The original `type FirstSet` associated type from the design doc is replaced by this simpler runtime approach
