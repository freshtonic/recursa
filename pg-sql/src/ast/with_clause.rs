/// WITH clause (Common Table Expressions) AST.
///
/// Supports `WITH [RECURSIVE] name [(col, ...)] AS [MATERIALIZED|NOT MATERIALIZED] (query)
///   [SEARCH DEPTH|BREADTH FIRST BY col, ... SET col]
///   [CYCLE col, ... SET col [TO val DEFAULT val] USING col]
///   [, ...] SELECT|INSERT|UPDATE|DELETE|MERGE`
use std::marker::PhantomData;

use recursa::seq::{NoTrailing, NonEmpty, Seq};
use recursa::surrounded::Surrounded;
use recursa::{Parse, Visit};

use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// Materialization option: `MATERIALIZED` or `NOT MATERIALIZED`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum MaterializedOption {
    NotMaterialized(NotMaterialized),
    Materialized(keyword::Materialized),
}

/// NOT MATERIALIZED
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NotMaterialized(
    PhantomData<keyword::Not>,
    PhantomData<keyword::Materialized>,
);

/// SEARCH direction: DEPTH or BREADTH
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SearchDirection {
    Depth(keyword::Depth),
    Breadth(keyword::Breadth),
}

/// SEARCH clause: `SEARCH DEPTH|BREADTH FIRST BY col, ... SET col`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SearchClause {
    pub _search: PhantomData<keyword::Search>,
    pub direction: SearchDirection,
    pub _first: PhantomData<keyword::First>,
    pub _by: PhantomData<keyword::By>,
    pub columns: Seq<literal::AliasName, punct::Comma>,
    pub _set: PhantomData<keyword::Set>,
    pub set_column: literal::AliasName,
}

/// CYCLE clause: `CYCLE col, ... SET col [TO val DEFAULT val] USING col`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CycleClause {
    pub _cycle: PhantomData<keyword::Cycle>,
    pub columns: Seq<literal::AliasName, punct::Comma>,
    pub _set: PhantomData<keyword::Set>,
    pub set_column: CycleSetColumn,
    pub _using: PhantomData<keyword::Using>,
    pub using_column: literal::AliasName,
}

/// SET column with optional TO/DEFAULT values.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CycleSetColumn {
    pub name: literal::AliasName,
    pub to_default: Option<CycleToDefault>,
}

/// TO value DEFAULT value
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CycleToDefault {
    pub _to: PhantomData<keyword::To>,
    pub to_value: Expr,
    pub _default: PhantomData<keyword::Default>,
    pub default_value: Expr,
}

/// A single CTE definition: `name [(col, ...)] AS [MATERIALIZED|NOT MATERIALIZED] (query)
///   [SEARCH ...] [CYCLE ...]`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CteDefinition {
    pub name: literal::AliasName,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub _as: PhantomData<keyword::As>,
    pub materialized: Option<MaterializedOption>,
    pub query: Surrounded<punct::LParen, Box<crate::ast::Statement>, punct::RParen>,
    pub search: Option<SearchClause>,
    pub cycle: Option<CycleClause>,
}

/// WITH clause: `WITH [RECURSIVE] cte_def, ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithClause {
    pub _with: PhantomData<keyword::With>,
    pub recursive: Option<PhantomData<keyword::Recursive>>,
    pub ctes: Seq<CteDefinition, punct::Comma, NoTrailing, NonEmpty>,
}

/// WITH statement: WITH clause followed by a body statement.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WithStatement {
    pub with_clause: WithClause,
    pub body: Box<crate::ast::Statement>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_simple_with() {
        let mut input = Input::new("WITH q1(x,y) AS (SELECT 1,2) SELECT * FROM q1");
        let stmt = WithStatement::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.with_clause.ctes.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_with_recursive() {
        let mut input = Input::new(
            "WITH RECURSIVE t(n) AS (VALUES (1) UNION ALL SELECT n+1 FROM t WHERE n < 100) SELECT sum(n) FROM t",
        );
        let stmt = WithStatement::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.with_clause.recursive.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_with_materialized() {
        let mut input = Input::new(
            "WITH x AS MATERIALIZED (SELECT unique1 FROM tenk1) SELECT count(*) FROM tenk1 a WHERE unique1 IN (SELECT * FROM x)",
        );
        let stmt = WithStatement::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(
            stmt.with_clause.ctes[0].materialized,
            Some(MaterializedOption::Materialized(_))
        ));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_with_multiple_ctes() {
        let mut input = Input::new(
            "WITH RECURSIVE y (id) AS (VALUES (1)), x (id) AS (SELECT * FROM y UNION ALL SELECT id+1 FROM x WHERE id < 5) SELECT * FROM x",
        );
        let stmt = WithStatement::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.with_clause.ctes.len(), 2);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_with_search_depth_first() {
        let sql = "WITH RECURSIVE search_graph(f, t, label) AS (SELECT * FROM graph0 g UNION ALL SELECT g.* FROM graph0 g, search_graph sg WHERE g.f = sg.t) SEARCH DEPTH FIRST BY f, t SET seq SELECT * FROM search_graph";
        let mut input = Input::new(sql);
        let stmt = WithStatement::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.with_clause.ctes[0].search.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_with_cycle() {
        let sql = "WITH RECURSIVE search_graph(f, t, label) AS (SELECT * FROM graph g UNION ALL SELECT g.* FROM graph g, search_graph sg WHERE g.f = sg.t) CYCLE f, t SET is_cycle USING path SELECT * FROM search_graph";
        let mut input = Input::new(sql);
        let stmt = WithStatement::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.with_clause.ctes[0].cycle.is_some());
        assert!(input.is_empty());
    }
}
