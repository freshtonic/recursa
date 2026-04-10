/// CREATE FUNCTION / DROP FUNCTION statement AST.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

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
///
/// Manual Parse impl needed because optional OR REPLACE prefix and
/// dollar-quoted body require special handling.
/// To eliminate this, recursa would need multi-keyword optional prefix chains.
/// Manual Visit impl needed because `or_replace: bool` doesn't implement Visit.
/// To eliminate this, recursa would need `#[visit(skip)]` field attribute support.
#[derive(Debug, Clone)]
pub struct CreateFunctionStmt {
    pub or_replace: bool,
    pub name: literal::Ident,
    pub args: Surrounded<punct::LParen, Seq<TypeName, punct::Comma>, punct::RParen>,
    pub returns: FuncReturnsClause,
    pub body: FuncBody,
    pub language: LanguageClause,
    pub immutable: Option<ImmutableAttr>,
}

impl recursa::visitor::AsNodeKey for CreateFunctionStmt {}

impl Visit for CreateFunctionStmt {
    fn visit<V: recursa::visitor::Visitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for CreateFunctionStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // CREATE [OR REPLACE] FUNCTION
        static PATTERN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        PATTERN.get_or_init(|| {
            r"(?i:CREATE\b)(?:\s+(?i:OR\b)\s+(?i:REPLACE\b))?\s+(?i:FUNCTION\b)".to_string()
        })
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            regex::Regex::new(&format!(r"\A(?:{})", Self::first_pattern())).unwrap()
        });
        re.is_match(input.remaining())
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        PhantomData::<keyword::Create>::parse(input, rules)?;
        R::consume_ignored(input);

        let or_replace = if keyword::Or::peek(input, rules) {
            PhantomData::<keyword::Or>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Replace>::parse(input, rules)?;
            R::consume_ignored(input);
            true
        } else {
            false
        };

        PhantomData::<keyword::Function>::parse(input, rules)?;
        R::consume_ignored(input);
        let name = literal::Ident::parse(input, rules)?;
        R::consume_ignored(input);
        let args = Surrounded::parse(input, rules)?;
        R::consume_ignored(input);
        let returns = FuncReturnsClause::parse(input, rules)?;
        R::consume_ignored(input);
        PhantomData::<keyword::As>::parse(input, rules)?;
        R::consume_ignored(input);
        let body = FuncBody::parse(input, rules)?;
        R::consume_ignored(input);
        let language = LanguageClause::parse(input, rules)?;
        R::consume_ignored(input);
        let immutable = Option::<ImmutableAttr>::parse(input, rules)?;

        Ok(CreateFunctionStmt {
            or_replace,
            name,
            args,
            returns,
            body,
            language,
            immutable,
        })
    }
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
