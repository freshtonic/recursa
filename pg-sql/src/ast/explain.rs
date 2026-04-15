/// EXPLAIN statement AST.
use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// An explain option value: ON, OFF, or identifier.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ExplainOptValue<'input> {
    On(ON),
    Off(OFF),
    Ident(literal::Ident<'input>),
}

/// A single explain option: `name value` (e.g., `costs off`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExplainOption<'input> {
    pub name: literal::AliasName<'input>,
    pub value: Option<ExplainOptValue<'input>>,
}

/// Explain options: `(opt, ...)`.
pub type ExplainOptions<'input> =
    Surrounded<punct::LParen, Seq<ExplainOption<'input>, punct::Comma>, punct::RParen>;

/// EXPLAIN statement: `EXPLAIN [(options)] statement`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExplainStmt<'input> {
    pub explain: EXPLAIN,
    pub options: Option<ExplainOptions<'input>>,
    pub body: Box<crate::ast::Statement<'input>>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::explain::ExplainStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_explain_costs_off() {
        let mut input = Input::new("explain (costs off) select * from t");
        let stmt = ExplainStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.options.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_explain_multiple_options() {
        let mut input =
            Input::new("explain (costs off, analyze on, timing off, summary off) select * from t");
        let stmt = ExplainStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.options.is_some());
        assert!(input.is_empty());
    }
}
