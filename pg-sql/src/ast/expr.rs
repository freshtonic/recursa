/// SQL expression AST with derived Pratt parsing for operator precedence.
///
/// Handles atoms, prefix (NOT), infix (AND, OR, comparisons), and
/// postfix operators (::type cast, IS [NOT] TRUE/FALSE/UNKNOWN/NULL).
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::rules::SqlRules;
use crate::tokens;

/// Boolean test kinds for IS [NOT] TRUE/FALSE/UNKNOWN/NULL.
///
/// No derive(Parse): keyword-sequence-to-variant mapping is handled manually.
#[derive(Debug, Clone, PartialEq, Eq, Visit)]
pub enum BoolTestKind {
    IsTrue,
    IsNotTrue,
    IsFalse,
    IsNotFalse,
    IsUnknown,
    IsNotUnknown,
    IsNull,
    IsNotNull,
}

/// Type name for casts (the types that boolean.sql uses).
///
/// Manual Parse: keyword-to-variant dispatch that the derive macro cannot express.
#[derive(Debug, Clone, PartialEq, Eq, Visit)]
pub enum TypeName {
    Bool,
    Boolean,
    Text,
    Int,
    /// A type name that is an identifier (for pg_input_error_info etc.)
    Ident(String),
}

impl<'input> Parse<'input> for TypeName {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // Type names start with a keyword or identifier
        r"(?i:BOOLEAN|BOOL|TEXT|INT)\b|[a-zA-Z_][a-zA-Z0-9_]*"
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        tokens::Boolean::peek(&fork, &SqlRules)
            || tokens::Bool::peek(&fork, &SqlRules)
            || tokens::Text::peek(&fork, &SqlRules)
            || tokens::Int::peek(&fork, &SqlRules)
            || tokens::Ident::peek(&fork, &SqlRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        if tokens::Boolean::peek(input, &SqlRules) {
            tokens::Boolean::parse(input, &SqlRules)?;
            return Ok(TypeName::Boolean);
        }
        if tokens::Bool::peek(input, &SqlRules) {
            tokens::Bool::parse(input, &SqlRules)?;
            return Ok(TypeName::Bool);
        }
        if tokens::Text::peek(input, &SqlRules) {
            tokens::Text::parse(input, &SqlRules)?;
            return Ok(TypeName::Text);
        }
        if tokens::Int::peek(input, &SqlRules) {
            tokens::Int::parse(input, &SqlRules)?;
            return Ok(TypeName::Int);
        }
        if tokens::Ident::peek(input, &SqlRules) {
            let ident = tokens::Ident::parse(input, &SqlRules)?;
            return Ok(TypeName::Ident(ident.0));
        }
        Err(ParseError::new(
            input.source().to_string(),
            input.cursor()..input.cursor(),
            "type name",
        ))
    }
}

/// Parses the suffix after `IS` in a boolean test: [NOT] TRUE/FALSE/UNKNOWN/NULL.
///
/// Manual Parse: complex keyword dispatch with optional NOT that the derive macro
/// cannot express.
#[derive(Debug, Clone, Visit)]
pub struct BoolTestSuffix {
    pub kind: BoolTestKind,
}

impl<'input> Parse<'input> for BoolTestSuffix {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        r"(?i:NOT|TRUE|FALSE|UNKNOWN|NULL)\b"
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        tokens::Not::peek(&fork, &SqlRules)
            || tokens::True::peek(&fork, &SqlRules)
            || tokens::False::peek(&fork, &SqlRules)
            || tokens::Unknown::peek(&fork, &SqlRules)
            || tokens::Null::peek(&fork, &SqlRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);

        let negated = if tokens::Not::peek(input, &SqlRules) {
            tokens::Not::parse(input, &SqlRules)?;
            SqlRules::consume_ignored(input);
            true
        } else {
            false
        };

        let kind = if tokens::True::peek(input, &SqlRules) {
            tokens::True::parse(input, &SqlRules)?;
            if negated {
                BoolTestKind::IsNotTrue
            } else {
                BoolTestKind::IsTrue
            }
        } else if tokens::False::peek(input, &SqlRules) {
            tokens::False::parse(input, &SqlRules)?;
            if negated {
                BoolTestKind::IsNotFalse
            } else {
                BoolTestKind::IsFalse
            }
        } else if tokens::Unknown::peek(input, &SqlRules) {
            tokens::Unknown::parse(input, &SqlRules)?;
            if negated {
                BoolTestKind::IsNotUnknown
            } else {
                BoolTestKind::IsUnknown
            }
        } else if tokens::Null::peek(input, &SqlRules) {
            tokens::Null::parse(input, &SqlRules)?;
            if negated {
                BoolTestKind::IsNotNull
            } else {
                BoolTestKind::IsNull
            }
        } else {
            return Err(ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                "TRUE, FALSE, UNKNOWN, or NULL",
            ));
        };

        Ok(BoolTestSuffix { kind })
    }
}

// --- Atom wrapper structs ---

/// Qualified column reference: `table.column`
///
/// Manual Parse: needs three-token lookahead (ident.ident) to distinguish from
/// plain column ref, qualified wildcard, and function call.
#[derive(Visit, Debug, Clone)]
pub struct QualifiedRef {
    pub table: tokens::Ident,
    pub dot: tokens::Dot,
    pub column: tokens::Ident,
}

impl<'input> Parse<'input> for QualifiedRef {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        tokens::Ident::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        if !tokens::Ident::peek(&fork, &SqlRules) {
            return false;
        }
        let _ = tokens::Ident::parse(&mut fork, &SqlRules);
        SqlRules::consume_ignored(&mut fork);
        if !tokens::Dot::peek(&fork, &SqlRules) {
            return false;
        }
        let _ = tokens::Dot::parse(&mut fork, &SqlRules);
        SqlRules::consume_ignored(&mut fork);
        tokens::Ident::peek(&fork, &SqlRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let table = tokens::Ident::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let dot = tokens::Dot::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let column = tokens::Ident::parse(input, &SqlRules)?;
        Ok(QualifiedRef { table, dot, column })
    }
}

/// Qualified wildcard: `table.*`
///
/// Manual Parse: needs three-token lookahead (ident.*) to distinguish from
/// plain column ref and qualified column ref.
#[derive(Visit, Debug, Clone)]
pub struct QualifiedWildcard {
    pub table: tokens::Ident,
    pub dot: tokens::Dot,
    pub star: tokens::Star,
}

impl<'input> Parse<'input> for QualifiedWildcard {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        tokens::Ident::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        if !tokens::Ident::peek(&fork, &SqlRules) {
            return false;
        }
        let _ = tokens::Ident::parse(&mut fork, &SqlRules);
        SqlRules::consume_ignored(&mut fork);
        if !tokens::Dot::peek(&fork, &SqlRules) {
            return false;
        }
        let _ = tokens::Dot::parse(&mut fork, &SqlRules);
        SqlRules::consume_ignored(&mut fork);
        tokens::Star::peek(&fork, &SqlRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let table = tokens::Ident::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let dot = tokens::Dot::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let star = tokens::Star::parse(input, &SqlRules)?;
        Ok(QualifiedWildcard { table, dot, star })
    }
}

/// Function call: `name(arg1, arg2, ...)`
///
/// Manual Parse: needs special handling for the comma-separated argument list
/// and empty argument case. Manual Visit: uses Vec<Expr> which implements Visit
/// but Seq does not.
#[derive(Debug, Clone, Visit)]
pub struct FuncCall {
    pub name: tokens::Ident,
    pub lparen: tokens::LParen,
    pub args: Vec<Expr>,
    pub rparen: tokens::RParen,
}

impl<'input> Parse<'input> for FuncCall {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        tokens::Ident::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);
        if !tokens::Ident::peek(&fork, &SqlRules) {
            return false;
        }
        // Must be ident followed by '('
        let _ = tokens::Ident::parse(&mut fork, &SqlRules);
        SqlRules::consume_ignored(&mut fork);
        tokens::LParen::peek(&fork, &SqlRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let name = tokens::Ident::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let lparen = tokens::LParen::parse(input, &SqlRules)?;

        let mut args = Vec::new();
        SqlRules::consume_ignored(input);
        if !tokens::RParen::peek(input, &SqlRules) {
            args.push(Expr::parse(input, &SqlRules)?);
            loop {
                SqlRules::consume_ignored(input);
                if !tokens::Comma::peek(input, &SqlRules) {
                    break;
                }
                tokens::Comma::parse(input, &SqlRules)?;
                args.push(Expr::parse(input, &SqlRules)?);
            }
        }
        SqlRules::consume_ignored(input);
        let rparen = tokens::RParen::parse(input, &SqlRules)?;

        Ok(FuncCall {
            name,
            lparen,
            args,
            rparen,
        })
    }
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
///
/// Manual Parse: needs two-token lookahead (type keyword followed by string
/// literal) to avoid matching plain identifiers or type keywords used in other
/// contexts.
#[derive(Visit, Debug, Clone)]
pub struct TypeCastFunc {
    pub type_name: TypeName,
    pub value: tokens::StringLit,
}

impl<'input> Parse<'input> for TypeCastFunc {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        r"(?i:BOOLEAN|BOOL|TEXT|INT)\b"
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);

        // Must be a known type keyword (not an arbitrary identifier)
        let type_kw = tokens::Boolean::peek(&fork, &SqlRules)
            || tokens::Bool::peek(&fork, &SqlRules)
            || tokens::Text::peek(&fork, &SqlRules)
            || tokens::Int::peek(&fork, &SqlRules);

        if !type_kw {
            return false;
        }

        // Consume the type keyword and check for string literal
        let _ = TypeName::parse(&mut fork, &SqlRules);
        SqlRules::consume_ignored(&mut fork);
        tokens::StringLit::peek(&fork, &SqlRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        SqlRules::consume_ignored(input);
        let type_name = TypeName::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);
        let value = tokens::StringLit::parse(input, &SqlRules)?;
        Ok(TypeCastFunc { type_name, value })
    }
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
    BoolTest(Box<Expr>, tokens::Is, BoolTestSuffix),

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
