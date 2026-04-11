/// SET/RESET statement AST.
use std::marker::PhantomData;

use recursa::{Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// The value in a SET statement: identifier, string literal, or keyword ON/OFF.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetValue {
    On(keyword::On),
    Off(keyword::Off),
    False(keyword::False),
    True(keyword::True),
    StringLit(literal::StringLit),
    Ident(literal::Ident),
}

/// The separator between param and value: TO or =.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetSep {
    To(keyword::To),
    Eq(punct::Eq),
}

/// SET statement: `SET [LOCAL] param TO|= value`.
///
/// Manual Parse impl needed because the optional LOCAL keyword is itself a keyword
/// that would be rejected by Ident, and the param name uses AliasName to allow
/// keywords as parameter names (e.g., SET LOCAL enable_seqscan = on).
/// To eliminate this, recursa would need keyword-tolerant identifiers.
/// Manual Visit impl needed because `local: bool` doesn't implement Visit.
/// To eliminate this, recursa would need `#[visit(skip)]` field attribute support.
#[derive(Debug, Clone)]
pub struct SetStmt {
    pub _set: PhantomData<keyword::Set>,
    pub local: bool,
    pub param: literal::AliasName,
    pub sep: SetSep,
    pub value: SetValue,
}

impl recursa::visitor::AsNodeKey for SetStmt {}

impl recursa::Visit for SetStmt {
    fn visit<V: recursa::visitor::TotalVisitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for SetStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        keyword::Set::first_pattern()
    }

    fn peek<R: recursa::ParseRules>(input: &recursa::Input<'input>, rules: &R) -> bool {
        keyword::Set::peek(input, rules)
    }

    fn parse<R: recursa::ParseRules>(
        input: &mut recursa::Input<'input>,
        rules: &R,
    ) -> Result<Self, recursa::ParseError> {
        let _set = PhantomData::<keyword::Set>::parse(input, rules)?;
        R::consume_ignored(input);

        let local = if keyword::Local::peek(input, rules) {
            PhantomData::<keyword::Local>::parse(input, rules)?;
            R::consume_ignored(input);
            true
        } else {
            false
        };

        let param = literal::AliasName::parse(input, rules)?;
        R::consume_ignored(input);
        let sep = SetSep::parse(input, rules)?;
        R::consume_ignored(input);
        let value = SetValue::parse(input, rules)?;

        Ok(SetStmt {
            _set,
            local,
            param,
            sep,
            value,
        })
    }
}

/// RESET statement: `RESET param`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ResetStmt {
    pub _reset: PhantomData<keyword::Reset>,
    pub param: literal::AliasName,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::set_reset::{ResetStmt, SetStmt};
    use crate::rules::SqlRules;

    #[test]
    fn parse_set_to() {
        let mut input = Input::new("SET enable_seqscan TO off");
        let stmt = SetStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.param.0, "enable_seqscan");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_eq() {
        let mut input = Input::new("SET enable_sort = false");
        let stmt = SetStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.param.0, "enable_sort");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reset() {
        let mut input = Input::new("RESET enable_seqscan");
        let stmt = ResetStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.param.0, "enable_seqscan");
        assert!(input.is_empty());
    }
}
