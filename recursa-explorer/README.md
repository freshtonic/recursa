# recursa-explorer

`recursa-explorer` is reserved for Recursa's future local Explorer application.
It currently provides no launcher, HTTP server, or browser UI.

The implemented Explorer capability is a different layer: enabling the
`explorer` features on `recursa` and `recursa-codegen` can generate
grammar-erased adapters. The public `recursa::explorer` contracts expose static
schema, lexed tokens and errors, parsed or synthetic documents, occurrence
trees, spans, configured Pretty output, and deterministic samples when the
grammar supports them. Those contracts do not themselves display an interface
or start a process.

Applications should not depend on this reserved package yet. If working with
the adapter layer, use the
[`recursa::explorer` Rustdoc](https://docs.rs/recursa/latest/recursa/explorer/)
and keep the authored AST in the grammar-linked process. The planned local UI
is intentionally not a public remote service or stable transport protocol.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[workspace package map](https://github.com/freshtonic/recursa#workspace-packages).
