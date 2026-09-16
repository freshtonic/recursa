# Recursa

Recursa is an AST-first parser generator for Rust: `ast_node!` declarations
describe the data a parser produces, while attributes describe the syntax
around that data. Generation supplies concrete Rust types for the selected AST
storage style.

Keeping the grammar beside the semantic model shortens the distance between
syntax and the code that consumes it. Delimiters, separators, and keywords stay
out of the AST when they carry no meaning. `Option<T>`, `zero_or_many!(T)`, and
`one_or_many!(T)` make cardinality explicit without fixing collection storage.

Recursa is a different trade-off from parser combinators and lex/yacc-style
tools. Combinators build parsers directly in Rust; lex/yacc systems usually keep
a separate grammar and semantic actions. Recursa instead generates lexing and
parsing from annotated AST Node declarations. Which model is clearest depends
on the language and the surrounding application.

## Quickstart

Recursa uses a runtime facade in normal dependencies and the generator in build
dependencies:

```toml
[dependencies]
recursa = "0.1"

[build-dependencies]
recursa-codegen = "0.1"
```

Point a build script at the Rust module that owns the grammar:

```rust
fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/grammar.rs")?;
    Ok(())
}
```

Then declare tokens and semantic types. This excerpt comes from the
[compiled JSON example](examples/json/src/grammar.rs):

```rust,ignore
recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        TRUE => "true",
        FALSE => "false",
        NULL => "null",
    }
    punctuation {
        LBRACE => "{",
        RBRACE => "}",
        LBRACKET => "[",
        RBRACKET => "]",
        COMMA => ",",
        COLON => ":",
    }
}

recursa::ast_node! {
    #[tok(LBRACKET, this, RBRACKET)]
    struct Array {
        #[sep(COMMA)]
        values: Option<one_or_many!(Value)>,
    }
}
```

The brackets and commas shape the syntax without becoming dead fields.
`Option<one_or_many!(Value)>` says that an array is either empty or contains at least
one value.

Declarations omit input lifetimes and public visibility boilerplate. The generated
Rust types still carry required lifetimes, and explicit visibility restrictions
remain available. Use `boxed!(T)` wherever a child needs indirection; `arena_ast`
on the grammar selects arena-backed nested containers for both boxes and lists.
See the [declaration guide](recursa/README.md#node-declarations) for syntax and
trait configuration.

Fixed-token options do not require marker nodes. Use `optional(TOKENS...)`
inside the sole `#[tok(...)]` sequence for erased syntax, or store its presence
in a plain `bool` with a separate `#[presence(TOKENS...)]` attribute:

```rust,ignore
recursa::ast_node! {
    struct CreateIndex {
        #[tok(CREATE, optional(OR, REPLACE), INDEX, this)]
        #[presence(CONCURRENTLY)]
        concurrently: bool,
        name: Identifier,
    }
}
```

Both forms match the entire fixed-token sequence atomically. An incomplete
optional sequence consumes nothing; synthetic Pretty output emits a presence
sequence only when its boolean is true and omits erased optional groups.

Generation adds grammar-local public lexing and parsing entry points. The
[complete example](examples/json/src/lib.rs) checks lexical errors, parses a
spanned value, rejects trailing input, and then extracts the semantic AST:

```rust,ignore
pub fn parse(source: &str) -> Result<grammar::Value<'_>, ParseFailure> {
    let lexed = grammar::lex(source);
    let lexical_errors = lexed
        .errors()
        .map(|error| error.span())
        .collect::<Vec<_>>();
    if !lexical_errors.is_empty() {
        return Err(ParseFailure::Lexical(lexical_errors));
    }
    let mut input = lexed.input();
    let parsed = grammar::Value::parse_spanned(&mut input).map_err(ParseFailure::Syntax)?;
    if !input.is_eof() {
        return Err(ParseFailure::TrailingInput);
    }
    Ok(parsed.into_ast())
}
```

Languages with source-backed regions can use the closed matcher vocabulary in
`tokens!`: same-delimiter and nested regions, next-scalar exclusions,
configurable operator runs, and physical-line regions. Emitting declarations
live in `matchers { ... }` and bind through `#[lex(matcher)]`; the same forms in
`ignore { ... }` are non-emitting. Final match extent, exact fixed-token status,
and explicit priority determine arbitration, never declaration order. See the
[facade grammar guide](recursa/README.md) for the declaration forms and lexical
error contract.

Operator fences apply one narrow eligibility exception before that arbitration.
An operator-shaped fixed candidate crossing a related operator run's interior
configured fence is ineligible only when ordinary local arbitration at the
fence selects diagnostic-free skip trivia covering the fence. The candidate
remains eligible when another related operator run reaches its full extent.

Grammars that need exact whitespace or comments can opt into classified trivia
with named `trivia { ... }` rules. Generated lexing and complete parsed-root
views then expose immutable beginning, inter-record, and end gaps while the
significant-token parser remains unchanged. Non-retaining grammars continue to
discard `ignore` matches without a trivia sidecar.

Document-oriented grammars can declare generated exact-source framing under
the `explorer` capability. Framing recognizes repeated typed islands, atomic
closed regions, and optional payload data opened only by a complete island
variant. The result is transactional: either every island remains a strict
`Parsed<T>`, or the document is rejected at its first failing island with that
island's strict diagnostics. See the
[document-framing contract](docs/document-framing.md).

Run both compiled examples from a checkout:

```console
cargo run -p recursa-example-json -- '{"answer":[true,null,42]}'
cargo run -p recursa-example-expressions -- '-a! + 2 * 3 ^ 4'
```

- [JSON](examples/json) covers objects, arrays, literals, delimiters,
  separators, and semantic repetition cardinality.
- [Expressions](examples/expressions) covers atoms, grouping, and
  prefix, postfix, and infix operators with declared precedence and
  associativity.

### Local Explorer

Enable the `explorer` feature for both the facade and generator, add
`recursa-explorer` to a small development binary, and launch the generated root
adapter:

```rust,ignore
fn main() -> Result<(), recursa_explorer::RunError> {
    recursa_explorer::run(grammar::Value::explorer())
}
```

The bundled, dependency-free browser workspace edits and highlights source,
shows strict diagnostics or a lazy recursive AST, links the deepest source span
in both directions, inspects schema/tokens, and exposes Pretty and deterministic
sampling only when configured. See the
[`recursa-explorer` walkthrough](recursa-explorer/README.md#visual-walkthrough).
It runs on an OS-assigned loopback port through a private matching-version
protocol; it is not a remote playground.

## Capabilities

The `recursa` facade has no default features. Optional features are:

| Feature | Capability |
|---|---|
| `arbitrary` | Generated `arbitrary::Arbitrary` implementations when configured by the grammar |
| `diagrams` | Matching facade support for generated Rustdoc railroad diagrams, rendered only under `cargo doc` |
| `explorer` | Grammar-erased strict Explorer adapter and document-framing contracts |
| `serde` | Generated Serde implementations when configured by the grammar |
| `spans` | Occurrence provenance for ordinary parses; exact-source framing support |

Generated grammars always expose `parse_without_spans`. Build-time `diagrams`,
`explorer`, and `spans` generation also require the corresponding
`recursa-codegen` feature. Configured generated capabilities must match the
facade features used to compile the grammar.

The Explorer adapter contracts live behind `recursa`'s `explorer` feature. The
supported `recursa-explorer` package serves the bundled local browser UI linked
to those adapters.

## Workspace packages

| Package | Role |
|---|---|
| [`recursa`](recursa) | User-facing facade for authoring macros and runtime contracts |
| [`recursa-codegen`](recursa-codegen) | Build-time source analysis, semantic analysis, diagnostics, and generation |
| [`recursa-core`](recursa-core) | Grammar-erased runtime, parsed provenance, traversal, and Pretty support |
| [`recursa-diagrams`](recursa-diagrams) | Published railroad-diagram plans and doc-time SVG rendering |
| [`recursa-macros`](recursa-macros) | Matching-version structural loaders re-exported by the facade |
| [`recursa-test`](recursa-test) | Reserved package; currently no public test harness |
| [`recursa-explorer`](recursa-explorer) | Supported loopback launcher and bundled interactive Explorer UI |
| [`recursa-example-json`](examples/json) | Compiled and runnable JSON example |
| [`recursa-example-expressions`](examples/expressions) | Compiled and runnable declared-precedence expression example |

Most applications depend on `recursa` and use `recursa-codegen` only from
`build.rs`. The other packages document implementation and advanced support
boundaries; hidden `__private` modules are a matching-version generation
protocol, not a public parsing API.

## Compatibility and documentation

Recursa uses Rust edition 2024 and has a minimum supported Rust version of
1.88.

- [`recursa` API reference](https://docs.rs/recursa/latest/recursa/)
- [`recursa-codegen` API reference](https://docs.rs/recursa-codegen/latest/recursa_codegen/)
- [Domain vocabulary](CONTEXT.md)

The repository is under active development. Prefer the compiled examples and
current Rustdoc over historical design material.
