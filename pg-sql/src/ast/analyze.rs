/// ANALYZE statement AST: `ANALYZE [table [(col, ...)]]`.
use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// ANALYZE statement with optional qualified table name and column list.
///
/// ```sql
/// ANALYZE [VERBOSE] [table_name [(column, ...)]]
/// ```
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AnalyzeStmt<'input> {
    pub analyze: ANALYZE,
    /// Optional `VERBOSE` keyword (legacy bareword form).
    pub verbose: Option<VERBOSE>,
    /// Optional parenthesized options list, e.g.
    /// `(VERBOSE, SKIP_LOCKED, BUFFER_USAGE_LIMIT '512 kB')`.
    pub options: Option<
        Surrounded<punct::LParen, Seq<AnalyzeOption<'input>, punct::Comma>, punct::RParen>,
    >,
    pub targets: Option<Seq<AnalyzeTarget<'input>, punct::Comma>>,
}

/// One option inside the parenthesized `ANALYZE (...)` options list.
///
/// Each option is a keyword-ish name (so we use `AliasName` to tolerate
/// identifiers that happen to collide with keywords) followed by an optional
/// value (string literal, integer, or ON/OFF-style AliasName).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AnalyzeOption<'input> {
    pub name: literal::AliasName<'input>,
    pub value: Option<AnalyzeOptionValue<'input>>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum AnalyzeOptionValue<'input> {
    String(literal::StringLit<'input>),
    Integer(literal::IntegerLit<'input>),
    Name(literal::AliasName<'input>),
}

/// `table_name [(column, ...)]` target of an ANALYZE statement.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AnalyzeTarget<'input> {
    pub table_name: crate::ast::common::QualifiedName<'input>,
    pub columns: Option<
        Surrounded<punct::LParen, Seq<literal::Ident<'input>, punct::Comma>, punct::RParen>,
    >,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::analyze::AnalyzeStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_analyze() {
        let mut input = Input::new("ANALYZE onek2");
        let stmt = AnalyzeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.targets.unwrap().first().unwrap().table_name.object(), "onek2");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_analyze_bare() {
        let mut input = Input::new("ANALYZE");
        let _stmt = AnalyzeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_analyze_columns() {
        let mut input = Input::new("ANALYZE atacc1(a, b)");
        let _stmt = AnalyzeStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
