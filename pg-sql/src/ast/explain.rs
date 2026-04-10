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
///
/// Manual Parse impl needed because the body can be SELECT, WITH, INSERT,
/// UPDATE, DELETE, or MERGE -- not just SelectStmt.
/// To eliminate this, recursa would need a way to parse "any of these" inline.
#[derive(Debug, Clone, Visit)]
pub struct ExplainStmt {
    pub _explain: PhantomData<keyword::Explain>,
    pub options: Option<ExplainOptions>,
    pub body: Box<crate::ast::Statement>,
}

impl<'input> recursa::Parse<'input> for ExplainStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::Explain::first_pattern()
    }

    fn peek<R: recursa::ParseRules>(input: &recursa::Input<'input>, rules: &R) -> bool {
        keyword::Explain::peek(input, rules)
    }

    fn parse<R: recursa::ParseRules>(
        input: &mut recursa::Input<'input>,
        rules: &R,
    ) -> Result<Self, recursa::ParseError> {
        let _explain = PhantomData::<keyword::Explain>::parse(input, rules)?;
        R::consume_ignored(input);
        let options = Option::<ExplainOptions>::parse(input, rules)?;
        R::consume_ignored(input);
        let body = Box::new(crate::ast::Statement::parse(input, rules)?);
        Ok(ExplainStmt {
            _explain,
            options,
            body,
        })
    }
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
