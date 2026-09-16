use recursa::NodeView as _;
use recursa_generated_shared_suffix::grammar;

fn main() {
    // The dispatch commits where `Qualified` and `Named` part and leaves the call tail,
    // whose arguments are open expressions, to the selected variant's parser.
    for (source, qualified) in [
        ("f(a + b)", false),
        ("s.f(a + b)", true),
        ("f(a => b + c, d)", false),
        ("f(1 + 2)", false),
        ("f(NULL + 1)", false),
        ("f()", false),
        ("s.t.f(a, b + 1)", true),
    ] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::FuncCall::parse(&mut input)
            .unwrap_or_else(|error| panic!("`{source}` parses as a call: {error:?}"));
        assert!(input.is_eof(), "`{source}` is consumed through EOF");
        assert_eq!(
            matches!(parsed.root().value(), grammar::FuncCall::Qualified(_)),
            qualified,
            "`{source}` selects the variant its second token names",
        );
    }

    for (source, variant) in [
        ("f(a + b)", "func"),
        ("s.f(a + b)", "func"),
        ("f", "col"),
        ("(a + b)", "paren"),
        ("f(1 + 2)", "func"),
    ] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::Target::parse(&mut input)
            .unwrap_or_else(|error| panic!("`{source}` parses as a target: {error:?}"));
        assert!(input.is_eof(), "`{source}` is consumed through EOF");
        let selected = match parsed.root().value() {
            grammar::Target::Paren(_) => "paren",
            grammar::Target::Func(_) => "func",
            grammar::Target::Col(_) => "col",
        };
        assert_eq!(
            selected, variant,
            "`{source}` selects the target its prefix names"
        );
    }

    let lexed = grammar::lex("f(a +)");
    let mut input = lexed.input();
    let error = grammar::FuncCall::parse(&mut input)
        .err()
        .expect("a dangling operator is rejected by the expression parser, not the dispatch");
    assert_eq!(error.span(), recursa::Span::new(5, 6).unwrap());

    println!("shared-suffix-ok");
}
