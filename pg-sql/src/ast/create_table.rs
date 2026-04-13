/// CREATE TABLE statement AST.
use std::marker::PhantomData;

use std::ops::ControlFlow;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::visitor::{AsNodeKey, Break, TotalVisitor};
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::partition::{ForValuesInClause, PartitionByClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// PRIMARY KEY column constraint.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrimaryKeyConstraint(PhantomData<keyword::Primary>, PhantomData<keyword::Key>);

/// NOT NULL column constraint.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotNullConstraint(PhantomData<keyword::Not>, PhantomData<keyword::Null>);

/// UNIQUE column constraint.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UniqueConstraint(PhantomData<keyword::Unique>);

/// REFERENCES constraint: `REFERENCES table [(col)]`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReferencesConstraint {
    pub _references: PhantomData<keyword::References>,
    pub table: literal::AliasName,
    pub column: Option<Surrounded<punct::LParen, literal::AliasName, punct::RParen>>,
}

/// GENERATED ALWAYS AS IDENTITY column constraint.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct GeneratedIdentityConstraint(
    PhantomData<keyword::Generated>,
    PhantomData<keyword::Always>,
    PhantomData<keyword::As>,
    PhantomData<keyword::Identity>,
);

/// DEFAULT expr column constraint.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DefaultConstraint {
    pub _default: PhantomData<keyword::Default>,
    pub expr: crate::ast::expr::Expr,
}

/// Column constraint kind.
///
/// Variant ordering for longest-match-wins:
/// - GeneratedIdentity (`GENERATED`) before others (unique keyword)
/// - PrimaryKey (`PRIMARY KEY`) before others (unique keyword)
/// - NotNull (`NOT NULL`) before others (unique keyword)
/// - References, Unique, Default all start with distinct keywords
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ColumnConstraint {
    GeneratedIdentity(GeneratedIdentityConstraint),
    PrimaryKey(PrimaryKeyConstraint),
    NotNull(NotNullConstraint),
    Unique(UniqueConstraint),
    References(ReferencesConstraint),
    Default(DefaultConstraint),
}

/// A column definition: `name type [constraints...]`.
///
/// Manual Parse impl needed because column constraints are a variable-length
/// sequence of optional clauses (PRIMARY KEY, NOT NULL, REFERENCES, UNIQUE,
/// GENERATED ALWAYS AS IDENTITY, DEFAULT) that must be consumed in any order.
/// To eliminate this, recursa would need unordered optional field groups.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: literal::Ident,
    pub type_name: crate::ast::expr::CastType,
    pub constraints: Vec<ColumnConstraint>,
}

impl ColumnDef {
    /// Returns the primary_key constraint if present (for backward compat).
    pub fn primary_key(&self) -> bool {
        self.constraints
            .iter()
            .any(|c| matches!(c, ColumnConstraint::PrimaryKey(_)))
    }
}

impl AsNodeKey for ColumnDef {}

impl Visit for ColumnDef {
    fn visit<V: TotalVisitor>(&self, _visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for ColumnDef {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        literal::Ident::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        literal::Ident::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let name = literal::Ident::parse(input, rules)?;
        R::consume_ignored(input);
        let type_name = crate::ast::expr::CastType::parse(input, rules)?;
        R::consume_ignored(input);

        let mut constraints = Vec::new();
        loop {
            if ColumnConstraint::peek(input, rules) {
                let mut fork = input.fork();
                match ColumnConstraint::parse(&mut fork, rules) {
                    Ok(c) => {
                        input.commit(fork);
                        R::consume_ignored(input);
                        constraints.push(c);
                    }
                    Err(_) => break,
                }
            } else {
                break;
            }
        }

        Ok(ColumnDef {
            name,
            type_name,
            constraints,
        })
    }
}

/// Optional TEMP or TEMPORARY keyword.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TempKw {
    Temp(PhantomData<keyword::Temp>),
    Temporary(PhantomData<keyword::Temporary>),
}

/// INHERITS clause: `INHERITS (parent, ...)`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InheritsClause {
    pub _inherits: PhantomData<keyword::Inherits>,
    pub parents: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
}

/// Column-based table body: `(cols) [INHERITS (...)] [PARTITION BY ...]`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnsBody {
    pub columns: Surrounded<punct::LParen, Seq<ColumnDef, punct::Comma>, punct::RParen>,
    pub inherits: Option<InheritsClause>,
    pub partition_by: Option<PartitionByClause>,
}

/// Partition-of table body: `PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...]`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionOfBody {
    pub _partition: PhantomData<keyword::Partition>,
    pub _of: PhantomData<keyword::Of>,
    pub parent: literal::Ident,
    pub for_values: ForValuesInClause,
    pub partition_by: Option<PartitionByClause>,
}

/// AS-query table body: `AS SELECT ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsQueryBody {
    pub _as: PhantomData<keyword::As>,
    pub query: Box<crate::ast::Statement>,
}

/// The body of a CREATE TABLE statement after `CREATE [TEMP] TABLE name`.
///
/// Variant ordering: AsQuery (`AS`) and PartitionOf (`PARTITION`) start with
/// keywords; Columns starts with `(`. Longest-match-wins disambiguates.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum CreateTableBody {
    AsQuery(AsQueryBody),
    PartitionOf(PartitionOfBody),
    Columns(ColumnsBody),
}

/// CREATE [TEMP] TABLE statement.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateTableStmt {
    pub _create: PhantomData<keyword::Create>,
    pub temp: Option<TempKw>,
    pub _table: PhantomData<keyword::Table>,
    pub name: literal::Ident,
    pub body: CreateTableBody,
}

impl CreateTableStmt {
    /// Returns the column definitions, if this is a columns-based table.
    pub fn columns(
        &self,
    ) -> Option<&Surrounded<punct::LParen, Seq<ColumnDef, punct::Comma>, punct::RParen>> {
        match &self.body {
            CreateTableBody::Columns(b) => Some(&b.columns),
            CreateTableBody::PartitionOf(_) | CreateTableBody::AsQuery(_) => None,
        }
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
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL1");
        assert_eq!(stmt.columns().unwrap().len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_multiple_columns() {
        let mut input = Input::new("CREATE TABLE BOOLTBL3 (d text, b bool, o int)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "BOOLTBL3");
        assert_eq!(stmt.columns().unwrap().len(), 3);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_boolean_type() {
        let mut input = Input::new("CREATE TABLE t (f1 boolean)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns().unwrap().len(), 1);
    }

    #[test]
    fn parse_create_temp_table() {
        let mut input = Input::new("CREATE TEMP TABLE foo (f1 int)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.temp.is_some());
        assert_eq!(stmt.name.0, "foo");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_partitioned_table() {
        let mut input =
            Input::new("create table list_parted_tbl (a int,b int) partition by list (a)");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "list_parted_tbl");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_partition_of() {
        let mut input = Input::new(
            "create table list_parted_tbl1 partition of list_parted_tbl for values in (1) partition by list(b)",
        );
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "list_parted_tbl1");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_temp_table_empty_columns() {
        let mut input = Input::new("CREATE TEMP TABLE nocols()");
        let stmt = CreateTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns().unwrap().len(), 0);
        assert!(input.is_empty());
    }
}
