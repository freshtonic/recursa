# recursa

**Derive recursive descent parsers — and pretty-printers, and visitors — directly from your Rust types.**

Recursa is a parsing framework where the AST *is* the grammar. You declare your syntax as ordinary Rust structs and enums, slap on `#[derive(Parse)]`, and the parser falls out. No `.lalrpop` file. No `.pest` file. No build script. No separate token tree to keep in sync with the types you actually want to work with.

```rust
#[derive(Parse, Visit, FormatTokens)]
#[parse(rules = SqlRules)]
struct SelectStmt {
    select_kw: SelectKw,
    columns:   Seq<Column, Comma>,
    from:      Option<FromClause>,
    where_:    Option<WhereClause>,
}
```

That declaration is your grammar, your AST, your visitor target, and your pretty-printer source — all at once. Change a field, and every layer updates with it.

## Why recursa over the alternatives?

The Rust ecosystem already has good parser tooling. Here's what's missing from each, and what recursa does instead.

### vs. `lalrpop`, `pest`, `tree-sitter`, `nom`, `chumsky`

| | external grammar file | runtime parse tables | AST is hand-written separately | hand-rolled lexer required | derive-based |
|---|:---:|:---:|:---:|:---:|:---:|
| **lalrpop**     | yes | yes | yes | yes | no  |
| **pest**        | yes | yes | yes | no  | no  |
| **tree-sitter** | yes | yes | yes | no  | no  |
| **nom**         | no  | no  | yes | yes | no  |
| **chumsky**     | no  | no  | yes | yes | no  |
| **recursa**     | **no** | **no** | **no — your types *are* the grammar** | **no — scannerless** | **yes** |

With LALRPOP or pest, you write the grammar twice: once in the DSL, then again as Rust types you map onto in actions. The two drift. With nom and chumsky, you build a parser by composing combinators — clever, but the AST is still a separate thing you wire by hand, and the resulting types are anonymous tuples until you map them.

Recursa borrows the trick that makes `syn` so pleasant to use — *the type is the parser* — and pushes it all the way to a full grammar framework with Pratt expressions, separated lists, delimited groups, multi-token lookahead, and rich diagnostics.

### vs. `syn`'s `Parse` trait

`syn::Parse` is the inspiration, but you have to **write the impl by hand**. Every field. Every variant. Every lookahead. For a Rust-macro-sized grammar that's fine; for SQL it's a nightmare.

Recursa makes `Parse` *derivable*. The macro analyzes your fields, computes first-sets, builds a combined peek regex per enum, caches it in a `OnceLock`, and emits the recursive descent code for you. You only ever touch the type definition.

## The three pillars

### 1. No manual parsing. Ever.

A manual `Parse` impl in a recursa codebase is a code smell — it means the derive macro has a gap that should be fixed upstream, or your AST shape doesn't match the grammar and needs restructuring. The framework gives you enough building blocks (`Seq<T, S>`, `Surrounded<L, T, R>`, `Option<T>`, `Box<T>`, Pratt enums, scannerless tokens) that you almost never need an escape hatch.

```rust
#[derive(Parse)]
#[parse(rules = SqlRules)]
struct FuncCall {
    name: Ident,
    args: Surrounded<LParen, Seq<Expr, Comma>, RParen>,  // delimited, separated, no manual fiddling
}
```

The `Surrounded` and `Seq` helpers carry their own parse logic. The peek regex for an enum is built from the union of its variants' first-sets. Disambiguation is longest-match-wins with declaration order as tiebreaker. You think in *types*, not in parse cursors.

### 2. A built-in visitor pattern with `#[derive(Visit)]`

Most parser generators stop at "here's your tree, good luck." Recursa ships a typed visitor framework that's derived alongside the parser:

```rust
#[derive(Parse, Visit)]
#[parse(rules = SqlRules)]
struct SelectStmt { /* ... */ }

struct ColumnCounter { count: usize }

impl Visitor<Column> for ColumnCounter {
    type Error = Infallible;
    fn enter(&mut self, _: &Column) -> ControlFlow<Break<Infallible>> {
        self.count += 1;
        ControlFlow::Continue(())
    }
}
```

Key features the others don't bundle:

- **`#[derive(Visit)]`** walks every child field automatically — no manual recursion, no missed branches when you add a variant.
- **`SkipChildren` control flow** lets a visitor enter a node, decide it doesn't care about the subtree, and bail out cheaply.
- **Type-safe downcasting** via `NodeKey<'ast>` — store nodes of any type as a hashmap key without losing type information, then recover the original `&T` with `get_as::<T>()`.
- **`TotalVisitor`** dispatches to per-type `Visitor<N>` impls based on `TypeId`, so adding a hook for a new node type is one `impl` block, not a giant match arm in a god-trait.

The visitor and the parser stay in lockstep automatically because they're derived from the *same* type definition.

### 3. Configurable code formatting via `FormatTokens`

A parser without a printer is half a tool. Anything you parse with recursa, you can also pretty-print — and the printer is configurable, not just `Debug`.

```rust
pub trait FormatTokens {
    fn format_tokens(&self, tokens: &mut Vec<Token>);
}
```

Implementations emit a stream of Wadler-style IR tokens — `String`, `Break { flat, broken }`, `Begin(GroupKind)`, `End`, `Indent`, `Dedent` — which a `PrintEngine` then lays out against a `FormatStyle`:

```rust
pub struct FormatStyle {
    pub max_width: usize,         // wrap point
    pub indent_width: usize,      // spaces per level
    pub uppercase_keywords: bool, // SELECT vs select
    pub leading_commas: bool,     // ", col" vs "col,"
}
```

The engine supports both **consistent** groups (all-or-nothing breaks, like Lisp pretty-printing) and **inconsistent** groups (each break decides independently, like fill-paragraph). Punctuation attachment (`,`, `;`, `)`, `.`) is automatic. Style is decided at runtime, so the same AST renders as a tight one-liner or a wrapped, indented block depending on the `FormatStyle` you hand the printer — no second pass over the tree.

Compare this to most parser-generator ecosystems where pretty-printing is a separate library you have to write yourself, often without access to the original whitespace or comment information.

## Other things recursa gets right

- **Scannerless parsing.** No separate lexer phase to keep in sync with the grammar. Tokens are types like `LetKw` or `Ident(String)`, scanned on demand from a regex pattern attached to the type. Whitespace is governed by a `ParseRules::IGNORE` regex per language.
- **Pratt expressions.** `#[parse(pratt)]` on an enum gives you operator precedence with prefix, infix, and postfix variants — no precedence table to maintain in a separate file.
- **Multi-token lookahead.** Combined peek regexes are built from variant first-sets and cached in a `OnceLock`. Compiled once, used forever.
- **`miette` diagnostics.** Source spans, breadcrumb context, and aggregated "expected one of …" expectations come out of the box.
- **Bulk token declaration.** `keywords!`, `punctuation!`, and `literals!` macros let you declare hundreds of tokens in a few lines — useful for real grammars like SQL.

## Quick example

```rust
use recursa::{Input, Parse, ParseRules, Scan, Visit};

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

struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

#[derive(Parse, Visit, Debug)]
#[parse(rules = Lang)]
struct LetStmt {
    let_kw: LetKw,
    name:   Ident,
    eq:     Eq,
    value:  IntLit,
    semi:   Semi,
}

let mut input = Input::new("let x = 42;");
let stmt = LetStmt::parse(&mut input, &Lang).unwrap();
assert_eq!(stmt.name.0, "x");
assert_eq!(stmt.value.0, "42");
```

That single declaration gives you: a parser, a visitor target, and (with `#[derive(FormatTokens)]`) a pretty-printer. Three derives, zero hand-written parsing code, zero external grammar file.

## Workspace

| Crate | Description |
|---|---|
| [`recursa`](.) | Facade crate — re-exports everything |
| [`recursa-core`](recursa-core) | Core traits (`Parse`, `Scan`, `Visit`, `Visitor`, `FormatTokens`), types (`Input`, `Seq`, `Surrounded`, `NodeKey`), the Wadler print engine, error handling |
| [`recursa-derive`](recursa-derive) | Derive macros (`#[derive(Parse)]`, `#[derive(Scan)]`, `#[derive(Visit)]`, `#[derive(FormatTokens)]`) |

The `pg-sql` crate in this workspace is a real-world stress test: a derived PostgreSQL parser exercising every feature.

## License

MIT
