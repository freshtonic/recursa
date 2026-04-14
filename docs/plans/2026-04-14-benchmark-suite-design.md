# pg-sql Benchmark Suite — Design

## Goal

Measure pg-sql parser throughput against the full SQL fixture corpus and a
curated set of stress inputs, and compare directly against `sqlparser-rs` as a
reference implementation.

## Scope

- Criterion-based benches living in `pg-sql/benches/parse.rs`.
- Stress fixtures committed under `pg-sql/fixtures/stress/`, produced by a
  deterministic generator binary.
- Head-to-head comparison against `sqlparser` with `PostgreSqlDialect`.

Out of scope: CI integration, custom reporters, memory/allocation profiling,
per-file corpus breakdowns.

## Layout

```
pg-sql/
├── Cargo.toml              # criterion + sqlparser dev-deps; [[bench]] parse
├── benches/
│   └── parse.rs
├── src/bin/
│   └── gen_stress.rs       # regenerates fixtures/stress/
└── fixtures/
    ├── sql/                # existing 226 corpus files
    └── stress/             # committed generated stress inputs
```

**Cargo.toml additions:**

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
sqlparser = "0.52"

[[bench]]
name = "parse"
harness = false
```

## Benchmark groups

### A. `corpus/pg-sql-full`

- Load all files from `fixtures/sql/` once at startup into
  `Vec<(String, String)>`.
- One measured iteration parses every file via `parse_sql_file`.
- `Throughput::Bytes(total_bytes)` → MB/s headline.
- Parse errors counted, not fatal (today's corpus acceptance is ~81%).

### B. `corpus/head-to-head`

- Startup: parse each corpus file with both pg-sql and sqlparser-rs. Retain
  only files both accept → intersection set.
- Two benchmarks in the group: `pg-sql` and `sqlparser`, each parsing the
  intersection set.
- `Throughput::Bytes(intersection_bytes)` so MB/s is directly comparable.
- Startup prints:
  `corpus: 226 files, pg-sql accepts X, sqlparser accepts Y, intersection Z`.

### C. `stress/<shape>`

One `BenchmarkGroup` per shape, using `bench_with_input` over size parameters,
with two functions per size: `pg-sql` and `sqlparser`. `Throughput::Bytes` per
case so Criterion plots scaling curves per parser.

### D. `stress/aggregate`

Single measurement parsing all stress files per parser — quick regression
check alongside the detailed shape groups.

## Stress shapes

| Shape               | Sizes               | Template                                                          |
|---------------------|---------------------|-------------------------------------------------------------------|
| `insert_values_{N}` | 100, 1_000, 10_000  | `INSERT INTO t (a,b,c,d) VALUES (1,'x',2.5,true), ... ;` (N rows) |
| `bool_chain_{N}`    | 10, 100, 1_000      | `SELECT 1 WHERE a AND a AND a ... ;` (N terms)                    |
| `select_list_{N}`   | 100, 1_000, 10_000  | `SELECT c1, c2, ..., cN FROM t;`                                  |
| `nested_subquery_{N}` | 10, 50, 100       | `SELECT * FROM (SELECT * FROM (... FROM t) s1) s2;` (N deep)      |
| `in_list_{N}`       | 100, 1_000, 10_000  | `SELECT * FROM t WHERE x IN (1, 2, ..., N);`                      |

`bool_chain` is scaled an order of magnitude lower than flat shapes because
boolean chains exercise Pratt recursion depth rather than iterative Seq
parsing. `nested_subquery` caps at 100 for the same reason — 10k would just
measure stack overflow handling.

## Stress generator

`pg-sql/src/bin/gen_stress.rs`, invoked via
`cargo run --bin gen-stress -p pg-sql`.

- Pure string building, no RNG, no parser validation.
- Identity values (`1, 2, 3, ...`) keep output deterministic so regeneration
  only diffs intentional changes.
- Overwrites `pg-sql/fixtures/stress/` unconditionally.
- If a generated file fails to parse, the bench surfaces that as a real bug.

## Error handling during benches

- Parse errors from either parser are counted but do not panic.
- Error paths are still `black_box`'d as real work.
- Acceptance counts are reported once at startup, not per iteration.

## Workflow

- `cargo bench -p pg-sql` — runs all groups.
- `cargo bench -p pg-sql -- corpus/head-to-head` — filter to comparison.
- `cargo run --bin gen-stress -p pg-sql` — regenerate stress fixtures.

## Explicit non-goals (YAGNI)

- No custom reporters or JSON export.
- No baseline-vs-current automation beyond Criterion's built-in support.
- No per-file breakdowns in the full-corpus group.
- No memory or allocation profiling.
- No CI wiring in this task.
