/// ANALYZE statement AST: `ANALYZE tablename`.
use std::marker::PhantomData;

use recursa::{Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal};

/// ANALYZE statement.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AnalyzeStmt {
    pub _analyze: PhantomData<keyword::Analyze>,
    pub table_name: literal::Ident,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::analyze::AnalyzeStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_analyze() {
        let mut input = Input::new("ANALYZE onek2");
        let stmt = AnalyzeStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "onek2");
        assert!(input.is_empty());
    }
}
