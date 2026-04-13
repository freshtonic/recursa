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

/// ROLLBACK [WORK | TRANSACTION] [TO [SAVEPOINT] name]
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

/// RELEASE [SAVEPOINT] name
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

/// DEALLOCATE [PREPARE] name | ALL
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

/// TRUNCATE [TABLE] name [, ...] [CASCADE | RESTRICT]
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

/// LOCK [TABLE] name [, ...] [IN mode MODE] [NOWAIT]
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

/// FETCH [direction] [FROM | IN] cursor
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

/// MOVE [direction] [FROM | IN] cursor
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

/// REFRESH MATERIALIZED VIEW [CONCURRENTLY] name [WITH [NO] DATA]
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

/// CLUSTER [VERBOSE] [table [USING index]]
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
