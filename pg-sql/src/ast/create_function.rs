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

/// Function body: either single-quoted string or dollar-quoted string.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncBody {
    Dollar(literal::DollarStringLit),
    String(literal::StringLit),
}

/// Function return type name -- extends TypeName with additional types
/// that are valid as function return types (e.g., `trigger`, `void`).
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncReturnTypeName {
    Trigger(keyword::Trigger),
    Base(TypeName),
}

/// RETURNS clause for functions: `RETURNS [SETOF] type`.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncReturnsClause {
    pub _returns: PhantomData<keyword::Returns>,
    pub return_type: FuncReturnType,
}

/// Function return type: SETOF type, VOID, or plain type.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncReturnType {
    Setof(FuncSetofReturn),
    Plain(FuncReturnTypeName),
}

/// SETOF type for function returns.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncSetofReturn {
    pub _setof: PhantomData<keyword::Setof>,
    pub type_name: FuncReturnTypeName,
}

/// CREATE [OR REPLACE] FUNCTION statement.
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateFunctionStmt {
    pub _create: PhantomData<keyword::Create>,
    pub or_replace: Option<crate::ast::create_view::OrReplaceKw>,
    pub _function: PhantomData<keyword::Function>,
    pub name: literal::Ident,
    pub args: Surrounded<punct::LParen, Seq<TypeName, punct::Comma>, punct::RParen>,
    pub returns: FuncReturnsClause,
    pub _as: PhantomData<keyword::As>,
    pub body: FuncBody,
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
