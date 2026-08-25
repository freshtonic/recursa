use recursa::{
    ExpectedAtomId, FollowSet, GrammarInfo, HasSpan, Input, LexResult, Parse, ParseError,
    ParseErrorKind, Span, SpanBlock, TokenRecord, Vec1,
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
struct GeneratedNumberList;

#[derive(Debug, Eq, PartialEq)]
struct NumberNode<'i> {
    text: &'i str,
    spans: Option<SpanBlock<0>>,
}

impl HasSpan for NumberNode<'_> {
    fn span(&self) -> Option<Span> {
        self.spans.as_ref().map(SpanBlock::node)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct NumberListNode<'i> {
    numbers: Vec1<NumberNode<'i>>,
    spans: Option<SpanBlock<1>>,
}

impl HasSpan for NumberListNode<'_> {
    fn span(&self) -> Option<Span> {
        self.spans.as_ref().map(SpanBlock::node)
    }
}

#[derive(Default)]
struct GeneratedBuilder {
    number_nodes: usize,
    list_nodes: usize,
}

impl GeneratedBuilder {
    fn number<'i>(&mut self, text: &'i str, spans: Option<SpanBlock<0>>) -> NumberNode<'i> {
        self.number_nodes += 1;
        NumberNode { text, spans }
    }

    fn number_list<'i>(
        &mut self,
        numbers: Vec1<NumberNode<'i>>,
        spans: Option<SpanBlock<1>>,
    ) -> NumberListNode<'i> {
        self.list_nodes += 1;
        NumberListNode { numbers, spans }
    }
}

impl<'i> Parse<'i, GeneratedBuilder> for GeneratedNumber {
    type Out = NumberNode<'i>;

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
        let text = &input.source()[record.span.range()];
        let spans = recursa::__private::captures_spans(input).then(|| {
            let mut block = SpanBlock::synthetic();
            recursa::__private::set_node_span(&mut block, record.span);
            block
        });
        Ok(input.builder().number(text, spans))
    }
}

impl<'i> Parse<'i, GeneratedBuilder> for GeneratedNumberList {
    type Out = NumberListNode<'i>;

    fn parse(
        input: &mut Input<'i, GeneratedBuilder>,
        follow: &FollowSet,
    ) -> Result<Self::Out, ParseError> {
        let start = input.byte_position();
        let first = GeneratedNumber::parse(input, follow)?;
        let mut numbers = Vec1::new(first);
        while input.peek(0).is_some_and(|record| record.kind == NUMBER) {
            numbers.push(GeneratedNumber::parse(input, follow)?);
        }

        let spans = recursa::__private::captures_spans(input).then(|| {
            let extent = Span::new(start, recursa::__private::consumed_end(input)).unwrap();
            let mut block = SpanBlock::synthetic();
            recursa::__private::set_node_span(&mut block, extent);
            assert!(recursa::__private::set_element_span(&mut block, 0, extent));
            block
        });
        Ok(input.builder().number_list(numbers, spans))
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

    assert_eq!(parsed.text, "123");
    assert_eq!(parsed.span(), None);
    assert_eq!(input.cursor(), 1);
    assert_eq!(input.byte_position(), 3);
    assert!(input.is_eof());
    assert_eq!(input.builder().number_nodes, 1);
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

#[test]
fn generated_consumer_builds_a_nonempty_sequence_with_optional_spans() {
    let records = vec![
        TokenRecord {
            kind: NUMBER,
            span: Span::new(0, 1).unwrap(),
        },
        TokenRecord {
            kind: NUMBER,
            span: Span::new(2, 4).unwrap(),
        },
    ];

    let normal_lexed = LexResult::new("1 22", records.clone());
    let mut normal_input = normal_lexed.input_with_builder(&GRAMMAR, GeneratedBuilder::default());
    let normal = GeneratedNumberList::parse(&mut normal_input, &CALLER_FOLLOW).unwrap();
    assert_eq!(normal.numbers.len(), 2);
    assert_eq!(normal.numbers.first().text, "1");
    assert_eq!(normal.numbers[1].text, "22");
    assert_eq!(normal.span(), None);
    assert!(normal.numbers.iter().all(|number| number.span().is_none()));
    assert_eq!(normal_input.builder().number_nodes, 2);
    assert_eq!(normal_input.builder().list_nodes, 1);

    let spanned_lexed = LexResult::new("1 22", records);
    let mut spanned_input = spanned_lexed.input_with_builder(&GRAMMAR, GeneratedBuilder::default());
    let spanned = GeneratedNumberList::parse_spanned(&mut spanned_input, &CALLER_FOLLOW).unwrap();
    assert_eq!(spanned.span(), Some(Span::new(0, 4).unwrap()));
    assert_eq!(spanned.numbers[0].span(), Some(Span::new(0, 1).unwrap()));
    assert_eq!(spanned.numbers[1].span(), Some(Span::new(2, 4).unwrap()));
    assert!(!recursa::__private::captures_spans(&spanned_input));

    let empty = LexResult::new("", vec![]);
    let mut failing_input = empty.input_with_builder(&GRAMMAR, GeneratedBuilder::default());
    assert!(GeneratedNumberList::parse_spanned(&mut failing_input, &CALLER_FOLLOW).is_err());
    assert!(!recursa::__private::captures_spans(&failing_input));
}
