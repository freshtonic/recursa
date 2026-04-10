/// SQL expression AST with derived Pratt parsing for operator precedence.
///
/// Handles atoms, prefix (NOT), infix (AND, OR, comparisons), and
/// postfix operators (::type cast, IS [NOT] TRUE/FALSE/UNKNOWN/NULL).
use recursa::seq::Seq;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::rules::SqlRules;
use crate::tokens;

/// Type name for casts.
#[derive(Debug, Clone, PartialEq, Eq, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TypeName {
    Bool(tokens::Bool),
    Boolean(tokens::Boolean),
    Text(tokens::Text),
    Int(tokens::Int),
    Ident(tokens::Ident),
}

// --- Boolean test suffix structs ---
// NOT variants listed before non-NOT variants so the longer pattern wins via
// longest-match lookahead (e.g., "NOT TRUE" matches before "TRUE").

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotTrue(pub tokens::Not, pub tokens::True);

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotFalse(pub tokens::Not, pub tokens::False);

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotUnknown(pub tokens::Not, pub tokens::Unknown);

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotNull(pub tokens::Not, pub tokens::Null);

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsTrue(pub tokens::True);

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsFalse(pub tokens::False);

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsUnknown(pub tokens::Unknown);

#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNull(pub tokens::Null);

/// Boolean test suffix: the part after `IS` in `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`.
///
/// NOT variants are listed first so the combined peek regex disambiguates
/// via longest match (e.g., `NOT TRUE` is longer than `TRUE`).
#[derive(Debug, Clone, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum BoolTestKind {
    IsNotTrue(IsNotTrue),
    IsNotFalse(IsNotFalse),
    IsNotUnknown(IsNotUnknown),
    IsNotNull(IsNotNull),
    IsTrue(IsTrue),
    IsFalse(IsFalse),
    IsUnknown(IsUnknown),
    IsNull(IsNull),
}

// --- Atom wrapper structs ---

/// Qualified column reference: `table.column`
#[derive(Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct QualifiedRef {
    pub table: tokens::Ident,
    pub dot: tokens::Dot,
    pub column: tokens::Ident,
}

/// Qualified wildcard: `table.*`
#[derive(Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct QualifiedWildcard {
    pub table: tokens::Ident,
    pub dot: tokens::Dot,
    pub star: tokens::Star,
}

/// Function call: `name(arg1, arg2, ...)`
#[derive(Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct FuncCall {
    pub name: tokens::Ident,
    pub lparen: tokens::LParen,
    pub args: Seq<Expr, tokens::Comma>,
    pub rparen: tokens::RParen,
}

/// Parenthesized expression: `(expr)`
#[derive(Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ParenExpr {
    pub lparen: tokens::LParen,
    pub inner: Box<Expr>,
    pub rparen: tokens::RParen,
}

/// Function-style type cast: `bool 'value'`, `text 'hello'`
#[derive(Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct TypeCastFunc {
    pub type_name: TypeName,
    pub value: tokens::StringLit,
}

// --- Pratt expression enum ---

/// SQL expression with Pratt-derived parsing.
#[derive(Parse, Debug, Clone, Visit)]
#[parse(rules = SqlRules, pratt)]
pub enum Expr {
    // --- Prefix ---
    #[parse(prefix, bp = 15)]
    Not(tokens::Not, Box<Expr>),

    // --- Postfix ---
    /// Postgres-style cast: `expr::type`
    #[parse(postfix, bp = 20)]
    Cast(Box<Expr>, tokens::ColonColon, TypeName),
    /// Boolean test: `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`
    #[parse(postfix, bp = 8)]
    BoolTest(Box<Expr>, tokens::Is, BoolTestKind),

    // --- Infix ---
    // Multi-char operators before single-char to avoid partial matching
    #[parse(infix, bp = 1)]
    Or(Box<Expr>, tokens::Or, Box<Expr>),
    #[parse(infix, bp = 2)]
    And(Box<Expr>, tokens::And, Box<Expr>),
    #[parse(infix, bp = 5)]
    Neq(Box<Expr>, tokens::Neq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lte(Box<Expr>, tokens::Lte, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gte(Box<Expr>, tokens::Gte, Box<Expr>),
    #[parse(infix, bp = 5)]
    Eq(Box<Expr>, tokens::Eq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lt(Box<Expr>, tokens::Lt, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gt(Box<Expr>, tokens::Gt, Box<Expr>),

    // --- Atoms ---
    /// Function-style type cast: `bool 't'` -- must come before ColumnRef
    /// since type keywords like `bool` overlap with identifiers
    #[parse(atom)]
    CastFunc(TypeCastFunc),
    /// Function call: `func(args)` -- must come before ColumnRef
    #[parse(atom)]
    Func(FuncCall),
    /// Qualified wildcard: `table.*` -- must come before QualRef and ColumnRef
    #[parse(atom)]
    QualWild(QualifiedWildcard),
    /// Qualified column reference: `table.column` -- must come before ColumnRef
    #[parse(atom)]
    QualRef(QualifiedRef),
    /// Parenthesized expression: `(expr)`
    #[parse(atom)]
    Paren(ParenExpr),
    /// Integer literal: `42`
    #[parse(atom)]
    IntegerLit(tokens::IntegerLit),
    /// String literal: `'hello'`
    #[parse(atom)]
    StringLit(tokens::StringLit),
    /// Boolean true
    #[parse(atom)]
    BoolTrue(tokens::True),
    /// Boolean false
    #[parse(atom)]
    BoolFalse(tokens::False),
    /// NULL
    #[parse(atom)]
    Null(tokens::Null),
    /// Unqualified column reference: `f1`
    #[parse(atom)]
    ColumnRef(tokens::Ident),
    /// Bare wildcard: `*`
    #[parse(atom)]
    Star(tokens::Star),
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::expr::Expr;
    use crate::rules::SqlRules;

    // --- Atom tests ---

    #[test]
    fn parse_integer_literal() {
        let mut input = Input::new("42");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::IntegerLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_string_literal() {
        let mut input = Input::new("'hello'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::StringLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_bool_true() {
        let mut input = Input::new("true");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BoolTrue(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_bool_false() {
        let mut input = Input::new("false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BoolFalse(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_null() {
        let mut input = Input::new("null");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Null(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_column_ref() {
        let mut input = Input::new("f1");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::ColumnRef(_)));
    }

    #[test]
    fn parse_qualified_column_ref() {
        let mut input = Input::new("BOOLTBL1.f1");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::QualRef(_)));
    }

    #[test]
    fn parse_qualified_wildcard() {
        let mut input = Input::new("BOOLTBL1.*");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::QualWild(_)));
    }

    #[test]
    fn parse_star() {
        let mut input = Input::new("*");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Star(_)));
    }

    #[test]
    fn parse_function_call_no_args() {
        let mut input = Input::new("foo()");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_function_call_with_args() {
        let mut input = Input::new("pg_input_is_valid('true', 'bool')");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_function_call_booleq() {
        let mut input = Input::new("booleq(bool 'false', f1)");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_parenthesized_expr() {
        let mut input = Input::new("(1)");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Paren(_)));
    }

    // --- Type cast function-style: bool 'foo' ---

    #[test]
    fn parse_type_cast_bool_string() {
        let mut input = Input::new("bool 't'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::CastFunc(_)));
    }

    #[test]
    fn parse_type_cast_boolean_string() {
        let mut input = Input::new("boolean 'false'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::CastFunc(_)));
    }

    // --- Prefix operators ---

    #[test]
    fn parse_not_expr() {
        let mut input = Input::new("not false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Not(_, _)));
    }

    // --- Infix operators ---

    #[test]
    fn parse_and_expr() {
        let mut input = Input::new("true AND false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::And(..)));
    }

    #[test]
    fn parse_or_expr() {
        let mut input = Input::new("true OR false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Or(..)));
    }

    #[test]
    fn parse_eq_expr() {
        let mut input = Input::new("f1 = true");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Eq(..)));
    }

    #[test]
    fn parse_neq_expr() {
        let mut input = Input::new("f1 <> false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Neq(..)));
    }

    // --- Postfix: :: type cast ---

    #[test]
    fn parse_cast_colon_colon() {
        let mut input = Input::new("0::boolean");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Cast(..)));
    }

    #[test]
    fn parse_chained_cast() {
        let mut input = Input::new("'TrUe'::text::boolean");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        // Outer should be Cast
        assert!(matches!(expr, Expr::Cast(..)));
    }

    // --- Postfix: IS [NOT] TRUE/FALSE/UNKNOWN/NULL ---

    #[test]
    fn parse_is_true() {
        let mut input = Input::new("f1 IS TRUE");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_not_false() {
        let mut input = Input::new("f1 IS NOT FALSE");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_unknown() {
        let mut input = Input::new("b IS UNKNOWN");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_not_unknown() {
        let mut input = Input::new("b IS NOT UNKNOWN");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    // --- Precedence ---

    #[test]
    fn and_binds_tighter_than_or() {
        // a OR b AND c should parse as a OR (b AND c)
        let mut input = Input::new("true OR false AND true");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        // Top-level should be OR
        match &expr {
            Expr::Or(..) => {}
            other => panic!("expected OR at top level, got {other:?}"),
        }
    }

    #[test]
    fn comparison_binds_tighter_than_and() {
        // a AND b = c should parse as a AND (b = c)
        let mut input = Input::new("true AND f1 = false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        match &expr {
            Expr::And(..) => {}
            other => panic!("expected AND at top level, got {other:?}"),
        }
    }

    #[test]
    fn bool_cast_or_expr() {
        // bool 't' or bool 'f' should parse as (bool 't') OR (bool 'f')
        let mut input = Input::new("bool 't' or bool 'f'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Or(..)));
    }

    #[test]
    fn is_true_in_select_item() {
        // b IS TRUE should parse without consuming AS that follows
        let mut input = Input::new("b IS TRUE");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn cast_chain_in_expression() {
        // true::boolean::text should chain
        let mut input = Input::new("true::boolean::text");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Cast(..)));
    }
}
