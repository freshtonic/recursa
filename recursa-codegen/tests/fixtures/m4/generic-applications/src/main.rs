use recursa::Pretty as _;
use recursa_generated_generic_applications::grammar;

fn main() {
    fn assert_traits<T: std::fmt::Debug + Clone + PartialEq + recursa::Pretty>() {}
    assert_traits::<grammar::Atom<grammar::Integer<'static>>>();
    assert_traits::<grammar::Atom<grammar::Name<'static>>>();

    let lexed = grammar::lex("-12 - name");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse_spanned(&mut input).expect("generic applications parse");
    assert!(input.is_eof());
    assert_eq!(parsed.render(), "-12 - name");
    assert_eq!(parsed.root().sub_minus_span(), Some(recursa::Span::new(4, 5).unwrap()));

    let negated = grammar::lex("-12");
    let mut negated_input = negated.input();
    let negated = grammar::Expr::parse_spanned(&mut negated_input).expect("prefix parses");
    assert_eq!(
        negated.root().neg_minus_span(),
        Some(recursa::Span::new(0, 1).unwrap())
    );

    let detached = parsed.into_ast();
    match detached {
        grammar::Expr::Sub(lhs, rhs) => {
            assert!(matches!(*lhs, grammar::Expr::Neg(_)));
            assert!(matches!(*rhs, grammar::Expr::Name(grammar::Atom { .. })));
        }
        _ => panic!("infix subtraction is the root"),
    }

    let synthetic_integer = grammar::Atom {
        value: grammar::Integer {
            value: grammar::IntegerText::new("7"),
        },
    };
    let synthetic_name = grammar::Atom {
        value: grammar::Name {
            value: grammar::NameText::new("alpha"),
        },
    };
    assert_eq!(synthetic_integer.render(), "7");
    assert_eq!(synthetic_name.render(), "alpha");

    println!("generic-applications-ok");
}
