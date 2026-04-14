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
