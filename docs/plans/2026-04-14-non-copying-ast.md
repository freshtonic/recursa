# Non-Copying AST Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace owned `String` AST fields with `Cow<'input, str>` borrowed from the input source, eliminating per-token allocations during parsing.

**Architecture:** Change the `literals!` macro and `recursa-derive` scanner-tuple codegen to produce `Cow<'input, str>` leaves holding `Cow::Borrowed(&input[..])`. Then propagate `'input` lifetimes through every `pg-sql` AST type that transitively contains a captured literal, driven file-by-file by compiler errors. Hand-written `String` sites in `pg-sql/src/ast/mod.rs` and `pg-sql/src/tokens.rs` are updated individually.

**Tech Stack:** Rust 2024 edition, `recursa-core`, `recursa-derive` (proc-macro), `pg-sql` crate. No new dependencies — `std::borrow::Cow` only.

**Reference design:** `docs/plans/2026-04-14-non-copying-ast-design.md`

**Worktree:** `/Users/jamessadler/projects/recursa/.worktrees/non-copying-ast`
**Branch:** `feature/non-copying-ast` off `main` at `18de74a`

**Known pre-existing failure:** `pg-sql` test `ast::tests::parse_with_sql_fixture` stack-overflows on main. Treat as noise; verify it still behaves the same way at the end. All *other* tests must pass.

---

## Ground Rules

- **TDD:** each task writes or updates a test first where applicable. For mechanical lifetime propagation, the compiler *is* the test — a clean `cargo check` is the success signal.
- **Commit after every green task.** Small commits, conventional messages (`feat:`, `refactor:`, `chore:`).
- **Never use `--no-verify`.** Fix hook failures at the source.
- **No manual `Parse` / `Clone` / `Debug` impls.** Per `CLAUDE.md`. If a derive fails, fix the underlying type, don't hand-write the impl.
- **Verify the worktree before each commit:** `cargo build --workspace` must succeed.

---

## Task 1: Change `recursa-derive` scanner-tuple codegen to emit `Cow::Borrowed`

**Files:**
- Modify: `recursa-derive/src/parse_derive.rs:199-235`

**Context:** `derive_scanner_tuple` has two branches. The lifetime branch (line 199) currently emits `#name(matched)` assuming a `&'input str` field. The no-lifetime branch (line 220) emits `#name(matched.to_string())` for `String` fields. We're changing the lifetime branch to wrap in `Cow::Borrowed`, so captured-literal structs declared as `Foo<'input>(Cow<'input, str>)` work. The no-lifetime branch is left intact for backwards compatibility with any `String`-field scanner tuples, but after Task 2 nothing in this workspace will use it.

**Step 1: Edit the lifetime branch**

Change parse_derive.rs:203 from:
```rust
let parse_body =
    scanner_parse_body(pattern, &anchored, quote! { #name(matched) }, postcondition);
```
to:
```rust
let parse_body = scanner_parse_body(
    pattern,
    &anchored,
    quote! { #name(::std::borrow::Cow::Borrowed(matched)) },
    postcondition,
);
```

**Step 2: Check compilation**

Run: `cargo check -p recursa-derive`
Expected: clean.

**Step 3: Commit**

```bash
git add recursa-derive/src/parse_derive.rs
git commit -m "refactor(recursa-derive): emit Cow::Borrowed for scanner-tuple leaves with lifetime"
```

---

## Task 2: Update `literals!` macro to generate `Cow<'input, str>` leaves

**Files:**
- Modify: `recursa-core/src/macros.rs:82-113`

**Context:** The `literals!` macro currently generates `pub struct $name(pub String);`. We change it to generate `pub struct $name<'input>(pub ::std::borrow::Cow<'input, str>);` and update the enum wrapper and `FormatTokens` impls accordingly.

**Step 1: Rewrite the `literals!` macro body**

Replace the macro body (lines 84-113) with:
```rust
macro_rules! literals {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[parse(pattern = $pattern)]
            #[visit(terminal)]
            pub struct $name<'input>(pub ::std::borrow::Cow<'input, str>);

            impl<'input> $crate::fmt::FormatTokens for $name<'input> {
                fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                    tokens.push($crate::fmt::Token::String(self.0.as_ref().to_string()));
                }
            }
        )*

        #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[visit(terminal)]
        pub enum Literal<'input> {
            $($name($name<'input>)),*
        }

        impl<'input> $crate::fmt::FormatTokens for Literal<'input> {
            fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                match self {
                    $(Literal::$name(inner) => inner.format_tokens(tokens),)*
                }
            }
        }
    };
}
```

**Step 2: Check recursa-core**

Run: `cargo check -p recursa-core`
Expected: clean (recursa-core itself does not instantiate `literals!`; only consumers do).

**Step 3: Commit**

```bash
git add recursa-core/src/macros.rs
git commit -m "feat(recursa-core): literals! generates Cow<'input, str> leaves"
```

---

## Task 3: Update `Visit for String` → `Visit for Cow<'_, str>`

**Files:**
- Modify: `recursa-core/src/visitor.rs:156-167`

**Context:** The visitor leaf impl for `String` is only used transitively via types that currently hold `String`. After Task 2, the leaves hold `Cow<'_, str>`. We replace the `String` impl with one for `Cow<'a, str>`.

**Step 1: Read the current impl**

Run: `sed -n '150,167p' recursa-core/src/visitor.rs` (you'll see `impl AsNodeKey for String`, `impl Visit for String`).

**Step 2: Replace impls**

Change:
```rust
// -- Leaf Visit impl for String --

impl AsNodeKey for String {}
impl Visit for String {
    // ... body ...
}
```

to:
```rust
// -- Leaf Visit impl for Cow<str> --

impl<'a> AsNodeKey for ::std::borrow::Cow<'a, str> {}
impl<'a> Visit for ::std::borrow::Cow<'a, str> {
    // ... same body ...
}
```

Keep the body identical (it's a terminal leaf — no children to recurse into).

**Step 3: Check**

Run: `cargo check -p recursa-core`
Expected: clean.

**Step 4: Commit**

```bash
git add recursa-core/src/visitor.rs
git commit -m "refactor(recursa-core): Visit impl for Cow<str> replaces String"
```

---

## Task 4: Build checkpoint — observe cascade

**Files:** none (diagnostic only)

**Step 1: Attempt workspace build**

Run: `cargo build --workspace 2>&1 | head -80`
Expected: a large number of errors in `pg-sql/src/tokens.rs` and `pg-sql/src/ast/*.rs` — the cascade we're about to fix. Do NOT try to interpret all of them; just confirm the failure shape (missing lifetimes on captured-literal wrappers, `String` vs `Cow` mismatches).

**Step 2: Record error count as a progress marker**

Run: `cargo build --workspace 2>&1 | grep -c '^error'`
Write the number down in your scratch notes. It should monotonically decrease through Tasks 5–14.

---

## Task 5: Update `pg-sql/src/tokens.rs` — literal invocation + hand-written leaves

**Files:**
- Modify: `pg-sql/src/tokens.rs` (the `literals! { ... }` block, plus `UnquotedIdent`, `BareAliasName`, `RestOfLine`)

**Context:** The `literals!` block now generates `<'input>` types. Downstream users of `StringLit`, `NumericLit`, `DollarStringLit`, `EscapeStringLit`, etc. must reference them as `StringLit<'input>`. Three hand-written scanner tuples in this file also hold `String` and must convert to `Cow<'input, str>`.

**Step 1: Change the three hand-written leaves**

Find each of:
```rust
pub struct UnquotedIdent(pub String);
pub struct BareAliasName(pub String);
pub struct RestOfLine(pub String);
```
and change each to:
```rust
pub struct UnquotedIdent<'input>(pub ::std::borrow::Cow<'input, str>);
pub struct BareAliasName<'input>(pub ::std::borrow::Cow<'input, str>);
pub struct RestOfLine<'input>(pub ::std::borrow::Cow<'input, str>);
```

Update each corresponding `FormatTokens` impl from:
```rust
impl recursa::fmt::FormatTokens for UnquotedIdent {
    fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
        tokens.push(recursa::fmt::Token::String(self.0.clone()));
    }
}
```
to:
```rust
impl<'input> recursa::fmt::FormatTokens for UnquotedIdent<'input> {
    fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
        tokens.push(recursa::fmt::Token::String(self.0.as_ref().to_string()));
    }
}
```
(apply equivalently to `BareAliasName` and `RestOfLine`).

**Step 2: Re-check tokens.rs compiles**

Run: `cargo check -p pg-sql 2>&1 | head -40`
Expected: errors now shift into `ast/*.rs` files referencing these types. Errors *inside* `tokens.rs` itself should be gone.

**Step 3: Fix any in-file test constructions**

`tokens.rs` contains inline tests constructing `StringLit`, `UnquotedIdent`, etc. Any `Foo("x".to_string())` or `Foo(s)` where `s: String` must become `Foo(::std::borrow::Cow::Borrowed("x"))` or `Foo(s.into())`.

Run: `cargo check -p pg-sql --tests 2>&1 | grep tokens.rs | head -20`

Fix each line shown. Repeat until `tokens.rs` compiles cleanly.

**Step 4: Commit**

```bash
git add pg-sql/src/tokens.rs
git commit -m "refactor(pg-sql): propagate 'input lifetime through tokens.rs leaves"
```

---

## Tasks 6–13: Lifetime propagation through `pg-sql/src/ast/*.rs`

**Strategy:** One file per task, compiler-error-driven. Inside each task:

1. Run `cargo check -p pg-sql 2>&1 | grep 'pg-sql/src/ast/<file>' | head -30` to enumerate errors in that file.
2. For every type whose fields transitively hold a captured literal (nearly all), add `<'input>` to the `struct` or `enum` declaration and to all field references and generic bounds.
3. Update any `String` constructions in tests inside the file to `Cow::Borrowed(...)` / `.into()`.
4. Re-check the file alone: `cargo check -p pg-sql 2>&1 | grep -c 'pg-sql/src/ast/<file>'` should reach 0 (new errors may appear in *other* files — that's fine; they're the next task).
5. Commit.

**Mechanical rule for adding the lifetime:**
- If the struct/enum contains *any* field of type that carries `'input`, add `<'input>` and propagate it to those field types.
- Pure keyword/punctuation-only types stay plain.
- `Option<T<'input>>`, `Vec<T<'input>>`, `Box<T<'input>>`, `Surrounded<O, T<'input>, C>`, `Seq<T<'input>, …>` — all cascade naturally.

**File order (leaves first, to minimize error churn):**

- **Task 6:** `pg-sql/src/ast/common.rs` (49 LOC)
- **Task 7:** `pg-sql/src/ast/expr.rs` (2045 LOC — this is the big one)
- **Task 8:** `pg-sql/src/ast/with_clause.rs`, `values.rs`, `select.rs` (batch — they share `Expr`)
- **Task 9:** `pg-sql/src/ast/insert.rs`, `update.rs`, `delete.rs`, `merge.rs` (batch — DML)
- **Task 10:** `pg-sql/src/ast/create_table.rs`, `create_index.rs`, `create_view.rs`, `partition.rs`, `drop_table.rs` (batch — schema DDL)
- **Task 11:** `pg-sql/src/ast/create_function.rs`, `create_procedure.rs`, `create_tablespace.rs`, `set_reset.rs`, `simple_stmts.rs`, `explain.rs`, `analyze.rs` (batch — remaining stmts)
- **Task 12:** `pg-sql/src/ast/mod.rs` — top-level `FileItem`, `SqlStatement`, any root enums. Includes the hand-written `DollarStringLit { text: String }` and `RawLines(String)` sites.
- **Task 13:** Anything the compiler still complains about — mop-up.

Each of these is a *task* following the recipe above. Commit after each file compiles. Example commit message: `refactor(pg-sql): add 'input lifetime to ast::expr`.

Special note for Task 12 (`ast/mod.rs`):

- `DollarStringLit { text: String }` near line 219 — change field to `text: ::std::borrow::Cow<'input, str>`. The struct has a manual `Parse` impl; find it (grep `impl.*Parse.*DollarStringLit`) and update it to store `::std::borrow::Cow::Borrowed(&input.source()[start..end])` (or equivalent — the input slice is already accessible; no new data needed).
- `RawLines(String)` near line 352 — change to `RawLines(::std::borrow::Cow<'input, str>)`. Find the `raw_buf` accumulator near line 410. **Inspect:** if the raw lines are contiguous in the source slice, replace `raw_buf` with a start/end capture and store `Cow::Borrowed(&source[start..end])`. If they're not contiguous (interleaved with other parsing), keep the `String` accumulator and store as `Cow::Owned(raw_buf)`. Either works — `Cow` is chosen precisely to allow this escape hatch.

---

## Task 14: Full workspace build

**Step 1:** Run `cargo build --workspace 2>&1 | tail -20`
Expected: `Finished dev` — clean build.

**Step 2:** Commit any accumulated test-fix work that isn't yet committed.

---

## Task 15: Run tests

**Step 1:** Run the full test suite, excluding the known-broken test.

Run: `cargo test --workspace -- --skip parse_with_sql_fixture 2>&1 | tail -30`

Expected: all other tests pass.

**Step 2:** Diagnose any failures. Common issues:
- Test constructors still using `.to_string()` or `String::from` on AST fields — change to `.into()` or `Cow::Borrowed`.
- Pattern matches on `String` — change to binding and `.as_ref()` or `&**s`.
- `assert_eq!` comparisons between `String` and `Cow` — add `&**s == "expected"` or `s.as_ref() == "expected"`.

**Step 3:** Re-run the known-broken test and confirm it still stack-overflows the same way (regression check — if it suddenly passes, even better; if it fails differently, investigate).

Run: `cargo test -p pg-sql ast::tests::parse_with_sql_fixture 2>&1 | tail -10`
Expected: same stack overflow as baseline — unchanged behavior.

**Step 4:** Commit any test fixes.

---

## Task 16: Clippy pass

**Step 1:** Run `cargo clippy --workspace --all-targets 2>&1 | tail -30`

**Step 2:** Fix warnings. Likely candidates:
- `needless_borrow` where `&self.0` should become `self.0.as_ref()`.
- `needless_lifetimes` on fns where elision now applies.
- `redundant_clone` around `Cow` construction.

**Step 3:** Commit.

```bash
git commit -m "chore(pg-sql): clippy cleanup after Cow migration"
```

---

## Task 17: Sanity check — allocations removed

**Step 1:** Pick one stress file. Run the existing parse benchmark if there is one, otherwise:

Run: `cargo run --release --bin gen_stress -- --help` (orient yourself; this is the stress generator).

**Step 2:** Use an existing parse benchmark (check `pg-sql/benches/` or similar) to confirm throughput improved or at least didn't regress. This is a sanity check, not a gate — the real payoff is the allocator churn reduction.

**Step 3:** No commit. Report numbers in the final summary.

---

## Done

Final check:
```bash
git log main..feature/non-copying-ast --oneline
cargo build --workspace
cargo test --workspace -- --skip parse_with_sql_fixture
cargo clippy --workspace --all-targets
```

All must be clean. Then hand off to `cipherpowers:finishing-a-development-branch` for merge/PR options.
