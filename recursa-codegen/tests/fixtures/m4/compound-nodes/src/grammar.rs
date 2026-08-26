recursa::grammar! {
    module = crate::grammar,
    derives(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Pretty, Visit, VisitMut, Arbitrary, serde::Serialize, serde::Deserialize),
    keyword_matching = ascii_insensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        RECORD => "record",
        PAIR => "pair",
        EMPTY => "empty",
        LEFT => "left",
        RIGHT => "right",
    }
    punctuation {
        COLON => ":",
    }
}

#[allow(dead_code, non_snake_case)]
fn RecordView() {}

#[cfg(any())]
struct RecordView;

/// RECURSA_AUTHORED_RECORD_DOC: source-authored documentation remains authoritative.
#[derive(recursa::Node)]
#[tok(RECORD, this)]
pub struct Record<'input> {
    #[tok(COLON)]
    pub label: Box<Box<Label<'input>>>,
    pub count: MaybeCount<'input>,
}

pub type MaybeCount<'input> = Box<Option<Box<Count<'input>>>>;

#[derive(recursa::Node)]
#[tok(PAIR, this)]
pub struct Pair<'input>(pub Box<Label<'input>>, pub Option<Box<Count<'input>>>);

#[derive(recursa::Node)]
#[tok(LEFT, this)]
pub struct ManyLabels<'input> {
    #[sep(COLON, trailing)]
    pub values: Vec<Box<Label<'input>>>,
}

#[derive(recursa::Node)]
#[tok(RIGHT, this)]
pub struct SomeCounts<'input> {
    #[sep(COLON)]
    pub values: Option<Box<recursa::Vec1<Box<Count<'input>>>>>,
}

#[derive(recursa::Node)]
pub enum Entry<'input> {
    Record {
        value: Box<Box<Record<'input>>>,
    },
    Pair {
        value: Box<Pair<'input>>,
    },
    #[tok(EMPTY)]
    Empty,
}

#[derive(recursa::Node)]
pub struct Label<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub text: LabelText<'input>,
}

#[derive(recursa::Node)]
#[node(omit(Clone))]
pub struct Count<'input> {
    #[lex(pattern = r"[0-9]+")]
    pub text: CountText<'input>,
}

pub mod public_left {
    #[derive(recursa::Node)]
    #[tok(LEFT)]
    pub struct Duplicate;
}

pub mod public_right {
    #[derive(recursa::Node)]
    #[tok(RIGHT)]
    pub struct Duplicate;
}

#[derive(recursa::Node)]
#[tok(EMPTY)]
pub struct Opaque;

#[allow(non_camel_case_types)]
#[derive(recursa::Node)]
#[tok(PAIR)]
pub struct __RecursaTarget_11_public_left_9_Duplicate;

impl<'input> Clone for Count<'input> {
    fn clone(&self) -> Self {
        Self {
            text: self.text.clone(),
        }
    }
}

pub fn assert_private_sibling_spans() {
    left::assert_hidden_span();
    right::assert_hidden_span();
    nested::assert_local_token_binding();
    let right = shared_right::parse();
    let _: shared_left::SharedText<'_> = right;
}

mod nested {
    #[derive(recursa::Node)]
    struct Local<'input> {
        #[lex(pattern = r"@[a-z]+")]
        text: LocalText<'input>,
    }

    type MaybeLocalText<'input> = Box<Option<Box<LocalText<'input>>>>;

    #[derive(recursa::Node)]
    #[tok(LEFT, this)]
    struct OptionalLocal<'input> {
        text: MaybeLocalText<'input>,
    }

    pub(super) fn assert_local_token_binding() {
        let lexed = super::lex("@nested");
        let mut input = lexed.input();
        let parsed = Local::parse(&mut input).expect("nested local content token parses");
        assert!(input.is_eof());
        assert_eq!(parsed.into_ast().text.text(), "@nested");

        let lexed = super::lex("LeFt @alias");
        let mut input = lexed.input();
        let parsed = OptionalLocal::parse(&mut input)
            .expect("container alias ending in a content token parses");
        assert!(input.is_eof());
        assert_eq!(
            parsed
                .into_ast()
                .text
                .as_ref()
                .as_ref()
                .expect("optional alias is present")
                .text(),
            "@alias"
        );

        let lexed = super::lex("left");
        let mut input = lexed.input();
        let parsed = OptionalLocal::parse(&mut input)
            .expect("absent container alias ending in a content token parses");
        assert!(input.is_eof());
        assert!(parsed.into_ast().text.as_ref().is_none());
    }
}

mod shared_left {
    #[derive(recursa::Node)]
    struct Value<'input> {
        #[lex(pattern = r"#[a-z]+")]
        pub(super) text: SharedText<'input>,
    }

    #[allow(dead_code)]
    pub(super) fn parse() -> SharedText<'static> {
        SharedText::new("#left")
    }
}

mod shared_right {
    #[derive(recursa::Node)]
    struct Value<'input> {
        #[lex(pattern = r"#[a-z]+")]
        pub(super) text: SharedText<'input>,
    }

    pub(super) fn parse() -> SharedText<'static> {
        SharedText::new("#right")
    }
}

mod left {
    use recursa::HasSpan as _;

    #[derive(recursa::Node)]
    #[node(derive(Copy))]
    #[tok(LEFT)]
    struct Hidden;

    pub(super) fn assert_hidden_span() {
        let lexed = super::lex("left");
        let mut input = lexed.input();
        let parsed = Hidden::parse_spanned(&mut input).expect("left private sibling parses");
        assert!(input.is_eof());
        let _: HiddenView<'_> = parsed.root();
        assert_eq!(
            parsed.root().span(),
            Some(recursa::Span::new(0, 4).unwrap())
        );
        assert_eq!(parsed.root().left_span(), parsed.root().span());
        let elements = parsed.root().span_elements();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements.get(0).unwrap().name(), Some("left"));
        assert_eq!(
            elements.get(0).unwrap().role(),
            recursa::ElementRole::FixedToken
        );

        struct Counter(usize);
        impl<'input> recursa::TotalVisitor<super::__RecursaVisitEvents<'input>> for Counter {
            type Error = ();
            fn total_enter<'visit>(
                &mut self,
                _event: super::__RecursaVisitEvent<'visit, 'input>,
            ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
            where
                super::__RecursaVisitEvents<'input>: 'visit,
            {
                self.0 += 1;
                std::ops::ControlFlow::Continue(())
            }
            fn total_exit<'visit>(
                &mut self,
                _event: super::__RecursaVisitEvent<'visit, 'input>,
            ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
            where
                super::__RecursaVisitEvents<'input>: 'visit,
            {
                std::ops::ControlFlow::Continue(())
            }
        }
        let ast = parsed.into_ast();
        fn requires_copy<T: Copy>(_: T) {}
        requires_copy(ast);
        let mut counter = Counter(0);
        assert!(recursa::Visit::visit(&ast, &mut counter).is_continue());
        assert_eq!(counter.0, 1, "private Nodes participate in traversal");
    }
}

mod right {
    use recursa::HasSpan as _;

    #[derive(recursa::Node)]
    #[tok(RIGHT)]
    struct Hidden;

    pub(super) fn assert_hidden_span() {
        let lexed = super::lex("right");
        let mut input = lexed.input();
        let parsed = Hidden::parse_spanned(&mut input).expect("right private sibling parses");
        assert!(input.is_eof());
        let _: HiddenView<'_> = parsed.root();
        assert_eq!(
            parsed.root().span(),
            Some(recursa::Span::new(0, 5).unwrap())
        );
        assert_eq!(parsed.root().right_span(), parsed.root().span());
        let elements = parsed.root().span_elements();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements.get(0).unwrap().name(), Some("right"));
        assert_eq!(
            elements.get(0).unwrap().role(),
            recursa::ElementRole::FixedToken
        );
    }
}
