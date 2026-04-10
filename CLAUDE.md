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

## Enum Variant Ordering Matters

For `Parse`-derived enums, **variant declaration order affects disambiguation**. The combined peek regex uses longest-match-wins with declaration order as tiebreaker. When two variants could match the same input, the one declared first wins if both match the same length.

**Put more specific (longer) variants before less specific (shorter) ones:**

```rust
// Correct: NOT TRUE (2 tokens) listed before TRUE (1 token)
#[derive(Parse)]
#[parse(rules = MyRules)]
enum BoolTest {
    IsNotTrue(IsNotTrue),    // matches "NOT TRUE" (longer)
    IsNotFalse(IsNotFalse),  // matches "NOT FALSE"
    IsTrue(IsTrue),          // matches "TRUE" (shorter)
    IsFalse(IsFalse),        // matches "FALSE"
}

// Wrong: TRUE listed first would match before NOT TRUE
enum BoolTest {
    IsTrue(IsTrue),          // would incorrectly match the TRUE in "NOT TRUE"
    IsNotTrue(IsNotTrue),    // never reached for "NOT TRUE"
}
```

The same applies to any enum where variants share a common prefix — list the longer/more-specific variant first.

## Manual Parse Impls Are a Red Flag

A manual `Parse` impl means either:

1. **Recursa has a gap** — the derive macro can't express the required parsing pattern. This should be filed as a limitation to fix in recursa.
2. **The AST design needs a rethink** — the type structure doesn't fit recursa's model. Restructure the types so they can be derived.

Every manual `Parse` impl must have a comment explaining which of these applies and what would be needed to eliminate it. Treat manual impls as tech debt, not as a normal pattern.

## Always Use Surrounded for Delimited Groups

Any content enclosed in matching delimiters — `( ... )`, `[ ... ]`, `{ ... }`, `< ... >` — must use `Surrounded<Open, Inner, Close>`. Never store open/close delimiter tokens as separate struct fields.

```rust
// Correct
pub struct FuncArgs {
    pub args: Surrounded<LParen, Seq<Expr, Comma>, RParen>,
}

// Wrong: separate delimiter fields
pub struct FuncArgs {
    pub lparen: LParen,
    pub args: Seq<Expr, Comma>,
    pub rparen: RParen,
}
```

## No Manual Clone or Debug Impls

Always use `#[derive(Clone)]` and `#[derive(Debug)]`. If derive doesn't work, it means a field type or variant type is missing Clone or Debug — fix the dependency, don't write a manual impl.

The same applies to `PartialEq`, `Eq`, `Hash`. If a type should be comparable or hashable, derive it and ensure all constituent types also derive it.

## Code Style

- **Use method syntax, not UFCS.** Write `T::parse(input, rules)` not `<T as Parse>::parse(input, rules)`.
- **Derive Parse/Scan/Visit wherever possible.** Manual impls only as a last resort (see above). Document why with a comment.
