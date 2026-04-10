/// CREATE FUNCTION / DROP FUNCTION statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Parse, Visit};

use crate::ast::expr::TypeName;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// SETOF type: `SETOF typename`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetofReturn {
    pub _setof: PhantomData<keyword::Setof>,
    pub type_name: TypeName,
}

/// Function return type: `SETOF type` or plain `type`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ReturnType {
    Setof(SetofReturn),
    Plain(TypeName),
}

/// RETURNS clause: `RETURNS type`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReturnsClause {
    pub _returns: PhantomData<keyword::Returns>,
    pub return_type: ReturnType,
}

/// LANGUAGE clause: `LANGUAGE name`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LanguageClause {
    pub _language: PhantomData<keyword::Language>,
    pub name: literal::AliasName,
}

/// IMMUTABLE attribute.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ImmutableAttr(pub PhantomData<keyword::Immutable>);

/// CREATE FUNCTION statement.
///
/// `CREATE FUNCTION name(args) RETURNS type AS 'body' LANGUAGE name [IMMUTABLE]`
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateFunctionStmt {
    pub _create: PhantomData<keyword::Create>,
    pub _function: PhantomData<keyword::Function>,
    pub name: literal::Ident,
    pub args: Surrounded<punct::LParen, Seq<TypeName, punct::Comma>, punct::RParen>,
    pub returns: ReturnsClause,
    pub _as: PhantomData<keyword::As>,
    pub body: literal::StringLit,
    pub language: LanguageClause,
    pub immutable: Option<ImmutableAttr>,
}

/// DROP FUNCTION statement: `DROP FUNCTION name(args)`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropFunctionStmt {
    pub _drop: PhantomData<keyword::Drop>,
    pub _function: PhantomData<keyword::Function>,
    pub name: literal::Ident,
    pub args: Surrounded<punct::LParen, Seq<TypeName, punct::Comma>, punct::RParen>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_function::{CreateFunctionStmt, DropFunctionStmt};
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_function() {
        let mut input = Input::new(
            "create function sillysrf(int) returns setof int as 'values (1),(10),(2),($1)' language sql immutable",
        );
        let stmt = CreateFunctionStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "sillysrf");
        assert!(stmt.immutable.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_function() {
        let mut input = Input::new("drop function sillysrf(int)");
        let stmt = DropFunctionStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "sillysrf");
        assert!(input.is_empty());
    }
}
