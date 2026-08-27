# Recursa JSON example

This compiled example declares JSON directly as Rust semantic types. Fixed
delimiters, separators, and keywords are syntax attributes rather than dead
fields. `Vec<Member>` represents any number of object members, while
`Option<Vec1<Value>>` makes an array's empty-or-nonempty states explicit.

The supported setup is visible in three files:

- [`build.rs`](build.rs) calls the public `recursa_codegen::generate` entry point.
- [`src/grammar.rs`](src/grammar.rs) declares tokens and AST Nodes.
- [`src/lib.rs`](src/lib.rs) lexes and parses only through generated public APIs.

From a clean checkout, run:

```console
cargo test -p recursa-example-json
cargo run -p recursa-example-json -- '{"answer":[true,null,42]}'
```

The behavioral tests cover objects, arrays, literals, `Vec`, `Option`, `Vec1`,
lexical failure, syntax failure, and trailing input.
