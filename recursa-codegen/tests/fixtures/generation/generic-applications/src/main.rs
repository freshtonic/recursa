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

    let not_exists = grammar::lex("not exists alpha");
    let mut not_exists_input = not_exists.input();
    let not_exists = grammar::Expr::parse_spanned(&mut not_exists_input)
        .expect("multi-token prefix with a shared fixed token parses");
    assert_eq!(
        not_exists.root().not_exists_not_span(),
        Some(recursa::Span::new(0, 3).unwrap())
    );

    let is_not = grammar::lex("alpha is not beta");
    let mut is_not_input = is_not.input();
    let is_not = grammar::Expr::parse_spanned(&mut is_not_input)
        .expect("multi-token infix with a shared fixed token parses");
    assert_eq!(
        is_not.root().is_not_not_span(),
        Some(recursa::Span::new(9, 12).unwrap())
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

    fn assert_generic_declarations<T: recursa::Pretty>() {
        fn assert_pretty<U: recursa::Pretty>() {}
        assert_pretty::<grammar::OptionalOnly<T>>();
        assert_pretty::<grammar::Aliased<T>>();
        assert_pretty::<grammar::Chain<T>>();
        assert_pretty::<grammar::Separated<T>>();
        assert_pretty::<grammar::Wrap<T>>();
        assert_pretty::<grammar::AliasLifetimeRoot<'static, T>>();
    }
    assert_generic_declarations::<grammar::Name<'static>>();

    fn assert_serde<T>()
    where
        T: recursa::__private::serde::Serialize,
        T: for<'de> recursa::__private::serde::Deserialize<'de>,
    {
    }
    assert_serde::<grammar::OptionalOnly<grammar::Name<'static>>>();
    assert_serde::<grammar::Aliased<grammar::Integer<'static>>>();
    assert_serde::<grammar::AliasLifetimeRoot<'static, grammar::Integer<'static>>>();
    assert_serde::<grammar::Chain<grammar::Name<'static>>>();

    let audit = grammar::lex("(alpha)[7]{1{2}}{a{b}}(3,c)");
    let mut audit_input = audit.input();
    let parsed = grammar::AuditRoot::parse_spanned(&mut audit_input)
        .expect("authored generic declaration cases parse");
    assert!(audit_input.is_eof());
    let rendered = parsed.render();
    assert_eq!(rendered, "(alpha)[7]{1{2}}{a{b}}(3, c)");
    let reparsed = grammar::lex(&rendered);
    let mut reparsed_input = reparsed.input();
    let reparsed = grammar::AuditRoot::parse_spanned(&mut reparsed_input)
        .expect("Pretty output for adjacent generic applications reparses");
    assert!(reparsed_input.is_eof());
    assert_eq!(reparsed.render(), rendered);

    for (source, expected) in [("(MiXeD)", "(MiXeD)"), ("[One TWO]", "[One TWO]")] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::Mixed::parse_spanned(&mut input)
            .expect("mixed direct and repeated generic applications parse");
        assert!(input.is_eof());
        assert_eq!(parsed.render(), expected);
    }

    let generic_keyword = grammar::lex("iS MiXeD");
    let mut generic_keyword_input = generic_keyword.input();
    let generic_keyword = grammar::GenericKeywordRoot::parse_spanned(&mut generic_keyword_input)
        .expect("generic-dependent child with a keyword parses");
    assert!(generic_keyword_input.is_eof());
    let (generic_keyword_rendered, generic_keyword_map) = generic_keyword.render_mapped();
    assert_eq!(generic_keyword_rendered, "iS MiXeD");
    assert!(generic_keyword_map.entries().iter().any(|entry| {
        entry.source() == recursa::Span::new(0, 2).unwrap()
            && &generic_keyword_rendered[entry.output()] == "iS"
    }));
    let reparsed = grammar::lex(&generic_keyword_rendered);
    let mut reparsed_input = reparsed.input();
    let reparsed = grammar::GenericKeywordRoot::parse_spanned(&mut reparsed_input)
        .expect("mapped generic Pretty output reparses");
    assert!(reparsed_input.is_eof());
    assert_eq!(reparsed.render(), generic_keyword_rendered);

    let attached_generic = grammar::lex("{Is MiXeD}");
    let mut attached_generic_input = attached_generic.input();
    let attached_generic =
        grammar::GenericAttachedRoot::parse_spanned(&mut attached_generic_input)
            .expect("attached generic-dependent child with a keyword parses");
    assert!(attached_generic_input.is_eof());
    let (attached_generic_rendered, attached_generic_map) = attached_generic.render_mapped();
    assert_eq!(attached_generic_rendered, "{Is MiXeD}");
    assert!(attached_generic_map.entries().iter().any(|entry| {
        entry.source() == recursa::Span::new(1, 3).unwrap()
            && &attached_generic_rendered[entry.output()] == "Is"
    }));
    let reparsed = grammar::lex(&attached_generic_rendered);
    let mut reparsed_input = reparsed.input();
    let reparsed = grammar::GenericAttachedRoot::parse_spanned(&mut reparsed_input)
        .expect("mapped attached generic Pretty output reparses");
    assert!(reparsed_input.is_eof());
    assert_eq!(reparsed.render(), attached_generic_rendered);

    let alias = grammar::lex("({7, marker})");
    let mut alias_input = alias.input();
    let parsed = grammar::AliasLifetimeConcrete::parse_spanned(&mut alias_input)
        .expect("cross-module alias lifetime is rebound at its use site");
    assert!(alias_input.is_eof());
    assert_eq!(parsed.render(), "({7, marker})");

    let separated = grammar::lex("[One] aNd [TWO]");
    let mut separated_input = separated.input();
    let separated = grammar::SeparatorAudit::parse_spanned(&mut separated_input)
        .expect("ASCII-insensitive word-separated generic application parses");
    assert!(separated_input.is_eof());
    let separated = separated.into_ast().separated;
    assert_eq!(separated.values[0].value.text(), "One");
    assert_eq!(separated.values[1].value.text(), "TWO");
    assert_eq!(separated.render(), "[One] AND [TWO]");

    let rendered = separated.render();
    let reparsed = grammar::lex(&rendered);
    let mut reparsed_input = reparsed.input();
    grammar::SeparatorAudit::parse_spanned(&mut reparsed_input)
        .expect("canonical word-separated Pretty output reparses");
    assert!(reparsed_input.is_eof());

    println!("generic-applications-ok");
}
