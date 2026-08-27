use recursa_legacy_arith::arith;

fn parse(source: &str) -> Result<arith::Expr<'_>, recursa::ParseError> {
    let lexed = arith::lex(source);
    assert_eq!(lexed.errors().count(), 0, "lex errors in {source:?}");
    let mut input = lexed.input();
    let parsed = arith::Expr::parse(&mut input)?;
    assert!(input.is_eof(), "accepted parse must consume explicit EOF");
    Ok(parsed.into_ast())
}

fn parsed(source: &str) -> arith::Expr<'_> {
    parse(source).unwrap_or_else(|error| panic!("{source:?} did not parse: {error}"))
}

fn shape(value: &arith::Expr<'_>) -> String {
    match value {
        arith::Expr::IntLit(atom) => format!("int:{}", atom.value.value.text()),
        arith::Expr::Ident(atom) => format!("ident:{}", atom.value.value.text()),
        arith::Expr::Paren(inner) => format!("paren({})", shape(inner)),
        arith::Expr::Neg(inner) => format!("neg({})", shape(inner)),
        arith::Expr::Add(left, right) => format!("add({},{})", shape(left), shape(right)),
        arith::Expr::Sub(left, right) => format!("sub({},{})", shape(left), shape(right)),
        arith::Expr::Mul(left, right) => format!("mul({},{})", shape(left), shape(right)),
        arith::Expr::Div(left, right) => format!("div({},{})", shape(left), shape(right)),
    }
}

#[test]
fn int_literal_alone() {
    assert_eq!(shape(&parsed("42")), "int:42");
}

#[test]
fn ident_alone() {
    assert_eq!(shape(&parsed("alpha_1")), "ident:alpha_1");
}

#[test]
fn empty_input_is_error() {
    let Err(error) = parse("") else {
        panic!("empty input unexpectedly parsed")
    };
    assert_eq!(error.kind(), recursa::ParseErrorKind::UnexpectedEof);
}

#[test]
fn simple_add() {
    assert_eq!(shape(&parsed("1 + 2")), "add(int:1,int:2)");
}

#[test]
fn precedence_add_then_mul() {
    assert_eq!(shape(&parsed("1 + 2 * 3")), "add(int:1,mul(int:2,int:3))");
}

#[test]
fn precedence_mul_then_add() {
    assert_eq!(shape(&parsed("1 * 2 + 3")), "add(mul(int:1,int:2),int:3)");
}

#[test]
fn left_associativity_minus() {
    assert_eq!(shape(&parsed("8 - 3 - 1")), "sub(sub(int:8,int:3),int:1)");
}

#[test]
fn left_associativity_div() {
    assert_eq!(shape(&parsed("8 / 4 / 2")), "div(div(int:8,int:4),int:2)");
}

#[test]
fn paren_overrides_precedence() {
    assert_eq!(
        shape(&parsed("(1 + 2) * 3")),
        "mul(paren(add(int:1,int:2)),int:3)"
    );
}

#[test]
fn nested_parens() {
    assert_eq!(shape(&parsed("((x))")), "paren(paren(ident:x))");
}

#[test]
fn unclosed_paren_errors() {
    assert!(parse("(1 + 2").is_err());
}

#[test]
fn prefix_minus_lone() {
    assert_eq!(shape(&parsed("-1")), "neg(int:1)");
}

#[test]
fn prefix_minus_binds_tighter_than_mul() {
    assert_eq!(shape(&parsed("-1 * 2")), "mul(neg(int:1),int:2)");
}

#[test]
fn rhs_can_have_prefix_minus() {
    assert_eq!(shape(&parsed("1 * -2")), "mul(int:1,neg(int:2))");
    assert_eq!(shape(&parsed("1 + -2")), "add(int:1,neg(int:2))");
}

#[test]
fn double_prefix_minus() {
    assert_eq!(shape(&parsed("--x")), "neg(neg(ident:x))");
}

#[test]
fn tokenize_handles_whitespace_and_mixed_input() {
    let lexed = arith::lex("  foo + 12\n* (bar - 3) ");
    assert_eq!(lexed.errors().count(), 0);
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(
        tokens.iter().map(|token| token.text()).collect::<Vec<_>>(),
        ["foo", "+", "12", "*", "(", "bar", "-", "3", ")"]
    );
    assert_eq!(tokens[0].kind(), arith::TokenKind::Ident);
    assert_eq!(tokens[1].kind(), arith::TokenKind::PLUS);
    assert_eq!(tokens[2].kind(), arith::TokenKind::IntLit);
    assert_eq!(tokens[3].kind(), arith::TokenKind::STAR);
    assert_eq!(tokens[4].kind(), arith::TokenKind::LPAREN);
    assert_eq!(tokens[5].kind(), arith::TokenKind::Ident);
    assert_eq!(tokens[6].kind(), arith::TokenKind::MINUS);
    assert_eq!(tokens[7].kind(), arith::TokenKind::IntLit);
    assert_eq!(tokens[8].kind(), arith::TokenKind::RPAREN);
}
