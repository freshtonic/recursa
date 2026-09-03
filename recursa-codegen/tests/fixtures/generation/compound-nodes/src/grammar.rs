recursa::grammar! {
    module = crate::grammar,
    derives(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Pretty, Visit, VisitMut, Arbitrary, serde::Serialize, serde::Deserialize),
    keyword_matching = ascii_insensitive,
    max_lookahead = 3,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        RECORD => "record",
        PAIR => "pair",
        EMPTY => "empty",
        LEFT => "left",
        RIGHT => "right",
        INDEX => "index",
        CONCURRENTLY => "concurrently",
        CREATE => "create",
        UNLOGGED => "unlogged",
        MATERIALIZED => "materialized",
        VIEW => "view",
        IS => "is",
        NOT => "not",
        JSON => "json",
        OR => "or",
        REPLACE => "replace",
        WITH => "with",
    }
    punctuation {
        COLON => ":", spacing = tight,
        BANG => "!", spacing = tight,
    }
}

#[allow(dead_code, non_snake_case)]
fn View() {}

#[cfg(any())]
struct View;

/// RECURSA_AUTHORED_RECORD_DOC: source-authored documentation remains authoritative.
#[derive(recursa::Node)]
#[tok(RECORD, this)]
pub struct Record<'input> {
    #[tok(COLON)]
    pub label: Box<Box<Label<'input>>>,
    pub count: MaybeCount<'input>,
}

/// Required fixed syntax composes with an optional semantic presence value.
#[derive(recursa::Node)]
pub struct RequiredThenPresence<'input> {
    #[tok(INDEX)]
    #[presence(CONCURRENTLY)]
    pub concurrently: bool,
    pub label: Label<'input>,
}

/// Required syntax may wrap the optional presence at the same field boundary.
#[derive(recursa::Node)]
pub struct WrappedPresence<'input> {
    #[tok(CREATE, this, MATERIALIZED, VIEW)]
    #[presence(UNLOGGED)]
    pub unlogged: bool,
    pub label: Label<'input>,
}

/// A required suffix may follow an optional presence without making it conditional.
#[derive(recursa::Node)]
#[tok(IS, this)]
pub struct SuffixedPresence<'input> {
    #[tok(this, JSON)]
    #[presence(NOT)]
    pub not: bool,
    pub label: Label<'input>,
}

/// A multi-token presence is selected atomically and may share its first token with the tail.
#[derive(recursa::Node)]
pub struct AtomicPresence<'input> {
    #[presence(OR, REPLACE)]
    pub replace: bool,
    pub tail: PresenceTail<'input>,
}

#[derive(recursa::Node)]
pub enum PresenceTail<'input> {
    Or {
        #[tok(OR)]
        label: Label<'input>,
    },
    Label {
        label: Label<'input>,
    },
}

/// Punctuation participates in presence matching and canonical Pretty output.
#[derive(recursa::Node)]
pub struct PunctuationPresence<'input> {
    #[presence(BANG)]
    pub bang: bool,
    pub label: Label<'input>,
}

/// Presence fields keep their authored bool shape inside enum variants.
#[derive(recursa::Node)]
pub enum EnumPresence<'input> {
    Value {
        #[presence(NOT)]
        not: bool,
        label: Label<'input>,
    },
}

/// Erased fixed-token groups may occur independently before and after `this`.
#[derive(recursa::Node)]
#[tok(optional(RECORD), this, optional(PAIR))]
pub struct OptionalFillers<'input> {
    pub label: Label<'input>,
}

/// Optional groups preserve their exact position among required field attachments.
#[derive(recursa::Node)]
pub struct AttachedOptionalFillers<'input> {
    #[tok(CREATE, optional(UNLOGGED), this, optional(VIEW), MATERIALIZED)]
    pub label: Label<'input>,
}

/// An incomplete optional group leaves a shared-prefix semantic tail untouched.
#[derive(recursa::Node)]
pub struct AtomicOptionalAttachment<'input> {
    #[tok(optional(OR, REPLACE), this)]
    pub tail: PresenceTail<'input>,
}

/// An optional suffix stays outside the following nested optional container.
#[derive(recursa::Node)]
pub struct NestedWith<'input> {
    #[tok(this, optional(WITH))]
    pub name: Label<'input>,
    pub options: Box<Option<Box<Pair<'input>>>>,
}

/// Erased optional groups compose with a boolean presence attachment.
#[derive(recursa::Node)]
pub struct PresenceWithFillers<'input> {
    #[tok(CREATE, optional(OR), this, optional(VIEW), MATERIALIZED)]
    #[presence(UNLOGGED)]
    pub unlogged: bool,
    pub label: Label<'input>,
}

/// Prediction distinguishes a complete optional prefix from a shorter fixed alternative.
#[derive(recursa::Node)]
pub enum OptionalPrefixPrediction<'input> {
    WithReplace {
        #[tok(optional(OR, REPLACE), this)]
        label: Label<'input>,
    },
    Or {
        #[tok(OR)]
        label: Label<'input>,
    },
}

pub type MaybeCount<'input> = Box<Option<Box<Count<'input>>>>;

#[derive(recursa::Node)]
#[tok(PAIR, this)]
pub struct Pair<'input>(pub Box<Label<'input>>, pub Option<Box<Count<'input>>>);

/// A nested child must leave its owner's fixed suffix unconsumed.
#[derive(recursa::Node)]
#[tok(CREATE, this, VIEW)]
pub struct NestedRecovery<'input> {
    pub pair: Box<Pair<'input>>,
}

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
#[tok(CREATE, this)]
pub struct RequiredCounts<'input> {
    #[sep(COLON)]
    pub values: recursa::Vec1<Box<Count<'input>>>,
}

/// A zero-occurrence `Vec` remains absent before a later fixed suffix.
#[derive(recursa::Node)]
#[tok(LEFT, this, MATERIALIZED)]
pub struct ZeroLabelsThenSuffix<'input> {
    #[sep(COLON)]
    pub values: Vec<Box<Label<'input>>>,
}

/// An absent `Option<Vec1<_>>` remains valid before a later fixed suffix.
#[derive(recursa::Node)]
#[tok(RIGHT, this, VIEW, MATERIALIZED)]
pub struct OptionalCountsThenSuffix<'input> {
    #[sep(COLON)]
    pub values: Option<Box<recursa::Vec1<Box<Count<'input>>>>>,
}

/// Each required attachment token is parsed by its own fixed transition.
#[derive(recursa::Node)]
pub struct MultipleRequiredAttachment<'input> {
    #[tok(CREATE, LEFT, this, RIGHT, VIEW)]
    pub label: Label<'input>,
}

/// Repeated identical required sites prove exact diagnostic deduplication is non-vacuous.
#[derive(recursa::Node)]
pub struct DuplicateRequiredAttachment<'input> {
    #[tok(CREATE, CREATE, CREATE, this)]
    pub label: Label<'input>,
}

/// A two-token optional group is consumed atomically before its semantic value.
#[derive(recursa::Node)]
#[tok(optional(OR, REPLACE), this)]
pub struct StandaloneAtomicOptional<'input> {
    pub label: Label<'input>,
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

/// Distinct fixed prefixes select a branch before its field parses.
#[derive(recursa::Node)]
pub enum RecoveryChoice<'input> {
    Right {
        #[tok(RIGHT)]
        count: Count<'input>,
    },
    Left {
        #[tok(LEFT)]
        label: Label<'input>,
    },
}

/// A nested enum restart may overlap its owner's fixed FOLLOW token.
#[derive(recursa::Node)]
pub enum RestartStopChoice {
    #[tok(LEFT)]
    Left,
    #[tok(RIGHT)]
    Right,
}

#[derive(recursa::Node)]
#[tok(PAIR, this, LEFT)]
pub struct RestartStopOwner {
    pub choice: RestartStopChoice,
}

#[derive(recursa::Node)]
pub struct Label<'input> {
    #[lex(pattern = r"[A-Za-zé]+")]
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

pub mod node_named_like_token {
    #[derive(recursa::Node)]
    #[tok(LEFT)]
    pub struct SharedText;

    #[derive(recursa::Node)]
    #[tok(RIGHT, this)]
    pub struct UsesNode {
        pub value: SharedText,
    }
}

#[derive(recursa::Node)]
#[tok(EMPTY)]
pub struct Opaque;

#[derive(recursa::Node)]
#[tok(LEFT, this)]
pub struct PrivateRoot<'input> {
    child: private_nodes::Hidden<'input>,
}

mod private_nodes {
    #[derive(recursa::Node)]
    pub(super) struct Hidden<'input> {
        #[lex(pattern = r"~[a-z]+")]
        pub(super) text: PrivateText<'input>,
    }
}

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
        let _: super::View<'_, Hidden> = parsed.root();
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
        let _: super::View<'_, Hidden> = parsed.root();
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
