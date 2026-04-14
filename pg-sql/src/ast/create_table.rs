/// CREATE TABLE statement AST.
use std::marker::PhantomData;

use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::partition::{ForValuesInClause, PartitionByClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// PRIMARY KEY column constraint.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrimaryKeyConstraint(PhantomData<keyword::Primary>, PhantomData<keyword::Key>);

/// NOT NULL column constraint.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotNullConstraint(PhantomData<keyword::Not>, PhantomData<keyword::Null>);

/// UNIQUE column constraint.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UniqueConstraint(PhantomData<keyword::Unique>);

/// Referential action for `ON DELETE` / `ON UPDATE`.
///
/// Variant ordering: multi-word variants (`NO ACTION`, `SET NULL`, `SET DEFAULT`)
/// come before single-word ones to satisfy longest-match.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ReferentialAction {
    NoAction(NoActionKw),
    SetNull(SetNullKw),
    SetDefault(SetDefaultKw),
    Cascade(PhantomData<keyword::Cascade>),
    Restrict(PhantomData<keyword::Restrict>),
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NoActionKw {
    pub _no: PhantomData<keyword::No>,
    pub _action: PhantomData<keyword::Action>,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetNullKw {
    pub _set: PhantomData<keyword::Set>,
    pub _null: PhantomData<keyword::Null>,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetDefaultKw {
    pub _set: PhantomData<keyword::Set>,
    pub _default: PhantomData<keyword::Default>,
}

/// `ON DELETE <action>`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnDeleteAction {
    pub _on: PhantomData<keyword::On>,
    pub _delete: PhantomData<keyword::Delete>,
    pub action: ReferentialAction,
}

/// `ON UPDATE <action>`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OnUpdateAction {
    pub _on: PhantomData<keyword::On>,
    pub _update: PhantomData<keyword::Update>,
    pub action: ReferentialAction,
}

/// Match type for a foreign key: `MATCH FULL | PARTIAL | SIMPLE`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum MatchKind {
    Full(PhantomData<keyword::Full>),
    Partial(PhantomData<keyword::Partial>),
    Simple(PhantomData<keyword::Simple>),
}

/// `MATCH FULL | MATCH PARTIAL | MATCH SIMPLE`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MatchClause {
    pub _match: PhantomData<keyword::Match>,
    pub kind: MatchKind,
}

/// `DEFERRABLE | NOT DEFERRABLE`.
///
/// Variant ordering: `NotDeferrable` (two keywords) before `Deferrable`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum DeferrableKind {
    NotDeferrable(NotDeferrableKw),
    Deferrable(PhantomData<keyword::Deferrable>),
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotDeferrableKw {
    pub _not: PhantomData<keyword::Not>,
    pub _deferrable: PhantomData<keyword::Deferrable>,
}

/// `INITIALLY DEFERRED | INITIALLY IMMEDIATE`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InitiallyClause {
    pub _initially: PhantomData<keyword::Initially>,
    pub mode: InitiallyMode,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InitiallyMode {
    Deferred(PhantomData<keyword::Deferred>),
    Immediate(PhantomData<keyword::Immediate>),
}

/// REFERENCES constraint:
/// `REFERENCES table [(col, ...)] [MATCH ...] [ON DELETE ...] [ON UPDATE ...] [DEFERRABLE | NOT DEFERRABLE] [INITIALLY ...]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReferencesConstraint {
    pub _references: PhantomData<keyword::References>,
    pub table: literal::AliasName,
    pub columns: Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub match_clause: Option<MatchClause>,
    pub on_delete: Option<OnDeleteAction>,
    pub on_update: Option<OnUpdateAction>,
    pub deferrable: Option<DeferrableKind>,
    pub initially: Option<InitiallyClause>,
}

/// `CHECK (expr) [NO INHERIT]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CheckConstraint {
    pub _check: PhantomData<keyword::Check>,
    pub expr: Surrounded<punct::LParen, crate::ast::expr::Expr, punct::RParen>,
    pub no_inherit: Option<NoInheritKw>,
}

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NoInheritKw {
    pub _no: PhantomData<keyword::No>,
    pub _inherit: PhantomData<keyword::Inherit>,
}

/// GENERATED ALWAYS AS IDENTITY column constraint.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GeneratedIdentityConstraint(
    PhantomData<keyword::Generated>,
    PhantomData<keyword::Always>,
    PhantomData<keyword::As>,
    PhantomData<keyword::Identity>,
);

/// DEFAULT expr column constraint.
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
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ColumnConstraintKind {
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
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ConstraintNamePrefix {
    pub _constraint: PhantomData<keyword::Constraint>,
    pub name: literal::Ident,
}

/// A column constraint with its optional `CONSTRAINT name` prefix.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnConstraint {
    pub name: Option<ConstraintNamePrefix>,
    pub kind: ColumnConstraintKind,
}

/// A column definition: `name type [constraints...]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDef {
    pub name: literal::Ident,
    pub type_name: crate::ast::expr::CastType,
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
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ConstraintAttrs {
    pub deferrable: Option<DeferrableKind>,
    pub initially: Option<InitiallyClause>,
}

/// `PRIMARY KEY (col, ...)`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TablePrimaryKey {
    pub _primary: PhantomData<keyword::Primary>,
    pub _key: PhantomData<keyword::Key>,
    pub columns: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
    pub attrs: ConstraintAttrs,
}

/// `UNIQUE (col, ...)`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableUnique {
    pub _unique: PhantomData<keyword::Unique>,
    pub columns: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
    pub attrs: ConstraintAttrs,
}

/// `FOREIGN KEY (col, ...) REFERENCES table [(col, ...)] [MATCH ...] [ON ...] [DEFERRABLE ...] [INITIALLY ...]`
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
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TableConstraintKind {
    PrimaryKey(TablePrimaryKey),
    ForeignKey(TableForeignKey),
    Unique(TableUnique),
    Check(TableCheck),
}

/// A table-level constraint with optional `CONSTRAINT name` prefix.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TableConstraint {
    pub name: Option<ConstraintNamePrefix>,
    pub kind: TableConstraintKind,
}

/// One item in a CREATE TABLE column list: either a column definition or
/// a table-level constraint.
///
/// Variant ordering: `Constraint` must come first because its leading
/// tokens (`CONSTRAINT`, `PRIMARY`, `UNIQUE`, `FOREIGN`, `CHECK`) are
/// keywords, while a `Column` starts with an identifier — peek
/// disambiguates cleanly, but declaration order prefers the longer match.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ColumnOrConstraint {
    Constraint(TableConstraint),
    Column(ColumnDef),
}

/// Optional TEMP or TEMPORARY keyword.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TempKw {
    Temp(PhantomData<keyword::Temp>),
    Temporary(PhantomData<keyword::Temporary>),
}

/// INHERITS clause: `INHERITS (parent, ...)`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InheritsClause {
    pub _inherits: PhantomData<keyword::Inherits>,
    pub parents: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
}

/// Column-based table body: `(cols_and_constraints) [INHERITS (...)] [PARTITION BY ...]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnsBody {
    pub columns:
        Surrounded<punct::LParen, Seq<ColumnOrConstraint, punct::Comma>, punct::RParen>,
    pub inherits: Option<InheritsClause>,
    pub partition_by: Option<PartitionByClause>,
    pub with_storage: Option<crate::ast::create_index::WithStorage>,
}

/// Partition-of table body: `PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionOfBody {
    pub _partition: PhantomData<keyword::Partition>,
    pub _of: PhantomData<keyword::Of>,
    pub parent: literal::Ident,
    pub for_values: ForValuesInClause,
    pub partition_by: Option<PartitionByClause>,
    pub with_storage: Option<crate::ast::create_index::WithStorage>,
}

/// AS-query table body: `AS SELECT ...`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsQueryBody {
    pub _as: PhantomData<keyword::As>,
    pub query: Box<crate::ast::Statement>,
}

/// The body of a CREATE TABLE statement after `CREATE [TEMP] TABLE name`.
///
/// Variant ordering: AsQuery (`AS`) and PartitionOf (`PARTITION`) start with
/// keywords; Columns starts with `(`. Longest-match-wins disambiguates.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CreateTableBody {
    AsQuery(AsQueryBody),
    PartitionOf(PartitionOfBody),
    Columns(ColumnsBody),
}

/// ```sql
/// CREATE [TEMP] TABLE statement.
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTableStmt {
    pub _create: PhantomData<keyword::Create>,
    pub temp: Option<TempKw>,
    pub _table: PhantomData<keyword::Table>,
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
            CreateTableBody::PartitionOf(_) | CreateTableBody::AsQuery(_) => None,
        }
    }

    /// Returns only the column definitions (excluding table constraints).
    pub fn column_defs(&self) -> Option<Vec<&ColumnDef>> {
        self.items().map(|s| {
            s.iter()
                .filter_map(|item| match item {
                    ColumnOrConstraint::Column(c) => Some(c),
                    ColumnOrConstraint::Constraint(_) => None,
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
    fn parse_create_table_array_column_types() {
        let mut input = Input::new(
            "CREATE TABLE t (a int2[], b int4[][][], c varchar(5)[], d text[])",
        );
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
}
