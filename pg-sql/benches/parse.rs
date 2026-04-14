//! pg-sql benchmarks. See docs/plans/2026-04-14-benchmark-suite-design.md.

use std::fs;
use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pg_sql::ast::parse_sql_file;
use recursa::Input;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser as SqlParser;

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
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // Skip fixtures we can't read as UTF-8 (e.g. collate.windows.win1252.sql)
            // or any other read failure. Log so the corpus doesn't shrink invisibly.
            match fs::read_to_string(e.path()) {
                Ok(contents) => Some((name, contents)),
                Err(err) => {
                    eprintln!("warning: skipping fixture {name}: {err}");
                    None
                }
            }
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn parse_with_pg_sql(sql: &str) -> bool {
    let mut input = Input::new(sql);
    parse_sql_file(&mut input).is_ok()
}

fn parse_with_sqlparser(sql: &str) -> bool {
    SqlParser::parse_sql(&PostgreSqlDialect {}, sql).is_ok()
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

criterion_group!(benches, bench_corpus_full, bench_corpus_head_to_head);
criterion_main!(benches);
