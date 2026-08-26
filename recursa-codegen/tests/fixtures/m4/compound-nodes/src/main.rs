use recursa::{HasSpan as _, NodeView as _, Pretty as _};
use recursa_generated_compound_nodes::grammar;
use std::hash::{Hash as _, Hasher as _};

macro_rules! parse_spanned {
    ($ty:ty, $source:expr) => {{
        let lexed = grammar::lex($source);
        let mut input = lexed.input();
        let parsed = <$ty>::parse_spanned(&mut input).expect("compound Node parses");
        assert!(input.is_eof());
        parsed
    }};
}

#[derive(recursa::TotalVisitor)]
#[total_visitor(dispatch = [grammar::Record<'static>], error = ())]
struct TypedRecordVisitor(usize);

impl<'input> recursa::Visitor<grammar::Record<'input>> for TypedRecordVisitor {
    type Error = ();

    fn enter(
        &mut self,
        _node: &grammar::Record<'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>> {
        self.0 += 1;
        std::ops::ControlFlow::Continue(())
    }
}

#[derive(recursa::TotalVisitorMut)]
#[total_visitor_mut(dispatch = [grammar::Count<'static>], error = ())]
struct TypedCountMutator;

impl<'input> recursa::VisitorMut<grammar::Count<'input>> for TypedCountMutator {
    type Error = ();

    fn enter_mut(
        &mut self,
        node: &mut grammar::Count<'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>> {
        node.text = grammar::CountText::new("99");
        std::ops::ControlFlow::Continue(())
    }
}

#[derive(recursa::TotalVisitor)]
#[total_visitor(
    dispatch = [
        grammar::public_left::Duplicate,
        grammar::public_right::Duplicate,
    ],
    error = (),
    event = grammar::__RecursaVisitEvent,
)]
struct DuplicateVisitor(Vec<&'static str>);

impl recursa::Visitor<grammar::public_left::Duplicate> for DuplicateVisitor {
    type Error = ();

    fn enter(
        &mut self,
        _node: &grammar::public_left::Duplicate,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>> {
        self.0.push("left");
        std::ops::ControlFlow::Continue(())
    }
}

impl recursa::Visitor<grammar::public_right::Duplicate> for DuplicateVisitor {
    type Error = ();

    fn enter(
        &mut self,
        _node: &grammar::public_right::Duplicate,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>> {
        self.0.push("right");
        std::ops::ControlFlow::Continue(())
    }
}

struct OrderedMutator(Vec<&'static str>);

impl<'input> recursa::TotalVisitorMut<grammar::__RecursaVisitEventsMut<'input>> for OrderedMutator {
    type Error = ();

    fn total_enter_mut<'visit>(
        &mut self,
        event: grammar::__RecursaVisitEventMut<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEventsMut<'input>: 'visit,
    {
        let name = match event {
            grammar::__RecursaVisitEventMut::__RecursaTarget_6_Record(_) => "enter-record",
            grammar::__RecursaVisitEventMut::__RecursaTarget_5_Label(_) => "enter-label",
            grammar::__RecursaVisitEventMut::__RecursaTarget_9_LabelText(_) => "enter-label-text",
            grammar::__RecursaVisitEventMut::__RecursaTarget_5_Count(_) => "enter-count",
            grammar::__RecursaVisitEventMut::__RecursaTarget_9_CountText(text) => {
                *text = grammar::CountText::new("73");
                "enter-count-text"
            }
            _ => return std::ops::ControlFlow::Continue(()),
        };
        self.0.push(name);
        std::ops::ControlFlow::Continue(())
    }

    fn total_exit_mut<'visit>(
        &mut self,
        event: grammar::__RecursaVisitEventMut<'visit, 'input>,
    ) -> std::ops::ControlFlow<recursa::VisitBreak<Self::Error>>
    where
        grammar::__RecursaVisitEventsMut<'input>: 'visit,
    {
        let name = match event {
            grammar::__RecursaVisitEventMut::__RecursaTarget_6_Record(_) => "exit-record",
            grammar::__RecursaVisitEventMut::__RecursaTarget_5_Label(_) => "exit-label",
            grammar::__RecursaVisitEventMut::__RecursaTarget_9_LabelText(_) => "exit-label-text",
            grammar::__RecursaVisitEventMut::__RecursaTarget_5_Count(_) => "exit-count",
            grammar::__RecursaVisitEventMut::__RecursaTarget_9_CountText(_) => "exit-count-text",
            _ => return std::ops::ControlFlow::Continue(()),
        };
        self.0.push(name);
        std::ops::ControlFlow::Continue(())
    }
}

fn visit_borrowed_record(source: &str) -> usize {
    let parsed = parse_spanned!(grammar::Record<'_>, source);
    let mut visitor = TypedRecordVisitor(0);
    assert!(recursa::Visit::visit(&parsed, &mut visitor).is_continue());
    visitor.0
}

fn main() {
    fn assert_arbitrary<T: for<'arbitrary> recursa::Arbitrary<'arbitrary>>() {}
    assert_arbitrary::<grammar::Record<'static>>();

    let record = parse_spanned!(grammar::Record<'_>, "ReCoRd:alpha 42");
    let _: grammar::RecordView<'_, '_> = record.root();
    assert_eq!(
        record.root().span(),
        Some(recursa::Span::new(0, 15).unwrap())
    );
    assert_eq!(record.root().render(), "ReCoRd : alpha 42");
    assert_eq!(record.render(), "ReCoRd : alpha 42");
    assert_eq!(
        record.root().label().span(),
        Some(recursa::Span::new(7, 12).unwrap())
    );
    assert_eq!(
        record
            .root()
            .count()
            .expect("present optional field")
            .span(),
        Some(recursa::Span::new(13, 15).unwrap())
    );
    let elements = record.root().span_elements();
    assert_eq!(elements.len(), 4);
    assert_eq!(elements.get(0).unwrap().index(), 0);
    assert_eq!(elements.get(0).unwrap().name(), Some("record"));
    assert_eq!(
        elements.get(0).unwrap().role(),
        recursa::ElementRole::FixedToken
    );
    assert_eq!(elements.get(0).unwrap().span(), record.root().record_span());
    assert_eq!(elements.get(1).unwrap().index(), 1);
    assert_eq!(elements.get(1).unwrap().name(), Some("label_colon"));
    assert_eq!(
        elements.get(1).unwrap().role(),
        recursa::ElementRole::FixedToken
    );
    assert_eq!(
        elements.get(1).unwrap().span(),
        record.root().label_colon_span()
    );
    assert_eq!(elements.get(2).unwrap().index(), 2);
    assert_eq!(elements.get(2).unwrap().name(), Some("label"));
    assert_eq!(
        elements.get(2).unwrap().role(),
        recursa::ElementRole::SemanticField
    );
    assert_eq!(
        elements.get(2).unwrap().span(),
        record.root().label().span()
    );
    assert_eq!(elements.get(3).unwrap().index(), 3);
    assert_eq!(elements.get(3).unwrap().name(), Some("count"));
    assert_eq!(
        elements.get(3).unwrap().role(),
        recursa::ElementRole::SemanticField
    );
    assert_eq!(
        elements.get(3).unwrap().span(),
        record.root().count().and_then(|view| view.span())
    );
    struct TraversalLog(usize);
    impl<'input> recursa::TotalVisitor<grammar::__RecursaVisitEvents<'input>> for TraversalLog {
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
    let mut traversal = TraversalLog(0);
    assert!(recursa::Visit::visit(record.root().value(), &mut traversal).is_continue());
    assert_eq!(traversal.0, 5);
    let mut typed = TypedRecordVisitor(0);
    assert!(recursa::Visit::visit(&record, &mut typed).is_continue());
    assert_eq!(typed.0, 1);

    let mut duplicates = DuplicateVisitor(Vec::new());
    assert!(recursa::Visit::visit(&grammar::public_left::Duplicate, &mut duplicates).is_continue());
    assert!(recursa::Visit::visit(&grammar::public_right::Duplicate, &mut duplicates).is_continue());
    assert_eq!(duplicates.0, ["left", "right"]);

    let first_source = String::from("record:first 1");
    let second_source = String::from("record:second 2");
    assert_eq!(visit_borrowed_record(&first_source), 1);
    assert_eq!(visit_borrowed_record(&second_source), 1);

    let spaced = parse_spanned!(grammar::Record<'_>, "ReCoRd  : alpha   42");
    assert_eq!(spaced.render(), "ReCoRd : alpha 42");

    let mut arbitrary_input = recursa::__private::arbitrary::Unstructured::new(&[0; 64]);
    let arbitrary_record =
        <grammar::Record<'static> as recursa::Arbitrary>::arbitrary(&mut arbitrary_input)
            .expect("configured Arbitrary creates a Record");
    let arbitrary_rendered = recursa::Parsed::synthetic(arbitrary_record).render();
    let _roundtrip = parse_spanned!(grammar::Record<'_>, arbitrary_rendered.as_str());

    let forwarded_a = recursa::Parsed::synthetic(record.root().value().clone());
    let forwarded_b = recursa::Parsed::synthetic(record.root().value().clone());
    assert_eq!(forwarded_a, forwarded_b);
    assert_eq!(forwarded_a.cmp(&forwarded_b), std::cmp::Ordering::Equal);
    assert!(format!("{forwarded_a:?}").starts_with("Record"));
    let mut forwarded_hash = std::collections::hash_map::DefaultHasher::new();
    forwarded_a.hash(&mut forwarded_hash);
    assert_ne!(forwarded_hash.finish(), 0);
    assert_eq!(forwarded_a.render(), "RECORD : alpha 42");

    struct MutationCounter(usize);
    impl<'input> recursa::TotalVisitorMut<grammar::__RecursaVisitEventsMut<'input>>
        for MutationCounter
    {
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
    let mut detached = record.into_ast();
    let cloned = detached.clone();
    assert_eq!(detached, cloned);
    assert_eq!(detached.cmp(&cloned), std::cmp::Ordering::Equal);
    assert!(format!("{detached:?}").starts_with("Record"));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    detached.hash(&mut hasher);
    assert_ne!(hasher.finish(), 0);
    assert_eq!(recursa::Parsed::synthetic(cloned.clone()).render(), "RECORD : alpha 42");
    let mut order = OrderedMutator(Vec::new());
    assert!(recursa::VisitMut::visit_mut(&mut detached, &mut order).is_continue());
    assert_eq!(
        order.0,
        [
            "enter-record",
            "enter-label",
            "enter-label-text",
            "exit-label-text",
            "exit-label",
            "enter-count",
            "enter-count-text",
            "exit-count-text",
            "exit-count",
            "exit-record",
        ]
    );
    assert_eq!(detached.count.as_ref().as_ref().unwrap().text.text(), "73");
    let mut mutations = MutationCounter(0);
    assert!(recursa::VisitMut::visit_mut(&mut detached, &mut mutations).is_continue());
    assert_eq!(mutations.0, 5);
    assert!(
        recursa::VisitMut::visit_mut(&mut detached, &mut TypedCountMutator).is_continue()
    );
    let grammar::Record { label, count } = detached;
    assert_eq!(label.text.text(), "alpha");
    assert_eq!(count.as_ref().as_ref().expect("present optional Box").text.text(), "99");

    let absent = parse_spanned!(grammar::Record<'_>, "rEcOrD:beta");
    assert!(absent.root().count().is_none());
    assert_eq!(absent.root().span_elements().len(), 4);
    assert_eq!(absent.root().span_elements().get(3).unwrap().span(), None);
    let grammar::Record { label, count } = absent.into_ast();
    assert_eq!(label.text.text(), "beta");
    assert!(count.as_ref().is_none());

    let pair = parse_spanned!(grammar::Pair<'_>, "PaIr gamma 7");
    assert_eq!(pair.root().span(), Some(recursa::Span::new(0, 12).unwrap()));
    assert_eq!(
        pair.root().field_0().span(),
        Some(recursa::Span::new(5, 10).unwrap())
    );
    assert_eq!(
        pair.root()
            .field_1()
            .expect("present optional tuple field")
            .span(),
        Some(recursa::Span::new(11, 12).unwrap())
    );
    let pair_elements = pair.root().span_elements();
    assert_eq!(pair_elements.len(), 3);
    assert_eq!(pair_elements.get(0).unwrap().name(), Some("pair"));
    assert_eq!(
        pair_elements.get(0).unwrap().role(),
        recursa::ElementRole::FixedToken
    );
    assert_eq!(
        pair_elements.get(0).unwrap().span(),
        pair.root().pair_span()
    );
    assert_eq!(pair_elements.get(1).unwrap().name(), Some("field_0"));
    assert_eq!(
        pair_elements.get(1).unwrap().span(),
        pair.root().field_0().span()
    );
    assert_eq!(pair_elements.get(2).unwrap().name(), Some("field_1"));
    assert_eq!(
        pair_elements.get(2).unwrap().span(),
        pair.root().field_1().and_then(|view| view.span())
    );
    let grammar::Pair(label, count) = pair.into_ast();
    assert_eq!(label.text.text(), "gamma");
    assert_eq!(count.expect("second tuple field").text.text(), "7");

    let unit = parse_spanned!(grammar::Entry<'_>, "EmPtY");
    assert_eq!(unit.root().span(), Some(recursa::Span::new(0, 5).unwrap()));
    assert!(unit.root().record_value().is_none());
    assert!(unit.root().pair_value().is_none());
    assert_eq!(unit.root().empty_span(), unit.root().span());
    let unit_elements = unit.root().span_elements();
    assert_eq!(unit_elements.len(), 1);
    assert_eq!(unit_elements.get(0).unwrap().name(), Some("empty"));
    assert_eq!(
        unit_elements.get(0).unwrap().role(),
        recursa::ElementRole::FixedToken
    );
    assert!(matches!(unit.into_ast(), grammar::Entry::Empty));

    let variant = parse_spanned!(grammar::Entry<'_>, "RECORD:delta");
    assert_eq!(
        variant.root().span(),
        Some(recursa::Span::new(0, 12).unwrap())
    );
    assert_eq!(
        variant
            .root()
            .record_value()
            .expect("active named enum accessor")
            .span(),
        variant.root().span()
    );
    assert!(variant.root().pair_value().is_none());
    assert_eq!(variant.root().empty_span(), None);
    let variant_elements = variant.root().span_elements();
    assert_eq!(variant_elements.len(), 1);
    assert_eq!(
        variant_elements.get(0).unwrap().name(),
        Some("record_value")
    );
    assert_eq!(
        variant_elements.get(0).unwrap().role(),
        recursa::ElementRole::SemanticField
    );
    assert_eq!(
        variant_elements.get(0).unwrap().span(),
        variant.root().record_value().and_then(|view| view.span())
    );
    match variant.into_ast() {
        grammar::Entry::Record { value: record } => {
            assert_eq!(record.label.text.text(), "delta");
            assert!(record.count.is_none());
        }
        _ => panic!("authored enum variant shape changed"),
    }

    let pair_variant = parse_spanned!(grammar::Entry<'_>, "pair omega");
    assert!(pair_variant.root().record_value().is_none());
    assert_eq!(pair_variant.root().empty_span(), None);
    assert_eq!(
        pair_variant
            .root()
            .pair_value()
            .expect("active pair variant accessor")
            .span(),
        pair_variant.root().span()
    );
    assert_eq!(pair_variant.root().span_elements().len(), 1);
    assert_eq!(
        pair_variant.root().span_elements().get(0).unwrap().name(),
        Some("pair_value")
    );
    match pair_variant.into_ast() {
        grammar::Entry::Pair { value: pair } => {
            assert_eq!(pair.0.text.text(), "omega");
            assert!(pair.1.is_none());
        }
        _ => panic!("authored pair variant shape changed"),
    }

    grammar::assert_private_sibling_spans();

    println!("compound-nodes-ok");
}
