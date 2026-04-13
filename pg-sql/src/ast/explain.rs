/// EXPLAIN statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// An explain option value: ON, OFF, or identifier.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ExplainOptValue {
    On(keyword::On),
    Off(keyword::Off),
    Ident(literal::Ident),
}

/// A single explain option: `name value` (e.g., `costs off`).
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExplainOption {
    pub name: literal::AliasName,
    pub value: Option<ExplainOptValue>,
}

/// Explain options: `(opt, ...)`.
pub type ExplainOptions =
    Surrounded<punct::LParen, Seq<ExplainOption, punct::Comma>, punct::RParen>;

/// EXPLAIN statement: `EXPLAIN [(options)] statement`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ExplainStmt {
    pub _explain: PhantomData<keyword::Explain>,
    pub options: Option<ExplainOptions>,
    pub body: Box<crate::ast::Statement>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::explain::ExplainStmt;
    use crate::rules::SqlRules;

    #[test]
    fn parse_explain_costs_off() {
        let mut input = Input::new("explain (costs off) select * from t");
        let stmt = ExplainStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.options.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_explain_multiple_options() {
        let mut input =
            Input::new("explain (costs off, analyze on, timing off, summary off) select * from t");
        let stmt = ExplainStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.options.is_some());
        assert!(input.is_empty());
    }
}
