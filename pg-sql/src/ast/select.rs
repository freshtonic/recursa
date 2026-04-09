/// SELECT statement AST.
use recursa::seq::Seq;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens;

/// A single item in the SELECT list: `expr [AS alias]`.
///
/// Manual Parse: expression parsing followed by optional alias with custom
/// dispatch logic that the derive macro cannot express.
#[derive(Debug, Clone, Visit)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<Alias>,
}

/// AS alias clause.
///
/// Manual Parse: accepts keywords as alias names (e.g., `AS true`, `AS false`)
/// which requires raw regex matching instead of an Ident token.
#[derive(Debug, Clone, Visit)]
pub struct Alias {
    pub as_kw: tokens::As,
    /// Alias name stored as a plain string since SQL allows keywords as aliases.
    pub name: String,
}

/// FROM clause: `FROM table [, table ...]`.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FromClause {
    pub from_kw: tokens::From,
    pub tables: Seq<TableRef, tokens::Comma>,
}

/// A table reference: either a plain table name or a function call.
///
/// Manual Parse: requires lookahead after ident to distinguish
/// `table_name` from `func_name(args)`.
#[derive(Debug, Clone, Visit)]
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
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct WhereClause {
    pub where_kw: tokens::Where,
    pub condition: Expr,
}

/// ORDER BY clause: `ORDER BY expr [, expr ...]`.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct OrderByClause {
    pub order_kw: tokens::Order,
    pub by_kw: tokens::By,
    pub items: Seq<Expr, tokens::Comma>,
}

/// SELECT statement.
#[derive(Debug, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SelectStmt {
    pub select_kw: tokens::Select,
    pub items: Seq<SelectItem, tokens::Comma>,
    pub from_clause: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub order_by: Option<OrderByClause>,
}

// --- Parse implementations ---

/// Regex for any SQL word (identifier or keyword).
fn alias_name_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\A[a-zA-Z_][a-zA-Z0-9_]*").unwrap())
}

impl<'input> Parse<'input> for Alias {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        tokens::As::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        tokens::As::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let as_kw = tokens::As::parse(input, &SqlRules)?;
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

impl<'input> Parse<'input> for TableRef {
    const IS_TERMINAL: bool = false;
    fn first_pattern() -> &'static str {
        tokens::Ident::first_pattern()
    }
    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        tokens::Ident::peek(&fork, &SqlRules)
    }
    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let name = tokens::Ident::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        // Check for function call: ident(...)
        if tokens::LParen::peek(input, &SqlRules) {
            tokens::LParen::parse(input, &SqlRules)?;
            let mut args = Vec::new();
            SqlRules::consume_ignored(input);
            if !tokens::RParen::peek(input, &SqlRules) {
                args.push(Expr::parse(input, &SqlRules)?);
                loop {
                    SqlRules::consume_ignored(input);
                    if !tokens::Comma::peek(input, &SqlRules) {
                        break;
                    }
                    tokens::Comma::parse(input, &SqlRules)?;
                    args.push(Expr::parse(input, &SqlRules)?);
                }
            }
            SqlRules::consume_ignored(input);
            tokens::RParen::parse(input, &SqlRules)?;
            return Ok(TableRef::FuncCall { name, args });
        }
        Ok(TableRef::Table(name))
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
