use recursa::{HasSpan as _, ParseErrorKind, Pretty as _};
use recursa_generated_pratt::grammar;
use std::hash::{Hash as _, Hasher as _};

struct TraversalCount(usize);

impl<'input> recursa::TotalVisitor<grammar::__RecursaVisitEvents<'input>> for TraversalCount {
    type Error = ();

    fn total_enter<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEvent<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEvents<'input>: 'visit,
    {
        self.0 += 1;
        std::ops::ControlFlow::Continue(())
    }

    fn total_exit<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEvent<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEvents<'input>: 'visit,
    {
        std::ops::ControlFlow::Continue(())
    }
}

struct TraversalCountMut(usize);

impl<'input> recursa::TotalVisitorMut<grammar::__RecursaVisitEventsMut<'input>>
    for TraversalCountMut
{
    type Error = ();

    fn total_enter_mut<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEventMut<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEventsMut<'input>: 'visit,
    {
        self.0 += 1;
        std::ops::ControlFlow::Continue(())
    }

    fn total_exit_mut<'visit>(
        &mut self,
        _event: grammar::__RecursaVisitEventMut<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEventsMut<'input>: 'visit,
    {
        std::ops::ControlFlow::Continue(())
    }
}

fn expect_error<T>(result: Result<T, recursa::ParseError>) -> recursa::ParseError {
    match result {
        Ok(_) => panic!("parse unexpectedly succeeded"),
        Err(error) => error,
    }
}

fn parse(source: &str) -> recursa::Parsed<'_, grammar::Expr<'_>> {
    let lexed = grammar::lex(source);
    assert!(lexed.errors().next().is_none(), "lex errors in {source:?}");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse_spanned(&mut input).expect("expression parses");
    assert!(
        input.is_eof(),
        "parser stopped at token {} while parsing {source:?}",
        input.cursor()
    );
    parsed
}

fn name(text: &'static str) -> grammar::Expr<'static> {
    grammar::Expr::Name(grammar::Name {
        value: grammar::NameText::new(text),
    })
}

fn assert_pretty_pratt_parentheses() {
    let multiply_after_add = grammar::Expr::Multiply(
        Box::new(grammar::Expr::Add(Box::new(name("a")), Box::new(name("b")))),
        Box::new(name("c")),
    );
    assert_eq!(multiply_after_add.render(), "[a + b] * c");

    let left_nested_power = grammar::Expr::Power(
        Box::new(grammar::Expr::Power(Box::new(name("a")), Box::new(name("b")))),
        Box::new(name("c")),
    );
    assert_eq!(left_nested_power.render(), "[a ^ b] ^ c");

    let right_nested_power = grammar::Expr::Power(
        Box::new(name("a")),
        Box::new(grammar::Expr::Power(Box::new(name("b")), Box::new(name("c")))),
    );
    assert_eq!(right_nested_power.render(), "a ^ b ^ c");

    for expression in [multiply_after_add, left_nested_power, right_nested_power] {
        let formatted = expression.render();
        let reparsed = parse(&formatted);
        assert_eq!(reparsed.render(), formatted, "pretty output is idempotent");
    }
}

fn assert_pretty_multi_token_operators() {
    let infix = grammar::Expr::IsNot(Box::new(name("a")), Box::new(name("b")));
    assert_eq!(infix.render(), "a IS NOT b");

    let prefix = grammar::Expr::NotExists(Box::new(name("a")));
    assert_eq!(prefix.render(), "NEVER EXISTS a");

    let formatted = parse("a IS NOT b").render();
    assert_eq!(formatted, "a IS NOT b");
    assert_eq!(parse(&formatted).render(), formatted);

    let formatted = parse("NEVER EXISTS a").render();
    assert_eq!(formatted, "NEVER EXISTS a");
    assert_eq!(parse(&formatted).render(), formatted);

    let punctuation_prefix = grammar::Expr::Neg(Box::new(name("a")));
    assert_eq!(punctuation_prefix.render(), "-a");

    let punctuation_postfix = grammar::Expr::Factorial(Box::new(name("a")));
    assert_eq!(punctuation_postfix.render(), "a!");

    let tight_infix = grammar::Expr::Cast(Box::new(name("a")), Box::new(name("b")));
    assert_eq!(tight_infix.render(), "a::b");

    let word_postfix = grammar::Expr::IsNull(Box::new(name("a")));
    assert_eq!(word_postfix.render(), "a HAS NULL");
}

fn main() {
    fn assert_traits<T>()
    where
        T: std::fmt::Debug
            + Clone
            + PartialEq
            + Eq
            + PartialOrd
            + Ord
            + std::hash::Hash
            + recursa::Pretty
            + recursa::Visit
            + recursa::VisitMut,
    {
    }
    fn assert_arbitrary<T: for<'arbitrary> recursa::Arbitrary<'arbitrary>>() {}
    assert_traits::<grammar::Expr<'static>>();
    assert_arbitrary::<grammar::Expr<'static>>();
    assert_pretty_pratt_parentheses();
    assert_pretty_multi_token_operators();

    let lexed = grammar::lex("alpha + 12");
    assert_eq!(
        lexed.tokens().map(|token| token.text()).collect::<Vec<_>>(),
        ["alpha", "+", "12"]
    );

    let precedence = parse("a + 2 * 3");
    assert_eq!(precedence.render(), "a + 2 * 3");
    let mut traversal = TraversalCount(0);
    assert!(recursa::Visit::visit(&precedence, &mut traversal).is_continue());
    assert_eq!(traversal.0, 11, "Pratt lhs and rhs both participate");
    assert_eq!(precedence.root().span(), Some(recursa::Span::new(0, 9).unwrap()));
    let mut detached = precedence.into_ast();
    let cloned = detached.clone();
    assert_eq!(detached, cloned);
    assert_eq!(detached.cmp(&cloned), std::cmp::Ordering::Equal);
    assert!(format!("{detached:?}").starts_with("Add"));
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    detached.hash(&mut hash);
    assert_ne!(hash.finish(), 0);
    assert_eq!(recursa::Parsed::synthetic(cloned).render(), "a + 2 * 3");
    let mut mutations = TraversalCountMut(0);
    assert!(recursa::VisitMut::visit_mut(&mut detached, &mut mutations).is_continue());
    assert_eq!(mutations.0, 11, "mutable traversal includes the Pratt lhs");
    match detached {
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

    assert!(
        parse("1").into_ast() < parse("1 + 2").into_ast(),
        "Ord retains authored Pratt variant order"
    );

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

    let unary = parse("-x! + [2]");
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
            "fixed:NEVER",
            "fixed:-",
            "fixed:[",
            "content:\"[0-9]+\":callback=-:priority=None",
            "content:\"[A-Za-z_][A-Za-z0-9_]*\":callback=-:priority=None",
        ]
    );

    let missing_close = grammar::lex("[1 + 2");
    let mut input = missing_close.input();
    let error = expect_error(grammar::Expr::parse(&mut input));
    assert_eq!(error.kind(), ParseErrorKind::UnexpectedEof);
    assert_eq!(error.span(), recursa::Span::new(6, 6).unwrap());
    assert_eq!(error.expected().collect::<Vec<_>>(), ["fixed:]"]);

    let wrong_rhs = grammar::lex("1 + ]");
    let mut input = wrong_rhs.input();
    let error = expect_error(grammar::Expr::parse(&mut input));
    assert_eq!(error.kind(), ParseErrorKind::UnexpectedToken);
    assert_eq!(error.span(), recursa::Span::new(4, 5).unwrap());
    assert_eq!(error.found(), Some("fixed:]"));
    assert_eq!(
        error.expected().collect::<Vec<_>>(),
        [
            "fixed:NEVER",
            "fixed:-",
            "fixed:[",
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
        ["fixed:HAS", "fixed:*", "fixed:^", "fixed:!", "fixed:::"]
    );

    println!("pratt-ok");
}
