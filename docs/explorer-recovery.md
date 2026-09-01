# Explorer recovery specification

This document specifies recovery after a strict Explorer parse fails. It is the
normative contract for every recovery implementation; it does not change
strict parsing or authorize partially constructed authored AST values.

The architectural separation is recorded by
[ADR 0002](adr/0002-separate-explorer-recovery-projection.md). The local,
grammar-linked application and private-protocol constraints remain governed by
[ADR 0001](adr/0001-local-explorer-ui.md).

## Construction boundary

Strict parsing is the only operation allowed to construct an authored Node,
`Parsed<T>`, or `ExplorerDocument`. Its transactional behavior and diagnostics
remain unchanged: success returns one complete typed root with occurrence
provenance; failure returns `ParseFailure` and no document.

Recovery is a separate Explorer module behind the existing `explorer` feature.
Its interface adds an exploration operation with these conceptual states:

```text
Exploration<'input>
  = Complete(ExplorerDocument<'input>)
  | Recovered(RecoveryDocument<'input>)
```

The adapter first runs its existing strict parser. A strict success becomes
`Complete`. A lexical, syntax, Pratt-ambiguity, or trailing-input failure starts
recovery from the root cursor over the same finalized lexer records and becomes
`Recovered`; recovery never continues from a partially advanced strict cursor.
Invalid token spans, grammar metadata mismatch, or a violated generated-plan
invariant are fatal exploration errors and produce neither state.

A recovery document has no constructor or conversion to `T`, `Parsed<T>`,
`ExplorerDocument`, `ExplorerNode`, or `ExplorerElement`. Recovery must not:

- add error variants to authored enums;
- turn required authored fields into `Option`;
- construct empty `Vec1` values;
- fabricate token text, semantic fields, spans, or occurrence provenance; or
- select an enum or Pratt variant before frozen dispatch commits it.

Those prohibitions preserve the AST-first guarantee: source that cannot satisfy
the authored type remains incapable of producing that type.

## Recovery representation

`RecoveryDocument<'input>` borrows the exact complete source and owns one eager,
grammar-erased projection plus a non-empty ordered diagnostic collection. It
uses static Explorer schema references rather than copying schema.

```text
RecoveryDocument {
  source,
  root: RecoveryNode,
  diagnostics,
  progress: RecoveryProgressTrace,
}

RecoveryNode {
  schema,
  committed_variant: Option<VariantSchema>,
  extent: Span,
  parts: [RecoveryPart],
}

RecoveryPart
  = Element { schema, span, value: Token | RecoveryNode }
  | Separator { token_schema, span }
  | Missing { site, element, at, expected }
  | Skipped { span, input_items }
```

Parts are ordered by the grammar and source traversal. Every recognized repeat
occurrence is a separate `Element` part. Repetition separators remain separate
from semantic child ordinals, matching parsed provenance. `Missing.at` is a
checked zero-width UTF-8 span and describes an insertion suggestion only; it is
not a token or source edit. `Skipped.span` covers a maximal contiguous run
discarded by one recovery frame. Its token items retain static token schemas
when available; a lexical-error item has no invented schema.

A struct has its declared shape without a variant. An enum or Pratt recovery
node has `committed_variant = None` until the existing bounded dispatch trie
reaches a unique terminal. Commitment is permanent. A failure at owner FOLLOW
or EOF before commitment leaves the variant absent; declaration order is never
a tie-breaker. A committed branch may retain recognized parts around missing or
skipped syntax without implying that its authored variant exists.

`extent` is the checked contiguous navigation range from frame entry through
the last recognized or skipped part owned by that frame; a source-empty node
may have a zero-width extent. It is not authored occurrence provenance. A
node's completeness is derived from its parts and descendants rather than
stored independently: any missing, skipped, or lexical-error descendant makes
it incomplete.

Recovery input merges finalized token records and rejected lexical ranges in
checked source order; equal-position items preserve lexer order. Ignored lexical
text remains source text but is not an input item or recovery part. Every input
item is ultimately recognized or belongs to exactly one skipped part. The
deepest frame that performs synchronization owns that part.

## Frozen recovery decisions

Recovery consumes the same immutable semantic products as strict parsing:
bounded `DispatchPlan` tries, `ContainerMachine` transitions,
`FailureEdge`/`ExpectedRecipe`, exact Pratt binding powers, and evaluated
`FollowRecipe` values. Source-aware recovery planning may join schemas and
construction-free display facts to those products, but neither generation nor
the browser may recompute FIRST, FOLLOW, dispatch, container, or Pratt meaning.

At a failure site:

- `restart` is the analysis-ordered FIRST positions that can restart the failed
  child, branch, or continuation;
- `stop` is that site's evaluated FOLLOW recipe: static suffix FIRST, dynamic
  caller FOLLOW, or their union when the suffix is nullable; and
- EOF is always a stop condition.

For an uncommitted enum or Pratt null-denotation dispatch, restart is the union
of the existing branch prefixes and stop is the owner's caller FOLLOW. For a
required semantic element, restart is the target FIRST and stop is the
element-site FOLLOW. Container and Pratt-specific sets are defined below.

The current token is interpreted through the existing frozen selector. When a
non-Pratt token appears in both restart and stop, recovery follows a unique
action only when the frozen plan establishes one. Otherwise it reports an
unresolved choice and stops the current frame. It never prefers restart, stop,
or an earlier declaration by convention. Pratt caller-FOLLOW overlap has the
specific rule below.

The general synchronization transition is:

1. If lookahead is in stop, add one missing part for a required site, close the
   site, and return without consuming the synchronization token. An optional
   site closes without a part or diagnostic.
2. Otherwise consume recovery input items into one skipped part until restart,
   stop, or EOF. A lexical-error item is always consumed and cannot select
   restart or stop.
3. At restart, diagnose the skipped run and retry the failed operation at the
   new cursor. At stop, add a required missing part and return, leaving the stop
   token for the caller. At EOF, close the required site once and unwind.

## Hard progress guarantee

Every active recovery operation has this progress key:

```text
(input cursor, owner application, parse site, recovery phase,
 container state if any, Pratt minimum binding power if any)
```

Recovery keeps the zero-consumption keys visited by the current exploration.
One transition must do exactly one of the following:

1. consume at least one recovery input item, strictly increasing the cursor;
2. keep the cursor fixed while emitting at most one missing part and
   permanently closing the site, advancing a container phase, or popping the
   current frame; or
3. terminate at EOF.

A zero-consumption key may never be entered twice. In particular, inserting a
missing part cannot retry the same operation, and an uncommitted dispatch
cannot retry at the same cursor. Generated debug assertions and black-box trace
tests must reject duplicate zero-consumption keys.

Each recovered document retains a public, read-only `progress_trace()`. The
trace records the finalized input-record count, entered and closed frame counts,
guarded zero-consumption states, and the execution's explicit finite state
bound. Frame identity and element, repetition, and fixed-syntax states are typed
machine values rather than addresses or numeric phase conventions. The bound is
the input positions (`input_records + 1`) multiplied by the finite guarded state
slots contributed by every entered frame. A successful recovery has no duplicate
guarded key, has closed every entered frame, and reports `is_within_bound()`.

Fixed syntax participates in the same transition guard: one machine transition
may recognize or emit at most one fixed-syntax part. It cannot emit a sequence of
missing parts while bypassing the zero-consumption key invariant. Multi-token
optional and presence groups are selected atomically, then retain a typed
committed token index so each token advances in its own guarded transition.

The cursor is bounded by the finite recovery input, and every same-cursor action
irreversibly consumes a finite parse state or stack frame. Recovery therefore
terminates even across nullable nesting and EOF unwinding. With grammar-fixed
lookahead, work is bounded by consumed records, entered frames, closed states,
and emitted parts; synchronization must not repeatedly scan an already skipped
record.

## Nested failures

The deepest active frame owns the first attempt to recover its failure. It
evaluates its site FOLLOW with the caller FOLLOW supplied at entry. When it
reaches that stop token, it returns an incomplete recovery node without
consuming the token, allowing the parent to recognize it.

If a child returns without consuming input, its site is closed. The parent may
advance, synchronize, or close its own state, but cannot re-enter that child at
the same progress key. Skipped syntax belongs to the deepest frame whose scan
consumed it; parent extents include the child extent but do not duplicate its
skipped part or diagnostic.

## Containers and separators

Container recovery executes the frozen `ContainerMachine` rather than a
generic collection loop.

- `Option<T>` and `Vec<T>` stop before their first occurrence without a missing
  part or diagnostic.
- `Vec1<T>` stopped before occurrence one adds a required-occurrence missing
  part. The recovery projection may show zero recognized occurrences, but no
  `Vec1` value is constructed.
- `Option<Vec1<T>>` is validly absent when stopped before it starts. Once its
  element FIRST commits the container, it obeys the `Vec1` minimum.
- `Box` affects authored storage only and adds no recovery state.

At `Start`, element FIRST parses an element; a valid optional stop closes the
container; a required minimum at stop or EOF adds one missing occurrence.

At `AfterElement`:

- separator FIRST consumes one `Separator` part and enters `AfterSeparator`;
- stop closes the container;
- element FIRST in a separated repetition adds one zero-width missing
  separator and parses the next element; and
- other input is skipped toward separator FIRST, element FIRST, or stop.

At `AfterSeparator`, element FIRST parses the next element. Stop or EOF closes
cleanly only when the grammar permits a trailing separator; otherwise it adds
one missing element. Other input is skipped toward element FIRST or stop.
Recognized and missing separators never become semantic elements.

## Pratt recovery

A Pratt frame carries the exact minimum binding power, evaluated caller FOLLOW,
and any recognized recovery left-hand side.

Null-denotation selection uses the frozen nud trie. A uniquely committed atom
or prefix creates that recovery variant. If a prefix operand fails, its operand
recovers recursively at the prefix binding power; stop or EOF leaves the prefix
variant with a missing operand part. Failure before a unique nud terminal leaves
the variant uncommitted.

The led loop considers only frozen extenders whose left binding power qualifies
at the current minimum. A committed postfix closes immediately. A committed
infix parses its right operand at the frozen right binding power, retaining an
incomplete operator recovery node if that operand is missing. Binding-power
comparison and associativity are identical to strict parsing.

When a qualifying led extender is also admitted by caller FOLLOW, recovery
preserves the strict `PrattAmbiguity` diagnostic, stops the expression without
consuming that token, and lets the caller interpret it. It never guesses led or
caller ownership. Even if the caller then recognizes the token and all later
syntax, the Exploration remains `Recovered`.

## Diagnostics

Exploration diagnostics are Explorer diagnostics, not build diagnostics. Their
kind set is a strict superset of strict Explorer parse classifications:

```text
UnexpectedToken | UnexpectedEof | LexicalError | PrattAmbiguity |
InvalidTokenSpan | GrammarMismatch | TrailingInput |
MissingSyntax | SkippedSyntax | UnresolvedAlternative | InvariantViolation
```

`InvalidTokenSpan`, `GrammarMismatch`, and `InvariantViolation` occur only in a
fatal exploration error and never in a recovery document. Each recoverable
diagnostic carries a checked primary span, found token when present,
analysis-ordered expected atoms, owner and site, and one recovery action:
`Missing { at }`, `Skipped { span }`, or no source edit.

For a recoverable failure, the complete original strict `ParseFailure`
collection seeds recovery diagnostics unchanged and in its existing order.
This matters when lexical finalization reports more than one rejected range. A
recovery part representing the same failure attaches its action to the
corresponding seeded diagnostic rather than emitting another diagnostic.
Consequently the first diagnostic preserves the strict kind, span, found
token, expected order, and message exactly, and multiple seeded lexical
diagnostics retain their relative order.

A trailing-input seed attaches one skipped action covering the suffix consumed
by recovery. It remains `TrailingInput` rather than being rewritten as
`SkippedSyntax`; later independent skips use `SkippedSyntax`.

Later actions use `MissingSyntax`, `SkippedSyntax`, or
`UnresolvedAlternative`. Diagnostics are stable-sorted by `(span.start,
discovery ordinal)`. Left-to-right traversal assigns the ordinal; diagnostics
created at one position while frames unwind remain deepest-first. Expected
atoms keep semantic-analysis order.

Deduplication removes only an exact duplicate key:

```text
(kind, span, found, expected sequence, owner, site, recovery action)
```

The first duplicate is retained. Distinct grammar sites at the same EOF remain
distinct. Adjacent skipped records may coalesce only when one frame consumes
them in one action. Lexical errors retain exact source spans and diagnostics and
do not merge into an unexpected-token diagnostic.

## Adapter, protocol, and UI behavior

The existing strict adapter parse operation remains unchanged. Recovery adds a
separate exploration operation and an advertised `recovery` capability under
the existing `explorer` feature. Without that feature there are no recovery
types, callbacks, generated payload, or protocol fields.

Recovery availability is root-wide. A root advertises the capability only when
every reachable generated Node has an implemented recovery interpreter body.
Compound structs, predictive enums, their containers, and Pratt expressions
share the construction-free interpreter. Pratt frames retain their exact
minimum binding power and frozen nud/led products; strict `Adapter::parse`
behavior and Explorer schema remain unchanged. The root-wide rule still covers
two frozen products this slice cannot interpret yet: an enum selected through
a balanced-delimiter dispatch cut, and an optional container refined by a
sparse consume-vs-skip viability decision. A Pratt selector that contains a
balanced cut keeps the same exclusion. Recovery never falls back to a raw
selector where the frozen plan refines the choice.

The matching-version private JSON protocol uses mutually exclusive responses:

```text
{ "status": "ok",        "document": <complete document> }
{ "status": "recovered", "recovery": { "source": ..., "root": ... },
                          "diagnostics": [...] }
{ "status": "error",     "diagnostics": [...] }
```

The recovered transport shape flattens the recovery document's owned
diagnostics beside its source and root projection; it does not serialize a
second diagnostic copy.

`error` is reserved for fatal exploration failure or for an adapter without
the recovery capability. During capability transition, the UI preserves its
current fallback: it shows strict diagnostics and no current tree when
`recovery` is absent or false. A recovered response replaces any previous
complete tree atomically; stale async responses remain governed by the UI's
request revision.

Recognized recovery parts use normal token highlighting. Skipped and lexical
error spans use diagnostic styling. A missing part appears as a tree placeholder
and zero-width caret marker, never as highlighted or inserted source text.
Bidirectional selection uses the smallest non-empty containing part before its
enclosing recovery node; equal spans prefer a part, then discovery order.
Zero-width missing parts participate only when the caret or a selection endpoint
equals their byte position.

Recovery documents have no parsed provenance and cannot invoke Pretty. The UI
disables Pretty with that reason while retaining the authored source. Samples
continue to produce only complete synthetic Explorer documents and never enter
recovery. Lexical schema inspection remains available independently of either
state.

## Allocation and performance consequences

Strict success keeps the existing typed document and allocation behavior; it
does not allocate a recovery projection or recovery diagnostics. Invalid input
lexes once, performs the transactional strict attempt, then may allocate an
eager recovery tree, skipped-record storage, and diagnostics while borrowing
all source text.

Recovery storage is bounded by recognized input items, entered recovery
frames, emitted missing/skipped parts, and diagnostics. Static schemas are
shared. The progress rule forbids rescanning skipped input and guarantees
termination; this is a correctness contract, not a representative-scale
performance target.

## Normative recovery snapshots

The notation below names schema elements rather than Rust fields. `@N` is a
zero-width byte position, `skip[a..b]` is an exact rejected source range, and a
variant name appears only after unique dispatch.

### Required struct element

Grammar: `Pair := "pair" Name ":" Number`

Source: `pair alpha :`

```text
Pair {
  fixed("pair"), Name("alpha"), fixed(":"), Missing(Number @12)
}
diagnostics:
  UnexpectedEof @12 expected Number, action Missing(Number @12)
```

The seeded strict EOF diagnostic and its missing part are one diagnostic event.
No `Pair` authored value exists.

### Committed and uncommitted enum

Grammar: `Entry := Right("right" Number) | Left("left" Name)`

Source: `right nope`, where `nope` is a Name token rather than Number.

```text
Entry::Right { fixed("right"), skip[6..10], Missing(Number @10) }
diagnostics:
  UnexpectedToken 6..10 found Name expected Number, action Skipped(6..10)
  MissingSyntax @10 expected Number, action Missing(Number @10)
```

`Right` is retained because its fixed prefix uniquely committed it. At owner
FOLLOW or EOF before either prefix commits, the node instead has no variant and
one unresolved-alternative/missing action; recovery never selects `Right`
because it was declared first.

### Repetition and separator

Grammar: `List := "[" Vec1<Item> separated by "," "]"`, with no trailing
separator.

Source: `[a b,]`

```text
List {
  fixed("["), Item("a"), Missing("," @3), Item("b"),
  Separator("," 4..5), Missing(Item @5), fixed("]")
}
diagnostics:
  UnexpectedToken 3..4 found Item expected "," or "]", action Missing("," @3)
  MissingSyntax @5 expected Item
```

The same `[a,]` closes without a diagnostic only for a trailing-enabled
grammar. `[]` is complete for `Vec<Item>` and missing occurrence one for
`Vec1<Item>`; absent `Option<Vec1<Item>>` is complete.

### Nested FOLLOW ownership

Grammar: `Call := "call" "(" Pair ")" ";"` and
`Pair := Name ":" Number`

Source: `call(x:) ;`

```text
Call {
  fixed("call"), fixed("("),
  Pair { Name("x"), fixed(":"), Missing(Number @7) },
  fixed(")"), fixed(";")
}
diagnostics:
  UnexpectedToken 7..8 found ")" expected Number, action Missing(Number @7)
```

The Pair frame leaves `)` unconsumed because it is child FOLLOW. The Call frame
then recognizes both closing tokens. It neither duplicates the diagnostic nor
re-enters Pair.

### Pratt restart and ambiguity

Grammar: Names and integers are nud atoms; `+` is infix with `lbp=10,
rbp=11`; `*` has no nud role.

Source: `a + * 2`

```text
Expr::Add { lhs Name("a"), fixed("+"), skip[4..5], rhs Integer("2") }
diagnostics:
  UnexpectedToken 4..5 found "*" expected nud atom, action Skipped(4..5)
```

The `Add` branch is committed before its right operand recovers at binding
power 11. Source `-` at EOF for a prefix `Neg(bp=30)` retains a committed
`Neg` recovery node with one missing operand at binding power 30.

For `Followed := Expr "+" Integer` on `a + 1`, where `+` is both a qualifying
Expr extender and caller FOLLOW, the result is:

```text
Followed { Expr::Name("a"), fixed("+"), Integer("1") }
diagnostics:
  PrattAmbiguity 2..3 found "+", action none
```

Expr stops without consuming `+`; the caller recognizes it. The tree is still
a recovery document because strict parsing rejected the ambiguity.

## Implementation slices and verification

Recovery implementation is divided into independently reviewable slices that
do not change the strict parser interface:

1. Extend semantic and Explorer planning with construction-free recovery views
   over existing dispatch, container, failure, FOLLOW, and Pratt products.
   Validate identity, ordering, restart/stop derivation, and complete product
   coverage before emission.
2. Add the sealed runtime recovery representation, exploration interface, and
   generated callback. Drive compound, nested, repetition, lexical, EOF, UTF-8,
   diagnostic, and progress behavior through the adapter interface.
3. Add Pratt recovery using the same nud/led plans and exact binding powers.
   Cover prefix, postfix, infix, associativity, missing RHS, and caller-FOLLOW
   ambiguity.
4. Extend the private server projection and bundled UI behind the advertised
   capability. Preserve strict fallback, stale-response clearing,
   bidirectional UTF-8 span interaction, accessibility, and CSP constraints.

Black-box verification must include:

- the five normative snapshots above and both trailing-separator policies;
- `Option`, `Vec`, `Vec1`, `Option<Vec1>`, nested nullable, lexical-error, and
  EOF-unwind cases;
- a trace assertion that no zero-consumption progress key repeats and a bound
  by consumed input items plus entered/closed recovery states;
- exact seeded-diagnostic parity with the existing strict failure (including
  multiple lexical errors and trailing input), exact-key deduplication,
  same-position deepest-first order, and distinct EOF sites;
- UTF-8 source slicing and zero-width caret interaction;
- compile-fail examples proving that recovery cannot construct or convert to
  an authored Node, `Parsed<T>`, or `ExplorerDocument`;
- protocol golden responses for complete, recovered, fatal, and
  capability-disabled states; and
- browser smoke for complete-to-recovered replacement, invalid-input fallback,
  keyboard tree use, source/tree hover, and disabled Pretty.

The focused generated-fixture commands are:

```console
cargo test -p recursa-core --features explorer --locked
cargo test -p recursa-codegen --test generated_compound_nodes --locked
cargo test -p recursa-codegen --test generated_repetitions --locked
cargo test -p recursa-codegen --test generated_pratt --locked
cargo test -p recursa-explorer --locked
```

The authored-library Clippy, documentation-hidden policy, and private Rustdoc
gates remain the canonical commands in
[`docs/agents/documentation.md`](agents/documentation.md).
