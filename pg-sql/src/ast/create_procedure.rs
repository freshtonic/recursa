/// CREATE PROCEDURE / DROP PROCEDURE / CALL statement AST.
use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::create_function::{FuncOption, FuncParam};
use crate::ast::expr::FuncArg;
use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// CREATE [OR REPLACE] PROCEDURE name ( [ parameters ] ) options...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateProcedureStmt<'input> {
    pub create: CREATE,
    pub or_replace: Option<(OR, REPLACE)>,
    pub procedure: PROCEDURE,
    pub name: literal::Ident<'input>,
    pub args: Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>,
    pub options: Seq<FuncOption<'input>, (), OptionalTrailing>,
}

/// DROP PROCEDURE name [(args)]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropProcedureStmt<'input> {
    pub drop: DROP,
    pub procedure: PROCEDURE,
    pub if_exists: Option<(IF, EXISTS)>,
    pub name: crate::ast::common::QualifiedName<'input>,
    pub args:
        Option<Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>>,
    pub behavior: Option<crate::ast::common::DropBehavior>,
}

/// CALL name ( [ argument ] [, ...] )
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CallStmt<'input> {
    pub call: CALL,
    pub name: literal::Ident<'input>,
    pub args: Surrounded<punct::LParen, Seq<FuncArg<'input>, punct::Comma>, punct::RParen>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use super::*;

    #[test]
    fn parse_create_procedure_basic() {
        let mut input = Input::new(
            "CREATE PROCEDURE ptest1(x text) LANGUAGE SQL AS $$ INSERT INTO cp_test VALUES (1, x); $$",
        );
        let _stmt = CreateProcedureStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_call_basic() {
        let mut input = Input::new("CALL ptest1('a')");
        let _stmt = CallStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_call_concat_arg() {
        let mut input = Input::new("CALL ptest1('xy' || 'zzy')");
        let _stmt = CallStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_call_no_args() {
        let mut input = Input::new("CALL nonexistent()");
        let _stmt = CallStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_procedure() {
        let mut input = Input::new("DROP PROCEDURE ptest1");
        let _stmt = DropProcedureStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
