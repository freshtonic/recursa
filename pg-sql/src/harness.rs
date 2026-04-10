//! Test harness for regression testing.
//!
//! Parses SQL fixture files, prints them back to SQL, executes via `psql`,
//! and compares output against expected `.out` files.

use std::path::Path;

use recursa::Input;

use crate::ast::parse_sql_file;
use crate::printer::print_commands;

/// Run a regression test for the given test name against a specific psql connection URI.
///
/// Reads `fixtures/sql/{test_name}.sql`, parses it, prints it back to SQL,
/// executes via `psql`, and compares against `fixtures/expected/{test_name}.out`.
/// Run prerequisite SQL scripts directly via psql (without parsing through recursa).
/// Used to set up tables that later regression tests depend on.
pub fn run_prerequisites(prereqs: &[&str], psql_uri: &str) -> Result<(), String> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    for prereq in prereqs {
        let sql_path = base.join(format!("fixtures/sql/{prereq}.sql"));
        let sql = std::fs::read_to_string(&sql_path)
            .map_err(|e| format!("cannot read {}: {e}", sql_path.display()))?;
        let output = execute_via_psql(&sql, psql_uri)?;
    }
    Ok(())
}

pub fn run_regression_test(test_name: &str, psql_uri: &str) -> Result<(), String> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sql_path = base.join(format!("fixtures/sql/{test_name}.sql"));
    let out_path = base.join(format!("fixtures/expected/{test_name}.out"));

    let sql_source = std::fs::read_to_string(&sql_path)
        .map_err(|e| format!("cannot read {}: {e}", sql_path.display()))?;
    let expected_output = std::fs::read_to_string(&out_path)
        .map_err(|e| format!("cannot read {}: {e}", out_path.display()))?;

    // Parse
    let mut input = Input::new(&sql_source);
    let commands = parse_sql_file(&mut input).map_err(|e| format!("parse error: {e}"))?;

    if !input.is_empty() {
        return Err(format!(
            "leftover input at offset {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        ));
    }

    // Print back to SQL
    let printed = print_commands(&commands);

    // Execute via psql
    let actual_output = execute_via_psql(&printed, psql_uri)?;

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
fn execute_via_psql(sql: &str, psql_uri: &str) -> Result<String, String> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures_dir = base.join("fixtures");

    let output = std::process::Command::new("sh")
        // PG_ABS_SRCDIR points to the CONTAINER path where fixtures are mounted,
        // not the host path. test_setup.sql uses \getenv to read this, then
        // COPY ... FROM uses the path on the server side (inside the container).
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
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(sql.as_bytes())?;
            child.wait_with_output()
        })
        .map_err(|e| format!("psql failed: {e}"))?;

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

        use crate::harness::{run_prerequisites, run_regression_test};

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
            let (_container, uri) = start_postgres();
            run_regression_test("boolean", &uri).unwrap();
        }

        #[test]
        fn regress_comments() {
            let (_container, uri) = start_postgres();
            run_regression_test("comments", &uri).unwrap();
        }

        #[test]
        fn regress_delete() {
            let (_container, uri) = start_postgres();
            run_regression_test("delete", &uri).unwrap();
        }

        #[test]
        fn regress_select() {
            let (_container, uri) = start_postgres();
            // select.sql depends on tables created by test_setup
            run_prerequisites(&["test_setup"], &uri).unwrap();
            run_regression_test("select", &uri).unwrap();
        }
    }
}
