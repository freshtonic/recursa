use recursa::{FollowSet, GrammarInfo, Input, LexResult, Parse, ParseError, Span, TokenRecord};

const NUMBER: u16 = 0;
const LEXICAL_ERROR: u16 = 1;

static GRAMMAR: GrammarInfo = GrammarInfo {
    name: "numbers",
    kind_names: &["number", "lexical error"],
    atom_names: &["number"],
    lexical_error_kind: LEXICAL_ERROR,
};

static FOLLOW: FollowSet = recursa::__private::follow_set(2, &[0], &[]);

struct GeneratedNumber;

#[derive(Default)]
struct GeneratedBuilder {
    parsed_tokens: usize,
}

impl<'i> Parse<'i, GeneratedBuilder> for GeneratedNumber {
    type Out = &'i str;

    fn parse(
        input: &mut Input<'i, GeneratedBuilder>,
        _follow: &FollowSet,
    ) -> Result<Self::Out, ParseError> {
        let record = input.peek(0).expect("fixture contains a number");
        assert_eq!(record.kind, NUMBER);
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

    let parsed = GeneratedNumber::parse(&mut input, &FOLLOW).unwrap();

    assert_eq!(parsed, "123");
    assert_eq!(input.cursor(), 1);
    assert_eq!(input.byte_position(), 3);
    assert!(input.is_eof());
    assert_eq!(input.builder().parsed_tokens, 1);
}
