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
use std::marker::PhantomData;

use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::expr::Expr;
use crate::ast::select::{PlainTable, TableRef};
use crate::ast::update::{ReturningClause, SetAssignment};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};
use recursa_diagram::railroad;

/// `AND cond` qualifier on a WHEN clause.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AndCondition {
    pub _and: PhantomData<keyword::And>,
    pub condition: Expr,
}

/// `BY SOURCE` qualifier on `WHEN NOT MATCHED`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct BySource {
    pub _by: PhantomData<keyword::By>,
    pub _source: PhantomData<keyword::Source>,
}

/// `BY TARGET` qualifier on `WHEN NOT MATCHED`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ByTarget {
    pub _by: PhantomData<keyword::By>,
    pub _target: PhantomData<keyword::Target>,
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
pub struct UpdateAction {
    pub _update: PhantomData<keyword::Update>,
    pub _set: PhantomData<keyword::Set>,
    pub assignments: Seq<SetAssignment, punct::Comma>,
}

/// `DELETE` action body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DeleteAction {
    pub _delete: PhantomData<keyword::Delete>,
}

/// `DO NOTHING` action body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DoNothingAction {
    pub _do: PhantomData<keyword::Do>,
    pub _nothing: PhantomData<keyword::Nothing>,
}

/// Action allowed after `WHEN MATCHED ... THEN`.
///
/// Variant ordering: `DoNothing` (`DO NOTHING`) and `Update` (`UPDATE`) and
/// `Delete` (`DELETE`) all start with distinct keywords, so order is by
/// declaration only.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum MatchedAction {
    Update(UpdateAction),
    Delete(DeleteAction),
    DoNothing(DoNothingAction),
}

/// `DEFAULT VALUES` form of an INSERT body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertDefaultValues {
    pub _default: PhantomData<keyword::Default>,
    pub _values: PhantomData<keyword::Values>,
}

/// A single row of values: `(expr, ...)`.
pub type ValueRow = Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>;

/// `VALUES (row), (row), ...` body.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertValuesBody {
    pub _values: PhantomData<keyword::Values>,
    pub rows: Seq<ValueRow, punct::Comma>,
}

/// Body of an INSERT inside MERGE: `VALUES ...` or `DEFAULT VALUES`.
///
/// Variant ordering: `Default` (`DEFAULT VALUES`) is matched before
/// `Values` (`VALUES`) since they begin with different keywords.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InsertBody {
    Default(InsertDefaultValues),
    Values(InsertValuesBody),
}

/// Optional `INTO target_name` after `INSERT`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertInto {
    pub _into: PhantomData<keyword::Into>,
    pub name: literal::Ident,
}

/// `INSERT [INTO target] [(cols)] { VALUES ... | DEFAULT VALUES }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertAction {
    pub _insert: PhantomData<keyword::Insert>,
    pub into: Option<InsertInto>,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub body: InsertBody,
}

/// Action allowed after `WHEN NOT MATCHED ... THEN`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NotMatchedAction {
    Insert(InsertAction),
    DoNothing(DoNothingAction),
}

/// `WHEN NOT MATCHED [BY {SOURCE|TARGET}] [AND cond] THEN action`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhenNotMatched {
    pub _when: PhantomData<keyword::When>,
    pub _not: PhantomData<keyword::Not>,
    pub _matched: PhantomData<keyword::Matched>,
    pub by: Option<NotMatchedBy>,
    pub and: Option<AndCondition>,
    pub _then: PhantomData<keyword::Then>,
    pub action: NotMatchedAction,
}

/// `WHEN MATCHED [AND cond] THEN action`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhenMatched {
    pub _when: PhantomData<keyword::When>,
    pub _matched: PhantomData<keyword::Matched>,
    pub and: Option<AndCondition>,
    pub _then: PhantomData<keyword::Then>,
    pub action: MatchedAction,
}

/// A WHEN clause in MERGE.
///
/// Variant ordering: `NotMatched` (`WHEN NOT MATCHED`) is longer than
/// `Matched` (`WHEN MATCHED`); list it first.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum WhenClause {
    NotMatched(WhenNotMatched),
    Matched(WhenMatched),
}

/// MERGE statement.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MergeStmt {
    pub _merge: PhantomData<keyword::Merge>,
    pub _into: PhantomData<keyword::Into>,
    pub target: PlainTable,
    pub _using: PhantomData<keyword::Using>,
    pub source: TableRef,
    pub _on: PhantomData<keyword::On>,
    pub condition: Expr,
    pub when_clauses: Seq<WhenClause, (), OptionalTrailing>,
    pub returning: Option<ReturningClause>,
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
        let sql = "MERGE INTO t USING s ON t.a = s.a WHEN MATCHED AND t.a = 2 THEN UPDATE SET b = s.b";
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
        let sql = "MERGE INTO t USING s ON t.a = s.a WHEN NOT MATCHED THEN INSERT VALUES (1,1), (2,2)";
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
