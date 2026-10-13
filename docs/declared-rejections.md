# Declared rejections — Design

Status: accepted design, 2026-10-13. Raised by
[freshtonic/recursa#138](https://github.com/freshtonic/recursa/issues/138), for the consumer
[freshtonic/pg-sql#86](https://github.com/freshtonic/pg-sql/issues/86).

Some rejections a raw parser makes are not grammar shapes. They are checks in a `gram.y` action
that look at the set of items a list holds. `ConstraintAttributeSpec` (gram.y REL_17_11
6098-6125) is a list of `ConstraintAttributeElem` in any order; its action builds a bit set and
refuses `NOT DEFERRABLE` beside `INITIALLY DEFERRED`, `NOT DEFERRABLE` beside `DEFERRABLE`, and
`INITIALLY IMMEDIATE` beside `INITIALLY DEFERRED`.

A derived grammar cannot say that with shapes. One Node for each admitted combination needs eight
declarations, twenty-four from 18, and turns the AST into a linked list of the attributes a
statement happens to carry. So the grammar states the forbidden combinations instead, and keeps
one list of one element type.

A **rejection rule** is that statement: a type-level declaration on an enum naming forbidden
unordered pairs of its variants, with message data. A **declared rejection** is the parse failure
it produces.

## The declaration

One attribute carries one message and any number of pairs:

```rust,ignore
#[reject(
    pair(NotDeferrable, InitiallyDeferred),
    code = "42601",
    message = "constraint declared INITIALLY DEFERRED must be DEFERRABLE",
    cite = "gram.y REL_17_11 6114, ConstraintAttributeSpec",
)]
#[reject(
    pair(NotDeferrable, Deferrable),
    pair(InitiallyImmediate, InitiallyDeferred),
    code = "42601",
    message = "conflicting constraint properties",
    cite = "gram.y REL_17_11 6121, ConstraintAttributeSpec",
)]
pub enum ConstraintAttr { NotDeferrable, Deferrable, InitiallyImmediate, InitiallyDeferred }
```

It is accepted on an `ast_node!` enum, and nowhere else: every repetition of that enum runs the
rule, so the rule belongs to the enum as a type rather than to one variant, one field or one use
site.

`code`, `message` and `hint` are opaque to Recursa: they are three strings that the consumer
gives its own meaning. `cite` records the source that rejects the pair. `message` and `cite` are
required, which is stricter than a configuration gate deliberately: a rule that ends a parse must
say why, and must name the source that says so. `code` and `hint` are optional. A repeated key is
an error, exactly as it is for `config`.

A pair is unordered: a list is refused whether the left variant or the right one comes first. A
variant matches by name, so a payload never takes part in the test. One variant repeated is
admitted, because PostgreSQL admits `DEFERRABLE DEFERRABLE`; a pair of one name twice is therefore
an authoring error rather than a rule no list can complete.

## Version gating

A rule is gated only through the composition that already exists:

```rust,ignore
#[config_attr(
    since = pg18,
    reject(
        pair(NotEnforced, Enforced),
        code = "42601",
        message = "conflicting enforcement properties",
        cite = "gram.y REL_18_6, ConstraintAttributeSpec",
    )
)]
```

`reject` carries no `since` key of its own. `config_attr` already gates an attribute the way
`cfg_attr` does ([declared build configurations](configuration-requirements.md)), and a rule that
names a gated variant is gated by the same bound that admits the variant. A build that removed a
named variant and kept the rule is an authoring error, not a silently dropped rule, so the author
is made to write the bound.

## The semantics

Every repetition field whose element type is the enum — `Vec`, `zero_or_many!`, `one_or_many!`, a
separated list, whatever forms the grammar has — walks its items in source order. At item `i` the
walk tests the rules in authored order and, inside each rule, the pairs in written order, against
the variants of items `0..i`. The first pair a rule completes ends the parse with that rule's
message data and **the full extent of item `i`** as the error span.

That is `processCASbits` and `parser_errposition(@2)` reproduced: the specific message wins over
the general one when an item completes a pair of each, because the specific rule is written first;
and the position is the item that made the conflict, never the list.

The check runs at the reduce of the list step, which is the one place that holds both the items
the list has passed and the extent of the item just added. A list step is a rule the lowering
already emits, so no LR table changes.

### One walk, two call sites

`recursa-core`'s `rejection` module owns the walk. It takes the variant tag of every item in
source order, with the extent the caller projected for the item the step added, and the static
rule table, and it answers with the rule's note and that extent. Generation emits one thin call
site per backend:

- The value parse and the arena parse share the one generated list action, so one call site
  serves both. It reads each passed item's variant off the `Vec1` it is extending.
- The flat parse has a parallel generated action. A flat list is a run of handles, and a flat enum
  is one `Flat{Ident}` in a pool, so the variant of an item is a `match` on the pooled value and
  nothing is materialized. The items the list has passed are still on the list scratch stack.

Both take the extent from the same place — the extent of the element symbol the step consumed —
so all three storages accept and reject the same sources with the same note and the same span.
`docs/acceptance` has no row that differs.

### The extent stack

A nested parse never kept the LR extent stack: only flat construction asked for one
(`recursa-core/src/lr.rs`). The extent of a reduced nonterminal lives nowhere else, so a grammar
that declares a rejection rule now asks for the stack in its generated parse entry point, through
`ParseContext::demand_extents`. The request is independent of the capture request, because
accepting or rejecting a parse must not depend on the entry point and neither must the position of
the rejection. A grammar that declares no rule asks for nothing and allocates nothing, exactly as
before.

The cost is one span pushed per shift and one merge per reduce, paid only by a grammar that
declares a rule. The alternative — widening every state-stack frame to carry its first and last
token index — would make the extent of any nonterminal recoverable with no extra stack, and it
remains open if the stack ever costs a consumer too much.

## The failure

A declared rejection is a new `ParseErrorKind::DeclaredRejection` with its own append-only runtime
code, `RCA4105`. The error carries the rule's whole note:

```rust,ignore
if let Err(error) = Unique::parse_spanned(&mut input) {
    if let Some(note) = error.rejection() {
        // note.code(), note.message(), note.hint(), note.citation(), note.site()
        // error.code() == "RCA4105", error.span() == the item's extent
    }
}
```

`RejectionNote` is `RequirementNote`'s counterpart, and deliberately so: both answer the same
consumer question — which source rule refused this construct, and what does that source say about
it — so a consumer renders the two the same way. The error's `Display` is the rule's message. A
build without the `spans` feature reports `Span::SYNTHETIC`, and the message data is the same.

## Sampling

`Arbitrary` samples a repetition as one call against the field's type. After the call, generated
code runs the parser's own walk over the sampled items and drops every item that would complete a
forbidden pair, so a sampled value always parses back. The first item of a list completes nothing,
because a pair never names one variant twice, so a `one_or_many!` field stays non-empty.

The arena form samples its seed mirror and then materializes it, and the mirror receives the same
filter from the same rendering, so the two forms drop the same items. A flat grammar has no
sampler of its own.

## Diagnostics

New authoring diagnostics, with new codes:

| Code | Failure |
|---|---|
| `RCA3144` | A `#[reject(...)]` cannot be read: no pair, no `message`, no `cite`, a repeated key, an unsupported entry. |
| `RCA3145` | A `#[reject(...)]` on something other than an `ast_node!` enum, including a variant or a field. |
| `RCA3146` | A `pair(...)` names a variant the enum does not declare. |
| `RCA3147` | A `pair(A, A)` names one variant twice. |
| `RCA3148` | Two rules of one enum name the same unordered pair. |
| `RCA3149` | A `pair(...)` names a variant this build's configuration removed; gate the rule. |

A rule on an enum that no repetition field uses is accepted silently. It costs nothing, and a
grammar under migration reaches that state on the way to its first list.

## What stays the same

Diagrams, `Pretty`, `Visit` and the Explorer are untouched: a rejection rule constrains which
values a parse produces, not what a value holds or how it renders. The one generated addition to
documentation is a single rustdoc line on the enum listing its rules and their messages. No ADR:
the declaration adds authoring surface and a runtime failure class, and changes no decision this
project recorded.
