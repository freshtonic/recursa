# Recursa Design

A derive-macro framework for recursive descent parsers in Rust. Developers define AST node types as structs and enums; the framework derives `Parse` and `Scan` implementations automatically.

Inspired by how `syn` derives `Parse`.

## Workspace Structure

Three crates:

- **`recursa-core`** — traits (`Parse`, `Scan`, `ParseRules`), the `Input` type, error types
- **`recursa-derive`** — proc macros (`#[derive(Parse)]`, `#[derive(Scan)]`), depends on `recursa-core` for testing
- **`recursa`** — re-exports everything from `recursa-core` and `recursa-derive`

## Core Traits

### Scan

Leaf-level token matching via regex. Each token is its own type borrowing `&'input str` from the source.

```rust
trait Scan<'input>: Sized {
    const PATTERN: &'static str;
    fn from_match(matched: &'input str) -> Result<Self, ParseError>;
}
```

### Parse

Recursive descent parsing. Associated `Rules` type controls whitespace/comment handling.

```rust
trait Parse<'input>: Sized {
    type Rules: ParseRules;
    type FirstSet; // compile-time lookahead info (details deferred)
    fn peek(input: &Input<'input, Self::Rules>) -> bool;
    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError>;
}
```

### ParseRules

Configuration point for a grammar. Called between fields during struct parsing.

```rust
trait ParseRules {
    fn consume_ignored(input: &mut Input<Self>) where Self: Sized;
}
```

### Blanket Implementation

Every `Scan` type automatically implements `Parse` with `NoRules`:

```rust
struct NoRules;
impl ParseRules for NoRules {
    fn consume_ignored(_input: &mut Input<Self>) {}
}

impl<'input, T: Scan<'input>> Parse<'input> for T {
    type Rules = NoRules;
    // peek: try regex without advancing
    // parse: match regex, call from_match
}
```

This means `Scan` types are leaf parsers that don't skip whitespace — whitespace handling only applies at the `Parse` level for structs and enums.

## Derived Behaviour

### Structs (Sequence)

Parse each field in source order. Call `Rules::consume_ignored(input)` before each field. Fork input before starting; commit on success, return error on failure.

```rust
#[derive(Parse)]
#[parse(rules = MyRules)]
struct LetBinding<'input> {
    let_kw: LetKeyword,
    name: Ident<'input>,
    eq: Equals,
    value: Expr<'input>,
    semi: Semicolon,
}
```

- `peek` delegates to the first field's `peek`
- `FirstSet` equals the first field's `FirstSet`

### Enums (Choice)

Try each variant's `peek` in declaration order. Parse the first matching variant.

```rust
#[derive(Parse)]
#[parse(rules = MyRules)]
enum Statement<'input> {
    Let(LetBinding<'input>),
    Return(ReturnStatement<'input>),
    Expr(ExprStatement<'input>),
}
```

- `peek` returns true if any variant's `peek` returns true
- `FirstSet` is a union of all variants' first sets
- Error on no match collects all variants' expected tokens for a rich diagnostic

### Pratt Parsing (Expressions)

Opted in via `#[parse(pratt)]` on an enum for left-recursive grammars with operator precedence:

```rust
#[derive(Parse)]
#[parse(rules = MyRules, pratt)]
enum Expr<'input> {
    #[parse(prefix, bp = 9)]
    Neg(Minus, Box<Expr<'input>>),

    #[parse(infix, bp = 5)]
    Add(Box<Expr<'input>>, Plus, Box<Expr<'input>>),

    #[parse(infix, bp = 6)]
    Mul(Box<Expr<'input>>, Star, Box<Expr<'input>>),

    #[parse(atom)]
    Lit(IntLiteral<'input>),

    #[parse(atom)]
    Ident(Ident<'input>),
}
```

- `atom` — base cases, no recursion
- `prefix` — operator then recursive parse at given binding power
- `infix` — operator between two expressions; left/right associativity via `assoc = right` (default: left)
- `bp` — binding power; higher binds tighter

## Token Types

Each token is its own type with an associated regex:

```rust
#[derive(Scan)]
#[scan(pattern = r"let")]
struct LetKeyword;

#[derive(Scan)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident<'input>(&'input str);
```

- Unit structs discard the matched text
- Tuple structs capture it as `&'input str` (zero-copy)

### Scan Enums (Combined Regex)

Deriving `Scan` on an enum where all variants implement `Scan` generates a single combined regex:

```rust
#[derive(Scan)]
enum Keyword {
    Let(LetKeyword),
    While(WhileKeyword),
    If(IfKeyword),
}
```

Matching rules:
1. Longest match wins (maximal munch)
2. Declaration order as tiebreaker for equal-length matches

### Bulk Declaration Macros

Declarative macros to reduce boilerplate for large token sets:

```rust
keywords! {
    Let    => "let",
    While  => "while",
    If     => "if",
    Else   => "else",
    Return => "return",
    Fn     => "fn",
    Struct => "struct",
}
```

Expands each entry to a unit struct with `#[derive(Scan)]` and generates a combined `Keyword` enum.

```rust
punctuation! {
    Plus   => "+",
    Minus  => "-",
    Star   => "*",
    LParen => "(",
    RParen => ")",
    Arrow  => "->",
}
```

Same as `keywords!` but for punctuation. Patterns are auto-escaped for regex.

```rust
literals! {
    IntLiteral    => r"[0-9]+",
    StringLiteral => r#""[^"]*""#,
    Ident         => r"[a-zA-Z_][a-zA-Z0-9_]*",
}
```

Generates tuple structs with `&'input str` capture. Raw regex patterns (not escaped).

## Input Type

```rust
struct Input<'input, R: ParseRules> {
    source: &'input str,
    cursor: usize,
    rules: PhantomData<R>,
}
```

- `fork()` — creates a snapshot at the current cursor; original input is untouched until commit
- Tracks byte offset; line/column computed on demand for error spans
- Scannerless: lexing is driven by parsing context, no pre-tokenisation pass

## Error Handling

Errors are paramount. The framework uses `miette` for rich, rustc-quality diagnostics.

```rust
use miette::{Diagnostic, SourceSpan};

#[derive(Debug, Diagnostic, Error)]
#[error("expected {expected}")]
struct ParseError {
    #[source_code]
    src: String,

    #[label("found this")]
    span: SourceSpan,

    expected: String,

    #[related]
    context: Vec<ParseError>,

    #[help]
    help: Option<String>,
}
```

Design goals for error quality:

- **Source snippets** with underlined spans pointing to the exact problem location
- **Aggregated expectations** — enum failures report all valid alternatives ("expected one of: `let`, `return`, `if`")
- **Breadcrumb trail** — nested `#[related]` errors show "while parsing LetBinding > value > Expr"
- **Helpful suggestions** — context-aware help text where possible
- **Zero-cost descriptions** — `Scan` types provide static names/descriptions that flow into error messages automatically

Example output:

```
Error: expected statement
  --> input.rs:3:5
  |
3 |     123 = foo;
  |     ^^^ found integer literal
  |
  help: expected one of: `let`, `return`, `if`, `while`, or an expression
```

```
Error: failed to parse let binding
  --> input.rs:3:5
  |
3 |     let 123 = foo;
  |         ^^^ expected identifier
  |
  --> input.rs:3:5
  |
3 |     let 123 = foo;
  |     --- while parsing let binding starting here
```

## FirstSet (Deferred)

The `Parse` trait includes `type FirstSet` as an associated type to enable compile-time lookahead analysis. The exact representation is deferred to the implementation planning phase. Key requirements:

- `Scan` types: first set is the type itself (its pattern)
- Structs: first set equals the first field's first set
- Enums: first set is a union of all variants' first sets
- The compiler resolves first sets through associated type references — the derive macro emits references like `<FnDecl as Parse>::FirstSet` without needing to resolve them
- Multi-token lookahead for enums: when variants share a common prefix, the framework should generate a combined peek regex by concatenating the first N token patterns per variant

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Lexing approach | Scannerless (context-driven) | Which tokens are valid depends on parse context; pre-scanning is impossible |
| Whitespace handling | `ParseRules` on `Parse`, not `Scan` | Keeps `Scan` pure; rules are grammar-level configuration |
| Rules threading | Associated type on `Parse` | Simpler than generic parameter; each AST is bound to one grammar |
| Enum dispatch | Peek-based, no backtracking | Predictable, efficient; first-set analysis catches ambiguity |
| Backtracking | Fork/commit model | Safe by construction; original input untouched on failure |
| String ownership | `&'input str` throughout | Zero-copy; tokens borrow from source |
| Error quality | `miette`-based diagnostics | Rustc-quality errors with spans, context, and help text |
| Left recursion | Pratt parsing via `#[parse(pratt)]` | Standard solution for operator precedence in recursive descent |
| Combined regex | Longest match + declaration order | Standard lexer semantics; efficient single-pass matching |
