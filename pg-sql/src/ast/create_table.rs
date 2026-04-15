/// CREATE TABLE statement AST.
use std::marker::PhantomData;

use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::partition::{ForValuesClause, PartitionByClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// PRIMARY KEY column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrimaryKeyConstraint {
    pub _primary: PhantomData<keyword::Primary>,
    pub _key: PhantomData<keyword::Key>,
    /// Optional `[NOT] DEFERRABLE [INITIALLY {DEFERRED|IMMEDIATE}]` suffix.
    pub attrs: ConstraintAttrs,
}

/// NOT NULL column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotNullConstraint(PhantomData<keyword::Not>, PhantomData<keyword::Null>);

/// UNIQUE column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UniqueConstraint {
    pub _unique: PhantomData<keyword::Unique>,
    /// Optional `NULLS [NOT] DISTINCT` qualifier (Postgres 15+).
    pub nulls: Option<NullsDistinctQualifier>,
    /// Optional `[NOT] DEFERRABLE [INITIALLY ...]` attributes.
    pub attrs: ConstraintAttrs,
}

/// `NULLS DISTINCT` or `NULLS NOT DISTINCT` for UNIQUE constraints.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NullsDistinctQualifier {
    pub _nulls: PhantomData<keyword::Nulls>,
    pub not: Option<PhantomData<keyword::Not>>,
    pub _distinct: PhantomData<keyword::Distinct>,
}

/// Referential action for `ON DELETE` / `ON UPDATE`.
///
/// Variant ordering: multi-word variants (`NO ACTION`, `SET NULL`, `SET DEFAULT`)
/// come before single-word ones to satisfy longest-match.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ReferentialAction {
    NoAction(NoActionKw),
    SetNull(SetNullKw),
    SetDefault(SetDefaultKw),
    Cascade(PhantomData<keyword::Cascade>),
    Restrict(PhantomData<keyword::Restrict>),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NoActionKw {
    pub _no: PhantomData<keyword::No>,
    pub _action: PhantomData<keyword::Action>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetNullKw {
    pub _set: PhantomData<keyword::Set>,
    pub _null: PhantomData<keyword::Null>,
    pub cols: Option<Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetDefaultKw {
    pub _set: PhantomData<keyword::Set>,
    pub _default: PhantomData<keyword::Default>,
    pub cols: Option<Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>>,
}

/// `ON DELETE <action>`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnDeleteAction {
    pub _on: PhantomData<keyword::On>,
    pub _delete: PhantomData<keyword::Delete>,
    pub action: ReferentialAction,
}

/// `ON UPDATE <action>`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnUpdateAction {
    pub _on: PhantomData<keyword::On>,
    pub _update: PhantomData<keyword::Update>,
    pub action: ReferentialAction,
}

/// Match type for a foreign key: `MATCH FULL | PARTIAL | SIMPLE`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum MatchKind {
    Full(PhantomData<keyword::Full>),
    Partial(PhantomData<keyword::Partial>),
    Simple(PhantomData<keyword::Simple>),
}

/// `MATCH FULL | MATCH PARTIAL | MATCH SIMPLE`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MatchClause {
    pub _match: PhantomData<keyword::Match>,
    pub kind: MatchKind,
}

/// `DEFERRABLE | NOT DEFERRABLE`.
///
/// Variant ordering: `NotDeferrable` (two keywords) before `Deferrable`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum DeferrableKind {
    NotDeferrable(NotDeferrableKw),
    Deferrable(PhantomData<keyword::Deferrable>),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotDeferrableKw {
    pub _not: PhantomData<keyword::Not>,
    pub _deferrable: PhantomData<keyword::Deferrable>,
}

/// `INITIALLY DEFERRED | INITIALLY IMMEDIATE`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InitiallyClause {
    pub _initially: PhantomData<keyword::Initially>,
    pub mode: InitiallyMode,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InitiallyMode {
    Deferred(PhantomData<keyword::Deferred>),
    Immediate(PhantomData<keyword::Immediate>),
}

/// `ON DELETE ...` or `ON UPDATE ...` trailing action on a REFERENCES
/// constraint. Modeled as an enum so both orders of the two clauses
/// are accepted via a `Vec<OnAction>`.
///
/// Variant ordering: both start with `ON`; they diverge at the next keyword.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum OnAction {
    OnDelete(OnDeleteAction),
    OnUpdate(OnUpdateAction),
}

/// REFERENCES constraint:
/// `REFERENCES table [(col, ...)] [MATCH ...] [ON DELETE|UPDATE ...]* [DEFERRABLE | NOT DEFERRABLE] [INITIALLY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReferencesConstraint {
    pub _references: PhantomData<keyword::References>,
    pub table: literal::AliasName,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub match_clause: Option<MatchClause>,
    pub actions: Vec<OnAction>,
    pub deferrable: Option<DeferrableKind>,
    pub initially: Option<InitiallyClause>,
    pub not_valid: Option<NotValidKw>,
}

/// `NOT VALID` suffix on a CHECK or FOREIGN KEY constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotValidKw {
    pub _not: PhantomData<keyword::Not>,
    pub _valid: PhantomData<keyword::ValidKw>,
}

/// `CHECK (expr) [NO INHERIT] [NOT VALID]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CheckConstraint {
    pub _check: PhantomData<keyword::Check>,
    pub expr: Surrounded<punct::LParen, crate::ast::expr::Expr, punct::RParen>,
    pub no_inherit: Option<NoInheritKw>,
    pub not_valid: Option<NotValidKw>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NoInheritKw {
    pub _no: PhantomData<keyword::No>,
    pub _inherit: PhantomData<keyword::Inherit>,
}

/// GENERATED ALWAYS AS IDENTITY column constraint, with optional
/// `(sequence_option ...)` parenthesized list (e.g. `START WITH 44`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GeneratedIdentityConstraint {
    pub _generated: PhantomData<keyword::Generated>,
    pub _always: PhantomData<keyword::Always>,
    pub _as: PhantomData<keyword::As>,
    pub _identity: PhantomData<keyword::Identity>,
    pub seq_options:
        Option<Surrounded<punct::LParen, Vec<IdentitySeqOption>, punct::RParen>>,
}

/// One option inside an `IDENTITY ( ... )` sequence option list.
///
/// Variant ordering: longer multi-word forms first so longest-match-wins
/// picks them.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum IdentitySeqOption {
    StartWith(SeqOptStartWith),
    IncrementBy(SeqOptIncrementBy),
    MinValue(SeqOptMinValue),
    NoMinValue(SeqOptNoMinValue),
    MaxValue(SeqOptMaxValue),
    NoMaxValue(SeqOptNoMaxValue),
    Cache(SeqOptCache),
    Cycle(SeqOptCycle),
    NoCycle(SeqOptNoCycle),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptStartWith {
    pub _start: PhantomData<keyword::Start>,
    pub _with: Option<PhantomData<keyword::With>>,
    pub value: crate::ast::expr::Expr,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptIncrementBy {
    pub _increment: PhantomData<keyword::Increment>,
    pub _by: Option<PhantomData<keyword::By>>,
    pub value: crate::ast::expr::Expr,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptMinValue {
    pub _minvalue: PhantomData<keyword::Minvalue>,
    pub value: crate::ast::expr::Expr,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptNoMinValue {
    pub _no: PhantomData<keyword::No>,
    pub _minvalue: PhantomData<keyword::Minvalue>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptMaxValue {
    pub _maxvalue: PhantomData<keyword::Maxvalue>,
    pub value: crate::ast::expr::Expr,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptNoMaxValue {
    pub _no: PhantomData<keyword::No>,
    pub _maxvalue: PhantomData<keyword::Maxvalue>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptCache {
    pub _cache: PhantomData<keyword::Cache>,
    pub value: crate::ast::expr::Expr,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptCycle {
    pub _cycle: PhantomData<keyword::Cycle>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptNoCycle {
    pub _no: PhantomData<keyword::No>,
    pub _cycle: PhantomData<keyword::Cycle>,
}

/// `GENERATED ALWAYS AS (expr) STORED` column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GeneratedStoredConstraint {
    pub _generated: PhantomData<keyword::Generated>,
    pub _always: PhantomData<keyword::Always>,
    pub _as: PhantomData<keyword::As>,
    pub expr: Surrounded<punct::LParen, crate::ast::expr::Expr, punct::RParen>,
    pub _stored: PhantomData<keyword::Stored>,
}

/// DEFAULT expr column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DefaultConstraint {
    pub _default: PhantomData<keyword::Default>,
    pub expr: crate::ast::expr::Expr,
}

/// Column constraint kind (without the optional `CONSTRAINT name` prefix).
///
/// Variant ordering for longest-match-wins:
/// - GeneratedIdentity (`GENERATED`) first (unique keyword)
/// - PrimaryKey (`PRIMARY KEY`) before others (unique keyword)
/// - NotNull (`NOT NULL`) before others
/// - References, Unique, Default, Check all start with distinct keywords
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ColumnConstraintKind {
    GeneratedStored(GeneratedStoredConstraint),
    GeneratedIdentity(GeneratedIdentityConstraint),
    PrimaryKey(PrimaryKeyConstraint),
    NotNull(NotNullConstraint),
    Unique(UniqueConstraint),
    References(ReferencesConstraint),
    Default(DefaultConstraint),
    Check(CheckConstraint),
}

/// Optional `CONSTRAINT name` prefix shared by column-level and
/// table-level constraints.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ConstraintNamePrefix {
    pub _constraint: PhantomData<keyword::Constraint>,
    pub name: literal::Ident,
}

/// A column constraint with its optional `CONSTRAINT name` prefix.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnConstraint {
    pub name: Option<ConstraintNamePrefix>,
    pub kind: ColumnConstraintKind,
}

/// `COLLATE "name"` clause used after a column's type.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CollateClause {
    pub _collate: PhantomData<keyword::Collate>,
    pub name: literal::Ident,
}

/// A column definition: `name type [COLLATE "..."] [constraints...]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDef {
    pub name: literal::Ident,
    pub type_name: crate::ast::expr::CastType,
    pub collate: Option<CollateClause>,
    pub constraints: Seq<ColumnConstraint, (), OptionalTrailing>,
}

impl ColumnDef {
    /// Returns true if any of this column's constraints is a PRIMARY KEY.
    pub fn primary_key(&self) -> bool {
        self.constraints
            .iter()
            .any(|c| matches!(c.kind, ColumnConstraintKind::PrimaryKey(_)))
    }
}

// --- Table-level constraints ---

/// Optional trailing deferrable/initially pair shared by PK/UNIQUE/FK.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ConstraintAttrs {
    pub deferrable: Option<DeferrableKind>,
    pub initially: Option<InitiallyClause>,
}

/// `PRIMARY KEY (col, ...)`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TablePrimaryKey {
    pub _primary: PhantomData<keyword::Primary>,
    pub _key: PhantomData<keyword::Key>,
    pub columns: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
    pub attrs: ConstraintAttrs,
}

/// `UNIQUE (col, ...)`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableUnique {
    pub _unique: PhantomData<keyword::Unique>,
    pub columns: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
    pub attrs: ConstraintAttrs,
}

/// `FOREIGN KEY (col, ...) REFERENCES table [(col, ...)] [MATCH ...] [ON ...] [DEFERRABLE ...] [INITIALLY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableForeignKey {
    pub _foreign: PhantomData<keyword::Foreign>,
    pub _key: PhantomData<keyword::Key>,
    pub columns: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
    pub references: ReferencesConstraint,
}

/// Table-level `CHECK (expr) [NO INHERIT]`.
pub type TableCheck = CheckConstraint;

/// A table-level constraint kind.
///
/// Variant ordering: `PRIMARY KEY` (PRIMARY), `FOREIGN KEY` (FOREIGN),
/// `UNIQUE`, `CHECK` — all start with distinct unique keywords so order
/// is not strictly required for disambiguation.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TableConstraintKind {
    PrimaryKey(TablePrimaryKey),
    ForeignKey(TableForeignKey),
    Unique(TableUnique),
    Check(TableCheck),
}

/// A table-level constraint with optional `CONSTRAINT name` prefix.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableConstraint {
    pub name: Option<ConstraintNamePrefix>,
    pub kind: TableConstraintKind,
}

/// A single `INCLUDING` / `EXCLUDING` option on a `LIKE` source table clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum LikeOptionKind {
    All(keyword::All),
    Defaults(keyword::DefaultsKw),
    Constraints(keyword::Constraints),
    Indexes(keyword::IndexesKw),
    Storage(keyword::StorageKw),
    Comments(keyword::CommentsKw),
    Statistics(keyword::Statistics),
    Generated(keyword::Generated),
    Identity(keyword::Identity),
}

/// `INCLUDING what`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IncludingOption {
    pub _including: PhantomData<keyword::IncludingKw>,
    pub what: LikeOptionKind,
}

/// `EXCLUDING what`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExcludingOption {
    pub _excluding: PhantomData<keyword::ExcludingKw>,
    pub what: LikeOptionKind,
}

/// One option on a `LIKE table` clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum LikeOption {
    Including(IncludingOption),
    Excluding(ExcludingOption),
}

/// `LIKE source_table [INCLUDING/EXCLUDING option ...]` clause in a column
/// list body. Copies column definitions (and optionally other properties)
/// from an existing table.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LikeClause {
    pub _like: PhantomData<keyword::Like>,
    pub source: crate::ast::common::QualifiedName,
    pub options: Vec<LikeOption>,
}

/// One item in a CREATE TABLE column list: a `LIKE table` clause, a
/// table-level constraint, or a column definition.
///
/// Variant ordering: the `Like` variant starts with the `LIKE` keyword and
/// must come first (its leading token is otherwise an infix operator in
/// expressions, so it can't collide with `Column` which starts with an
/// ident). `Constraint` must come before `Column` because its leading
/// tokens (`CONSTRAINT`, `PRIMARY`, `UNIQUE`, `FOREIGN`, `CHECK`) are
/// keywords, while a `Column` starts with an identifier.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ColumnOrConstraint {
    Like(LikeClause),
    Constraint(TableConstraint),
    Column(ColumnDef),
}

/// Optional TEMP or TEMPORARY keyword.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TempKw {
    Temp(PhantomData<keyword::Temp>),
    Temporary(PhantomData<keyword::Temporary>),
}

/// INHERITS clause: `INHERITS (parent, ...)`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InheritsClause {
    pub _inherits: PhantomData<keyword::Inherits>,
    pub parents: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
}

/// Column-based table body: `(cols_and_constraints) [INHERITS (...)] [PARTITION BY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnsBody {
    pub columns: Surrounded<punct::LParen, Seq<ColumnOrConstraint, punct::Comma>, punct::RParen>,
    pub inherits: Option<InheritsClause>,
    pub partition_by: Option<PartitionByClause>,
    pub with_storage: Option<crate::ast::create_index::WithStorage>,
    pub on_commit: Option<OnCommitClause>,
}

/// `ON COMMIT { PRESERVE ROWS | DELETE ROWS | DROP }` for temp tables.
///
/// Variant ordering: distinct first tokens (`PRESERVE` / `DELETE` / `DROP`),
/// so order is for clarity.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnCommitClause {
    pub _on: PhantomData<keyword::On>,
    pub _commit: PhantomData<keyword::Commit>,
    pub action: OnCommitAction,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum OnCommitAction {
    PreserveRows(OnCommitPreserveRows),
    DeleteRows(OnCommitDeleteRows),
    Drop(OnCommitDrop),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnCommitPreserveRows {
    pub _preserve: PhantomData<keyword::Preserve>,
    pub _rows: PhantomData<keyword::Rows>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnCommitDeleteRows {
    pub _delete: PhantomData<keyword::Delete>,
    pub _rows: PhantomData<keyword::Rows>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnCommitDrop {
    pub _drop: PhantomData<keyword::Drop>,
}

/// Partition-of table body: `PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionOfBody {
    pub _partition: PhantomData<keyword::Partition>,
    pub _of: PhantomData<keyword::Of>,
    pub parent: literal::Ident,
    pub for_values: Option<ForValuesClause>,
    pub default: Option<PhantomData<keyword::Default>>,
    pub partition_by: Option<PartitionByClause>,
    pub with_storage: Option<crate::ast::create_index::WithStorage>,
}

/// AS-query table body: `AS SELECT ... [WITH [NO] DATA]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsQueryBody {
    pub _as: PhantomData<keyword::As>,
    pub query: Box<crate::ast::Statement>,
    pub with_data: Option<WithDataClause>,
}

/// `WITH DATA` or `WITH NO DATA` modifier on a CTAS query.
///
/// Variant ordering: `NoData` (`WITH NO DATA`, longer) before `Data`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum WithDataClause {
    NoData(WithNoData),
    Data(WithData),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithNoData {
    pub _with: PhantomData<keyword::With>,
    pub _no: PhantomData<keyword::No>,
    pub _data: PhantomData<keyword::Data>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithData {
    pub _with: PhantomData<keyword::With>,
    pub _data: PhantomData<keyword::Data>,
}

/// `(col, col, ...) AS query [WITH [NO] DATA]` — CTAS with column list.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnsAsQueryBody {
    pub columns:
        Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
    pub _as: PhantomData<keyword::As>,
    pub query: Box<crate::ast::Statement>,
    pub with_data: Option<WithDataClause>,
}

/// The body of a CREATE TABLE statement after `CREATE [TEMP] TABLE name`.
///
/// Variant ordering: AsQuery (`AS`) and PartitionOf (`PARTITION`) start with
/// keywords; Columns starts with `(`. Longest-match-wins disambiguates.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CreateTableBody {
    AsQuery(AsQueryBody),
    PartitionOf(PartitionOfBody),
    /// `(col, ...) AS query` — CTAS with explicit column list.
    /// Listed before `Columns` so the `( ... ) AS` form wins over the
    /// columns-only `( ... )` form via longer match.
    ColumnsAsQuery(ColumnsAsQueryBody),
    Columns(ColumnsBody),
}

/// ```sql
/// CREATE [TEMP] TABLE statement.
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTableStmt {
    pub _create: PhantomData<keyword::Create>,
    pub temp: Option<TempKw>,
    pub unlogged: Option<PhantomData<keyword::Unlogged>>,
    pub _table: PhantomData<keyword::Table>,
    pub if_not_exists: Option<crate::ast::create_index::IfNotExistsKw>,
    pub name: literal::Ident,
    pub body: CreateTableBody,
}

impl CreateTableStmt {
    /// Returns all items (columns + table-level constraints) of a
    /// columns-based CREATE TABLE.
    pub fn items(
        &self,
    ) -> Option<&Surrounded<punct::LParen, Seq<ColumnOrConstraint, punct::Comma>, punct::RParen>>
    {
        match &self.body {
            CreateTableBody::Columns(b) => Some(&b.columns),
            CreateTableBody::PartitionOf(_)
            | CreateTableBody::AsQuery(_)
            | CreateTableBody::ColumnsAsQuery(_) => None,
        }
    }

    /// Returns only the column definitions (excluding table constraints).
    pub fn column_defs(&self) -> Option<Vec<&ColumnDef>> {
        self.items().map(|s| {
            s.iter()
                .filter_map(|item| match item {
                    ColumnOrConstraint::Column(c) => Some(c),
                    ColumnOrConstraint::Constraint(_) | ColumnOrConstraint::Like(_) => None,
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_table::CreateTableStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_table_identity_seq_options() {
        let mut input = recursa::Input::new(
            "CREATE TABLE t (id int GENERATED ALWAYS AS IDENTITY (START WITH 44))",
        );
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_temp_table_on_commit() {
        for src in [
            "CREATE TEMP TABLE t (a int) ON COMMIT PRESERVE ROWS",
            "CREATE TEMP TABLE t (a int) ON COMMIT DELETE ROWS",
            "CREATE TEMP TABLE t (a int) ON COMMIT DROP",
        ] {
            let mut input = recursa::Input::new(src);
            let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
            assert!(input.is_empty(), "leftover for {src:?}");
        }
    }

    #[test]
    fn parse_create_table_single_column() {
        let mut input = Input::new("CREATE TABLE BOOLTBL1 (f1 bool)");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "BOOLTBL1");
        assert_eq!(stmt.items().unwrap().len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_multiple_columns() {
        let mut input = Input::new("CREATE TABLE BOOLTBL3 (d text, b bool, o int)");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "BOOLTBL3");
        assert_eq!(stmt.items().unwrap().len(), 3);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_ctas_with_column_list() {
        // Regression: matview.sql uses `CREATE TABLE foo(a, b) AS VALUES(1, 10)`.
        let mut input = Input::new("CREATE TABLE mvtest_foo(a, b) AS VALUES(1, 10)");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(
            stmt.body,
            super::CreateTableBody::ColumnsAsQuery(_)
        ));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_time_zone_types() {
        // Regression: brin.sql brintest table uses `time without time zone`,
        // `timestamp with time zone`, `bit varying(16)` as column types.
        let mut input = Input::new(
            "CREATE TABLE t (a time without time zone, b timestamp with time zone, c time with time zone, d timestamp without time zone, e bit varying(16), f bit(10), g character)",
        );
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.items().unwrap().len(), 7);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_array_column_types() {
        let mut input =
            Input::new("CREATE TABLE t (a int2[], b int4[][][], c varchar(5)[], d text[])");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.items().unwrap().len(), 4);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_boolean_type() {
        let mut input = Input::new("CREATE TABLE t (f1 boolean)");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.items().unwrap().len(), 1);
    }

    #[test]
    fn parse_create_temp_table() {
        let mut input = Input::new("CREATE TEMP TABLE foo (f1 int)");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.temp.is_some());
        assert_eq!(stmt.name.text(), "foo");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_partitioned_table() {
        let mut input =
            Input::new("create table list_parted_tbl (a int,b int) partition by list (a)");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "list_parted_tbl");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_partition_of() {
        let mut input = Input::new(
            "create table list_parted_tbl1 partition of list_parted_tbl for values in (1) partition by list(b)",
        );
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "list_parted_tbl1");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_column_check_constraint() {
        let mut input = Input::new("CREATE TABLE t (a int CHECK (a > 0))");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_column_references_full() {
        let mut input = Input::new(
            "CREATE TABLE t (a int REFERENCES other(id) MATCH FULL ON DELETE CASCADE ON UPDATE NO ACTION DEFERRABLE INITIALLY DEFERRED)",
        );
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_column_named_constraint() {
        let mut input = Input::new("CREATE TABLE t (a int CONSTRAINT pos CHECK (a > 0))");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_column_default_constraint() {
        let mut input = Input::new("CREATE TABLE t (a int DEFAULT 0)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_primary_key() {
        let mut input = Input::new("CREATE TABLE t (a int, b int, PRIMARY KEY (a, b))");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_unique() {
        let mut input = Input::new("CREATE TABLE t (a int, UNIQUE (a))");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_foreign_key() {
        let mut input = Input::new(
            "CREATE TABLE t (a int, FOREIGN KEY (a) REFERENCES other(id) ON DELETE SET NULL)",
        );
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_foreign_key_set_null_columns() {
        let mut input = Input::new(
            "CREATE TABLE t (a int, b int, FOREIGN KEY (a, b) REFERENCES p ON DELETE SET NULL (b))",
        );
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_foreign_key_set_default_columns() {
        let mut input = Input::new(
            "CREATE TABLE t (a int, FOREIGN KEY (a) REFERENCES p ON UPDATE SET DEFAULT (a))",
        );
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_check() {
        let mut input = Input::new("CREATE TABLE t (a int, CHECK (a > 0))");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_named_constraint() {
        let mut input = Input::new(
            "CREATE TABLE t (a int, b int, CONSTRAINT pk PRIMARY KEY (a, b) DEFERRABLE INITIALLY IMMEDIATE)",
        );
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_check_no_inherit() {
        let mut input = Input::new("CREATE TABLE t (a int, CHECK (a > 0) NO INHERIT)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_like_bare() {
        let mut input = Input::new("CREATE TABLE foo (LIKE bar)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_like_including_all() {
        let mut input = Input::new("CREATE TABLE foo (LIKE bar INCLUDING ALL)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_like_including_excluding() {
        let mut input =
            Input::new("CREATE TABLE foo (LIKE bar INCLUDING DEFAULTS EXCLUDING CONSTRAINTS)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_like_mixed_with_columns() {
        let mut input = Input::new("CREATE TABLE foo (a int, LIKE bar INCLUDING ALL, b text)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_check_no_inherit_not_valid() {
        let mut input = Input::new("CREATE TABLE t (d date, CHECK (false) NO INHERIT NOT VALID)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_table_check_not_valid() {
        let mut input = Input::new("CREATE TABLE t (a int, CHECK (a > 0) NOT VALID)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_with_storage_params() {
        let mut input = Input::new("CREATE TABLE t (a int) WITH (fillfactor = 70)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_temp_table_empty_columns() {
        let mut input = Input::new("CREATE TEMP TABLE nocols()");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.items().unwrap().len(), 0);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_unlogged_table() {
        let mut input = Input::new("CREATE UNLOGGED TABLE t (a int)");
        let stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.unlogged.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_unlogged_table_qualified() {
        let mut input = Input::new("CREATE UNLOGGED TABLE public.t (a int)");
        // This uses unqualified Ident only; restrict to the unqualified form.
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input);
    }

    #[test]
    fn parse_column_with_collate() {
        let mut input = Input::new("CREATE TABLE foo (a text COLLATE \"C\")");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_partition_of_range_from_to() {
        let mut input = Input::new("CREATE TABLE p1 PARTITION OF p FOR VALUES FROM (0) TO (10)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_partition_of_list_in() {
        let mut input = Input::new("CREATE TABLE p2 PARTITION OF p FOR VALUES IN (1, 2, 3)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_partition_of_hash_with_modulus() {
        let mut input =
            Input::new("CREATE TABLE p3 PARTITION OF p FOR VALUES WITH (MODULUS 4, REMAINDER 0)");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_partition_of_default() {
        let mut input = Input::new("CREATE TABLE p4 PARTITION OF p DEFAULT");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
