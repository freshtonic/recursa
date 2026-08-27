# Recursa Pratt-expression example

This compiled example declares atoms plus prefix, postfix, and infix operators
in one Rust enum. Pratt binding powers make precedence and associativity part
of the grammar while the resulting values remain ordinary semantic Rust types.

The supported setup is visible in three files:

- [`build.rs`](build.rs) calls the public `recursa_codegen::generate` entry point.
- [`src/grammar.rs`](src/grammar.rs) declares tokens, atoms, and Pratt variants.
- [`src/lib.rs`](src/lib.rs) lexes and parses only through generated public APIs.

From a clean checkout, run:

```console
cargo test -p recursa-example-pratt-expressions
cargo run -p recursa-example-pratt-expressions -- '-a! + 2 * 3 ^ 4'
```

The behavioral tests cover atom parsing, prefix and postfix binding, grouping,
operator precedence, left and right associativity, and invalid input.
