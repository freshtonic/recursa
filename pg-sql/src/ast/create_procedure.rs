/// CREATE PROCEDURE / DROP PROCEDURE / CALL statement AST.
use std::marker::PhantomData;

use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::create_function::{FuncOption, FuncParam};
use crate::ast::expr::Expr;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};
use recursa_diagram::railroad;

/// CREATE [OR REPLACE] PROCEDURE name ( [ parameters ] ) options...
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateProcedureStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub or_replace: Option<crate::ast::create_view::OrReplaceKw>,
    pub _procedure: PhantomData<keyword::Procedure>,
    pub name: literal::Ident<'input>,
    pub args: Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>,
    pub options: Seq<FuncOption<'input>, (), OptionalTrailing>,
}

/// DROP PROCEDURE name [(args)]
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropProcedureStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _procedure: PhantomData<keyword::Procedure>,
    pub name: literal::Ident<'input>,
    pub args:
        Option<Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>>,
}

/// CALL name ( [ argument ] [, ...] )
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CallStmt<'input> {
    pub _call: PhantomData<keyword::Call>,
    pub name: literal::Ident<'input>,
    pub args: Surrounded<punct::LParen, Seq<Expr<'input>, punct::Comma>, punct::RParen>,
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
