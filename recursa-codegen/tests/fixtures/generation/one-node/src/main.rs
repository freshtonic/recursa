use recursa::{HasSpan as _, NodeView as _};
use recursa_generated_one_node::r#type as grammar;

struct VisitCount(usize);

impl<'input> recursa::TotalVisitor<grammar::__RecursaVisitEvents<'input>> for VisitCount {
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

struct VisitMutCount(usize);

impl<'input> recursa::TotalVisitorMut<grammar::__RecursaVisitEventsMut<'input>> for VisitMutCount {
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

fn assert_token_kind_value<T: Copy + Eq + std::hash::Hash + std::fmt::Debug>() {}

fn assert_ast_traits<T: Clone + std::fmt::Debug>() {}

fn parse_with_capability<T, I>(input: &mut I) -> Result<T::Output, recursa::ParseError>
where
    T: recursa::Parse<I>,
{
    T::parse(input)
}

fn main() {
    assert_token_kind_value::<grammar::TokenKind>();
    assert_ast_traits::<grammar::r#match<'static>>();
    assert!(option_env!("RECURSA_GENERATION").is_none());
    assert_eq!(
        grammar::grammar_info().name().split("::").next(),
        Some("type")
    );
    let source = String::from("alpha");
    let lexed = grammar::lex(&source);
    assert_eq!(lexed.source(), "alpha");
    let token = lexed.tokens().next().expect("one generated token");
    assert_eq!(token.kind(), grammar::TokenKind::WordText);
    assert_eq!(token.text(), "alpha");
    assert_eq!(token.span(), recursa::Span::new(0, 5).unwrap());
    assert_eq!(lexed.tokens().count(), 1);

    let mut input = lexed.input();
    let parsed = parse_with_capability::<grammar::r#match<'_>, _>(&mut input)
        .expect("generated Parse capability succeeds");
    assert_eq!(parsed.root().value().text.text(), "alpha");
    assert!(std::ptr::eq(
        parsed.root().value().text.text().as_ptr(),
        source.as_ptr(),
    ));
    assert_eq!(parsed.root().span(), None);
    assert_eq!(parsed.root().text().span(), None);
    assert_eq!(input.cursor(), 1);
    assert!(input.is_eof());
    let mut ast = parsed.into_ast();
    assert_eq!(ast.text.text(), "alpha");
    ast.marker = Some(grammar::Marker);
    let mut visits = VisitCount(0);
    assert!(recursa::Visit::visit(&ast, &mut visits).is_continue());
    assert_eq!(
        visits.0, 3,
        "the parent, generated token, and lifetime-free child are visited",
    );
    let mut mutable_visits = VisitMutCount(0);
    assert!(recursa::VisitMut::visit_mut(&mut ast, &mut mutable_visits).is_continue());
    assert_eq!(
        mutable_visits.0, 3,
        "the parent, generated token, and lifetime-free child are visited mutably",
    );

    let synthetic = recursa::Parsed::synthetic(grammar::r#match {
        text: grammar::WordText::new(String::from("owned")),
        marker: None,
    });
    assert_eq!(synthetic.source(), None);
    assert_eq!(synthetic.root().span(), None);
    assert_eq!(synthetic.root().text().span(), None);
    assert_eq!(synthetic.root().value().text.text(), "owned");

    let spanned = grammar::lex("beta");
    let mut spanned_input = spanned.input();
    let spanned = grammar::r#match::parse_spanned(&mut spanned_input)
        .expect("generated spanned parse succeeds");
    assert_eq!(
        spanned.root().span(),
        Some(recursa::Span::new(0, 4).unwrap())
    );
    assert_eq!(
        spanned.root().text().span(),
        Some(recursa::Span::new(0, 4).unwrap())
    );

    let builder_lexed = grammar::lex("gamma");
    let mut builder_input = builder_lexed.input_with_builder(41_u32);
    let builder_parsed = parse_with_capability::<grammar::r#match<'_>, _>(&mut builder_input)
        .expect("the Parse capability supports a builder-mediated input");
    assert_eq!(builder_parsed.root().value().text.text(), "gamma");
    assert_eq!(*builder_input.builder(), 41);

    let empty = grammar::lex("");
    let mut empty_input = empty.input();
    let Err(error) = grammar::r#match::parse(&mut empty_input) else {
        panic!("empty input must not parse as the raw-named Node");
    };
    assert_eq!(error.kind(), recursa::ParseErrorKind::UnexpectedEof);
    assert_eq!(error.code(), "RCA4101");
    assert_eq!(error.span(), recursa::Span::new(0, 0).unwrap());
    assert_eq!(error.anchor(), recursa::Span::new(0, 0).unwrap());
    assert_eq!(empty_input.cursor(), 0);

    let unexpected = grammar::lex("!");
    let mut unexpected_input = unexpected.input();
    let Err(error) = grammar::r#match::parse(&mut unexpected_input) else {
        panic!("the marker token must not begin the raw-named Node");
    };
    assert_eq!(error.kind(), recursa::ParseErrorKind::UnexpectedToken);
    assert_eq!(error.code(), "RCA4100");
    assert_eq!(error.span(), recursa::Span::new(0, 1).unwrap());
    assert_eq!(error.anchor(), recursa::Span::new(0, 1).unwrap());
    assert_eq!(unexpected_input.cursor(), 0);

    println!("one-node-ok RCA4100 RCA4101");
}
