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
    }
}
