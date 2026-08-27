use recursa_example_pratt_expressions::{ParseFailure, grammar, parse};

fn integer<'ast>(expression: &'ast grammar::Expr<'_>) -> &'ast str {
    let grammar::Expr::Integer(integer) = expression else {
        panic!("expression is an integer");
    };
    integer.text.text()
}

#[test]
fn left_associativity_prefix_postfix_and_grouping_are_explicit() {
    let sum = parse("1 + 2 + 3").expect("sum parses");
    let grammar::Expr::Add(left, three) = sum else {
        panic!("addition is the root");
    };
    let grammar::Expr::Add(one, two) = *left else {
        panic!("addition is left associative");
    };
    assert_eq!(integer(&one), "1");
    assert_eq!(integer(&two), "2");
    assert_eq!(integer(&three), "3");

    let decorated = parse("-name!").expect("prefix and postfix expression parses");
    let grammar::Expr::Negate(negated) = decorated else {
        panic!("prefix operator is outermost");
    };
    let grammar::Expr::Factorial(factorial) = *negated else {
        panic!("postfix operator binds more tightly");
    };
    let grammar::Expr::Name(name) = *factorial else {
        panic!("operator operand is a name");
    };
    assert_eq!(name.text.text(), "name");

    let grouped = parse("(1 + 2) * 3").expect("grouped expression parses");
    let grammar::Expr::Multiply(left, three) = grouped else {
        panic!("grouping overrides precedence");
    };
    assert!(matches!(*left, grammar::Expr::Parenthesized(_)));
    assert_eq!(integer(&three), "3");
}

#[test]
fn invalid_or_trailing_expressions_are_rejected() {
    assert!(matches!(parse("@"), Err(ParseFailure::Lexical(_))));
    assert!(matches!(parse("1 +"), Err(ParseFailure::Syntax(_))));
    assert!(parse("1 2").is_err());
}

#[test]
fn precedence_and_associativity_are_visible_in_the_ast() {
    let expression = parse("1 + 2 * 3").expect("mixed-precedence expression parses");
    let grammar::Expr::Add(one, product) = expression else {
        panic!("addition is the root");
    };
    assert_eq!(integer(&one), "1");
    let grammar::Expr::Multiply(two, three) = *product else {
        panic!("multiplication binds more tightly");
    };
    assert_eq!(integer(&two), "2");
    assert_eq!(integer(&three), "3");

    let power = parse("2 ^ 3 ^ 4").expect("power expression parses");
    let grammar::Expr::Power(two, right) = power else {
        panic!("power is right associative");
    };
    assert_eq!(integer(&two), "2");
    let grammar::Expr::Power(three, four) = *right else {
        panic!("right operand contains the nested power");
    };
    assert_eq!(integer(&three), "3");
    assert_eq!(integer(&four), "4");
}

#[cfg(feature = "explorer")]
#[test]
fn recursive_pratt_expression_parses_through_the_erased_explorer_adapter() {
    let source = "1 + 2 * 3";
    let document = grammar::Expr::explorer()
        .parse(source)
        .expect("recursive Pratt expression parses through the adapter");
    assert_eq!(document.source(), Some(source));
    assert_eq!(document.root().active_variant().unwrap().name(), "Add");
}
