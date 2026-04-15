/// CREATE FUNCTION / DROP FUNCTION statement AST.
use std::marker::PhantomData;

use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::expr::TypeName;
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// SETOF type: `SETOF typename`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetofReturn<'input> {
    pub _setof: PhantomData<keyword::Setof>,
    pub type_name: TypeName<'input>,
}

/// Function return type: `SETOF type` or plain `type`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ReturnType<'input> {
    Setof(SetofReturn<'input>),
    Plain(TypeName<'input>),
}

/// LANGUAGE clause: `LANGUAGE name`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LanguageOption<'input> {
    pub _language: PhantomData<keyword::Language>,
    pub name: literal::AliasName<'input>,
}

/// Function body: either single-quoted string or dollar-quoted string.
///
/// Variant ordering: dollar-quoted before single-quoted (different first chars).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncBody<'input> {
    Dollar(literal::DollarStringLit<'input>),
    String(literal::StringLit<'input>),
}

/// Function return type name -- extends TypeName with additional types
/// that are valid as function return types (e.g., `trigger`).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncReturnTypeName<'input> {
    Trigger(keyword::Trigger),
    Base(TypeName<'input>),
}

/// RETURNS clause for functions: `RETURNS [SETOF] type`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncReturnsClause<'input> {
    pub _returns: PhantomData<keyword::Returns>,
    pub return_type: FuncReturnType<'input>,
}

/// Function return type: SETOF type, or plain type.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncReturnType<'input> {
    Setof(FuncSetofReturn<'input>),
    Plain(FuncReturnTypeName<'input>),
}

/// SETOF type for function returns.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncSetofReturn<'input> {
    pub _setof: PhantomData<keyword::Setof>,
    pub type_name: FuncReturnTypeName<'input>,
}

// --- Function parameters ---

/// Argument mode prefix: `IN | OUT | INOUT | VARIADIC`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ArgMode {
    In(keyword::In),
    Inout(keyword::Inout),
    Out(keyword::Out),
    Variadic(keyword::Variadic),
}

/// `[mode] name type` -- a named function parameter, optionally prefixed
/// with an argument mode (`IN`/`OUT`/`INOUT`/`VARIADIC`).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NamedFuncParam<'input> {
    pub mode: Option<ArgMode>,
    pub name: literal::Ident<'input>,
    pub type_name: TypeName<'input>,
}

/// `[mode] type` -- an unnamed function parameter with optional mode.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnnamedFuncParam<'input> {
    pub mode: Option<ArgMode>,
    pub type_name: TypeName<'input>,
}

/// A single function parameter: either `[mode] name type` or `[mode] type`.
///
/// Variant ordering: `Named` (`[mode] ident type`) is longer than `Unnamed`
/// (`[mode] type`); list it first so longest-match-wins picks it when both
/// could parse.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncParam<'input> {
    Named(NamedFuncParam<'input>),
    Unnamed(UnnamedFuncParam<'input>),
}

// --- Function options (unordered list) ---

/// `IMMUTABLE` / `STABLE` / `VOLATILE` volatility.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum VolatilityOption {
    Immutable(keyword::Immutable),
    Stable(keyword::Stable),
    Volatile(keyword::Volatile),
}

/// `CALLED ON NULL INPUT`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CalledOnNullInput {
    pub _called: PhantomData<keyword::Called>,
    pub _on: PhantomData<keyword::On>,
    pub _null: PhantomData<keyword::Null>,
    pub _input: PhantomData<keyword::Input>,
}

/// `RETURNS NULL ON NULL INPUT`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ReturnsNullOnNullInput {
    pub _returns: PhantomData<keyword::Returns>,
    pub _null: PhantomData<keyword::Null>,
    pub _on: PhantomData<keyword::On>,
    pub _null2: PhantomData<keyword::Null>,
    pub _input: PhantomData<keyword::Input>,
}

/// `STRICT` / `CALLED ON NULL INPUT` / `RETURNS NULL ON NULL INPUT`.
///
/// Variant ordering: longer (multi-keyword) forms before `Strict`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum StrictnessOption {
    CalledOnNullInput(CalledOnNullInput),
    ReturnsNullOnNullInput(ReturnsNullOnNullInput),
    Strict(keyword::Strict),
}

/// `AS body` clause.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct AsOption<'input> {
    pub _as: PhantomData<keyword::As>,
    pub body: FuncBody<'input>,
}

/// A single function option clause.
///
/// Variant ordering: multi-token options listed before single-keyword
/// options, and `StrictnessOption` (which itself has multi-keyword variants)
/// listed before plain `VolatilityOption`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncOption<'input> {
    Strictness(StrictnessOption),
    Volatility(VolatilityOption),
    Language(LanguageOption<'input>),
    As(AsOption<'input>),
}

/// CREATE [OR REPLACE] FUNCTION statement.
///
/// Function options after the signature/RETURNS may appear in any order.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateFunctionStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub or_replace: Option<crate::ast::create_view::OrReplaceKw>,
    pub _function: PhantomData<keyword::Function>,
    pub name: literal::Ident<'input>,
    pub args: Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>,
    pub returns: Option<FuncReturnsClause<'input>>,
    pub options: Seq<FuncOption<'input>, (), OptionalTrailing>,
}

/// DROP FUNCTION statement: `DROP FUNCTION name(args)`.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropFunctionStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _function: PhantomData<keyword::Function>,
    pub name: literal::Ident<'input>,
    pub args: Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::create_function::{CreateFunctionStmt, DropFunctionStmt};
    use crate::rules::SqlRules;

    #[test]
    fn parse_create_function_basic() {
        let mut input = Input::new(
            "create function sillysrf(int) returns setof int as 'values (1),(10),(2),($1)' language sql immutable",
        );
        let stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "sillysrf");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_function_basic() {
        let mut input = Input::new("drop function sillysrf(int)");
        let stmt = DropFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.name.text(), "sillysrf");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_named_param() {
        let mut input = Input::new(
            "create function polyf(x anyelement) returns anyelement as $$ select x + 1 $$ language sql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_function_named_param() {
        let mut input = Input::new("drop function polyf(x anyelement)");
        let _stmt = DropFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_returns_trigger() {
        let mut input = Input::new(
            "create function f() returns trigger language plpgsql as $$ begin end $$",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_strict_immutable() {
        let mut input = Input::new(
            "create function f() returns int immutable strict language sql as 'SELECT 1'",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_options_reordered() {
        let mut input = Input::new(
            "create function f() returns int language sql strict as 'SELECT 1'",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_in_out_named() {
        let mut input = Input::new(
            "create function f(in i int, out j int) returns int as $$ begin return i+1; end $$ language plpgsql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_in_out_no_returns() {
        let mut input = Input::new(
            "create function f(in i int, out j int) as $$ begin end $$ language plpgsql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_setof_record() {
        let mut input = Input::new(
            "create function gs(v integer, out a integer, out b integer) returns setof record as $f$ select 1 $f$ language plpgsql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_polymorphic_out() {
        let mut input = Input::new(
            "create function poly(a anyelement, b anyarray, OUT x anyarray) as $$ begin end $$ language plpgsql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_multi_named_params() {
        let mut input = Input::new(
            "create function tg_hub_adjustslots(hname bpchar, oldn integer, newn integer) returns integer as ' begin return 1; end ' language plpgsql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
