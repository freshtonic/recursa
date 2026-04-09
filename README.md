# recursa

Derive recursive descent parsers from Rust types.

Recursa is a framework where you define your grammar as Rust structs and enums, and the parser is derived automatically. Inspired by how [`syn`](https://docs.rs/syn) derives `Parse`.

## Features

- **Derive `Parse` and `Scan`** for AST types -- structs parse fields in sequence, enums parse as choices
- **Scannerless parsing** -- lexing is driven by parse context, no separate tokenisation pass
- **Pratt parsing** for expressions with operator precedence via `#[parse(pratt)]`
- **Multi-token lookahead** with combined peek regexes built from first-set analysis
- **Zero-cost regex caching** via `OnceLock` -- patterns compiled once per type
- **Rich error diagnostics** using [`miette`](https://docs.rs/miette) with source spans, context breadcrumbs, and aggregated expectations
- **Visitor pattern** with `#[derive(Visit)]` for AST traversal, type-safe downcasting, and `SkipChildren` control flow
- **Separated lists** via `Seq<T, S>` with configurable trailing separator and emptiness policies
- **Bulk token declaration** macros: `keywords!`, `punctuation!`, `literals!`

## Workspace

This is a Cargo workspace with three crates:

| Crate | Description |
|-------|-------------|
| [`recursa`](.) | Facade crate -- re-exports everything |
| [`recursa-core`](recursa-core) | Core traits (`Parse`, `Scan`, `Visit`, `Visitor`), types (`Input`, `Seq`, `NodeKey`), error handling |
| [`recursa-derive`](recursa-derive) | Derive macros (`#[derive(Parse)]`, `#[derive(Scan)]`, `#[derive(Visit)]`) |

## Quick Example

```rust
use recursa::{Input, Parse, ParseRules, Scan, Visit};

// Define tokens
#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = "=")]
struct Eq;

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident(String);

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = r"[0-9]+")]
struct IntLit(String);

// Define grammar rules
struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

// Define AST
#[derive(Parse, Visit, Debug)]
#[parse(rules = Lang)]
struct LetStmt {
    let_kw: LetKw,
    name: Ident,
    eq: Eq,
    value: IntLit,
    semi: Semi,
}

// Parse
let mut input = Input::new("let x = 42;");
let stmt = LetStmt::parse(&mut input, &Lang).unwrap();
assert_eq!(stmt.name.0, "x");
assert_eq!(stmt.value.0, "42");
```

## License

MIT
