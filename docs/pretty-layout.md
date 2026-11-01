# Pretty layout

A grammar that derives `Pretty` controls its layout in three places: the grammar settings, the
`#[pretty(...)]` hints on Nodes, variants and fields, and the `spacing` of each fixed token. The
caller of `render_with` gives the width and the indentation.

## How layout works

Generated `Pretty` code queues a layout program: texts, breaks, groups and indentation. Layout
then decides each group once:

- A **consistent group** (`group = consistent`) breaks all of its breaks, or none of them. It
  breaks when its flat text does not fit in the width that is left on the line.
- An **inconsistent group** (`group = inconsistent`) decides each break on its own, as a fill.
- A break belongs to the innermost group around it.
- Indentation (`indent`) has an effect only on the line breaks inside it. One level is the
  `indent` of the configuration.

Each Node has its own consistent group (its *baseline group*), with these exceptions:

- A Node with `#[pretty(group = none)]` has no group. Its breaks belong to the group around it.
- With `breaks = hinted`: a Node that a local group (a `group` hint) has just opened around, with
  only indentation between them, has no group of its own. The local group decides the breaks of
  that Node. The Nodes inside it, and a Node that starts after other output in the local group,
  have their own groups.
- With `breaks = any`: no Node inside a local group has a group of its own, so the local group
  decides each space between two tokens in it.

## Grammar settings

```rust,ignore
recursa::grammar! {
    module = crate,
    derives(Pretty),
    pretty(max_width = 80, indent = 2, breaks = hinted),
}
```

| Setting | Default | Effect |
|---|---|---|
| `max_width` | `88` | The width when the caller gives no configuration |
| `indent` | `2` | The spaces of one indentation level when the caller gives no configuration |
| `breaks` | `any` | `any`: the space between two tokens is a soft break of the group around it. `hinted`: that space never breaks |

With `breaks = any`, a Node that does not fit puts each of its tokens on its own line. That keeps
the width, but it makes poor text for a language with long statements, such as SQL. With
`breaks = hinted`, a line breaks only at a break that a hint puts, at a boundary between the items
of a repetition, and inside a delimited sequence such as `( ... )`. A line that has no such break
can be wider than the width.

## Hints

`#[pretty(...)]` goes on a Node, on a variant, or on a field.

| Hint | Where | Effect |
|---|---|---|
| `group = consistent` | Node, variant, field | A consistent group around the fragment |
| `group = inconsistent` | Node, variant, field | An inconsistent group around the fragment |
| `group = none` | Node | The Node has no group: its breaks belong to the group around it |
| `indent` | Node, variant, field | The breaks inside the fragment are one level in |
| `break_before = soft` | Node, variant, field | A break before the fragment: a space when its group is flat |
| `break_before = hard` | Node, variant, field | A line break before the fragment |
| `break_before = space` | Node, variant, field | One space before the fragment, which never breaks |
| `break_after = ...` | Node, variant, field | The same, after the fragment |
| `separator = leading` | Repeated field | A broken separator starts the next line |
| `trailing_separator = always` | Repeated field with `#[sep(..., trailing)]` | Write the trailing separator |

A field hint covers the field and the fixed tokens attached to it (`#[tok(KW, this)]`). A hint on
an optional or a repeated field has no effect when the field is absent or empty.

### Where the breaks go

A break before a fragment is inside the group and the indentation of the same hint. A break after
it is inside the group but outside the indentation:

```text
#[pretty(group = consistent, indent, break_before = soft)]   group(indent(break fragment))
#[pretty(group = consistent, break_after = soft)]            group(fragment break)
#[pretty(break_before = soft)]                               break fragment
```

So `group = consistent, indent, break_before = soft` on a list makes the list decide whether it
starts on the next line, one level in. Without a group or indentation, a break belongs to the group
around the fragment.

Two breaks with no token between them are one break. A break before the first token of a
rendering does nothing. A flat break just inside an opening delimiter, or just before a closing
delimiter, writes no space.

## Fixed-token spacing

A fixed token in `tokens!` takes `spacing = auto | tight | before | after | around`. A word, such as
a keyword, takes only `auto` or `around`. With `around`, an opening delimiter or punctuation after
the word gets a space (`IN (`, `RETURNING *`). A token after an opening delimiter or after a
`tight` token is never preceded by a space.

## Example

pgtk-sql renders a query with one clause on each line when the query does not fit on one line:

```rust,ignore
recursa::ast_node! {
    #[pretty(group = none)]
    #[tok(SELECT, this)]
    pub struct SelectStmt {
        pub head: Option<SelectHead>,
        #[pretty(break_before = soft)]
        pub where_clause: Option<WhereClause>,
    }
}

recursa::ast_node! {
    #[pretty(group = none)]
    pub struct SelectTargetList {
        #[sep(COMMA)]
        #[pretty(group = consistent, indent, break_before = soft)]
        pub items: one_or_many!(SelectItem),
        #[pretty(break_before = soft)]
        pub from_clause: Option<FromClause>,
    }
}
```

```text
SELECT
  e.id,
  e.first_name,
  d.name AS department
FROM employees e JOIN departments d ON d.id = e.department_id
WHERE e.salary > 50000
ORDER BY e.salary DESC
```

The breaks before `FROM` and `WHERE` belong to the query, because `SelectStmt` and
`SelectTargetList` have no group. The target list has its own group, so it breaks only when it
does not fit after `SELECT`.
