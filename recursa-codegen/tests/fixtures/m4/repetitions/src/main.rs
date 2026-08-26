use recursa::{HasSpan as _, NodeView as _, Pretty as _};
use recursa_generated_repetitions::grammar;

fn assert_mapping(
    source_map: &recursa::SourceMap,
    output: std::ops::Range<usize>,
    source: recursa::Span,
) {
    assert!(
        source_map
            .entries()
            .iter()
            .any(|entry| entry.output() == output && entry.source() == source),
        "missing exact Pretty mapping {source:?}",
    );
}

macro_rules! parse_spanned {
    ($ty:ty, $source:expr) => {{
        let lexed = grammar::lex($source);
        let mut input = lexed.input();
        let parsed = <$ty>::parse_spanned(&mut input).expect("repetition Node parses");
        assert!(input.is_eof());
        parsed
    }};
}

macro_rules! parse_default {
    ($ty:ty, $source:expr) => {{
        let lexed = grammar::lex($source);
        let mut input = lexed.input();
        let parsed = <$ty>::parse(&mut input).expect("capture-policy Node parses");
        assert!(input.is_eof());
        parsed
    }};
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CaptureMode {
    Off,
    Optional,
    Mandatory,
}

impl CaptureMode {
    fn from_environment() -> Self {
        match std::env::var("RECURSA_CAPTURE_MODE").as_deref() {
            Ok("off") => Self::Off,
            Ok("optional") => Self::Optional,
            Ok("mandatory") => Self::Mandatory,
            Ok(other) => panic!("unexpected capture mode {other}"),
            Err(error) => panic!("missing RECURSA_CAPTURE_MODE: {error}"),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Optional => "optional",
            Self::Mandatory => "mandatory",
        }
    }
}

fn move_many<'input>(
    parsed: recursa::Parsed<'input, grammar::Many<'input>>,
) -> grammar::Many<'input> {
    parsed.into_ast()
}

fn assert_capture_configuration(mode: CaptureMode) {
    let inherited = parse_default!(grammar::InheritedCapture<'_>, "capture word");
    let default_captures = mode == CaptureMode::Mandatory;
    assert_eq!(inherited.source().is_some(), default_captures);
    assert_eq!(
        inherited.root().span(),
        default_captures.then(|| recursa::Span::new(0, 12).unwrap())
    );
    assert_eq!(
        inherited.root().value().span(),
        default_captures.then(|| recursa::Span::new(8, 12).unwrap())
    );

    let inherited_spanned = parse_spanned!(grammar::InheritedCapture<'_>, "capture word");
    let spanned_captures = mode != CaptureMode::Off;
    assert_eq!(inherited_spanned.source().is_some(), spanned_captures);
    assert_eq!(
        inherited_spanned.root().span(),
        spanned_captures.then(|| recursa::Span::new(0, 12).unwrap())
    );
    assert_eq!(
        inherited_spanned.root().value().span(),
        spanned_captures.then(|| recursa::Span::new(8, 12).unwrap())
    );

    let node_off = parse_spanned!(grammar::NodeOff<'_>, "nodeoff word");
    assert_eq!(node_off.source(), None);
    assert_eq!(node_off.root().span(), None);
    assert_eq!(node_off.root().value().span(), None);

    let node_optional = parse_default!(grammar::NodeOptional<'_>, "nodeoptional word");
    assert_eq!(node_optional.source(), None);
    assert_eq!(node_optional.root().span(), None);
    assert_eq!(node_optional.root().value().span(), None);
    let node_optional = parse_spanned!(grammar::NodeOptional<'_>, "nodeoptional word");
    assert!(node_optional.source().is_some());
    assert_eq!(node_optional.root().span(), recursa::Span::new(0, 17).ok());
    assert_eq!(
        node_optional.root().value().span(),
        recursa::Span::new(13, 17).ok()
    );

    let node_mandatory = parse_default!(grammar::NodeMandatory<'_>, "nodemandatory word");
    assert!(node_mandatory.source().is_some());
    assert_eq!(node_mandatory.root().span(), recursa::Span::new(0, 18).ok());
    assert_eq!(
        node_mandatory.root().value().span(),
        recursa::Span::new(14, 18).ok()
    );

    let field_mandatory = parse_default!(grammar::FieldMandatory<'_>, "fieldmandatory word");
    assert!(field_mandatory.source().is_some());
    assert_eq!(field_mandatory.root().span(), None);
    assert_eq!(
        field_mandatory.root().value().span(),
        recursa::Span::new(15, 19).ok()
    );
    assert_eq!(
        field_mandatory
            .root()
            .span_elements()
            .get(1)
            .unwrap()
            .span(),
        recursa::Span::new(15, 19).ok()
    );

    let field_off = parse_spanned!(grammar::FieldOff<'_>, "fieldoff word");
    assert!(field_off.source().is_some());
    assert_eq!(field_off.root().span(), recursa::Span::new(0, 13).ok());
    assert_eq!(field_off.root().value().span(), None);
    assert_eq!(
        field_off.root().span_elements().get(1).unwrap().span(),
        None
    );

    let field_optional = parse_default!(grammar::FieldOptional<'_>, "fieldoptional word");
    assert!(field_optional.source().is_some());
    assert_eq!(field_optional.root().span(), recursa::Span::new(0, 18).ok());
    assert_eq!(field_optional.root().value().span(), None);
    let field_optional = parse_spanned!(grammar::FieldOptional<'_>, "fieldoptional word");
    assert_eq!(
        field_optional.root().value().span(),
        recursa::Span::new(14, 18).ok()
    );

    let variant_off = parse_spanned!(grammar::VariantCapture<'_>, "variantoff word");
    assert_eq!(variant_off.source(), None);
    assert_eq!(variant_off.root().span(), None);

    let variant_mandatory =
        parse_default!(grammar::VariantCapture<'_>, "variantmandatory word");
    assert!(variant_mandatory.source().is_some());
    assert_eq!(
        variant_mandatory.root().span(),
        recursa::Span::new(0, 21).ok()
    );

    let parent_off = parse_spanned!(
        grammar::ParentOff<'_>,
        "parentoff nodemandatory word"
    );
    assert!(parent_off.source().is_some());
    assert_eq!(parent_off.root().span(), None);
    assert_eq!(
        parent_off.root().child().span(),
        recursa::Span::new(10, 28).ok()
    );

    let parent_field_off = parse_spanned!(
        grammar::ParentFieldOff<'_>,
        "parentfieldoff nodemandatory word"
    );
    assert!(parent_field_off.source().is_some());
    assert_eq!(
        parent_field_off.root().span(),
        recursa::Span::new(0, 33).ok()
    );
    assert_eq!(parent_field_off.root().child().span(), None);

    let descendants = parse_spanned!(
        grammar::RepeatedDescendants<'_>,
        "descendants descendant one,descendant two"
    );
    assert!(descendants.source().is_some());
    assert_eq!(descendants.root().span(), None);
    let values = descendants.root().values();
    assert_eq!(values.span(), None);
    assert_eq!(values.get(0).unwrap().span(), None);
    assert_eq!(
        values.get(0).unwrap().value().span(),
        recursa::Span::new(23, 26).ok()
    );
    assert_eq!(
        values.get(1).unwrap().value().span(),
        recursa::Span::new(38, 41).ok()
    );

    let mixed = parse_spanned!(grammar::MixedMany, "mixed off,on,off,on");
    assert_eq!(mixed.render(), "MIXED OFF, on, OFF, on");
    let values = mixed.root().values();
    assert_eq!(values.len(), 4);
    assert_eq!(values.get(0).unwrap().span(), None);

    let program = parse_spanned!(grammar::MixedProgram, "off;on;off");
    let (rendered_program, program_map) = program.render_mapped();
    assert_eq!(rendered_program, "OFF\n; on\n; OFF");
    assert_mapping(
        &program_map,
        4..5,
        recursa::Span::new(3, 4).expect("real span"),
    );
    assert_mapping(
        &program_map,
        9..10,
        recursa::Span::new(6, 7).expect("real span"),
    );

    let words = parse_spanned!(grammar::WordSeparated<'_>, "alpha oR beta");
    assert_eq!(words.source().is_some(), mode != CaptureMode::Off);
    let (rendered_words, words_map) = words.render_mapped();
    assert_eq!(rendered_words, "alpha oR beta");
    assert_mapping(
        &words_map,
        6..8,
        recursa::Span::new(6, 8).expect("real span"),
    );
    let reparsed_words = parse_spanned!(grammar::WordSeparated<'_>, rendered_words.as_str());
    assert_eq!(reparsed_words.render(), rendered_words);

    let choices = parse_spanned!(grammar::OrdinaryChoices, "off on off");
    assert_eq!(choices.render(), "OFF on OFF");

    let duplicates = parse_spanned!(grammar::DuplicateList, "on on on");
    assert_eq!(
        duplicates.render(),
        if mode == CaptureMode::Off {
            "ON ON ON"
        } else {
            "on on on"
        }
    );

    let leading = parse_spanned!(grammar::LeadingPretty<'_>, "marks alpha,beta");
    let marks = if mode == CaptureMode::Off { "MARKS" } else { "marks" };
    assert_eq!(leading.render(), format!("{marks} alpha, beta,"));
    let (mapped_leading, leading_map) = leading.render_mapped();
    assert_eq!(mapped_leading, leading.render());
    assert_mapping(
        &leading_map,
        11..12,
        recursa::Span::new(11, 12).expect("real span"),
    );
    assert_mapping(&leading_map, 17..18, recursa::Span::SYNTHETIC);
    assert_eq!(
        leading.render_with(recursa::PrettyConfig::new(12, 2)),
        format!("{marks} alpha\n, beta,")
    );
    let (broken_leading, broken_leading_map) =
        leading.render_mapped_with(recursa::PrettyConfig::new(12, 2));
    assert_eq!(broken_leading, format!("{marks} alpha\n, beta,"));
    assert_mapping(
        &broken_leading_map,
        12..13,
        recursa::Span::new(11, 12).expect("real span"),
    );
    assert_mapping(&broken_leading_map, 18..19, recursa::Span::SYNTHETIC);
    let reparsed_leading =
        parse_spanned!(grammar::LeadingPretty<'_>, mapped_leading.as_str());
    assert_eq!(reparsed_leading.render(), mapped_leading);
    let rerendered_leading = reparsed_leading.render();
    let reparsed_leading_again =
        parse_spanned!(grammar::LeadingPretty<'_>, rerendered_leading.as_str());
    assert_eq!(reparsed_leading_again.render(), rerendered_leading);

    let captured_trailing =
        parse_spanned!(grammar::LeadingPretty<'_>, "marks alpha,beta,");
    let (captured_trailing_rendered, captured_trailing_map) =
        captured_trailing.render_mapped();
    assert_eq!(captured_trailing_rendered, format!("{marks} alpha, beta,"));
    assert_mapping(
        &captured_trailing_map,
        11..12,
        recursa::Span::new(11, 12).expect("real span"),
    );
    assert_mapping(
        &captured_trailing_map,
        17..18,
        recursa::Span::new(16, 17).expect("real span"),
    );

    let trailing = parse_spanned!(grammar::TrailingPretty<'_>, "marks alpha,beta,");
    assert_eq!(trailing.render(), format!("{marks} alpha, beta"));
    let (mapped_trailing, trailing_map) = trailing.render_mapped();
    assert_eq!(mapped_trailing, trailing.render());
    assert_mapping(
        &trailing_map,
        11..12,
        recursa::Span::new(11, 12).expect("real span"),
    );
    assert!(
        trailing_map
            .entries()
            .iter()
            .all(|entry| entry.source() != recursa::Span::new(16, 17).expect("real span")),
        "trailing_separator = never must not map the suppressed source separator",
    );
    assert_eq!(
        trailing.render_with(recursa::PrettyConfig::new(12, 2)),
        format!("{marks} alpha,\nbeta")
    );
    let (broken_trailing, broken_trailing_map) =
        trailing.render_mapped_with(recursa::PrettyConfig::new(12, 2));
    assert_eq!(broken_trailing, format!("{marks} alpha,\nbeta"));
    assert_mapping(
        &broken_trailing_map,
        11..12,
        recursa::Span::new(11, 12).expect("real span"),
    );

    let synthetic_leading = grammar::LeadingPretty {
        values: vec![
            Box::new(grammar::Item {
                text: grammar::ItemText::new("alpha"),
            }),
            Box::new(grammar::Item {
                text: grammar::ItemText::new("beta"),
            }),
        ],
    };
    let (synthetic_rendered, synthetic_map) = synthetic_leading.render_mapped();
    assert_eq!(synthetic_rendered, "MARKS alpha, beta,");
    assert_mapping(&synthetic_map, 11..12, recursa::Span::SYNTHETIC);
    assert_mapping(&synthetic_map, 17..18, recursa::Span::SYNTHETIC);

    let attachment = parse_spanned!(grammar::AttachmentPretty<'_>, ". <:boxed alpha.?> .");
    let boxed = if mode == CaptureMode::Off { "BOXED" } else { "boxed" };
    assert_eq!(attachment.render(), format!(".\n<:{boxed} alpha.?>\n."));
    let attachment_around = parse_spanned!(grammar::AttachmentAround, ". @ .");
    assert_eq!(attachment_around.render(), ". @ .");
    assert_eq!(
        grammar::HintAttachedOptional {
            before: grammar::Marker,
            value: None,
            after: grammar::Marker,
        }
        .render(),
        ". ."
    );
    let attached_optional =
        parse_spanned!(grammar::HintAttachedOptional<'_>, ". <:boxed alpha.?> .");
    assert_eq!(
        attached_optional.render(),
        format!(".\n<:{boxed} alpha.?>\n.")
    );
    assert_eq!(
        grammar::HintAttachedMany {
            before: grammar::Marker,
            values: Vec::new(),
            after: grammar::Marker,
        }
        .render(),
        ". ."
    );
    let attached_many = parse_spanned!(
        grammar::HintAttachedMany<'_>,
        ". <:many alpha.?> <:many beta.?> ."
    );
    let many_keyword = if mode == CaptureMode::Off { "MANY" } else { "many" };
    assert_eq!(
        attached_many.render(),
        format!(".\n<:{many_keyword} alpha.?>\n<:{many_keyword} beta.?>\n.")
    );
    let (mapped_attached_many, attached_many_map) = attached_many.render_mapped();
    assert_eq!(mapped_attached_many, attached_many.render());
    if !matches!(mode, CaptureMode::Off) {
        assert!(
            attached_many_map
                .entries()
                .iter()
                .any(|entry| !entry.source().is_synthetic())
        );
    }
    assert_eq!(
        grammar::AttachedMaybeMany {
            before: grammar::Marker,
            values: None,
            after: grammar::Marker,
        }
        .render(),
        ". ."
    );
    let attached_maybe_many = parse_spanned!(
        grammar::AttachedMaybeMany<'_>,
        ". <:maybe alpha.?> <:maybe beta.?> ."
    );
    let maybe_keyword = if mode == CaptureMode::Off { "MAYBE" } else { "maybe" };
    assert_eq!(
        attached_maybe_many.render(),
        format!(".\n<:{maybe_keyword} alpha.?>\n<:{maybe_keyword} beta.?>\n.")
    );

    let attached_leading = parse_spanned!(
        grammar::AttachedLeadingPretty<'_>,
        "marks [alpha],[beta]"
    );
    assert_eq!(attached_leading.render(), format!("{marks}[alpha], [beta],"));
    assert_eq!(
        attached_leading.render_with(recursa::PrettyConfig::new(12, 2)),
        format!("{marks}[\n  alpha\n]\n, [\n  beta\n],")
    );

    let attached_trailing = parse_spanned!(
        grammar::AttachedTrailingPretty<'_>,
        "marks [alpha],[beta],"
    );
    assert_eq!(attached_trailing.render(), format!("{marks}[alpha], [beta]"));
    assert_eq!(
        attached_trailing.render_with(recursa::PrettyConfig::new(12, 2)),
        format!("{marks}[\n  alpha\n],\n[\n  beta\n]")
    );

    let attached_program = parse_spanned!(grammar::AttachedProgram, "[off];[on]");
    let (off, on) = ("OFF", "on");
    assert_eq!(
        attached_program.render(),
        format!("[\n  {off}\n];\n[\n  {on}\n]")
    );

    let node_group = parse_spanned!(grammar::NodeGroupPretty<'_>, "nodehint alpha beta");
    assert_eq!(
        node_group.render_with(recursa::PrettyConfig::new(14, 2)),
        format!(
            "{} alpha\nbeta",
            if mode == CaptureMode::Off { "NODEHINT" } else { "nodehint" }
        )
    );
    let variant_group =
        parse_spanned!(grammar::VariantGroupPretty<'_>, "enumhint alpha beta");
    assert_eq!(
        variant_group.render_with(recursa::PrettyConfig::new(14, 2)),
        format!(
            "{} alpha\nbeta",
            if mode == CaptureMode::Off { "ENUMHINT" } else { "enumhint" }
        )
    );
    let consistent = parse_spanned!(
        grammar::ConsistentIndentPretty<'_>,
        "consistent aaaaa bbbbb ccccc"
    );
    let consistent_keyword = if mode == CaptureMode::Off {
        "CONSISTENT"
    } else {
        "consistent"
    };
    assert_eq!(
        consistent.render_with(recursa::PrettyConfig::new(25, 2)),
        format!("{consistent_keyword}\n  aaaaa\n  bbbbb\n  ccccc")
    );
    let inconsistent = parse_spanned!(
        grammar::InconsistentIndentPretty<'_>,
        "inconsistent aaaaa bbbbb ccccc"
    );
    let inconsistent_keyword = if mode == CaptureMode::Off {
        "INCONSISTENT"
    } else {
        "inconsistent"
    };
    assert_eq!(
        inconsistent.render_with(recursa::PrettyConfig::new(25, 2)),
        format!("{inconsistent_keyword} aaaaa bbbbb\n  ccccc")
    );
    let generic_alias = parse_spanned!(
        grammar::GenericAliasOutput<'_>,
        "alpha beta gamma"
    );
    assert_eq!(
        generic_alias.render_with(recursa::PrettyConfig::new(10, 2)),
        "alpha beta\ngamma"
    );
    assert_eq!(
        values.get(1).unwrap().span(),
        recursa::Span::new(10, 12).ok()
    );
    assert_eq!(values.get(2).unwrap().span(), None);
    assert_eq!(
        values.get(3).unwrap().span(),
        recursa::Span::new(17, 19).ok()
    );
}

fn assert_repetition_behavior() {
    let boxed = parse_spanned!(grammar::Boxed<'_>, "boxed alpha");
    assert_eq!(boxed.root().span(), recursa::Span::new(0, 11).ok());
    assert_eq!(boxed.root().value().span(), recursa::Span::new(6, 11).ok());
    assert_eq!(boxed.into_ast().value.text.text(), "alpha");

    let absent = parse_spanned!(grammar::Optional<'_>, "optional");
    assert!(absent.root().value().is_none());
    assert!(absent.into_ast().value.is_none());

    let present = parse_spanned!(grammar::Optional<'_>, "optional beta");
    assert_eq!(
        present.root().value().and_then(|value| value.span()),
        recursa::Span::new(9, 13).ok()
    );
    assert_eq!(present.into_ast().value.unwrap().text.text(), "beta");

    let empty = parse_spanned!(grammar::Many<'_>, "many");
    assert!(empty.root().values().is_empty());
    assert_eq!(empty.root().values().span(), None);
    assert_eq!(empty.root().span_elements().get(1).unwrap().span(), None);
    assert!(empty.into_ast().values.is_empty());

    let many = parse_spanned!(grammar::Many<'_>, "many same,same,");
    assert_eq!(many.render(), "many same, same");
    assert_eq!(
        many.render_with(recursa::PrettyConfig::new(10, 2)),
        "many same,\nsame"
    );
    let values = many.root().values();
    assert_eq!(values.len(), 2);
    assert_eq!(values.span(), recursa::Span::new(5, 15).ok());
    assert_eq!(
        many.root().span_elements().get(1).unwrap().span(),
        recursa::Span::new(5, 15).ok()
    );
    assert_eq!(values.get(0).unwrap().span(), recursa::Span::new(5, 9).ok());
    assert_eq!(
        values.get(1).unwrap().span(),
        recursa::Span::new(10, 14).ok()
    );
    assert_eq!(values.get(0).unwrap().value().text.text(), "same");
    assert_eq!(values.get(1).unwrap().value().text.text(), "same");
    let many = move_many(many);
    assert_eq!(many.values.len(), 2);
    assert_eq!(many.values[0].text.text(), many.values[1].text.text());
    struct Counter(usize);
    impl<'input> recursa::TotalVisitor<grammar::__RecursaVisitEvents<'input>> for Counter {
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
    let mut counter = Counter(0);
    assert!(recursa::Visit::visit(&many, &mut counter).is_continue());
    assert_eq!(counter.0, 5, "Vec traversal includes Node and token leaves");

    let unspanned_lexed = grammar::lex("many left,right");
    let mut unspanned_input = unspanned_lexed.input();
    let unspanned = grammar::Many::parse(&mut unspanned_input).expect("ordinary parse succeeds");
    assert_eq!(unspanned.root().span(), None);
    assert_eq!(unspanned.root().values().span(), None);
    assert_eq!(unspanned.root().values().get(0).unwrap().span(), None);

    let maybe_absent = parse_spanned!(grammar::MaybeMany<'_>, "maybe");
    assert!(maybe_absent.root().values().is_none());
    assert!(maybe_absent.into_ast().values.is_none());

    let maybe_present = parse_spanned!(grammar::MaybeMany<'_>, "maybe one,two,");
    let maybe_values = maybe_present
        .root()
        .values()
        .expect("optional Vec1 is present");
    assert_eq!(maybe_values.len(), 2);
    assert_eq!(maybe_values.span(), recursa::Span::new(6, 14).ok());
    let maybe_ast = maybe_present.into_ast();
    assert_eq!(maybe_ast.values.unwrap().len(), 2);

    assert_eq!(std::mem::size_of::<grammar::Marker>(), 0);
    let markers = parse_spanned!(grammar::NonEmptyMarkers, "marks . , .");
    let marker_values = markers.root().values();
    assert_eq!(marker_values.len(), 2);
    assert_eq!(marker_values.span(), recursa::Span::new(6, 11).ok());
    assert_eq!(
        marker_values.get(0).unwrap().span(),
        recursa::Span::new(6, 7).ok()
    );
    assert_eq!(
        marker_values.get(1).unwrap().span(),
        recursa::Span::new(10, 11).ok()
    );
    let mut counter = Counter(0);
    assert!(
        recursa::Visit::visit(markers.root().value(), &mut counter).is_continue()
    );
    assert_eq!(counter.0, 3, "Vec1 traversal preserves one-or-more children");
    assert_eq!(markers.into_ast().values.len(), 2);

    let choice = parse_spanned!(grammar::Choice<'_>, "choice x,y");
    assert_eq!(choice.root().span_elements().len(), 2);
    assert!(choice.root().empty_span().is_none());
    let choice_items = choice.root().items().expect("Items is the active variant");
    assert_eq!(choice_items.len(), 2);
    assert_eq!(choice_items.span(), recursa::Span::new(7, 10).ok());
    match choice.into_ast() {
        grammar::Choice::Items { items } => assert_eq!(items.len(), 2),
        grammar::Choice::Empty => panic!("active variant changed"),
    }

    let empty_choice = parse_spanned!(grammar::Choice<'_>, "empty");
    assert!(empty_choice.root().items().is_none());
    assert_eq!(empty_choice.root().empty_span(), empty_choice.root().span());

    let failed_lexed = grammar::lex("strict");
    let mut failed_input = failed_lexed.input();
    assert!(grammar::Strict::parse_spanned(&mut failed_input).is_err());
    assert_eq!(
        failed_input.cursor(),
        0,
        "a mandatory Vec1 with zero items rolls back its input cursor"
    );

    let failed_lexed = grammar::lex("strict alpha,");
    let mut failed_input = failed_lexed.input();
    assert!(grammar::Strict::parse_spanned(&mut failed_input).is_err());
    assert_eq!(
        failed_input.cursor(),
        0,
        "a bad trailing separator commits neither semantic values nor provenance"
    );

    let synthetic = recursa::Parsed::synthetic(grammar::Many { values: Vec::new() });
    assert_eq!(synthetic.source(), None);
    assert_eq!(synthetic.root().span(), None);
    assert_eq!(synthetic.root().values().span(), None);

    let attached_absent = parse_spanned!(grammar::AttachedOptional<'_>, "attachedoptional");
    assert!(attached_absent.root().value().is_none());
    assert_eq!(attached_absent.render(), "attachedoptional");

    let attached_present =
        parse_spanned!(grammar::AttachedOptional<'_>, "attachedoptional (alpha)");
    assert_eq!(attached_present.render(), "attachedoptional(alpha)");
    assert_eq!(
        attached_present
            .root()
            .value()
            .and_then(|value| value.span()),
        recursa::Span::new(18, 23).ok(),
    );
    let rendered = attached_present.render();
    let reparsed = parse_spanned!(grammar::AttachedOptional<'_>, rendered.as_str());
    assert_eq!(reparsed.render(), rendered);

    let attached_many_empty = parse_spanned!(grammar::AttachedMany<'_>, "attachedmany");
    assert!(attached_many_empty.root().values().is_empty());
    assert_eq!(attached_many_empty.render(), "attachedmany");

    let attached_many =
        parse_spanned!(grammar::AttachedMany<'_>, "attachedmany [one],[two],");
    assert_eq!(attached_many.root().values().len(), 2);
    assert_eq!(
        attached_many.root().values().get(0).unwrap().span(),
        recursa::Span::new(14, 17).ok(),
    );
    assert_eq!(
        attached_many.root().values().get(1).unwrap().span(),
        recursa::Span::new(20, 23).ok(),
    );
    assert_eq!(attached_many.render(), "attachedmany[one], [two]");
    let rendered = attached_many.render();
    let reparsed = parse_spanned!(grammar::AttachedMany<'_>, rendered.as_str());
    assert_eq!(reparsed.render(), rendered);

    let attached_nonempty_absent =
        parse_spanned!(grammar::AttachedNonEmpty<'_>, "attachednonempty");
    assert!(attached_nonempty_absent.root().values().is_none());
    let attached_nonempty = parse_spanned!(
        grammar::AttachedNonEmpty<'_>,
        "attachednonempty {left},{right},"
    );
    assert_eq!(attached_nonempty.root().values().unwrap().len(), 2);
    assert_eq!(
        attached_nonempty
            .root()
            .values()
            .unwrap()
            .get(0)
            .unwrap()
            .span(),
        recursa::Span::new(18, 22).ok(),
    );
    assert_eq!(
        attached_nonempty
            .root()
            .values()
            .unwrap()
            .get(1)
            .unwrap()
            .span(),
        recursa::Span::new(25, 30).ok(),
    );
    assert_eq!(
        attached_nonempty.render(),
        "attachednonempty{left}, {right}"
    );
    let rendered = attached_nonempty.render();
    let reparsed = parse_spanned!(grammar::AttachedNonEmpty<'_>, rendered.as_str());
    assert_eq!(reparsed.render(), rendered);
    let rerendered = reparsed.render();
    let reparsed_again = parse_spanned!(grammar::AttachedNonEmpty<'_>, rerendered.as_str());
    assert_eq!(reparsed_again.render(), rerendered);
}

fn main() {
    let mode = CaptureMode::from_environment();
    assert_capture_configuration(mode);
    if mode == CaptureMode::Optional {
        assert_repetition_behavior();
    }
    println!("repetitions-ok-{}", mode.name());
}
