# pg-sql: PostgreSQL SQL Parser Design

A PostgreSQL SQL parser built with recursa, validated against Postgres's regression test suite.

## Goal

Build a parser that can parse PostgreSQL SQL, convert the AST back to SQL text, execute against a real Postgres database, and produce output that matches the official regression test expected output.

## Pipeline

```
.sql file → recursa parser → AST → print to SQL → pipe through psql → capture output → strip echoed SQL → compare against .out file
```

## Project Structure

`pg-sql` is a workspace member crate:

```
pg-sql/
  Cargo.toml
  src/
    lib.rs
    tokens.rs           # keywords, punctuation, literals
    ast/
      mod.rs
      expr.rs           # expressions (literals, operators, casts, etc.)
      select.rs         # SELECT
      insert.rs         # INSERT
      update.rs         # UPDATE
      delete.rs         # DELETE
      create_table.rs   # CREATE TABLE
      ...
    printer.rs          # AST → SQL text via Visit
  tests/
    regress.rs          # test harness
```

## Top-Level AST

```rust
enum PsqlCommand {
    Statement(Statement),
    Directive(String),       // psql commands like \set, \pset, \d
    Comment(String),         // -- line comments, /* block comments */
}
```

The top-level parse result is a `Vec<PsqlCommand>` (using recursa's `Vec<T>` Parse impl — repeated parse while peek succeeds).

Directives are lines starting with `\`. They pass through the pipeline verbatim — the parser stores them as raw strings, the printer emits them unchanged, and `psql` executes them normally.

## Test Harness

The harness in `tests/regress.rs`:

1. Reads a `.sql` file from the Postgres source tree
2. Parses into `Vec<PsqlCommand>`
3. Prints each command back to SQL text (directives verbatim, statements from AST)
4. Pipes the combined text through `psql` connected to a test database
5. Captures stdout
6. Strips echoed SQL lines from both actual output and the expected `.out` file
7. Compares result-only lines (query results, error messages, row counts)

### Output Comparison

The `.out` files contain alternating blocks of echoed SQL and result tables. Since our re-emitted SQL may differ textually from the original (different whitespace, casing, etc.), we strip the echoed SQL and compare only:
- Column headers and divider lines
- Data rows
- Row counts (`(N rows)`)
- Error and notice messages

This validates that our AST captures enough semantic information to produce identical query results, without requiring character-for-character SQL reproduction.

## Token Strategy

### Keywords

Case-insensitive via `(?i:...)` regex wrapping. Added incrementally as test files demand them:

```rust
keywords! {
    Select => "(?i:SELECT)",
    From   => "(?i:FROM)",
    Where  => "(?i:WHERE)",
    // ... grow as needed
}
```

The `keywords!` macro should automatically wrap patterns in `(?i:...)` so the user writes just `"SELECT"`.

### Identifiers

- Unquoted: case-insensitive, folded to lowercase. Pattern: `[a-zA-Z_][a-zA-Z0-9_]*`
- Quoted: case-sensitive, preserves exact text. Pattern: `"[^"]*"` (with `""` escaping)

### String Literals

- Single-quoted: `'...'` with `''` for escaping
- Escape strings: `E'...'`
- Dollar-quoted: `$$...$$` and `$tag$...$tag$`

### Numeric Literals

Integers, decimals, scientific notation.

### Comments

Handled by `ParseRules::consume_ignored` (not as tokens):
- Line comments: `--` to end of line
- Block comments: `/* ... */` with nesting support

## ParseRules for SQL

```rust
struct SqlRules;

impl ParseRules for SqlRules {
    const IGNORE: &'static str = "";

    fn ignore_cache() -> &'static OnceLock<Regex> {
        static CACHE: OnceLock<Regex> = OnceLock::new();
        &CACHE
    }

    fn consume_ignored(input: &mut Input) {
        loop {
            let before = input.cursor();
            // skip whitespace
            skip_whitespace(input);
            // skip -- line comments
            skip_line_comment(input);
            // skip /* nested block comments */
            skip_block_comment(input);
            // if nothing was skipped, break
            if input.cursor() == before {
                break;
            }
        }
    }
}
```

Nested block comments use a depth counter — increment on `/*`, decrement on `*/`, stop when depth reaches 0.

## Bootstrapping Phases

### Phase 1: boolean.sql

Drives the initial grammar:
- **Statements:** `SELECT`, `CREATE TABLE`, `INSERT INTO`, `DROP TABLE`
- **Expressions:** literals (int, string, bool), column refs, type casts, binary operators (`AND`, `OR`, comparison), `NOT`, `IS [NOT] NULL`
- **Clauses:** `AS` alias, `WHERE`, column list, `VALUES`

### Phase 2: delete.sql

Adds:
- **Statements:** `DELETE FROM`
- **Features:** `SERIAL`, `PRIMARY KEY`, table aliases (`AS`), `char_length()` function calls

### Phase 3: select.sql

Adds:
- **Clauses:** `ORDER BY` (with `USING`), `LIMIT`/`OFFSET`
- **Statements:** `SET`, `ANALYZE`
- **Features:** `*` wildcard, qualified column names (`table.column`), subqueries

### Subsequent Phases

Driven by test file complexity. Each new test file reveals grammar gaps and drives additions.

## Recursa Changes Required

Before starting the SQL parser, three changes to recursa:

### 1. Vec\<T\> Parse impl

Repeated parse while `T::peek` succeeds. Zero-or-more. No separator.

```rust
impl<'input, T: Parse<'input>> Parse<'input> for Vec<T> {
    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let mut items = Vec::new();
        while T::peek(input, rules) {
            items.push(T::parse(input, rules)?);
        }
        Ok(items)
    }
}
```

### 2. Case-insensitive keywords! macro

The `keywords!` macro automatically wraps patterns in `(?i:...)`:

```rust
keywords! {
    Select => "SELECT",  // generates pattern (?i:SELECT)
}
```

### 3. ParseRules::consume_ignored trait method

Override for custom ignore logic (nested block comments):

```rust
trait ParseRules {
    const IGNORE: &'static str;
    fn ignore_cache() -> &'static OnceLock<Regex>;
    fn ignore_regex() -> Option<&'static Regex> { /* default */ }

    fn consume_ignored(input: &mut Input) {
        input.consume_ignored(Self::ignore_regex());
    }
}
```

All generated code calls `R::consume_ignored(input)` instead of `input.consume_ignored(R::ignore_regex())`.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Project location | Workspace member crate | Large enough for its own crate; tightly coupled during dev |
| Execution strategy | Pipe through real `psql` | Avoids building a psql output formatter |
| Output comparison | Strip echoed SQL, compare results only | Our SQL text may differ from original; results must match |
| Directive handling | Pass through verbatim | Directives are psql concerns, not parser concerns |
| Comment handling | Custom `consume_ignored` | Regex can't handle nested `/* */` comments |
| Case sensitivity | `(?i:...)` regex patterns | SQL keywords are case-insensitive |
| Grammar bootstrapping | Test-file driven, grammar spec as reference | Build what's needed, get it right from the spec |

## Deferred

- Pretty-printing (only need semantically correct output)
- Full PostgreSQL grammar (build incrementally)
- `Transform` / `Transformable` for AST rewriting
