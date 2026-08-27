# Run the Explorer UI from a local grammar-linked server

Status: accepted

## Context

Explorer adapters depend on authored AST types that a standalone executable
cannot discover safely. A browser UI also needs an interaction boundary, but a
public remote service or stable transport protocol would enlarge Recursa's
security and compatibility surface.

## Decision

Launch the Explorer UI from a small development binary linked to the user's
grammar through the experimental call shape
`recursa_explorer::run(MyRoot::explorer())`. The Rust server binds only to
`127.0.0.1` on an OS-assigned port, becomes ready before opening the browser,
serves bundled HTML/CSS/vanilla JavaScript, accepts same-origin requests only,
and supports normal shutdown. It exposes an intentionally private JSON
protocol and offers no remote bind, permissive CORS, Node toolchain, WASM
build, or plugin system.

Strict parse failure returns structured diagnostics without a partial Explorer
document. Partial-tree recovery remains a separate design problem so it cannot
make an illegal authored AST state representable.

## Consequences

The grammar and AST remain in-process, the UI works without external assets,
and matching Recursa versions may evolve the browser protocol freely. Users
must add a development binary to launch the Explorer, and remote/shared
playgrounds require a future architectural decision rather than an accidental
configuration switch.
