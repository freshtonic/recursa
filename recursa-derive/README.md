# recursa-derive

Derive macros for the [recursa](https://github.com/freshtonic/recursa) parser framework.

This crate provides:

- **`#[derive(Scan)]`** -- derive `Scan` and `Parse` for token types (unit structs, tuple structs, enums)
- **`#[derive(Parse)]`** -- derive `Parse` for AST node types (structs as sequences, enums as choices, Pratt parsing for expressions)
- **`#[derive(Visit)]`** -- derive `Visit` for AST traversal via the visitor pattern

Most users should depend on the [`recursa`](https://crates.io/crates/recursa) facade crate instead of this crate directly.

## License

MIT
