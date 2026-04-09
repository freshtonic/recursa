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
            let exp = expected_results.get(i).map(String::as_str).unwrap_or("<missing>");
            let act = actual_results.get(i).map(String::as_str).unwrap_or("<missing>");
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

/// Execute SQL via psql and return combined stdout+stderr.
fn execute_via_psql(sql: &str, psql_uri: &str) -> Result<String, String> {
    let output = std::process::Command::new("psql")
        .args([psql_uri, "--no-psqlrc", "-f", "-"])
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

    // Combine stdout and stderr. psql sends errors to stderr, but they're
    // interleaved in the expected .out file. Using `2>&1` via shell would
    // preserve ordering, but for now we merge stdout+stderr and rely on
    // strip_echoed_sql to normalize both sides identically.
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let mut combined = stdout;
    if !stderr.is_empty() {
        combined.push_str(&stderr);
    }

    Ok(combined)
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
    let lines: Vec<&str> = output.lines().collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Keep error/notice messages
        if line.starts_with("ERROR:") || line.starts_with("NOTICE:") || line.starts_with("WARNING:") {
            result.push(line.to_string());
            i += 1;
            // Also keep continuation lines (indented)
            while i < lines.len() && (lines[i].starts_with(' ') || lines[i].starts_with("LINE") || lines[i].starts_with('^')) {
                result.push(lines[i].to_string());
                i += 1;
            }
            continue;
        }

        // Keep DETAIL/HINT lines that follow errors
        if line.starts_with("DETAIL:") || line.starts_with("HINT:") || line.starts_with("LINE ") || line.starts_with("CONTEXT:") {
            result.push(line.to_string());
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

        // Keep psql informational output (e.g., SET, CREATE TABLE, INSERT, DROP)
        if is_psql_status_line(line) {
            result.push(line.to_string());
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
    if trimmed.is_empty() {
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
        || (trimmed.starts_with("INSERT ") && !trimmed.starts_with("INSERT INTO"))
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
    fn strip_keeps_create_table_status() {
        let output = "CREATE TABLE BOOLTBL1 (f1 bool);\nCREATE TABLE\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "CREATE TABLE");
    }

    #[test]
    fn strip_keeps_insert_status() {
        let output = "INSERT INTO t (f1) VALUES (true);\nINSERT 0 1\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "INSERT 0 1");
    }

    #[test]
    fn strip_keeps_drop_table_status() {
        let output = "DROP TABLE t;\nDROP TABLE\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "DROP TABLE");
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
        let output =
            "CREATE TABLE t (f1 bool);\nCREATE TABLE\nSELECT 1;\n ?column? \n----------\n        1\n(1 row)\n";
        let result = strip_echoed_sql(output);
        assert_eq!(result.len(), 5); // CREATE TABLE + 4 result lines
        assert_eq!(result[0], "CREATE TABLE");
        assert_eq!(result[1], " ?column? ");
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
        use testcontainers::runners::SyncRunner;
        use testcontainers_modules::postgres::Postgres;

        use crate::harness::run_regression_test;

        /// Start a Postgres container and return the psql connection URI.
        /// Returns the container (must be kept alive) and the URI.
        fn start_postgres() -> (testcontainers::Container<Postgres>, String) {
            let container = Postgres::default()
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
    }
}
