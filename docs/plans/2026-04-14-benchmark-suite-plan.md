# pg-sql Benchmark Suite Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Criterion-based benchmark suite for pg-sql that measures throughput on the SQL fixture corpus and generated stress tests, comparing head-to-head against sqlparser-rs.

**Architecture:** A single `pg-sql/benches/parse.rs` hosts four Criterion groups (full corpus, head-to-head intersection, per-shape stress, aggregate stress). A `gen-stress` binary deterministically regenerates committed stress fixtures under `pg-sql/fixtures/stress/`.

**Tech Stack:** Rust, Criterion 0.5 (`html_reports`), sqlparser 0.52 (`PostgreSqlDialect`), recursa/pg-sql for the parser under test.

**Design reference:** `docs/plans/2026-04-14-benchmark-suite-design.md`

---

## Notes for the executor

- Each task ends with a commit. Commit messages follow the `feat(pg-sql): ...` / `chore(pg-sql): ...` convention used in recent history.
- pg-sql's baseline has ~224 failing lib tests representing known grammar coverage gaps. **Do not "fix" them** — they are orthogonal to this work. Only fail the build if *new* failures appear.
- The parser entry point is `pg_sql::ast::parse_sql_file(&mut Input::new(&source))`.
- All paths below are relative to the repository root (the worktree).

---

## Task 1: Add dev-dependencies and bench target to pg-sql/Cargo.toml

**Files:**
- Modify: `pg-sql/Cargo.toml`

**Step 1: Edit Cargo.toml**

Add `criterion` and `sqlparser` to `[dev-dependencies]`, and register the bench target. Final `[dev-dependencies]` block and new `[[bench]]` section:

```toml
[dev-dependencies]
testcontainers = { version = "0.27", features = ["blocking"] }
testcontainers-modules = { version = "0.15", features = ["postgres"] }
criterion = { version = "0.5", features = ["html_reports"] }
sqlparser = "0.52"

[[bench]]
name = "parse"
harness = false
```

**Step 2: Verify the manifest still builds**

Run: `cargo check -p pg-sql --benches`

Expected: compilation succeeds. It is fine for `benches/parse.rs` to not yet exist — `cargo check --benches` without the file will still succeed at the manifest level once we create a stub. If `cargo check` complains about missing bench source, proceed to Task 2 first and re-verify there.

Actually — create an empty stub now to keep this task self-contained:

**Step 3: Create empty bench stub**

Create `pg-sql/benches/parse.rs` with:

```rust
fn main() {}
```

**Step 4: Verify**

Run: `cargo check -p pg-sql --benches`
Expected: success.

**Step 5: Commit**

```bash
git add pg-sql/Cargo.toml pg-sql/benches/parse.rs
git commit -m "chore(pg-sql): add criterion and sqlparser bench dependencies"
```

---

## Task 2: Create the stress fixture generator binary

**Files:**
- Create: `pg-sql/src/bin/gen_stress.rs`

**Step 1: Write the generator**

Create `pg-sql/src/bin/gen_stress.rs`:

```rust
//! Deterministically regenerates stress-test SQL fixtures under
//! `pg-sql/fixtures/stress/`. Run with `cargo run --bin gen-stress -p pg-sql`.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("fixtures/stress");
    fs::create_dir_all(&out).expect("create stress dir");

    for n in [100usize, 1_000, 10_000] {
        write(&out, &format!("insert_values_{n}.sql"), &insert_values(n));
    }
    for n in [10usize, 100, 1_000] {
        write(&out, &format!("bool_chain_{n}.sql"), &bool_chain(n));
    }
    for n in [100usize, 1_000, 10_000] {
        write(&out, &format!("select_list_{n}.sql"), &select_list(n));
    }
    for n in [10usize, 50, 100] {
        write(&out, &format!("nested_subquery_{n}.sql"), &nested_subquery(n));
    }
    for n in [100usize, 1_000, 10_000] {
        write(&out, &format!("in_list_{n}.sql"), &in_list(n));
    }

    println!("wrote stress fixtures to {}", out.display());
}

fn write(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

fn insert_values(n: usize) -> String {
    let mut s = String::from("INSERT INTO t (a, b, c, d) VALUES\n");
    for i in 0..n {
        if i > 0 {
            s.push_str(",\n");
        }
        s.push_str(&format!("  ({i}, 'x{i}', {i}.5, true)"));
    }
    s.push_str(";\n");
    s
}

fn bool_chain(n: usize) -> String {
    let mut s = String::from("SELECT 1 WHERE ");
    for i in 0..n {
        if i > 0 {
            s.push_str(" AND ");
        }
        s.push('a');
    }
    s.push_str(";\n");
    s
}

fn select_list(n: usize) -> String {
    let mut s = String::from("SELECT ");
    for i in 0..n {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("c{i}"));
    }
    s.push_str(" FROM t;\n");
    s
}

fn nested_subquery(n: usize) -> String {
    let mut s = String::from("SELECT * FROM ");
    for _ in 0..n {
        s.push_str("(SELECT * FROM ");
    }
    s.push('t');
    for i in 0..n {
        s.push_str(&format!(") s{i}"));
    }
    s.push_str(";\n");
    s
}

fn in_list(n: usize) -> String {
    let mut s = String::from("SELECT * FROM t WHERE x IN (");
    for i in 0..n {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("{i}"));
    }
    s.push_str(");\n");
    s
}
```

**Step 2: Run the generator**

Run: `cargo run --bin gen-stress -p pg-sql`

Expected output: `wrote stress fixtures to .../pg-sql/fixtures/stress` and 15 files created under `pg-sql/fixtures/stress/`.

**Step 3: Verify the files exist**

Run: `ls pg-sql/fixtures/stress/`

Expected: 15 `.sql` files matching the shapes and sizes in the design doc.

**Step 4: Spot-check one file**

Run: `head -3 pg-sql/fixtures/stress/insert_values_100.sql`

Expected: `INSERT INTO t (a, b, c, d) VALUES` followed by rows.

**Step 5: Commit**

```bash
git add pg-sql/src/bin/gen_stress.rs pg-sql/fixtures/stress/
git commit -m "feat(pg-sql): add gen-stress binary and generated stress fixtures"
```

---

## Task 3: Implement the full-corpus benchmark (Group A)

**Files:**
- Modify: `pg-sql/benches/parse.rs`

**Step 1: Replace the stub with Group A**

Replace `pg-sql/benches/parse.rs` with:

```rust
//! pg-sql benchmarks. See docs/plans/2026-04-14-benchmark-suite-design.md.

use std::fs;
use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pg_sql::ast::parse_sql_file;
use recursa::Input;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Load every `.sql` file under `dir` (non-recursive) into
/// `(filename, contents)` pairs, sorted by filename for determinism.
fn load_sql_dir(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("sql"))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let contents = fs::read_to_string(e.path()).expect("read fixture");
            (name, contents)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn parse_with_pg_sql(sql: &str) -> bool {
    let mut input = Input::new(sql);
    parse_sql_file(&mut input).is_ok()
}

fn bench_corpus_full(c: &mut Criterion) {
    let corpus = load_sql_dir(&fixtures_root().join("sql"));
    let total_bytes: u64 = corpus.iter().map(|(_, s)| s.len() as u64).sum();
    let file_count = corpus.len();

    let mut group = c.benchmark_group("corpus/pg-sql-full");
    group.throughput(Throughput::Bytes(total_bytes));
    group.bench_function(BenchmarkId::from_parameter(file_count), |b| {
        b.iter(|| {
            let mut ok = 0u32;
            for (_, sql) in &corpus {
                if parse_with_pg_sql(sql) {
                    ok += 1;
                }
            }
            criterion::black_box(ok);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_corpus_full);
criterion_main!(benches);
```

**Step 2: Verify it compiles**

Run: `cargo check -p pg-sql --benches`
Expected: success.

**Step 3: Smoke-run the bench**

Run: `cargo bench -p pg-sql --bench parse -- --quick corpus/pg-sql-full`

Expected: Criterion runs the group and reports a throughput number in MB/s or GB/s. Exact numbers will vary; what matters is that it ran without panicking.

**Step 4: Commit**

```bash
git add pg-sql/benches/parse.rs
git commit -m "feat(pg-sql): add full-corpus throughput benchmark"
```

---

## Task 4: Add the head-to-head comparison benchmark (Group B)

**Files:**
- Modify: `pg-sql/benches/parse.rs`

**Step 1: Add sqlparser helper and group B function**

Add at the top of `parse.rs` below existing imports:

```rust
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser as SqlParser;
```

Add these helpers below `parse_with_pg_sql`:

```rust
fn parse_with_sqlparser(sql: &str) -> bool {
    SqlParser::parse_sql(&PostgreSqlDialect {}, sql).is_ok()
}
```

Add a new bench function:

```rust
fn bench_corpus_head_to_head(c: &mut Criterion) {
    let corpus = load_sql_dir(&fixtures_root().join("sql"));
    let total = corpus.len();

    let mut pg_ok = 0usize;
    let mut sp_ok = 0usize;
    let intersection: Vec<(String, String)> = corpus
        .into_iter()
        .filter(|(_, sql)| {
            let a = parse_with_pg_sql(sql);
            let b = parse_with_sqlparser(sql);
            if a {
                pg_ok += 1;
            }
            if b {
                sp_ok += 1;
            }
            a && b
        })
        .collect();

    let bytes: u64 = intersection.iter().map(|(_, s)| s.len() as u64).sum();
    eprintln!(
        "corpus: {total} files, pg-sql accepts {pg_ok}, sqlparser accepts {sp_ok}, intersection {}",
        intersection.len()
    );

    let mut group = c.benchmark_group("corpus/head-to-head");
    group.throughput(Throughput::Bytes(bytes));

    group.bench_function("pg-sql", |b| {
        b.iter(|| {
            for (_, sql) in &intersection {
                criterion::black_box(parse_with_pg_sql(sql));
            }
        });
    });

    group.bench_function("sqlparser", |b| {
        b.iter(|| {
            for (_, sql) in &intersection {
                criterion::black_box(parse_with_sqlparser(sql));
            }
        });
    });

    group.finish();
}
```

Update `criterion_group!`:

```rust
criterion_group!(benches, bench_corpus_full, bench_corpus_head_to_head);
```

**Step 2: Verify it compiles**

Run: `cargo check -p pg-sql --benches`
Expected: success.

**Step 3: Smoke-run**

Run: `cargo bench -p pg-sql --bench parse -- --quick corpus/head-to-head`

Expected: the startup `eprintln!` line appears showing acceptance counts, then two bench results (`pg-sql` and `sqlparser`) with MB/s throughput.

**Step 4: Commit**

```bash
git add pg-sql/benches/parse.rs
git commit -m "feat(pg-sql): add head-to-head corpus benchmark against sqlparser"
```

---

## Task 5: Add per-shape stress benchmarks (Group C)

**Files:**
- Modify: `pg-sql/benches/parse.rs`

**Step 1: Add shape enumeration and bench function**

Add near the top (after `load_sql_dir`):

```rust
/// (shape_name, list of (size, filename)) — must match files produced by
/// `src/bin/gen_stress.rs`. Keep in sync when adding shapes.
fn stress_shapes() -> Vec<(&'static str, Vec<(usize, String)>)> {
    vec![
        ("insert_values", vec![
            (100, "insert_values_100.sql".into()),
            (1_000, "insert_values_1000.sql".into()),
            (10_000, "insert_values_10000.sql".into()),
        ]),
        ("bool_chain", vec![
            (10, "bool_chain_10.sql".into()),
            (100, "bool_chain_100.sql".into()),
            (1_000, "bool_chain_1000.sql".into()),
        ]),
        ("select_list", vec![
            (100, "select_list_100.sql".into()),
            (1_000, "select_list_1000.sql".into()),
            (10_000, "select_list_10000.sql".into()),
        ]),
        ("nested_subquery", vec![
            (10, "nested_subquery_10.sql".into()),
            (50, "nested_subquery_50.sql".into()),
            (100, "nested_subquery_100.sql".into()),
        ]),
        ("in_list", vec![
            (100, "in_list_100.sql".into()),
            (1_000, "in_list_1000.sql".into()),
            (10_000, "in_list_10000.sql".into()),
        ]),
    ]
}

fn bench_stress_shapes(c: &mut Criterion) {
    let stress_dir = fixtures_root().join("stress");

    for (shape, sizes) in stress_shapes() {
        let mut group = c.benchmark_group(format!("stress/{shape}"));
        for (n, file) in sizes {
            let sql = fs::read_to_string(stress_dir.join(&file))
                .unwrap_or_else(|e| panic!("read {file}: {e}"));
            group.throughput(Throughput::Bytes(sql.len() as u64));

            group.bench_with_input(BenchmarkId::new("pg-sql", n), &sql, |b, sql| {
                b.iter(|| criterion::black_box(parse_with_pg_sql(sql)));
            });
            group.bench_with_input(BenchmarkId::new("sqlparser", n), &sql, |b, sql| {
                b.iter(|| criterion::black_box(parse_with_sqlparser(sql)));
            });
        }
        group.finish();
    }
}
```

Update `criterion_group!`:

```rust
criterion_group!(
    benches,
    bench_corpus_full,
    bench_corpus_head_to_head,
    bench_stress_shapes
);
```

**Step 2: Verify it compiles**

Run: `cargo check -p pg-sql --benches`
Expected: success.

**Step 3: Smoke-run**

Run: `cargo bench -p pg-sql --bench parse -- --quick stress/`

Expected: five groups (`stress/insert_values`, `stress/bool_chain`, `stress/select_list`, `stress/nested_subquery`, `stress/in_list`), each with `pg-sql/<n>` and `sqlparser/<n>` entries.

If any case panics because one parser rejects a stress input, that is a real finding — report it. Do not silently skip. Options: (a) adjust the generated shape so both parsers accept it, (b) keep the case and let the bench measure the error path (it already returns `bool`, so panics should not occur — only parse failures).

**Step 4: Commit**

```bash
git add pg-sql/benches/parse.rs
git commit -m "feat(pg-sql): add per-shape stress benchmarks"
```

---

## Task 6: Add the stress aggregate benchmark (Group D)

**Files:**
- Modify: `pg-sql/benches/parse.rs`

**Step 1: Add aggregate function**

Add below `bench_stress_shapes`:

```rust
fn bench_stress_aggregate(c: &mut Criterion) {
    let stress = load_sql_dir(&fixtures_root().join("stress"));
    let bytes: u64 = stress.iter().map(|(_, s)| s.len() as u64).sum();

    let mut group = c.benchmark_group("stress/aggregate");
    group.throughput(Throughput::Bytes(bytes));

    group.bench_function("pg-sql", |b| {
        b.iter(|| {
            for (_, sql) in &stress {
                criterion::black_box(parse_with_pg_sql(sql));
            }
        });
    });
    group.bench_function("sqlparser", |b| {
        b.iter(|| {
            for (_, sql) in &stress {
                criterion::black_box(parse_with_sqlparser(sql));
            }
        });
    });

    group.finish();
}
```

Update `criterion_group!`:

```rust
criterion_group!(
    benches,
    bench_corpus_full,
    bench_corpus_head_to_head,
    bench_stress_shapes,
    bench_stress_aggregate
);
```

**Step 2: Verify it compiles**

Run: `cargo check -p pg-sql --benches`
Expected: success.

**Step 3: Smoke-run the full suite**

Run: `cargo bench -p pg-sql --bench parse -- --quick`

Expected: all four groups run to completion without panics. Total wallclock should be a minute or two in `--quick` mode.

**Step 4: Commit**

```bash
git add pg-sql/benches/parse.rs
git commit -m "feat(pg-sql): add stress aggregate benchmark"
```

---

## Task 7: Document the benchmark suite in pg-sql/README.md

**Files:**
- Modify: `pg-sql/README.md`

**Step 1: Append a Benchmarks section**

Add to the end of `pg-sql/README.md`:

```markdown
## Benchmarks

The `parse` Criterion bench measures pg-sql parser throughput and compares against `sqlparser-rs` (`PostgreSqlDialect`).

Run everything:

```bash
cargo bench -p pg-sql
```

Filter to one group:

```bash
cargo bench -p pg-sql -- corpus/head-to-head
cargo bench -p pg-sql -- stress/insert_values
```

Groups:

- `corpus/pg-sql-full` — parse every file under `fixtures/sql/`, reports MB/s.
- `corpus/head-to-head` — parse the set of files accepted by *both* parsers; compares pg-sql vs sqlparser directly.
- `stress/<shape>` — per-shape scaling curves over generated stress inputs (`insert_values`, `bool_chain`, `select_list`, `nested_subquery`, `in_list`).
- `stress/aggregate` — single regression-check measurement across all stress files.

### Regenerating stress fixtures

Stress fixtures live in `fixtures/stress/` and are deterministic. Regenerate with:

```bash
cargo run --bin gen-stress -p pg-sql
```
```

**Step 2: Commit**

```bash
git add pg-sql/README.md
git commit -m "docs(pg-sql): document benchmark suite usage"
```

---

## Final verification

After Task 7:

1. Run `cargo check -p pg-sql --benches` — expect success.
2. Run `cargo bench -p pg-sql --bench parse -- --quick` — expect all four groups to complete without panics.
3. Run `cargo test -p pg-sql --lib` — expect the same 278 passing / 224 failing baseline as before (no new regressions).
4. `git log --oneline` — expect 7 commits on `feature/bench-suite`.

If all four checks pass, the branch is ready for review.
