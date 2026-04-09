# pg-sql Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build enough of a PostgreSQL SQL parser to parse, print, execute, and validate `boolean.sql` from the Postgres regression test suite.

**Architecture:** `pg-sql` workspace member crate. Tokens defined via recursa macros. AST types derive `Parse` and `Visit`. A printer converts AST back to SQL text. A test harness pipes the output through `psql` and compares results against the expected `.out` file (stripping echoed SQL).

**Tech Stack:** Rust, recursa, `regex`, `std::process::Command` (for shelling out to `psql`)

**Design doc:** `docs/plans/2026-04-10-pg-sql-design.md`

---

## SQL Constructs Required by boolean.sql

From reading the file, these SQL constructs are needed:

**Statements:**
- `SELECT expr [, expr ...] [FROM table [, table ...]] [WHERE expr] [ORDER BY ...];`
- `CREATE TABLE name (col type [, ...]);`
- `INSERT INTO name [(col [, ...])] VALUES (expr [, ...]);`
- `DROP TABLE name;`

**Expressions:**
- Integer literals: `1`, `0`, `2`
- String literals: `'t'`, `'true'`, `'   f           '`
- Boolean literals: `true`, `false`, `null`
- Column refs: `f1`, `BOOLTBL1.f1`, `BOOLTBL2.f1`
- Wildcard: `*`, `BOOLTBL1.*`
- Type casts: `bool 't'` (function-style), `0::boolean`, `'TrUe'::text::boolean`
- Binary operators: `=`, `<>`, `>`, `>=`, `<`, `<=`, `AND`, `OR`
- Unary: `NOT`
- Boolean tests: `IS TRUE`, `IS NOT TRUE`, `IS FALSE`, `IS NOT FALSE`, `IS UNKNOWN`, `IS NOT UNKNOWN`
- Function calls: `pg_input_is_valid('true', 'bool')`, `booleq(...)`, `boolne(...)`
- Aliased expressions: `expr AS alias`
- `SELECT *` from `pg_input_error_info(...)`

**psql directives:**
- `\pset null '(null)'`

---

## Task 1: Crate Scaffold and Vendored Fixtures

Set up the `pg-sql` workspace member crate and copy in the Postgres test fixtures.

**Files:**
- Create: `pg-sql/Cargo.toml`
- Create: `pg-sql/src/lib.rs`
- Modify: `Cargo.toml` (add workspace member)
- Copy: Postgres test fixtures into `pg-sql/fixtures/`

**Step 1: Create crate**

`pg-sql/Cargo.toml`:

```toml
[package]
name = "pg-sql"
version = "0.1.0"
edition = "2024"
description = "PostgreSQL SQL parser built with recursa"
license = "MIT"
publish = false

[dependencies]
recursa = { path = ".." }
regex = "1"
```

`pg-sql/src/lib.rs`:

```rust
pub mod tokens;
pub mod ast;
pub mod printer;
```

Create stub modules: `pg-sql/src/tokens.rs`, `pg-sql/src/ast/mod.rs`, `pg-sql/src/printer.rs`.

**Step 2: Add to workspace**

In root `Cargo.toml`, change members:

```toml
members = ["recursa-core", "recursa-derive", "pg-sql", "."]
```

**Step 3: Copy fixtures**

```bash
mkdir -p pg-sql/fixtures
cp -r /Users/jamessadler/code/postgres/src/test/regress/sql pg-sql/fixtures/
cp -r /Users/jamessadler/code/postgres/src/test/regress/expected pg-sql/fixtures/
```

**Step 4: Build**

Run: `cargo build -p pg-sql`
Expected: Compiles.

**Step 5: Commit**

```bash
git add pg-sql/ Cargo.toml
git commit -m "Add pg-sql crate scaffold with vendored Postgres test fixtures"
```

Note: The fixtures are ~17MB. Consider adding a `.gitattributes` to mark them as binary/linguist-vendored so they don't pollute diffs.

---

## Task 2: SQL ParseRules with Nested Block Comments

Implement `SqlRules` with custom `consume_ignored` that handles whitespace, `--` line comments, and nested `/* */` block comments.

**Files:**
- Create: `pg-sql/src/rules.rs`
- Modify: `pg-sql/src/lib.rs`

**Step 1: Write the failing tests**

Create `pg-sql/src/rules.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use recursa::{Input, ParseRules};
    use super::SqlRules;

    #[test]
    fn skip_whitespace() {
        let mut input = Input::new("   SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_line_comment() {
        let mut input = Input::new("-- this is a comment\nSELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_block_comment() {
        let mut input = Input::new("/* comment */SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_nested_block_comment() {
        let mut input = Input::new("/* outer /* inner */ still outer */SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_deeply_nested_block_comment() {
        let mut input = Input::new("/* a /* b /* c */ d */ e */SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_mixed_whitespace_and_comments() {
        let mut input = Input::new("  -- line comment\n  /* block */  SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn no_skip_when_no_ignored() {
        let mut input = Input::new("SELECT 1");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT 1");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p pg-sql`
Expected: FAIL — `SqlRules` not defined.

**Step 3: Implement SqlRules**

```rust
use std::sync::OnceLock;
use regex::Regex;
use recursa::{Input, ParseRules};

pub struct SqlRules;

impl ParseRules for SqlRules {
    const IGNORE: &'static str = "";

    fn ignore_cache() -> &'static OnceLock<Regex> {
        static CACHE: OnceLock<Regex> = OnceLock::new();
        &CACHE
    }

    fn consume_ignored(input: &mut Input) {
        loop {
            let before = input.cursor();
            skip_whitespace(input);
            skip_line_comment(input);
            skip_block_comment(input);
            if input.cursor() == before {
                break;
            }
        }
    }
}

fn skip_whitespace(input: &mut Input) {
    let remaining = input.remaining();
    let trimmed = remaining.len() - remaining.trim_start().len();
    if trimmed > 0 {
        input.advance(trimmed);
    }
}

fn skip_line_comment(input: &mut Input) {
    if input.remaining().starts_with("--") {
        if let Some(newline) = input.remaining().find('\n') {
            input.advance(newline + 1);
        } else {
            input.advance(input.remaining().len());
        }
    }
}

fn skip_block_comment(input: &mut Input) {
    if !input.remaining().starts_with("/*") {
        return;
    }
    let bytes = input.remaining().as_bytes();
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() - 1 {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                input.advance(i);
                return;
            }
        } else {
            i += 1;
        }
    }
    // Unclosed comment — advance to end
    input.advance(bytes.len());
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p pg-sql`
Expected: PASS

**Step 5: Commit**

```bash
git add pg-sql/src/rules.rs pg-sql/src/lib.rs
git commit -m "Add SqlRules with nested block comment support"
```

---

## Task 3: SQL Tokens

Define the tokens needed for Phase 1 using recursa's `keywords!`, `punctuation!`, and `literals!` macros, plus some hand-crafted `Scan` types.

**Files:**
- Modify: `pg-sql/src/tokens.rs`

**Step 1: Write token tests**

```rust
#[cfg(test)]
mod tests {
    use recursa::{Input, NoRules, Parse};
    use super::*;

    #[test]
    fn keyword_select_case_insensitive() {
        let mut input = Input::new("SELECT");
        assert!(<Select as Parse>::peek(&input, &NoRules));
        let mut input2 = Input::new("select");
        assert!(<Select as Parse>::peek(&input2, &NoRules));
        let mut input3 = Input::new("SeLeCt");
        assert!(<Select as Parse>::peek(&input3, &NoRules));
    }

    #[test]
    fn string_literal() {
        let mut input = Input::new("'hello world'");
        let lit = <StringLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'hello world'");
    }

    #[test]
    fn string_literal_with_escapes() {
        let mut input = Input::new("'it''s'");
        let lit = <StringLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "'it''s'");
    }

    #[test]
    fn integer_literal() {
        let mut input = Input::new("42");
        let lit = <IntegerLit as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(lit.0, "42");
    }

    #[test]
    fn identifier() {
        let mut input = Input::new("my_table");
        let id = <Ident as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(id.0, "my_table");
    }

    #[test]
    fn identifier_not_keyword() {
        // "SELECT" should not match as an identifier when keywords are defined
        let input = Input::new("SELECT");
        assert!(!<Ident as Parse>::peek(&input, &NoRules));
    }
}
```

**Step 2: Implement tokens**

Keywords (case-insensitive via the macro):

```rust
recursa::keywords! {
    Select => "SELECT",
    From => "FROM",
    Where => "WHERE",
    As => "AS",
    And => "AND",
    Or => "OR",
    Not => "NOT",
    True => "TRUE",
    False => "FALSE",
    Null => "NULL",
    Is => "IS",
    Unknown => "UNKNOWN",
    Create => "CREATE",
    Table => "TABLE",
    Insert => "INSERT",
    Into => "INTO",
    Values => "VALUES",
    Drop => "DROP",
    Order => "ORDER",
    By => "BY",
    Bool => "BOOL",
    Boolean => "BOOLEAN",
    Text => "TEXT",
    Int => "INT",
}
```

Punctuation:

```rust
recursa::punctuation! {
    Semi => ";",
    Comma => ",",
    LParen => r"\(",
    RParen => r"\)",
    Star => r"\*",
    Dot => r"\.",
    Eq => "=",
    Neq => "<>",
    Lt => "<",
    Gt => ">",
    Lte => "<=",
    Gte => ">=",
    ColonColon => "::",
}
```

Literals (hand-crafted `Scan` impls because they're more complex than simple regex):

```rust
// String literals: 'text' with '' escaping
// Pattern: '[^']*(?:''[^']*)*'
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLit(pub String);

// Integer literals: [0-9]+
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegerLit(pub String);

// Identifiers: [a-zA-Z_][a-zA-Z0-9_]* but NOT a keyword
// This requires negative lookahead or keyword checking
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident(pub String);
```

The `Ident` token is tricky — it must not match keywords. The simplest approach: scan with the identifier regex, then check if the matched text is a keyword. If it is, fail the match.

**Step 3: Run tests**

Run: `cargo test -p pg-sql`
Expected: PASS

**Step 4: Commit**

```bash
git add pg-sql/src/tokens.rs
git commit -m "Add SQL tokens: keywords, punctuation, string/integer literals, identifiers"
```

---

## Task 4: AST Types — Expressions

Define expression AST types needed for boolean.sql.

**Files:**
- Create: `pg-sql/src/ast/expr.rs`
- Modify: `pg-sql/src/ast/mod.rs`

**Step 1: Write parse tests for expressions**

```rust
#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};
    use crate::rules::SqlRules;
    use super::*;

    #[test]
    fn parse_integer_expr() {
        let mut input = Input::new("42");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Lit(Literal::Integer(_))));
    }

    #[test]
    fn parse_string_expr() {
        let mut input = Input::new("'hello'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Lit(Literal::String(_))));
    }

    #[test]
    fn parse_column_ref() {
        let mut input = Input::new("f1");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::ColumnRef(_)));
    }

    #[test]
    fn parse_qualified_column_ref() {
        let mut input = Input::new("BOOLTBL1.f1");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::QualifiedColumnRef(_, _, _)));
    }

    #[test]
    fn parse_type_cast_function_style() {
        let mut input = Input::new("bool 't'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::TypeCast(_, _)));
    }

    #[test]
    fn parse_type_cast_postgres_style() {
        let mut input = Input::new("0::boolean");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        // Should be a cast chain
    }

    #[test]
    fn parse_binary_and() {
        let mut input = Input::new("true AND false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::And(_, _, _)));
    }

    #[test]
    fn parse_function_call() {
        let mut input = Input::new("pg_input_is_valid('true', 'bool')");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::FuncCall(_, _)));
    }
}
```

**Step 2: Implement Expr AST**

Expressions use Pratt parsing for operator precedence:

```rust
#[derive(Parse, Visit, Debug)]
#[parse(rules = SqlRules, pratt)]
pub enum Expr {
    // Atoms
    #[parse(atom)]
    Lit(Literal),
    #[parse(atom)]
    ColumnRef(Ident),
    #[parse(atom)]
    QualifiedRef(QualifiedColumnRef),
    #[parse(atom)]
    FuncCall(FuncCall),
    #[parse(atom)]
    Star(tokens::Star),
    #[parse(atom)]
    BoolTrue(tokens::True),
    #[parse(atom)]
    BoolFalse(tokens::False),
    #[parse(atom)]
    BoolNull(tokens::Null),
    #[parse(atom)]
    Parens(ParenExpr),

    // Prefix
    #[parse(prefix, bp = 15)]
    Not(tokens::Not, Box<Expr>),

    // Infix — comparison
    #[parse(infix, bp = 5)]
    Eq(Box<Expr>, tokens::Eq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Neq(Box<Expr>, tokens::Neq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lt(Box<Expr>, tokens::Lt, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gt(Box<Expr>, tokens::Gt, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lte(Box<Expr>, tokens::Lte, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gte(Box<Expr>, tokens::Gte, Box<Expr>),

    // Infix — logical
    #[parse(infix, bp = 2)]
    And(Box<Expr>, tokens::And, Box<Expr>),
    #[parse(infix, bp = 1)]
    Or(Box<Expr>, tokens::Or, Box<Expr>),

    // Postfix — IS [NOT] TRUE/FALSE/UNKNOWN, ::type cast
    // Note: postfix operators need special handling in Pratt parsing
    // These may need to be handled differently
}
```

Note: `IS TRUE`, `IS NOT FALSE`, and `::type` casts are postfix operators. The current Pratt implementation only supports atoms, prefix, and infix. Postfix support may need to be added to recursa or handled as a special case. This task should implement what's possible and flag what needs additional recursa support.

**Step 3: Run tests**

Run: `cargo test -p pg-sql`

**Step 4: Commit**

```bash
git add pg-sql/src/ast/
git commit -m "Add expression AST with Pratt parsing"
```

---

## Task 5: AST Types — Statements

Define statement AST types for SELECT, CREATE TABLE, INSERT, DROP TABLE.

**Files:**
- Create: `pg-sql/src/ast/select.rs`
- Create: `pg-sql/src/ast/create_table.rs`
- Create: `pg-sql/src/ast/insert.rs`
- Create: `pg-sql/src/ast/drop_table.rs`
- Modify: `pg-sql/src/ast/mod.rs`

**Step 1: Write parse tests**

```rust
#[test]
fn parse_simple_select() {
    let mut input = Input::new("SELECT 1 AS one");
    let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
    assert_eq!(stmt.columns.len(), 1);
}

#[test]
fn parse_select_from_where() {
    let mut input = Input::new("SELECT f1 FROM BOOLTBL1 WHERE f1 = true");
    let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
}

#[test]
fn parse_create_table() {
    let mut input = Input::new("CREATE TABLE BOOLTBL1 (f1 bool)");
    let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
}

#[test]
fn parse_insert() {
    let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't')");
    let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
}

#[test]
fn parse_drop_table() {
    let mut input = Input::new("DROP TABLE BOOLTBL1");
    let stmt = DropTableStmt::parse(&mut input, &SqlRules).unwrap();
}
```

**Step 2: Implement statement ASTs**

Each statement is a struct deriving `Parse` with `#[parse(rules = SqlRules)]`. Example:

```rust
#[derive(Parse, Visit, Debug)]
#[parse(rules = SqlRules)]
pub struct SelectStmt {
    pub select_kw: tokens::Select,
    pub columns: Seq<SelectItem, tokens::Comma>,
    pub from_clause: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub order_by: Option<OrderByClause>,
}
```

The top-level `Statement` enum dispatches:

```rust
#[derive(Parse, Visit, Debug)]
#[parse(rules = SqlRules)]
pub enum Statement {
    Select(SelectStmt),
    CreateTable(CreateTableStmt),
    Insert(InsertStmt),
    DropTable(DropTableStmt),
}
```

And `PsqlCommand`:

```rust
pub enum PsqlCommand {
    Statement(Statement),
    Directive(String),
}
```

`PsqlCommand` will need a manual `Parse` impl since directives are line-based (`\...` to end of line).

**Step 3: Run tests**

Run: `cargo test -p pg-sql`

**Step 4: Commit**

```bash
git add pg-sql/src/ast/
git commit -m "Add statement ASTs: SELECT, CREATE TABLE, INSERT, DROP TABLE"
```

---

## Task 6: SQL Printer

Implement a printer that converts the AST back to SQL text using the `Visit` trait.

**Files:**
- Modify: `pg-sql/src/printer.rs`

**Step 1: Write tests**

```rust
#[test]
fn print_simple_select() {
    let mut input = Input::new("SELECT 1 AS one");
    let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
    let sql = print_statement(&Statement::Select(stmt));
    // Just needs to be valid SQL, not identical to original
    assert!(sql.to_uppercase().contains("SELECT"));
    assert!(sql.contains("1"));
}
```

**Step 2: Implement printer**

The printer walks the AST and emits SQL text. It doesn't need to match original formatting — just produce valid, semantically equivalent SQL.

```rust
pub fn print_psql_commands(commands: &[PsqlCommand]) -> String {
    let mut output = String::new();
    for cmd in commands {
        match cmd {
            PsqlCommand::Directive(d) => {
                output.push_str(d);
                output.push('\n');
            }
            PsqlCommand::Statement(stmt) => {
                print_statement_to(&mut output, stmt);
                output.push_str(";\n");
            }
        }
    }
    output
}
```

**Step 3: Run tests**

Run: `cargo test -p pg-sql`

**Step 4: Commit**

```bash
git add pg-sql/src/printer.rs
git commit -m "Add SQL printer for AST-to-text conversion"
```

---

## Task 7: Test Harness

Build the test harness that runs the full pipeline: parse → print → psql → compare.

**Files:**
- Create: `pg-sql/tests/regress.rs`
- Create: `pg-sql/src/harness.rs`

**Step 1: Implement output comparison**

The harness needs to:
1. Read a `.sql` fixture file
2. Parse into `Vec<PsqlCommand>`
3. Print back to SQL text
4. Shell out to `psql` with the printed text
5. Capture stdout
6. Read the corresponding `.out` fixture
7. Strip echoed SQL from both actual and expected
8. Compare result-only lines

```rust
pub fn run_regression_test(test_name: &str) -> Result<(), String> {
    let sql_path = format!("fixtures/sql/{test_name}.sql");
    let out_path = format!("fixtures/expected/{test_name}.out");

    let sql_source = std::fs::read_to_string(&sql_path)
        .map_err(|e| format!("cannot read {sql_path}: {e}"))?;
    let expected = std::fs::read_to_string(&out_path)
        .map_err(|e| format!("cannot read {out_path}: {e}"))?;

    // Parse
    let mut input = Input::new(&sql_source);
    let commands: Vec<PsqlCommand> = parse_sql_file(&mut input)?;

    // Print
    let printed = print_psql_commands(&commands);

    // Execute via psql
    let actual = execute_via_psql(&printed)?;

    // Compare (strip echoed SQL)
    let expected_results = strip_echoed_sql(&expected);
    let actual_results = strip_echoed_sql(&actual);

    if expected_results != actual_results {
        return Err(format!("Output mismatch for {test_name}"));
    }

    Ok(())
}
```

**Step 2: Write the boolean.sql regression test**

```rust
#[test]
fn regress_boolean() {
    run_regression_test("boolean").unwrap();
}
```

Note: This test requires a running Postgres database. It should be marked `#[ignore]` by default and run explicitly with `cargo test -p pg-sql -- --ignored`.

**Step 3: Implement the harness**

The `strip_echoed_sql` function needs to identify result blocks vs echoed SQL. In `.out` files, result blocks follow a pattern:
- Column header row(s)
- Separator row of dashes: `-----+------`
- Data rows
- Row count: `(N rows)` or `(N row)`
- Error messages: `ERROR:`, `NOTICE:`, etc.

Everything else (SQL statements, comments) is stripped.

**Step 4: Run with a Postgres database**

Run: `cargo test -p pg-sql -- --ignored`

This will likely fail initially as we iterate on the parser and printer to handle all constructs in boolean.sql.

**Step 5: Commit**

```bash
git add pg-sql/tests/regress.rs pg-sql/src/harness.rs
git commit -m "Add regression test harness with boolean.sql test"
```

---

## Task 8: Iterate Until boolean.sql Passes

This is the iterative task — run the regression test, fix parse errors, fix printer issues, and repeat until `boolean.sql` passes.

This task is intentionally open-ended. Common issues to expect:

1. **Missing tokens** — add keywords/operators as needed
2. **Postfix operators** — `IS TRUE`, `::type` may need recursa changes (postfix Pratt support)
3. **Expression precedence** — adjust binding powers
4. **Printer bugs** — missing spaces, wrong formatting
5. **Edge cases** — multiple FROM tables, qualified wildcards (`BOOLTBL1.*`)

**Strategy:** Run the test, read the first error, fix it, repeat.

Run: `cargo test -p pg-sql -- --ignored --nocapture`

---

## Summary

| Task | What it delivers |
|------|-----------------|
| 1 | Crate scaffold with vendored fixtures |
| 2 | SqlRules with nested block comment support |
| 3 | SQL tokens (keywords, punctuation, literals) |
| 4 | Expression AST with Pratt parsing |
| 5 | Statement ASTs (SELECT, CREATE TABLE, INSERT, DROP TABLE) |
| 6 | SQL printer (AST → text) |
| 7 | Test harness (parse → print → psql → compare) |
| 8 | Iterate until boolean.sql passes |

**Known recursa gaps that may need addressing:**
- Postfix operators in Pratt parsing (for `::type` casts and `IS TRUE` tests)
- Keyword-aware identifier scanning (identifiers that don't match keywords)
- `\pset` directive handling in the test harness
