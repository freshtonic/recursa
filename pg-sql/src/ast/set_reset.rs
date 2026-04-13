/// SET/RESET statement AST.
use std::marker::PhantomData;

use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// The value in a SET statement: identifier, string literal, or keyword ON/OFF.
#[derive(Debug, Clone, FormatTokens, Parse, Visit, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, FormatTokens, Parse, Visit, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[parse(rules = SqlRules)]
pub enum SetSep {
    To(keyword::To),
    Eq(punct::Eq),
}

/// SET statement: `SET [LOCAL] param TO|= value`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[parse(rules = SqlRules)]
pub struct SetStmt {
    pub _set: PhantomData<keyword::Set>,
    pub _local: Option<PhantomData<keyword::Local>>,
    pub param: literal::AliasName,
    pub sep: SetSep,
    pub value: SetValue,
}

/// RESET statement: `RESET param`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
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
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.param.0, "enable_seqscan");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_eq() {
        let mut input = Input::new("SET enable_sort = false");
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.param.0, "enable_sort");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reset() {
        let mut input = Input::new("RESET enable_seqscan");
        let stmt = ResetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.param.0, "enable_seqscan");
        assert!(input.is_empty());
    }
}
