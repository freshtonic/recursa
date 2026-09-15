recursa::grammar! {
    module = crate::grammar,
    derives(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Pretty, Visit, VisitMut, Arbitrary, serde::Serialize, serde::Deserialize),
    keyword_matching = ascii_insensitive,
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

recursa::ast_node! {
    /// RECURSA_AUTHORED_RECORD_DOC: source-authored documentation remains authoritative.

    #[tok(RECORD, this)]
    pub struct Record {
        #[tok(COLON)]
        pub label: boxed!(boxed!(Label)),
        pub count: boxed!(MaybeCount<boxed!(Count)>),
    }
}

recursa::ast_node! {
    /// Required fixed syntax composes with an optional semantic presence value.

    pub struct RequiredThenPresence {
        #[tok(INDEX)]
        #[presence(CONCURRENTLY)]
        pub concurrently: bool,
        pub label: Label,
    }
}

recursa::ast_node! {
    /// Required syntax may wrap the optional presence at the same field boundary.

    pub struct WrappedPresence {
        #[tok(CREATE, this, MATERIALIZED, VIEW)]
        #[presence(UNLOGGED)]
        pub unlogged: bool,
        pub label: Label,
    }
}

recursa::ast_node! {
    /// A required suffix may follow an optional presence without making it conditional.

    #[tok(IS, this)]
    pub struct SuffixedPresence {
        #[tok(this, JSON)]
        #[presence(NOT)]
        pub not: bool,
        pub label: Label,
    }
}

recursa::ast_node! {
    /// A multi-token presence is selected atomically and may share its first token with the tail.

    pub struct AtomicPresence {
        #[presence(OR, REPLACE)]
        pub replace: bool,
        pub tail: PresenceTail,
    }
}

recursa::ast_node! {

    pub enum PresenceTail {
        Or {
            #[tok(OR)]
            label: Label,
        },
        Label {
            label: Label,
        },
    }
}

recursa::ast_node! {
    /// Punctuation participates in presence matching and canonical Pretty output.

    pub struct PunctuationPresence {
        #[presence(BANG)]
        pub bang: bool,
        pub label: Label,
    }
}

recursa::ast_node! {
    /// Presence fields keep their authored bool shape inside enum variants.

    pub enum EnumPresence {
        Value {
            #[presence(NOT)]
            not: bool,
            label: Label,
        },
    }
}

recursa::ast_node! {
    /// Erased fixed-token groups may occur independently before and after `this`.

    #[tok(optional(RECORD), this, optional(PAIR))]
    pub struct OptionalFillers {
        pub label: Label,
    }
}

recursa::ast_node! {
    /// Optional groups preserve their exact position among required field attachments.

    pub struct AttachedOptionalFillers {
        #[tok(CREATE, optional(UNLOGGED), this, optional(VIEW), MATERIALIZED)]
        pub label: Label,
    }
}

recursa::ast_node! {
    /// An incomplete optional group leaves a shared-prefix semantic tail untouched.

    pub struct AtomicOptionalAttachment {
        #[tok(optional(OR, REPLACE), this)]
        pub tail: PresenceTail,
    }
}

recursa::ast_node! {
    /// An optional suffix stays outside the following nested optional container.

    pub struct NestedWith {
        #[tok(this, optional(WITH))]
        pub name: Label,
        pub options: boxed!(Option<boxed!(Pair)>),
    }
}

recursa::ast_node! {
    /// Erased optional groups compose with a boolean presence attachment.

    pub struct PresenceWithFillers {
        #[tok(CREATE, optional(OR), this, optional(VIEW), MATERIALIZED)]
        #[presence(UNLOGGED)]
        pub unlogged: bool,
        pub label: Label,
    }
}

recursa::ast_node! {
    /// Prediction distinguishes a complete optional prefix from a shorter fixed alternative.

    pub enum OptionalPrefixPrediction {
        WithReplace {
            #[tok(optional(OR, REPLACE), this)]
            label: Label,
        },
        Or {
            #[tok(OR)]
            label: Label,
        },
    }
}

pub type MaybeCount<T> = Option<T>;

recursa::ast_node! {

    #[tok(PAIR, this)]
    pub struct Pair(pub boxed!(Label), pub Option<boxed!(Count)>);
}

recursa::ast_node! {
    /// A nested child must leave its owner's fixed suffix unconsumed.

    #[tok(CREATE, this, VIEW)]
    pub struct NestedRecovery {
        pub pair: boxed!(Pair),
    }
}

recursa::ast_node! {

    #[tok(LEFT, this)]
    pub struct ManyLabels {
        #[sep(COLON, trailing)]
        pub values: zero_or_many!(boxed!(Label)),
    }
}

recursa::ast_node! {

    #[tok(RIGHT, this)]
    pub struct SomeCounts {
        #[sep(COLON)]
        pub values: Option<boxed!(one_or_many!(boxed!(Count)))>,
    }
}

recursa::ast_node! {

    #[tok(CREATE, this)]
    pub struct RequiredCounts {
        #[sep(COLON)]
        pub values: one_or_many!(boxed!(Count)),
    }
}

recursa::ast_node! {
    /// A zero-occurrence `Vec` remains absent before a later fixed suffix.

    #[tok(LEFT, this, MATERIALIZED)]
    pub struct ZeroLabelsThenSuffix {
        #[sep(COLON)]
        pub values: zero_or_many!(boxed!(Label)),
    }
}

recursa::ast_node! {
    /// An absent `Option<Vec1<_>>` remains valid before a later fixed suffix.

    #[tok(RIGHT, this, VIEW, MATERIALIZED)]
    pub struct OptionalCountsThenSuffix {
        #[sep(COLON)]
        pub values: Option<boxed!(one_or_many!(boxed!(Count)))>,
    }
}

recursa::ast_node! {
    /// Each required attachment token is parsed by its own fixed transition.

    pub struct MultipleRequiredAttachment {
        #[tok(CREATE, LEFT, this, RIGHT, VIEW)]
        pub label: Label,
    }
}

recursa::ast_node! {
    /// Repeated identical required sites prove exact diagnostic deduplication is non-vacuous.

    pub struct DuplicateRequiredAttachment {
        #[tok(CREATE, CREATE, CREATE, this)]
        pub label: Label,
    }
}

recursa::ast_node! {
    /// A two-token optional group is consumed atomically before its semantic value.

    #[tok(optional(OR, REPLACE), this)]
    pub struct StandaloneAtomicOptional {
        pub label: Label,
    }
}

recursa::ast_node! {

    pub enum Entry {
        Record {
            value: boxed!(boxed!(Record)),
        },
        Pair {
            value: boxed!(Pair),
        },
        #[tok(EMPTY)]
        Empty,
    }
}

recursa::ast_node! {
    /// Distinct fixed prefixes select a branch before its field parses.

    pub enum RecoveryChoice {
        Right {
            #[tok(RIGHT)]
            count: Count,
        },
        Left {
            #[tok(LEFT)]
            label: Label,
        },
    }
}

recursa::ast_node! {
    /// A nested enum restart may overlap its owner's fixed FOLLOW token.

    pub enum RestartStopChoice {
        #[tok(LEFT)]
        Left,
        #[tok(RIGHT)]
        Right,
    }
}

recursa::ast_node! {

    #[tok(PAIR, this, LEFT)]
    pub struct RestartStopOwner {
        pub choice: RestartStopChoice,
    }
}

recursa::ast_node! {

    pub struct Label {
        #[lex(pattern = r"[A-Za-zé]+")]
        pub text: LabelText,
    }
}

recursa::ast_node! {

    #[omit(Clone)]
    pub struct Count {
        #[lex(pattern = r"[0-9]+")]
        pub text: CountText,
    }
}

pub mod public_left {
    recursa::ast_node! {

        #[tok(LEFT)]
        pub struct Duplicate;
    }
}

pub mod public_right {
    recursa::ast_node! {

        #[tok(RIGHT)]
        pub struct Duplicate;
    }
}

pub mod node_named_like_token {
    recursa::ast_node! {

        #[tok(LEFT)]
        pub struct SharedText;
    }

    recursa::ast_node! {

        #[tok(RIGHT, this)]
        pub struct UsesNode {
            pub value: SharedText,
        }
    }
}

recursa::ast_node! {

    #[tok(EMPTY)]
    pub struct Opaque;
}

recursa::ast_node! {

    #[tok(LEFT, this)]
    pub struct PrivateRoot {
        child: private_nodes::Hidden,
    }
}

mod private_nodes {
    recursa::ast_node! {

        pub(super) struct Hidden {
            #[lex(pattern = r"~[a-z]+")]
            pub(super) text: PrivateText,
        }
    }
}

recursa::ast_node! {
    #[allow(non_camel_case_types)]
    #[tok(PAIR)]
    pub struct __RecursaTarget_11_public_left_9_Duplicate;
}

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
    recursa::ast_node! {

        struct Local {
            #[lex(pattern = r"@[a-z]+")]
            text: LocalText,
        }
    }

    type MaybeLocalText<T> = Option<T>;

    recursa::ast_node! {

        #[tok(LEFT, this)]
        struct OptionalLocal {
            text: boxed!(MaybeLocalText<boxed!(LocalText)>),
        }
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
    recursa::ast_node! {

        struct Value {
            #[lex(pattern = r"#[a-z]+")]
            pub(super) text: SharedText,
        }
    }

    #[allow(dead_code)]
    pub(super) fn parse() -> SharedText<'static> {
        SharedText::new("#left")
    }
}

mod shared_right {
    recursa::ast_node! {

        struct Value {
            #[lex(pattern = r"#[a-z]+")]
            pub(super) text: SharedText,
        }
    }

    pub(super) fn parse() -> SharedText<'static> {
        SharedText::new("#right")
    }
}

mod left {
    use recursa::HasSpan as _;

    recursa::ast_node! {

        #[node(derive(Copy))]
        #[tok(LEFT)]
        struct Hidden;
    }

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

    recursa::ast_node! {

        #[tok(RIGHT)]
        struct Hidden;
    }

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
