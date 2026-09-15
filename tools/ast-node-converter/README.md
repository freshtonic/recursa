# AST Node converter

This standalone Rust tool performs the mechanical migration from derived `Node` structs and enums
to representation-independent `ast_node!` declarations. It parses Rust with `syn`, edits source
spans, reports unsupported sites, and is idempotent.

Run its focused tests with:

```sh
cargo test --offline --manifest-path tools/ast-node-converter/Cargo.toml
```

Convert one or more Rust files or directory trees in place with:

```sh
cargo run --offline --quiet \
  --manifest-path tools/ast-node-converter/Cargo.toml -- PATH...
```

The pg-sql migration used one invocation:

```sh
cargo run --offline --quiet \
  --manifest-path tools/ast-node-converter/Cargo.toml -- ../pg-sql/src
```

It converted 1,478 declarations and 752 concrete storage constructors, removed 3,837
declaration-side lifetime occurrences, and reported zero unsupported sites. A second invocation
reported zero changes.

Use `--stdin` for one complete Rust source on standard input. Converted source is the only standard
output; counts and unsupported-site diagnostics go to standard error. Use `--rust-strings` with
host Rust files to convert complete Rust sources embedded in string literals. That mode also
normalizes already-migrated multiline sources to readable raw strings and formats declaration
bodies. It preserves `format!` placeholders and doubled braces.

Use `--markdown` with explicit Markdown paths to format Rust fences containing `ast_node!` without
changing other fenced examples.

Add `--format` to run rustfmt over declaration bodies. The formatter temporarily presents each
`ast_node!` body as an ordinary uniquely named module, formats the complete file once, and restores
the original macro path. This lets rustfmt format marker types and declarations without interpreting
or expanding the macro.

The converter leaves unsupported sites unchanged where possible, reports each one, and exits with
a nonzero status. It specifically reports comments inside a field type that requires rewriting and
combined legacy `node` attributes whose intent cannot be split mechanically.
