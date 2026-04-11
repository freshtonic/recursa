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
use recursa::visitor::{AsNodeKey, Break, TotalVisitor};
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::partition::{ForValuesInClause, PartitionByClause};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// PRIMARY KEY column constraint.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct PrimaryKey(PhantomData<keyword::Primary>, PhantomData<keyword::Key>);

/// Column constraint kind.
#[derive(Debug, Clone)]
pub enum ColumnConstraint {
    PrimaryKey,
    NotNull,
    Unique,
    References(String, Option<String>),
    GeneratedAlwaysAsIdentity,
    Default(crate::ast::expr::Expr),
}

impl AsNodeKey for ColumnConstraint {}
impl Visit for ColumnConstraint {
    fn visit<V: TotalVisitor>(&self, _visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        ControlFlow::Continue(())
    }
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
            .any(|c| matches!(c, ColumnConstraint::PrimaryKey))
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
            if keyword::Primary::peek(input, rules) {
                PhantomData::<keyword::Primary>::parse(input, rules)?;
                R::consume_ignored(input);
                PhantomData::<keyword::Key>::parse(input, rules)?;
                R::consume_ignored(input);
                constraints.push(ColumnConstraint::PrimaryKey);
            } else if keyword::Not::peek(input, rules) {
                // NOT NULL
                let mut fork = input.fork();
                if PhantomData::<keyword::Not>::parse(&mut fork, rules).is_ok() {
                    R::consume_ignored(&mut fork);
                    if keyword::Null::peek(&fork, rules) {
                        PhantomData::<keyword::Null>::parse(&mut fork, rules)?;
                        input.advance(fork.cursor() - input.cursor());
                        R::consume_ignored(input);
                        constraints.push(ColumnConstraint::NotNull);
                        continue;
                    }
                }
                break;
            } else if keyword::Unique::peek(input, rules) {
                PhantomData::<keyword::Unique>::parse(input, rules)?;
                R::consume_ignored(input);
                constraints.push(ColumnConstraint::Unique);
            } else if keyword::References::peek(input, rules) {
                PhantomData::<keyword::References>::parse(input, rules)?;
                R::consume_ignored(input);
                let ref_table = literal::AliasName::parse(input, rules)?;
                R::consume_ignored(input);
                // Optional (col) reference
                let ref_col = if punct::LParen::peek(input, rules) {
                    punct::LParen::parse(input, rules)?;
                    R::consume_ignored(input);
                    let col = literal::AliasName::parse(input, rules)?;
                    R::consume_ignored(input);
                    punct::RParen::parse(input, rules)?;
                    R::consume_ignored(input);
                    Some(col.0)
                } else {
                    None
                };
                constraints.push(ColumnConstraint::References(ref_table.0, ref_col));
            } else if keyword::Generated::peek(input, rules) {
                // GENERATED ALWAYS AS IDENTITY
                PhantomData::<keyword::Generated>::parse(input, rules)?;
                R::consume_ignored(input);
                PhantomData::<keyword::Always>::parse(input, rules)?;
                R::consume_ignored(input);
                PhantomData::<keyword::As>::parse(input, rules)?;
                R::consume_ignored(input);
                PhantomData::<keyword::Identity>::parse(input, rules)?;
                R::consume_ignored(input);
                constraints.push(ColumnConstraint::GeneratedAlwaysAsIdentity);
            } else if keyword::Default::peek(input, rules) {
                PhantomData::<keyword::Default>::parse(input, rules)?;
                R::consume_ignored(input);
                let expr = crate::ast::expr::Expr::parse(input, rules)?;
                R::consume_ignored(input);
                constraints.push(ColumnConstraint::Default(expr));
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

/// The body of a CREATE TABLE statement after `CREATE [TEMP] TABLE name`.
#[derive(Debug, Clone)]
pub enum CreateTableBody {
    /// Regular or partitioned: `(cols) [PARTITION BY ...] [INHERITS (parent, ...)]`
    Columns {
        columns: Surrounded<punct::LParen, Seq<ColumnDef, punct::Comma>, punct::RParen>,
        inherits:
            Option<Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>>,
        partition_by: Option<PartitionByClause>,
    },
    /// Partition of: `PARTITION OF parent FOR VALUES IN (...) [PARTITION BY ...]`
    PartitionOf {
        parent: literal::Ident,
        for_values: ForValuesInClause,
        partition_by: Option<PartitionByClause>,
    },
    /// AS query: `AS SELECT ...`
    AsQuery { query: Box<crate::ast::Statement> },
}

impl AsNodeKey for CreateTableBody {}

impl Visit for CreateTableBody {
    fn visit<V: TotalVisitor>(&self, _visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        ControlFlow::Continue(())
    }
}

/// CREATE [TEMP] TABLE statement.
#[derive(Debug, Clone, Visit)]
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
            CreateTableBody::PartitionOf { .. } | CreateTableBody::AsQuery { .. } => None,
        }
    }
}

impl<'input> Parse<'input> for CreateTableStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // Pattern to distinguish CREATE [TEMP|TEMPORARY] TABLE from CREATE RULE/TRIGGER/etc.
        static PATTERN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        PATTERN.get_or_init(|| {
            r"(?i:CREATE\b)(?:\s+(?i:TEMP\b|\bTEMPORARY\b))?\s+(?i:TABLE\b)".to_string()
        })
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            regex::Regex::new(&format!(r"\A(?:{})", Self::first_pattern())).unwrap()
        });
        re.is_match(input.remaining())
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

        // Distinguish: AS query vs PARTITION OF ... vs (columns) [PARTITION BY ...]
        let body = if keyword::As::peek(input, rules) {
            // AS query
            PhantomData::<keyword::As>::parse(input, rules)?;
            R::consume_ignored(input);
            let query = Box::new(crate::ast::Statement::parse(input, rules)?);
            CreateTableBody::AsQuery { query }
        } else if keyword::Partition::peek(input, rules) {
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
            // (columns) [INHERITS (...)] [PARTITION BY ...]
            let columns = Surrounded::parse(input, rules)?;
            R::consume_ignored(input);
            // Optional INHERITS (parent, ...)
            let inherits = if keyword::Inherits::peek(input, rules) {
                PhantomData::<keyword::Inherits>::parse(input, rules)?;
                R::consume_ignored(input);
                Some(Surrounded::parse(input, rules)?)
            } else {
                None
            };
            R::consume_ignored(input);
            let partition_by = Option::<PartitionByClause>::parse(input, rules)?;
            CreateTableBody::Columns {
                columns,
                inherits,
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
