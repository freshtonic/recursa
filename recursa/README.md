# recursa

`recursa` is the user-facing facade for the Recursa AST-first parser generator.
It re-exports the grammar authoring macros and the public runtime contracts used
by generated grammars.

With Recursa, semantic Rust types are the grammar's center. Fixed tokens remain
attributes when their text has no semantic meaning, and containers such as
`Option`, `Vec`, and `Vec1` state cardinality in the AST itself.

## Setup

```toml
[dependencies]
recursa = "0.1"

[build-dependencies]
recursa-codegen = "0.1"
```

The build script calls the generator's public entry point:

```rust
fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/grammar.rs")?;
    Ok(())
}
```

A grammar module combines `grammar!`, `tokens!`, and `#[derive(Node)]`:

```rust,ignore
recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

#[derive(recursa::Node)]
#[tok(LBRACKET, this, RBRACKET)]
pub struct Array<'input> {
    #[sep(COMMA)]
    pub values: Option<recursa::Vec1<Value<'input>>>,
}
```

This is an excerpt from the
[compiled JSON grammar](https://github.com/freshtonic/recursa/blob/main/examples/json/src/grammar.rs).
Its companion
[public parse entry point](https://github.com/freshtonic/recursa/blob/main/examples/json/src/lib.rs)
shows grammar-local `lex`, `input`, and `parse_spanned` usage without depending
on generated file layout.

## Features

No features are enabled by default.

- `arbitrary` supplies facade support for configured generated `Arbitrary`
  implementations.
- `diagrams` supplies the matching facade capability for generated diagrams.
- `explorer` exposes grammar-erased Explorer adapter and document contracts.
- `serde` supplies facade support for configured generated Serde
  implementations.

Build-time diagram or Explorer generation also enables the same feature on
`recursa-codegen`.

## Public boundary

Use the facade's generated grammar APIs plus its public spans, parse errors,
parsed values and views, traversal traits, Pretty interfaces, and `Vec1` type.
The hidden `__private` module is exclusively a matching-version contract for
generated code.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[API reference](https://docs.rs/recursa/latest/recursa/) and the
[compiled examples](https://github.com/freshtonic/recursa/tree/main/examples).
