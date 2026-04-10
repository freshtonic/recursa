# Recursa Development Guidelines

## Enum Variant Convention

All `Parse`-derived enum variants must be single-field tuple variants holding a type that itself implements `Parse`:

```rust
// Correct: each variant wraps a single Parse type
#[derive(Parse)]
#[parse(rules = MyRules)]
enum Statement {
    Select(SelectStmt),
    Insert(InsertStmt),
}

// Wrong: struct-like variants are rejected by the derive macro
enum Statement {
    Select { columns: Vec<Column>, from: FromClause },
}

// Wrong: multi-field tuple variants are rejected
enum Statement {
    Select(SelectKw, Vec<Column>, FromClause),
}
```

When an enum variant needs to hold multiple fields, wrap them in a struct that derives `Parse`:

```rust
#[derive(Parse)]
#[parse(rules = MyRules)]
struct SelectStmt {
    select_kw: SelectKw,
    columns: Seq<Column, Comma>,
    from_clause: Option<FromClause>,
}

#[derive(Parse)]
#[parse(rules = MyRules)]
enum Statement {
    Select(SelectStmt),
}
```

This principle also applies to Pratt enum atoms. Prefix, infix, and postfix variants have specific field layouts (see derive macro docs).

## Manual Parse Impls Are a Red Flag

A manual `Parse` impl means either:

1. **Recursa has a gap** — the derive macro can't express the required parsing pattern. This should be filed as a limitation to fix in recursa.
2. **The AST design needs a rethink** — the type structure doesn't fit recursa's model. Restructure the types so they can be derived.

Every manual `Parse` impl must have a comment explaining which of these applies and what would be needed to eliminate it. Treat manual impls as tech debt, not as a normal pattern.

## Code Style

- **Use method syntax, not UFCS.** Write `T::parse(input, rules)` not `<T as Parse>::parse(input, rules)`.
- **Derive Parse/Scan/Visit wherever possible.** Manual impls only as a last resort (see above). Document why with a comment.
