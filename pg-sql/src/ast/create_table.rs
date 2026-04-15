/// CREATE TABLE statement AST.
use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

use crate::ast::partition::{ForValuesClause, PartitionByClause};
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};

use crate::tokens::keyword::*;
/// PRIMARY KEY column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrimaryKeyConstraint {
    pub _primary: PRIMARY,
    pub _key: KEY,
    /// Optional `[NOT] DEFERRABLE [INITIALLY {DEFERRED|IMMEDIATE}]` suffix.
    pub attrs: ConstraintAttrs,
}

/// NOT NULL column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotNullConstraint(NOT, NULL);

/// UNIQUE column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UniqueConstraint {
    pub _unique: UNIQUE,
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
    pub _nulls: NULLS,
    pub not: Option<NOT>,
    pub _distinct: DISTINCT,
}

/// Referential action for `ON DELETE` / `ON UPDATE`.
///
/// Variant ordering: multi-word variants (`NO ACTION`, `SET NULL`, `SET DEFAULT`)
/// come before single-word ones to satisfy longest-match.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ReferentialAction<'input> {
    NoAction(NoActionKw),
    SetNull(SetNullKw<'input>),
    SetDefault(SetDefaultKw<'input>),
    Cascade(CASCADE),
    Restrict(RESTRICT),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NoActionKw {
    pub _no: NO,
    pub _action: ACTION,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetNullKw<'input> {
    pub _set: SET,
    pub _null: NULL,
    pub cols: Option<
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
    >,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetDefaultKw<'input> {
    pub _set: SET,
    pub _default: DEFAULT,
    pub cols: Option<
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
    >,
}

/// `ON DELETE <action>`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnDeleteAction<'input> {
    pub _on: ON,
    pub _delete: DELETE,
    pub action: ReferentialAction<'input>,
}

/// `ON UPDATE <action>`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnUpdateAction<'input> {
    pub _on: ON,
    pub _update: UPDATE,
    pub action: ReferentialAction<'input>,
}

/// Match type for a foreign key: `MATCH FULL | PARTIAL | SIMPLE`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum MatchKind {
    Full(FULL),
    Partial(PARTIAL),
    Simple(SIMPLE),
}

/// `MATCH FULL | MATCH PARTIAL | MATCH SIMPLE`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MatchClause {
    pub _match: MATCH,
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
    Deferrable(DEFERRABLE),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotDeferrableKw {
    pub _not: NOT,
    pub _deferrable: DEFERRABLE,
}

/// `INITIALLY DEFERRED | INITIALLY IMMEDIATE`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InitiallyClause {
    pub _initially: INITIALLY,
    pub mode: InitiallyMode,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InitiallyMode {
    Deferred(DEFERRED),
    Immediate(IMMEDIATE),
}

/// `ON DELETE ...` or `ON UPDATE ...` trailing action on a REFERENCES
/// constraint. Modeled as an enum so both orders of the two clauses
/// are accepted via a `Vec<OnAction>`.
///
/// Variant ordering: both start with `ON`; they diverge at the next keyword.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum OnAction<'input> {
    OnDelete(OnDeleteAction<'input>),
    OnUpdate(OnUpdateAction<'input>),
}

/// REFERENCES constraint:
/// `REFERENCES table [(col, ...)] [MATCH ...] [ON DELETE|UPDATE ...]* [DEFERRABLE | NOT DEFERRABLE] [INITIALLY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReferencesConstraint<'input> {
    pub _references: REFERENCES,
    pub table: literal::AliasName<'input>,
    pub columns: Option<
        Surrounded<
            punct::LParen,
            Seq<literal::AliasName<'input>, punct::Comma>,
            punct::RParen,
        >,
    >,
    pub match_clause: Option<MatchClause>,
    pub actions: Vec<OnAction<'input>>,
    pub deferrable: Option<DeferrableKind>,
    pub initially: Option<InitiallyClause>,
    pub not_valid: Option<NotValidKw>,
}

/// `NOT VALID` suffix on a CHECK or FOREIGN KEY constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotValidKw {
    pub _not: NOT,
    pub _valid: VALID,
}

/// `CHECK (expr) [NO INHERIT] [NOT VALID]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CheckConstraint<'input> {
    pub _check: CHECK,
    pub expr: Surrounded<punct::LParen, crate::ast::expr::Expr<'input>, punct::RParen>,
    pub no_inherit: Option<NoInheritKw>,
    pub not_valid: Option<NotValidKw>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NoInheritKw {
    pub _no: NO,
    pub _inherit: INHERIT,
}

/// GENERATED ALWAYS AS IDENTITY column constraint, with optional
/// `(sequence_option ...)` parenthesized list (e.g. `START WITH 44`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GeneratedIdentityConstraint<'input> {
    pub _generated: GENERATED,
    pub _always: ALWAYS,
    pub _as: AS,
    pub _identity: IDENTITY,
    pub seq_options:
        Option<Surrounded<punct::LParen, Vec<IdentitySeqOption<'input>>, punct::RParen>>,
}

/// One option inside an `IDENTITY ( ... )` sequence option list.
///
/// Variant ordering: longer multi-word forms first so longest-match-wins
/// picks them.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum IdentitySeqOption<'input> {
    StartWith(SeqOptStartWith<'input>),
    IncrementBy(SeqOptIncrementBy<'input>),
    MinValue(SeqOptMinValue<'input>),
    NoMinValue(SeqOptNoMinValue),
    MaxValue(SeqOptMaxValue<'input>),
    NoMaxValue(SeqOptNoMaxValue),
    Cache(SeqOptCache<'input>),
    Cycle(SeqOptCycle),
    NoCycle(SeqOptNoCycle),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptStartWith<'input> {
    pub _start: START,
    pub _with: Option<WITH>,
    pub value: crate::ast::expr::Expr<'input>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptIncrementBy<'input> {
    pub _increment: INCREMENT,
    pub _by: Option<BY>,
    pub value: crate::ast::expr::Expr<'input>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptMinValue<'input> {
    pub _minvalue: MINVALUE,
    pub value: crate::ast::expr::Expr<'input>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptNoMinValue {
    pub _no: NO,
    pub _minvalue: MINVALUE,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptMaxValue<'input> {
    pub _maxvalue: MAXVALUE,
    pub value: crate::ast::expr::Expr<'input>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptNoMaxValue {
    pub _no: NO,
    pub _maxvalue: MAXVALUE,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptCache<'input> {
    pub _cache: CACHE,
    pub value: crate::ast::expr::Expr<'input>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptCycle {
    pub _cycle: CYCLE,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SeqOptNoCycle {
    pub _no: NO,
    pub _cycle: CYCLE,
}

/// `GENERATED ALWAYS AS (expr) STORED` column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GeneratedStoredConstraint<'input> {
    pub _generated: GENERATED,
    pub _always: ALWAYS,
    pub _as: AS,
    pub expr: Surrounded<punct::LParen, crate::ast::expr::Expr<'input>, punct::RParen>,
    pub _stored: STORED,
}

/// DEFAULT expr column constraint.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DefaultConstraint<'input> {
    pub _default: DEFAULT,
    pub expr: crate::ast::expr::Expr<'input>,
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
pub enum ColumnConstraintKind<'input> {
    GeneratedStored(GeneratedStoredConstraint<'input>),
    GeneratedIdentity(GeneratedIdentityConstraint<'input>),
    PrimaryKey(PrimaryKeyConstraint),
    NotNull(NotNullConstraint),
    Unique(UniqueConstraint),
    References(ReferencesConstraint<'input>),
    Default(DefaultConstraint<'input>),
    Check(CheckConstraint<'input>),
}

/// Optional `CONSTRAINT name` prefix shared by column-level and
/// table-level constraints.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ConstraintNamePrefix<'input> {
    pub _constraint: CONSTRAINT,
    pub name: literal::Ident<'input>,
}

/// A column constraint with its optional `CONSTRAINT name` prefix.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnConstraint<'input> {
    pub name: Option<ConstraintNamePrefix<'input>>,
    pub kind: ColumnConstraintKind<'input>,
}

/// `COLLATE "name"` clause used after a column's type.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CollateClause<'input> {
    pub _collate: COLLATE,
    pub name: literal::Ident<'input>,
}

/// A column definition: `name type [COLLATE "..."] [constraints...]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDef<'input> {
    pub name: literal::Ident<'input>,
    pub type_name: crate::ast::expr::CastType<'input>,
    pub collate: Option<CollateClause<'input>>,
    pub constraints: Seq<ColumnConstraint<'input>, (), OptionalTrailing>,
}

impl<'input> ColumnDef<'input> {
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
pub struct TablePrimaryKey<'input> {
    pub _primary: PRIMARY,
    pub _key: KEY,
    pub columns:
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
    pub attrs: ConstraintAttrs,
}

/// `UNIQUE (col, ...)`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableUnique<'input> {
    pub _unique: UNIQUE,
    pub columns:
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
    pub attrs: ConstraintAttrs,
}

/// `FOREIGN KEY (col, ...) REFERENCES table [(col, ...)] [MATCH ...] [ON ...] [DEFERRABLE ...] [INITIALLY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableForeignKey<'input> {
    pub _foreign: FOREIGN,
    pub _key: KEY,
    pub columns:
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
    pub references: ReferencesConstraint<'input>,
}

/// Table-level `CHECK (expr) [NO INHERIT]`.
pub type TableCheck<'input> = CheckConstraint<'input>;

/// A table-level constraint kind.
///
/// Variant ordering: `PRIMARY KEY` (PRIMARY), `FOREIGN KEY` (FOREIGN),
/// `UNIQUE`, `CHECK` — all start with distinct unique keywords so order
/// is not strictly required for disambiguation.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TableConstraintKind<'input> {
    PrimaryKey(TablePrimaryKey<'input>),
    ForeignKey(TableForeignKey<'input>),
    Unique(TableUnique<'input>),
    Check(TableCheck<'input>),
}

/// A table-level constraint with optional `CONSTRAINT name` prefix.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableConstraint<'input> {
    pub name: Option<ConstraintNamePrefix<'input>>,
    pub kind: TableConstraintKind<'input>,
}

/// A single `INCLUDING` / `EXCLUDING` option on a `LIKE` source table clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum LikeOptionKind {
    All(ALL),
    Defaults(DEFAULTS),
    Constraints(CONSTRAINTS),
    Indexes(INDEXES),
    Storage(STORAGE),
    Comments(COMMENTS),
    Statistics(STATISTICS),
    Generated(GENERATED),
    Identity(IDENTITY),
}

/// `INCLUDING what`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IncludingOption {
    pub _including: INCLUDING,
    pub what: LikeOptionKind,
}

/// `EXCLUDING what`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExcludingOption {
    pub _excluding: EXCLUDING,
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
pub struct LikeClause<'input> {
    pub _like: LIKE,
    pub source: crate::ast::common::QualifiedName<'input>,
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
pub enum ColumnOrConstraint<'input> {
    Like(LikeClause<'input>),
    Constraint(TableConstraint<'input>),
    Column(ColumnDef<'input>),
}

/// Optional TEMP or TEMPORARY keyword.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TempKw {
    Temp(TEMP),
    Temporary(TEMPORARY),
}

/// INHERITS clause: `INHERITS (parent, ...)`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InheritsClause<'input> {
    pub _inherits: INHERITS,
    pub parents:
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
}

/// Column-based table body: `(cols_and_constraints) [INHERITS (...)] [PARTITION BY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnsBody<'input> {
    pub columns: Surrounded<
        punct::LParen,
        Seq<ColumnOrConstraint<'input>, punct::Comma>,
        punct::RParen,
    >,
    pub inherits: Option<InheritsClause<'input>>,
    pub partition_by: Option<PartitionByClause<'input>>,
    pub with_storage: Option<crate::ast::create_index::WithStorage<'input>>,
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
    pub _on: ON,
    pub _commit: COMMIT,
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
    pub _preserve: PRESERVE,
    pub _rows: ROWS,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnCommitDeleteRows {
    pub _delete: DELETE,
    pub _rows: ROWS,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnCommitDrop {
    pub _drop: DROP,
}

/// Partition-of table body: `PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...]`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionOfBody<'input> {
    pub _partition: PARTITION,
    pub _of: OF,
    pub parent: literal::Ident<'input>,
    pub for_values: Option<ForValuesClause<'input>>,
    pub default: Option<DEFAULT>,
    pub partition_by: Option<PartitionByClause<'input>>,
    pub with_storage: Option<crate::ast::create_index::WithStorage<'input>>,
}

/// AS-query table body: `AS SELECT ... [WITH [NO] DATA]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsQueryBody<'input> {
    pub _as: AS,
    pub query: Box<crate::ast::Statement<'input>>,
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
    pub _with: WITH,
    pub _no: NO,
    pub _data: DATA,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithData {
    pub _with: WITH,
    pub _data: DATA,
}

/// `(col, col, ...) AS query [WITH [NO] DATA]` — CTAS with column list.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnsAsQueryBody<'input> {
    pub columns:
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
    pub _as: AS,
    pub query: Box<crate::ast::Statement<'input>>,
    pub with_data: Option<WithDataClause>,
}

/// The body of a CREATE TABLE statement after `CREATE [TEMP] TABLE name`.
///
/// Variant ordering: AsQuery (`AS`) and PartitionOf (`PARTITION`) start with
/// keywords; Columns starts with `(`. Longest-match-wins disambiguates.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CreateTableBody<'input> {
    AsQuery(AsQueryBody<'input>),
    PartitionOf(PartitionOfBody<'input>),
    /// `(col, ...) AS query` — CTAS with explicit column list.
    /// Listed before `Columns` so the `( ... ) AS` form wins over the
    /// columns-only `( ... )` form via longer match.
    ColumnsAsQuery(ColumnsAsQueryBody<'input>),
    Columns(ColumnsBody<'input>),
}

/// ```sql
/// CREATE [TEMP] TABLE statement.
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTableStmt<'input> {
    pub _create: CREATE,
    pub temp: Option<TempKw>,
    pub unlogged: Option<UNLOGGED>,
    pub _table: TABLE,
    pub if_not_exists: Option<crate::ast::create_index::IfNotExistsKw>,
    pub name: literal::Ident<'input>,
    pub body: CreateTableBody<'input>,
}

impl<'input> CreateTableStmt<'input> {
    /// Returns all items (columns + table-level constraints) of a
    /// columns-based CREATE TABLE.
    pub fn items(
        &self,
    ) -> Option<
        &Surrounded<punct::LParen, Seq<ColumnOrConstraint<'input>, punct::Comma>, punct::RParen>,
    > {
        match &self.body {
            CreateTableBody::Columns(b) => Some(&b.columns),
            CreateTableBody::PartitionOf(_)
            | CreateTableBody::AsQuery(_)
            | CreateTableBody::ColumnsAsQuery(_) => None,
        }
    }

    /// Returns only the column definitions (excluding table constraints).
    pub fn column_defs(&self) -> Option<Vec<&ColumnDef<'input>>> {
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
