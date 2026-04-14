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

use crate::ast::expr::Expr;
use crate::ast::RawStatement;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

// --- Transaction control ---

/// Isolation level following `ISOLATION LEVEL`.
///
/// Variant ordering: multi-word forms before single-word `Serializable`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum IsolationLevelKind {
    RepeatableRead(RepeatableReadLevel),
    ReadCommitted(ReadCommittedLevel),
    ReadUncommitted(ReadUncommittedLevel),
    Serializable(keyword::Serializable),
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RepeatableReadLevel {
    pub _repeatable: PhantomData<keyword::Repeatable>,
    pub _read: PhantomData<keyword::ReadKw>,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadCommittedLevel {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _committed: PhantomData<keyword::Committed>,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadUncommittedLevel {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _uncommitted: PhantomData<keyword::Uncommitted>,
}

/// `ISOLATION LEVEL <level>` transaction mode.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsolationLevelMode {
    pub _isolation: PhantomData<keyword::Isolation>,
    pub _level: PhantomData<keyword::Level>,
    pub level: IsolationLevelKind,
}

/// `READ ONLY` or `READ WRITE` transaction mode.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadOnlyMode {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _only: PhantomData<keyword::Only>,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReadWriteMode {
    pub _read: PhantomData<keyword::ReadKw>,
    pub _write: PhantomData<keyword::WriteKw>,
}

/// `[NOT] DEFERRABLE` transaction mode.
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
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum WorkOrTransaction {
    Work(keyword::Work),
    Transaction(keyword::Transaction),
}

/// BEGIN [WORK | TRANSACTION] [transaction_mode [, ...]]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct BeginStmt {
    pub _begin: PhantomData<keyword::Begin>,
    pub work: Option<WorkOrTransaction>,
    pub modes: Option<Seq<TransactionMode, punct::Comma>>,
}

/// END [WORK | TRANSACTION] — alias for COMMIT.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct EndStmt {
    pub _end: PhantomData<keyword::End>,
    pub work: Option<WorkOrTransaction>,
}

/// ABORT [WORK | TRANSACTION] — alias for ROLLBACK.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AbortStmt {
    pub _abort: PhantomData<keyword::Abort>,
    pub work: Option<WorkOrTransaction>,
}

/// START TRANSACTION [transaction_mode [, ...]]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StartTransactionStmt {
    pub _start: PhantomData<keyword::Start>,
    pub _transaction: PhantomData<keyword::Transaction>,
    pub modes: Option<Seq<TransactionMode, punct::Comma>>,
}

/// SET TRANSACTION transaction_mode [, ...]
/// SET SESSION CHARACTERISTICS AS TRANSACTION transaction_mode [, ...]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetTransactionStmt {
    pub _set: PhantomData<keyword::Set>,
    pub target: SetTransactionTarget,
    pub modes: Seq<TransactionMode, punct::Comma>,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetTransactionTarget {
    SessionCharacteristics(SetSessionCharacteristics),
    Transaction(keyword::Transaction),
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetSessionCharacteristics {
    pub _session: PhantomData<keyword::Session>,
    pub _characteristics: PhantomData<keyword::Characteristics>,
    pub _as: PhantomData<keyword::As>,
    pub _transaction: PhantomData<keyword::Transaction>,
}

/// `SET CONSTRAINTS { ALL | name [, …] } { DEFERRED | IMMEDIATE }`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetConstraintsStmt {
    pub _set: PhantomData<keyword::Set>,
    pub _constraints: PhantomData<keyword::Constraints>,
    pub target: SetConstraintsTarget,
    pub mode: DeferredMode,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetConstraintsTarget {
    All(keyword::All),
    Names(Seq<literal::Ident, punct::Comma>),
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum DeferredMode {
    Deferred(keyword::Deferred),
    Immediate(keyword::Immediate),
}

/// COMMIT [WORK | TRANSACTION]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CommitStmt {
    pub _commit: PhantomData<keyword::Commit>,
    pub tail: Option<RawStatement>,
}

/// ```sql
/// ROLLBACK [WORK | TRANSACTION] [TO [SAVEPOINT] name]
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RollbackStmt {
    pub _rollback: PhantomData<keyword::Rollback>,
    pub tail: Option<RawStatement>,
}

/// SAVEPOINT name
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SavepointStmt {
    pub _savepoint: PhantomData<keyword::Savepoint>,
    pub tail: Option<RawStatement>,
}

/// ```sql
/// RELEASE [SAVEPOINT] name
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReleaseStmt {
    pub _release: PhantomData<keyword::Release>,
    pub tail: Option<RawStatement>,
}

// --- PREPARE / EXECUTE / DEALLOCATE ---

/// PREPARE name [(types)] AS statement
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrepareStmt {
    pub _prepare: PhantomData<keyword::Prepare>,
    pub tail: Option<RawStatement>,
}

/// EXECUTE name [(params)]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExecuteStmt {
    pub _execute: PhantomData<keyword::Execute>,
    pub tail: Option<RawStatement>,
}

/// ```sql
/// DEALLOCATE [PREPARE] name | ALL
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeallocateStmt {
    pub _deallocate: PhantomData<keyword::Deallocate>,
    pub tail: Option<RawStatement>,
}

// --- GRANT / REVOKE ---

/// GRANT privileges ON object TO role [WITH GRANT OPTION]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GrantStmt {
    pub _grant: PhantomData<keyword::Grant>,
    pub tail: Option<RawStatement>,
}

/// REVOKE privileges ON object FROM role
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RevokeStmt {
    pub _revoke: PhantomData<keyword::Revoke>,
    pub tail: Option<RawStatement>,
}

// --- COPY ---

/// COPY table [(columns)] FROM/TO target [WITH options]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CopyStmt {
    pub _copy: PhantomData<keyword::Copy>,
    pub tail: Option<RawStatement>,
}

// --- TRUNCATE ---

/// ```sql
/// TRUNCATE [TABLE] name [, ...] [CASCADE | RESTRICT]
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TruncateStmt {
    pub _truncate: PhantomData<keyword::Truncate>,
    pub tail: Option<RawStatement>,
}

// --- COMMENT ---

/// COMMENT ON object IS 'text' | NULL
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CommentStmt {
    pub _comment: PhantomData<keyword::Comment>,
    pub tail: Option<RawStatement>,
}

// --- LOCK ---

/// ```sql
/// LOCK [TABLE] name [, ...] [IN mode MODE] [NOWAIT]
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LockStmt {
    pub _lock: PhantomData<keyword::Lock>,
    pub tail: Option<RawStatement>,
}

// --- Cursor operations ---

/// DECLARE name CURSOR FOR query
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeclareStmt {
    pub _declare: PhantomData<keyword::Declare>,
    pub tail: Option<RawStatement>,
}

/// `FROM` or `IN` cursor-source keyword in FETCH/MOVE.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchSource {
    From(keyword::From),
    In(keyword::In),
}

/// `ABSOLUTE n` form.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchAbsolute {
    pub _absolute: PhantomData<keyword::Absolute>,
    pub count: literal::IntegerLit,
}

/// `RELATIVE n` form.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchRelative {
    pub _relative: PhantomData<keyword::Relative>,
    pub count: literal::IntegerLit,
}

/// `FORWARD [n|ALL]` form.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchForward {
    pub _forward: PhantomData<keyword::Forward>,
    pub count: Option<FetchCountOrAll>,
}

/// `BACKWARD [n|ALL]` form.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchBackward {
    pub _backward: PhantomData<keyword::Backward>,
    pub count: Option<FetchCountOrAll>,
}

/// A count or `ALL` marker following `FORWARD`/`BACKWARD`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchCountOrAll {
    All(keyword::All),
    Count(literal::IntegerLit),
}

/// FETCH/MOVE direction clause.
///
/// Variant ordering: multi-token forms (`ABSOLUTE n`, `RELATIVE n`,
/// `FORWARD [...]`, `BACKWARD [...]`) before single-keyword directions.
/// `Count` (bare integer) listed last since it has no keyword prefix.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchDirection {
    Absolute(FetchAbsolute),
    Relative(FetchRelative),
    Forward(FetchForward),
    Backward(FetchBackward),
    Next(keyword::Next),
    Prior(keyword::Prior),
    First(keyword::First),
    Last(keyword::Last),
    All(keyword::All),
    Count(literal::IntegerLit),
}

/// ```sql
/// FETCH [direction] [FROM|IN] cursor_name
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchStmt {
    pub _fetch: PhantomData<keyword::Fetch>,
    pub direction: Option<FetchDirection>,
    pub source: Option<FetchSource>,
    pub cursor: literal::AliasName,
}

/// CLOSE cursor | ALL
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CloseStmt {
    pub _close: PhantomData<keyword::Close>,
    pub tail: Option<RawStatement>,
}

/// ```sql
/// MOVE [direction] [FROM|IN] cursor_name
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MoveStmt {
    pub _move: PhantomData<keyword::Move>,
    pub direction: Option<FetchDirection>,
    pub source: Option<FetchSource>,
    pub cursor: literal::AliasName,
}

// --- REINDEX ---

/// A single option inside a VACUUM/REINDEX `( ... )` list: `name [value]`.
///
/// The option name may be any SQL word (including keywords like `FULL`,
/// `FREEZE`, `PARALLEL`) so it uses `AliasName`. The value is any expression,
/// which covers integers, floats, identifiers, boolean literals, and signed
/// numbers (`-1`).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VacuumOption {
    pub name: literal::AliasName,
    pub value: Option<Expr>,
}

/// Parenthesized options list: `( opt [= val] [, ...] )`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VacuumOptions {
    pub list: Surrounded<punct::LParen, Seq<VacuumOption, punct::Comma>, punct::RParen>,
}

/// REINDEX [( options )] { INDEX | TABLE | SCHEMA | DATABASE | SYSTEM } name
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReindexStmt {
    pub _reindex: PhantomData<keyword::Reindex>,
    pub options: Option<VacuumOptions>,
    pub tail: Option<RawStatement>,
}

// --- REFRESH ---

/// ```sql
/// REFRESH MATERIALIZED VIEW [CONCURRENTLY] name [WITH [NO] DATA]
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RefreshStmt {
    pub _refresh: PhantomData<keyword::Refresh>,
    pub tail: Option<RawStatement>,
}

// --- NOTIFY / LISTEN / UNLISTEN ---

/// NOTIFY channel [, payload]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotifyStmt {
    pub _notify: PhantomData<keyword::Notify>,
    pub tail: Option<RawStatement>,
}

/// LISTEN channel
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ListenStmt {
    pub _listen: PhantomData<keyword::Listen>,
    pub tail: Option<RawStatement>,
}

/// UNLISTEN channel | *
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnlistenStmt {
    pub _unlisten: PhantomData<keyword::Unlisten>,
    pub tail: Option<RawStatement>,
}

// --- DO ---

/// DO [LANGUAGE lang] code
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoStmt {
    pub _do: PhantomData<keyword::DoBlock>,
    pub tail: Option<RawStatement>,
}

// --- DISCARD ---

/// DISCARD ALL | PLANS | SEQUENCES | TEMP
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DiscardStmt {
    pub _discard: PhantomData<keyword::Discard>,
    pub tail: Option<RawStatement>,
}

// --- REASSIGN ---

/// REASSIGN OWNED BY role TO role
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReassignStmt {
    pub _reassign: PhantomData<keyword::Reassign>,
    pub tail: Option<RawStatement>,
}

// --- SECURITY LABEL ---

/// SECURITY LABEL [FOR provider] ON object IS 'label' | NULL
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SecurityLabelStmt {
    pub _security: PhantomData<keyword::Security>,
    pub tail: Option<RawStatement>,
}

// --- CLUSTER ---

/// ```sql
/// CLUSTER [VERBOSE] [table [USING index]]
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ClusterStmt {
    pub _cluster: PhantomData<keyword::Clusterw>,
    pub tail: Option<RawStatement>,
}

// --- VACUUM ---

/// VACUUM [(options)] [table [(columns)]]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VacuumStmt {
    pub _vacuum: PhantomData<keyword::Vacuumw>,
    pub options: Option<VacuumOptions>,
    pub tail: Option<RawStatement>,
}

// --- ALTER TABLE ---

/// ALTER TABLE ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTableStmt {
    pub _alter: PhantomData<keyword::Alter>,
    pub _table: PhantomData<keyword::Table>,
    pub tail: Option<RawStatement>,
}

// --- CREATE/DROP for types not yet fully parsed ---
// These capture the leading keywords for disambiguation, with raw tail.

/// CREATE TRIGGER ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTriggerStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _trigger: PhantomData<keyword::Trigger>,
    pub tail: Option<RawStatement>,
}

/// DROP TRIGGER ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTriggerStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _trigger: PhantomData<keyword::Trigger>,
    pub tail: Option<RawStatement>,
}

/// CREATE RULE ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateRuleStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _rule: PhantomData<keyword::Rule>,
    pub tail: Option<RawStatement>,
}

/// DROP RULE ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropRuleStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _rule: PhantomData<keyword::Rule>,
    pub tail: Option<RawStatement>,
}

// --- CREATE/DROP/ALTER for remaining object types ---
// Each captures the leading keyword pair for enum disambiguation.

macro_rules! create_drop_stmts {
    ($($name:ident, $create_name:ident, $drop_name:ident, $kw:ident);* $(;)?) => {
        $(
            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $create_name {
                pub _create: PhantomData<keyword::Create>,
                pub _obj: PhantomData<keyword::$kw>,
                pub tail: Option<RawStatement>,
            }

            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $drop_name {
                pub _drop: PhantomData<keyword::Drop>,
                pub _obj: PhantomData<keyword::$kw>,
                pub tail: Option<RawStatement>,
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
}

macro_rules! alter_stmts {
    ($($name:ident, $alter_name:ident, $kw:ident);* $(;)?) => {
        $(
            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $alter_name {
                pub _alter: PhantomData<keyword::Alter>,
                pub _obj: PhantomData<keyword::$kw>,
                pub tail: Option<RawStatement>,
            }
        )*
    };
}

alter_stmts! {
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
}

// Special multi-keyword DDL types

/// CREATE FOREIGN TABLE / DATA WRAPPER
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterForeignStmt {
    pub _alter: PhantomData<keyword::Alter>,
    pub _foreign: PhantomData<keyword::Foreign>,
    pub tail: Option<RawStatement>,
}

/// CREATE EVENT TRIGGER
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateEventTriggerStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _event: PhantomData<keyword::Event>,
    pub tail: Option<RawStatement>,
}

/// DROP EVENT TRIGGER
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropEventTriggerStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _event: PhantomData<keyword::Event>,
    pub tail: Option<RawStatement>,
}

/// ALTER EVENT TRIGGER
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterEventTriggerStmt {
    pub _alter: PhantomData<keyword::Alter>,
    pub _event: PhantomData<keyword::Event>,
    pub tail: Option<RawStatement>,
}

/// DROP OWNED BY
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropOwnedStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _owned: PhantomData<keyword::Owned>,
    pub tail: Option<RawStatement>,
}

/// CREATE ACCESS METHOD
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateAccessMethodStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _access: PhantomData<keyword::Access>,
    pub tail: Option<RawStatement>,
}

/// DROP ACCESS METHOD
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropAccessMethodStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _access: PhantomData<keyword::Access>,
    pub tail: Option<RawStatement>,
}

/// CREATE MATERIALIZED VIEW
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateMaterializedViewStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _materialized: PhantomData<keyword::Materialized>,
    pub tail: Option<RawStatement>,
}

/// DROP MATERIALIZED VIEW
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropMaterializedViewStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _materialized: PhantomData<keyword::Materialized>,
    pub tail: Option<RawStatement>,
}

/// ALTER MATERIALIZED VIEW
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterMaterializedViewStmt {
    pub _alter: PhantomData<keyword::Alter>,
    pub _materialized: PhantomData<keyword::Materialized>,
    pub tail: Option<RawStatement>,
}

/// CREATE FOREIGN ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateForeignStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _foreign: PhantomData<keyword::Foreign>,
    pub tail: Option<RawStatement>,
}

/// DROP FOREIGN ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropForeignStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _foreign: PhantomData<keyword::Foreign>,
    pub tail: Option<RawStatement>,
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
