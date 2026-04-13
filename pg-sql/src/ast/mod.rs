pub mod analyze;
pub mod create_function;
pub mod create_index;
pub mod create_table;
pub mod create_view;
pub mod delete;
pub mod drop_table;
pub mod explain;
pub mod expr;
pub mod insert;
pub mod merge;
pub mod partition;
pub mod select;
pub mod set_reset;
pub mod simple_stmts;
pub mod update;
pub mod values;
pub mod with_clause;

use recursa::{FormatTokens, Input, Parse, ParseError, ParseRules, Visit};

use crate::rules::SqlRules;
use crate::tokens::{literal, punct};

use self::{
    analyze::AnalyzeStmt,
    create_function::{CreateFunctionStmt, DropFunctionStmt},
    create_index::{CreateIndexStmt, DropIndexStmt},
    create_table::CreateTableStmt,
    create_view::{CreateViewStmt, DropViewStmt},
    delete::DeleteStmt,
    drop_table::DropTableStmt,
    explain::ExplainStmt,
    insert::InsertStmt,
    merge::MergeStmt,
    select::SelectStmt,
    set_reset::{ResetStmt, SetStmt},
    simple_stmts::*,
    update::UpdateStmt,
    values::{CompoundQuery, TableStmt},
    with_clause::WithStatement,
};

/// Top-level SQL statement.
///
/// Variant ordering matters for disambiguation. More specific (longer leading
/// keyword sequences) must come before less specific:
/// - `With` must come before `Select` so `WITH ... SELECT` matches before bare `SELECT`.
/// - `Explain` wraps a Statement, so it must come before `Select`.
/// - `CreateFunction` and `CreateIndex` come before `CreateTable` because they
///   have `CREATE FUNCTION` / `CREATE INDEX` which are longer than `CREATE TABLE`.
///   `CreateView` likewise comes before `CreateTable`.
///   `CreateTable` handles regular, partitioned, and partition-of forms internally.
/// - `DropFunction` and `DropIndex` come before `DropTable` for the same reason.
/// - `Values` (CompoundQuery) starts with VALUES/TABLE/SELECT so it could
///   conflict. It must come after Explain but before bare Select to handle
///   `VALUES ... UNION ALL ...` and `TABLE tablename`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[allow(clippy::large_enum_variant)]
pub enum Statement {
    // --- Multi-keyword statements (longest first_pattern first) ---
    With(WithStatement),
    Explain(ExplainStmt),
    // CREATE variants: longer keyword sequences before shorter
    CreateFunction(CreateFunctionStmt),
    CreateTrigger(CreateTriggerStmt),
    CreateIndex(CreateIndexStmt),
    CreateView(CreateViewStmt),
    CreateRule(CreateRuleStmt),
    CreateTable(CreateTableStmt),
    // DROP variants: longer before shorter
    DropFunction(DropFunctionStmt),
    DropTrigger(DropTriggerStmt),
    DropIndex(DropIndexStmt),
    DropView(DropViewStmt),
    DropRule(DropRuleStmt),
    DropTable(DropTableStmt),
    // ALTER TABLE
    AlterTable(AlterTableStmt),
    // DML
    Insert(InsertStmt),
    Update(UpdateStmt),
    Merge(MergeStmt),
    Delete(DeleteStmt),
    // Transaction control
    Rollback(RollbackStmt),
    Savepoint(SavepointStmt),
    Release(ReleaseStmt),
    Begin(BeginStmt),
    Commit(CommitStmt),
    // PREPARE / EXECUTE / DEALLOCATE
    Deallocate(DeallocateStmt),
    Prepare(PrepareStmt),
    Execute(ExecuteStmt),
    // Permissions
    Grant(GrantStmt),
    Revoke(RevokeStmt),
    // Utility
    SecurityLabel(SecurityLabelStmt),
    Comment(CommentStmt),
    Copy(CopyStmt),
    Truncate(TruncateStmt),
    Reindex(ReindexStmt),
    Refresh(RefreshStmt),
    Cluster(ClusterStmt),
    Vacuum(VacuumStmt),
    Lock(LockStmt),
    Notify(NotifyStmt),
    Listen(ListenStmt),
    Unlisten(UnlistenStmt),
    Discard(DiscardStmt),
    Reassign(ReassignStmt),
    Do(DoStmt),
    // Cursor
    Declare(DeclareStmt),
    Fetch(FetchStmt),
    Close(CloseStmt),
    Move(MoveStmt),
    // Configuration
    Set(SetStmt),
    Reset(ResetStmt),
    Analyze(AnalyzeStmt),
    // Query
    Values(CompoundQuery),
    Select(SelectStmt),
    Table(TableStmt),
    /// Catch-all: consumes tokens until the next semicolon.
    Raw(RawStatement),
}

/// A raw statement: consumes everything up to (but not including) the next semicolon.
///
/// Manual Parse impl needed because this is a catch-all that doesn't use structured
/// token parsing. It's intentionally the last variant in Statement.
/// To eliminate this, implement proper AST types for each statement kind.
#[derive(Debug, Clone, FormatTokens, Visit)]
pub struct RawStatement {
    pub text: String,
}

impl<'input> Parse<'input> for RawStatement {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // Match any word character to act as a fallback
        r"[a-zA-Z_]"
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        !input.is_empty()
            && input
                .remaining()
                .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        let remaining = input.remaining();
        // Find the next semicolon, respecting parenthesized groups and string literals
        let mut depth = 0i32;
        let mut in_string = false;
        let mut in_dollar_string = false;
        let mut dollar_tag = String::new();
        let chars: Vec<char> = remaining.chars().collect();
        static DOLLAR_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let dollar_re = DOLLAR_RE.get_or_init(|| regex::Regex::new(r"^\$([a-zA-Z_]*)\$").unwrap());
        let mut i = 0;
        let mut byte_pos = 0;

        while i < chars.len() {
            let c = chars[i];

            if in_dollar_string {
                // Look for closing $$ or $tag$
                if c == '$' {
                    let rest: String = chars[i..].iter().collect();
                    let end_tag = format!("${}$", dollar_tag);
                    if rest.starts_with(&end_tag) {
                        byte_pos += end_tag.len();
                        i += end_tag.chars().count();
                        in_dollar_string = false;
                        continue;
                    }
                }
                byte_pos += c.len_utf8();
                i += 1;
            } else if in_string {
                if c == '\'' {
                    // Check for escaped quote
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        byte_pos += 2;
                        i += 2;
                    } else {
                        in_string = false;
                        byte_pos += 1;
                        i += 1;
                    }
                } else {
                    byte_pos += c.len_utf8();
                    i += 1;
                }
            } else if c == '\'' {
                in_string = true;
                byte_pos += 1;
                i += 1;
            } else if c == '$' {
                // Check for dollar-quoted string: $tag$...$tag$ or $$...$$
                let rest: String = chars[i..].iter().collect();
                if let Some(m) = dollar_re.find(&rest) {
                    let tag_text = m.as_str();
                    dollar_tag = tag_text[1..tag_text.len() - 1].to_string();
                    byte_pos += tag_text.len();
                    i += tag_text.chars().count();
                    in_dollar_string = true;
                } else {
                    byte_pos += 1;
                    i += 1;
                }
            } else if c == '(' {
                depth += 1;
                byte_pos += 1;
                i += 1;
            } else if c == ')' {
                depth -= 1;
                byte_pos += 1;
                i += 1;
            } else if c == ';' && depth <= 0 {
                break;
            } else {
                byte_pos += c.len_utf8();
                i += 1;
            }
        }

        let text = remaining[..byte_pos].to_string();
        input.advance(byte_pos);
        Ok(RawStatement { text })
    }
}

/// A SQL statement followed by a semicolon.
#[derive(Debug, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TerminatedStatement {
    pub stmt: Statement,
    pub semi: punct::Semi,
}

/// A psql directive: backslash followed by the rest of the line.
#[derive(Debug, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PsqlDirective {
    pub backslash: punct::BackSlash,
    pub rest: literal::RestOfLine,
}

/// A command in a psql input file: either a SQL statement or a psql directive.
#[derive(Debug, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[allow(clippy::large_enum_variant)]
pub enum PsqlCommand {
    /// A psql directive (e.g., `\pset null '(null)'`).
    /// Listed first so `\` is checked before statement keywords.
    Directive(PsqlDirective),
    /// A SQL statement followed by a semicolon.
    Statement(TerminatedStatement),
}

/// Parse a complete SQL file into a list of commands.
///
/// Gracefully handles parse errors by falling back to RawStatement parsing,
/// which consumes everything up to the next semicolon. This allows the parser
/// to continue past intentionally invalid SQL (e.g., error test cases in
/// PostgreSQL regression test files).
/// An item in a parsed SQL file: either a parsed command or raw text
/// (e.g., COPY FROM stdin data blocks that can't be parsed as SQL).
pub enum FileItem {
    Command(PsqlCommand),
    RawLines(String),
}

/// Statistics about how statements were parsed in a file.
#[derive(Debug, Default)]
pub struct ParseStats {
    /// Statements parsed into structured AST types (SELECT, INSERT, etc.)
    pub structured: usize,
    /// Statements that fell through to RawStatement (catch-all)
    pub raw: usize,
    /// Psql directives (\pset, etc.)
    pub directives: usize,
    /// Raw lines (COPY FROM stdin data, etc.)
    pub raw_lines: usize,
}

impl ParseStats {
    /// Percentage of SQL statements (excluding directives/raw lines) that were
    /// parsed into structured AST types.
    pub fn structured_pct(&self) -> f64 {
        let total = self.structured + self.raw;
        if total == 0 {
            100.0
        } else {
            (self.structured as f64 / total as f64) * 100.0
        }
    }
}

/// Compute parsing statistics for a list of file items.
pub fn parse_stats(items: &[FileItem]) -> ParseStats {
    let mut stats = ParseStats::default();
    for item in items {
        match item {
            FileItem::Command(PsqlCommand::Statement(ts)) => {
                if matches!(ts.stmt, Statement::Raw(_)) {
                    stats.raw += 1;
                } else {
                    stats.structured += 1;
                }
            }
            FileItem::Command(PsqlCommand::Directive(_)) => {
                stats.directives += 1;
            }
            FileItem::RawLines(_) => {
                stats.raw_lines += 1;
            }
        }
    }
    stats
}

/// Parse a complete SQL file into a list of file items.
///
/// Gracefully handles parse errors and unparseable content (COPY FROM stdin
/// data blocks, etc.) by preserving them as `RawLines`.
pub fn parse_sql_file(input: &mut Input<'_>) -> Result<Vec<FileItem>, ParseError> {
    let mut items = Vec::new();
    let mut raw_buf = String::new();
    loop {
        SqlRules::consume_ignored(input);
        if input.is_empty() {
            break;
        }
        if !PsqlCommand::peek(input, &SqlRules) {
            // Collect unparseable lines (e.g., COPY FROM stdin data blocks).
            let line = take_line(input);
            raw_buf.push_str(&line);
            raw_buf.push('\n');
            continue;
        }
        // Flush any accumulated raw lines before the next command
        if !raw_buf.is_empty() {
            items.push(FileItem::RawLines(std::mem::take(&mut raw_buf)));
        }
        match PsqlCommand::parse(input, &SqlRules) {
            Ok(cmd) => items.push(FileItem::Command(cmd)),
            Err(_) => {
                // Parse error -- skip to next semicolon and create a Raw statement
                let raw = RawStatement::parse(input, &SqlRules)?;
                SqlRules::consume_ignored(input);
                if punct::Semi::peek(input, &SqlRules) {
                    let semi = punct::Semi::parse(input, &SqlRules)?;
                    items.push(FileItem::Command(PsqlCommand::Statement(
                        TerminatedStatement {
                            stmt: Statement::Raw(raw),
                            semi,
                        },
                    )));
                }
            }
        }
    }
    // Flush trailing raw lines
    if !raw_buf.is_empty() {
        items.push(FileItem::RawLines(raw_buf));
    }
    Ok(items)
}

/// Take the current line from input and advance past it.
fn take_line<'a>(input: &mut Input<'a>) -> &'a str {
    let remaining = input.remaining();
    match remaining.find('\n') {
        Some(pos) => {
            let line = &remaining[..pos];
            input.advance(pos + 1);
            line
        }
        None => {
            let line = remaining;
            input.advance(remaining.len());
            line
        }
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use super::*;

    #[test]
    fn parse_statement_select() {
        let mut input = Input::new("SELECT 1 AS one");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        // Bare SELECT now matches via CompoundQuery path since Values variant
        // precedes Select for compound query (UNION etc.) support.
        assert!(matches!(stmt, Statement::Values(_)));
    }

    #[test]
    fn parse_statement_create_table() {
        let mut input = Input::new("CREATE TABLE t (f1 bool)");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::CreateTable(_)));
    }

    #[test]
    fn parse_statement_insert() {
        let mut input = Input::new("INSERT INTO t (f1) VALUES (true)");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::Insert(_)));
    }

    #[test]
    fn parse_statement_delete() {
        let mut input = Input::new("DELETE FROM t WHERE a > 1");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::Delete(_)));
    }

    #[test]
    fn parse_statement_drop_table() {
        let mut input = Input::new("DROP TABLE t");
        let stmt = Statement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt, Statement::DropTable(_)));
    }

    #[test]
    fn parse_psql_command_statement() {
        let mut input = Input::new("SELECT 1;");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_psql_command_directive() {
        let mut input = Input::new("\\pset null '(null)'\n");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        match cmd {
            PsqlCommand::Directive(d) => assert_eq!(d.rest.0, "pset null '(null)'"),
            _ => panic!("expected directive"),
        }
    }

    #[test]
    fn parse_multiple_commands() {
        let sql = "SELECT 1;\n\\pset null '(null)'\nSELECT 2;\n";
        let mut input = Input::new(sql);
        let items = parse_sql_file(&mut input).unwrap();
        assert_eq!(items.len(), 3);
        assert!(matches!(
            items[0],
            FileItem::Command(PsqlCommand::Statement(_))
        ));
        assert!(matches!(
            items[1],
            FileItem::Command(PsqlCommand::Directive(_))
        ));
        assert!(matches!(
            items[2],
            FileItem::Command(PsqlCommand::Statement(_))
        ));
    }

    #[test]
    fn parse_select_with_where_and_bool_test() {
        let mut input = Input::new("SELECT f1 FROM BOOLTBL1 WHERE f1 IS TRUE;");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_full_insert_with_type_cast() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't');");
        let cmd = PsqlCommand::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
    }

    #[test]
    fn parse_create_drop_sequence() {
        let sql = "CREATE TABLE t (f1 bool);\nDROP TABLE t;\n";
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn parse_boolean_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/boolean.sql")
            .expect("boolean.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();

        assert!(
            commands.len() > 50,
            "expected >50 commands, got {}",
            commands.len()
        );

        assert!(
            input.is_empty(),
            "leftover input at offset {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_comments_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/comments.sql")
            .expect("comments.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 3,
            "expected >3 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_select_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/select.sql")
            .expect("select.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 10,
            "expected >10 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_union_sql_fixture() {
        let sql =
            std::fs::read_to_string("fixtures/sql/union.sql").expect("union.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 10,
            "expected >10 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_subselect_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/subselect.sql")
            .expect("subselect.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 10,
            "expected >10 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_case_sql_fixture() {
        let sql =
            std::fs::read_to_string("fixtures/sql/case.sql").expect("case.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 10,
            "expected >10 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_delete_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/delete.sql")
            .expect("delete.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 5,
            "expected >5 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_with_sql_fixture() {
        let sql =
            std::fs::read_to_string("fixtures/sql/with.sql").expect("with.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 10,
            "expected >10 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_select_having_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/select_having.sql")
            .expect("select_having.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_select_implicit_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/select_implicit.sql")
            .expect("select_implicit.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_select_distinct_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/select_distinct.sql")
            .expect("select_distinct.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_select_into_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/select_into.sql")
            .expect("select_into.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_prepared_xacts_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/prepared_xacts.sql")
            .expect("prepared_xacts.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_namespace_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/namespace.sql")
            .expect("namespace.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_btree_index_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/btree_index.sql")
            .expect("btree_index.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_hash_index_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/hash_index.sql")
            .expect("hash_index.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_update_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/update.sql")
            .expect("update.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_transactions_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/transactions.sql")
            .expect("transactions.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_aggregates_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/aggregates.sql")
            .expect("aggregates.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_arrays_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/arrays.sql")
            .expect("arrays.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_join_sql_fixture() {
        let sql =
            std::fs::read_to_string("fixtures/sql/join.sql").expect("join.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_limit_sql_fixture() {
        let sql =
            std::fs::read_to_string("fixtures/sql/limit.sql").expect("limit.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_returning_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/returning.sql")
            .expect("returning.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_truncate_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/truncate.sql")
            .expect("truncate.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_alter_table_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/alter_table.sql")
            .expect("alter_table.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_create_table_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/create_table.sql")
            .expect("create_table.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_insert_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/insert.sql")
            .expect("insert.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_typed_table_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/typed_table.sql")
            .expect("typed_table.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_vacuum_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/vacuum.sql")
            .expect("vacuum.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }

    #[test]
    fn parse_drop_if_exists_sql_fixture() {
        let sql = std::fs::read_to_string("fixtures/sql/drop_if_exists.sql")
            .expect("drop_if_exists.sql fixture not found");
        let mut input = Input::new(&sql);
        let commands = parse_sql_file(&mut input).unwrap();
        assert!(
            commands.len() > 0,
            "expected >0 commands, got {}",
            commands.len()
        );
        assert!(
            input.is_empty(),
            "leftover at {}: {:?}",
            input.cursor(),
            &input.remaining()[..input.remaining().len().min(100)]
        );
    }
}
