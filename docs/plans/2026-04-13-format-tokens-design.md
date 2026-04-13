# FormatTokens Trait and Derive Macro

## Context

The current SQL formatter uses the Visitor pattern to walk AST nodes and emit pretty-printer tokens. This requires manually implementing `Visitor<N>` for every node type. A better approach: each AST type derives `FormatTokens` which mechanically emits tokens in field order. The type of each field determines what token to emit.

The derive produces a flat `Token::String` stream by default. Formatting structure (groups, breaks, indentation) is layered on top via `#[format_token(...)]` attributes on structs and fields — only where the defaults are insufficient.

## Design

### New traits in `recursa-core/src/fmt.rs`

```rust
pub trait TokenText {
    const TEXT: &'static str;
}

pub trait FormatTokens {
    fn format_tokens(&self, tokens: &mut Vec<Token>);
}
```

### Type-to-token mapping (no attributes needed)

| Field type | Emits |
|---|---|
| `PhantomData<T: TokenText>` | `Token::String(T::TEXT)` |
| `String` | `Token::String(self.clone())` |
| `Option<T>` | delegate if Some |
| `Box<T>` | delegate |
| `Seq<T, S>` | iterate pairs, delegate |
| `Surrounded<O, I, C>` | emit Open text, delegate Inner, emit Close text |
| `()` | no-op |

Literal types (`Ident(String)`, `StringLit(String)`, etc.) are tuple structs wrapping `String`. The derive calls `self.0.format_tokens(tokens)` which delegates to `String`'s impl.

### How keywords/punctuation get their TEXT

- **Keywords**: `strip_word_boundary` const fn strips `\b` from the pattern. No macro API change needed. `r"SELECT\b"` → `"SELECT"`.
- **Punctuation**: Change macro signature to accept display text as second arg. `LParen => r"\(", "("`. Regex patterns can't be reliably unescaped at compile time.

### Format attributes

Attributes control structural token emission. Most types need none — only clause-level structures need markup.

#### `#[format_token(group(consistent))]` or `#[format_token(group(inconsistent))]`

- **On a struct/enum**: wraps the entire type's tokens in `Begin(kind)` ... `End`
- **On a field**: wraps that field's tokens in `Begin(kind)` ... `End`

#### `#[format_token(break(flat = "...", broken = "..."))]`

- **On a field**: emits a `Token::Break { flat, broken }` *before* the field's tokens

#### `#[format_token(indent)]`

- **On a field**: emits `Token::Indent` before and `Token::Dedent` after the field's tokens

#### Example

```rust
#[derive(FormatTokens)]
#[format_token(group(consistent))]
pub struct SelectStmt {
    pub _select: PhantomData<keyword::Select>,
    #[format_token(indent, break(flat = " ", broken = "\n"))]
    pub items: Seq<SelectItem, punct::Comma>,
    pub from_clause: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    // ...
}
```

Produces: `Begin(Consistent)`, `"SELECT"`, `Indent`, `Break{" ", "\n"}`, *items tokens*, `Dedent`, *from tokens*, *where tokens*, `End`

## Implementation Steps

### 1. `recursa-core/src/fmt.rs` — traits + blanket impls

Add `TokenText`, `FormatTokens`, `strip_word_boundary` const fn. Blanket impls for `PhantomData<T: TokenText>`, `String`, `Option<T>`, `Box<T>`, `Vec<T>`, `()`.

### 2. `recursa-core/src/macros.rs` — update all three macros

- `keywords!`: add `TokenText` impl (using `strip_word_boundary`) and `FormatTokens` impl per keyword + on the Keyword enum
- `punctuation!`: change signature to `$name => $pattern, $display`. Add `TokenText` + `FormatTokens` impls per punct + on the Punctuation enum
- `literals!`: add `FormatTokens` impl per literal + on the Literal enum

### 3. `recursa-core/src/seq.rs` — FormatTokens for Seq

Iterate pairs, call format_tokens on each element and separator.

### 4. `recursa-core/src/surrounded.rs` — FormatTokens for Surrounded

Emit `Open::TEXT`, delegate to inner, emit `Close::TEXT`.

### 5. `recursa-core/src/lib.rs` — re-export

Export `FormatTokens` and `TokenText`.

### 6. `recursa-derive/src/format_tokens_derive.rs` — new derive macro

**Default behaviour (no attributes):**
- **Structs**: call `self.field.format_tokens(tokens)` for each field in order
- **Enums**: match on variant, delegate to inner
- **Tuple structs**: call `self.0.format_tokens(tokens)` etc.

**Attribute handling:**
- Parse `#[format_token(...)]` on struct/enum and on each field
- Struct-level `group(kind)`: emit `Begin(kind)` before fields, `End` after
- Field-level `group(kind)`: wrap field emission in `Begin(kind)` ... `End`
- Field-level `break(flat, broken)`: emit `Break { flat, broken }` before field
- Field-level `indent`: emit `Indent` before, `Dedent` after field

### 7. `recursa-derive/src/lib.rs` — register macro

### 8. `pg-sql/src/tokens.rs` — update call sites

- Update `punctuation!` calls with display text second arg
- `Ident`, `AliasName`, `RestOfLine` (defined outside macros): the derive handles them automatically since they're tuple structs wrapping `String`
- `DollarNum` is unused in AST — not a blocker

### 9. `pg-sql/src/ast/*.rs` — add derive + format attributes

Add `#[derive(FormatTokens)]` to all AST types. Add `#[format_token(...)]` attributes to clause-level structures (SelectStmt, FromClause, WhereClause, etc.) for proper formatting. Most leaf types need no attributes.

## Verification

1. `cargo test --all --all-targets` — no regressions
2. Write a test that parses `"SELECT 1 AS one;"` and calls `format_tokens`, asserting the token stream contains `["SELECT", "1", "AS", "one", ";"]` (flat, no formatting attributes)
3. Write a test with formatting attributes on SelectStmt verifying `Begin`, `Indent`, `Break`, `Dedent`, `End` tokens are emitted at correct positions
4. Verify the pg-sql binary still works: `echo "select 1 as one;" | cargo run -p pg-sql -- fmt -`
