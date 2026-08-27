# recursa-macros

`recursa-macros` contains Recursa's matching-version structural loaders. The
`recursa` facade re-exports them as `grammar!`, `tokens!`, and
`#[derive(recursa::Node)]`, together with the total-visitor derives.

Users should depend on [`recursa`](https://docs.rs/recursa/latest/recursa/), not
on this package directly. `recursa-codegen` interprets grammar declarations at
build time; these macros then load that exact generation and verify its release,
protocol, capabilities, selected root, and Node declarations during Rust
compilation. They are not an independent parser-generator frontend.

The `arbitrary`, `diagrams`, `explorer`, and `serde` features mirror facade
capabilities for the generated-code compatibility check. Select those features
through `recursa` and, where generation is involved, `recursa-codegen`.

Generated output layout and the loader protocol are private matching-version
details. Supported application setup is demonstrated by the compiled
[JSON](https://github.com/freshtonic/recursa/tree/main/examples/json) and
[Pratt-expression](https://github.com/freshtonic/recursa/tree/main/examples/pratt-expressions)
examples.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[macro API reference](https://docs.rs/recursa-macros/latest/recursa_macros/) and
the [workspace package map](https://github.com/freshtonic/recursa#workspace-packages).
