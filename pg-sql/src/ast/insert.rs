/// INSERT INTO statement AST.
///
/// Supports: `INSERT INTO table [(cols)] source [ON CONFLICT ...] [RETURNING ...]`
/// where source is DEFAULT VALUES, VALUES rows, or SELECT query.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::expr::Expr;
use crate::ast::select::WhereClause;
use crate::ast::update::{ReturningClause, SetAssignment};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// DEFAULT VALUES variant.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DefaultValues {
    pub _default: PhantomData<keyword::Default>,
    pub _values: PhantomData<keyword::Values>,
}

/// Multiple value rows: `VALUES (row1), (row2), ...`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertValueRows {
    pub _values: PhantomData<keyword::Values>,
    pub rows: Seq<ValueList, punct::Comma>,
}

/// Insert source: DEFAULT VALUES, VALUES (row), ..., or SELECT query.
///
/// Manual Parse impl needed because DEFAULT VALUES and VALUES share
/// the VALUES keyword but DEFAULT VALUES must be checked first.
/// The SELECT variant allows a full compound query as insert source.
/// To eliminate this, recursa would need contextual keyword disambiguation.
#[derive(Debug, Clone)]
pub enum InsertSource {
    Default(DefaultValues),
    Rows(InsertValueRows),
    Select(Box<crate::ast::values::CompoundQuery>),
}

impl recursa::visitor::AsNodeKey for InsertSource {}

impl Visit for InsertSource {
    fn visit<V: recursa::visitor::TotalVisitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for InsertSource {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::Default::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        keyword::Default::peek(input, rules)
            || keyword::Values::peek(input, rules)
            || keyword::Select::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        if keyword::Default::peek(input, rules) {
            let dv = DefaultValues::parse(input, rules)?;
            Ok(InsertSource::Default(dv))
        } else if keyword::Values::peek(input, rules) {
            let rows = InsertValueRows::parse(input, rules)?;
            Ok(InsertSource::Rows(rows))
        } else {
            let query = Box::new(crate::ast::values::CompoundQuery::parse(input, rules)?);
            Ok(InsertSource::Select(query))
        }
    }
}

/// ON CONFLICT DO UPDATE details.
#[derive(Debug, Clone)]
pub struct DoUpdateAction {
    pub assignments: Seq<SetAssignment, punct::Comma>,
    pub where_clause: Option<WhereClause>,
}

impl recursa::visitor::AsNodeKey for DoUpdateAction {}
impl Visit for DoUpdateAction {
    fn visit<V: recursa::visitor::TotalVisitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

/// ON CONFLICT action: DO UPDATE SET ... [WHERE ...] or DO NOTHING
#[derive(Debug, Clone)]
pub enum ConflictAction {
    DoUpdate(Box<DoUpdateAction>),
    DoNothing,
}

impl recursa::visitor::AsNodeKey for ConflictAction {}

impl Visit for ConflictAction {
    fn visit<V: recursa::visitor::TotalVisitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

/// ON CONFLICT clause: `ON CONFLICT [(col, ...)] DO UPDATE SET ... | DO NOTHING`
///
/// Manual Parse impl needed because the conflict target (column list) is optional
/// and the DO UPDATE/DO NOTHING alternatives require lookahead.
/// To eliminate this, recursa would need multi-keyword alternative dispatch.
#[derive(Debug, Clone)]
pub struct OnConflictClause {
    pub target:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub action: ConflictAction,
}

impl recursa::visitor::AsNodeKey for OnConflictClause {}

impl Visit for OnConflictClause {
    fn visit<V: recursa::visitor::TotalVisitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for OnConflictClause {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::On::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        if !keyword::On::peek(input, rules) {
            return false;
        }
        let mut fork = input.fork();
        if keyword::On::parse(&mut fork, rules).is_err() {
            return false;
        }
        R::consume_ignored(&mut fork);
        keyword::Conflict::peek(&fork, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        PhantomData::<keyword::On>::parse(input, rules)?;
        R::consume_ignored(input);
        PhantomData::<keyword::Conflict>::parse(input, rules)?;
        R::consume_ignored(input);

        // Optional target columns
        let target = if punct::LParen::peek(input, rules) {
            Some(Surrounded::parse(input, rules)?)
        } else {
            None
        };
        R::consume_ignored(input);

        PhantomData::<keyword::Do>::parse(input, rules)?;
        R::consume_ignored(input);

        let action = if keyword::Nothing::peek(input, rules) {
            PhantomData::<keyword::Nothing>::parse(input, rules)?;
            ConflictAction::DoNothing
        } else {
            PhantomData::<keyword::Update>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Set>::parse(input, rules)?;
            R::consume_ignored(input);
            let assignments = Seq::<SetAssignment, punct::Comma>::parse(input, rules)?;
            R::consume_ignored(input);
            let where_clause = Option::<WhereClause>::parse(input, rules)?;
            ConflictAction::DoUpdate(Box::new(DoUpdateAction {
                assignments,
                where_clause,
            }))
        };

        Ok(OnConflictClause { target, action })
    }
}

/// INSERT INTO statement with optional ON CONFLICT and RETURNING.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct InsertStmt {
    pub _insert: PhantomData<keyword::Insert>,
    pub _into: PhantomData<keyword::Into>,
    pub table_name: literal::Ident,
    pub columns: Option<ColumnList>,
    pub source: InsertSource,
    pub on_conflict: Option<OnConflictClause>,
    pub returning: Option<ReturningClause>,
}

/// Column list: `(col1, col2, ...)`.
pub type ColumnList = Surrounded<punct::LParen, Seq<literal::Ident, punct::Comma>, punct::RParen>;

/// Value list: `(col1, col2, ...)`.
pub type ValueList = Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>;

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::insert::InsertStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_insert_with_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL1 (f1) VALUES (bool 't')");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.table_name.0, "BOOLTBL1");
        assert!(stmt.columns.is_some());
        assert_eq!(stmt.columns.as_ref().unwrap().inner.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_multiple_columns() {
        let mut input = Input::new("INSERT INTO BOOLTBL3 (d, b, o) VALUES ('true', true, 1)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.columns.as_ref().unwrap().inner.len(), 3);
    }

    #[test]
    fn parse_insert_without_columns() {
        let mut input = Input::new("INSERT INTO booltbl4 VALUES (false, true, null)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.columns.is_none());
    }

    #[test]
    fn parse_insert_default_values_returning() {
        let mut input = Input::new("INSERT INTO t DEFAULT VALUES RETURNING *");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt.source, super::InsertSource::Default(_)));
        assert!(stmt.returning.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_select() {
        let mut input = Input::new("INSERT INTO y SELECT generate_series(1, 10)");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(stmt.source, super::InsertSource::Select(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_on_conflict_do_nothing() {
        let mut input = Input::new("INSERT INTO t VALUES (1) ON CONFLICT (k) DO NOTHING");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.on_conflict.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_insert_on_conflict_do_update() {
        let mut input =
            Input::new("INSERT INTO t VALUES (1) ON CONFLICT (k) DO UPDATE SET v = 'updated'");
        let stmt = InsertStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.on_conflict.is_some());
        assert!(input.is_empty());
    }
}
