# Give each Flat AST its own typed stores

Status: accepted

Each parsed Flat AST owns its typed Node stores and a typed root index, keeping allocation lifetime
and cleanup local to one result rather than sharing reusable stores between independently parsed
roots. The reachable structure retains the primary AST's tree semantics: each non-root Node has
one parent and cycles are invalid, so moving a subtree is allowed but duplicating its use requires
copying its Nodes. This preserves familiar editing semantics instead of introducing aliasing
between SQL locations.

Indices are transparent typed wrappers around `NonZeroU32`, without runtime owner tags; raw
construction is private and access is bounds-checked. Encode slots as `slot + 1`, keeping optional
indices the same four-byte size. Keep slots until the owning result is dropped, with no initial
slot removal, reuse, or compaction. The Flat AST must be `Send + Sync`.

Retain `ParseError` for ordinary parsing failures. Exhausting the fixed-width index encoding uses
a checked panic with a clear message in all builds; no new public error type is introduced, and
indices must never wrap or truncate. This is a representation-capacity guard, distinct from vector
allocation failure.

Parsed content-token text may still borrow the input; inserted or replaced tokens can own their
text, as in the existing token model. The result owns its Node storage, not necessarily all text
to which those Nodes refer. Use standard `Vec` stores for the first working parser and measure
arena allocation separately against that baseline; an arena investigation must not delay parsing.

Mutation API design is deferred until parsing works. Its eventual contract must preserve the tree
and index invariants by construction; unrestricted mutation followed by validation is not the
accepted model. The lack of stored owner tags does not waive this future API requirement.
