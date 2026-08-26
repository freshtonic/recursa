use recursa::{HasSpan as _, ParseErrorKind};
use recursa_generated_pratt::grammar;

fn expect_error<T>(result: Result<T, recursa::ParseError>) -> recursa::ParseError {
    match result {
        Ok(_) => panic!("parse unexpectedly succeeded"),
        Err(error) => error,
    }
}

fn parse(source: &str) -> recursa::Parsed<'_, grammar::Expr<'_>> {
    let lexed = grammar::lex(source);
    assert!(lexed.errors().next().is_none());
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse_spanned(&mut input).expect("expression parses");
    assert!(
        input.is_eof(),
        "parser stopped at token {} while parsing {source:?}",
        input.cursor()
    );
    parsed
}

fn main() {
    let lexed = grammar::lex("alpha + 12");
    assert_eq!(
        lexed.tokens().map(|token| token.text()).collect::<Vec<_>>(),
        ["alpha", "+", "12"]
    );

    let precedence = parse("a + 2 * 3");
    assert_eq!(precedence.root().span(), Some(recursa::Span::new(0, 9).unwrap()));
    match precedence.into_ast() {
        grammar::Expr::Add(lhs, rhs) => {
            match *lhs {
                grammar::Expr::Name(name) => assert_eq!(name.value.text(), "a"),
                _ => panic!("left atom must preserve exact identifier text"),
            }
            match *rhs {
                grammar::Expr::Multiply(lhs, rhs) => {
                    match *lhs {
                        grammar::Expr::Integer(integer) => {
                            assert_eq!(integer.value.text(), "2")
                        }
                        _ => panic!("multiplication lhs must be an integer"),
                    }
                    match *rhs {
                        grammar::Expr::Integer(integer) => {
                            assert_eq!(integer.value.text(), "3")
                        }
                        _ => panic!("multiplication rhs must be an integer"),
                    }
                }
                _ => panic!("addition must bind less tightly than multiplication"),
            }
        }
        _ => panic!("addition must bind less tightly than multiplication"),
    }

    let left = parse("1 + 2 + 3").into_ast();
    match left {
        grammar::Expr::Add(lhs, rhs) => {
            assert!(matches!(*lhs, grammar::Expr::Add(_, _)));
            assert!(matches!(*rhs, grammar::Expr::Integer(_)));
        }
        _ => panic!("rbp = lbp + 1 must be left associative"),
    }

    let right = parse("2 ^ 3 ^ 4").into_ast();
    match right {
        grammar::Expr::Power(lhs, rhs) => {
            assert!(matches!(*lhs, grammar::Expr::Integer(_)));
            assert!(matches!(*rhs, grammar::Expr::Power(_, _)));
        }
        _ => panic!("rbp = lbp must be right associative"),
    }

    let unary = parse("-x! + (2)");
    let root = unary.root();
    assert_eq!(root.span(), Some(recursa::Span::new(0, 9).unwrap()));
    assert_eq!(root.span_elements().len(), 3);
    assert_eq!(root.add_field_0().unwrap().span(), Some(recursa::Span::new(0, 3).unwrap()));
    assert_eq!(root.plus_span(), Some(recursa::Span::new(4, 5).unwrap()));
    assert_eq!(root.add_field_1().unwrap().span(), Some(recursa::Span::new(6, 9).unwrap()));
    match unary.into_ast() {
        grammar::Expr::Add(lhs, rhs) => {
            assert!(matches!(*lhs, grammar::Expr::Neg(_)));
            assert!(matches!(*rhs, grammar::Expr::Paren(_)));
        }
        _ => panic!("prefix, postfix, and grouping must follow planned binding powers"),
    }

    let missing_rhs = grammar::lex("1 +");
    let mut input = missing_rhs.input();
    let error = expect_error(grammar::Expr::parse(&mut input));
    assert_eq!(error.kind(), ParseErrorKind::UnexpectedEof);
    assert_eq!(error.span(), recursa::Span::new(3, 3).unwrap());
    assert_eq!(
        error.expected().collect::<Vec<_>>(),
        [
            "fixed:-",
            "fixed:(",
            "content:\"[0-9]+\":callback=-:priority=None",
            "content:\"[A-Za-z_][A-Za-z0-9_]*\":callback=-:priority=None",
        ]
    );

    let missing_close = grammar::lex("(1 + 2");
    let mut input = missing_close.input();
    let error = expect_error(grammar::Expr::parse(&mut input));
    assert_eq!(error.kind(), ParseErrorKind::UnexpectedEof);
    assert_eq!(error.span(), recursa::Span::new(6, 6).unwrap());
    assert_eq!(error.expected().collect::<Vec<_>>(), ["fixed:)"]);

    let wrong_rhs = grammar::lex("1 + )");
    let mut input = wrong_rhs.input();
    let error = expect_error(grammar::Expr::parse(&mut input));
    assert_eq!(error.kind(), ParseErrorKind::UnexpectedToken);
    assert_eq!(error.span(), recursa::Span::new(4, 5).unwrap());
    assert_eq!(error.found(), Some("fixed:)"));
    assert_eq!(
        error.expected().collect::<Vec<_>>(),
        [
            "fixed:-",
            "fixed:(",
            "content:\"[0-9]+\":callback=-:priority=None",
            "content:\"[A-Za-z_][A-Za-z0-9_]*\":callback=-:priority=None",
        ]
    );

    let ambiguous = grammar::lex("1 + 2");
    let mut input = ambiguous.input();
    let error = expect_error(grammar::Followed::parse(&mut input));
    assert_eq!(error.kind(), ParseErrorKind::PrattAmbiguity);
    assert_eq!(error.span(), recursa::Span::new(2, 3).unwrap());
    assert_eq!(error.found(), Some("fixed:+"));

    let nested_ambiguous = grammar::lex("1 + 2 * 3");
    let mut input = nested_ambiguous.input();
    let error = expect_error(grammar::NestedFollowed::parse(&mut input));
    assert_eq!(error.kind(), ParseErrorKind::PrattAmbiguity);
    assert_eq!(error.span(), recursa::Span::new(6, 7).unwrap());
    assert_eq!(error.found(), Some("fixed:*"));
    assert_eq!(
        error.expected().collect::<Vec<_>>(),
        ["fixed:*", "fixed:^", "fixed:!"]
    );

    println!("pratt-ok");
}
