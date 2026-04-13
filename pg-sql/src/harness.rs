//! Test harness for regression testing.
//!
//! Parses SQL fixture files, prints them back to SQL, executes via `psql`,
//! and compares output against expected `.out` files.

use std::path::Path;

use recursa::Input;

use recursa_core::fmt::FormatStyle;

use crate::ast::parse_sql_file;
use crate::formatter::format_file;

pub fn run_regression_test(
    test_name: &str,
    prereqs: &[&str],
    psql_uri: &str,
) -> Result<(), String> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sql_path = base.join(format!("fixtures/sql/{test_name}.sql"));

    let sql_source = std::fs::read_to_string(&sql_path)
        .map_err(|e| format!("cannot read {}: {e}", sql_path.display()))?;

    // Parse and reformat
    let mut input = Input::new(&sql_source);
    let items = parse_sql_file(&mut input).map_err(|e| format!("parse error: {e}"))?;

    if !input.is_empty() {
        return Err(format!(
            "leftover input at offset {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        ));
    }

    let printed = format_file(&items, FormatStyle::default());

    // Execute both original and reformatted SQL in separate databases
    // on the same PG instance, avoiding version mismatches with .out files.
    // Create separate databases for expected (original SQL) and actual (reformatted)
    execute_via_psql("CREATE DATABASE regress_expected;", psql_uri)?;
    execute_via_psql("CREATE DATABASE regress_actual;", psql_uri)?;

    let expected_uri = psql_uri.replace("/postgres", "/regress_expected");
    let actual_uri = psql_uri.replace("/postgres", "/regress_actual");

    // Disable parallel query for deterministic EXPLAIN output on both
    let disable_parallel =
        "ALTER SYSTEM SET max_parallel_workers_per_gather = 0; SELECT pg_reload_conf();";
    let _ = execute_via_psql(disable_parallel, psql_uri);

    // Run prerequisites on both databases
    for prereq in prereqs {
        let prereq_path = base.join(format!("fixtures/sql/{prereq}.sql"));
        let prereq_sql = std::fs::read_to_string(&prereq_path)
            .map_err(|e| format!("cannot read {}: {e}", prereq_path.display()))?;
        let _ = execute_via_psql(&prereq_sql, &expected_uri);
        let _ = execute_via_psql(&prereq_sql, &actual_uri);
    }

    let expected_output = execute_via_psql(&sql_source, &expected_uri)?;
    let actual_output = execute_via_psql(&printed, &actual_uri)?;

    // Compare outputs (strip echoed SQL from both)
    let expected_results = strip_echoed_sql(&expected_output);
    let actual_results = strip_echoed_sql(&actual_output);

    if expected_results != actual_results {
        let mut msg = format!("Output mismatch for {test_name}\n\n");

        // Show first few differences
        let max_lines = expected_results.len().max(actual_results.len());
        let mut diff_count = 0;
        for i in 0..max_lines {
            let exp = expected_results
                .get(i)
                .map(String::as_str)
                .unwrap_or("<missing>");
            let act = actual_results
                .get(i)
                .map(String::as_str)
                .unwrap_or("<missing>");
            if exp != act {
                msg.push_str(&format!("  line {i}: expected: {exp:?}\n"));
                msg.push_str(&format!("  line {i}:   actual: {act:?}\n"));
                diff_count += 1;
                if diff_count >= 10 {
                    msg.push_str("  ... (more differences omitted)\n");
                    break;
                }
            }
        }
        msg.push_str(&format!(
            "\nExpected {} result lines, got {} result lines\n",
            expected_results.len(),
            actual_results.len()
        ));
        return Err(msg);
    }

    Ok(())
}

/// Execute SQL via psql and return combined stdout+stderr with correct interleaving.
///
/// Uses `sh -c` with `2>&1` to merge stderr into stdout at the point errors occur,
/// matching how psql output appears in the expected `.out` files.
pub(crate) fn execute_via_psql(sql: &str, psql_uri: &str) -> Result<String, String> {
    let mut child = std::process::Command::new("sh")
        .env("PG_ABS_SRCDIR", "/fixtures")
        .env("PG_LIBDIR", "/usr/lib/postgresql/17/lib")
        .env("PG_DLSUFFIX", ".so")
        .args([
            "-c",
            &format!("psql '{}' --no-psqlrc --echo-all -f - 2>&1", psql_uri),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("psql failed to start: {e}"))?;

    // Write stdin in a separate thread to avoid deadlock on large inputs
    let sql_owned = sql.to_string();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(sql_owned.as_bytes());
    });

    let output = child.wait_with_output().map_err(|e| format!("psql failed: {e}"))?;
    let _ = writer.join();

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Strip echoed SQL from psql output, keeping only result lines.
///
/// In psql output (with `-f -`), SQL statements are echoed before their results.
/// Result blocks have these characteristics:
/// - Column headers (text followed by a divider line of dashes)
/// - Data rows (typically indented with leading space)
/// - Row counts like `(1 row)` or `(N rows)`
/// - Separator lines of dashes: `---+---`
/// - Error/notice messages: `ERROR:`, `NOTICE:`, `LINE`, `^`
///
/// Everything else (echoed SQL, blank lines between statements) is stripped.
pub fn strip_echoed_sql(output: &str) -> Vec<String> {
    // Pre-process: strip psql file/line prefixes from error messages.
    // psql outputs errors like `psql:/path/file.sql:7: ERROR: ...`
    // but the expected .out files just have `ERROR: ...`
    let re_psql_prefix = regex::Regex::new(r"^psql:[^:]*:\d+: ").unwrap();
    let lines: Vec<String> = output
        .lines()
        .map(|line| re_psql_prefix.replace(line, "").to_string())
        .collect();
    let lines: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Keep error/notice messages (but skip LINE N: and ^ caret lines,
        // since our reformatted SQL may have different line numbers/positions)
        if line.starts_with("ERROR:") || line.starts_with("NOTICE:") || line.starts_with("WARNING:")
        {
            result.push(line.to_string());
            i += 1;
            // Skip continuation lines: LINE N:, caret (^), indented SQL context
            while i < lines.len() {
                let cont = lines[i];
                if cont.starts_with("LINE ") || cont.trim() == "^" || cont.trim().starts_with('^') {
                    // Skip LINE and caret lines — they reference positions in the
                    // SQL text which may differ due to reformatting
                    i += 1;
                } else if cont.starts_with("DETAIL:")
                    || cont.starts_with("HINT:")
                    || cont.starts_with("CONTEXT:")
                {
                    // Keep DETAIL/HINT/CONTEXT
                    result.push(cont.to_string());
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }

        // Keep standalone DETAIL/HINT/CONTEXT lines
        if line.starts_with("DETAIL:") || line.starts_with("HINT:") || line.starts_with("CONTEXT:")
        {
            result.push(line.to_string());
            i += 1;
            continue;
        }

        // Skip standalone LINE/caret lines
        if line.starts_with("LINE ") || line.trim() == "^" || line.trim().starts_with('^') {
            i += 1;
            continue;
        }

        // Detect result table: a line followed by a divider line of dashes/pluses
        if i + 1 < lines.len() && is_divider_line(lines[i + 1]) {
            // This is a column header line
            result.push(line.to_string());
            // Add the divider
            result.push(lines[i + 1].to_string());
            i += 2;
            // Add data rows until we hit a row count or empty line
            while i < lines.len() {
                let data_line = lines[i];
                if data_line.is_empty() {
                    i += 1;
                    break;
                }
                result.push(data_line.to_string());
                // Check if this was a row count
                if is_row_count(data_line) {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Keep standalone row counts (e.g., for INSERT/CREATE/DROP)
        if is_row_count(line) {
            result.push(line.to_string());
            i += 1;
            continue;
        }

        // Skip psql status lines (CREATE TABLE, INSERT 0 1, DROP TABLE, SET, etc.)
        // The expected .out files do not include these.
        if is_psql_status_line(line) {
            i += 1;
            continue;
        }

        // Skip everything else (echoed SQL, comments, blank lines)
        i += 1;
    }

    result
}

/// Check if a line is a divider (e.g., `-----+------` or `----------`).
fn is_divider_line(line: &str) -> bool {
    let trimmed = line.trim();
    // Must be at least 3 chars to distinguish from `--` (SQL comment marker)
    if trimmed.len() < 3 {
        return false;
    }
    trimmed.chars().all(|c| c == '-' || c == '+')
}

/// Check if a line is a row count like `(1 row)` or `(5 rows)`.
fn is_row_count(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('(') && (trimmed.ends_with(" row)") || trimmed.ends_with(" rows)"))
}

/// Check if a line is a psql status message (CREATE TABLE, INSERT, etc.)
fn is_psql_status_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "CREATE TABLE"
        || trimmed == "DROP TABLE"
        || trimmed == "CREATE INDEX"
        || trimmed == "DROP INDEX"
        || trimmed == "CREATE FUNCTION"
        || trimmed == "DROP FUNCTION"
        || trimmed == "ANALYZE"
        || trimmed == "RESET"
        || (trimmed.starts_with("INSERT ") && !trimmed.starts_with("INSERT INTO"))
        || (trimmed.starts_with("DELETE ") && !trimmed.starts_with("DELETE FROM"))
        || trimmed == "SET"
        || trimmed.starts_with("Null display")
        || trimmed.starts_with("Pager")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- strip_echoed_sql tests ---

    #[test]
    fn strip_empty_input() {
        let result = strip_echoed_sql("");
        assert!(result.is_empty());
    }

    #[test]
    fn strip_keeps_result_table() {
        let output = " col1 \n------\n val1\n(1 row)\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], " col1 ");
        assert_eq!(result[1], "------");
        assert_eq!(result[2], " val1");
        assert_eq!(result[3], "(1 row)");
    }

    #[test]
    fn strip_removes_echoed_sql() {
        let output = "SELECT 1;\n ?column? \n----------\n        1\n(1 row)\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], " ?column? ");
        assert_eq!(result[1], "----------");
        assert_eq!(result[2], "        1");
        assert_eq!(result[3], "(1 row)");
    }

    #[test]
    fn strip_keeps_error_messages() {
        let output = "ERROR:  invalid input syntax for type boolean: \"junk\"\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 1);
        assert!(result[0].starts_with("ERROR:"));
    }

    #[test]
    fn strip_removes_create_table_status() {
        let output = "CREATE TABLE BOOLTBL1 (f1 bool);\nCREATE TABLE\n";
        let result = strip_echoed_sql(output);
        assert!(result.is_empty());
    }

    #[test]
    fn strip_removes_insert_status() {
        let output = "INSERT INTO t (f1) VALUES (true);\nINSERT 0 1\n";
        let result = strip_echoed_sql(output);
        assert!(result.is_empty());
    }

    #[test]
    fn strip_removes_drop_table_status() {
        let output = "DROP TABLE t;\nDROP TABLE\n";
        let result = strip_echoed_sql(output);
        assert!(result.is_empty());
    }

    #[test]
    fn strip_keeps_multi_column_result() {
        let output = " col1 | col2 \n------+------\n a    | b\n c    | d\n(2 rows)\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], " col1 | col2 ");
        assert_eq!(result[1], "------+------");
        assert_eq!(result[2], " a    | b");
        assert_eq!(result[3], " c    | d");
        assert_eq!(result[4], "(2 rows)");
    }

    #[test]
    fn strip_handles_mixed_output() {
        let output = "CREATE TABLE t (f1 bool);\nCREATE TABLE\nSELECT 1;\n ?column? \n----------\n        1\n(1 row)\n";
        let result = strip_echoed_sql(output);
        // CREATE TABLE status is stripped, only the SELECT result remains
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], " ?column? ");
    }

    // --- is_divider_line tests ---

    #[test]
    fn divider_line_dashes() {
        assert!(is_divider_line("----------"));
    }

    #[test]
    fn divider_line_dashes_and_plus() {
        assert!(is_divider_line("------+------"));
    }

    #[test]
    fn not_divider_line_text() {
        assert!(!is_divider_line(" some text "));
    }

    #[test]
    fn not_divider_line_empty() {
        assert!(!is_divider_line(""));
    }

    // --- is_row_count tests ---

    #[test]
    fn row_count_singular() {
        assert!(is_row_count("(1 row)"));
    }

    #[test]
    fn row_count_plural() {
        assert!(is_row_count("(5 rows)"));
    }

    #[test]
    fn not_row_count() {
        assert!(!is_row_count("something else"));
    }

    // --- Regression test using testcontainers ---

    #[cfg(test)]
    mod regress {
        use testcontainers::ImageExt;
        use testcontainers::runners::SyncRunner;
        use testcontainers_modules::postgres::Postgres;

        use crate::harness::run_regression_test;

        /// Start a Postgres container and return the psql connection URI.
        /// Returns the container (must be kept alive) and the URI.
        fn start_postgres() -> (testcontainers::Container<Postgres>, String) {
            let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let fixtures_dir = base.join("fixtures");

            // Use Postgres 17 to match the vendored test fixtures (from REL_17_9).
            // Mount the fixtures directory into the container so COPY commands
            // in test_setup.sql can find the data files.
            let container = Postgres::default()
                .with_tag("17")
                .with_mount(testcontainers::core::Mount::bind_mount(
                    fixtures_dir.to_str().unwrap(),
                    "/fixtures",
                ))
                .with_env_var("PG_ABS_SRCDIR", "/fixtures")
                // Use C locale for deterministic sort ordering matching expected .out files
                .with_env_var("POSTGRES_INITDB_ARGS", "--locale=C")
                .start()
                .expect("Failed to start Postgres container");

            let host = container.get_host().expect("Failed to get host");
            let port = container
                .get_host_port_ipv4(5432)
                .expect("Failed to get port");

            let uri = format!("postgres://postgres:postgres@{host}:{port}/postgres");
            (container, uri)
        }

        #[test]
        fn regress_boolean() {
            let (_c, uri) = start_postgres();
            run_regression_test("boolean", &[], &uri).unwrap();
        }

        #[test]
        fn regress_comments() {
            let (_c, uri) = start_postgres();
            run_regression_test("comments", &[], &uri).unwrap();
        }

        #[test]
        fn regress_delete() {
            let (_c, uri) = start_postgres();
            run_regression_test("delete", &[], &uri).unwrap();
        }

        #[test]
        fn regress_select() {
            let (_c, uri) = start_postgres();
            run_regression_test("select", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_with() {
            let (_c, uri) = start_postgres();
            run_regression_test("with", &["test_setup", "create_index", "create_misc"], &uri)
                .unwrap();
        }

        #[test]
        fn regress_case() {
            let (_c, uri) = start_postgres();
            run_regression_test("case", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_union() {
            let (_c, uri) = start_postgres();
            run_regression_test("union", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_subselect() {
            let (_c, uri) = start_postgres();
            run_regression_test("subselect", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_join() {
            let (_c, uri) = start_postgres();
            run_regression_test("join", &["test_setup", "create_index", "create_misc"], &uri)
                .unwrap();
        }

        #[test]
        fn regress_aggregates() {
            let (_c, uri) = start_postgres();
            run_regression_test(
                "aggregates",
                &["test_setup", "create_index", "create_misc", "create_aggregate"],
                &uri,
            )
            .unwrap();
        }

        #[test]
        fn regress_arrays() {
            let (_c, uri) = start_postgres();
            run_regression_test("arrays", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_limit() {
            let (_c, uri) = start_postgres();
            run_regression_test("limit", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_create_table() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_table", &[], &uri).unwrap();
        }

        #[test]
        fn regress_drop_if_exists() {
            let (_c, uri) = start_postgres();
            run_regression_test("drop_if_exists", &[], &uri).unwrap();
        }

        #[test]
        fn regress_insert() {
            let (_c, uri) = start_postgres();
            run_regression_test("insert", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        #[ignore = "uses psql :variable syntax not supported by parser"]
        fn regress_update() {
            let (_c, uri) = start_postgres();
            run_regression_test("update", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_returning() {
            let (_c, uri) = start_postgres();
            run_regression_test("returning", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_select_distinct() {
            let (_c, uri) = start_postgres();
            run_regression_test("select_distinct", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_select_having() {
            let (_c, uri) = start_postgres();
            run_regression_test("select_having", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_select_implicit() {
            let (_c, uri) = start_postgres();
            run_regression_test("select_implicit", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_transactions() {
            let (_c, uri) = start_postgres();
            run_regression_test("transactions", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_truncate() {
            let (_c, uri) = start_postgres();
            run_regression_test("truncate", &[], &uri).unwrap();
        }

        #[test]
        fn regress_namespace() {
            let (_c, uri) = start_postgres();
            run_regression_test("namespace", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_btree_index() {
            let (_c, uri) = start_postgres();
            run_regression_test("btree_index", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_hash_index() {
            let (_c, uri) = start_postgres();
            run_regression_test("hash_index", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_index() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_index", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_misc() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_misc", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_function_sql() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_function_sql", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_constraints() {
            let (_c, uri) = start_postgres();
            run_regression_test("constraints", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_copy() {
            let (_c, uri) = start_postgres();
            run_regression_test("copy", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_copyselect() {
            let (_c, uri) = start_postgres();
            run_regression_test("copyselect", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_copydml() {
            let (_c, uri) = start_postgres();
            run_regression_test("copydml", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_vacuum() {
            let (_c, uri) = start_postgres();
            run_regression_test("vacuum", &[], &uri).unwrap();
        }

        #[test]
        fn regress_prepared_xacts() {
            let (_c, uri) = start_postgres();
            run_regression_test("prepared_xacts", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_typed_table() {
            let (_c, uri) = start_postgres();
            run_regression_test("typed_table", &[], &uri).unwrap();
        }

        #[test]
        fn regress_select_into() {
            let (_c, uri) = start_postgres();
            run_regression_test("select_into", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_alter_table() {
            let (_c, uri) = start_postgres();
            run_regression_test("alter_table", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_bit() {
            let (_c, uri) = start_postgres();
            run_regression_test("bit", &[], &uri).unwrap();
        }

        #[test]
        fn regress_char() {
            let (_c, uri) = start_postgres();
            run_regression_test("char", &[], &uri).unwrap();
        }

        #[test]
        fn regress_cluster() {
            let (_c, uri) = start_postgres();
            run_regression_test("cluster", &["test_setup", "create_index"], &uri).unwrap();
        }

        #[test]
        fn regress_combocid() {
            let (_c, uri) = start_postgres();
            run_regression_test("combocid", &[], &uri).unwrap();
        }

        #[test]
        fn regress_conversion() {
            let (_c, uri) = start_postgres();
            run_regression_test("conversion", &[], &uri).unwrap();
        }

        #[test]
        fn regress_advisory_lock() {
            let (_c, uri) = start_postgres();
            run_regression_test("advisory_lock", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_alter_generic() {
            let (_c, uri) = start_postgres();
            run_regression_test("alter_generic", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_alter_operator() {
            let (_c, uri) = start_postgres();
            run_regression_test("alter_operator", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_amutils() {
            let (_c, uri) = start_postgres();
            run_regression_test("amutils", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_async() {
            let (_c, uri) = start_postgres();
            run_regression_test("async", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_bitmapops() {
            let (_c, uri) = start_postgres();
            run_regression_test("bitmapops", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_box() {
            let (_c, uri) = start_postgres();
            run_regression_test("box", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_brin() {
            let (_c, uri) = start_postgres();
            run_regression_test("brin", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_brin_bloom() {
            let (_c, uri) = start_postgres();
            run_regression_test("brin_bloom", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_brin_multi() {
            let (_c, uri) = start_postgres();
            run_regression_test("brin_multi", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_circle() {
            let (_c, uri) = start_postgres();
            run_regression_test("circle", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_collate() {
            let (_c, uri) = start_postgres();
            run_regression_test("collate", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_collate_icu_utf8() {
            let (_c, uri) = start_postgres();
            run_regression_test("collate.icu.utf8", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_collate_linux_utf8() {
            let (_c, uri) = start_postgres();
            run_regression_test("collate.linux.utf8", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_collate_utf8() {
            let (_c, uri) = start_postgres();
            run_regression_test("collate.utf8", &["test_setup"], &uri).unwrap();
        }

        #[test]
        #[ignore = "Windows-specific collation test"]
        fn regress_collate_windows_win1252() {
            let (_c, uri) = start_postgres();
            run_regression_test("collate.windows.win1252", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_compression() {
            let (_c, uri) = start_postgres();
            run_regression_test("compression", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_copy2() {
            let (_c, uri) = start_postgres();
            run_regression_test("copy2", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_aggregate() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_aggregate", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_am() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_am", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_cast() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_cast", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_function_c() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_function_c", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_index_spgist() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_index_spgist", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_operator() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_operator", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_procedure() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_procedure", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_role() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_role", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_schema() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_schema", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_table_like() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_table_like", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_type() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_type", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_create_view() {
            let (_c, uri) = start_postgres();
            run_regression_test("create_view", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_database() {
            let (_c, uri) = start_postgres();
            run_regression_test("database", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_date() {
            let (_c, uri) = start_postgres();
            run_regression_test("date", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_dbsize() {
            let (_c, uri) = start_postgres();
            run_regression_test("dbsize", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_dependency() {
            let (_c, uri) = start_postgres();
            run_regression_test("dependency", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_domain() {
            let (_c, uri) = start_postgres();
            run_regression_test("domain", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_drop_operator() {
            let (_c, uri) = start_postgres();
            run_regression_test("drop_operator", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_encoding() {
            let (_c, uri) = start_postgres();
            run_regression_test("encoding", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_enum() {
            let (_c, uri) = start_postgres();
            run_regression_test("enum", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_equivclass() {
            let (_c, uri) = start_postgres();
            run_regression_test("equivclass", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_errors() {
            let (_c, uri) = start_postgres();
            run_regression_test("errors", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_euc_kr() {
            let (_c, uri) = start_postgres();
            run_regression_test("euc_kr", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_event_trigger() {
            let (_c, uri) = start_postgres();
            run_regression_test("event_trigger", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_event_trigger_login() {
            let (_c, uri) = start_postgres();
            run_regression_test("event_trigger_login", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_explain() {
            let (_c, uri) = start_postgres();
            run_regression_test("explain", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_expressions() {
            let (_c, uri) = start_postgres();
            run_regression_test("expressions", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_fast_default() {
            let (_c, uri) = start_postgres();
            run_regression_test("fast_default", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_float4() {
            let (_c, uri) = start_postgres();
            run_regression_test("float4", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_float8() {
            let (_c, uri) = start_postgres();
            run_regression_test("float8", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_foreign_data() {
            let (_c, uri) = start_postgres();
            run_regression_test("foreign_data", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_foreign_key() {
            let (_c, uri) = start_postgres();
            run_regression_test("foreign_key", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_functional_deps() {
            let (_c, uri) = start_postgres();
            run_regression_test("functional_deps", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_generated() {
            let (_c, uri) = start_postgres();
            run_regression_test("generated", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_geometry() {
            let (_c, uri) = start_postgres();
            run_regression_test("geometry", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_gin() {
            let (_c, uri) = start_postgres();
            run_regression_test("gin", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_gist() {
            let (_c, uri) = start_postgres();
            run_regression_test("gist", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_groupingsets() {
            let (_c, uri) = start_postgres();
            run_regression_test("groupingsets", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_guc() {
            let (_c, uri) = start_postgres();
            run_regression_test("guc", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_hash_func() {
            let (_c, uri) = start_postgres();
            run_regression_test("hash_func", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_hash_part() {
            let (_c, uri) = start_postgres();
            run_regression_test("hash_part", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_horology() {
            let (_c, uri) = start_postgres();
            run_regression_test("horology", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_identity() {
            let (_c, uri) = start_postgres();
            run_regression_test("identity", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_incremental_sort() {
            let (_c, uri) = start_postgres();
            run_regression_test("incremental_sort", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_index_including() {
            let (_c, uri) = start_postgres();
            run_regression_test("index_including", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_index_including_gist() {
            let (_c, uri) = start_postgres();
            run_regression_test("index_including_gist", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_indexing() {
            let (_c, uri) = start_postgres();
            run_regression_test("indexing", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_indirect_toast() {
            let (_c, uri) = start_postgres();
            run_regression_test("indirect_toast", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_inet() {
            let (_c, uri) = start_postgres();
            run_regression_test("inet", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_infinite_recurse() {
            let (_c, uri) = start_postgres();
            run_regression_test("infinite_recurse", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_inherit() {
            let (_c, uri) = start_postgres();
            run_regression_test("inherit", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_init_privs() {
            let (_c, uri) = start_postgres();
            run_regression_test("init_privs", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_insert_conflict() {
            let (_c, uri) = start_postgres();
            run_regression_test("insert_conflict", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_int2() {
            let (_c, uri) = start_postgres();
            run_regression_test("int2", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_int4() {
            let (_c, uri) = start_postgres();
            run_regression_test("int4", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_int8() {
            let (_c, uri) = start_postgres();
            run_regression_test("int8", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_interval() {
            let (_c, uri) = start_postgres();
            run_regression_test("interval", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_join_hash() {
            let (_c, uri) = start_postgres();
            run_regression_test("join_hash", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_json() {
            let (_c, uri) = start_postgres();
            run_regression_test("json", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_json_encoding() {
            let (_c, uri) = start_postgres();
            run_regression_test("json_encoding", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_jsonb() {
            let (_c, uri) = start_postgres();
            run_regression_test("jsonb", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_jsonb_jsonpath() {
            let (_c, uri) = start_postgres();
            run_regression_test("jsonb_jsonpath", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_jsonpath() {
            let (_c, uri) = start_postgres();
            run_regression_test("jsonpath", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_jsonpath_encoding() {
            let (_c, uri) = start_postgres();
            run_regression_test("jsonpath_encoding", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_largeobject() {
            let (_c, uri) = start_postgres();
            run_regression_test("largeobject", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_line() {
            let (_c, uri) = start_postgres();
            run_regression_test("line", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_lock() {
            let (_c, uri) = start_postgres();
            run_regression_test("lock", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_lseg() {
            let (_c, uri) = start_postgres();
            run_regression_test("lseg", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_macaddr() {
            let (_c, uri) = start_postgres();
            run_regression_test("macaddr", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_macaddr8() {
            let (_c, uri) = start_postgres();
            run_regression_test("macaddr8", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_maintain_every() {
            let (_c, uri) = start_postgres();
            run_regression_test("maintain_every", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_matview() {
            let (_c, uri) = start_postgres();
            run_regression_test("matview", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_md5() {
            let (_c, uri) = start_postgres();
            run_regression_test("md5", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_memoize() {
            let (_c, uri) = start_postgres();
            run_regression_test("memoize", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_merge() {
            let (_c, uri) = start_postgres();
            run_regression_test("merge", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_misc() {
            let (_c, uri) = start_postgres();
            run_regression_test("misc", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_misc_functions() {
            let (_c, uri) = start_postgres();
            run_regression_test("misc_functions", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_misc_sanity() {
            let (_c, uri) = start_postgres();
            run_regression_test("misc_sanity", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_money() {
            let (_c, uri) = start_postgres();
            run_regression_test("money", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_multirangetypes() {
            let (_c, uri) = start_postgres();
            run_regression_test("multirangetypes", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_mvcc() {
            let (_c, uri) = start_postgres();
            run_regression_test("mvcc", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_name() {
            let (_c, uri) = start_postgres();
            run_regression_test("name", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_numeric() {
            let (_c, uri) = start_postgres();
            run_regression_test("numeric", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_numeric_big() {
            let (_c, uri) = start_postgres();
            run_regression_test("numeric_big", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_numerology() {
            let (_c, uri) = start_postgres();
            run_regression_test("numerology", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_object_address() {
            let (_c, uri) = start_postgres();
            run_regression_test("object_address", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_oid() {
            let (_c, uri) = start_postgres();
            run_regression_test("oid", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_oidjoins() {
            let (_c, uri) = start_postgres();
            run_regression_test("oidjoins", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_opr_sanity() {
            let (_c, uri) = start_postgres();
            run_regression_test("opr_sanity", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_partition_aggregate() {
            let (_c, uri) = start_postgres();
            run_regression_test("partition_aggregate", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_partition_info() {
            let (_c, uri) = start_postgres();
            run_regression_test("partition_info", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_partition_join() {
            let (_c, uri) = start_postgres();
            run_regression_test("partition_join", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_partition_prune() {
            let (_c, uri) = start_postgres();
            run_regression_test("partition_prune", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_password() {
            let (_c, uri) = start_postgres();
            run_regression_test("password", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_path() {
            let (_c, uri) = start_postgres();
            run_regression_test("path", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_pg_lsn() {
            let (_c, uri) = start_postgres();
            run_regression_test("pg_lsn", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_plancache() {
            let (_c, uri) = start_postgres();
            run_regression_test("plancache", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_plpgsql() {
            let (_c, uri) = start_postgres();
            run_regression_test("plpgsql", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_point() {
            let (_c, uri) = start_postgres();
            run_regression_test("point", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_polygon() {
            let (_c, uri) = start_postgres();
            run_regression_test("polygon", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_polymorphism() {
            let (_c, uri) = start_postgres();
            run_regression_test("polymorphism", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_portals() {
            let (_c, uri) = start_postgres();
            run_regression_test("portals", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_portals_p2() {
            let (_c, uri) = start_postgres();
            run_regression_test("portals_p2", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_predicate() {
            let (_c, uri) = start_postgres();
            run_regression_test("predicate", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_prepare() {
            let (_c, uri) = start_postgres();
            run_regression_test("prepare", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_privileges() {
            let (_c, uri) = start_postgres();
            run_regression_test("privileges", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_psql() {
            let (_c, uri) = start_postgres();
            run_regression_test("psql", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_psql_crosstab() {
            let (_c, uri) = start_postgres();
            run_regression_test("psql_crosstab", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_publication() {
            let (_c, uri) = start_postgres();
            run_regression_test("publication", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_random() {
            let (_c, uri) = start_postgres();
            run_regression_test("random", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_rangefuncs() {
            let (_c, uri) = start_postgres();
            run_regression_test("rangefuncs", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_rangetypes() {
            let (_c, uri) = start_postgres();
            run_regression_test("rangetypes", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_regex() {
            let (_c, uri) = start_postgres();
            run_regression_test("regex", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_regproc() {
            let (_c, uri) = start_postgres();
            run_regression_test("regproc", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_reindex_catalog() {
            let (_c, uri) = start_postgres();
            run_regression_test("reindex_catalog", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_reloptions() {
            let (_c, uri) = start_postgres();
            run_regression_test("reloptions", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_replica_identity() {
            let (_c, uri) = start_postgres();
            run_regression_test("replica_identity", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_roleattributes() {
            let (_c, uri) = start_postgres();
            run_regression_test("roleattributes", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_rowsecurity() {
            let (_c, uri) = start_postgres();
            run_regression_test("rowsecurity", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_rowtypes() {
            let (_c, uri) = start_postgres();
            run_regression_test("rowtypes", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_rules() {
            let (_c, uri) = start_postgres();
            run_regression_test("rules", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_sanity_check() {
            let (_c, uri) = start_postgres();
            run_regression_test("sanity_check", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_security_label() {
            let (_c, uri) = start_postgres();
            run_regression_test("security_label", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_select_distinct_on() {
            let (_c, uri) = start_postgres();
            run_regression_test("select_distinct_on", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_select_parallel() {
            let (_c, uri) = start_postgres();
            run_regression_test("select_parallel", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_select_views() {
            let (_c, uri) = start_postgres();
            run_regression_test("select_views", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_sequence() {
            let (_c, uri) = start_postgres();
            run_regression_test("sequence", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_spgist() {
            let (_c, uri) = start_postgres();
            run_regression_test("spgist", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_sqljson() {
            let (_c, uri) = start_postgres();
            run_regression_test("sqljson", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_sqljson_jsontable() {
            let (_c, uri) = start_postgres();
            run_regression_test("sqljson_jsontable", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_sqljson_queryfuncs() {
            let (_c, uri) = start_postgres();
            run_regression_test("sqljson_queryfuncs", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_stats() {
            let (_c, uri) = start_postgres();
            run_regression_test("stats", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_stats_ext() {
            let (_c, uri) = start_postgres();
            run_regression_test("stats_ext", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_strings() {
            let (_c, uri) = start_postgres();
            run_regression_test("strings", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_subscription() {
            let (_c, uri) = start_postgres();
            run_regression_test("subscription", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_sysviews() {
            let (_c, uri) = start_postgres();
            run_regression_test("sysviews", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tablesample() {
            let (_c, uri) = start_postgres();
            run_regression_test("tablesample", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tablespace() {
            let (_c, uri) = start_postgres();
            run_regression_test("tablespace", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_temp() {
            let (_c, uri) = start_postgres();
            run_regression_test("temp", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_test_setup() {
            let (_c, uri) = start_postgres();
            run_regression_test("test_setup", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_text() {
            let (_c, uri) = start_postgres();
            run_regression_test("text", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tid() {
            let (_c, uri) = start_postgres();
            run_regression_test("tid", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tidrangescan() {
            let (_c, uri) = start_postgres();
            run_regression_test("tidrangescan", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tidscan() {
            let (_c, uri) = start_postgres();
            run_regression_test("tidscan", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_time() {
            let (_c, uri) = start_postgres();
            run_regression_test("time", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_timestamp() {
            let (_c, uri) = start_postgres();
            run_regression_test("timestamp", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_timestamptz() {
            let (_c, uri) = start_postgres();
            run_regression_test("timestamptz", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_timetz() {
            let (_c, uri) = start_postgres();
            run_regression_test("timetz", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_triggers() {
            let (_c, uri) = start_postgres();
            run_regression_test("triggers", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tsdicts() {
            let (_c, uri) = start_postgres();
            run_regression_test("tsdicts", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tsearch() {
            let (_c, uri) = start_postgres();
            run_regression_test("tsearch", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tsrf() {
            let (_c, uri) = start_postgres();
            run_regression_test("tsrf", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tstypes() {
            let (_c, uri) = start_postgres();
            run_regression_test("tstypes", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_tuplesort() {
            let (_c, uri) = start_postgres();
            run_regression_test("tuplesort", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_txid() {
            let (_c, uri) = start_postgres();
            run_regression_test("txid", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_type_sanity() {
            let (_c, uri) = start_postgres();
            run_regression_test("type_sanity", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_unicode() {
            let (_c, uri) = start_postgres();
            run_regression_test("unicode", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_updatable_views() {
            let (_c, uri) = start_postgres();
            run_regression_test("updatable_views", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_uuid() {
            let (_c, uri) = start_postgres();
            run_regression_test("uuid", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_vacuum_parallel() {
            let (_c, uri) = start_postgres();
            run_regression_test("vacuum_parallel", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_varchar() {
            let (_c, uri) = start_postgres();
            run_regression_test("varchar", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_window() {
            let (_c, uri) = start_postgres();
            run_regression_test("window", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_write_parallel() {
            let (_c, uri) = start_postgres();
            run_regression_test("write_parallel", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_xid() {
            let (_c, uri) = start_postgres();
            run_regression_test("xid", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_xml() {
            let (_c, uri) = start_postgres();
            run_regression_test("xml", &["test_setup"], &uri).unwrap();
        }

        #[test]
        fn regress_xmlmap() {
            let (_c, uri) = start_postgres();
            run_regression_test("xmlmap", &["test_setup"], &uri).unwrap();
        }
    }
}
