# Let a validation hook refuse a Node when its reduce builds it

Status: accepted

Design: [`docs/declared-rejections.md`](../declared-rejections.md), "Refused variants and
validation hooks". Raised by freshtonic/pgtk#55.

A `gram.y` action can refuse a statement that its rule matched, and declared rejection rules
reproduce the checks that read a list: pairs, `once` sets and text rules. Other checks read one
Node and compare its values: `row OVERLAPS row` counts the values of each row, `makeOrderedSetArgs`
compares the types of two arguments, and `insertSelectOptions` merges the clauses of a
parenthesized query into the query around it. No declaration that Recursa can evaluate says these,
and each new declarative form would carry one consumer's semantics into the generator.

We choose two additions. A refused variant, `#[reject(variant(V), ...)]`, is a declared rule like
the others: the variant keeps its syntax, so its tokens keep their place in the LR tables, and the
reduce of the variant ends the parse with the rule's message data. A validation hook,
`#[validate(path, flat = path)]` on a Node or on one variant, names consumer functions that the
generated reduce calls with the Node it has just built; a `Refusal` names a static
`RejectionNote` and, optionally, the field whose extent the error reports. This is the first
consumer code that runs inside the parse. We accept it because the parse already calls consumer
code (a classification's fold function), because the hook is pure and runs once per reduce of one
Node, and because the error has the same shape as every declared rejection.

The value, the arena and the flat reduces must refuse the same sources, and the flat reduce holds
handles, not values. A flat hook therefore receives the flat Node and the stores so far, lent by the
builder as a `FlatAst` rooted at a placeholder handle and taken back after the call, so the hook
reads and renders with the owner's own API. The consumer writes the nested and the flat function of
each check; reading the same fields in the same order keeps them equal, and comparing values by
their Pretty rendering keeps them equal by construction, because one renderer serves both
storages. `Arbitrary` never samples a refused variant, and it does not run a hook, because the
arena form samples a seed mirror that the hook's types do not describe; a consumer's round trip
accepts a sample that both parsers refuse with the same message.
