/// CREATE FUNCTION / DROP FUNCTION statement AST.
use std::marker::PhantomData;

use recursa::seq::{OptionalTrailing, Seq};
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::ast::expr::{CastType, Expr, TypeName};
use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};
use recursa_diagram::railroad;

/// SETOF type: `SETOF typename`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetofReturn<'input> {
    pub _setof: PhantomData<keyword::Setof>,
    pub type_name: TypeName<'input>,
}

/// Function return type: `SETOF type` or plain `type`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ReturnType<'input> {
    Setof(SetofReturn<'input>),
    Plain(TypeName<'input>),
}

/// LANGUAGE clause: `LANGUAGE name`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct LanguageOption<'input> {
    pub _language: PhantomData<keyword::Language>,
    pub name: literal::AliasName<'input>,
}

/// Function body: either single-quoted string or dollar-quoted string.
///
/// Variant ordering: dollar-quoted before single-quoted (different first chars).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncBody<'input> {
    Dollar(literal::DollarStringLit<'input>),
    String(literal::StringLit<'input>),
}

/// Function return type name -- extends TypeName with additional types
/// that are valid as function return types (e.g., `trigger`), and allows
/// array suffixes via `CastType`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncReturnTypeName<'input> {
    Trigger(keyword::Trigger),
    Base(CastType<'input>),
}

/// RETURNS clause for functions: `RETURNS [SETOF] type`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncReturnsClause<'input> {
    pub _returns: PhantomData<keyword::Returns>,
    pub return_type: FuncReturnType<'input>,
}

/// Function return type: SETOF type, or plain type.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncReturnType<'input> {
    Setof(FuncSetofReturn<'input>),
    Plain(FuncReturnTypeName<'input>),
}

/// SETOF type for function returns.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncSetofReturn<'input> {
    pub _setof: PhantomData<keyword::Setof>,
    pub type_name: FuncReturnTypeName<'input>,
}

// --- Function parameters ---

/// Argument mode prefix: `IN | OUT | INOUT | VARIADIC`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ArgMode {
    In(keyword::In),
    Inout(keyword::Inout),
    Out(keyword::Out),
    Variadic(keyword::Variadic),
}

/// `[mode] name type [default]` -- a named function parameter.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct NamedFuncParam<'input> {
    pub mode: Option<ArgMode>,
    pub name: literal::Ident<'input>,
    pub type_name: CastType<'input>,
    pub default: Option<ParamDefault<'input>>,
}

/// `[mode] type [default]` -- an unnamed function parameter.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct UnnamedFuncParam<'input> {
    pub mode: Option<ArgMode>,
    pub type_name: CastType<'input>,
    pub default: Option<ParamDefault<'input>>,
}

/// Default value separator: `DEFAULT` or `=`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ParamDefaultSep {
    Default(keyword::Default),
    Eq(punct::Eq),
}

/// `DEFAULT expr` or `= expr` trailing default on a function parameter.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ParamDefault<'input> {
    pub sep: ParamDefaultSep,
    pub value: Expr<'input>,
}

/// A single function parameter: either `[mode] name type` or `[mode] type`.
///
/// Variant ordering: `Named` (`[mode] ident type`) is longer than `Unnamed`
/// (`[mode] type`); list it first so longest-match-wins picks it when both
/// could parse.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncParam<'input> {
    Named(NamedFuncParam<'input>),
    Unnamed(UnnamedFuncParam<'input>),
}

// --- Function options (unordered list) ---

/// `IMMUTABLE` / `STABLE` / `VOLATILE` volatility.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum VolatilityOption {
    Immutable(keyword::Immutable),
    Stable(keyword::Stable),
    Volatile(keyword::Volatile),
}

/// `PARALLEL SAFE` / `PARALLEL RESTRICTED` / `PARALLEL UNSAFE` parallelism
/// declaration.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ParallelMode {
    Safe(keyword::SafeKw),
    Restricted(keyword::RestrictedKw),
    Unsafe(keyword::UnsafeKw),
}

/// `PARALLEL { SAFE | RESTRICTED | UNSAFE }` function option.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ParallelOption {
    pub _parallel: PhantomData<keyword::ParallelKw>,
    pub mode: ParallelMode,
}

/// Separator between a SET config parameter name and its value — either
/// `=` or `TO`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetAssignSep {
    Eq(punct::Eq),
    To(keyword::To),
}

/// `SET config_param { = | TO } value` function option — per-function GUC
/// override applied when the function runs.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetFuncOption<'input> {
    pub _set: PhantomData<keyword::Set>,
    pub name: literal::AliasName<'input>,
    pub sep: SetAssignSep,
    pub value: crate::ast::set_reset::SetValue<'input>,
}

/// `CALLED ON NULL INPUT`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CalledOnNullInput {
    pub _called: PhantomData<keyword::Called>,
    pub _on: PhantomData<keyword::On>,
    pub _null: PhantomData<keyword::Null>,
    pub _input: PhantomData<keyword::Input>,
}

/// `RETURNS NULL ON NULL INPUT`.
#[railroad]
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
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum StrictnessOption {
    CalledOnNullInput(CalledOnNullInput),
    ReturnsNullOnNullInput(ReturnsNullOnNullInput),
    Strict(keyword::Strict),
}

/// `AS body` clause.
#[railroad]
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
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum FuncOption<'input> {
    Strictness(StrictnessOption),
    Volatility(VolatilityOption),
    Parallel(ParallelOption),
    Set(SetFuncOption<'input>),
    Language(LanguageOption<'input>),
    As(AsOption<'input>),
}

/// CREATE [OR REPLACE] FUNCTION statement.
///
/// Function options after the signature/RETURNS may appear in any order.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CreateFunctionStmt<'input> {
    pub _create: PhantomData<keyword::Create>,
    pub or_replace: Option<crate::ast::create_view::OrReplaceKw>,
    pub _function: PhantomData<keyword::Function>,
    pub name: crate::ast::common::QualifiedName<'input>,
    pub args: Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>,
    pub returns: Option<FuncReturnsClause<'input>>,
    pub options: Seq<FuncOption<'input>, (), OptionalTrailing>,
}

/// A single entry in a `DROP FUNCTION` target list: optional qualified name
/// plus an optional parenthesized signature.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropFunctionTarget<'input> {
    pub name: crate::ast::common::QualifiedName<'input>,
    pub args:
        Option<Surrounded<punct::LParen, Seq<FuncParam<'input>, punct::Comma>, punct::RParen>>,
}

/// DROP FUNCTION statement: `DROP FUNCTION name[(args)] [, name[(args)] ...]`.
///
/// The argument list on each target is optional: when the function name is
/// unambiguous in the current schema, Postgres allows omitting the signature.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct DropFunctionStmt<'input> {
    pub _drop: PhantomData<keyword::Drop>,
    pub _function: PhantomData<keyword::Function>,
    pub targets: Seq<DropFunctionTarget<'input>, punct::Comma>,
    pub behavior: Option<crate::ast::common::DropBehavior>,
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
        assert_eq!(stmt.name.object(), "sillysrf");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_function_basic() {
        let mut input = Input::new("drop function sillysrf(int)");
        let stmt = DropFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(
            stmt.targets.iter().next().unwrap().name.object(),
            "sillysrf"
        );
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_function_multi() {
        let mut input = Input::new("drop function a(), b(), c()");
        let _stmt = DropFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
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
    fn parse_drop_function_cascade() {
        let mut input = Input::new("DROP FUNCTION int4_casttesttype(int4) CASCADE");
        let _stmt = DropFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
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
        let mut input =
            Input::new("create function f() returns trigger language plpgsql as $$ begin end $$");
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
        let mut input =
            Input::new("create function f() returns int language sql strict as 'SELECT 1'");
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
    fn parse_create_function_param_eq_default() {
        let mut input = Input::new(
            "create function f(a int = 1, b int = 2) returns int as $$ select 1 $$ language sql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_param_default_keyword() {
        let mut input = Input::new(
            "create function f(a int default 1) returns int as $$ select 1 $$ language sql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_unnamed_default() {
        let mut input = Input::new(
            "create function dfunc(a int = 1, int = 2) returns int as $$ select 1 $$ language sql",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_array_arg() {
        let mut input =
            Input::new("CREATE FUNCTION stfnp(int[]) RETURNS int[] AS 'select $1' LANGUAGE SQL");
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_array_arg_multi() {
        let mut input = Input::new(
            "CREATE FUNCTION f(int[], text[]) RETURNS int[] AS 'select $1' LANGUAGE SQL",
        );
        let _stmt = CreateFunctionStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_function_nested_array() {
        let mut input =
            Input::new("CREATE FUNCTION f(x int[][]) RETURNS int[][] AS 'select x' LANGUAGE SQL");
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
