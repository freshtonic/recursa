/// Partition table support.
///
/// `CREATE TABLE ... PARTITION BY LIST (col)`
/// `CREATE TABLE ... PARTITION OF parent FOR VALUES IN (val, ...)`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Parse, Visit};

use crate::ast::expr::{Expr, TypeName};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// PARTITION BY LIST (col) clause.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionByClause {
    pub _partition: PhantomData<keyword::Partition>,
    pub _by: PhantomData<keyword::By>,
    pub strategy: literal::AliasName,
    pub columns: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
}

/// FOR VALUES IN (val, ...) clause.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ForValuesInClause {
    pub _for: PhantomData<keyword::For>,
    pub _values: PhantomData<keyword::Values>,
    pub _in: PhantomData<keyword::In>,
    pub values: Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>,
}

/// Column definition in partition table: `name type`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionColumnDef {
    pub name: literal::Ident,
    pub type_name: TypeName,
}

/// CREATE TABLE with PARTITION BY: `CREATE TABLE name (cols) PARTITION BY strategy (cols)`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreatePartitionedTableStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _table: PhantomData<keyword::Table>,
    pub name: literal::Ident,
    pub columns: Surrounded<punct::LParen, Seq<PartitionColumnDef, punct::Comma>, punct::RParen>,
    pub partition_by: PartitionByClause,
}

/// CREATE TABLE ... PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...].
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreatePartitionOfStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _table: PhantomData<keyword::Table>,
    pub name: literal::Ident,
    pub _partition: PhantomData<keyword::Partition>,
    pub _of: PhantomData<keyword::Of>,
    pub parent: literal::Ident,
    pub for_values: ForValuesInClause,
    pub partition_by: Option<PartitionByClause>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::partition::{CreatePartitionOfStmt, CreatePartitionedTableStmt};
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_partitioned_table() {
        let mut input =
            Input::new("create table list_parted_tbl (a int,b int) partition by list (a)");
        let stmt = CreatePartitionedTableStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "list_parted_tbl");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_partition_of() {
        let mut input = Input::new(
            "create table list_parted_tbl1 partition of list_parted_tbl for values in (1) partition by list(b)",
        );
        let stmt = CreatePartitionOfStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "list_parted_tbl1");
        assert_eq!(stmt.parent.0, "list_parted_tbl");
        assert!(stmt.partition_by.is_some());
        assert!(input.is_empty());
    }
}
