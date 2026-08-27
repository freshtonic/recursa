# recursa-codegen

`recursa-codegen` is Recursa's build-time generator. It discovers an authored
Rust grammar module, resolves and lowers its grammar model, performs lexical and
semantic analysis, reports structured diagnostics, and publishes one complete
generation for the matching facade and structural loaders.

Applications normally use its public `generate` function from `build.rs`:

```rust
fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/grammar.rs")?;
    Ok(())
}
```

The path is relative to the package manifest. The generator owns Cargo
invalidation for the selected Rust module tree. Consumers should not inspect
`OUT_DIR` or include generated files themselves; `recursa::grammar!` and
`#[derive(recursa::Node)]` load the matching generation.

See the complete build scripts for the compiled
[JSON](https://github.com/freshtonic/recursa/blob/main/examples/json/build.rs)
and
[Pratt-expression](https://github.com/freshtonic/recursa/blob/main/examples/pratt-expressions/build.rs)
examples.

## Analysis API

The crate also exposes source-independent grammar IR, FIRST/FOLLOW and
lookahead analysis, decision products, provenance, and structured diagnostic
types. These are advanced analysis interfaces; authored-source discovery and
the generation protocol remain generator-owned implementation details.

## Features

No features are enabled by default.

- `diagrams` uses `railroad` to generate semantic railroad diagrams in aggregate
  grammar Rustdoc and on each generated Node and content-token type. Request it
  with `diagrams` in `recursa::grammar!`; content tokens may use
  `#[lex(pattern = r"...", diagram = "human label")]`, otherwise the regex is
  the deterministic label. Fixed-token terminals are syntax-only.
- `explorer` generates grammar-erased Explorer adapters for eligible roots.

Enable the matching capability on the `recursa` facade when a generated grammar
uses it.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[API reference](https://docs.rs/recursa-codegen/latest/recursa_codegen/) and
the [facade setup](https://github.com/freshtonic/recursa/tree/main/recursa).
