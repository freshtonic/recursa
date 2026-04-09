/// SELECT statement AST.
use std::ops::ControlFlow;

use recursa::seq::Seq;
use recursa::{AsNodeKey, Break, Input, Parse, ParseError, ParseRules, Visit, Visitor};

use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens;

/// A single item in the SELECT list: `expr [AS alias]`.
#[derive(Debug, Clone)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<Alias>,
}

/// AS alias clause.
///
/// The alias name can be an identifier or a keyword (e.g., `AS true`, `AS false`).
#[derive(Debug, Clone)]
pub struct Alias {
    pub as_kw: tokens::As,
    /// Alias name stored as a plain string since SQL allows keywords as aliases.
    pub name: String,
}

/// FROM clause: `FROM table [, table ...]`.
#[derive(Debug)]
pub struct FromClause {
    pub from_kw: tokens::From,
    pub tables: Seq<TableRef, tokens::Comma>,
}

/// A table reference: either a plain table name or a function call.
#[derive(Debug, Clone)]
pub enum TableRef {
    /// Simple table name: `BOOLTBL1`
    Table(tokens::Ident),
    /// Function call in FROM: `pg_input_error_info('junk', 'bool')`
    FuncCall {
        name: tokens::Ident,
        args: Vec<Expr>,
    },
}

/// WHERE clause: `WHERE expr`.
#[derive(Debug, Clone)]
pub struct WhereClause {
    pub where_kw: tokens::Where,
    pub condition: Expr,
}

/// ORDER BY clause: `ORDER BY expr [, expr ...]`.
#[derive(Debug)]
pub struct OrderByClause {
    pub order_kw: tokens::Order,
    pub by_kw: tokens::By,
    pub items: Seq<Expr, tokens::Comma>,
}

/// SELECT statement.
#[derive(Debug)]
pub struct SelectStmt {
    pub select_kw: tokens::Select,
    pub items: Vec<SelectItem>,
    pub from_clause: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub order_by: Option<OrderByClause>,
}

// --- Parse implementations ---

impl AsNodeKey for Alias {}
impl Visit for Alias {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.as_kw.visit(visitor)?;
        self.name.visit(visitor)?;
        visitor.exit(self)
    }
}

/// Regex for any SQL word (identifier or keyword).
fn alias_name_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\A[a-zA-Z_][a-zA-Z0-9_]*").unwrap())
}

impl<'input> Parse<'input> for Alias {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::As as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::As as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let as_kw = <tokens::As as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        // Accept any word (identifier or keyword) as an alias name
        let re = alias_name_regex();
        match re.find(input.remaining()) {
            Some(m) => {
                let name = m.as_str().to_string();
                input.advance(m.len());
                Ok(Alias { as_kw, name })
            }
            None => Err(ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                "alias name",
            )),
        }
    }
}

impl AsNodeKey for SelectItem {}
impl Visit for SelectItem {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.expr.visit(visitor)?;
        self.alias.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for SelectItem {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        Expr::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        Expr::peek(input, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        let expr = Expr::parse(input, &SqlRules)?;
        let alias = Option::<Alias>::parse(input, &SqlRules)?;
        Ok(SelectItem { expr, alias })
    }
}

impl AsNodeKey for TableRef {}
impl Visit for TableRef {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        match self {
            TableRef::Table(name) => name.visit(visitor)?,
            TableRef::FuncCall { name, args } => {
                name.visit(visitor)?;
                for arg in args {
                    arg.visit(visitor)?;
                }
            }
        }
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for TableRef {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Ident as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Ident as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let name = <tokens::Ident as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        // Check for function call: ident(...)
        if <tokens::LParen as Parse>::peek(input, &SqlRules) {
            <tokens::LParen as Parse>::parse(input, &SqlRules)?;
            let mut args = Vec::new();
            SqlRules::consume_ignored(input);
            if !<tokens::RParen as Parse>::peek(input, &SqlRules) {
                args.push(Expr::parse(input, &SqlRules)?);
                loop {
                    SqlRules::consume_ignored(input);
                    if !<tokens::Comma as Parse>::peek(input, &SqlRules) {
                        break;
                    }
                    <tokens::Comma as Parse>::parse(input, &SqlRules)?;
                    args.push(Expr::parse(input, &SqlRules)?);
                }
            }
            SqlRules::consume_ignored(input);
            <tokens::RParen as Parse>::parse(input, &SqlRules)?;
            return Ok(TableRef::FuncCall { name, args });
        }
        Ok(TableRef::Table(name))
    }
}

impl AsNodeKey for FromClause {}
impl Visit for FromClause {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.from_kw.visit(visitor)?;
        self.tables.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for FromClause {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::From as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::From as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let from_kw = <tokens::From as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let tables = Seq::<TableRef, tokens::Comma>::parse(input, &SqlRules)?;
        Ok(FromClause { from_kw, tables })
    }
}

impl AsNodeKey for WhereClause {}
impl Visit for WhereClause {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.where_kw.visit(visitor)?;
        self.condition.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for WhereClause {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Where as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Where as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let where_kw = <tokens::Where as Parse>::parse(input, &SqlRules)?;
        let condition = Expr::parse(input, &SqlRules)?;
        Ok(WhereClause {
            where_kw,
            condition,
        })
    }
}

impl AsNodeKey for OrderByClause {}
impl Visit for OrderByClause {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.order_kw.visit(visitor)?;
        self.by_kw.visit(visitor)?;
        self.items.visit(visitor)?;
        visitor.exit(self)
    }
}

impl<'input> Parse<'input> for OrderByClause {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Order as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Order as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let order_kw = <tokens::Order as Parse>::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let by_kw = <tokens::By as Parse>::parse(input, &SqlRules)?;
        let items = Seq::<Expr, tokens::Comma>::parse(input, &SqlRules)?;
        Ok(OrderByClause {
            order_kw,
            by_kw,
            items,
        })
    }
}

impl AsNodeKey for SelectStmt {}
impl Visit for SelectStmt {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        self.select_kw.visit(visitor)?;
        for item in &self.items {
            item.visit(visitor)?;
        }
        self.from_clause.visit(visitor)?;
        self.where_clause.visit(visitor)?;
        self.order_by.visit(visitor)?;
        visitor.exit(self)
    }
}

/// Parse a comma-separated list of select items.
/// Stops when encountering FROM, WHERE, ORDER, or semicolon.
fn parse_select_items(input: &mut Input<'_>) -> Result<Vec<SelectItem>, ParseError> {
    let mut items = Vec::new();

    items.push(SelectItem::parse(input, &SqlRules)?);

    loop {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        if !<tokens::Comma as Parse>::peek(&fork, &SqlRules) {
            break;
        }
        <tokens::Comma as Parse>::parse(&mut fork, &SqlRules)?;
        input.commit(fork);
        items.push(SelectItem::parse(input, &SqlRules)?);
    }

    Ok(items)
}

impl<'input> Parse<'input> for SelectStmt {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        <tokens::Select as Parse>::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        <tokens::Select as Parse>::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let select_kw = <tokens::Select as Parse>::parse(input, &SqlRules)?;
        let items = parse_select_items(input)?;
        let from_clause = Option::<FromClause>::parse(input, &SqlRules)?;
        let where_clause = Option::<WhereClause>::parse(input, &SqlRules)?;
        let order_by = Option::<OrderByClause>::parse(input, &SqlRules)?;
        Ok(SelectStmt {
            select_kw,
            items,
            from_clause,
            where_clause,
            order_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::select::SelectStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_select_literal() {
        let mut input = Input::new("SELECT 1 AS one");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(stmt.items[0].alias.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_select_star() {
        let mut input = Input::new("SELECT *");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
    }

    #[test]
    fn parse_select_from() {
        let mut input = Input::new("SELECT f1 FROM BOOLTBL1");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.from_clause.is_some());
        assert_eq!(stmt.from_clause.as_ref().unwrap().tables.len(), 1);
    }

    #[test]
    fn parse_select_from_where() {
        let mut input = Input::new("SELECT f1 FROM BOOLTBL1 WHERE f1 = true");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.from_clause.is_some());
        assert!(stmt.where_clause.is_some());
    }

    #[test]
    fn parse_select_qualified_wildcard_from() {
        let mut input = Input::new("SELECT BOOLTBL1.* FROM BOOLTBL1");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(stmt.from_clause.is_some());
    }

    #[test]
    fn parse_select_multiple_tables() {
        let mut input =
            Input::new("SELECT BOOLTBL1.*, BOOLTBL2.* FROM BOOLTBL1, BOOLTBL2 WHERE f1 = true");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 2);
        assert_eq!(stmt.from_clause.as_ref().unwrap().tables.len(), 2);
    }

    #[test]
    fn parse_select_order_by() {
        let mut input = Input::new(
            "SELECT BOOLTBL1.*, BOOLTBL2.* FROM BOOLTBL1, BOOLTBL2 ORDER BY BOOLTBL1.f1, BOOLTBL2.f1",
        );
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.order_by.is_some());
        assert_eq!(stmt.order_by.as_ref().unwrap().items.len(), 2);
    }

    #[test]
    fn parse_select_star_from_function() {
        let mut input = Input::new("SELECT * FROM pg_input_error_info('junk', 'bool')");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(stmt.from_clause.is_some());
        let tables = &stmt.from_clause.as_ref().unwrap().tables;
        assert_eq!(tables.len(), 1);
        assert!(matches!(tables[0], super::TableRef::FuncCall { .. }));
    }

    #[test]
    fn parse_select_bool_cast_as_alias() {
        let mut input = Input::new("SELECT bool 't' AS true_val");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(stmt.items[0].alias.is_some());
    }

    #[test]
    fn parse_select_is_true_as_alias() {
        // b IS TRUE AS istrue
        let mut input = Input::new("SELECT b IS TRUE AS istrue");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
        assert!(stmt.items[0].alias.is_some());
    }

    #[test]
    fn parse_select_multiple_is_tests_with_aliases() {
        let mut input = Input::new(
            "SELECT d, b IS TRUE AS istrue, b IS NOT TRUE AS isnottrue FROM booltbl3 ORDER BY o",
        );
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 3);
        assert!(stmt.order_by.is_some());
    }

    #[test]
    fn parse_select_or_with_type_cast() {
        let mut input = Input::new("SELECT bool 't' or bool 'f' AS true");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 1);
    }

    #[test]
    fn parse_select_cast_chain() {
        let mut input =
            Input::new("SELECT 'TrUe'::text::boolean AS true, 'fAlse'::text::boolean AS false");
        let stmt = SelectStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.items.len(), 2);
    }
}
