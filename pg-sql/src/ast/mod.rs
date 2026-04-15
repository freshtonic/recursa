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
    create_function::{CreateFunctionStmt, DropFunctionStmt, DropRoutineStmt},
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
pub enum Statement<'input> {
    // --- Multi-keyword statements (longest first_pattern first) ---
    With(Box<WithStatement<'input>>),
    Explain(Box<ExplainStmt<'input>>),
    // CREATE variants: multi-keyword before single-keyword
    CreateFunction(Box<CreateFunctionStmt<'input>>),
    CreateProcedure(Box<CreateProcedureStmt<'input>>),
    CreateTablespace(Box<CreateTablespaceStmt<'input>>),
    ImportForeignSchema(ImportForeignSchemaStmt<'input>),
    CreateConstraintTrigger(CreateConstraintTriggerStmt<'input>),
    CreateTrigger(CreateTriggerStmt<'input>),
    CreateEventTrigger(CreateEventTriggerStmt<'input>),
    CreateAccessMethod(CreateAccessMethodStmt<'input>),
    CreateMaterializedView(CreateMaterializedViewStmt<'input>),
    CreateTextSearch(CreateTextSearchStmt<'input>),
    CreateForeign(CreateForeignStmt<'input>),
    CreateIndex(Box<CreateIndexStmt<'input>>),
    CreateView(Box<CreateViewStmt<'input>>),
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
    CreateTable(Box<CreateTableStmt<'input>>),
    // DROP variants
    DropFunction(Box<DropFunctionStmt<'input>>),
    DropProcedure(Box<DropProcedureStmt<'input>>),
    DropRoutine(Box<DropRoutineStmt<'input>>),
    DropTablespace(Box<DropTablespaceStmt<'input>>),
    DropTrigger(DropTriggerStmt<'input>),
    DropEventTrigger(DropEventTriggerStmt<'input>),
    DropAccessMethod(DropAccessMethodStmt<'input>),
    DropMaterializedView(DropMaterializedViewStmt<'input>),
    DropTextSearch(DropTextSearchStmt<'input>),
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
    DropTable(Box<DropTableStmt<'input>>),
    // ALTER variants: multi-keyword before single-keyword
    AlterDefaultPrivileges(AlterDefaultPrivilegesStmt<'input>),
    AlterForeign(AlterForeignStmt<'input>),
    AlterEventTrigger(AlterEventTriggerStmt<'input>),
    AlterMaterializedView(AlterMaterializedViewStmt<'input>),
    AlterTextSearch(AlterTextSearchStmt<'input>),
    AlterTablespace(AlterTablespaceStmt<'input>),
    AlterTable(AlterTableStmt<'input>),
    AlterRule(AlterRuleStmt<'input>),
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
    Insert(Box<InsertStmt<'input>>),
    Update(Box<UpdateStmt<'input>>),
    Merge(Box<MergeStmt<'input>>),
    Delete(Box<DeleteStmt<'input>>),
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
    Reindex(Box<ReindexStmt<'input>>),
    Refresh(RefreshStmt<'input>),
    Cluster(ClusterStmt<'input>),
    Checkpoint(CheckpointStmt),
    Vacuum(Box<VacuumStmt<'input>>),
    Lock(LockStmt<'input>),
    Notify(NotifyStmt<'input>),
    Listen(ListenStmt<'input>),
    Unlisten(UnlistenStmt<'input>),
    Discard(DiscardStmt<'input>),
    Reassign(ReassignStmt<'input>),
    Do(Box<DoStmt<'input>>),
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
    Values(Box<CompoundQuery<'input>>),
    Select(Box<SelectStmt<'input>>),
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
        // Accept the start of a statement-like fragment: an SQL identifier
        // (alpha or `_`) or a quoted ident (`"foo"`). The double-quote case
        // matters for `Option<RawStatement>` tails on partially-implemented
        // stmts like `ALTER COLLATION "en_US" REFRESH VERSION` whose tail
        // starts with a quoted ident. Other leading characters (digits,
        // backslash, etc.) are intentionally rejected so COPY-from-stdin
        // data blocks remain as `RawLines`, not raw statements.
        !input.is_empty()
            && input
                .remaining()
                .starts_with(|c: char| {
                    c.is_ascii_alphabetic()
                        || c == '_'
                        || c == '"'
                        || c == ':'
                        || c == '('
                })
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

    /// Regression guard: keep the top-level statement enums small enough that the
    /// recursive descent parser fits in the default test thread stack.
    /// Prior to boxing the largest variants, `Statement` was 1480 bytes and
    /// fixture-parsing tests required `RUST_MIN_STACK=16777216`.
    #[test]
    fn statement_size_is_bounded() {
        use std::mem::size_of;
        let stmt = size_of::<Statement<'_>>();
        let item = size_of::<FileItem<'_>>();
        assert!(
            stmt <= 128,
            "Statement grew to {stmt} bytes — Box the largest variants",
        );
        assert!(
            item <= 128,
            "FileItem grew to {item} bytes — Box the largest variants",
        );
    }

    /// Print sizes of major AST node types. Run with `--nocapture` to see output.
    /// `#[ignore]` so it doesn't run by default but stays available for diagnosis.
    #[test]
    #[ignore]
    fn report_ast_sizes() {
        use std::mem::size_of;
        let mut sizes: Vec<(&'static str, usize)> = vec![
            ("FileItem", size_of::<FileItem<'_>>()),
            ("PsqlCommand", size_of::<PsqlCommand<'_>>()),
            ("TerminatedStatement", size_of::<TerminatedStatement<'_>>()),
            ("Statement", size_of::<Statement<'_>>()),
            ("Expr", size_of::<crate::ast::expr::Expr<'_>>()),
            ("CaseSearched", size_of::<crate::ast::expr::CaseSearched<'_>>()),
            ("CaseSimple", size_of::<crate::ast::expr::CaseSimple<'_>>()),
            ("IntervalLit", size_of::<crate::ast::expr::IntervalLit<'_>>()),
            ("TimestampLit", size_of::<crate::ast::expr::TimestampLit<'_>>()),
            ("TypeCastFunc", size_of::<crate::ast::expr::TypeCastFunc<'_>>()),
            ("XmlElement", size_of::<crate::ast::expr::XmlElement<'_>>()),
            ("XmlForest", size_of::<crate::ast::expr::XmlForest<'_>>()),
            ("XmlAttributes", size_of::<crate::ast::expr::XmlAttributes<'_>>()),
            ("XmlPi", size_of::<crate::ast::expr::XmlPi<'_>>()),
            ("ArrayExpr", size_of::<crate::ast::expr::ArrayExpr<'_>>()),
            ("QualifiedRef", size_of::<crate::ast::expr::QualifiedRef<'_>>()),
            ("QualifiedWildcard", size_of::<crate::ast::expr::QualifiedWildcard<'_>>()),
            ("ParenExpr", size_of::<crate::ast::expr::ParenExpr<'_>>()),
            ("ExistsExpr", size_of::<crate::ast::expr::ExistsExpr<'_>>()),
            ("ArrayBracket", size_of::<crate::ast::expr::ArrayBracket<'_>>()),
            ("RowExpr", size_of::<crate::ast::expr::RowExpr<'_>>()),
            ("CastType", size_of::<crate::ast::expr::CastType<'_>>()),
            ("ExtractCall", size_of::<crate::ast::expr::ExtractCall<'_>>()),
            ("NotInSuffix", size_of::<crate::ast::expr::NotInSuffix<'_>>()),
            ("InContent", size_of::<crate::ast::expr::InContent<'_>>()),
            ("InList", size_of::<crate::ast::expr::InList<'_>>()),
            ("SubstringCall", size_of::<crate::ast::expr::SubstringCall<'_>>()),
            ("OverlayCall", size_of::<crate::ast::expr::OverlayCall<'_>>()),
            ("TrimCall", size_of::<crate::ast::expr::TrimCall<'_>>()),
            ("PositionCall", size_of::<crate::ast::expr::PositionCall<'_>>()),
            ("SelectStmt", size_of::<crate::ast::select::SelectStmt<'_>>()),
            ("CreateTableStmt", size_of::<crate::ast::create_table::CreateTableStmt<'_>>()),
            ("CreateFunctionStmt", size_of::<crate::ast::create_function::CreateFunctionStmt<'_>>()),
            ("InsertStmt", size_of::<crate::ast::insert::InsertStmt<'_>>()),
            ("UpdateStmt", size_of::<crate::ast::update::UpdateStmt<'_>>()),
            ("DeleteStmt", size_of::<crate::ast::delete::DeleteStmt<'_>>()),
            ("MergeStmt", size_of::<crate::ast::merge::MergeStmt<'_>>()),
            ("ExplainStmt", size_of::<crate::ast::explain::ExplainStmt<'_>>()),
            ("CompoundQuery", size_of::<crate::ast::values::CompoundQuery<'_>>()),
            ("WithStatement", size_of::<crate::ast::with_clause::WithStatement<'_>>()),
            ("FuncCall", size_of::<crate::ast::expr::FuncCall<'_>>()),
            ("ColumnDef", size_of::<crate::ast::create_table::ColumnDef<'_>>()),
            ("ConflictAction", size_of::<crate::ast::insert::ConflictAction<'_>>()),
            ("DoUpdateAction", size_of::<crate::ast::insert::DoUpdateAction<'_>>()),
            ("GroupByItem", size_of::<crate::ast::select::GroupByItem<'_>>()),
            ("FuncArg", size_of::<crate::ast::expr::FuncArg<'_>>()),
            ("AlterTableStmt", size_of::<simple_stmts::AlterTableStmt<'_>>()),
            ("CreateTriggerStmt", size_of::<simple_stmts::CreateTriggerStmt<'_>>()),
            ("CreateRuleStmt", size_of::<simple_stmts::CreateRuleStmt<'_>>()),
            ("CreateForeignStmt", size_of::<simple_stmts::CreateForeignStmt<'_>>()),
            ("CreateMaterializedViewStmt", size_of::<simple_stmts::CreateMaterializedViewStmt<'_>>()),
            ("AlterMaterializedViewStmt", size_of::<simple_stmts::AlterMaterializedViewStmt<'_>>()),
            ("CopyStmt", size_of::<simple_stmts::CopyStmt<'_>>()),
            ("VacuumStmt", size_of::<simple_stmts::VacuumStmt<'_>>()),
            ("ReindexStmt", size_of::<simple_stmts::ReindexStmt<'_>>()),
            ("ClusterStmt", size_of::<simple_stmts::ClusterStmt<'_>>()),
            ("GrantStmt", size_of::<simple_stmts::GrantStmt<'_>>()),
            ("RevokeStmt", size_of::<simple_stmts::RevokeStmt<'_>>()),
            ("DoStmt", size_of::<simple_stmts::DoStmt<'_>>()),
            ("CreateRoleStmt", size_of::<simple_stmts::CreateRoleStmt<'_>>()),
            ("CreateAggregateStmt", size_of::<simple_stmts::CreateAggregateStmt<'_>>()),
            ("CreateOperatorStmt", size_of::<simple_stmts::CreateOperatorStmt<'_>>()),
            ("AnalyzeStmt", size_of::<crate::ast::analyze::AnalyzeStmt<'_>>()),
            ("CreateIndexStmt", size_of::<crate::ast::create_index::CreateIndexStmt<'_>>()),
            ("CreateViewStmt", size_of::<crate::ast::create_view::CreateViewStmt<'_>>()),
            ("DropTableStmt", size_of::<crate::ast::drop_table::DropTableStmt<'_>>()),
            ("RawStatement", size_of::<RawStatement<'_>>()),
            ("CreateProcedureStmt", size_of::<crate::ast::create_procedure::CreateProcedureStmt<'_>>()),
            ("CreateTablespaceStmt", size_of::<crate::ast::create_tablespace::CreateTablespaceStmt<'_>>()),
            ("DropFunctionStmt", size_of::<crate::ast::create_function::DropFunctionStmt<'_>>()),
            ("CreateEventTriggerStmt", size_of::<simple_stmts::CreateEventTriggerStmt<'_>>()),
            ("CreateAccessMethodStmt", size_of::<simple_stmts::CreateAccessMethodStmt<'_>>()),
            ("CreateLanguageStmt", size_of::<simple_stmts::CreateLanguageStmt<'_>>()),
            ("CreateDatabaseStmt", size_of::<simple_stmts::CreateDatabaseStmt<'_>>()),
            ("CreateUserStmt", size_of::<simple_stmts::CreateUserStmt<'_>>()),
            ("CreateSchemaStmt", size_of::<simple_stmts::CreateSchemaStmt<'_>>()),
            ("CreateSequenceStmt", size_of::<simple_stmts::CreateSequenceStmt<'_>>()),
            ("CreateTypeStmt", size_of::<simple_stmts::CreateTypeStmt<'_>>()),
            ("CreateDomainStmt", size_of::<simple_stmts::CreateDomainStmt<'_>>()),
            ("CreateCastStmt", size_of::<simple_stmts::CreateCastStmt<'_>>()),
            ("CreateCollationStmt", size_of::<simple_stmts::CreateCollationStmt<'_>>()),
            ("CreateExtensionStmt", size_of::<simple_stmts::CreateExtensionStmt<'_>>()),
            ("CreatePolicyStmt", size_of::<simple_stmts::CreatePolicyStmt<'_>>()),
            ("CreateStatisticsStmt", size_of::<simple_stmts::CreateStatisticsStmt<'_>>()),
            ("CreatePublicationStmt", size_of::<simple_stmts::CreatePublicationStmt<'_>>()),
            ("CreateSubscriptionStmt", size_of::<simple_stmts::CreateSubscriptionStmt<'_>>()),
            ("CreateConversionStmt", size_of::<simple_stmts::CreateConversionStmt<'_>>()),
            ("CreateServerStmt", size_of::<simple_stmts::CreateServerStmt<'_>>()),
            ("CreateGroupStmt", size_of::<simple_stmts::CreateGroupStmt<'_>>()),
            ("AlterIndexStmt", size_of::<simple_stmts::AlterIndexStmt<'_>>()),
            ("AlterViewStmt", size_of::<simple_stmts::AlterViewStmt<'_>>()),
            ("AlterFunctionStmt", size_of::<simple_stmts::AlterFunctionStmt<'_>>()),
            ("CommentStmt", size_of::<simple_stmts::CommentStmt<'_>>()),
            ("SecurityLabelStmt", size_of::<simple_stmts::SecurityLabelStmt<'_>>()),
            ("PrepareStmt", size_of::<simple_stmts::PrepareStmt<'_>>()),
            ("TableRef", size_of::<crate::ast::select::TableRef<'_>>()),
            ("SimpleTableRef", size_of::<crate::ast::select::SimpleTableRef<'_>>()),
            ("CompoundQuery (if any)", size_of::<crate::ast::values::CompoundQuery<'_>>()),
        ];
        sizes.sort_by(|a, b| b.1.cmp(&a.1));
        eprintln!("\n=== AST sizes (bytes) ===");
        for (name, size) in &sizes {
            eprintln!("{size:>6}  {name}");
        }
        eprintln!();
    }

    /// Convert a byte offset into `(line, col)` (both 1-based).
    fn line_col(src: &str, byte_offset: usize) -> (usize, usize) {
        let cap = byte_offset.min(src.len());
        let prefix = &src[..cap];
        let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
        let last_nl = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = src[last_nl..cap].chars().count() + 1;
        (line, col)
    }

    /// Parse a SQL fixture file, panicking with `path:line:col: …` context on error.
    ///
    /// Parses the whole file; on any parse error or leftover input, computes the
    /// human-readable line/column of the offending byte and includes it in the
    /// panic message alongside a short snippet.
    fn parse_fixture(name: &str) -> Vec<FileItem<'static>> {
        let path = format!("fixtures/sql/{name}");
        let sql = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{path}: cannot read fixture: {e}"));
        // Leak so the returned Vec borrows 'static (test-only convenience).
        let sql: &'static str = Box::leak(sql.into_boxed_str());
        let mut input = Input::new(sql);
        let items = match parse_sql_file(&mut input) {
            Ok(items) => items,
            Err(e) => {
                let span = e.span();
                let (line, col) = line_col(sql, span.start);
                let snippet_end = (span.start + 80).min(sql.len());
                let snippet = &sql[span.start..snippet_end];
                panic!(
                    "{path}:{line}:{col}: parse error: {e}\n  near: {}",
                    snippet.replace('\n', "\\n")
                );
            }
        };
        if !input.is_empty() {
            let cursor = input.cursor();
            let (line, col) = line_col(sql, cursor);
            let snippet_end = (cursor + 80).min(sql.len());
            let snippet = &sql[cursor..snippet_end];
            panic!(
                "{path}:{line}:{col}: leftover input after parse:\n  near: {}",
                snippet.replace('\n', "\\n")
            );
        }
        items
    }

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
        let items = parse_fixture("boolean.sql");
        assert!(items.len() > 50, "expected >50 commands, got {}", items.len());
    }

    #[test]
    fn parse_comments_sql_fixture() {
        let items = parse_fixture("comments.sql");
        assert!(items.len() > 3, "expected >3 commands, got {}", items.len());
    }

    #[test]
    fn parse_select_sql_fixture() {
        let items = parse_fixture("select.sql");
        assert!(items.len() > 10, "expected >10 commands, got {}", items.len());
    }

    #[test]
    fn parse_union_sql_fixture() {
        let items = parse_fixture("union.sql");
        assert!(items.len() > 10, "expected >10 commands, got {}", items.len());
    }

    #[test]
    fn parse_subselect_sql_fixture() {
        let items = parse_fixture("subselect.sql");
        assert!(items.len() > 10, "expected >10 commands, got {}", items.len());
    }

    #[test]
    fn parse_case_sql_fixture() {
        let items = parse_fixture("case.sql");
        assert!(items.len() > 10, "expected >10 commands, got {}", items.len());
    }

    #[test]
    fn parse_delete_sql_fixture() {
        let items = parse_fixture("delete.sql");
        assert!(items.len() > 5, "expected >5 commands, got {}", items.len());
    }

    #[test]
    fn parse_with_sql_fixture() {
        let items = parse_fixture("with.sql");
        assert!(items.len() > 10, "expected >10 commands, got {}", items.len());
    }

    #[test]
    fn parse_select_having_sql_fixture() {
        let items = parse_fixture("select_having.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_select_implicit_sql_fixture() {
        let items = parse_fixture("select_implicit.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_select_distinct_sql_fixture() {
        let items = parse_fixture("select_distinct.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_select_into_sql_fixture() {
        let items = parse_fixture("select_into.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_prepared_xacts_sql_fixture() {
        let items = parse_fixture("prepared_xacts.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_namespace_sql_fixture() {
        let items = parse_fixture("namespace.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_btree_index_sql_fixture() {
        let items = parse_fixture("btree_index.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_hash_index_sql_fixture() {
        let items = parse_fixture("hash_index.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_update_sql_fixture() {
        let items = parse_fixture("update.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_transactions_sql_fixture() {
        let items = parse_fixture("transactions.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_aggregates_sql_fixture() {
        let items = parse_fixture("aggregates.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_arrays_sql_fixture() {
        let items = parse_fixture("arrays.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_join_sql_fixture() {
        let items = parse_fixture("join.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_limit_sql_fixture() {
        let items = parse_fixture("limit.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_returning_sql_fixture() {
        let items = parse_fixture("returning.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_truncate_sql_fixture() {
        let items = parse_fixture("truncate.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_alter_table_sql_fixture() {
        let items = parse_fixture("alter_table.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_create_table_sql_fixture() {
        let items = parse_fixture("create_table.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_insert_sql_fixture() {
        let items = parse_fixture("insert.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_typed_table_sql_fixture() {
        let items = parse_fixture("typed_table.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_vacuum_sql_fixture() {
        let items = parse_fixture("vacuum.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }

    #[test]
    fn parse_drop_if_exists_sql_fixture() {
        let items = parse_fixture("drop_if_exists.sql");
        assert!(items.len() > 0, "expected >0 commands, got {}", items.len());
    }
}
