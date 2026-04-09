# recursa-core

Core traits and types for the [recursa](https://github.com/freshtonic/recursa) parser framework.

This crate provides:

- **`Parse`** -- recursive descent parser trait, derived for structs (sequence) and enums (choice)
- **`Scan`** -- leaf-level token matching via regex
- **`ParseRules`** -- grammar configuration for whitespace/comment handling
- **`Input`** -- cursor over source text with fork/commit for backtracking
- **`ParseError`** -- rich diagnostics via `miette` with source spans and context
- **`Seq`** -- type-level configurable separated lists
- **`Visit`** / **`Visitor`** -- AST traversal via the visitor pattern
- **`NodeKey`** -- type-erased node handles for AST analysis
- **`keywords!`**, **`punctuation!`**, **`literals!`** -- bulk token declaration macros

Most users should depend on the [`recursa`](https://crates.io/crates/recursa) facade crate instead of this crate directly.

## License

MIT
