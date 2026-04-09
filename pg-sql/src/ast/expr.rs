/// SQL expression AST with manual Pratt parsing for operator precedence.
///
/// Handles atoms, prefix (NOT), infix (AND, OR, comparisons), and
/// postfix operators (::type cast, IS [NOT] TRUE/FALSE/UNKNOWN/NULL)
/// which the derive macro does not support.
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::rules::SqlRules;
use crate::tokens;

/// Binary operator kinds for infix expressions.
///
/// No derive(Parse): operator-to-variant mapping is handled by the Pratt parser.
#[derive(Debug, Clone, PartialEq, Eq, Visit)]
pub enum BinOpKind {
    And,
    Or,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
}

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
/// No derive(Parse): keyword-to-variant mapping is handled manually.
#[derive(Debug, Clone, PartialEq, Eq, Visit)]
pub enum TypeName {
    Bool,
    Boolean,
    Text,
    Int,
    /// A type name that is an identifier (for pg_input_error_info etc.)
    Ident(String),
}

/// SQL expression.
///
/// Manual Parse: Pratt parser with postfix operators (::cast, IS tests)
/// that the derive macro does not support.
#[derive(Debug, Clone, Visit)]
pub enum Expr {
    /// Integer literal: `42`
    IntegerLit(tokens::IntegerLit),
    /// String literal: `'hello'`
    StringLit(tokens::StringLit),
    /// Boolean true
    BoolTrue(tokens::True),
    /// Boolean false
    BoolFalse(tokens::False),
    /// NULL
    Null(tokens::Null),
    /// Unqualified column reference: `f1`
    ColumnRef(tokens::Ident),
    /// Qualified column reference: `BOOLTBL1.f1`
    QualifiedRef {
        table: tokens::Ident,
        dot: tokens::Dot,
        column: tokens::Ident,
    },
    /// Qualified wildcard: `BOOLTBL1.*`
    QualifiedWildcard {
        table: tokens::Ident,
        dot: tokens::Dot,
        star: tokens::Star,
    },
    /// Bare wildcard: `*`
    Star(tokens::Star),
    /// Function call: `pg_input_is_valid('true', 'bool')`
    FuncCall {
        name: tokens::Ident,
        args: Vec<Expr>,
    },
    /// Parenthesized expression: `(expr)`
    Paren { inner: Box<Expr> },
    /// Function-style type cast: `bool 't'`, `text 'hello'`
    TypeCastFunc {
        type_name: TypeName,
        value: tokens::StringLit,
    },
    /// NOT expr
    Not(tokens::Not, Box<Expr>),
    /// Binary operation: `a AND b`, `a = b`, etc.
    BinOp {
        left: Box<Expr>,
        op: BinOpKind,
        right: Box<Expr>,
    },
    /// Postgres-style cast: `expr::type`
    Cast {
        expr: Box<Expr>,
        type_name: TypeName,
    },
    /// Boolean test: `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`
    BooleanTest { expr: Box<Expr>, test: BoolTestKind },
}

// --- Binding powers for Pratt parsing ---

/// Returns the left binding power for an infix operator, or None if the
/// current token is not an infix operator.
fn infix_bp(input: &Input<'_>) -> Option<(BinOpKind, u32)> {
    let mut fork = input.fork();
    SqlRules::consume_ignored(&mut fork);

    if tokens::Or::peek(&fork, &SqlRules) {
        return Some((BinOpKind::Or, 1));
    }
    if tokens::And::peek(&fork, &SqlRules) {
        return Some((BinOpKind::And, 2));
    }
    // Comparison operators -- check multi-char before single-char
    if tokens::Neq::peek(&fork, &SqlRules) {
        return Some((BinOpKind::Neq, 5));
    }
    if tokens::Lte::peek(&fork, &SqlRules) {
        return Some((BinOpKind::Lte, 5));
    }
    if tokens::Gte::peek(&fork, &SqlRules) {
        return Some((BinOpKind::Gte, 5));
    }
    if tokens::Eq::peek(&fork, &SqlRules) {
        return Some((BinOpKind::Eq, 5));
    }
    if tokens::Lt::peek(&fork, &SqlRules) {
        return Some((BinOpKind::Lt, 5));
    }
    if tokens::Gt::peek(&fork, &SqlRules) {
        return Some((BinOpKind::Gt, 5));
    }
    None
}

/// Consume the infix operator token (assumes infix_bp already confirmed the kind).
fn consume_infix_op(input: &mut Input<'_>, kind: &BinOpKind) -> Result<(), ParseError> {
    SqlRules::consume_ignored(input);
    match kind {
        BinOpKind::Or => {
            tokens::Or::parse(input, &SqlRules)?;
        }
        BinOpKind::And => {
            tokens::And::parse(input, &SqlRules)?;
        }
        BinOpKind::Eq => {
            tokens::Eq::parse(input, &SqlRules)?;
        }
        BinOpKind::Neq => {
            tokens::Neq::parse(input, &SqlRules)?;
        }
        BinOpKind::Lt => {
            tokens::Lt::parse(input, &SqlRules)?;
        }
        BinOpKind::Gt => {
            tokens::Gt::parse(input, &SqlRules)?;
        }
        BinOpKind::Lte => {
            tokens::Lte::parse(input, &SqlRules)?;
        }
        BinOpKind::Gte => {
            tokens::Gte::parse(input, &SqlRules)?;
        }
    }
    Ok(())
}

/// Parse a type name (for casts).
fn parse_type_name(input: &mut Input<'_>) -> Result<TypeName, ParseError> {
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

/// Check if the current position has a function-style type cast (type_name followed by string literal).
fn peek_type_cast_func(input: &Input<'_>) -> bool {
    let mut fork = input.fork();
    SqlRules::consume_ignored(&mut fork);

    // Must be a known type keyword followed by a string literal
    let type_kw = tokens::Boolean::peek(&fork, &SqlRules)
        || tokens::Bool::peek(&fork, &SqlRules)
        || tokens::Text::peek(&fork, &SqlRules)
        || tokens::Int::peek(&fork, &SqlRules);

    if !type_kw {
        return false;
    }

    // Try to consume the type keyword and check for string lit
    if tokens::Boolean::peek(&fork, &SqlRules) {
        let _ = tokens::Boolean::parse(&mut fork, &SqlRules);
    } else if tokens::Bool::peek(&fork, &SqlRules) {
        let _ = tokens::Bool::parse(&mut fork, &SqlRules);
    } else if tokens::Text::peek(&fork, &SqlRules) {
        let _ = tokens::Text::parse(&mut fork, &SqlRules);
    } else if tokens::Int::peek(&fork, &SqlRules) {
        let _ = tokens::Int::parse(&mut fork, &SqlRules);
    }

    SqlRules::consume_ignored(&mut fork);
    tokens::StringLit::peek(&fork, &SqlRules)
}

/// Try to parse a boolean test postfix (IS [NOT] TRUE/FALSE/UNKNOWN/NULL).
/// Returns None if not at an IS token or IS is not followed by a valid test keyword.
fn try_parse_bool_test(input: &mut Input<'_>) -> Result<Option<BoolTestKind>, ParseError> {
    let mut fork = input.fork();
    SqlRules::consume_ignored(&mut fork);

    if !tokens::Is::peek(&fork, &SqlRules) {
        return Ok(None);
    }
    tokens::Is::parse(&mut fork, &SqlRules)?;
    SqlRules::consume_ignored(&mut fork);

    let negated = if tokens::Not::peek(&fork, &SqlRules) {
        tokens::Not::parse(&mut fork, &SqlRules)?;
        SqlRules::consume_ignored(&mut fork);
        true
    } else {
        false
    };

    let kind = if tokens::True::peek(&fork, &SqlRules) {
        tokens::True::parse(&mut fork, &SqlRules)?;
        if negated {
            BoolTestKind::IsNotTrue
        } else {
            BoolTestKind::IsTrue
        }
    } else if tokens::False::peek(&fork, &SqlRules) {
        tokens::False::parse(&mut fork, &SqlRules)?;
        if negated {
            BoolTestKind::IsNotFalse
        } else {
            BoolTestKind::IsFalse
        }
    } else if tokens::Unknown::peek(&fork, &SqlRules) {
        tokens::Unknown::parse(&mut fork, &SqlRules)?;
        if negated {
            BoolTestKind::IsNotUnknown
        } else {
            BoolTestKind::IsUnknown
        }
    } else if tokens::Null::peek(&fork, &SqlRules) {
        tokens::Null::parse(&mut fork, &SqlRules)?;
        if negated {
            BoolTestKind::IsNotNull
        } else {
            BoolTestKind::IsNull
        }
    } else {
        // IS [NOT] but not followed by valid keyword -- not a boolean test
        return Ok(None);
    };

    input.commit(fork);
    Ok(Some(kind))
}

/// Parse an atom (primary expression) including postfix operators.
fn parse_atom(input: &mut Input<'_>) -> Result<Expr, ParseError> {
    SqlRules::consume_ignored(input);

    // Function-style type cast: bool 'foo', text 'hello'
    if peek_type_cast_func(input) {
        let type_name = parse_type_name(input)?;
        SqlRules::consume_ignored(input);
        let value = tokens::StringLit::parse(input, &SqlRules)?;
        let mut expr = Expr::TypeCastFunc { type_name, value };
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }

    // NOT prefix
    if tokens::Not::peek(input, &SqlRules) {
        let not = tokens::Not::parse(input, &SqlRules)?;
        let operand = parse_expr(input, 15)?; // NOT binds tightly
        return Ok(Expr::Not(not, Box::new(operand)));
    }

    // Parenthesized expression
    if tokens::LParen::peek(input, &SqlRules) {
        tokens::LParen::parse(input, &SqlRules)?;
        let inner = parse_expr(input, 0)?;
        SqlRules::consume_ignored(input);
        tokens::RParen::parse(input, &SqlRules)?;
        let mut expr = Expr::Paren {
            inner: Box::new(inner),
        };
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }

    // Star (bare wildcard)
    if tokens::Star::peek(input, &SqlRules) {
        let star = tokens::Star::parse(input, &SqlRules)?;
        return Ok(Expr::Star(star));
    }

    // Boolean literals
    if tokens::True::peek(input, &SqlRules) {
        let t = tokens::True::parse(input, &SqlRules)?;
        let mut expr = Expr::BoolTrue(t);
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }
    if tokens::False::peek(input, &SqlRules) {
        let f = tokens::False::parse(input, &SqlRules)?;
        let mut expr = Expr::BoolFalse(f);
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }
    if tokens::Null::peek(input, &SqlRules) {
        let n = tokens::Null::parse(input, &SqlRules)?;
        let mut expr = Expr::Null(n);
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }

    // Identifier -- could be column ref, qualified ref, qualified wildcard, or function call
    if tokens::Ident::peek(input, &SqlRules) {
        let ident = tokens::Ident::parse(input, &SqlRules)?;
        SqlRules::consume_ignored(input);

        // Function call: ident(...)
        if tokens::LParen::peek(input, &SqlRules) {
            tokens::LParen::parse(input, &SqlRules)?;
            let args = parse_func_args(input)?;
            SqlRules::consume_ignored(input);
            tokens::RParen::parse(input, &SqlRules)?;
            let mut expr = Expr::FuncCall { name: ident, args };
            expr = apply_postfix(input, expr)?;
            return Ok(expr);
        }

        // Qualified: ident.ident or ident.*
        if tokens::Dot::peek(input, &SqlRules) {
            let dot = tokens::Dot::parse(input, &SqlRules)?;
            SqlRules::consume_ignored(input);
            if tokens::Star::peek(input, &SqlRules) {
                let star = tokens::Star::parse(input, &SqlRules)?;
                let mut expr = Expr::QualifiedWildcard {
                    table: ident,
                    dot,
                    star,
                };
                expr = apply_postfix(input, expr)?;
                return Ok(expr);
            }
            let column = tokens::Ident::parse(input, &SqlRules)?;
            let mut expr = Expr::QualifiedRef {
                table: ident,
                dot,
                column,
            };
            expr = apply_postfix(input, expr)?;
            return Ok(expr);
        }

        // Simple column ref
        let mut expr = Expr::ColumnRef(ident);
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }

    // Integer literal
    if tokens::IntegerLit::peek(input, &SqlRules) {
        let lit = tokens::IntegerLit::parse(input, &SqlRules)?;
        let mut expr = Expr::IntegerLit(lit);
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }

    // String literal
    if tokens::StringLit::peek(input, &SqlRules) {
        let lit = tokens::StringLit::parse(input, &SqlRules)?;
        let mut expr = Expr::StringLit(lit);
        expr = apply_postfix(input, expr)?;
        return Ok(expr);
    }

    Err(ParseError::new(
        input.source().to_string(),
        input.cursor()..input.cursor(),
        "expression",
    ))
}

/// Apply postfix operators: `::type` casts and `IS [NOT] TRUE/FALSE/UNKNOWN/NULL`.
fn apply_postfix(input: &mut Input<'_>, mut expr: Expr) -> Result<Expr, ParseError> {
    loop {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);

        // :: type cast
        if tokens::ColonColon::peek(&fork, &SqlRules) {
            tokens::ColonColon::parse(&mut fork, &SqlRules)?;
            let type_name = parse_type_name(&mut fork)?;
            input.commit(fork);
            expr = Expr::Cast {
                expr: Box::new(expr),
                type_name,
            };
            continue;
        }

        // IS [NOT] TRUE/FALSE/UNKNOWN/NULL
        if let Some(test) = try_parse_bool_test(input)? {
            expr = Expr::BooleanTest {
                expr: Box::new(expr),
                test,
            };
            continue;
        }

        break;
    }
    Ok(expr)
}

/// Parse a comma-separated list of function arguments.
fn parse_func_args(input: &mut Input<'_>) -> Result<Vec<Expr>, ParseError> {
    let mut args = Vec::new();
    SqlRules::consume_ignored(input);

    // Empty args
    if tokens::RParen::peek(input, &SqlRules) {
        return Ok(args);
    }

    // First arg
    args.push(parse_expr(input, 0)?);

    // Subsequent args
    loop {
        SqlRules::consume_ignored(input);
        if !tokens::Comma::peek(input, &SqlRules) {
            break;
        }
        tokens::Comma::parse(input, &SqlRules)?;
        args.push(parse_expr(input, 0)?);
    }

    Ok(args)
}

/// Pratt expression parser entry point.
fn parse_expr(input: &mut Input<'_>, min_bp: u32) -> Result<Expr, ParseError> {
    let mut lhs = parse_atom(input)?;

    loop {
        // Check for infix operator
        let Some((op_kind, bp)) = infix_bp(input) else {
            break;
        };
        if bp < min_bp {
            break;
        }

        consume_infix_op(input, &op_kind)?;

        let right_bp = bp + 1; // left-associative
        let rhs = parse_expr(input, right_bp)?;

        lhs = Expr::BinOp {
            left: Box::new(lhs),
            op: op_kind,
            right: Box::new(rhs),
        };
    }

    Ok(lhs)
}

// --- Parse trait implementation ---

impl<'input> Parse<'input> for Expr {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // Expressions can start with many things -- use a broad pattern
        r"[a-zA-Z_*'(0-9]"
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        let mut fork = input.fork();
        SqlRules::consume_ignored(&mut fork);

        // Can start with: type keyword, NOT, '(', '*', true, false, null,
        // identifier, integer, string
        tokens::Not::peek(&fork, &SqlRules)
            || tokens::LParen::peek(&fork, &SqlRules)
            || tokens::Star::peek(&fork, &SqlRules)
            || tokens::True::peek(&fork, &SqlRules)
            || tokens::False::peek(&fork, &SqlRules)
            || tokens::Null::peek(&fork, &SqlRules)
            || tokens::Bool::peek(&fork, &SqlRules)
            || tokens::Boolean::peek(&fork, &SqlRules)
            || tokens::Text::peek(&fork, &SqlRules)
            || tokens::Int::peek(&fork, &SqlRules)
            || tokens::Ident::peek(&fork, &SqlRules)
            || tokens::IntegerLit::peek(&fork, &SqlRules)
            || tokens::StringLit::peek(&fork, &SqlRules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, _rules: &R) -> Result<Self, ParseError> {
        parse_expr(input, 0)
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::expr::{BinOpKind, Expr};
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
        assert!(matches!(expr, Expr::QualifiedRef { .. }));
    }

    #[test]
    fn parse_qualified_wildcard() {
        let mut input = Input::new("BOOLTBL1.*");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::QualifiedWildcard { .. }));
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
        assert!(matches!(expr, Expr::FuncCall { .. }));
    }

    #[test]
    fn parse_function_call_with_args() {
        let mut input = Input::new("pg_input_is_valid('true', 'bool')");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::FuncCall { .. }));
    }

    #[test]
    fn parse_function_call_booleq() {
        let mut input = Input::new("booleq(bool 'false', f1)");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::FuncCall { .. }));
    }

    #[test]
    fn parse_parenthesized_expr() {
        let mut input = Input::new("(1)");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Paren { .. }));
    }

    // --- Type cast function-style: bool 'foo' ---

    #[test]
    fn parse_type_cast_bool_string() {
        let mut input = Input::new("bool 't'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::TypeCastFunc { .. }));
    }

    #[test]
    fn parse_type_cast_boolean_string() {
        let mut input = Input::new("boolean 'false'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::TypeCastFunc { .. }));
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
        assert!(matches!(expr, Expr::BinOp { .. }));
    }

    #[test]
    fn parse_or_expr() {
        let mut input = Input::new("true OR false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BinOp { .. }));
    }

    #[test]
    fn parse_eq_expr() {
        let mut input = Input::new("f1 = true");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BinOp { .. }));
    }

    #[test]
    fn parse_neq_expr() {
        let mut input = Input::new("f1 <> false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BinOp { .. }));
    }

    // --- Postfix: :: type cast ---

    #[test]
    fn parse_cast_colon_colon() {
        let mut input = Input::new("0::boolean");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Cast { .. }));
    }

    #[test]
    fn parse_chained_cast() {
        let mut input = Input::new("'TrUe'::text::boolean");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        // Outer should be Cast
        assert!(matches!(expr, Expr::Cast { .. }));
    }

    // --- Postfix: IS [NOT] TRUE/FALSE/UNKNOWN/NULL ---

    #[test]
    fn parse_is_true() {
        let mut input = Input::new("f1 IS TRUE");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BooleanTest { .. }));
    }

    #[test]
    fn parse_is_not_false() {
        let mut input = Input::new("f1 IS NOT FALSE");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BooleanTest { .. }));
    }

    #[test]
    fn parse_is_unknown() {
        let mut input = Input::new("b IS UNKNOWN");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BooleanTest { .. }));
    }

    #[test]
    fn parse_is_not_unknown() {
        let mut input = Input::new("b IS NOT UNKNOWN");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BooleanTest { .. }));
    }

    // --- Precedence ---

    #[test]
    fn and_binds_tighter_than_or() {
        // a OR b AND c should parse as a OR (b AND c)
        let mut input = Input::new("true OR false AND true");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        // Top-level should be OR
        match &expr {
            Expr::BinOp {
                op: BinOpKind::Or, ..
            } => {}
            other => panic!("expected OR at top level, got {other:?}"),
        }
    }

    #[test]
    fn comparison_binds_tighter_than_and() {
        // a AND b = c should parse as a AND (b = c)
        let mut input = Input::new("true AND f1 = false");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        match &expr {
            Expr::BinOp {
                op: BinOpKind::And, ..
            } => {}
            other => panic!("expected AND at top level, got {other:?}"),
        }
    }

    #[test]
    fn bool_cast_or_expr() {
        // bool 't' or bool 'f' should parse as (bool 't') OR (bool 'f')
        let mut input = Input::new("bool 't' or bool 'f'");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(
            expr,
            Expr::BinOp {
                op: BinOpKind::Or,
                ..
            }
        ));
    }

    #[test]
    fn is_true_in_select_item() {
        // b IS TRUE should parse without consuming AS that follows
        let mut input = Input::new("b IS TRUE");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::BooleanTest { .. }));
        assert!(input.is_empty());
    }

    #[test]
    fn cast_chain_in_expression() {
        // true::boolean::text should chain
        let mut input = Input::new("true::boolean::text");
        let expr = Expr::parse(&mut input, &SqlRules).unwrap();
        assert!(matches!(expr, Expr::Cast { .. }));
    }
}
