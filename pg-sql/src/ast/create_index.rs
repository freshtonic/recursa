/// CREATE INDEX / DROP INDEX statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};
use recursa_diagram::railroad;

pub use crate::ast::common::{CascadeKw, DropBehavior, RestrictKw};

use crate::ast::create_view::IfExistsKw;
use crate::ast::expr::{Expr, FuncCall};
use crate::ast::select::{NullsOrder, SortDir, WhereClause};
use crate::ast::set_reset::SetValue;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// `IF NOT EXISTS` keyword sequence.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IfNotExistsKw {
    pub _if: PhantomData<keyword::If>,
    pub _not: PhantomData<keyword::Not>,
    pub _exists: PhantomData<keyword::Exists>,
}

/// Index access method: `USING method_name`.
///
/// The method name can be an identifier or one of the built-in method
/// keywords (`btree`, `gin`, ...). We accept `literal::AliasName` so both
/// identifiers and keywords are allowed in this position.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UsingMethod {
    pub _using: PhantomData<keyword::Using>,
    pub method: literal::AliasName,
}

/// A single opclass option: `name = value`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OpclassOption {
    pub name: literal::AliasName,
    pub _eq: punct::Eq,
    pub value: Expr,
}

/// Parenthesized opclass option list: `(name = value, ...)`.
pub type OpclassOptions =
    Surrounded<punct::LParen, Seq<OpclassOption, punct::Comma>, punct::RParen>;

/// Opclass name plus optional options: `int4_ops [(opt = val, ...)]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OpclassSpec {
    pub name: literal::Ident,
    pub options: Option<OpclassOptions>,
}

/// A storage parameter entry: `name [= value]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StorageParam {
    pub name: literal::AliasName,
    pub value: Option<StorageParamValue>,
}

/// `= value` suffix for a storage parameter.
///
/// The value is a permissive SetValue (keywords like `off`, `on`, string/numeric
/// literals, identifiers) rather than a full `Expr` — storage param values are
/// simple literals and `Expr::ColumnRef` rejects keywords like `off`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct StorageParamValue {
    pub _eq: punct::Eq,
    pub value: SetValue,
}

/// `WITH (name = value, ...)` storage parameters clause.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithStorage {
    pub _with: PhantomData<keyword::With>,
    pub params: Surrounded<punct::LParen, Seq<StorageParam, punct::Comma>, punct::RParen>,
}

/// `INCLUDE (col, ...)` covering-index clause.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IncludeClause {
    pub _include: PhantomData<keyword::Include>,
    pub columns: Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>,
}

/// Index column target: a parenthesized expression, a bare function call
/// (e.g., `lower(fruit)`), or a plain column identifier.
///
/// Variant ordering:
/// - `Expr` (`(`) starts with a different token than the others.
/// - `Func` (`ident(`) must come before `Col` (`ident`) so longest-match
///   prefers the function call form.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum IndexTarget {
    Expr(Surrounded<punct::LParen, Box<Expr>, punct::RParen>),
    Func(Box<FuncCall>),
    Col(literal::Ident),
}

/// `COLLATE "name"` on an index element.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IndexCollate {
    pub _collate: PhantomData<keyword::Collate>,
    pub name: literal::Ident,
}

/// An index element:
/// `column_or_expr [COLLATE "name"] [opclass [(options)]] [ASC|DESC] [NULLS FIRST|LAST]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IndexElem {
    pub target: IndexTarget,
    pub collate: Option<IndexCollate>,
    pub opclass: Option<OpclassSpec>,
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
pub struct CreateIndexStmt {
    pub _create: PhantomData<keyword::Create>,
    pub unique: Option<PhantomData<keyword::Unique>>,
    pub _index: PhantomData<keyword::Index>,
    pub concurrently: Option<PhantomData<keyword::Concurrently>>,
    pub if_not_exists: Option<IfNotExistsKw>,
    pub name: Option<literal::Ident>,
    pub _on: PhantomData<keyword::On>,
    /// Optional `ONLY` modifier — restricts the index to the named table
    /// without descending into inheritance children (partitioned tables).
    pub only: Option<PhantomData<keyword::Only>>,
    pub table_name: literal::Ident,
    pub using: Option<UsingMethod>,
    pub columns: Surrounded<punct::LParen, Seq<IndexElem, punct::Comma>, punct::RParen>,
    pub include: Option<IncludeClause>,
    pub with_storage: Option<WithStorage>,
    pub where_clause: Option<WhereClause>,
}

/// DROP INDEX statement:
///
/// ```sql
/// DROP INDEX [CONCURRENTLY] [IF EXISTS] name [, name ...] [CASCADE | RESTRICT]
/// ```
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropIndexStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _index: PhantomData<keyword::Index>,
    pub concurrently: Option<PhantomData<keyword::Concurrently>>,
    pub if_exists: Option<IfExistsKw>,
    pub names: Seq<literal::Ident, punct::Comma>,
    pub behavior: Option<DropBehavior>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_index::{CreateIndexStmt, DropIndexStmt};
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
