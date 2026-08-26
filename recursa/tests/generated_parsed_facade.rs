use recursa::{
    ExpectedAtomId, FollowSet, GrammarInfo, HasSpan, Input, LexResult, NodeView, ParseError,
    ParseErrorKind, Parsed, Span, TokenRecord,
};
use std::hash::{DefaultHasher, Hash, Hasher};

const WORD: u16 = 0;
const OTHER: u16 = 1;
const LEXICAL_ERROR: u16 = 2;
const WORD_ATOM: ExpectedAtomId = ExpectedAtomId(0);

static GRAMMAR: GrammarInfo = GrammarInfo {
    name: "generated-parsed",
    kind_names: &["word", "other", "lexical error"],
    atom_names: &["word"],
    lexical_error_kind: LEXICAL_ERROR,
};

static WRONG_GRAMMAR: GrammarInfo = GrammarInfo {
    name: "wrong-generated-parsed",
    kind_names: &["word", "other", "lexical error"],
    atom_names: &["word"],
    lexical_error_kind: LEXICAL_ERROR,
};

static WORD_START: FollowSet = recursa::__private::follow_set(3, &[1 << WORD], &[WORD_ATOM]);

#[derive(Clone, Copy)]
struct TokenView<'view, T> {
    value: &'view T,
    cursor: recursa::__private::ViewCursor<'view>,
}

impl<T> NodeView for TokenView<'_, T> {
    type Value = T;

    fn value(&self) -> &Self::Value {
        self.value
    }
}

impl<T> HasSpan for TokenView<'_, T> {
    fn span(&self) -> Option<Span> {
        self.cursor.span()
    }
}

macro_rules! generated_token {
    ($name:ident, $policy:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        struct $name<'input> {
            text: recursa::__private::ContentText<'input>,
        }

        impl $name<'_> {
            fn new(text: impl Into<Box<str>>) -> Self {
                Self {
                    text: recursa::__private::ContentText::owned(text.into()),
                }
            }

            fn text(&self) -> &str {
                self.text.as_str()
            }
        }

        impl<'input> recursa::__private::NodeViewFactory for $name<'input> {
            type View<'view>
                = TokenView<'view, Self>
            where
                Self: 'view;

            fn view<'view>(
                &'view self,
                cursor: recursa::__private::ViewCursor<'view>,
            ) -> Self::View<'view> {
                TokenView {
                    value: self,
                    cursor,
                }
            }
        }

        impl<'input> recursa::__private::Parse<'input> for $name<'input> {
            const CAPTURE_POLICY: recursa::__private::CapturePolicy =
                recursa::__private::CapturePolicy::$policy;

            fn grammar() -> &'static GrammarInfo {
                &GRAMMAR
            }

            fn root_follow() -> &'static FollowSet {
                &WORD_START
            }

            fn parse(
                context: &mut recursa::__private::ParseContext<'_, 'input, ()>,
            ) -> Result<recursa::__private::ParsedNode<Self>, ParseError> {
                let Some(record) = context.peek(0) else {
                    return Err(context.unexpected(&WORD_START));
                };
                if record.kind != WORD {
                    return Err(context.unexpected(&WORD_START));
                }
                context.advance();
                let text = context.matched_text(record)?;
                context.finish(Self { text }, record.span)
            }
        }

        impl<'input> $name<'input> {
            fn parse(input: &mut Input<'input>) -> Result<Parsed<'input, Self>, ParseError> {
                recursa::__private::parse(input)
            }

            fn parse_spanned(
                input: &mut Input<'input>,
            ) -> Result<Parsed<'input, Self>, ParseError> {
                recursa::__private::parse_spanned(input)
            }
        }
    };
}

generated_token!(OffToken, Off);
generated_token!(OptionalToken, Optional);
generated_token!(MandatoryToken, Mandatory);

#[derive(Debug, Eq, PartialEq)]
struct RecursiveRoot<'input> {
    token: OptionalToken<'input>,
}

#[derive(Clone, Copy)]
struct RecursiveRootView<'view, 'input> {
    value: &'view RecursiveRoot<'input>,
    cursor: recursa::__private::ViewCursor<'view>,
}

impl<'view, 'input> RecursiveRootView<'view, 'input> {
    fn token(&self) -> TokenView<'view, OptionalToken<'input>> {
        recursa::__private::NodeViewFactory::view(&self.value.token, self.cursor.child(0))
    }
}

impl HasSpan for RecursiveRootView<'_, '_> {
    fn span(&self) -> Option<Span> {
        self.cursor.span()
    }
}

impl<'input> NodeView for RecursiveRootView<'_, 'input> {
    type Value = RecursiveRoot<'input>;

    fn value(&self) -> &Self::Value {
        self.value
    }
}

impl<'input> recursa::__private::NodeViewFactory for RecursiveRoot<'input> {
    type View<'view>
        = RecursiveRootView<'view, 'input>
    where
        Self: 'view;

    fn view<'view>(
        &'view self,
        cursor: recursa::__private::ViewCursor<'view>,
    ) -> Self::View<'view> {
        RecursiveRootView {
            value: self,
            cursor,
        }
    }
}

impl<'input> recursa::__private::Parse<'input> for RecursiveRoot<'input> {
    const CAPTURE_POLICY: recursa::__private::CapturePolicy =
        recursa::__private::CapturePolicy::Off;

    fn grammar() -> &'static GrammarInfo {
        &GRAMMAR
    }

    fn root_follow() -> &'static FollowSet {
        &WORD_START
    }

    fn parse(
        context: &mut recursa::__private::ParseContext<'_, 'input, ()>,
    ) -> Result<recursa::__private::ParsedNode<Self>, ParseError> {
        let (token, occurrence) = context
            .parse_node::<OptionalToken<'input>>(
                &WORD_START,
                recursa::__private::CapturePolicy::Mandatory,
            )?
            .into_parts();
        context.finish_with_children(
            Self { token },
            Span::new(0, context.source().len()).unwrap(),
            [occurrence],
        )
    }
}

impl<'input> RecursiveRoot<'input> {
    fn parse(input: &mut Input<'input>) -> Result<Parsed<'input, Self>, ParseError> {
        recursa::__private::parse(input)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FailsAfterOccurrence;

impl recursa::__private::NodeViewFactory for FailsAfterOccurrence {
    type View<'view> = TokenView<'view, Self>;

    fn view<'view>(
        &'view self,
        cursor: recursa::__private::ViewCursor<'view>,
    ) -> Self::View<'view> {
        TokenView {
            value: self,
            cursor,
        }
    }
}

impl<'input> recursa::__private::Parse<'input> for FailsAfterOccurrence {
    const CAPTURE_POLICY: recursa::__private::CapturePolicy =
        recursa::__private::CapturePolicy::Mandatory;

    fn grammar() -> &'static GrammarInfo {
        &GRAMMAR
    }

    fn root_follow() -> &'static FollowSet {
        &WORD_START
    }

    fn parse(
        context: &mut recursa::__private::ParseContext<'_, 'input, ()>,
    ) -> Result<recursa::__private::ParsedNode<Self>, ParseError> {
        let record = context.advance().expect("fixture has a first token");
        let _partial = context.finish(Self, record.span)?;
        Err(context.unexpected(&WORD_START))
    }
}

impl FailsAfterOccurrence {
    fn parse<'input>(input: &mut Input<'input>) -> Result<Parsed<'input, Self>, ParseError> {
        recursa::__private::parse(input)
    }
}

fn one_word<'input>(source: &'input str) -> LexResult<'input> {
    LexResult::new(
        source,
        vec![TokenRecord {
            kind: WORD,
            span: Span::new(0, source.len()).unwrap(),
        }],
    )
}

#[test]
fn generated_parse_entries_return_one_stable_parsed_shape_for_all_capture_modes() {
    let off_lexed = one_word("off");
    let mut off_input = off_lexed.input(&GRAMMAR);
    let off = OffToken::parse(&mut off_input).unwrap();
    assert_eq!(off.source(), None);
    assert_eq!(off.root().value().text(), "off");
    assert_eq!(off.root().span(), None);

    let synthetic_off = Parsed::synthetic(OffToken::new("synthetic-off"));
    assert_eq!(synthetic_off.root().value().text(), "synthetic-off");

    let off_spanned_lexed = one_word("off-spanned");
    let mut off_spanned_input = off_spanned_lexed.input(&GRAMMAR);
    let off_spanned = OffToken::parse_spanned(&mut off_spanned_input).unwrap();
    assert_eq!(off_spanned.source(), None);
    assert_eq!(off_spanned.root().span(), None);

    let optional_lexed = one_word("optional");
    let mut optional_input = optional_lexed.input(&GRAMMAR);
    let optional = OptionalToken::parse(&mut optional_input).unwrap();
    assert_eq!(optional.source(), None);
    assert_eq!(optional.root().span(), None);

    let optional_spanned_lexed = one_word("optional-spanned");
    let mut optional_spanned_input = optional_spanned_lexed.input(&GRAMMAR);
    let optional_spanned = OptionalToken::parse_spanned(&mut optional_spanned_input).unwrap();
    assert_eq!(optional_spanned.source(), Some("optional-spanned"));
    assert_eq!(
        optional_spanned.root().span(),
        Some(Span::new(0, 16).unwrap())
    );

    let mandatory_lexed = one_word("mandatory");
    let mut mandatory_input = mandatory_lexed.input(&GRAMMAR);
    let mandatory = MandatoryToken::parse(&mut mandatory_input).unwrap();
    assert_eq!(mandatory.source(), Some("mandatory"));
    assert_eq!(mandatory.root().span(), Some(Span::new(0, 9).unwrap()));

    let mandatory_spanned_lexed = one_word("mandatory-spanned");
    let mut mandatory_spanned_input = mandatory_spanned_lexed.input(&GRAMMAR);
    let mandatory_spanned = MandatoryToken::parse_spanned(&mut mandatory_spanned_input).unwrap();
    assert_eq!(mandatory_spanned.source(), Some("mandatory-spanned"));
    assert_eq!(
        mandatory_spanned.root().span(),
        Some(Span::new(0, 17).unwrap())
    );
}

#[test]
fn recursive_generated_parsing_moves_the_child_value_and_occurrence_together() {
    let lexed = one_word("root");
    let mut input = lexed.input(&GRAMMAR);

    let parsed = RecursiveRoot::parse(&mut input).unwrap();
    assert_eq!(parsed.source(), Some("root"));
    assert_eq!(parsed.root().value().token.text(), "root");
    assert_eq!(parsed.root().span(), None);
    assert_eq!(parsed.root().token().span(), Some(Span::new(0, 4).unwrap()));
}

#[test]
fn parsed_values_borrow_matches_and_synthetic_values_own_text() {
    let source = String::from("borrowed");
    let lexed = one_word(&source);
    let mut input = lexed.input(&GRAMMAR);
    let parsed = OptionalToken::parse_spanned(&mut input).unwrap();
    let mut relocated = Vec::new();
    relocated.push(parsed);
    let parsed = relocated.pop().unwrap();
    let root = parsed.root();
    assert_eq!(root.value().text(), "borrowed");
    assert!(std::ptr::eq(root.value().text().as_ptr(), source.as_ptr()));
    assert_eq!(root.span(), Some(Span::new(0, 8).unwrap()));

    let synthetic = Parsed::synthetic(OptionalToken::new(String::from("owned")));
    assert_eq!(synthetic.source(), None);
    assert_eq!(synthetic.root().value().text(), "owned");
    assert_eq!(synthetic.root().span(), None);
}

#[test]
fn into_ast_discards_provenance_before_the_value_can_be_mutated() {
    let lexed = one_word("plain");
    let mut input = lexed.input(&GRAMMAR);
    let parsed = MandatoryToken::parse(&mut input).unwrap();

    let ast = parsed.into_ast();
    assert_eq!(ast.text(), "plain");
    let owned = MandatoryToken::new("plain");
    assert_eq!(ast, owned);
    assert_eq!(ast.clone(), owned);
    assert_eq!(ast.cmp(&owned), std::cmp::Ordering::Equal);

    let mut parsed_hash = DefaultHasher::new();
    ast.hash(&mut parsed_hash);
    let mut synthetic_hash = DefaultHasher::new();
    owned.hash(&mut synthetic_hash);
    assert_eq!(parsed_hash.finish(), synthetic_hash.finish());
}

#[test]
fn a_failed_generated_parse_publishes_no_partial_occurrence() {
    let lexed = LexResult::new(
        "a b",
        vec![
            TokenRecord {
                kind: WORD,
                span: Span::new(0, 1).unwrap(),
            },
            TokenRecord {
                kind: WORD,
                span: Span::new(2, 3).unwrap(),
            },
        ],
    );
    let mut input = lexed.input(&GRAMMAR);

    assert!(FailsAfterOccurrence::parse(&mut input).is_err());
    let parsed = OptionalToken::parse_spanned(&mut input).unwrap();
    assert_eq!(parsed.root().value().text(), "b");
    assert_eq!(parsed.root().span(), Some(Span::new(2, 3).unwrap()));
}

#[test]
fn a_wrong_token_is_a_structured_failure_without_a_parsed_value() {
    let lexed = LexResult::new(
        "?",
        vec![TokenRecord {
            kind: OTHER,
            span: Span::new(0, 1).unwrap(),
        }],
    );
    let mut input = lexed.input(&GRAMMAR);

    let error = OptionalToken::parse_spanned(&mut input).unwrap_err();
    assert_eq!(error.expected(), &[WORD_ATOM]);
    assert_eq!(input.cursor(), 0);
}

#[test]
fn generated_root_parsing_rejects_an_input_from_another_grammar() {
    let lexed = one_word("word");
    let mut input = lexed.input(&WRONG_GRAMMAR);

    let error = OptionalToken::parse(&mut input).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::GrammarMismatch);
    assert_eq!(input.cursor(), 0);
}

#[test]
fn an_invalid_matched_span_is_a_structured_error_instead_of_a_slice_panic() {
    for invalid in [
        Span::new(0, 1).unwrap(),
        Span::new(0, 3).unwrap(),
        Span::SYNTHETIC,
    ] {
        let lexed = LexResult::new(
            "é",
            vec![TokenRecord {
                kind: WORD,
                span: invalid,
            }],
        );
        let mut input = lexed.input(&GRAMMAR);

        let error = OptionalToken::parse_spanned(&mut input).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::InvalidTokenSpan);
        assert_eq!(error.span(), invalid);
        assert_eq!(error.found(), Some(WORD));
    }
}
