/// Simple keyword-led statements that are recognized by their leading keyword(s)
/// but whose body is captured as raw text (not fully parsed).
///
/// Each struct has keyword PhantomData fields for disambiguation in the Statement
/// enum, followed by an optional RawStatement tail that captures any remaining
/// content before the semicolon.
use std::marker::PhantomData;

use recursa::{FormatTokens, Parse, Visit};

use crate::ast::RawStatement;
use crate::rules::SqlRules;
use crate::tokens::keyword;

// --- Transaction control ---

/// BEGIN [WORK | TRANSACTION] [isolation/read options...]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct BeginStmt {
    pub _begin: PhantomData<keyword::Begin>,
    pub tail: Option<RawStatement>,
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

/// ```sql
/// FETCH [direction] [FROM | IN] cursor
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FetchStmt {
    pub _fetch: PhantomData<keyword::Fetch>,
    pub tail: Option<RawStatement>,
}

/// CLOSE cursor | ALL
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CloseStmt {
    pub _close: PhantomData<keyword::Close>,
    pub tail: Option<RawStatement>,
}

/// ```sql
/// MOVE [direction] [FROM | IN] cursor
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MoveStmt {
    pub _move: PhantomData<keyword::Move>,
    pub tail: Option<RawStatement>,
}

// --- REINDEX ---

/// REINDEX [( options )] { INDEX | TABLE | SCHEMA | DATABASE | SYSTEM } name
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReindexStmt {
    pub _reindex: PhantomData<keyword::Reindex>,
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
