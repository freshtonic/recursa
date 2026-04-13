/// SQL expression AST with derived Pratt parsing for operator precedence.
///
/// Handles atoms, prefix (NOT, unary minus), infix (AND, OR, comparisons,
/// arithmetic), and postfix operators (::type cast, IS [NOT] TRUE/FALSE/UNKNOWN/NULL,
/// IN (list)).
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};

/// Content inside IN parentheses: either a subquery or expression list.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum InContent {
    Subquery(Box<crate::ast::values::CompoundQuery>),
    Exprs(Seq<Expr, punct::Comma>),
}

/// `IN (expr, ...)` or `IN (subquery)` postfix suffix.
pub type InList = Surrounded<punct::LParen, InContent, punct::RParen>;

/// Parenthesized precision/scale for type names: `(10,2)` or `(3)`.
pub type TypePrecision =
    Surrounded<punct::LParen, Seq<literal::IntegerLit, punct::Comma>, punct::RParen>;

/// Array type suffix: `[]`
#[derive(Debug, Clone, FormatTokens, PartialEq, Eq, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ArrayTypeSuffix(pub punct::LBracket, pub punct::RBracket);

/// Type name for casts.
#[derive(Debug, Clone, FormatTokens, PartialEq, Eq, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum TypeName {
    Bool(keyword::Bool),
    Boolean(keyword::Boolean),
    Text(keyword::Text),
    Integer(keyword::Integer),
    Int(keyword::Int),
    Serial(keyword::Serial),
    Numeric(keyword::Numeric),
    Varchar(keyword::Varchar),
    Ident(literal::Ident),
}

// --- Boolean test suffix structs ---
// NOT variants listed before non-NOT variants so the longer pattern wins via
// longest-match lookahead (e.g., "NOT TRUE" matches before "TRUE").

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotTrue(pub keyword::Not, pub keyword::True);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotFalse(pub keyword::Not, pub keyword::False);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotUnknown(pub keyword::Not, pub keyword::Unknown);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNotNull(pub keyword::Not, pub keyword::Null);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsTrue(pub keyword::True);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsFalse(pub keyword::False);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsUnknown(pub keyword::Unknown);

#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct IsNull(pub keyword::Null);

/// Boolean test suffix: the part after `IS` in `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`.
///
/// NOT variants are listed first so the combined peek regex disambiguates
/// via longest match (e.g., `NOT TRUE` is longer than `TRUE`).
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
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
///
/// Uses AliasName for the table part to allow keywords like EXCLUDED, NEW, OLD.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct QualifiedRef {
    pub table: literal::AliasName,
    pub dot: punct::Dot,
    pub column: literal::AliasName,
}

/// Qualified wildcard: `table.*`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct QualifiedWildcard {
    pub table: literal::AliasName,
    pub dot: punct::Dot,
    pub star: punct::Star,
}

/// Optional DISTINCT keyword in function calls: `count(DISTINCT x)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct DistinctKw(pub keyword::Distinct);

/// Window specification: `OVER (...)` or `OVER ()`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowSpec {
    pub _over: keyword::Over,
    pub _lparen: punct::LParen,
    pub partition_by: Option<WindowPartitionBy>,
    pub order_by: Option<crate::ast::select::OrderByClause>,
    pub _rparen: punct::RParen,
}

/// PARTITION BY in window: `PARTITION BY expr, ...`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct WindowPartitionBy {
    pub _partition: keyword::Partition,
    pub _by: keyword::By,
    pub exprs: Seq<Expr, punct::Comma>,
}

/// Function call: `name(arg1, arg2, ...)`
///
/// Keeps explicit `lparen` field rather than using `Surrounded` because the
/// derive macro chains `IS_TERMINAL` fields for `first_pattern` — the
/// `Ident + LParen` pattern is what disambiguates `FuncCall` from a plain
/// `Ident` in `TableRef` enum lookahead.
///
/// Function call: `name([*] [DISTINCT] args) [OVER (...)]`
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct FuncCall {
    pub name: literal::AliasName,
    pub lparen: punct::LParen,
    pub star_arg: Option<punct::Star>,
    pub distinct: Option<DistinctKw>,
    pub args: Seq<Expr, punct::Comma>,
    pub rparen: punct::RParen,
    pub window: Option<WindowSpec>,
}

/// Content inside parentheses: either a subquery or a comma-separated expression list.
/// Subquery (CompoundQuery) must come first so SELECT/VALUES/WITH keywords are matched
/// before trying to parse as a regular expression.
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ParenContent {
    Subquery(Box<crate::ast::values::CompoundQuery>),
    Exprs(Seq<Expr, punct::Comma>),
}

/// Parenthesized expression: `(expr)`, `(expr, expr, ...)`, or `(SELECT/VALUES ...)`
pub type ParenExpr = Surrounded<punct::LParen, ParenContent, punct::RParen>;

/// EXISTS subquery: `EXISTS (SELECT ...)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ExistsExpr {
    pub _exists: keyword::Exists,
    pub subquery: Surrounded<punct::LParen, Box<crate::ast::values::CompoundQuery>, punct::RParen>,
}

/// ARRAY bracket constructor: `ARRAY[expr, ...]`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ArrayBracket {
    pub _array: PhantomData<keyword::Array>,
    pub lbracket: punct::LBracket,
    pub elements: Seq<Expr, punct::Comma>,
    pub rbracket: punct::RBracket,
}

/// ARRAY subquery constructor: `ARRAY(subquery)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct ArraySubquery {
    pub _array: PhantomData<keyword::Array>,
    pub subquery: Surrounded<punct::LParen, Box<crate::ast::values::CompoundQuery>, punct::RParen>,
}

/// ARRAY constructor: `ARRAY[expr, ...]` or `ARRAY(subquery)`
///
/// Variant ordering: Bracket (`ARRAY[`) has a longer first_pattern than
/// Subquery (`ARRAY(`) because `[` is a different token than `(`.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub enum ArrayExpr {
    Bracket(ArrayBracket),
    Subquery(ArraySubquery),
}

/// ROW constructor: `ROW(expr, ...)`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct RowExpr {
    pub _row: keyword::Row,
    pub values: Surrounded<punct::LParen, Seq<Expr, punct::Comma>, punct::RParen>,
}

/// Cast type with optional precision/array suffix: `numeric(10,0)`, `integer[]`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct CastType {
    pub base: TypeName,
    pub precision: Option<TypePrecision>,
    pub array_suffix: Option<ArrayTypeSuffix>,
}

/// NOT IN list: `expr NOT IN (val, ...)` suffix.
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct NotInSuffix {
    pub _not: keyword::Not,
    pub _in: keyword::In,
    pub list: InList,
}

/// Function-style type cast: `bool 'value'`, `text 'hello'`
#[derive(FormatTokens, Parse, Visit, Debug, Clone)]
#[parse(rules = SqlRules)]
pub struct TypeCastFunc {
    pub type_name: TypeName,
    pub value: literal::StringLit,
}

// --- Pratt expression enum ---

/// SQL expression with Pratt-derived parsing.
#[derive(FormatTokens, Parse, Debug, Clone, Visit)]
#[parse(rules = SqlRules, pratt)]
pub enum Expr {
    // --- Prefix ---
    #[parse(prefix, bp = 15)]
    Not(keyword::Not, Box<Expr>),
    #[parse(prefix, bp = 12)]
    Neg(punct::Minus, Box<Expr>),

    // --- Postfix ---
    /// Postgres-style cast: `expr::type`
    #[parse(postfix, bp = 20)]
    Cast(Box<Expr>, punct::ColonColon, CastType),
    /// Array subscript: `expr[idx]`
    #[parse(postfix, bp = 20)]
    Subscript(Box<Expr>, punct::LBracket, Box<Expr>, punct::RBracket),
    /// Boolean test: `expr IS [NOT] TRUE/FALSE/UNKNOWN/NULL`
    #[parse(postfix, bp = 8)]
    BoolTest(Box<Expr>, keyword::Is, BoolTestKind),
    /// NOT IN list: `expr NOT IN (val, ...)`
    #[parse(postfix, bp = 6)]
    NotInExpr(Box<Expr>, NotInSuffix),
    /// IN list: `expr IN (val, ...)`
    #[parse(postfix, bp = 6)]
    InExpr(Box<Expr>, keyword::In, InList),

    // --- Infix ---
    // Multi-char operators before single-char to avoid partial matching.
    //
    // JSON / JSONB operators are listed FIRST among infix so that their
    // longer tokens are peeked before conflicting shorter ones
    // (e.g. `<@` before `<`, `->` before `-`). All use bp = 10 — same tier
    // as Concat/Add/Sub (which is Postgres's convention for these ops).
    /// JSON path as text: `expr #>> path`
    #[parse(infix, bp = 10)]
    JsonPathText(Box<Expr>, punct::HashArrowArrow, Box<Expr>),
    /// JSON path: `expr #> path`
    #[parse(infix, bp = 10)]
    JsonPath(Box<Expr>, punct::HashArrow, Box<Expr>),
    /// JSON field as text: `expr ->> field`
    #[parse(infix, bp = 10)]
    JsonFieldText(Box<Expr>, punct::ArrowArrow, Box<Expr>),
    /// JSON field: `expr -> field`
    #[parse(infix, bp = 10)]
    JsonField(Box<Expr>, punct::Arrow, Box<Expr>),
    /// JSON any-key-exists: `expr ?| keys`
    #[parse(infix, bp = 10)]
    JsonAnyKey(Box<Expr>, punct::QuestionPipe, Box<Expr>),
    /// JSON all-keys-exist: `expr ?& keys`
    #[parse(infix, bp = 10)]
    JsonAllKeys(Box<Expr>, punct::QuestionAmp, Box<Expr>),
    /// JSON key-exists: `expr ? key`
    #[parse(infix, bp = 10)]
    JsonKey(Box<Expr>, punct::Question, Box<Expr>),
    /// JSONB contains: `expr @> expr`
    #[parse(infix, bp = 10)]
    JsonContains(Box<Expr>, punct::AtGt, Box<Expr>),
    /// JSONB contained-by: `expr <@ expr`
    #[parse(infix, bp = 10)]
    JsonContainedBy(Box<Expr>, punct::LtAt, Box<Expr>),

    #[parse(infix, bp = 1)]
    Or(Box<Expr>, keyword::Or, Box<Expr>),
    #[parse(infix, bp = 2)]
    And(Box<Expr>, keyword::And, Box<Expr>),
    #[parse(infix, bp = 5)]
    BangEq(Box<Expr>, punct::BangEq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Neq(Box<Expr>, punct::Neq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lte(Box<Expr>, punct::Lte, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gte(Box<Expr>, punct::Gte, Box<Expr>),
    #[parse(infix, bp = 5)]
    Eq(Box<Expr>, punct::Eq, Box<Expr>),
    #[parse(infix, bp = 5)]
    Lt(Box<Expr>, punct::Lt, Box<Expr>),
    #[parse(infix, bp = 5)]
    Gt(Box<Expr>, punct::Gt, Box<Expr>),
    /// String concatenation: `expr || expr`
    #[parse(infix, bp = 10)]
    Concat(Box<Expr>, punct::Concat, Box<Expr>),
    #[parse(infix, bp = 10)]
    Add(Box<Expr>, punct::Plus, Box<Expr>),
    #[parse(infix, bp = 10)]
    Sub(Box<Expr>, punct::Minus, Box<Expr>),
    /// Multiplication: `expr * expr`
    #[parse(infix, bp = 11)]
    Mul(Box<Expr>, punct::Star, Box<Expr>),
    /// Division: `expr / expr`
    #[parse(infix, bp = 11)]
    Div(Box<Expr>, punct::Slash, Box<Expr>),
    /// Modulo: `expr % expr`
    #[parse(infix, bp = 11)]
    Mod(Box<Expr>, punct::Percent, Box<Expr>),

    // --- Atoms ---
    /// EXISTS subquery: `EXISTS (SELECT ...)`
    #[parse(atom)]
    Exists(ExistsExpr),
    /// ARRAY constructor: `ARRAY[...]` or `ARRAY(...)`
    #[parse(atom)]
    Array(ArrayExpr),
    /// ROW constructor: `ROW(...)`
    #[parse(atom)]
    RowExpr(RowExpr),
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
    /// Numeric literal: `77.7` -- must come before IntegerLit for longest match
    #[parse(atom)]
    NumericLit(literal::NumericLit),
    /// Integer literal: `42`
    #[parse(atom)]
    IntegerLit(literal::IntegerLit),
    /// String literal: `'hello'`
    #[parse(atom)]
    StringLit(literal::StringLit),
    /// Boolean true
    #[parse(atom)]
    BoolTrue(keyword::True),
    /// Boolean false
    #[parse(atom)]
    BoolFalse(keyword::False),
    /// NULL
    #[parse(atom)]
    Null(keyword::Null),
    /// Unqualified column reference: `f1` or `"Foo"`
    #[parse(atom)]
    ColumnRef(literal::Ident),
    /// Bare wildcard: `*`
    #[parse(atom)]
    Star(punct::Star),
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
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::IntegerLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_string_literal() {
        let mut input = Input::new("'hello'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::StringLit(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_bool_true() {
        let mut input = Input::new("true");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTrue(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_bool_false() {
        let mut input = Input::new("false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolFalse(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_null() {
        let mut input = Input::new("null");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Null(_)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_column_ref() {
        let mut input = Input::new("f1");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::ColumnRef(_)));
    }

    #[test]
    fn parse_qualified_column_ref() {
        let mut input = Input::new("BOOLTBL1.f1");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::QualRef(_)));
    }

    #[test]
    fn parse_qualified_wildcard() {
        let mut input = Input::new("BOOLTBL1.*");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::QualWild(_)));
    }

    #[test]
    fn parse_star() {
        let mut input = Input::new("*");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Star(_)));
    }

    #[test]
    fn parse_function_call_no_args() {
        let mut input = Input::new("foo()");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_function_call_with_args() {
        let mut input = Input::new("pg_input_is_valid('true', 'bool')");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_function_call_booleq() {
        let mut input = Input::new("booleq(bool 'false', f1)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Func(_)));
    }

    #[test]
    fn parse_parenthesized_expr() {
        let mut input = Input::new("(1)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Paren(_)));
    }

    // --- Type cast function-style: bool 'foo' ---

    #[test]
    fn parse_type_cast_bool_string() {
        let mut input = Input::new("bool 't'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::CastFunc(_)));
    }

    #[test]
    fn parse_type_cast_boolean_string() {
        let mut input = Input::new("boolean 'false'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::CastFunc(_)));
    }

    // --- Prefix operators ---

    #[test]
    fn parse_not_expr() {
        let mut input = Input::new("not false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Not(_, _)));
    }

    // --- Infix operators ---

    #[test]
    fn parse_and_expr() {
        let mut input = Input::new("true AND false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::And(..)));
    }

    #[test]
    fn parse_or_expr() {
        let mut input = Input::new("true OR false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Or(..)));
    }

    #[test]
    fn parse_eq_expr() {
        let mut input = Input::new("f1 = true");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Eq(..)));
    }

    #[test]
    fn parse_neq_expr() {
        let mut input = Input::new("f1 <> false");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Neq(..)));
    }

    // --- Postfix: :: type cast ---

    #[test]
    fn parse_cast_colon_colon() {
        let mut input = Input::new("0::boolean");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Cast(..)));
    }

    #[test]
    fn parse_chained_cast() {
        let mut input = Input::new("'TrUe'::text::boolean");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        // Outer should be Cast
        assert!(matches!(expr, Expr::Cast(..)));
    }

    // --- Postfix: IS [NOT] TRUE/FALSE/UNKNOWN/NULL ---

    #[test]
    fn parse_is_true() {
        let mut input = Input::new("f1 IS TRUE");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_not_false() {
        let mut input = Input::new("f1 IS NOT FALSE");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_unknown() {
        let mut input = Input::new("b IS UNKNOWN");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    #[test]
    fn parse_is_not_unknown() {
        let mut input = Input::new("b IS NOT UNKNOWN");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
    }

    // --- Precedence ---

    #[test]
    fn and_binds_tighter_than_or() {
        // a OR b AND c should parse as a OR (b AND c)
        let mut input = Input::new("true OR false AND true");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
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
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        match &expr {
            Expr::And(..) => {}
            other => panic!("expected AND at top level, got {other:?}"),
        }
    }

    #[test]
    fn bool_cast_or_expr() {
        // bool 't' or bool 'f' should parse as (bool 't') OR (bool 'f')
        let mut input = Input::new("bool 't' or bool 'f'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Or(..)));
    }

    #[test]
    fn is_true_in_select_item() {
        // b IS TRUE should parse without consuming AS that follows
        let mut input = Input::new("b IS TRUE");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::BoolTest(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn cast_chain_in_expression() {
        // true::boolean::text should chain
        let mut input = Input::new("true::boolean::text");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Cast(..)));
    }

    // --- Arithmetic operators ---

    #[test]
    fn parse_addition() {
        let mut input = Input::new("4+4");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Add(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_subtraction() {
        let mut input = Input::new("10-3");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Sub(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_unary_minus() {
        let mut input = Input::new("-1");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Neg(..)));
        assert!(input.is_empty());
    }

    // --- Numeric literal ---

    #[test]
    fn parse_numeric_literal() {
        let mut input = Input::new("77.7");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::NumericLit(_)));
        assert!(input.is_empty());
    }

    // --- IN expression ---

    #[test]
    fn parse_in_expr() {
        let mut input = Input::new("f1 IN (1, 2, 3)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::InExpr(..)));
        assert!(input.is_empty());
    }

    // --- JSON / JSONB operators ---

    #[test]
    fn parse_json_field() {
        let mut input = Input::new("data -> 'key'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonField(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_json_field_text() {
        let mut input = Input::new("data ->> 'key'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonFieldText(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_json_path() {
        let mut input = Input::new("data #> '{a,b}'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonPath(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_json_path_text() {
        let mut input = Input::new("data #>> '{a,b}'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonPathText(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_contains() {
        let mut input = Input::new("a @> b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonContains(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_contained_by() {
        let mut input = Input::new("a <@ b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonContainedBy(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_key_exists() {
        let mut input = Input::new("a ? 'k'");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonKey(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_any_key() {
        let mut input = Input::new("a ?| b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonAnyKey(..)));
        assert!(input.is_empty());
    }

    #[test]
    fn parse_jsonb_all_keys() {
        let mut input = Input::new("a ?& b");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::JsonAllKeys(..)));
        assert!(input.is_empty());
    }

    // --- Subquery expression ---

    #[test]
    fn parse_subquery_expr() {
        let mut input = Input::new("(SELECT 1)");
        let expr = Expr::parse::<SqlRules>(&mut input).unwrap();
        assert!(matches!(expr, Expr::Paren(_)));
        assert!(input.is_empty());
    }
}
