//! pg-sql benchmarks. See docs/plans/2026-04-14-benchmark-suite-design.md.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, BenchmarkId, Criterion,
    Throughput,
};

/// Cap warm-up and measurement time so a single pathological case (e.g. deep
/// nested subquery parsing) can't stall the whole suite. Applied to every
/// group in this file.
fn cap_time(group: &mut BenchmarkGroup<'_, WallTime>) {
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_secs(1));
    group.sample_size(10);
}
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
            (5, "nested_subquery_5.sql".into()),
            (10, "nested_subquery_10.sql".into()),
            (15, "nested_subquery_15.sql".into()),
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
        cap_time(&mut group);
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

fn bench_corpus_full(c: &mut Criterion) {
    let corpus = load_sql_dir(&fixtures_root().join("sql"));
    let total_bytes: u64 = corpus.iter().map(|(_, s)| s.len() as u64).sum();
    let file_count = corpus.len();

    let mut group = c.benchmark_group("corpus/pg-sql-full");
    cap_time(&mut group);
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
    cap_time(&mut group);
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

criterion_group!(
    benches,
    bench_corpus_full,
    bench_corpus_head_to_head,
    bench_stress_shapes
);
criterion_main!(benches);
