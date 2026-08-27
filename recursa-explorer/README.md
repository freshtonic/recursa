# recursa-explorer

`recursa-explorer` launches a small local server linked directly to a generated
grammar. A development binary can start it with:

```rust,no_run
fn main() -> Result<(), recursa_explorer::RunError> {
    recursa_explorer::run(grammar::Value::explorer())
}
```

The launcher binds only to `127.0.0.1` on an operating-system-assigned port,
becomes ready before opening the system browser, and exits after the browser
requests orderly shutdown. Its HTML, CSS, and JavaScript are compiled into the
crate; this release intentionally contains only a readiness page, not the
interactive UI planned separately.

The browser uses an intentionally private, same-origin JSON protocol to read
schema, lex, parse, Pretty, and deterministic sample projections. Strict parse
failure returns diagnostics and no partial tree. The server rejects nonlocal
clients, unexpected Host or Origin headers, oversized bodies, unsupported
methods and media types, and sends a restrictive content security policy. It
is not a public remote service or stable transport protocol.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[workspace package map](https://github.com/freshtonic/recursa#workspace-packages).
