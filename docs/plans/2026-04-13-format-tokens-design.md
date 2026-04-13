# FormatTokens Trait and Derive Macro

## Context

The current SQL formatter uses the Visitor pattern to walk AST nodes and emit pretty-printer tokens. This requires manually implementing `Visitor<N>` for every node type. A better approach: each AST type derives `FormatTokens` which mechanically emits tokens in field order. The type of each field determines what token to emit — no per-field attributes needed.

Structural tokens (Begin/End, Indent/Dedent, Break) are out of scope — those represent formatting *decisions* that will be layered on top separately.

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
| `literal::Ident(String)` etc. | `Token::String(self.0)` |
| `Option<T>` | delegate if Some |
| `Box<T>` | delegate |
| `Seq<T, S>` | iterate pairs, delegate |
| `Surrounded<O, I, C>` | emit Open text, delegate Inner, emit Close text |
| `()` | no-op |

### How keywords/punctuation get their TEXT

- **Keywords**: `strip_word_boundary` const fn strips `\b` from the pattern. No macro API change needed. `r"SELECT\b"` → `"SELECT"`.
- **Punctuation**: Change macro signature to accept display text as second arg. `LParen => r"\(", "("`. Regex patterns can't be reliably unescaped at compile time.

## Implementation Steps

### 1. `recursa-core/src/fmt.rs` — traits + blanket impls

Add `TokenText`, `FormatTokens`, `strip_word_boundary` const fn. Add blanket impls for `PhantomData<T: TokenText>`, `Option<T>`, `Box<T>`, `Vec<T>`, `()`.

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

- **Structs**: call `self.field.format_tokens(tokens)` for each field (including PhantomData fields — unlike Visit)
- **Enums**: match on variant, delegate to inner
- No attributes needed

### 7. `recursa-derive/src/lib.rs` — register macro

### 8. `pg-sql/src/tokens.rs` — update call sites

- Update `punctuation!` calls with display text second arg
- Add manual `FormatTokens` impls for `Ident`, `AliasName`, `RestOfLine` (defined outside macros)
- `DollarNum` is unused in AST — not a blocker

### 9. `pg-sql/src/ast/*.rs` — add derive

Add `#[derive(FormatTokens)]` alongside Parse and Visit on all AST types. Manual impl for `RawStatement` (emits its text field).

## Verification

1. `cargo test --all --all-targets` — no regressions
2. Write a test that parses `"SELECT 1 AS one;"` and calls `format_tokens`, asserting the token stream contains `["SELECT", "1", "AS", "one", ";"]`
3. Verify the pg-sql binary still works: `echo "select 1 as one;" | cargo run -p pg-sql -- fmt -`
