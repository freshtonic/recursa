# recursa-core

`recursa-core` is the grammar-erased runtime beneath the `recursa` facade. It
provides checked spans, parse errors, parsed occurrence provenance, node views,
AST traversal contracts, structured Pretty output and source maps, and the
non-empty `Vec1<T>` container.

Most applications should depend on
[`recursa`](https://docs.rs/recursa/latest/recursa/) instead. The facade
re-exports the supported runtime types and keeps generated grammars aligned
with the matching runtime and macros.

## Public boundary

The public runtime surface includes:

- `Span`, `ParseError`, and `ParseErrorKind`;
- `Parsed<T>`, `Node`, `NodeView`, element views, and repeated views;
- `Visit`/`VisitMut` and visitor contracts;
- `Pretty`, `PrettyConfig`, `PrettyContext`, `PrettySink`, and source maps;
- `HasSpan` and `Vec1<T>`.

Grammar-local token, lexer, input, and parse entry types are generated for each
authored grammar. The hidden `__private` module in this crate exists only for
matching-version generated code and is not a general-purpose lexer or parser
API.

## Runtime parse diagnostics

`ParseError::code` returns an append-only machine-readable code, while
`ParseError::anchor` returns its reported primary source range. Ordinary parse
failures use the complete failure span as their anchor. A lexical parse failure
preserves the generated lexical diagnostic's `RCA400x` code and opener or
failure anchor; a lexical error record without that metadata falls back to
`RCA4000` and the record span. `InvalidTokenSpan` is the exception to source
validity: its reported span and anchor can describe a range that is invalid for
the source.

| Code | `ParseErrorKind` |
| --- | --- |
| `RCA4100` | `UnexpectedToken` |
| `RCA4101` | `UnexpectedEof` |
| `RCA4102` | `PrattAmbiguity` |
| `RCA4103` | `InvalidTokenSpan` |
| `RCA4104` | `GrammarMismatch` |

Published runtime codes are never renumbered or reused for another failure
class. New classifications receive a new code.

## Features

No features are enabled by default.

- `arbitrary` supports configured generated `Arbitrary` implementations.
- `serde` supports configured serialization implementations for runtime-backed
  values.
- `explorer` exposes grammar-erased schema, lexing, document, and occurrence
  contracts. It does not include a server or browser UI.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[API reference](https://docs.rs/recursa-core/latest/recursa_core/) and the
[workspace package map](https://github.com/freshtonic/recursa#workspace-packages).
