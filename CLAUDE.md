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

## Code Style

- **Use method syntax, not UFCS.** Write `T::parse(input, rules)` not `<T as Parse>::parse(input, rules)`.
- **Derive Parse/Scan/Visit wherever possible.** Manual impls only when the derive macro can't handle the case. Document why with a comment.
