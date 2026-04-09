# Container Types Design

Blanket `Parse` impls for `Option<T>`, `Box<T>`, and a type-level configurable `Seq<T, S>` for separated lists.

## Option\<T: Parse\>

Blanket impl. Peek-based: if `T::peek` succeeds, parse `T` and return `Some`. Otherwise return `None` without consuming input. If peek succeeds but parse fails, the error propagates (it's a genuine parse error, not "the optional wasn't present").

```rust
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

## Box\<T: Parse\>

Blanket impl. Delegates everything to `T`. Needed for recursive types like `Box<Expr>` in Pratt parsing.

```rust
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

## Seq\<T, S, Trailing, Empty\>

A type-level configurable separated list. Parameterised by element type, separator type, trailing separator policy, and emptiness.

### Type Parameters

| Parameter | Options | Default |
|-----------|---------|---------|
| `T` | Element type (`Parse`) | required |
| `S` | Separator type (`Parse`) | required |
| `Trailing` | `NoTrailing`, `RequiredTrailing`, `OptionalTrailing` | `NoTrailing` |
| `Empty` | `AllowEmpty`, `NonEmpty` | `AllowEmpty` |

### Marker Types

```rust
pub struct NoTrailing;
pub struct RequiredTrailing;
pub struct OptionalTrailing;

pub struct AllowEmpty;
pub struct NonEmpty;
```

### Data Structure

```rust
pub struct Seq<T, S, Trailing = NoTrailing, Empty = AllowEmpty> {
    pairs: Vec<(T, Option<S>)>,
    _phantom: PhantomData<(Trailing, Empty)>,
}
```

Stores element-separator pairs. The last element's separator is `None` (for `NoTrailing`) or `Some` (for `RequiredTrailing`) or either (for `OptionalTrailing`).

### Deref

- `Seq<T, S, _, AllowEmpty>` derefs to `Vec<T>` (extracts elements, discards separators)
- `Seq<T, S, _, NonEmpty>` derefs to `Vec1<T>` (from the `vec1` crate)

The full pairs are accessible via methods for when separator spans or values are needed.

### Usage Examples

```rust
// Zero-or-more, no trailing comma (e.g., function arguments)
args: Seq<Expr<'input>, Comma>,

// Zero-or-more, optional trailing comma (e.g., array literal)
elements: Seq<Expr<'input>, Comma, OptionalTrailing>,

// One-or-more, required trailing semicolon (e.g., statement block)
stmts: Seq<Stmt<'input>, Semi, RequiredTrailing, NonEmpty>,

// One-or-more, no trailing (e.g., pipe chain)
stages: Seq<Stage<'input>, Pipe, NoTrailing, NonEmpty>,
```

### Parse Logic

**NoTrailing:**
1. Peek `T`. If false and `AllowEmpty`, return empty. If false and `NonEmpty`, error.
2. Parse `T`.
3. Peek `S`. If false, done (last element has `separator: None`).
4. Parse `S`, store pair `(T, Some(S))`, goto 2.

**RequiredTrailing:**
1. Peek `T`. If false and `AllowEmpty`, return empty. If false and `NonEmpty`, error.
2. Parse `T`.
3. Parse `S` (error if missing — trailing is required).
4. Store pair `(T, Some(S))`.
5. Peek `T`. If false, done. Otherwise goto 2.

**OptionalTrailing:**
1. Peek `T`. If false and `AllowEmpty`, return empty. If false and `NonEmpty`, error.
2. Parse `T`.
3. Peek `S`. If false, done (last element has `separator: None`).
4. Parse `S`.
5. Peek `T`. If false, this was a trailing separator — store pair `(T, Some(S))`, done.
6. Otherwise store pair `(T, Some(S))`, goto 2.

### Parse Trait Implementation

```rust
const IS_TERMINAL: bool = false;
```

`first_pattern()` delegates to `T::first_pattern()`.

`peek()`:
- `AllowEmpty` — always returns true (empty sequence is valid)
- `NonEmpty` — delegates to `T::peek`

Note: `AllowEmpty` sequences that always peek-true should only appear in positions where the surrounding grammar already knows it's the right production (e.g., inside brackets). They should not be used as enum variants.

### Dependencies

- `vec1` crate for `Vec1<T>` (used by `NonEmpty` deref)
