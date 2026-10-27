# Declared rejections — Design

Status: accepted design, 2026-10-13. Raised by
[freshtonic/recursa#138](https://github.com/freshtonic/recursa/issues/138), for the consumer
[freshtonic/pg-sql#86](https://github.com/freshtonic/pg-sql/issues/86). Extended 2026-10-15 by
[freshtonic/recursa#141](https://github.com/freshtonic/recursa/issues/141), for the consumers
[freshtonic/pg-sql#85](https://github.com/freshtonic/pg-sql/issues/85) and
[freshtonic/pg-sql#87](https://github.com/freshtonic/pg-sql/issues/87): `once`, classifications
and text rules ([Sets, classifications and text rules](#sets-classifications-and-text-rules)).
Extended 2026-10-15 by [freshtonic/recursa#144](https://github.com/freshtonic/recursa/issues/144),
for the consumer [freshtonic/pg-sql#88](https://github.com/freshtonic/pg-sql/issues/88): text
rules on a struct field ([Text rules on a struct](#text-rules-on-a-struct)). Extended
2026-10-27 for names of more than one token: classifications through a projection
([Classification through a projection](#classification-through-a-projection)) and omitted
variants ([Sampling](#sampling)). Decision record:
[ADR 0014](adr/0014-classify-through-a-projection-and-omit-variants-from-sampling.md).

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
site. (A text rule may also go on a struct, which then refuses itself: see
[Text rules on a struct](#text-rules-on-a-struct).)

`code`, `message` and `hint` are opaque to Recursa: they are three strings that the consumer
gives its own meaning. `cite` records the source that rejects the pair. `message` and `cite` are
required, which is stricter than a configuration gate deliberately: a rule that ends a parse must
say why, and must name the source that says so. `code` and `hint` are optional. A repeated key is
an error, exactly as it is for `config`.

A pair is unordered: a list is refused whether the left variant or the right one comes first. A
variant matches by name, so a payload never takes part in the test. One variant repeated is
admitted, because PostgreSQL admits `DEFERRABLE DEFERRABLE`; a pair of one name twice is therefore
an authoring error rather than a rule no list can complete. A rule that must refuse a repeat says
so with `once(...)` ([below](#sets-classifications-and-text-rules)), so a repeat is refused only
where a declaration names it.

## Sets, classifications and text rules

`xmltable_column_el` (gram.y REL_17_11 14000-14090) needs three more things than
`ConstraintAttributeSpec` does. Its option list holds options in any order, and its action then
refuses a second PATH, a second DEFAULT and a second nullability, the same option written twice
included. An option may be the keyword arm or `IDENT b_expr`, and the action reads the name of the
identifier: `path` and `default` count as the keyword arms, and every other name is refused as an
"unrecognized column option". A derived grammar cannot say any of the three with shapes or pairs.

### `once`

```rust,ignore
#[reject(once(NotNull, Null), code = "42601", message = "conflicting or redundant NULL / NOT NULL declarations", cite = "...")]
```

`once(A, B, ...)` admits at most one item whose label is in the set: the second such item is
refused, whether it repeats a member or names another one. **Decision (2026-10-15):** this
reverses the choice above that a repeat is admitted, but only for the rules that name a `once`
set. Every other rule keeps admitting repeats, `DEFERRABLE DEFERRABLE` stays accepted, and
`pair(A, A)` stays the authoring error `RCA3147`, whose message now points to `once(A)`. A
declaration makes one test, so `pair(...)` entries and a `once(...)` do not share one
declaration.

### Classification

```rust,ignore
#[classify(Named.name, fold = crate::ident::fold_name, text("path", Path), text("default", Default))]
```

A classification names one variant and the identifier field of its payload, the consumer's fold
function, and a list of folded texts with the label each one counts as. The variant must hold
exactly one non-generic struct Node, and the field must be a content token or a non-generic enum
whose every variant holds one content token (a token wrapper such as pg-sql's `IdentOnly`).
Generation checks that shape, because it is known only once the Nodes are planned.

The fold function is the consumer's, because the scanner's folding rules are the consumer's:
pg-sql downcases an unquoted name and keeps a quoted one, as `scan.l` does. Its type is
`fn(&str) -> Option<impl AsRef<str>>`; `None` means the text has no folded form.

A **label** is a variant name or a name a classification maps a text to. A label that is no
variant of this build is a **text label**, numbered after the variants. That is what lets one
rule serve every build: before PostgreSQL 17 `PATH` is no keyword and the enum has no `Path`
variant, so `text("path", Path)` makes `Path` a text label, and `once(Path)` refuses a second
`"path"` there exactly as it refuses `PATH` beside `"path"` from 17. A rule that names a removed
variant which no classification maps a text to is still `RCA3149`.

An item's **class** is the label the rules see: the variant itself, or the label its folded text
maps to. Pairs and `once` sets compare classes.

### Text rules

```rust,ignore
#[reject(not_in(Named, "path", "default"), code = "42601", message = "unrecognized column option", cite = "...")]
#[config_attr(since = pg15, reject(one_of(Named, "__pg__is_not_null"), ...))]
```

`one_of(V, "a", ...)` refuses an item of variant `V` whose folded text is one of the texts, and
`not_in(V, ...)` refuses one whose folded text is none of them, an item with no folded text
included. `V` must carry a classification, which says where its text is and how to fold it.
Unlike a pair or a set, a text rule refuses one item on its own, so the check also runs at the
first step of a list for an element type that declares one; an element type with no text rule
renders no check there, and its generated code is unchanged.

### Text rules on a struct

`opt_float` (gram.y REL_17_11 14466-14487) refuses `FLOAT(p)` when `p` is below 1 or above 53.
Each limit has its own message, and `parser_errposition(@2)` puts the error at the `Iconst`. The
precision is one value in a struct, not an item of a list, so no rule above can refuse it.

```rust,ignore
#[classify(value, fold = crate::literal::float_precision_class)]
#[reject(one_of(value, "below_1"), code = "22023", message = "precision for type float must be at least 1 bit", cite = "...")]
#[reject(one_of(value, "above_53"), code = "22023", message = "precision for type float must be less than 54 bits", cite = "...")]
pub struct FloatPrecision { pub value: IntegerLit }
```

On a struct, a classification names one field of the struct itself, with no variant and no
`text(...)` mappings, and a text rule names that field in place of a variant. The field has the
shape a classified field has on an enum: a content token or a token wrapper, behind nothing but
`boxed!`. The fold function gives the text the rules compare, and it may give any text: here it
reads the integer value and gives the range the value falls in, so leading zeros and, for
PostgreSQL 16 and later, every radix fold to the same text.

The struct is refused when it is reduced, which is the one place that holds its field. The rule is
planned as a text rule over a single variant, tag 0, so the runtime walk is the one above with one
item. The error span is the extent of the field's own value, without the fixed tokens its
`#[tok(...)]` group writes around it, as `parser_errposition(@2)` gives. The value, arena and flat
reduces each check the struct they have just built, so all three agree. A struct takes text rules
only: `pair(...)` and `once(...)` read a repetition, and on a struct they are `RCA3145`.

### Classification through a projection

A classification reads one token. But in some languages one name can have more than one token,
for example a quoted name with an optional clause that names its escape character. A grammar
models the form as a struct of a name and an optional clause, and adds a variant that holds the
struct to each token-wrapper enum of its identifiers. Without more, a classification cannot read
such a wrapper, because one of its variants holds no token. The struct says what a
classification reads instead:

```rust,ignore
#[classify(via(name, escape), spell = crate::names::escaped_spelling)]
pub struct EscapedName {
    pub name: QuotedName,
    pub escape: Option<EscapeClause>,
}

#[classify(via = escape_char)]
pub struct EscapeClause {
    #[tok(ESCAPE, this)]
    pub escape_char: StringLit,
}

#[classify(via = name.text)]
pub struct WrappedName {
    #[tok(OPTION, this)]
    pub name: ColumnName,
}
```

A **projection** names one or more field paths of the struct. A path names a field of the
struct, then a field of each struct it passes through, behind nothing but `boxed!`, and it ends
at a field that a classification reads: a content token, a token wrapper, or another struct with a
projection. The last field of a path may be an `Option`; an absent value gives the empty text,
which no token has. With one path, the text is the text of that path. With `spell = path`, the consumer's
function `fn(&[&str]) -> Option<String>` makes the one text from the texts of the paths in
written order; more than one path needs it. The classification then folds that text with its
own fold function, as if one token held it, so a spelling function gives a spelling, not a
folded text. In the example, the spelling function decodes the name with the escape character
of the clause (a default without one) and gives a quoted name, which the fold of every
classification reads as a name. A path with no text,
or a spelling function that answers `None`, gives the item no folded text.

A struct with a projection stands where a classification expects a token: as the classified
field itself, or as the payload of one variant of a token-wrapper enum, behind nothing but
`boxed!`. A wrapper whose every variant holds a token is planned exactly as before, so a grammar
with no projection generates the code it generated before projections existed. A projection is
no classification of the struct, so the struct can also classify one of its own fields for its own
text rules, as `EscapeClause` can refuse an escape character that is not valid.

The nested key reads the paths through field access. The flat key reads the struct and each
struct a path passes through out of their pools, and the token texts out of the text pool, so
nothing is materialized and all three storages still agree. A text that a spelling function makes
is owned, so only a key that reads one holds a `Cow<str>`.

A projection is written on a struct, once. On an enum it is `RCA3145`, a second one on one struct
is `RCA3154`, and a declaration that cannot be read, or that names more than one path without
`spell`, is `RCA3144`. A path that ends at no field a classification reads is a generation
error, because the shape is known only once the Nodes are planned.

### The key

The walk reads one **key** per item: its variant tag, its class tag, and the index of its folded
text in the enum's text table (classification texts first, then rule texts), or none. Generated
code computes the key and the runtime compares integers, so the walk never sees text. The
nested key reads the field through the payload; the flat key reads the payload and the wrapper
out of their pools and the text out of the text pool, so nothing is materialized and all three
storages still accept and reject the same sources with the same note and span.

### Order and a known difference

`gram.y` runs the `IDENT b_expr` check (`__pg__is_not_null`) when the option is reduced and the
`xmltable_column_el` checks when the whole list is, so with two errors in one list PostgreSQL
reports the reserved name first. The walk checks each item as the list grows, so it reports the
first refused item in source order. Both refuse the same lists; only the message of a list with
two errors can differ.

## Refused variants and validation hooks

Extended by freshtonic/pgtk#55. Decision record: [ADR 0013](adr/0013-validation-hooks-run-consumer-code-at-the-reduce.md).

Some checks of a `gram.y` action read one Node, not a list. `frame_extent` refuses a frame that
starts at `UNBOUNDED FOLLOWING` (REL_17_11 gram.y 16335), although `frame_bound` admits the
form: the keyword must shift there, so the grammar keeps the arm. `row OVERLAPS row` counts the
values of each row (15002), `makeOrderedSetArgs` compares the types of two arguments (18843), and
`insertSelectOptions` merges the clauses of a parenthesized query into the query around it
(18880-18936). Two declarations cover them.

### `variant(V)`

```rust,ignore
#[reject(
    variant(UnboundedFollowing),
    code = "42P20",
    message = "frame start cannot be UNBOUNDED FOLLOWING",
    cite = "gram.y REL_17_11 16335, frame_extent",
)]
pub enum WindowFrameSingleBound { CurrentRow, UnboundedPreceding, UnboundedFollowing, ... }
```

A refused variant belongs to the enum, as every rule does, and it refuses every value of the
variant. The variant keeps its syntax, so its tokens keep their place in the LR tables, and the
reduce of the variant ends the parse with the rule's message data at the extent of the variant. A
`variant(...)` entry makes the declaration's one test. The diagnostics of the names are the ones
of a pair: `RCA3146` for an unknown variant and `RCA3149` for a variant the configuration removed.
On a struct it is `RCA3145`.

### `#[validate(...)]`

```rust,ignore
#[validate(crate::validation::overlaps, flat = crate::validation::overlaps_flat)]
Overlaps(boxed!(Row), #[tok(OVERLAPS, this)] boxed!(Row)),
```

A validation hook names consumer functions, on a Node or on one variant of an enum. The generated
reduce calls them when it has built the Node, after the Node's own text rules:

- the value and the arena reduces call `path` with `&Node`;
- the flat reduce calls `flat = path` with `&FlatNode` and `&FlatAst<'_, FlatNode>`. The builder
  lends its stores as a result rooted at a placeholder handle and takes them back after the call,
  so the hook indexes handles and lists, and renders a child, with the owner's own API.

A grammar that emits the Flat AST requires `flat = path`, because the three storages must accept
and reject the same sources. Each function answers `Result<(), Refusal>`. A `Refusal` names a
static `RejectionNote`, which the consumer builds with `RejectionNote::new`, and optionally the
semantic field (counted in declaration order over the fields of the Node's variant) whose extent
the error reports, as `parser_errposition` names one. Without a field, the error is at the extent
of the whole Node. The failure is the ordinary declared rejection, `RCA4105`.

The nested and the flat function of one check read the same fields in the same order. A check that
compares values compares their Pretty renderings, which are equal in both storages by
construction, because one renderer serves both.

A hook adds the extent stack to the parse, as a declared rule does. A grammar with neither pays
nothing.

### Sampling

`Arbitrary` never samples a refused variant. It does not run a hook: the arena form samples a seed
mirror, and the hook takes the arena type. A sampled value can therefore be one that a hook
refuses; a consumer's round trip accepts it when both parsers refuse it with the same message.

A variant that parses can still be one whose samples a hook or a text rule refuses nearly always.
A name with an escape clause is one: few sampled escape strings name a valid escape character,
and identifiers occur in nearly every Node, so sampling the variant spends the sample of nearly
every Node that holds an identifier. `#[omit(Arbitrary)]` on the
variant keeps it out of sampling:

```rust,ignore
pub enum ColId {
    Text(ColIdText),
    #[omit(Arbitrary)]
    Escaped(EscapedName),
}
```

On a declaration, `#[omit(...)]` suppresses configured trait implementations. On a variant it
names `Arbitrary` only, once (`RCA3108` otherwise): the sampler chooses among the other variants,
as it does for a refused variant, and the payload type keeps its own `Arbitrary` implementation.
The arena form's seed mirror keeps the Node and variant identities, so it reads the same omission
and makes the same choice. An enum whose every variant is omitted or refused answers
`arbitrary::Error::IncorrectFormat`. A grammar that omits no variant generates the code it
generated before the declaration existed.

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
the classes of items `0..i`. The first rule that refuses item `i` ends the parse with that rule's
message data and **the full extent of item `i`** as the error span. Without a classification an
item's class is its variant.

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
code runs the parser's own walk over the sampled items and drops every item a rule refuses, so a
sampled value always parses back. A text rule can refuse every item of a sampled
`one_or_many!` list, and then the sampler answers `arbitrary::Error::IncorrectFormat`: those input
bytes describe no value.

The arena form samples its seed mirror and then materializes it, and the mirror receives the same
filter from the same rendering, so the two forms drop the same items. A flat grammar has no
sampler of its own.

A struct with text rules samples again when a rule refuses the sampled value, at most
`ARBITRARY_REJECTION_ATTEMPTS` (8) times, and then answers `arbitrary::Error::IncorrectFormat`.
A content token samples one canonical spelling, so a rule that refuses that spelling refuses every
retry too. An optional field whose type is such a struct is therefore sampled as absent when the
struct cannot be sampled: a rule that refuses most values never starves the Node that holds it.

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

The extension reuses these codes for its own forms: `RCA3144` for a `classify(...)` that cannot be
read or a declaration that mixes `pair(...)` with another test, `RCA3145` for a `classify(...)`
that is not on an enum, `RCA3146` for an unknown label or variant, `RCA3147` for `once(A, A)`,
and `RCA3149` for a removed variant that no classification maps a text to. It adds two:

| Code | Failure |
|---|---|
| `RCA3154` | A variant is classified twice, or one classification maps one text twice. |
| `RCA3155` | A text rule names a variant that no classification of the enum classifies. |

The struct form reuses them: `RCA3144` for a struct classification with `text(...)` mappings or an
enum classification without a variant, `RCA3145` for a pair, a `once` set or a `Variant.field`
classification on a struct, `RCA3154` for a second classification of one struct, and `RCA3155`
for a text rule that names a field no classification of the struct names. A field that does not
exist or has another shape is a generation error, because its shape is known only once the Nodes
are planned.

A rule on an enum that no repetition field uses is accepted silently. It costs nothing, and a
grammar under migration reaches that state on the way to its first list.

## What stays the same

Diagrams, `Pretty`, `Visit` and the Explorer are untouched: a rejection rule constrains which
values a parse produces, not what a value holds or how it renders. The one generated addition to
documentation is a single rustdoc line on the enum listing its rules, their messages and its
classifications. The rules of a list need no ADR: they add authoring surface and a runtime
failure class, and change no decision this project recorded. A validation hook runs consumer code
inside the parse, and ADR 0013 records that decision.
