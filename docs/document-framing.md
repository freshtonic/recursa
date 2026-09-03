# Document framing

Document framing is a generated exact-source adapter for languages whose input
alternates between repeated grammar islands and regions that must remain raw.
It is available when the `explorer` feature is enabled for both the `recursa`
facade and `recursa-codegen`. Framing uses strict parsing only; it has no
recovery path.

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

Generation adds `Entry::frame(source) -> Result<CompleteFrame<Entry>,
FrameFailure>`.

`CompleteFrame` is returned only when every island parses strictly and every
payload terminates. Its islands retain strict `Parsed<Entry>` values. A final
island does not need a trailing boundary: when its strict parse consumes every
significant token to end of input, the document completes and the island keeps
its authored value, with only trivia permitted after it as the end-of-file gap.

`FrameFailure` has two variants:

- `FrameFailure::Rejected(FrameRejection)` is ordinary invalid input. The
  rejection retains the complete source, the extent of the first failing island
  candidate (`island()`), that candidate's strict parse diagnostics
  (`diagnostics()`), and a framing cause (`framing()`) when the island itself
  parsed strictly but the document could not continue. The framing causes are
  `RCA5001`, a complete payload-opening island without its standalone
  terminator, and `RCA5002`, a complete island followed by significant source
  without its required boundary. A rejection never contains an authored value,
  and later islands are not parsed.
- `FrameFailure::Fatal(FrameError)` is reserved for invalid generated
  identities, spans, partitions, or progress invariants (`RCA5090`) and retains
  the complete source.

For a syntax failure the rejected extent runs from the island start to the end
of the first candidate boundary at or after the failure location, or to the end
of input when no candidate boundary follows it. The failure is a missing
boundary, and carries the `RCA5002` framing cause, when the source before the
failing token is itself a complete strict island; the strict diagnostic is the
same one that Explorer parsing reports for the same source. A missing
terminator reports the unterminated payload suffix as the rejected extent with
no strict diagnostics.

Each island's `Parsed::source()` is the bounded island slice, including its
framing boundary when one is present. Captured occurrence spans remain absolute
offsets in the complete framed document; they are not rebased into that slice.
Call `Parsed::source_bounds()` to inspect the absolute window and
`Parsed::local_source_range(span)` before using an occurrence span to index
`Parsed::source()`. This preserves document-wide span correlation without
making later-island indexing unsafe or ambiguous.

## Exact ownership

The ordered parts of a complete frame own every source byte exactly once.
Ownership covers the beginning-of-file gap, inter-item gaps, island content and
boundary, atomic regions, payload data and control line, and the end-of-file
gap. Empty, trivia-only, comment-only, CRLF, and missing-final-newline inputs
preserve their original bytes. Concatenating part spans reconstructs the source
exactly. A boundary-only input is an ordinary rejected island: the island
grammar decides whether an empty island parses.

Closed lexical regions remain atomic. Boundary spellings inside a closed
string, comment, nested region, or payload do not split an island. A boundary
candidate ends an island only when the island parses strictly up to that exact
token; boundary spellings consumed by nested grammar structure therefore stay
inside their island, and a failure inside such a structure is reported with
the strict diagnostic at its exact location.

## Payloads

Payload mode begins only after a complete strict island commits the configured
variant. An incomplete prefix never triggers it. The payload scanner treats
the terminator as a complete standalone physical line beginning at BOF or
immediately after LF and ending in LF (with an optional preceding CR), so
boundary spellings inside payload data remain data. A same-line marker or a
final marker without its required line ending remains payload data.
`PayloadRegion::data_span` and `terminator_span` are distinct; the latter
includes its original line ending. A missing terminator rejects the document
with the `RCA5001` framing cause at end of input.
