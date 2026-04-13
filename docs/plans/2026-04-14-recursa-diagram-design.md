# recursa-diagram: Railroad Syntax Diagrams in Rustdoc

**Date:** 2026-04-14
**Status:** Design approved, ready for implementation planning

## Goal

Generate attractive railroad syntax diagrams for `Parse`-derived AST nodes and embed them directly in rustdoc output, so that every documented AST type carries a visual grammar reference alongside its prose.

## High-Level Approach

A new workspace crate `recursa-diagram` provides an attribute macro `#[railroad]` that, when applied to a type, walks the type's immediate structure, builds a railroad diagram, serializes it to inline SVG, and emits it as a `#[doc = "<svg>...</svg>"]` attribute on the type. Rustdoc passes HTML through doc comments, so the diagram renders inline on the type's documentation page with no external files, no build step, and no path resolution.

Each type gets its own diagram. References to other diagrammed types render as labeled boxes that link (via intra-doc relative paths) to the referenced type's rustdoc page. This matches how real-world railroad grammars compose (EBNF, SQLite docs, tabatkins examples) and sidesteps recursion: the macro walks only one level deep.

No CLI tool is produced. Diagrams live exclusively in rustdoc; `cargo doc --open` is how you view them.

## Crate Structure and Public Surface

**New crate:** `recursa-diagram`, sibling to `recursa-core` and `recursa-derive`.

Contents:

- Attribute macro `#[railroad]`.
- Pure-Rust implementation of the tabatkins railroad layout algorithm, producing self-contained SVG as a `String`.
- Helper attribute forms: `#[railroad(label = "...")]`, `#[railroad(skip)]`.

Usage in pg-sql:

```rust
#[railroad(label = "SELECT")]
pub struct SelectKw;                  // leaf; diagram shows one "SELECT" box

#[railroad]
pub struct SelectStmt {
    select_kw: SelectKw,              // box labeled "SelectKw", links to its page
    columns: Seq<Column, Comma>,      // expanded as a loop with Comma separator
    from_clause: Option<FromClause>,  // expanded as a skip branch
}
```

**Crate boundaries.** `recursa-diagram` performs its own `syn` analysis of the annotated type. It does not depend on `recursa-derive` and is not depended on by it. Users add both `#[derive(Parse)]` and `#[railroad]` to types where they want diagrams. `recursa-derive` is untouched by this work.

**No feature gating** in the initial version. The macro always expands to a doc attribute. Downstream crates can gate `#[railroad]` behind `cfg_attr` if they need to skip generation in some builds. A central feature flag can be added later if doc-build cost becomes a problem.

## Macro Behaviour

When `#[railroad]` is applied to a type, the macro walks the type's AST one level deep, builds an in-memory railroad tree, serializes it to SVG, and emits it as a `#[doc = "..."]` attribute on the type.

### Structural rules

- **Struct** → sequence of its fields, in declaration order.
- **Enum** → choice block with one branch per variant. Per the project's single-field-tuple convention (see `CLAUDE.md`), each variant wraps exactly one inner type, which the diagram renders as a reference.
- **Unit struct with `#[railroad(label = "...")]`** → a single terminal box containing the literal text.
- **Unit struct without `label`** → a single non-terminal box labeled with the type name.

### Combinator recognition

Recognized syntactically by matching the outermost path segment of the field type:

| Type pattern              | Railroad shape                                 |
|---------------------------|------------------------------------------------|
| `Option<T>`               | skip branch around T                           |
| `Seq<T, Sep>`             | one-or-more loop: T with Sep on the back-edge  |
| `Surrounded<L, I, R>`     | sequence: L → I → R                            |
| `Box<T>`, `Rc<T>`, `Arc<T>` | transparent — unwrap to T                    |
| anything else             | labeled non-terminal box, linking to that type |

**Pratt enums** are treated as regular enums: a flat choice over variants, with a note in the doc string that operator precedence is not depicted. Custom precedence visualization is out of scope.

### Field and variant attributes

- `#[railroad(label = "...")]` on a field or variant — overrides the auto-generated label for that reference.
- `#[railroad(skip)]` on a field — omits it entirely. Escape hatch for fields that exist for parser bookkeeping but shouldn't appear in the grammar.
- Conflicting attributes (`label` + `skip` on the same field) produce a `compile_error!` with a clear message. This is the only hard error the macro emits.
- Unknown type patterns fall through to the "labeled non-terminal box" rule. The macro never fails to compile on unrecognized input.

### Label scoping

Because a proc macro can only see field types as syntax, the macro expanding on `SelectStmt` has no access to the `#[railroad(label = "...")]` attribute on the `SelectKw` type definition. In practice:

- A terminal type annotated with `label = "SELECT"` renders as a "SELECT" box on its own rustdoc page.
- In any composite type that references `SelectKw`, the reference renders as a "SelectKw" box (a rustdoc intra-doc link) — the reader clicks through to see the literal.
- The optional per-field `#[railroad(label = "...")]` is the escape hatch for composites where the type-name-link style reads poorly. Applied at the use site, not the definition site.

## Layout Algorithm and SVG Output

Direct port of the tabatkins railroad-diagrams layout algorithm to Rust. The algorithm is small (~800 lines of JS in the reference implementation) and well-specified.

### Primitives

Mirror the reference implementation:

- `Terminal(text)` — rounded-rect box with literal text.
- `NonTerminal(text, href)` — rectangular box, optionally an intra-doc link.
- `Sequence(children)` — horizontal concatenation with connecting tracks.
- `Choice(default_idx, children)` — vertical stack with entry/exit rails.
- `Optional(child)` — choice between `child` and a skip line.
- `OneOrMore(child, separator)` — child on the forward track, separator on the back-edge.
- `ZeroOrMore(child, separator)` — optional wrapping one-or-more.

Each primitive computes `width`, `height`, `up`, `down` at construction time. SVG emission is a second pass writing `<path>`, `<rect>`, and `<text>` elements with absolute coordinates.

### SVG output

Output is a self-contained `<svg xmlns="http://www.w3.org/2000/svg">...</svg>` element with inline `<style>` for fonts and stroke. No external CSS, no external fonts, no dependencies — rustdoc renders it identically in any environment. A trailing `<!-- railroad -->` comment marks each block so it's identifiable in rendered output.

### Intra-doc links

Non-terminal boxes emit `<a href="TypeName.html">...</a>` wrappers. Rustdoc does not rewrite raw HTML hrefs the way it rewrites markdown `[links]`, so the macro resolves references as relative paths within the same module (`ChildType.html`). For types in other modules, the macro emits a best-effort relative path (`../other_mod/ChildType.html`) computed from the `syn::Path` of the field type.

### No runtime dependency

`recursa-diagram` depends on no `svg` or `railroad` crate. Layout and serialization live in-crate. This keeps proc-macro compile cost low and avoids pulling unrelated code into pg-sql's doc builds.

## Testing Strategy

1. **Layout unit tests** in `recursa-diagram/tests/`. Construct railroad trees programmatically (e.g. `Sequence(vec![Terminal("SELECT"), NonTerminal("Column")])`), render to SVG, compare against committed `.svg` fixtures. Snapshot-style; diffs are readable in review.
2. **Macro expansion tests** using `trybuild` or `macrotest`. Apply `#[railroad]` to representative types — unit struct, struct with combinators, enum, Pratt-style enum, `skip`-annotated field — and assert the expanded doc attribute matches a fixture.
3. **Visual smoke test**. Run `cargo doc -p pg-sql` on a few annotated types and confirm rustdoc output contains the expected `<svg>` blocks. Catches regressions in the full pipeline.

## Rollout Plan

1. Land `recursa-diagram` with the layout algorithm and unit tests. No macro yet.
2. Add the `#[railroad]` attribute macro, tested in isolation on toy types.
3. Apply to a handful of pg-sql types — `SelectStmt`, a Pratt `Expr`, a `Seq`-heavy type — and review rendered output by eye with `cargo doc --open`.
4. Extend pg-sql's keyword-defining macro to inject `#[railroad(label = "...")]` on every keyword type automatically.
5. Sweep `#[railroad]` across the rest of pg-sql's AST.

## YAGNI Cuts and Open Questions

- **No CLI.** `pg-sql diagram ::some::ast::Node` is dropped. Diagrams live in rustdoc.
- **No feature flag on `recursa-diagram`.** Add later if doc-build cost becomes a problem.
- **No precedence visualization for Pratt enums.** Flat choice plus a doc note is the agreed compromise.
- **No diagrams for types outside the Parse ecosystem.** The macro targets Parse-style AST types specifically.
- **No fully-inlined recursive expansion.** One level deep, always.
