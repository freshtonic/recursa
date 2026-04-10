/// CREATE TABLE statement AST.
///
/// Handles three forms:
/// 1. `CREATE [TEMP] TABLE name (cols)`
/// 2. `CREATE [TEMP] TABLE name (cols) PARTITION BY strategy (cols)`
/// 3. `CREATE TABLE name PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...]`
///
/// Manual Parse impl required because the three forms share the same leading
/// keywords (`CREATE [TEMP] TABLE name`) but diverge after the name. The derive
/// macro's enum dispatch uses longest-match on first_pattern without fork-and-try,
/// so it can't distinguish between these forms at the regex level. A single struct
/// with a manual impl handles all three via sequential token inspection.
use std::marker::PhantomData;
use std::ops::ControlFlow;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::visitor::{AsNodeKey, Break, Visitor};
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::expr::TypeName;
use crate::ast::partition::{ForValuesInClause, PartitionByClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// PRIMARY KEY column constraint.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrimaryKey(PhantomData<keyword::Primary>, PhantomData<keyword::Key>);

/// A column definition: `name type [PRIMARY KEY]`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ColumnDef {
    pub name: literal::Ident,
    pub type_name: TypeName,
    pub primary_key: Option<PrimaryKey>,
}

/// Optional TEMP keyword.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct TempKw(PhantomData<keyword::Temp>);

/// The body of a CREATE TABLE statement after `CREATE [TEMP] TABLE name`.
#[derive(Debug, Clone)]
pub enum CreateTableBody {
    /// Regular or partitioned: `(cols) [PARTITION BY ...]`
    Columns {
        columns: Surrounded<punct::LParen, Seq<ColumnDef, punct::Comma>, punct::RParen>,
        partition_by: Option<PartitionByClause>,
    },
    /// Partition of: `PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...]`
    PartitionOf {
        parent: literal::Ident,
        for_values: ForValuesInClause,
        partition_by: Option<PartitionByClause>,
    },
}

impl AsNodeKey for CreateTableBody {}

impl Visit for CreateTableBody {
    fn visit<V: Visitor>(&self, _visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        ControlFlow::Continue(())
    }
}

/// CREATE [TEMP] TABLE statement.
#[derive(Debug, Visit)]
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
            CreateTableBody::Columns { columns, .. } => Some(columns),
            CreateTableBody::PartitionOf { .. } => None,
        }
    }
}

impl<'input> Parse<'input> for CreateTableStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::Create::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        keyword::Create::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        let _create = PhantomData::<keyword::Create>::parse(input, rules)?;
        R::consume_ignored(input);
        let temp = Option::<TempKw>::parse(input, rules)?;
        R::consume_ignored(input);
        let _table = PhantomData::<keyword::Table>::parse(input, rules)?;
        R::consume_ignored(input);
        let name = literal::Ident::parse(input, rules)?;
        R::consume_ignored(input);

        // Distinguish: PARTITION OF ... vs (columns) [PARTITION BY ...]
        let body = if keyword::Partition::peek(input, rules) {
            // PARTITION OF parent FOR VALUES IN (...)
            PhantomData::<keyword::Partition>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Of>::parse(input, rules)?;
            R::consume_ignored(input);
            let parent = literal::Ident::parse(input, rules)?;
            R::consume_ignored(input);
            let for_values = ForValuesInClause::parse(input, rules)?;
            R::consume_ignored(input);
            let partition_by = Option::<PartitionByClause>::parse(input, rules)?;
            CreateTableBody::PartitionOf {
                parent,
                for_values,
                partition_by,
            }
        } else {
            // (columns) [PARTITION BY ...]
            let columns = Surrounded::parse(input, rules)?;
            R::consume_ignored(input);
            let partition_by = Option::<PartitionByClause>::parse(input, rules)?;
            CreateTableBody::Columns {
                columns,
                partition_by,
            }
        };

        Ok(CreateTableStmt {
            _create,
            temp,
            _table,
            name,
            body,
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
