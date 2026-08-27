# Keep Explorer recovery separate from authored AST values

Status: accepted

## Context

Authored Nodes make required fields, selected enum variants, and non-empty
sequences part of the Rust type. Representing failed source by adding error
variants, making required fields optional, or constructing an empty
`recursa::Vec1` would weaken those guarantees for every user in order to serve
one development interface.

## Decision

Strict parsing remains the only source-to-authored-AST construction path and
continues to produce either a complete typed value or diagnostics. Explorer
recovery runs only after strict failure and produces a separate, eager,
source-borrowing recovery projection. An Exploration contains either a complete
Explorer document backed by an authored value or a recovery document; recovery
objects never contain, construct, or convert into authored AST values.

Recovery decisions reuse the grammar's frozen dispatch, FIRST, FOLLOW,
container, failure, and Pratt products. The browser does not infer grammar
structure from tokens, and recovery never chooses an ambiguous branch merely
to display a tree. The complete representation, synchronization, progress, and
diagnostic contract is defined by the
[Explorer recovery specification](../explorer-recovery.md).

## Considered options

- Generated error variants and optional required fields were rejected because
  they make illegal authored states representable outside the Explorer.
- A partial Explorer document backed by uninitialized or fabricated authored
  values was rejected because it violates the document's complete-value and
  safety contracts.
- Browser-only token heuristics were rejected because they duplicate grammar
  semantics and cannot provide a parser-owned progress guarantee.

## Consequences

Valid parsing, Pretty rendering, and deterministic samples keep their existing
typed contracts. Invalid Explorer input that reaches recovery pays for a strict
attempt followed by an allocated recovery projection, and Pretty remains
unavailable for that projection. The private loopback protocol can distinguish
complete, recovered, and fatal results without exposing recovery as a public
parsing mode.
