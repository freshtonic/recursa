/// MERGE statement AST.
///
/// `MERGE INTO table USING source ON condition
///   WHEN MATCHED THEN UPDATE SET ...
///   WHEN NOT MATCHED THEN INSERT VALUES (...)`
use std::marker::PhantomData;

use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::expr::Expr;
use crate::ast::select::TableRef;
use crate::ast::update::{ReturningClause, SetAssignment};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// WHEN MATCHED THEN UPDATE SET ...
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhenMatchedUpdate {
    pub _when: PhantomData<keyword::When>,
    pub _matched: PhantomData<keyword::Matched>,
    pub _then: PhantomData<keyword::Then>,
    pub _update: PhantomData<keyword::Update>,
    pub _set: PhantomData<keyword::Set>,
    pub assignments: Seq<SetAssignment, punct::Comma>,
}

/// WHEN MATCHED THEN DELETE
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhenMatchedDelete {
    pub _when: PhantomData<keyword::When>,
    pub _matched: PhantomData<keyword::Matched>,
    pub _then: PhantomData<keyword::Then>,
    pub _delete: PhantomData<keyword::Delete>,
}

/// WHEN NOT MATCHED THEN INSERT [columns] VALUES (...)
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhenNotMatchedInsert {
    pub _when: PhantomData<keyword::When>,
    pub _not: PhantomData<keyword::Not>,
    pub _matched: PhantomData<keyword::Matched>,
    pub _then: PhantomData<keyword::Then>,
    pub _insert: PhantomData<keyword::Insert>,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub _values: PhantomData<keyword::Values>,
    pub values: Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>,
}

/// A WHEN clause in MERGE.
///
/// Variant ordering: NotMatchedInsert (`WHEN NOT ...`) is longest,
/// then MatchedDelete/MatchedUpdate (`WHEN MATCHED THEN DELETE/UPDATE`)
/// are disambiguated by their trailing keyword.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum WhenClause {
    NotMatchedInsert(WhenNotMatchedInsert),
    MatchedDelete(WhenMatchedDelete),
    MatchedUpdate(WhenMatchedUpdate),
}

/// MERGE statement.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct MergeStmt {
    pub _merge: PhantomData<keyword::Merge>,
    pub _into: PhantomData<keyword::Into>,
    pub table_name: literal::Ident,
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
        let stmt = MergeStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "m");
        assert_eq!(stmt.when_clauses.len(), 2);
        assert!(input.is_empty());
    }
}
