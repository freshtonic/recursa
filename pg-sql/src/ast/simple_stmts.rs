/// Simple keyword-led statements that are recognized by their leading keyword(s)
/// but whose body is captured as raw text (not fully parsed).
///
/// Each struct has keyword PhantomData fields for disambiguation in the Statement
/// enum, followed by an optional RawStatement tail that captures any remaining
/// content before the semicolon.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::RawStatement;
use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};
use recursa_diagram::railroad;

// --- Transaction control ---

/// Isolation level following `ISOLATION LEVEL`.
///
/// Variant ordering: multi-word forms before single-word `Serializable`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum IsolationLevelKind {
    RepeatableRead(RepeatableReadLevel),
    ReadCommitted(ReadCommittedLevel),
    ReadUncommitted(ReadUncommittedLevel),
    Serializable(keyword::Serializable),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RepeatableReadLevel {
    pub _repeatable: PhantomData<keyword::Repeatable>,
    pub _read: PhantomData<keyword::ReadKw>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadCommittedLevel {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _committed: PhantomData<keyword::Committed>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadUncommittedLevel {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _uncommitted: PhantomData<keyword::Uncommitted>,
}

/// `ISOLATION LEVEL <level>` transaction mode.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsolationLevelMode {
    pub _isolation: PhantomData<keyword::Isolation>,
    pub _level: PhantomData<keyword::Level>,
    pub level: IsolationLevelKind,
}

/// `READ ONLY` or `READ WRITE` transaction mode.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadOnlyMode {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _only: PhantomData<keyword::Only>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadWriteMode {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _write: PhantomData<keyword::WriteKw>,
}

/// `[NOT] DEFERRABLE` transaction mode.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotDeferrableMode {
    pub _not: PhantomData<keyword::Not>,
    pub _deferrable: PhantomData<keyword::Deferrable>,
}

/// A single transaction mode.
///
/// Variant ordering: multi-word before single, and `NotDeferrable` (NOT
/// DEFERRABLE) before bare `Deferrable`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TransactionMode {
    IsolationLevel(IsolationLevelMode),
    ReadOnly(ReadOnlyMode),
    ReadWrite(ReadWriteMode),
    NotDeferrable(NotDeferrableMode),
    Deferrable(keyword::Deferrable),
}

/// Optional `WORK | TRANSACTION` suffix.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum WorkOrTransaction {
    Work(keyword::Work),
    Transaction(keyword::Transaction),
}

/// BEGIN [WORK | TRANSACTION] [transaction_mode [, ...]]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct BeginStmt {
    pub _begin: PhantomData<keyword::Begin>,
    pub work: Option<WorkOrTransaction>,
    pub modes: Option<Seq<TransactionMode, punct::Comma>>,
}

/// END [WORK | TRANSACTION] — alias for COMMIT.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct EndStmt {
    pub _end: PhantomData<keyword::End>,
    pub work: Option<WorkOrTransaction>,
}

/// ABORT [WORK | TRANSACTION] — alias for ROLLBACK.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AbortStmt {
    pub _abort: PhantomData<keyword::Abort>,
    pub work: Option<WorkOrTransaction>,
}

/// START TRANSACTION [transaction_mode [, ...]]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StartTransactionStmt {
    pub _start: PhantomData<keyword::Start>,
    pub _transaction: PhantomData<keyword::Transaction>,
    pub modes: Option<Seq<TransactionMode, punct::Comma>>,
}

/// SET TRANSACTION transaction_mode [, ...]
/// SET SESSION CHARACTERISTICS AS TRANSACTION transaction_mode [, ...]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetTransactionStmt {
    pub _set: PhantomData<keyword::Set>,
    pub target: SetTransactionTarget,
    pub modes: Seq<TransactionMode, punct::Comma>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetTransactionTarget {
    SessionCharacteristics(SetSessionCharacteristics),
    Transaction(keyword::Transaction),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetSessionCharacteristics {
    pub _session: PhantomData<keyword::Session>,
    pub _characteristics: PhantomData<keyword::Characteristics>,
    pub _as: PhantomData<keyword::As>,
    pub _transaction: PhantomData<keyword::Transaction>,
}

/// `SET CONSTRAINTS { ALL | name [, …] } { DEFERRED | IMMEDIATE }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetConstraintsStmt<'input> {
    pub _set: PhantomData<keyword::Set>,
    pub _constraints: PhantomData<keyword::Constraints>,
    pub target: SetConstraintsTarget<'input>,
    pub mode: DeferredMode,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetConstraintsTarget<'input> {
    All(keyword::All),
    Names(Seq<literal::Ident<'input>, punct::Comma>),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum DeferredMode {
    Deferred(keyword::Deferred),
    Immediate(keyword::Immediate),
}

/// COMMIT [WORK | TRANSACTION]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CommitStmt<'input> {
    pub _commit: PhantomData<keyword::Commit>,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// ROLLBACK [WORK | TRANSACTION] [TO [SAVEPOINT] name]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RollbackStmt<'input> {
    pub _rollback: PhantomData<keyword::Rollback>,
    pub tail: Option<RawStatement<'input>>,
}

/// SAVEPOINT name
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SavepointStmt<'input> {
    pub _savepoint: PhantomData<keyword::Savepoint>,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// RELEASE [SAVEPOINT] name
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReleaseStmt<'input> {
    pub _release: PhantomData<keyword::Release>,
    pub tail: Option<RawStatement<'input>>,
}

// --- PREPARE / EXECUTE / DEALLOCATE ---

/// PREPARE name [(types)] AS statement
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrepareStmt<'input> {
    pub _prepare: PhantomData<keyword::Prepare>,
    pub tail: Option<RawStatement<'input>>,
}

/// EXECUTE name [(params)]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExecuteStmt<'input> {
    pub _execute: PhantomData<keyword::Execute>,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// DEALLOCATE [PREPARE] name | ALL
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeallocateStmt<'input> {
    pub _deallocate: PhantomData<keyword::Deallocate>,
    pub tail: Option<RawStatement<'input>>,
}

// --- GRANT / REVOKE ---

/// GRANT privileges ON object TO role [WITH GRANT OPTION]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GrantStmt<'input> {
    pub _grant: PhantomData<keyword::Grant>,
    pub tail: Option<RawStatement<'input>>,
}

/// REVOKE privileges ON object FROM role
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RevokeStmt<'input> {
    pub _revoke: PhantomData<keyword::Revoke>,
    pub tail: Option<RawStatement<'input>>,
}

// --- COPY ---

/// COPY table [(columns)] FROM/TO target [WITH options]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CopyStmt<'input> {
    pub _copy: PhantomData<keyword::Copy>,
    pub tail: Option<RawStatement<'input>>,
}

// --- TRUNCATE ---

/// ```sql
/// TRUNCATE [TABLE] name [, ...] [CASCADE | RESTRICT]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TruncateStmt<'input> {
    pub _truncate: PhantomData<keyword::Truncate>,
    pub tail: Option<RawStatement<'input>>,
}

// --- COMMENT ---

/// COMMENT ON object IS 'text' | NULL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CommentStmt<'input> {
    pub _comment: PhantomData<keyword::Comment>,
    pub tail: Option<RawStatement<'input>>,
}

// --- LOCK ---

/// ```sql
/// LOCK [TABLE] name [, ...] [IN mode MODE] [NOWAIT]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LockStmt<'input> {
    pub _lock: PhantomData<keyword::Lock>,
    pub tail: Option<RawStatement<'input>>,
}

// --- Cursor operations ---

/// DECLARE name CURSOR FOR query
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeclareStmt<'input> {
    pub _declare: PhantomData<keyword::Declare>,
    pub tail: Option<RawStatement<'input>>,
}

/// `FROM` or `IN` cursor-source keyword in FETCH/MOVE.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchSource {
    From(keyword::From),
    In(keyword::In),
}

/// `ABSOLUTE n` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchAbsolute<'input> {
    pub _absolute: PhantomData<keyword::Absolute>,
    pub count: literal::IntegerLit<'input>,
}

/// `RELATIVE n` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchRelative<'input> {
    pub _relative: PhantomData<keyword::Relative>,
    pub count: literal::IntegerLit<'input>,
}

/// `FORWARD [n|ALL]` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchForward<'input> {
    pub _forward: PhantomData<keyword::Forward>,
    pub count: Option<FetchCountOrAll<'input>>,
}

/// `BACKWARD [n|ALL]` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchBackward<'input> {
    pub _backward: PhantomData<keyword::Backward>,
    pub count: Option<FetchCountOrAll<'input>>,
}

/// A count or `ALL` marker following `FORWARD`/`BACKWARD`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchCountOrAll<'input> {
    All(keyword::All),
    Count(literal::IntegerLit<'input>),
}

/// FETCH/MOVE direction clause.
///
/// Variant ordering: multi-token forms (`ABSOLUTE n`, `RELATIVE n`,
/// `FORWARD [...]`, `BACKWARD [...]`) before single-keyword directions.
/// `Count` (bare integer) listed last since it has no keyword prefix.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchDirection<'input> {
    Absolute(FetchAbsolute<'input>),
    Relative(FetchRelative<'input>),
    Forward(FetchForward<'input>),
    Backward(FetchBackward<'input>),
    Next(keyword::Next),
    Prior(keyword::Prior),
    First(keyword::First),
    Last(keyword::Last),
    All(keyword::All),
    Count(literal::IntegerLit<'input>),
}

/// ```sql
/// FETCH [direction] [FROM|IN] cursor_name
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchStmt<'input> {
    pub _fetch: PhantomData<keyword::Fetch>,
    pub direction: Option<FetchDirection<'input>>,
    pub source: Option<FetchSource>,
    pub cursor: literal::AliasName<'input>,
}

/// CLOSE cursor | ALL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CloseStmt<'input> {
    pub _close: PhantomData<keyword::Close>,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// MOVE [direction] [FROM|IN] cursor_name
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MoveStmt<'input> {
    pub _move: PhantomData<keyword::Move>,
    pub direction: Option<FetchDirection<'input>>,
    pub source: Option<FetchSource>,
    pub cursor: literal::AliasName<'input>,
}

// --- REINDEX ---

/// A single option inside a VACUUM/REINDEX `( ... )` list: `name [value]`.
///
/// The option name may be any SQL word (including keywords like `FULL`,
/// `FREEZE`, `PARALLEL`) so it uses `AliasName`. The value is any expression,
/// which covers integers, floats, identifiers, boolean literals, and signed
/// numbers (`-1`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VacuumOption<'input> {
    pub name: literal::AliasName<'input>,
    pub value: Option<Expr<'input>>,
}

/// Parenthesized options list: `( opt [= val] [, ...] )`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VacuumOptions<'input> {
    pub list: Surrounded<punct::LParen, Seq<VacuumOption<'input>, punct::Comma>, punct::RParen>,
}

/// REINDEX [( options )] { INDEX | TABLE | SCHEMA | DATABASE | SYSTEM } name
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReindexStmt<'input> {
    pub _reindex: PhantomData<keyword::Reindex>,
    pub options: Option<VacuumOptions<'input>>,
    pub tail: Option<RawStatement<'input>>,
}

// --- REFRESH ---

/// ```sql
/// REFRESH MATERIALIZED VIEW [CONCURRENTLY] name [WITH [NO] DATA]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RefreshStmt<'input> {
    pub _refresh: PhantomData<keyword::Refresh>,
    pub tail: Option<RawStatement<'input>>,
}

// --- NOTIFY / LISTEN / UNLISTEN ---

/// NOTIFY channel [, payload]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotifyStmt<'input> {
    pub _notify: PhantomData<keyword::Notify>,
    pub tail: Option<RawStatement<'input>>,
}

/// LISTEN channel
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ListenStmt<'input> {
    pub _listen: PhantomData<keyword::Listen>,
    pub tail: Option<RawStatement<'input>>,
}

/// Target of an UNLISTEN statement: a channel name or `*` (all channels).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum UnlistenTarget<'input> {
    /// `*` — unlisten from every channel.
    All(punct::Star),
    /// A specific channel name.
    Channel(literal::Ident<'input>),
}

/// UNLISTEN channel | *
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnlistenStmt<'input> {
    pub _unlisten: PhantomData<keyword::Unlisten>,
    pub target: UnlistenTarget<'input>,
}

// --- DO ---

/// `DO [LANGUAGE lang] $$ ... $$` anonymous code block.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoStmt<'input> {
    pub _do: PhantomData<keyword::DoBlock>,
    pub language: Option<DoLanguage<'input>>,
    pub body: literal::DollarStringLit<'input>,
    pub trailing_language: Option<DoLanguage<'input>>,
}

/// `LANGUAGE lang` clause on a `DO` block (may appear before or after body).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoLanguage<'input> {
    pub _language: PhantomData<keyword::Language>,
    pub name: literal::Ident<'input>,
}

// --- DISCARD ---

/// DISCARD ALL | PLANS | SEQUENCES | TEMP
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DiscardStmt<'input> {
    pub _discard: PhantomData<keyword::Discard>,
    pub tail: Option<RawStatement<'input>>,
}

// --- REASSIGN ---

/// REASSIGN OWNED BY role TO role
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReassignStmt<'input> {
    pub _reassign: PhantomData<keyword::Reassign>,
    pub tail: Option<RawStatement<'input>>,
}

// --- SECURITY LABEL ---

/// SECURITY LABEL [FOR provider] ON object IS 'label' | NULL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SecurityLabelStmt<'input> {
    pub _security: PhantomData<keyword::Security>,
    pub tail: Option<RawStatement<'input>>,
}

// --- CLUSTER ---

/// ```sql
/// CLUSTER [VERBOSE] [table [USING index]]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ClusterStmt<'input> {
    pub _cluster: PhantomData<keyword::Clusterw>,
    pub tail: Option<RawStatement<'input>>,
}

// --- VACUUM ---

/// VACUUM [(options)] [table [(columns)]]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VacuumStmt<'input> {
    pub _vacuum: PhantomData<keyword::Vacuumw>,
    pub options: Option<VacuumOptions<'input>>,
    pub tail: Option<RawStatement<'input>>,
}

// --- ALTER TABLE ---

/// ALTER TABLE ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTableStmt<'input> {
    pub _alter: PhantomData<keyword::Alter>,
    pub _table: PhantomData<keyword::Table>,
    pub tail: Option<RawStatement<'input>>,
}

/// `CHECKPOINT` — force a transaction log checkpoint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CheckpointStmt {
    pub _checkpoint: PhantomData<keyword::Checkpoint>,
}

/// `ALTER DEFAULT PRIVILEGES [FOR ROLE ...] [IN SCHEMA ...] { GRANT | REVOKE } ...`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterDefaultPrivilegesStmt<'input> {
    pub _alter: PhantomData<keyword::Alter>,
    pub _default: PhantomData<keyword::Default>,
    pub _privileges: PhantomData<keyword::Privileges>,
    pub tail: Option<RawStatement<'input>>,
}

// --- CREATE/DROP for types not yet fully parsed ---
// These capture the leading keywords for disambiguation, with raw tail.

/// CREATE TRIGGER ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTriggerStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub _trigger: PhantomData<keyword::Trigger>,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP TRIGGER ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTriggerStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _trigger: PhantomData<keyword::Trigger>,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE [OR REPLACE] RULE ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateRuleStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub or_replace: Option<crate::ast::create_view::OrReplaceKw>,
    pub _rule: PhantomData<keyword::Rule>,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP RULE ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropRuleStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _rule: PhantomData<keyword::Rule>,
    pub tail: Option<RawStatement<'input>>,
}

/// Optional `TEMP` or `TEMPORARY` modifier that can appear between `CREATE`
/// and the object keyword for temporary objects (sequences, tables, views,
/// etc.).
///
/// Variant ordering: `Temporary` (longer) before `Temp` so the longer keyword
/// wins longest-match disambiguation.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TempModifier {
    Temporary(keyword::Temporary),
    Temp(keyword::Temp),
}

// --- CREATE/DROP/ALTER for remaining object types ---
// Each captures the leading keyword pair for enum disambiguation.

macro_rules! create_drop_stmts {
    ($($name:ident, $create_name:ident, $drop_name:ident, $kw:ident);* $(;)?) => {
        $(
            #[railroad]
            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $create_name<'input> {
                pub _create: PhantomData<keyword::Create>,
                /// Optional temporary modifier: `TEMP` or `TEMPORARY`. Postgres
                /// accepts it on sequence/view/table/etc. so we tolerate it
                /// uniformly in these raw-tailed stubs.
                pub temp: Option<TempModifier>,
                pub _obj: PhantomData<keyword::$kw>,
                pub tail: Option<RawStatement<'input>>,
            }

            #[railroad]
            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $drop_name<'input> {
                pub _drop: PhantomData<keyword::Drop>,
                pub _obj: PhantomData<keyword::$kw>,
                pub tail: Option<RawStatement<'input>>,
            }
        )*
    };
}

create_drop_stmts! {
    Group, CreateGroupStmt, DropGroupStmt, Group;
    Role, CreateRoleStmt, DropRoleStmt, Role;
    User, CreateUserStmt, DropUserStmt, User;
    Schema, CreateSchemaStmt, DropSchemaStmt, Schema;
    Sequence, CreateSequenceStmt, DropSequenceStmt, Sequence;
    Type, CreateTypeStmt, DropTypeStmt, Type;
    Domain, CreateDomainStmt, DropDomainStmt, Domain;
    Aggregate, CreateAggregateStmt, DropAggregateStmt, Aggregate;
    Operator, CreateOperatorStmt, DropOperatorStmt, Operator;
    Cast, CreateCastStmt, DropCastStmt, Cast;
    Collation, CreateCollationStmt, DropCollationStmt, Collation;
    Extension, CreateExtensionStmt, DropExtensionStmt, Extension;
    Policy, CreatePolicyStmt, DropPolicyStmt, Policy;
    Statistics, CreateStatisticsStmt, DropStatisticsStmt, Statistics;
    Publication, CreatePublicationStmt, DropPublicationStmt, Publication;
    Subscription, CreateSubscriptionStmt, DropSubscriptionStmt, Subscription;
    Conversion, CreateConversionStmt, DropConversionStmt, Conversion;
    Server, CreateServerStmt, DropServerStmt, Server;
    Language, CreateLanguageStmt, DropLanguageStmt, Language;
    Database, CreateDatabaseStmt, DropDatabaseStmt, Database;
}

macro_rules! alter_stmts {
    ($($name:ident, $alter_name:ident, $kw:ident);* $(;)?) => {
        $(
            #[railroad]
            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $alter_name<'input> {
                pub _alter: PhantomData<keyword::Alter>,
                pub _obj: PhantomData<keyword::$kw>,
                pub tail: Option<RawStatement<'input>>,
            }
        )*
    };
}

alter_stmts! {
    Rule, AlterRuleStmt, Rule;
    Group, AlterGroupStmt, Group;
    Role, AlterRoleStmt, Role;
    User, AlterUserStmt, User;
    Schema, AlterSchemaStmt, Schema;
    Sequence, AlterSequenceStmt, Sequence;
    Type, AlterTypeStmt, Type;
    Domain, AlterDomainStmt, Domain;
    Aggregate, AlterAggregateStmt, Aggregate;
    Operator, AlterOperatorStmt, Operator;
    Collation, AlterCollationStmt, Collation;
    Extension, AlterExtensionStmt, Extension;
    Policy, AlterPolicyStmt, Policy;
    Statistics, AlterStatisticsStmt, Statistics;
    Publication, AlterPublicationStmt, Publication;
    Subscription, AlterSubscriptionStmt, Subscription;
    Conversion, AlterConversionStmt, Conversion;
    Server, AlterServerStmt, Server;
    Index, AlterIndexStmt, Index;
    View, AlterViewStmt, View;
    Function, AlterFunctionStmt, Function;
    Language, AlterLanguageStmt, Language;
    Database, AlterDatabaseStmt, Database;
}

// Special multi-keyword DDL types

/// CREATE FOREIGN TABLE / DATA WRAPPER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterForeignStmt<'input> {
    pub _alter: PhantomData<keyword::Alter>,
    pub _foreign: PhantomData<keyword::Foreign>,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE EVENT TRIGGER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateEventTriggerStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub _event: PhantomData<keyword::Event>,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP EVENT TRIGGER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropEventTriggerStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _event: PhantomData<keyword::Event>,
    pub tail: Option<RawStatement<'input>>,
}

/// ALTER EVENT TRIGGER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterEventTriggerStmt<'input> {
    pub _alter: PhantomData<keyword::Alter>,
    pub _event: PhantomData<keyword::Event>,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP OWNED BY
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropOwnedStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _owned: PhantomData<keyword::Owned>,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE ACCESS METHOD
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateAccessMethodStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub _access: PhantomData<keyword::Access>,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP ACCESS METHOD
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropAccessMethodStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _access: PhantomData<keyword::Access>,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE MATERIALIZED VIEW
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateMaterializedViewStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub _materialized: PhantomData<keyword::Materialized>,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP MATERIALIZED VIEW
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropMaterializedViewStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _materialized: PhantomData<keyword::Materialized>,
    pub tail: Option<RawStatement<'input>>,
}

/// ALTER MATERIALIZED VIEW
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterMaterializedViewStmt<'input> {
    pub _alter: PhantomData<keyword::Alter>,
    pub _materialized: PhantomData<keyword::Materialized>,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE FOREIGN ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateForeignStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub _foreign: PhantomData<keyword::Foreign>,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP FOREIGN ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropForeignStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _foreign: PhantomData<keyword::Foreign>,
    pub tail: Option<RawStatement<'input>>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use super::*;
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_group() {
        let mut input = Input::new("CREATE GROUP g1");
        let _stmt = CreateGroupStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_group_with_users() {
        let mut input = Input::new("CREATE GROUP g1 WITH USER u1, u2");
        let _stmt = CreateGroupStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_alter_group_add_user() {
        let mut input = Input::new("ALTER GROUP g1 ADD USER u1");
        let _stmt = AlterGroupStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_alter_group_drop_user() {
        let mut input = Input::new("ALTER GROUP g1 DROP USER u1");
        let _stmt = AlterGroupStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_language() {
        let mut input = Input::new("CREATE LANGUAGE plpgsql HANDLER plpgsql_call_handler");
        let _stmt = CreateLanguageStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_alter_language_owner() {
        let mut input = Input::new("ALTER LANGUAGE plpgsql OWNER TO foo");
        let _stmt = AlterLanguageStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_language() {
        let mut input = Input::new("DROP LANGUAGE plpgsql");
        let _stmt = DropLanguageStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_group() {
        let mut input = Input::new("DROP GROUP g1");
        let _stmt = DropGroupStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_end_stmt() {
        let mut input = Input::new("END");
        let _stmt = EndStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_abort_stmt() {
        let mut input = Input::new("ABORT");
        let _stmt = AbortStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_abort_work() {
        let mut input = Input::new("ABORT WORK");
        let _stmt = AbortStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_start_transaction_read_write() {
        let mut input = Input::new("START TRANSACTION READ WRITE");
        let _stmt = StartTransactionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_transaction_modes() {
        let mut input =
            Input::new("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ ONLY, DEFERRABLE");
        let _stmt = SetTransactionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_transaction_read_write() {
        let mut input = Input::new("SET TRANSACTION READ WRITE");
        let _stmt = SetTransactionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_session_characteristics() {
        let mut input = Input::new("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY");
        let _stmt = SetTransactionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_constraints_all_deferred() {
        let mut input = Input::new("SET CONSTRAINTS ALL DEFERRED");
        let _stmt = SetConstraintsStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_begin_isolation() {
        let mut input = Input::new("BEGIN ISOLATION LEVEL SERIALIZABLE");
        let _stmt = BeginStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_vacuum_full() {
        let mut input = Input::new("VACUUM (FULL) tbl");
        let stmt = VacuumStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.options.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_vacuum_full_freeze() {
        let mut input = Input::new("VACUUM (FULL, FREEZE) tbl");
        let _stmt = VacuumStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_vacuum_parallel_value() {
        let mut input = Input::new("VACUUM (PARALLEL 2) tbl");
        let _stmt = VacuumStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reindex_tablespace_table() {
        let mut input = Input::new("REINDEX (TABLESPACE ts) TABLE tbl");
        let _stmt = ReindexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reindex_verbose_index() {
        let mut input = Input::new("REINDEX (VERBOSE) INDEX i");
        let _stmt = ReindexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
