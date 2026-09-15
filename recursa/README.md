# recursa

`recursa` is the user-facing facade for the Recursa AST-first parser generator.
It re-exports the grammar authoring macros and the public runtime contracts used
by generated grammars.

With Recursa, semantic Node declarations are the grammar's center. Fixed tokens
remain attributes when their text has no semantic meaning. `Option<T>` and the
`zero_or_many!(T)`/`one_or_many!(T)` markers state cardinality, while the selected
AST style determines concrete storage.

## Setup

```toml
[dependencies]
recursa = "0.1"

[build-dependencies]
recursa-codegen = "0.1"
```

The build script calls the generator's public entry point:

```rust
fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/grammar.rs")?;
    Ok(())
}
```

A grammar module combines `grammar!`, `tokens!`, and `ast_node!`:

```rust,ignore
recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
}

recursa::tokens! {
    categories { UNRESERVED, RESERVED }
    flags { bare_label }
    keywords {
        WINDOW => "WINDOW" in UNRESERVED + bare_label,
        ROWS => "ROWS" in RESERVED,
    }
    admissions {
        ColId = UNRESERVED + RESERVED,
        WindowRefName = ColId - { ROWS },
    }
    punctuation {
        LBRACKET => "[",
        RBRACKET => "]",
        COMMA => ",",
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

Each metadata-bearing keyword has one category and any number of orthogonal
flags. Named admission expressions may use categories, flags, fixed-token
names, the `keywords` universe, and other admissions with `+` and `-`.
Generation rejects cycles, unknown names, contradictory metadata, and
subtractions that remove nothing; duplicate union membership is a warning.
`#[lex(..., admits(WindowRefName))]` applies the compiled canonical membership
to that generated content-token type.

Use `optional(TOKENS...)` inside the sole `#[tok(...)]` sequence for fixed
syntax that should be accepted but erased from the AST. Use a separate
`#[presence(TOKENS...)]` on an unqualified `bool` field when the AST must retain
whether the sequence occurred. Presence composes with the field's ordinary
attachment, including required syntax around `this`:

```rust,ignore
recursa::ast_node! {
    struct CreateMaterializedView {
        #[tok(CREATE, this, MATERIALIZED, VIEW)]
        #[presence(UNLOGGED)]
        unlogged: bool,
        name: Identifier,
    }
}
```

These optional sequences are non-empty, unnested, fixed-only, and selected
atomically. A partial prefix consumes nothing. Pretty reuses parsed source
syntax, emits a canonical presence sequence for a synthetic `true`, and omits
both a synthetic `false` and synthetic erased optional groups.

Closed lexical mechanisms live beside the token grammar. Declare an emitting
matcher in `matchers`, bind its same-named token
type with `#[lex(matcher)]`, or declare the same matcher vocabulary in `ignore`
for non-emitting input:

```rust,ignore
recursa::tokens! {
    matchers {
        DollarString => same_delimiter(opener = r"\$(?:[A-Za-z_][A-Za-z0-9_]*)?\$"),
        Number => next_exclusion(pattern = r"[0-9]+", excluded = r"[A-Za-z0-9_]"),
        MetaCommand => physical_line(prefix = r"\\[A-Za-z]+"),
    }
    ignore {
        BlockComment => nested(opener = "/*", closer = "*/"),
    }
}

recursa::ast_node! {
    struct Definitions {
        #[lex(matcher)]
        dollar: DollarString,
        #[lex(matcher)]
        number: Number,
        #[lex(matcher)]
        command: MetaCommand,
    }
}
```

The closed vocabulary also includes configurable operator runs. Arbitration
uses final byte extent, then exact fixed-token status, then optional explicit
`priority`; declaration order never decides a tie. A physical-line matcher
includes its LF, CRLF, or CR ending, or reaches EOF. Unterminated delimited and
nested regions produce opener-anchored lexical errors over the complete
opener-to-EOF range; generated `LexError::code`, `diagnostic_code`, and `anchor`
preserve their stable diagnostic identity.

Authored lexer callbacks, post-lex hooks, and parser postconditions are not
supported. Source analysis reports their retired syntax with targeted stable
diagnostics; use closed matchers and named admissions instead.

Opt into exact whitespace and comment provenance by declaring named rules in
`trivia`. Regular expressions and the closed matcher vocabulary are both
supported:

```rust,ignore
recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\r\n]+",
        LineComment => r"--[^\n]*",
        BlockComment => nested(opener = "/*", closer = "*/"),
    }
}
```

Retained trivia never enters significant parsing, FIRST, or dispatch. Generated
`LexResult::trivia_gaps` and a complete parsed root view expose immutable
beginning, inter-record, and end gaps with exact text, checked spans, ordered
records, and grammar-local `TriviaKind` values. An empty or trivia-only source
still has one beginning/end gap. Grammars that use only `ignore` retain no
trivia sidecar.

An alternative lexer constructs the same public result through `LexBuilder`.
Each supplied token, error, or `append_trivia` record must use an exact
grammar-local kind and a source-bounded UTF-8 span, in global source order and
without overlap. Finalization preserves unrecorded source gaps verbatim and
never rescans them.

## Node declarations

An `ast_node!` invocation contains one or more structs or enums. Named, tuple,
and unit forms are supported; generated types keep their declared names in the
surrounding module. Handwritten implementations and ordinary Rust type aliases
stay outside the invocation.

| Declaration form | Meaning | Ordinary nested storage | With `arena_ast` |
| --- | --- | --- | --- |
| `T` | One child | `T` | `T` |
| `Option<T>` | Optional child | `Option<T>` | `Option<T>` |
| `boxed!(T)` | Explicit indirection | `Box<T>` | `ArenaBox<'input, T>` |
| `zero_or_many!(T)` | Zero or more children | `Vec<T>` | `ArenaVec<'input, T>` |
| `one_or_many!(T)` | One or more children | `Vec1<T>` | `ArenaVec1<'input, T>` |

The inner markers are fixed declaration syntax and require no imports. Import
or qualify the outer macro normally. Write `boxed!(T)` where the nested layout
requires indirection; generation does not insert boxes automatically. Concrete
storage containers are rejected inside declarations, including through aliases.

Boxing may appear at any layer. Ignoring boxes, the supported shapes are `T`,
`Option<T>`, either repetition marker, and `Option<one_or_many!(T)>`. Optional
zero-or-more lists and nested repetitions are not supported. Existing fixed-token
presence fields remain direct `#[presence(...)] bool` fields.

Omitted type and struct-field visibility defaults to `pub`; explicit restrictions
such as `pub(crate)` are preserved. Put documentation, derives, and grammar
attributes on their intended declaration, variant, or field inside the macro.
An outer `cfg` or `cfg_attr` can gate an entire invocation.

Omit lifetime parameters from declarations and semantic type references.
Generation adds one `'input` lifetime only where fields or storage require it.
For example, `Wrap<T> { value: T }` remains `Wrap<T>` and can be applied as
`Wrap<Name<'input>>`; an arena-backed list of `T` needs its own storage lifetime.
Handwritten Rust uses those generated signatures normally. Generic Node type
arguments must be explicit; defaulted Node parameters are not supported. Bounds
and `where` clauses remain ordinary Rust syntax and cannot name the inferred
declaration lifetime.

As with the previous authoring API, each generic Node needs at least one closed
application in the selected grammar, such as a field of type `Wrap<Name>`.

Node support is intrinsic. Other configured traits are opt-in through
`grammar! { ..., derives(Debug, Clone, Pretty), ... }`, with local additions in
`#[node(derive(...))]`. Use `#[omit(Clone)]` to suppress a configured
implementation when supplying your own. Ordinary Rust `#[derive(...)]`, including
third-party derives and their helper attributes, remains available inside the
declaration. Omitting a configured trait and deriving it through ordinary Rust
is valid; requesting both implementations without an omission is an error.
The old `#[node(omit(...))]` spelling is replaced by `#[omit(...)]`.

## Document framing

With `explorer` enabled for both `recursa` and `recursa-codegen`, a grammar may
freeze an exact-source document partition at generation time:

```rust,ignore
recursa::grammar! {
    module = crate::grammar,
    spans = mandatory,
    framing(
        island = Entry,
        boundary = SEMI,
        line = [MetaCommand],
        delimited = [RawBlock],
        payload(after = Entry::CopyFromStdin, terminator = "\\.")
    ),
}
```

`Entry::frame(source)` returns a `CompleteFrame` only when every island parses
strictly and every payload terminates. Complete islands expose `Parsed<Entry>`.
The first ordinary island, ownership, or payload failure instead returns
`FrameFailure::Rejected`: a `FrameRejection` names the failing island extent,
carries that island's strict parse diagnostics, and reports the framing cause
when the island itself parsed but lacked its boundary or payload terminator.
No strict value survives a rejection. Atomic raw regions and payload regions
provide exact text and spans but no AST conversion or `Pretty` capability.

`payload(after = ...)` must name a reachable variant of the framing island. It
is selected only after the complete strict island commits that variant; a COPY-
like token prefix cannot guess payload mode. The terminator is exact nonempty
single-line text and matches only at a physical-line start with an LF or CRLF
line ending. Payload data and the terminator retain distinct spans. See the repository's
[full framing contract](../docs/document-framing.md).

This is an excerpt from the
[compiled JSON grammar](https://github.com/freshtonic/recursa/blob/main/examples/json/src/grammar.rs).
Its companion
[public parse entry point](https://github.com/freshtonic/recursa/blob/main/examples/json/src/lib.rs)
shows grammar-local `lex`, `input`, and `parse_spanned` usage without depending
on generated file layout.

## Features

No features are enabled by default.

- `arbitrary` supplies facade support for configured generated `Arbitrary`
  implementations.
- `diagrams` supplies the matching facade capability for generated diagrams and
  renders them, with standalone SVG files, only while `cargo doc` runs.
- `explorer` exposes grammar-erased strict documents plus generated document
  framing. Strict parsing is the only source operation: `Adapter::parse`
  returns one complete document or structured diagnostics.
- `serde` supplies facade support for configured generated Serde
  implementations.
- `spans` makes ordinary parses retain occurrence provenance and enables
  exact-source framing. Generated parsers always expose `parse_without_spans`
  for an explicitly provenance-free parse.

Build-time diagram, Explorer, or spans generation also enables the same
feature on `recursa-codegen`.

## Public boundary

Use the facade's generated grammar APIs plus its public spans, parse errors,
parsed values and views, traversal traits, Pretty interfaces, and `Vec1` type.
The hidden `__private` module is exclusively a matching-version contract for
generated code.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[API reference](https://docs.rs/recursa/latest/recursa/) and the
[compiled examples](https://github.com/freshtonic/recursa/tree/main/examples).
