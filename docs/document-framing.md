# Document framing

Document framing is a generated exact-source adapter for languages whose input
alternates between repeated grammar islands and regions that must remain raw.
It is available when the `explorer` feature is enabled for both the `recursa`
facade and `recursa-codegen`; its recovery path reuses the same generated,
construction-free recovery plan as Explorer.

## Declaration

The grammar root owns one optional framing declaration:

```rust,ignore
recursa::grammar! {
    module = crate::grammar,
    spans = mandatory,
    framing(
        island = Entry,
        boundary = SEMI,
        line = [MetaCommand],
        delimited = [RawBlock],
        payload(after = Entry::CopyFromStdin, terminator = "\\.")
    ),
}
```

- `island` resolves through the ordinary typed import/type-alias pipeline to
  exactly one materialized Node application.
- `boundary` names one fixed token outside the island Node.
- `line` names closed `physical_line` matchers owned atomically.
- `delimited` names closed `same_delimiter` or `nested` matchers owned
  atomically.
- `payload(after = ..., terminator = ...)` is optional. The owner in `after`
  uses that same typed resolution pipeline and its last segment names a
  reachable variant of `island`; `terminator` is nonempty exact single-line
  text.

These identities are resolved and validated during generation. The generated
adapter contains no authored callback or runtime rule configuration.

## Transactional result

Generation adds `Entry::frame(source) -> Result<FrameResult<Entry>,
FrameError>`. `FrameError` is reserved for invalid generated identities, spans,
partitions, or progress invariants and retains the complete source.

An ordinary syntax, ownership, or missing-terminator failure is not fatal. It
selects `FrameResult::Recovered` and discards every strict authored value from
the first pass. Framing then replays every frozen island through the
construction-free recovery interpreter. A later valid island is therefore a
`RecoveredIsland` and cannot be converted into an authored Node or
`Parsed<Entry>`. Each recovered island exposes its recovery root, diagnostics,
and finite-machine progress trace.

Only a wholly successful document selects `FrameResult::Complete`. Its islands
retain strict `Parsed<Entry>` values. Both results expose the borrowed source
and `render_exact`; only authored values have structured `Pretty` rendering.

Each island's `Parsed::source()` is the bounded island slice, including its
framing boundary. Captured occurrence spans remain absolute offsets in the
complete framed document; they are not rebased into that slice. Call
`Parsed::source_bounds()` to inspect the absolute window and
`Parsed::local_source_range(span)` before using an occurrence span to index
`Parsed::source()`. This preserves document-wide span correlation without
making later-island indexing unsafe or ambiguous.

## Exact ownership

The ordered framing parts own every source byte exactly once. Ownership covers
the beginning-of-file gap, inter-item gaps, island content and boundary,
atomic regions, payload data and control line, and the end-of-file gap. Empty,
trivia-only, comment-only, CRLF, and missing-final-newline inputs preserve their
original bytes. Concatenating part spans reconstructs the source exactly.
Boundary-only and consecutive-boundary inputs are ordinary missing islands:
the boundary advances framing, while recovery supplies deterministic missing
syntax diagnostics.

Closed lexical regions remain atomic. Boundary spellings inside a closed
string, comment, nested region, or payload do not split an island. If a unique
continuation cannot be assigned, recovery retains the remaining source as an
unresolved part and reports fail-closed ownership rather than guessing. A
delimiter candidate becomes a framing boundary only when generated recovery
completes the island while leaving that exact token unconsumed as root FOLLOW;
nested grammar separators are therefore consumed by their nested owner. At
EOF, the generated application FIRST set—including struct, enum, and Pratt
roots—is the only evidence that can split a distinct unresolved suffix.

## Payloads

Payload mode begins only after a complete strict island commits the configured
variant. An incomplete prefix never triggers it. The payload scanner treats
the terminator as a complete standalone physical line beginning at BOF or
immediately after LF and ending in LF (with an optional preceding CR), so
boundary spellings inside payload data remain data. A same-line marker or a
final marker without its required line ending remains payload data.
`PayloadRegion::data_span` and
`terminator_span` are distinct; the latter includes its original line ending.
A missing terminator retains the complete suffix as payload data and reports a
recoverable framing diagnostic.
