/// CREATE INDEX / DROP INDEX statement AST.
use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

pub use crate::ast::common::DropBehavior;

use crate::ast::expr::{Expr, FuncCall};
use crate::ast::select::{NullsOrder, SortDir, WhereClause};
use crate::ast::set_reset::SetValue;
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};

use crate::tokens::keyword::*;
/// Index access method: `USING method_name`.
///
/// The method name can be an identifier or one of the built-in method
/// keywords (`btree`, `gin`, ...). We accept `literal::AliasName` so both
/// identifiers and keywords are allowed in this position.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UsingMethod<'input> {
    pub _using: USING,
    pub method: literal::AliasName<'input>,
}

/// A single opclass option: `name = value`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OpclassOption<'input> {
    pub name: literal::AliasName<'input>,
    pub _eq: punct::Eq,
    pub value: Expr<'input>,
}

/// Parenthesized opclass option list: `(name = value, ...)`.
pub type OpclassOptions<'input> =
    Surrounded<punct::LParen, Seq<OpclassOption<'input>, punct::Comma>, punct::RParen>;

/// Opclass name plus optional options: `int4_ops [(opt = val, ...)]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OpclassSpec<'input> {
    pub name: literal::Ident<'input>,
    pub options: Option<OpclassOptions<'input>>,
}

/// A storage parameter entry: `name [= value]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StorageParam<'input> {
    pub name: literal::AliasName<'input>,
    pub value: Option<StorageParamValue<'input>>,
}

/// `= value` suffix for a storage parameter.
///
/// The value is a permissive SetValue (keywords like `off`, `on`, string/numeric
/// literals, identifiers) rather than a full `Expr` — storage param values are
/// simple literals and `Expr::ColumnRef` rejects keywords like `off`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StorageParamValue<'input> {
    pub _eq: punct::Eq,
    pub value: SetValue<'input>,
}

/// `WITH (name = value, ...)` storage parameters clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithStorage<'input> {
    pub _with: WITH,
    pub params:
        Surrounded<punct::LParen, Seq<StorageParam<'input>, punct::Comma>, punct::RParen>,
}

/// `INCLUDE (col, ...)` covering-index clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IncludeClause<'input> {
    pub _include: INCLUDE,
    pub columns:
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
}

/// Index column target: a parenthesized expression, a bare function call
/// (e.g., `lower(fruit)`), or a plain column identifier.
///
/// Variant ordering:
/// - `Expr` (`(`) starts with a different token than the others.
/// - `Func` (`ident(`) must come before `Col` (`ident`) so longest-match
///   prefers the function call form.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum IndexTarget<'input> {
    Expr(Surrounded<punct::LParen, Box<Expr<'input>>, punct::RParen>),
    Func(Box<FuncCall<'input>>),
    Col(literal::Ident<'input>),
}

/// `COLLATE "name"` on an index element.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IndexCollate<'input> {
    pub _collate: COLLATE,
    pub name: literal::Ident<'input>,
}

/// An index element:
/// `column_or_expr [COLLATE "name"] [opclass [(options)]] [ASC|DESC] [NULLS FIRST|LAST]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IndexElem<'input> {
    pub target: IndexTarget<'input>,
    pub collate: Option<IndexCollate<'input>>,
    pub opclass: Option<OpclassSpec<'input>>,
    pub dir: Option<SortDir>,
    pub nulls: Option<NullsOrder>,
}

/// CREATE INDEX statement.
///
/// ```sql
/// CREATE [UNIQUE] INDEX [CONCURRENTLY] [IF NOT EXISTS] [name]
///        ON table [USING method] (index_elem, ...)
///        [INCLUDE (col, ...)]
///        [WITH (storage_param = value, ...)]
///        [WHERE predicate]
/// ```
///
/// The index name is optional (Postgres allows it to be omitted).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateIndexStmt<'input> {
    pub _create: CREATE,
    pub unique: Option<UNIQUE>,
    pub _index: INDEX,
    pub concurrently: Option<CONCURRENTLY>,
    pub if_not_exists: Option<(IF, NOT, EXISTS)>,
    pub name: Option<literal::Ident<'input>>,
    pub _on: ON,
    /// Optional `ONLY` modifier — restricts the index to the named table
    /// without descending into inheritance children (partitioned tables).
    pub only: Option<ONLY>,
    pub table_name: literal::Ident<'input>,
    pub using: Option<Box<UsingMethod<'input>>>,
    pub columns:
        Surrounded<punct::LParen, Seq<IndexElem<'input>, punct::Comma>, punct::RParen>,
    pub include: Option<Box<IncludeClause<'input>>>,
    pub nulls_distinct: Option<NullsDistinctClause>,
    pub with_storage: Option<Box<WithStorage<'input>>>,
    pub where_clause: Option<Box<WhereClause<'input>>>,
}

/// `NULLS [NOT] DISTINCT` modifier on a unique index.
///
/// Variant ordering: `NotDistinct` (`NULLS NOT DISTINCT`, longer) before
/// `Distinct` (`NULLS DISTINCT`, shorter).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NullsDistinctClause {
    NotDistinct((NULLS, NOT, DISTINCT)),
    Distinct((NULLS, DISTINCT)),
}

/// DROP INDEX statement:
///
/// ```sql
/// DROP INDEX [CONCURRENTLY] [IF EXISTS] name [, name ...] [CASCADE | RESTRICT]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropIndexStmt<'input> {
    pub _drop: DROP,
    pub _index: INDEX,
    pub concurrently: Option<CONCURRENTLY>,
    pub if_exists: Option<(IF, EXISTS)>,
    pub names: Seq<literal::Ident<'input>, punct::Comma>,
    pub behavior: Option<DropBehavior>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_index::{CreateIndexStmt, DropIndexStmt};

    #[test]
    fn parse_create_unique_index_nulls_distinct() {
        let mut input =
            recursa::Input::new("CREATE UNIQUE INDEX i ON t (i) NULLS NOT DISTINCT");
        let _stmt = CreateIndexStmt::parse::<crate::rules::SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
        let mut input = recursa::Input::new("CREATE UNIQUE INDEX i ON t (i) NULLS DISTINCT");
        let _stmt = CreateIndexStmt::parse::<crate::rules::SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_index() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.as_ref().unwrap().text(), "fooi");
        assert_eq!(stmt.table_name.text(), "foo");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_with_desc() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1 DESC)");
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_desc_nulls_last() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1 DESC NULLS LAST)");
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_if_not_exists() {
        let mut input = Input::new("CREATE INDEX IF NOT EXISTS fooi ON foo (f1)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.if_not_exists.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_concurrently() {
        let mut input = Input::new("CREATE INDEX CONCURRENTLY fooi ON foo (f1)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.concurrently.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_on_only() {
        let mut input = Input::new("CREATE INDEX idx ON ONLY ptif_test (a)");
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_unnamed() {
        let mut input = Input::new("CREATE INDEX ON foo (f1)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.name.is_none());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_using_btree() {
        let mut input = Input::new("CREATE INDEX fooi ON foo USING btree (f1)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.using.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_using_gin() {
        let mut input = Input::new("CREATE INDEX fooi ON foo USING gin (f1)");
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_opclass() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1 int4_ops)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
        let _ = stmt;
    }

    #[test]
    fn parse_create_index_opclass_desc() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1 text_pattern_ops DESC)");
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_expr_column() {
        let mut input = Input::new("CREATE INDEX i ON t ((lower(name)))");
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_include() {
        let mut input = Input::new("CREATE INDEX i ON t (a) INCLUDE (b, c)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.include.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_where_predicate() {
        let mut input = Input::new("CREATE INDEX i ON t (a) WHERE a > 0");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_with_storage() {
        let mut input = Input::new("CREATE INDEX i ON t (a) WITH (fillfactor = 70)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.with_storage.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_with_storage_keyword_value_off() {
        use crate::ast::create_table::CreateTableStmt;
        let mut input = Input::new(
            "CREATE TABLE target (tid integer, balance integer) WITH (autovacuum_enabled=off)",
        );
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_table_with_storage_string_value() {
        use crate::ast::create_table::CreateTableStmt;
        let mut input = Input::new("CREATE TABLE t (a int) WITH (foo = 'bar')");
        let _stmt = CreateTableStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_unique_index() {
        let mut input = Input::new("CREATE UNIQUE INDEX i ON t (a)");
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.unique.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_full_kitchen_sink() {
        let mut input = Input::new(
            "CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx ON t USING btree (a int4_ops ASC, (lower(b))) INCLUDE (c) WITH (fillfactor = 70) WHERE c > 0",
        );
        let stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.unique.is_some());
        assert!(stmt.concurrently.is_some());
        assert!(stmt.if_not_exists.is_some());
        assert!(stmt.using.is_some());
        assert!(stmt.include.is_some());
        assert!(stmt.with_storage.is_some());
        assert!(stmt.where_clause.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_opclass_on_second_col() {
        let mut input = Input::new(
            "create unique index op_index_key on insertconflicttest(key, fruit text_pattern_ops)",
        );
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_collate() {
        let mut input = Input::new(
            "create unique index collation_index_key on insertconflicttest(key, fruit collate \"C\")",
        );
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_collate_and_opclass() {
        let mut input = Input::new(
            "create unique index both_index_key on insertconflicttest(key, fruit collate \"C\" text_pattern_ops)",
        );
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_func_target_collate_opclass() {
        let mut input = Input::new(
            "create unique index both_index_expr_key on insertconflicttest(key, lower(fruit) collate \"C\" text_pattern_ops)",
        );
        let _stmt = CreateIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_index() {
        let mut input = Input::new("DROP INDEX fooi");
        let stmt = DropIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.names.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_index_if_exists() {
        let mut input = Input::new("DROP INDEX IF EXISTS fooi");
        let stmt = DropIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.if_exists.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_index_concurrently() {
        let mut input = Input::new("DROP INDEX CONCURRENTLY fooi");
        let stmt = DropIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.concurrently.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_index_multiple() {
        let mut input = Input::new("DROP INDEX a, b, c");
        let stmt = DropIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.names.len(), 3);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_index_cascade() {
        let mut input = Input::new("DROP INDEX fooi CASCADE");
        let stmt = DropIndexStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.behavior.is_some());
        assert!(input.is_empty());
    }
}
