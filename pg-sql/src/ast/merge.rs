/// MERGE statement AST.
///
/// ```sql
/// MERGE INTO [ONLY] target [[AS] alias]
/// USING source [[AS] alias] ON condition
/// WHEN [NOT] MATCHED [BY {SOURCE|TARGET}] [AND cond] THEN
///     { UPDATE SET ... | DELETE | DO NOTHING
///     | INSERT [INTO target] [(cols)] { VALUES (...) [, (...)] | DEFAULT VALUES } }
/// [RETURNING ...]
/// ```
use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::expr::Expr;
use crate::ast::select::{PlainTable, TableRef};
use crate::ast::update::{ReturningClause, SetAssignment};
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// `AND cond` qualifier on a WHEN clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AndCondition<'input> {
    pub _and: AND,
    pub condition: Expr<'input>,
}

/// `BY SOURCE` qualifier on `WHEN NOT MATCHED`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct BySource {
    pub _by: BY,
    pub _source: SOURCE,
}

/// `BY TARGET` qualifier on `WHEN NOT MATCHED`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ByTarget {
    pub _by: BY,
    pub _target: TARGET,
}

/// `BY SOURCE` or `BY TARGET`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NotMatchedBy {
    Source(BySource),
    Target(ByTarget),
}

/// `UPDATE SET col = expr, ...` action body (the part after THEN).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UpdateAction<'input> {
    pub _update: UPDATE,
    pub _set: SET,
    pub assignments: Seq<SetAssignment<'input>, punct::Comma>,
}

/// `DELETE` action body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeleteAction {
    pub _delete: DELETE,
}

/// `DO NOTHING` action body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoNothingAction {
    pub _do: DO,
    pub _nothing: NOTHING,
}

/// Action allowed after `WHEN MATCHED ... THEN`.
///
/// Variant ordering: `DoNothing` (`DO NOTHING`) and `Update` (`UPDATE`) and
/// `Delete` (`DELETE`) all start with distinct keywords, so order is by
/// declaration only.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum MatchedAction<'input> {
    Update(UpdateAction<'input>),
    Delete(DeleteAction),
    DoNothing(DoNothingAction),
}

/// `DEFAULT VALUES` form of an INSERT body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertDefaultValues {
    pub _default: DEFAULT,
    pub _values: VALUES,
}

/// A single row of values: `(expr, ...)`.
pub type ValueRow<'input> =
    Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>;

/// `VALUES (row), (row), ...` body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertValuesBody<'input> {
    pub _values: VALUES,
    pub rows: Seq<ValueRow<'input>, punct::Comma>,
}

/// Body of an INSERT inside MERGE: `VALUES ...` or `DEFAULT VALUES`.
///
/// Variant ordering: `Default` (`DEFAULT VALUES`) is matched before
/// `Values` (`VALUES`) since they begin with different keywords.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InsertBody<'input> {
    Default(InsertDefaultValues),
    Values(InsertValuesBody<'input>),
}

/// Optional `INTO target_name` after `INSERT`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertInto<'input> {
    pub _into: INTO,
    pub name: literal::Ident<'input>,
}

/// `INSERT [INTO target] [(cols)] { VALUES ... | DEFAULT VALUES }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertAction<'input> {
    pub _insert: INSERT,
    pub into: Option<InsertInto<'input>>,
    pub columns: Option<
        Surrounded<punct::LParen, Seq<literal::AliasName<'input>, punct::Comma>, punct::RParen>,
    >,
    pub body: InsertBody<'input>,
}

/// Action allowed after `WHEN NOT MATCHED ... THEN`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NotMatchedAction<'input> {
    Insert(InsertAction<'input>),
    DoNothing(DoNothingAction),
}

/// `WHEN NOT MATCHED [BY {SOURCE|TARGET}] [AND cond] THEN action`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhenNotMatched<'input> {
    pub _when: WHEN,
    pub _not: NOT,
    pub _matched: MATCHED,
    pub by: Option<NotMatchedBy>,
    pub and: Option<AndCondition<'input>>,
    pub _then: THEN,
    pub action: NotMatchedAction<'input>,
}

/// `WHEN MATCHED [AND cond] THEN action`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhenMatched<'input> {
    pub _when: WHEN,
    pub _matched: MATCHED,
    pub and: Option<AndCondition<'input>>,
    pub _then: THEN,
    pub action: MatchedAction<'input>,
}

/// A WHEN clause in MERGE.
///
/// Variant ordering: `NotMatched` (`WHEN NOT MATCHED`) is longer than
/// `Matched` (`WHEN MATCHED`); list it first.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum WhenClause<'input> {
    NotMatched(WhenNotMatched<'input>),
    Matched(WhenMatched<'input>),
}

/// MERGE statement.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MergeStmt<'input> {
    pub _merge: MERGE,
    pub _into: INTO,
    pub target: Box<PlainTable<'input>>,
    pub _using: USING,
    pub source: Box<TableRef<'input>>,
    pub _on: ON,
    pub condition: Box<Expr<'input>>,
    pub when_clauses: Seq<WhenClause<'input>, (), OptionalTrailing>,
    pub returning: Option<Box<ReturningClause<'input>>>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_merge_basic() {
        let sql = "MERGE INTO m USING (select 0 k, 'v' v) o ON m.k = o.k WHEN MATCHED THEN UPDATE SET v = 'updated' WHEN NOT MATCHED THEN INSERT VALUES(o.k, o.v)";
        let mut input = Input::new(sql);
        let stmt = MergeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.when_clauses.len(), 2);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_merge_target_alias() {
        let sql = "MERGE INTO target t USING source s ON t.tid = s.sid WHEN MATCHED THEN DELETE";
        let mut input = Input::new(sql);
        let _stmt = MergeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_merge_when_matched_and() {
        let sql =
            "MERGE INTO t USING s ON t.a = s.a WHEN MATCHED AND t.a = 2 THEN UPDATE SET b = s.b";
        let mut input = Input::new(sql);
        let _stmt = MergeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_merge_not_matched_by_source_default_values() {
        let sql = "MERGE INTO t USING s ON t.a = s.a WHEN NOT MATCHED BY SOURCE THEN INSERT DEFAULT VALUES";
        let mut input = Input::new(sql);
        let _stmt = MergeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_merge_do_nothing_both() {
        let sql = "MERGE INTO t USING s ON t.a = s.a WHEN MATCHED THEN DO NOTHING WHEN NOT MATCHED THEN DO NOTHING";
        let mut input = Input::new(sql);
        let _stmt = MergeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_merge_insert_multi_values() {
        let sql =
            "MERGE INTO t USING s ON t.a = s.a WHEN NOT MATCHED THEN INSERT VALUES (1,1), (2,2)";
        let mut input = Input::new(sql);
        let _stmt = MergeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_merge_insert_into_default_values() {
        let sql = "MERGE INTO target t USING source s ON t.tid = s.sid WHEN NOT MATCHED THEN INSERT INTO target DEFAULT VALUES";
        let mut input = Input::new(sql);
        let _stmt = MergeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
