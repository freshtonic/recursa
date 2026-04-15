/// Simple keyword-led statements that are recognized by their leading keyword(s)
/// but whose body is captured as raw text (not fully parsed).
///
/// Each struct has keyword PhantomData fields for disambiguation in the Statement
/// enum, followed by an optional RawStatement tail that captures any remaining
/// content before the semicolon.
use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::RawStatement;
use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

// --- Transaction control ---

/// Isolation level following `ISOLATION LEVEL`.
///
/// Variant ordering: multi-word forms before single-word `Serializable`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum IsolationLevelKind {
    RepeatableRead((REPEATABLE, READ)),
    ReadCommitted((READ, COMMITTED)),
    ReadUncommitted((READ, UNCOMMITTED)),
    Serializable(SERIALIZABLE),
}

/// `ISOLATION LEVEL level` transaction mode.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsolationLevelMode {
    pub isolation: ISOLATION,
    pub level_kw: LEVEL,
    pub level: IsolationLevelKind,
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
    ReadOnly((READ, ONLY)),
    ReadWrite((READ, WRITE)),
    NotDeferrable((NOT, DEFERRABLE)),
    Deferrable(DEFERRABLE),
}

/// Optional `WORK | TRANSACTION` suffix.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum WorkOrTransaction {
    Work(WORK),
    Transaction(TRANSACTION),
}

/// BEGIN [WORK | TRANSACTION] [transaction_mode [, ...]]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct BeginStmt {
    pub begin: BEGIN,
    pub work: Option<WorkOrTransaction>,
    pub modes: Option<Seq<TransactionMode, punct::Comma>>,
}

/// END [WORK | TRANSACTION] — alias for COMMIT.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct EndStmt {
    pub end: END,
    pub work: Option<WorkOrTransaction>,
}

/// ABORT [WORK | TRANSACTION] — alias for ROLLBACK.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AbortStmt {
    pub abort: ABORT,
    pub work: Option<WorkOrTransaction>,
}

/// START TRANSACTION [transaction_mode [, ...]]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StartTransactionStmt {
    pub start: START,
    pub transaction: TRANSACTION,
    pub modes: Option<Seq<TransactionMode, punct::Comma>>,
}

/// SET TRANSACTION transaction_mode [, ...]
/// SET SESSION CHARACTERISTICS AS TRANSACTION transaction_mode [, ...]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetTransactionStmt {
    pub set: SET,
    pub target: SetTransactionTarget,
    pub modes: Seq<TransactionMode, punct::Comma>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetTransactionTarget {
    SessionCharacteristics((SESSION, CHARACTERISTICS, AS, TRANSACTION)),
    Transaction(TRANSACTION),
}

/// `SET CONSTRAINTS { ALL | name [, …] } { DEFERRED | IMMEDIATE }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetConstraintsStmt<'input> {
    pub set: SET,
    pub constraints: CONSTRAINTS,
    pub target: SetConstraintsTarget<'input>,
    pub mode: DeferredMode,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetConstraintsTarget<'input> {
    All(ALL),
    Names(Seq<literal::Ident<'input>, punct::Comma>),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum DeferredMode {
    Deferred(DEFERRED),
    Immediate(IMMEDIATE),
}

/// COMMIT [WORK | TRANSACTION]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CommitStmt<'input> {
    pub commit: COMMIT,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// ROLLBACK [WORK | TRANSACTION] [TO [SAVEPOINT] name]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RollbackStmt<'input> {
    pub rollback: ROLLBACK,
    pub tail: Option<RawStatement<'input>>,
}

/// SAVEPOINT name
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SavepointStmt<'input> {
    pub savepoint: SAVEPOINT,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// RELEASE [SAVEPOINT] name
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReleaseStmt<'input> {
    pub release: RELEASE,
    pub tail: Option<RawStatement<'input>>,
}

// --- PREPARE / EXECUTE / DEALLOCATE ---

/// PREPARE name [(types)] AS statement
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrepareStmt<'input> {
    pub prepare: PREPARE,
    pub tail: Option<RawStatement<'input>>,
}

/// EXECUTE name [(params)]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExecuteStmt<'input> {
    pub execute: EXECUTE,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// DEALLOCATE [PREPARE] name | ALL
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeallocateStmt<'input> {
    pub deallocate: DEALLOCATE,
    pub tail: Option<RawStatement<'input>>,
}

// --- GRANT / REVOKE ---

/// GRANT privileges ON object TO role [WITH GRANT OPTION]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GrantStmt<'input> {
    pub grant: GRANT,
    pub tail: Option<RawStatement<'input>>,
}

/// REVOKE privileges ON object FROM role
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RevokeStmt<'input> {
    pub revoke: REVOKE,
    pub tail: Option<RawStatement<'input>>,
}

// --- COPY ---

/// COPY table [(columns)] FROM/TO target [WITH options]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CopyStmt<'input> {
    pub copy: COPY,
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
    pub truncate: TRUNCATE,
    pub tail: Option<RawStatement<'input>>,
}

// --- COMMENT ---

/// COMMENT ON object IS 'text' | NULL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CommentStmt<'input> {
    pub comment: COMMENT,
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
    pub lock: LOCK,
    pub tail: Option<RawStatement<'input>>,
}

// --- Cursor operations ---

/// DECLARE name CURSOR FOR query
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeclareStmt<'input> {
    pub declare: DECLARE,
    pub tail: Option<RawStatement<'input>>,
}

/// `FROM` or `IN` cursor-source keyword in FETCH/MOVE.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchSource {
    From(FROM),
    In(IN),
}

/// `ABSOLUTE n` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchAbsolute<'input> {
    pub absolute: ABSOLUTE,
    pub count: literal::IntegerLit<'input>,
}

/// `RELATIVE n` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchRelative<'input> {
    pub relative: RELATIVE,
    pub count: literal::IntegerLit<'input>,
}

/// `FORWARD [n|ALL]` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchForward<'input> {
    pub forward: FORWARD,
    pub count: Option<FetchCountOrAll<'input>>,
}

/// `BACKWARD [n|ALL]` form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchBackward<'input> {
    pub backward: BACKWARD,
    pub count: Option<FetchCountOrAll<'input>>,
}

/// A count or `ALL` marker following `FORWARD`/`BACKWARD`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FetchCountOrAll<'input> {
    All(ALL),
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
    Next(NEXT),
    Prior(PRIOR),
    First(FIRST),
    Last(LAST),
    All(ALL),
    Count(literal::IntegerLit<'input>),
}

/// ```sql
/// FETCH [direction] [FROM|IN] cursor_name
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchStmt<'input> {
    pub fetch: FETCH,
    pub direction: Option<FetchDirection<'input>>,
    pub source: Option<FetchSource>,
    pub cursor: literal::AliasName<'input>,
}

/// CLOSE cursor | ALL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CloseStmt<'input> {
    pub close: CLOSE,
    pub tail: Option<RawStatement<'input>>,
}

/// ```sql
/// MOVE [direction] [FROM|IN] cursor_name
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MoveStmt<'input> {
    pub r#move: MOVE,
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
    pub reindex: REINDEX,
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
    pub refresh: REFRESH,
    pub tail: Option<RawStatement<'input>>,
}

// --- NOTIFY / LISTEN / UNLISTEN ---

/// NOTIFY channel [, payload]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotifyStmt<'input> {
    pub notify: NOTIFY,
    pub tail: Option<RawStatement<'input>>,
}

/// LISTEN channel
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ListenStmt<'input> {
    pub listen: LISTEN,
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
    pub unlisten: UNLISTEN,
    pub target: UnlistenTarget<'input>,
}

// --- DO ---

/// `DO [LANGUAGE lang] $$ ... $$` anonymous code block.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoStmt<'input> {
    pub r#do: DO,
    pub language: Option<DoLanguage<'input>>,
    pub body: literal::DollarStringLit<'input>,
    pub trailing_language: Option<DoLanguage<'input>>,
}

/// `LANGUAGE lang` clause on a `DO` block (may appear before or after body).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoLanguage<'input> {
    pub language: LANGUAGE,
    pub name: literal::Ident<'input>,
}

// --- DISCARD ---

/// DISCARD ALL | PLANS | SEQUENCES | TEMP
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DiscardStmt<'input> {
    pub discard: DISCARD,
    pub tail: Option<RawStatement<'input>>,
}

// --- REASSIGN ---

/// REASSIGN OWNED BY role TO role
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReassignStmt<'input> {
    pub reassign: REASSIGN,
    pub tail: Option<RawStatement<'input>>,
}

// --- SECURITY LABEL ---

/// SECURITY LABEL [FOR provider] ON object IS 'label' | NULL
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SecurityLabelStmt<'input> {
    pub security: SECURITY,
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
    pub cluster: CLUSTER,
    pub tail: Option<RawStatement<'input>>,
}

// --- VACUUM ---

/// VACUUM [(options)] [table [(columns)]]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct VacuumStmt<'input> {
    pub vacuum: VACUUM,
    pub options: Option<VacuumOptions<'input>>,
    pub tail: Option<RawStatement<'input>>,
}

// --- ALTER TABLE ---

/// ALTER TABLE ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterTableStmt<'input> {
    pub alter: ALTER,
    pub table: TABLE,
    pub tail: Option<RawStatement<'input>>,
}

/// `CHECKPOINT` — force a transaction log checkpoint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CheckpointStmt {
    pub checkpoint: CHECKPOINT,
}

/// `ALTER DEFAULT PRIVILEGES [FOR ROLE ...] [IN SCHEMA ...] { GRANT | REVOKE } ...`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterDefaultPrivilegesStmt<'input> {
    pub alter: ALTER,
    pub default: DEFAULT,
    pub privileges: PRIVILEGES,
    pub tail: Option<RawStatement<'input>>,
}

// --- CREATE/DROP for types not yet fully parsed ---
// These capture the leading keywords for disambiguation, with raw tail.

/// CREATE TRIGGER ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTriggerStmt<'input> {
    pub create: CREATE,
    pub trigger: TRIGGER,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP TRIGGER ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropTriggerStmt<'input> {
    pub drop: DROP,
    pub trigger: TRIGGER,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE [OR REPLACE] RULE ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateRuleStmt<'input> {
    pub create: CREATE,
    pub or_replace: Option<(OR, REPLACE)>,
    pub rule: RULE,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP RULE ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropRuleStmt<'input> {
    pub drop: DROP,
    pub rule: RULE,
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
    Temporary(TEMPORARY),
    Temp(TEMP),
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
                pub _create: CREATE,
                /// Optional temporary modifier: `TEMP` or `TEMPORARY`. Postgres
                /// accepts it on sequence/view/table/etc. so we tolerate it
                /// uniformly in these raw-tailed stubs.
                pub temp: Option<TempModifier>,
                pub _obj: $kw,
                pub tail: Option<RawStatement<'input>>,
            }

            #[railroad]
            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $drop_name<'input> {
                pub _drop: DROP,
                pub _obj: $kw,
                pub tail: Option<RawStatement<'input>>,
            }
        )*
    };
}

create_drop_stmts! {
    Group, CreateGroupStmt, DropGroupStmt, GROUP;
    Role, CreateRoleStmt, DropRoleStmt, ROLE;
    User, CreateUserStmt, DropUserStmt, USER;
    Schema, CreateSchemaStmt, DropSchemaStmt, SCHEMA;
    Sequence, CreateSequenceStmt, DropSequenceStmt, SEQUENCE;
    Type, CreateTypeStmt, DropTypeStmt, TYPE;
    Domain, CreateDomainStmt, DropDomainStmt, DOMAIN;
    Aggregate, CreateAggregateStmt, DropAggregateStmt, AGGREGATE;
    Operator, CreateOperatorStmt, DropOperatorStmt, OPERATOR;
    Cast, CreateCastStmt, DropCastStmt, CAST;
    Collation, CreateCollationStmt, DropCollationStmt, COLLATION;
    Extension, CreateExtensionStmt, DropExtensionStmt, EXTENSION;
    Policy, CreatePolicyStmt, DropPolicyStmt, POLICY;
    Statistics, CreateStatisticsStmt, DropStatisticsStmt, STATISTICS;
    Publication, CreatePublicationStmt, DropPublicationStmt, PUBLICATION;
    Subscription, CreateSubscriptionStmt, DropSubscriptionStmt, SUBSCRIPTION;
    Conversion, CreateConversionStmt, DropConversionStmt, CONVERSION;
    Server, CreateServerStmt, DropServerStmt, SERVER;
    Language, CreateLanguageStmt, DropLanguageStmt, LANGUAGE;
    Database, CreateDatabaseStmt, DropDatabaseStmt, DATABASE;
}

macro_rules! alter_stmts {
    ($($name:ident, $alter_name:ident, $kw:ident);* $(;)?) => {
        $(
            #[railroad]
            #[derive(Debug, Clone, FormatTokens, Parse, Visit)]
            #[parse(rules = SqlRules)]
            pub struct $alter_name<'input> {
                pub _alter: ALTER,
                pub _obj: $kw,
                pub tail: Option<RawStatement<'input>>,
            }
        )*
    };
}

alter_stmts! {
    Rule, AlterRuleStmt, RULE;
    Group, AlterGroupStmt, GROUP;
    Role, AlterRoleStmt, ROLE;
    User, AlterUserStmt, USER;
    Schema, AlterSchemaStmt, SCHEMA;
    Sequence, AlterSequenceStmt, SEQUENCE;
    Type, AlterTypeStmt, TYPE;
    Domain, AlterDomainStmt, DOMAIN;
    Aggregate, AlterAggregateStmt, AGGREGATE;
    Operator, AlterOperatorStmt, OPERATOR;
    Collation, AlterCollationStmt, COLLATION;
    Extension, AlterExtensionStmt, EXTENSION;
    Policy, AlterPolicyStmt, POLICY;
    Statistics, AlterStatisticsStmt, STATISTICS;
    Publication, AlterPublicationStmt, PUBLICATION;
    Subscription, AlterSubscriptionStmt, SUBSCRIPTION;
    Conversion, AlterConversionStmt, CONVERSION;
    Server, AlterServerStmt, SERVER;
    Index, AlterIndexStmt, INDEX;
    View, AlterViewStmt, VIEW;
    Function, AlterFunctionStmt, FUNCTION;
    Language, AlterLanguageStmt, LANGUAGE;
    Database, AlterDatabaseStmt, DATABASE;
}

// Special multi-keyword DDL types

/// CREATE FOREIGN TABLE / DATA WRAPPER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterForeignStmt<'input> {
    pub alter: ALTER,
    pub foreign: FOREIGN,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE EVENT TRIGGER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateEventTriggerStmt<'input> {
    pub create: CREATE,
    pub event: EVENT,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP EVENT TRIGGER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropEventTriggerStmt<'input> {
    pub drop: DROP,
    pub event: EVENT,
    pub tail: Option<RawStatement<'input>>,
}

/// ALTER EVENT TRIGGER
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterEventTriggerStmt<'input> {
    pub alter: ALTER,
    pub event: EVENT,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP OWNED BY
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropOwnedStmt<'input> {
    pub drop: DROP,
    pub owned: OWNED,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE ACCESS METHOD
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateAccessMethodStmt<'input> {
    pub create: CREATE,
    pub access: ACCESS,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP ACCESS METHOD
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropAccessMethodStmt<'input> {
    pub drop: DROP,
    pub access: ACCESS,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE MATERIALIZED VIEW
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateMaterializedViewStmt<'input> {
    pub create: CREATE,
    pub materialized: MATERIALIZED,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP MATERIALIZED VIEW
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropMaterializedViewStmt<'input> {
    pub drop: DROP,
    pub materialized: MATERIALIZED,
    pub tail: Option<RawStatement<'input>>,
}

/// ALTER MATERIALIZED VIEW
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AlterMaterializedViewStmt<'input> {
    pub alter: ALTER,
    pub materialized: MATERIALIZED,
    pub tail: Option<RawStatement<'input>>,
}

/// CREATE FOREIGN ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateForeignStmt<'input> {
    pub create: CREATE,
    pub foreign: FOREIGN,
    pub tail: Option<RawStatement<'input>>,
}

/// DROP FOREIGN ...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropForeignStmt<'input> {
    pub drop: DROP,
    pub foreign: FOREIGN,
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
