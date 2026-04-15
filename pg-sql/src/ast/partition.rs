/// Partition table support.
///
/// `CREATE TABLE ... PARTITION BY LIST (col)`
/// `CREATE TABLE ... PARTITION OF parent FOR VALUES IN (val, ...)`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::expr::{Expr, TypeName};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};
use recursa_diagram::railroad;

/// PARTITION BY LIST (col) clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionByClause<'input> {
    pub _partition: PhantomData<keyword::Partition>,
    pub _by: PhantomData<keyword::By>,
    pub strategy: literal::AliasName<'input>,
    /// Partition key items — may be plain column names or expressions like
    /// `((a+b)/2)`.
    pub columns: Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
}

/// FOR VALUES IN (val, ...) clause — legacy name kept for backward compat
/// with partition.rs own tests; the general form lives in `ForValuesClause`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ForValuesInClause<'input> {
    pub _for: PhantomData<keyword::For>,
    pub _values: PhantomData<keyword::Values>,
    pub _in: PhantomData<keyword::In>,
    pub values: Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
}

/// `FROM (...) TO (...)` range partition spec.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FromToSpec<'input> {
    pub _from: PhantomData<keyword::From>,
    pub from_values: Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
    pub _to: PhantomData<keyword::To>,
    pub to_values: Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
}

/// `IN (val, ...)` list partition spec.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InListSpec<'input> {
    pub _in: PhantomData<keyword::In>,
    pub values: Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
}

/// `MODULUS n` entry.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ModulusEntry<'input> {
    pub _modulus: PhantomData<keyword::Modulus>,
    pub value: Expr<'input>,
}

/// `REMAINDER n` entry.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RemainderEntry<'input> {
    pub _remainder: PhantomData<keyword::Remainder>,
    pub value: Expr<'input>,
}

/// One item in `WITH (...)` for hash partitioning: MODULUS n or REMAINDER n.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum HashPartItem<'input> {
    Modulus(ModulusEntry<'input>),
    Remainder(RemainderEntry<'input>),
}

/// `WITH (MODULUS n, REMAINDER m)` hash partition spec.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithModulusSpec<'input> {
    pub _with: PhantomData<keyword::With>,
    pub items:
        Surrounded<punct::LParen, Seq<HashPartItem<'input>, punct::Comma>, punct::RParen>,
}

/// Body after `FOR VALUES` in a PARTITION OF clause. Variant ordering:
/// `From` starts with `FROM`, `In` starts with `IN`, `With` starts with `WITH` —
/// all distinct keywords, so peek disambiguation is trivial.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ForValuesSpec<'input> {
    From(FromToSpec<'input>),
    In(InListSpec<'input>),
    With(WithModulusSpec<'input>),
}

/// Full `FOR VALUES ...` clause in a `PARTITION OF ...` body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ForValuesClause<'input> {
    pub _for: PhantomData<keyword::For>,
    pub _values: PhantomData<keyword::Values>,
    pub spec: ForValuesSpec<'input>,
}

/// Column definition in partition table: `name type`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PartitionColumnDef<'input> {
    pub name: literal::Ident<'input>,
    pub type_name: TypeName<'input>,
}

/// CREATE TABLE with PARTITION BY: `CREATE TABLE name (cols) PARTITION BY strategy (cols)`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreatePartitionedTableStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub _table: PhantomData<keyword::Table>,
    pub name: literal::Ident<'input>,
    pub columns:
        Surrounded<punct::LParen, Seq<PartitionColumnDef<'input>, punct::Comma>, punct::RParen>,
    pub partition_by: PartitionByClause<'input>,
}

/// CREATE TABLE ... PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...].
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreatePartitionOfStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub _table: PhantomData<keyword::Table>,
    pub name: literal::Ident<'input>,
    pub _partition: PhantomData<keyword::Partition>,
    pub _of: PhantomData<keyword::Of>,
    pub parent: literal::Ident<'input>,
    pub for_values: ForValuesInClause<'input>,
    pub partition_by: Option<PartitionByClause<'input>>,
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
        let stmt = CreatePartitionedTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "list_parted_tbl");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_partition_of() {
        let mut input = Input::new(
            "create table list_parted_tbl1 partition of list_parted_tbl for values in (1) partition by list(b)",
        );
        let stmt = CreatePartitionOfStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "list_parted_tbl1");
        assert_eq!(stmt.parent.text(), "list_parted_tbl");
        assert!(stmt.partition_by.is_some());
        assert!(input.is_empty());
    }
}
