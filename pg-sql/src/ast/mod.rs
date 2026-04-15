pub mod analyze;
pub mod common;
pub mod create_function;
pub mod create_index;
pub mod create_procedure;
pub mod create_table;
pub mod create_tablespace;
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
use recursa_diagram::railroad;

use crate::rules::SqlRules;
use crate::tokens::{literal, punct};

use self::{
    analyze::AnalyzeStmt,
    create_function::{CreateFunctionStmt, DropFunctionStmt},
    create_index::{CreateIndexStmt, DropIndexStmt},
    create_procedure::{CallStmt, CreateProcedureStmt, DropProcedureStmt},
    create_table::CreateTableStmt,
    create_tablespace::{AlterTablespaceStmt, CreateTablespaceStmt, DropTablespaceStmt},
    create_view::{CreateViewStmt, DropViewStmt},
    delete::DeleteStmt,
    drop_table::DropTableStmt,
    explain::ExplainStmt,
    insert::InsertStmt,
    merge::MergeStmt,
    select::SelectStmt,
    set_reset::{ResetStmt, SetRoleStmt, SetSessionAuthStmt, SetStmt, SetTimeZoneStmt, ShowStmt},
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
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[allow(clippy::large_enum_variant)]
pub enum Statement<'input> {
    // --- Multi-keyword statements (longest first_pattern first) ---
    With(WithStatement<'input>),
    Explain(ExplainStmt<'input>),
    // CREATE variants: multi-keyword before single-keyword
    CreateFunction(CreateFunctionStmt<'input>),
    CreateProcedure(CreateProcedureStmt<'input>),
    CreateTablespace(CreateTablespaceStmt<'input>),
    CreateTrigger(CreateTriggerStmt<'input>),
    CreateEventTrigger(CreateEventTriggerStmt<'input>),
    CreateAccessMethod(CreateAccessMethodStmt<'input>),
    CreateMaterializedView(CreateMaterializedViewStmt<'input>),
    CreateForeign(CreateForeignStmt<'input>),
    CreateIndex(CreateIndexStmt<'input>),
    CreateView(CreateViewStmt<'input>),
    CreateRule(CreateRuleStmt<'input>),
    CreateGroup(CreateGroupStmt<'input>),
    CreateRole(CreateRoleStmt<'input>),
    CreateUser(CreateUserStmt<'input>),
    CreateSchema(CreateSchemaStmt<'input>),
    CreateSequence(CreateSequenceStmt<'input>),
    CreateType(CreateTypeStmt<'input>),
    CreateDomain(CreateDomainStmt<'input>),
    CreateAggregate(CreateAggregateStmt<'input>),
    CreateOperator(CreateOperatorStmt<'input>),
    CreateCast(CreateCastStmt<'input>),
    CreateCollation(CreateCollationStmt<'input>),
    CreateExtension(CreateExtensionStmt<'input>),
    CreatePolicy(CreatePolicyStmt<'input>),
    CreateStatistics(CreateStatisticsStmt<'input>),
    CreatePublication(CreatePublicationStmt<'input>),
    CreateSubscription(CreateSubscriptionStmt<'input>),
    CreateConversion(CreateConversionStmt<'input>),
    CreateServer(CreateServerStmt<'input>),
    CreateLanguage(CreateLanguageStmt<'input>),
    CreateDatabase(CreateDatabaseStmt<'input>),
    CreateTable(CreateTableStmt<'input>),
    // DROP variants
    DropFunction(DropFunctionStmt<'input>),
    DropProcedure(DropProcedureStmt<'input>),
    DropTablespace(DropTablespaceStmt<'input>),
    DropTrigger(DropTriggerStmt<'input>),
    DropEventTrigger(DropEventTriggerStmt<'input>),
    DropAccessMethod(DropAccessMethodStmt<'input>),
    DropMaterializedView(DropMaterializedViewStmt<'input>),
    DropForeign(DropForeignStmt<'input>),
    DropOwned(DropOwnedStmt<'input>),
    DropIndex(DropIndexStmt<'input>),
    DropView(DropViewStmt<'input>),
    DropRule(DropRuleStmt<'input>),
    DropGroup(DropGroupStmt<'input>),
    DropRole(DropRoleStmt<'input>),
    DropUser(DropUserStmt<'input>),
    DropSchema(DropSchemaStmt<'input>),
    DropSequence(DropSequenceStmt<'input>),
    DropType(DropTypeStmt<'input>),
    DropDomain(DropDomainStmt<'input>),
    DropAggregate(DropAggregateStmt<'input>),
    DropOperator(DropOperatorStmt<'input>),
    DropCast(DropCastStmt<'input>),
    DropCollation(DropCollationStmt<'input>),
    DropExtension(DropExtensionStmt<'input>),
    DropPolicy(DropPolicyStmt<'input>),
    DropStatistics(DropStatisticsStmt<'input>),
    DropPublication(DropPublicationStmt<'input>),
    DropSubscription(DropSubscriptionStmt<'input>),
    DropConversion(DropConversionStmt<'input>),
    DropServer(DropServerStmt<'input>),
    DropLanguage(DropLanguageStmt<'input>),
    DropDatabase(DropDatabaseStmt<'input>),
    DropTable(DropTableStmt<'input>),
    // ALTER variants: multi-keyword before single-keyword
    AlterDefaultPrivileges(AlterDefaultPrivilegesStmt<'input>),
    AlterForeign(AlterForeignStmt<'input>),
    AlterEventTrigger(AlterEventTriggerStmt<'input>),
    AlterMaterializedView(AlterMaterializedViewStmt<'input>),
    AlterTablespace(AlterTablespaceStmt<'input>),
    AlterTable(AlterTableStmt<'input>),
    AlterGroup(AlterGroupStmt<'input>),
    AlterRole(AlterRoleStmt<'input>),
    AlterUser(AlterUserStmt<'input>),
    AlterSchema(AlterSchemaStmt<'input>),
    AlterSequence(AlterSequenceStmt<'input>),
    AlterType(AlterTypeStmt<'input>),
    AlterDomain(AlterDomainStmt<'input>),
    AlterAggregate(AlterAggregateStmt<'input>),
    AlterOperator(AlterOperatorStmt<'input>),
    AlterCollation(AlterCollationStmt<'input>),
    AlterExtension(AlterExtensionStmt<'input>),
    AlterPolicy(AlterPolicyStmt<'input>),
    AlterStatistics(AlterStatisticsStmt<'input>),
    AlterPublication(AlterPublicationStmt<'input>),
    AlterSubscription(AlterSubscriptionStmt<'input>),
    AlterConversion(AlterConversionStmt<'input>),
    AlterServer(AlterServerStmt<'input>),
    AlterLanguage(AlterLanguageStmt<'input>),
    AlterDatabase(AlterDatabaseStmt<'input>),
    AlterIndex(AlterIndexStmt<'input>),
    AlterView(AlterViewStmt<'input>),
    AlterFunction(AlterFunctionStmt<'input>),
    // CALL stored procedure
    Call(CallStmt<'input>),
    // DML
    Insert(InsertStmt<'input>),
    Update(UpdateStmt<'input>),
    Merge(MergeStmt<'input>),
    Delete(DeleteStmt<'input>),
    // Transaction control
    Rollback(RollbackStmt<'input>),
    Savepoint(SavepointStmt<'input>),
    Release(ReleaseStmt<'input>),
    StartTransaction(StartTransactionStmt),
    Begin(BeginStmt),
    Commit(CommitStmt<'input>),
    End(EndStmt),
    Abort(AbortStmt),
    // PREPARE / EXECUTE / DEALLOCATE
    Deallocate(DeallocateStmt<'input>),
    Prepare(PrepareStmt<'input>),
    Execute(ExecuteStmt<'input>),
    // Permissions
    Grant(GrantStmt<'input>),
    Revoke(RevokeStmt<'input>),
    // Utility
    SecurityLabel(SecurityLabelStmt<'input>),
    Comment(CommentStmt<'input>),
    Copy(CopyStmt<'input>),
    Truncate(TruncateStmt<'input>),
    Reindex(ReindexStmt<'input>),
    Refresh(RefreshStmt<'input>),
    Cluster(ClusterStmt<'input>),
    Checkpoint(CheckpointStmt),
    Vacuum(VacuumStmt<'input>),
    Lock(LockStmt<'input>),
    Notify(NotifyStmt<'input>),
    Listen(ListenStmt<'input>),
    Unlisten(UnlistenStmt<'input>),
    Discard(DiscardStmt<'input>),
    Reassign(ReassignStmt<'input>),
    Do(DoStmt<'input>),
    // Cursor
    Declare(DeclareStmt<'input>),
    Fetch(FetchStmt<'input>),
    Close(CloseStmt<'input>),
    Move(MoveStmt<'input>),
    // Configuration
    // Multi-keyword SET variants must come before plain Set so
    // longest-match-wins picks the more specific form.
    SetConstraints(SetConstraintsStmt<'input>),
    SetTransaction(SetTransactionStmt),
    SetSessionAuth(SetSessionAuthStmt<'input>),
    SetTimeZone(SetTimeZoneStmt<'input>),
    SetRole(SetRoleStmt<'input>),
    Set(SetStmt<'input>),
    Reset(ResetStmt<'input>),
    Show(ShowStmt<'input>),
    Analyze(AnalyzeStmt<'input>),
    // Query
    Values(CompoundQuery<'input>),
    Select(SelectStmt<'input>),
    Table(TableStmt<'input>),
    /// Catch-all: consumes tokens until the next semicolon.
    Raw(RawStatement<'input>),
}

/// A raw statement: consumes everything up to (but not including) the next semicolon.
///
/// Manual Parse impl needed because this is a catch-all that doesn't use structured
/// token parsing. It's intentionally the last variant in Statement.
/// To eliminate this, implement proper AST types for each statement kind.
#[derive(Debug, Clone, Visit)]
pub struct RawStatement<'input> {
    pub text: ::std::borrow::Cow<'input, str>,
}

impl<'input> FormatTokens for RawStatement<'input> {
    fn format_tokens(&self, tokens: &mut Vec<recursa::fmt::Token>) {
        tokens.push(recursa::fmt::Token::String(self.text.as_ref().to_string()));
    }
}

impl<'input> Parse<'input> for RawStatement<'input> {
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        !input.is_empty()
            && input
                .remaining()
                .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
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

        let text = ::std::borrow::Cow::Borrowed(&remaining[..byte_pos]);
        input.advance(byte_pos);
        Ok(RawStatement { text })
    }
}

/// A psql meta-command that terminates a SQL statement in place of `;`.
///
/// Psql accepts `\gset`, `\gexec`, `\g`, `\gx`, and `\crosstabview` as
/// statement terminators: e.g. `SELECT oid FROM pg_database \gset` sends the
/// query and binds the results to psql variables, ending the statement just
/// like `;` would.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[parse(rules = SqlRules)]
pub enum PsqlTerminator {
    /// `\crosstabview` — listed first as the longest-prefix variant.
    Crosstabview(punct::PsqlCrosstabview),
    /// `\gexec`
    Gexec(punct::PsqlGexec),
    /// `\gset`
    Gset(punct::PsqlGset),
    /// `\gx`
    Gx(punct::PsqlGx),
    /// `\g`
    G(punct::PsqlG),
}

/// The terminator of a SQL statement: a semicolon or a psql meta-command.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[parse(rules = SqlRules)]
pub enum StatementTerminator {
    /// A psql meta-command like `\gset`.
    Psql(PsqlTerminator),
    /// A plain semicolon.
    Semi(punct::Semi),
}

/// A SQL statement followed by a terminator (`;` or a psql meta-command).
#[railroad]
#[derive(Debug, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TerminatedStatement<'input> {
    pub stmt: Statement<'input>,
    pub terminator: StatementTerminator,
}

/// A psql directive: backslash followed by the rest of the line.
#[railroad]
#[derive(Debug, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PsqlDirective<'input> {
    pub backslash: punct::BackSlash,
    pub rest: literal::RestOfLine<'input>,
}

/// A command in a psql input file: either a SQL statement or a psql directive.
#[railroad]
#[derive(Debug, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
#[allow(clippy::large_enum_variant)]
pub enum PsqlCommand<'input> {
    /// A psql directive (e.g., `\pset null '(null)'`).
    /// Listed first so `\` is checked before statement keywords.
    Directive(PsqlDirective<'input>),
    /// A SQL statement followed by a semicolon.
    Statement(TerminatedStatement<'input>),
}

/// Parse a complete SQL file into a list of commands.
///
/// Gracefully handles parse errors by falling back to RawStatement parsing,
/// which consumes everything up to the next semicolon. This allows the parser
/// to continue past intentionally invalid SQL (e.g., error test cases in
/// PostgreSQL regression test files).
/// An item in a parsed SQL file: either a parsed command or raw text
/// (e.g., COPY FROM stdin data blocks that can't be parsed as SQL).
pub enum FileItem<'input> {
    Command(PsqlCommand<'input>),
    RawLines(::std::borrow::Cow<'input, str>),
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
pub fn parse_stats(items: &[FileItem<'_>]) -> ParseStats {
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
pub fn parse_sql_file<'input>(
    input: &mut Input<'input>,
) -> Result<Vec<FileItem<'input>>, ParseError> {
    let mut items = Vec::new();
    // Owned accumulator: raw lines are not contiguous in the source (intervening
    // consume_ignored calls may skip whitespace/comments between them), so we
    // can't borrow a slice. Stored as Cow::Owned on flush.
    let mut raw_buf = String::new();
    loop {
        SqlRules::consume_ignored(input);
        if input.is_empty() {
            break;
        }
        if !PsqlCommand::peek::<SqlRules>(input) {
            // Collect unparseable lines (e.g., COPY FROM stdin data blocks).
            let line = take_line(input);
            raw_buf.push_str(line);
            raw_buf.push('\n');
            continue;
        }
        // Flush any accumulated raw lines before the next command
        if !raw_buf.is_empty() {
            items.push(FileItem::RawLines(::std::borrow::Cow::Owned(
                std::mem::take(&mut raw_buf),
            )));
        }
        match PsqlCommand::parse::<SqlRules>(input) {
            Ok(cmd) => items.push(FileItem::Command(cmd)),
            Err(_) => {
                // Parse error -- skip to next semicolon and create a Raw statement
                let raw = RawStatement::parse::<SqlRules>(input)?;
                SqlRules::consume_ignored(input);
                if punct::Semi::peek::<SqlRules>(input) {
                    let semi = punct::Semi::parse::<SqlRules>(input)?;
                    items.push(FileItem::Command(PsqlCommand::Statement(
                        TerminatedStatement {
                            stmt: Statement::Raw(raw),
                            terminator: StatementTerminator::Semi(semi),
                        },
                    )));
                }
            }
        }
    }
    // Flush trailing raw lines
    if !raw_buf.is_empty() {
        items.push(FileItem::RawLines(::std::borrow::Cow::Owned(raw_buf)));
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
        let stmt = Statement::parse::<SqlRules>(&mut input).unwrap();
        // Bare SELECT now matches via CompoundQuery path since Values variant
        // precedes Select for compound query (UNION etc.) support.
        assert!(matches!(stmt, Statement::Values(_)));
    }

    #[test]
    fn parse_statement_create_table() {
        let mut input = Input::new("CREATE TABLE t (f1 bool)");
        let stmt = Statement::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(stmt, Statement::CreateTable(_)));
    }

    #[test]
    fn parse_statement_insert() {
        let mut input = Input::new("INSERT INTO t (f1) VALUES (true)");
        let stmt = Statement::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(stmt, Statement::Insert(_)));
    }

    #[test]
    fn parse_statement_delete() {
        let mut input = Input::new("DELETE FROM t WHERE a > 1");
        let stmt = Statement::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(stmt, Statement::Delete(_)));
    }

    #[test]
    fn parse_statement_drop_table() {
        let mut input = Input::new("DROP TABLE t");
        let stmt = Statement::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(stmt, Statement::DropTable(_)));
    }

    #[test]
    fn parse_psql_command_statement() {
        let mut input = Input::new("SELECT 1;");
        let cmd = PsqlCommand::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_psql_command_directive() {
        let mut input = Input::new("\\pset null '(null)'\n");
        let cmd = PsqlCommand::parse::<SqlRules>(&mut input).unwrap();
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
        let cmd = PsqlCommand::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(cmd, PsqlCommand::Statement(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_full_insert_with_type_cast() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't');");
        let cmd = PsqlCommand::parse::<SqlRules>(&mut input).unwrap();
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
