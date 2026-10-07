# Declared build configurations — Design

Status: accepted design, 2026-09-24. Raised by
[freshtonic/recursa#135](https://github.com/freshtonic/recursa/issues/135), for the consumer
[freshtonic/pg-sql#92](https://github.com/freshtonic/pg-sql/issues/92) and its ADR 0010.

A grammar that follows a language through its releases keeps some elements out of an older
build. Recursa already does the removal with `#[cfg(...)]`: generation evaluates the predicate
against the Cargo configuration and drops the item. The build then knows what it accepts, but
nothing can say which configuration a parsed value needs, because the predicate is gone.

A declared gate keeps both halves. `#[config(since = NAME)]` removes its element from a build
that excludes it, exactly as the `cfg` does, and records that the element needs `NAME`. From
that record, generation emits a `Requires` implementation per Node, and a walk over one parsed
value answers the two questions a consumer asks.

Recursa knows no language and no release name. The grammar names its own configurations, and a
gate names one of them.

## The configuration list

The root declaration carries one ordered list:

```rust,ignore
recursa::grammar! {
    module = crate,
    configurations(
        pg14,
        pg15 = feature("since-pg15"),
        pg16 = feature("since-pg16"),
        pg17 = feature("since-pg17"),
    ),
    derives(Pretty, Visit, Requires),
}
```

The order is the meaning: a later name is a later, more demanding configuration. The first
name is the baseline and takes no feature, because every build admits it. Each later name
gives the Cargo feature that enables it, so the removal half keeps using the same features the
`cfg` predicates use today, and a consumer migrates one gate at a time.

Generation evaluates each feature and requires the enabled names to form a prefix of the list.
A configuration includes every earlier one, so `pg17` enabled while `pg16` is not is an
authoring error, not a second dimension. **The build states its configuration** through the
highest enabled name, and generation publishes it as `Configuration::BUILD`. No separate
setting says which configuration a build is; the features already say it.

The list produces one generated type in the grammar module:

```rust,ignore
pub enum Configuration { Pg14, Pg15, Pg16, Pg17 }

impl Configuration {
    pub const ALL: [Self; 4];
    pub const BUILD: Self;
    pub const fn name(self) -> &'static str;
    pub const fn id(self) -> recursa::Configuration;
    pub const fn from_id(id: recursa::Configuration) -> Option<Self>;
}
```

`recursa::Configuration` is the grammar-independent ordinal that the runtime compares. The
generated enum is the name the consumer reads and writes.

## The gate

One attribute covers every element that takes a `cfg` today:

```rust,ignore
#[config(since = pg16)]                      // exists from pg16
#[config(before = pg17)]                     // exists before pg17
#[config(since = pg16, before = pg18)]       // exists from pg16 and before pg18
```

It is accepted on an `ast_node!` struct, enum, field or variant, and on a `tokens!` keyword,
matcher, literal, lexer token, lookahead entry or precedence name. `#[config_attr(BOUNDS,
META, ...)]` gates attributes instead of the element, the way `cfg_attr` does, which is how a
scoped LR resolution or an alternative `#[tok(...)]` is gated.

`since` both removes and records. `before` only removes: inside a build where the element
still exists, an upper bound demands nothing, and a build that is newer than the bound does
not hold the element at all. So a `before` gate never reaches a parsed value.

A gate may carry message data and a citation:

```rust,ignore
#[config(
    since = pg16,
    on = absent,
    code = "42601",
    message = "subquery in FROM must have an alias",
    hint = "For example, FROM (SELECT ...) [AS] foo.",
    cite = "gram.y REL_15_19, table_ref: select_with_parens opt_alias_clause",
)]
```

`code`, `message` and `hint` are opaque to Recursa: they are three strings that the consumer
gives its own meaning. `cite` records the source that justifies the gate.

## The shape rule

A newer configuration sometimes makes optional what an older one required. Then the
requirement belongs to the absence of a value, not to its presence. `on = absent` says so, and
it applies to an `Option<T>` field, a repetition field and a `#[presence(...)]` `bool` field:
the scan records the requirement when the field holds no value.

The element is still removed below the bound, because the two configurations give the field
two different types. The complete declaration is the same pair of opposite predicates that the
`keywords` and `matchers` sections already use:

```rust,ignore
pub struct ParenQueryRef {
    pub body: ParenTableBody,
    #[config(since = pg16, on = absent, code = "42601", message = "...", cite = "...")]
    pub alias: Option<PlainTableAlias>,
    #[config(before = pg16)]
    pub alias: PlainTableAlias,
}
```

In a `pg16` build the field is optional, and a `None` reports `pg16` with the message. In a
`pg15` build the field is required, and the parser rejects the same source, which is the
behaviour that build must have. One declaration therefore covers both halves of the shape
rule, and no new kind of declaration was needed.

## What generation records

A requirement belongs to one of three sites: a Node type, an enum variant, or a field. Those
are the sites a walk over a parsed value can find, and they are exactly what the consumer
needs. Generation keys its plan by them, computes the ceilings described below, and emits,
beside each Node, a static table of notes:

```rust,ignore
static __RECURSA_REQUIREMENTS_7: [recursa::RequirementNote; 1] = [
    recursa::RequirementNote::new(
        2, "pg16", "ParenQueryRef.alias", true,
        Some("42601"), Some("subquery in FROM must have an alias"), Some("For example, ..."),
        Some("gram.y REL_15_19, ..."),
    ),
];
```

A gate on a `tokens!` entry removes its entry and records nothing. A parsed value cannot tell
two same-named lexical entries apart — that is the whole point of writing them with opposite
predicates — so a lexical requirement is not a property of the value's shape. A new keyword
almost always arrives together with a gated Node, field or variant, which does record.

## The trait

```rust,ignore
pub trait Requires {
    const REQUIREMENT_CEILING: Configuration;

    fn minimum_requirement(&self) -> Option<Requirement>;
    fn requirement_above(&self, floor: Configuration) -> Option<Requirement>;
}
```

`Parsed<T>` has the same two methods, and they keep the span, because a span lives in parse
provenance and not in the value. `ArenaParsed<F>` has the same pair, for the same reason: an
`arena_ast` grammar returns that wrapper instead, and a consumer of such a grammar needs the
extent just as much. Both wrappers run the one shared scan from their own root occurrence
cursor, so neither holds a second copy of the walk. The trait's own methods answer for a
detached value and report no span.

A `Requirement` carries the static note and the extent of the construct.

### The flat form

A `flat` grammar parses into pools of four-byte handles and holds no value of a Node type, so it
cannot implement the trait. Its `FlatAst` answers the same two questions with the same two
methods instead:

```rust,ignore
let flat = Statement::parse_flat(&mut input)?;
let minimum = flat.minimum_requirement();
let rejected = flat.requirement_above(grammar::Configuration::Pg15.id());
```

A `FlatAst`'s root Node is a type parameter, so one generated trait, `FlatRequires`, names the
walk and the ceiling of that root, exactly as `FlatNodeRef` names its `NodeRef` variant. The
fold, the ceiling test and the answer are the runtime's own, so the third storage adds no second
copy of them either, and the generated ceiling is the same literal the nested Node carries.

One thing differs, and it is the projection of the extent. A value walk takes each extent from
the occurrence sidecar. A flat result has no sidecar: it keeps one span per Node and per text and
nothing else (`adr/0008-keep-flat-asts-semantic-only.md`), so the flat walk takes each extent
from the stores. The two agree on every site but one:

| Site | Extent |
|---|---|
| A Node, a variant, a present value | The Node's own span, or a content token's text span. |
| A missing optional value | The span of the Node that holds the field, as in the nested forms. |
| A repetition | From the first item's start to the last item's end; an empty run has none. |
| An element with attached fixed syntax | **Differs.** The nested forms report the element, fixed tokens included. The flat form reports the value, because no flat store holds a fixed token. |

A presence field is the same case as the last row: `#[presence(...)]` stores a `bool`, so the
flat form reports no extent for it at all. The configuration, the site and the message data are
the same in every row.

Generation also prunes the flat walk at generation time. An application whose ceiling is the
baseline can hold no requirement, so neither its walk nor the call to it is emitted; the nested
walk makes the same decision at run time, with one comparison.

### Why two methods and one walk

The consumer asks two different questions. "What is the lowest configuration that accepts this
value?" is a maximum over the whole value. "Which construct does my older target reject
first?" is a minimum over source position, above a floor. One is not a special case of the
other: a value can need `pg17` because of a construct that is late in the source while an
earlier construct needs `pg16`.

Both are folds of one pre-order walk in authored source order, and the walk is generated once:

- `requirement_above(floor)` keeps the first construct above `floor` and stops.
- `minimum_requirement()` keeps a running maximum and the earliest construct that reached it.
  A later construct at the same configuration never replaces an earlier one, which is what
  "the first construct that sets it" means.

### Why the walk is cheap

Generation computes, by a fixpoint over the reachable type graph, the highest requirement each
Node type can hold, and emits it as `REQUIREMENT_CEILING`. The scan enters a subtree only when
that ceiling can still change the answer:

- above the floor already answered, nothing counts;
- while looking for the minimum, a ceiling at or below the best so far cannot win, because a
  tie keeps the earlier construct.

A grammar region with no gate therefore has a baseline ceiling and costs one comparison,
whatever its size. That matters, because the common answer is "nothing is above the target",
and the common answer must not pay for the whole tree. A generic Node takes the grammar's
maximum, because its type arguments are not known at that point; that loses pruning at the
parent only, never soundness, and the type argument's own ceiling is still exact.

The ceiling is a generated literal, not a computed associated constant. A grammar is a cyclic
type graph, and a constant that reads another type's constant of the same name cannot be
written for a cycle. Generation solves the fixpoint once instead.

### Why not `Visit`

`Visit` reaches every Node, but its events name only the Node type. It has no field identity,
so it cannot see which optional field is absent, and it has no cursor, so it cannot report a
span: the `Visit` contract is defined on detached values. The scan therefore carries a
`ViewCursor`, the same occurrence cursor the generated views and `Pretty` use, with the same
arithmetic: element `e` of the active shape is `cursor.child(e)`, a stored value is one level
below it, and repetition item `i` is `cursor.child(e).child(i)`.

## What stays the same

`#[cfg(...)]` and `#[cfg_attr(...)]` keep working everywhere they work today, with the same
diagnostics. A grammar mixes both while it migrates. A grammar that declares no
`configurations(...)` list behaves exactly as before, and a `config` gate in such a grammar is
an error, not a silent pass.
