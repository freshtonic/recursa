# Flat AST allocation research

Date: 2026-09-14. Status: research findings and proposed comparison, not an allocator decision.

Scope: the [Flat AST design](flat-ast-design.md), with one owning result, typed Node stores,
indexed relationships, editable child lists, borrowed parsed text, and owned replacement text.
The workspace locks bumpalo **3.20.3** and enables its `collections` and `boxed` features.
The implementation findings below were checked against that installed crate's source
(upstream revision `84654ace6be4444da3ff102a0a0af3b38c4df4fb`) and the official online
documentation, which also reports 3.20.3. No prototype or benchmark was run.
[Workspace lock](../Cargo.lock), [dependency features](../recursa-core/Cargo.toml),
[bumpalo documentation](https://docs.rs/bumpalo/latest/bumpalo/collections/vec/struct.Vec.html).

## Finding

**A bump allocator is worth comparing, but typed `std::Vec` stores are the useful baseline.**
Replacing owning Node relationships with indices already removes their individual box
allocations. Allocator choice then mainly concerns Node-store buffers and child-list buffers.
This is a consequence of the accepted transformation, not measured evidence of a speedup.
[Accepted transformation](flat-ast-design.md#field-mapping).

| Concern | Typed `std::Vec` stores | Typed bumpalo vectors |
| --- | --- | --- |
| Allocation | Each non-empty, non-zero-sized vector needs a buffer; pushes within capacity do not allocate. | Buffers come from shared bump chunks; allocating within an existing chunk avoids a new global allocation. |
| Growth | May reallocate; growth strategy is unspecified. | Can reuse space when growing the latest allocation; otherwise allocates a new buffer and copies. |
| Reclamation | Dropping a vector drops elements and releases its buffer to its allocator. Clearing retains capacity. | Dropping a vector drops elements; buffer space is generally retained until arena reset/drop, except last-allocation reclamation. |
| Ownership | Vectors own their storage without an allocator borrow. | Each vector borrows the `Bump` and cannot outlive it. |

Sources: [Rust Vec guarantees](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#guarantees),
[bumpalo allocation model](https://github.com/fitzgen/bumpalo#bump-allocation),
[bumpalo growth and deallocation implementation](https://docs.rs/bumpalo/latest/src/bumpalo/lib.rs.html#2240-2399),
[bumpalo Vec destructor](https://docs.rs/bumpalo/latest/src/bumpalo/collections/vec.rs.html#2550-2560).

## Consequences for this representation

**Growing many interleaved stores can retain superseded buffers.** In 3.20.3, `Bump::grow`
tries to allocate only the additional bytes when the buffer is the latest allocation and space
permits. Even this path copies existing bytes because the allocator grows downward. Its fallback
allocates and copies without reclaiming the old buffer. `dealloc` only rewinds the latest
allocation. Therefore interleaving different Node stores and child-list growth can increase
retained memory; how much depends on the actual reduction order and capacities. The final
sentence is an inference requiring measurement.
[Implementation](https://docs.rs/bumpalo/latest/src/bumpalo/lib.rs.html#2240-2399).

**Flat does not eliminate list allocation.** `Vec<ItemIndex>` still has a separately allocated
buffer when populated; standard `Vec` has no inline small-list storage. Using bump vectors only
for Node stores leaves those child-list allocations unchanged. Changing lists to bump vectors
would be a further container/API choice, including equivalent support for `Vec1`.
[Rust Vec guarantees](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#guarantees),
[accepted field mapping](flat-ast-design.md#field-mapping).

**Destructors remain necessary.** `Bump` does not run destructors for values merely allocated
inside it. Recursa's synthesized token text owns `Box<str>`, and standard child lists own heap
buffers. Skipping Node destruction would leak those external allocations. A retained, normally
dropped bump vector does destroy its elements; ensure the owning result drops every initialized
Node store before releasing its arena, including on parse failure. Converting stores to borrowed
bump slices and relying solely on arena teardown would lose that ownership behavior.
[Bump destruction contract](https://docs.rs/bumpalo/latest/bumpalo/struct.Bump.html#no-drops),
[Vec slice conversion and destructor](https://docs.rs/bumpalo/latest/src/bumpalo/collections/vec.rs.html),
[ContentText storage](../recursa-core/src/parsed.rs).

**Indices simplify edges, but bump vectors still introduce allocator lifetimes.** An owned result
containing its own `Bump` and vectors borrowing it needs a sound owner/dependent arrangement.
Parsed text separately borrows the input. Existing `ArenaParsed` demonstrates lifetime confinement,
but exposes only immutable AST access; it does not establish the mutable Flat AST API. Keep
mutation scoped to the owner so inserted collections use the correct allocator and borrowed
values cannot escape. This is an API-design implication, not a proven implementation.
[BumpVec constructors and lifetimes](https://docs.rs/bumpalo/latest/bumpalo/collections/vec/struct.Vec.html#method.new_in),
[current owned wrapper](../recursa-core/src/parsed.rs).

Append-driven reallocation preserves numeric positions, whereas `remove`, `swap_remove`, and
sorting can change which Node occupies an index. Neither allocator enforces tree semantics,
index ownership, or stale-index rejection. Slot deletion/reuse must be designed separately from
buffer reclamation. BumpVec also reports `!Send` and `!Sync`; thread transfer or sharing cannot
be assumed for a wrapper containing it.
[Rust Vec mutation API](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.remove),
[BumpVec auto traits](https://docs.rs/bumpalo/latest/bumpalo/collections/vec/struct.Vec.html#synthetic-implementations).

## Proposed bounded comparison

Once implementation and measurement are authorized, compare:

1. Primary AST with occurrence capture disabled.
2. Flat AST using standard Node-store and child-list vectors.
3. The same Flat AST with bump-backed Node stores, keeping lists and token text unchanged.

Only add bump-backed child lists if allocation evidence justifies that additional variable.
Use identical SQL fixtures and recognition behavior; report parsing, dropping, global allocation
counts, peak allocated bytes, and retained bytes separately. Include one parse/edit/render case
that replaces a token with owned text and grows a child list, then verify semantic reparsing and
cleanup. Report a capacity policy rather than silently pre-sizing one candidate with foreknowledge
of its fixture. These are proposed measurements, with no acceptance threshold yet.

`Bump::allocated_bytes()` alone is not comparable to total process allocation: it measures arena
chunks and includes padding, while external strings, standard lists, input and rendered output
remain outside that figure. `allocated_bytes_including_metadata()` includes arena metadata but
still excludes those external allocations.
[Memory counters](https://docs.rs/bumpalo/latest/bumpalo/struct.Bump.html#method.allocated_bytes).

Keep any eventual benchmarks explicitly opt-in under the
[repository test policy](agents/testing.md). Chunked typed storage is a possible later design
branch if copying dominates; it is outside this first allocator comparison.

## Unresolved questions

- How much of parsing cost remains in allocations after flattening and suppressing provenance?
- Do interleaved store/list growth and edit history retain unacceptable arena capacity?
- Are detached Nodes retained, destroyed in place, or reclaimed through slot reuse/compaction?
- What safe mutable-owner API and thread-transfer contract must both implementations support?
- Should child-list container types be fixed public API or permit a later allocator change?
