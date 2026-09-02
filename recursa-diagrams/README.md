# recursa-diagrams

`recursa-diagrams` holds the semantic railroad-diagram plan that
`recursa-codegen` derives from a frozen Generation, the compact text codec that
publishes that plan next to the generated source, and the deterministic
Rustdoc-safe SVG renderer.

The build-time generator only plans and publishes diagram topology. The
`recursa` facade renders SVG from the published plan while `cargo doc` expands
generated documentation attributes, so ordinary builds never carry diagram
markup. Rendered diagrams are also written next to the plan as standalone SVG
files.

Users should depend on [`recursa`](https://docs.rs/recursa/latest/recursa/) and
[`recursa-codegen`](https://docs.rs/recursa-codegen/latest/recursa_codegen/),
not on this package directly. The plan layout is a private matching-version
detail.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[workspace package map](https://github.com/freshtonic/recursa#workspace-packages).
