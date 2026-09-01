# recursa

`recursa` is the user-facing facade for the Recursa AST-first parser generator.
It re-exports the grammar authoring macros and the public runtime contracts used
by generated grammars.

With Recursa, semantic Rust types are the grammar's center. Fixed tokens remain
attributes when their text has no semantic meaning, and containers such as
`Option`, `Vec`, and `Vec1` state cardinality in the AST itself.

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

A grammar module combines `grammar!`, `tokens!`, and `#[derive(Node)]`:

```rust,ignore
recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    max_lookahead = 1,
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

#[derive(recursa::Node)]
#[tok(LBRACKET, this, RBRACKET)]
pub struct Array<'input> {
    #[sep(COMMA)]
    pub values: Option<recursa::Vec1<Value<'input>>>,
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
#[derive(recursa::Node)]
pub struct CreateMaterializedView<'input> {
    #[tok(CREATE, this, MATERIALIZED, VIEW)]
    #[presence(UNLOGGED)]
    pub unlogged: bool,
    pub name: Identifier<'input>,
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

#[derive(recursa::Node)]
pub struct Definitions<'input> {
    #[lex(matcher)]
    dollar: DollarString<'input>,
    #[lex(matcher)]
    number: Number<'input>,
    #[lex(matcher)]
    command: MetaCommand<'input>,
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
- `diagrams` supplies the matching facade capability for generated diagrams.
- `explorer` exposes grammar-erased Explorer adapter and document contracts.
- `serde` supplies facade support for configured generated Serde
  implementations.

Build-time diagram or Explorer generation also enables the same feature on
`recursa-codegen`.

## Public boundary

Use the facade's generated grammar APIs plus its public spans, parse errors,
parsed values and views, traversal traits, Pretty interfaces, and `Vec1` type.
The hidden `__private` module is exclusively a matching-version contract for
generated code.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[API reference](https://docs.rs/recursa/latest/recursa/) and the
[compiled examples](https://github.com/freshtonic/recursa/tree/main/examples).
