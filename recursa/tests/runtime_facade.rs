use recursa::{
    ExpectedAtomId, FollowSet, GrammarInfo, Input, LexResult, Parse, ParseError, ParseErrorKind,
    Span, TokenRecord,
};

const NUMBER: u16 = 0;
const PLUS: u16 = 1;
const LEXICAL_ERROR: u16 = 2;

const NUMBER_ATOM: ExpectedAtomId = ExpectedAtomId(0);
const EXPRESSION_ATOM: ExpectedAtomId = ExpectedAtomId(1);

static GRAMMAR: GrammarInfo = GrammarInfo {
    name: "numbers",
    kind_names: &["number", "+", "lexical error"],
    atom_names: &["number", "expression"],
    lexical_error_kind: LEXICAL_ERROR,
};

static NUMBER_START: FollowSet = recursa::__private::follow_set(3, &[1 << NUMBER], &[NUMBER_ATOM]);
static CALLER_FOLLOW: FollowSet =
    recursa::__private::follow_set(3, &[1 << PLUS], &[EXPRESSION_ATOM]);

struct GeneratedNumber;

#[derive(Default)]
struct GeneratedBuilder {
    parsed_tokens: usize,
}

impl<'i> Parse<'i, GeneratedBuilder> for GeneratedNumber {
    type Out = &'i str;

    fn parse(
        input: &mut Input<'i, GeneratedBuilder>,
        follow: &FollowSet,
    ) -> Result<Self::Out, ParseError> {
        let _effective_follow = NUMBER_START.union(follow).unwrap();
        let Some(record) = input.peek(0) else {
            return Err(recursa::__private::unexpected(input, &NUMBER_START));
        };
        if record.kind != NUMBER {
            return Err(recursa::__private::unexpected(input, &NUMBER_START));
        }
        input.advance();
        input.builder().parsed_tokens += 1;
        Ok(&input.source()[record.span.range()])
    }
}

#[test]
fn generated_consumer_parses_one_token_through_the_facade() {
    let lexed = LexResult::new(
        "123",
        vec![TokenRecord {
            kind: NUMBER,
            span: Span::new(0, 3).unwrap(),
        }],
    );
    let mut input = lexed.input_with_builder(&GRAMMAR, GeneratedBuilder::default());

    assert_eq!(input.source(), "123");
    assert_eq!(input.grammar().name, "numbers");
    assert_eq!(input.cursor(), 0);
    assert_eq!(input.byte_position(), 0);
    assert!(!input.is_eof());
    assert_eq!(input.peek(0).map(|record| record.kind), Some(NUMBER));
    assert_eq!(input.peek(100), None);

    let checkpoint = recursa::__private::checkpoint(&input);
    input.advance();
    assert!(input.is_eof());
    recursa::__private::rewind(&mut input, checkpoint);
    assert_eq!(input.cursor(), 0);

    let parsed = GeneratedNumber::parse(&mut input, &CALLER_FOLLOW).unwrap();

    assert_eq!(parsed, "123");
    assert_eq!(input.cursor(), 1);
    assert_eq!(input.byte_position(), 3);
    assert!(input.is_eof());
    assert_eq!(input.builder().parsed_tokens, 1);
}

#[test]
fn generated_consumer_composes_borrowed_follow_sets() {
    let composed = NUMBER_START.union(&CALLER_FOLLOW).unwrap();

    assert!(composed.contains(NUMBER));
    assert!(composed.contains(PLUS));
    assert!(!composed.contains(LEXICAL_ERROR));
    assert_eq!(composed.expected(), &[NUMBER_ATOM, EXPRESSION_ATOM]);
    assert_eq!(GRAMMAR.kind_name(PLUS), Some("+"));
    assert_eq!(GRAMMAR.atom_name(EXPRESSION_ATOM), Some("expression"));
}

#[test]
fn generated_consumer_reports_structured_symbolic_failures() {
    let wrong = LexResult::new(
        "+",
        vec![TokenRecord {
            kind: PLUS,
            span: Span::new(0, 1).unwrap(),
        }],
    );
    let mut wrong_input = wrong.input_with_builder(&GRAMMAR, GeneratedBuilder::default());
    let wrong_error = GeneratedNumber::parse(&mut wrong_input, &CALLER_FOLLOW).unwrap_err();
    assert_eq!(wrong_error.kind(), ParseErrorKind::UnexpectedToken);
    assert_eq!(wrong_error.span(), Span::new(0, 1).unwrap());
    assert_eq!(wrong_error.found(), Some(PLUS));
    assert_eq!(wrong_error.expected(), &[NUMBER_ATOM]);
    assert_eq!(wrong_error.grammar(), &GRAMMAR);

    let empty = LexResult::new("", vec![]);
    let mut empty_input = empty.input_with_builder(&GRAMMAR, GeneratedBuilder::default());
    let eof_error = GeneratedNumber::parse(&mut empty_input, &CALLER_FOLLOW).unwrap_err();
    assert_eq!(eof_error.kind(), ParseErrorKind::UnexpectedEof);
    assert_eq!(eof_error.span(), Span::new(0, 0).unwrap());
    assert_eq!(eof_error.found(), None);
    assert_eq!(eof_error.expected(), &[NUMBER_ATOM]);

    let invalid = LexResult::new(
        "?",
        vec![TokenRecord {
            kind: LEXICAL_ERROR,
            span: Span::new(0, 1).unwrap(),
        }],
    );
    let mut invalid_input = invalid.input_with_builder(&GRAMMAR, GeneratedBuilder::default());
    let lexical_error = GeneratedNumber::parse(&mut invalid_input, &CALLER_FOLLOW).unwrap_err();
    assert_eq!(lexical_error.kind(), ParseErrorKind::LexicalError);
    assert_eq!(lexical_error.span(), Span::new(0, 1).unwrap());
    assert_eq!(lexical_error.found(), Some(LEXICAL_ERROR));
    assert_eq!(lexical_error.expected(), &[NUMBER_ATOM]);
}
