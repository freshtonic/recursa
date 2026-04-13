/// CREATE INDEX / DROP INDEX statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::select::{NullsOrder, SortDir};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// An index element: `column [ASC|DESC] [NULLS FIRST|LAST]`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IndexElem {
    pub column: literal::Ident,
    pub dir: Option<SortDir>,
    pub nulls: Option<NullsOrder>,
}

/// CREATE INDEX statement: `CREATE INDEX name ON table (col, ...)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateIndexStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _index: PhantomData<keyword::Index>,
    pub name: literal::Ident,
    pub _on: PhantomData<keyword::On>,
    pub table_name: literal::Ident,
    pub columns: Surrounded<punct::LParen, Seq<IndexElem, punct::Comma>, punct::RParen>,
}

/// DROP INDEX statement: `DROP INDEX name`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropIndexStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _index: PhantomData<keyword::Index>,
    pub name: literal::Ident,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_index::{CreateIndexStmt, DropIndexStmt};
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_index() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1)");
        let stmt = CreateIndexStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.text(), "fooi");
        assert_eq!(stmt.table_name.text(), "foo");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_with_desc() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1 DESC)");
        let _stmt = CreateIndexStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_index_desc_nulls_last() {
        let mut input = Input::new("CREATE INDEX fooi ON foo (f1 DESC NULLS LAST)");
        let _stmt = CreateIndexStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_index() {
        let mut input = Input::new("DROP INDEX fooi");
        let stmt = DropIndexStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.text(), "fooi");
        assert!(input.is_empty());
    }
}
