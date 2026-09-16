# Store Flat ASTs in author-selected typed pools over one shared pool

Status: accepted

Design: [`docs/flat-ast-design.md`](../flat-ast-design.md).

ADR 0009 gave every Node type its own `Vec` store. pg-sql has 1,463 declarations, so one result
would carry roughly 35 KB of empty vectors and pay one allocation per type it touches, and the
allocation research showed that per-field child vectors survive flattening. The other extreme,
one arena with one enum over every Node, is unusable at 1,400 variants. We choose the middle:
a declaration marked `#[flat(pool)]` gets a contiguous typed pool per application, every other
Node lives in one shared pool of 4-byte words, every list lives in one list store, and every
content token's text lives in one text pool. Only the author knows which types a consumer scans
linearly or annotates with a dense side table, so the choice is an attribute on the declaration,
not a generator heuristic, and the default is the shared pool.

One `Handle<T>` type covers both pools; a generated trait impl on `T` selects the storage, so
moving a type between pools changes layout and nothing else. Shared-pool reads cast aligned
pointers into the word buffer; that `unsafe` code is confined to one Miri-tested module in
`recursa-core`, with the value stack as precedent. Nodes hold text handles instead of inline
`ContentText`, which makes every flat Node `Copy` and keeps the no-copy text promise. Spans are
captured per Node under the existing `spans` request, which amends ADR 0008 from "no spans" to
"no occurrence provenance; spans on request". Pools stay append-only with stable handles, and
public mutable indexing is ruled out, so the deferred mutation API keeps ADR 0009's
invariant-by-construction requirement. This supersedes ADR 0009's "one store per type" and
"standard `Vec` stores first"; its ownership, index encoding, capacity guard, and `Send + Sync`
decisions stand.
