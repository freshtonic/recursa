# recursa-test

`recursa-test` is a reserved workspace package for future reusable Recursa
testing support. It currently exposes no public harness or application API.

Grammar authors should use ordinary Rust tests against their generated public
lexing and parsing entry points. The compiled
[JSON behavior tests](https://github.com/freshtonic/recursa/blob/main/examples/json/tests/behavior.rs)
and
[expression behavior tests](https://github.com/freshtonic/recursa/blob/main/examples/expressions/tests/behavior.rs)
show this approach. Recursa's own generator validation remains in workspace
tests rather than being promised as an API from this package.

Do not add `recursa-test` as a dependency expecting corpus, differential, or
round-trip helpers today. Its eventual public scope will be documented when
such support exists.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[workspace package map](https://github.com/freshtonic/recursa#workspace-packages).
