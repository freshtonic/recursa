use recursa::{HasSpan as _, NodeView as _};
use recursa_generated_predictive::grammar;

fn main() {
    let create = grammar::lex("CREATE TABLE widgets");
    let mut input = create.input();
    let parsed = grammar::Statement::parse(&mut input).expect("SQL LL(2) statement parses");
    assert!(input.is_eof(), "the frozen root dispatch consumed through EOF");
    match parsed.root().value() {
        grammar::Statement::CreateTable(node) => assert_eq!(node.rest.text(), "widgets"),
        _ => panic!("CREATE TABLE selected the wrong frozen branch"),
    }
    assert_eq!(parsed.root().span(), Some(recursa::Span::new(0, 20).unwrap()));
    assert_eq!(parsed.root().span_elements().len(), 1);

    let drop_view = grammar::lex("DROP VIEW visible");
    let mut input = drop_view.input();
    let parsed = grammar::Statement::parse(&mut input).expect("DROP VIEW parses");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.root().value(),
        grammar::Statement::DropView(_)
    ));

    let qualified = grammar::lex("DEBUG PRINT message");
    let mut input = qualified.input();
    let parsed = grammar::Stmt::parse(&mut input).expect("nullable qualifier branch parses");
    assert!(input.is_eof());
    match parsed.root().value() {
        grammar::Stmt::Print(node) => {
            assert!(node.debug.is_some());
            assert_eq!(node.rest.text(), "message");
        }
        _ => panic!("DEBUG PRINT selected the wrong frozen branch"),
    }

    let unqualified = grammar::lex("READ source");
    let mut input = unqualified.input();
    let parsed = grammar::Stmt::parse(&mut input).expect("unqualified branch parses");
    assert!(input.is_eof());
    match parsed.root().value() {
        grammar::Stmt::Read(node) => assert!(node.debug.is_none()),
        _ => panic!("READ selected the wrong frozen branch"),
    }

    let extra = grammar::lex("PRINT message trailing");
    let mut input = extra.input();
    let error = match grammar::Stmt::parse(&mut input) {
        Err(error) => error,
        Ok(_) => panic!("caller-FOLLOW guard accepted a non-EOF suffix"),
    };
    assert_eq!(error.span(), recursa::Span::new(14, 22).unwrap());
    assert_eq!(
        error.found(),
        Some("content:\"[a-zA-Z_][a-zA-Z0-9_]*\":priority=None")
    );
    assert_eq!(input.cursor(), 0, "failed nullable dispatch is transactional");

    let malformed = grammar::lex("CREATE unknown");
    let mut input = malformed.input();
    let error = match grammar::Statement::parse(&mut input) {
        Err(error) => error,
        Ok(_) => panic!("the second LL(2) position selected a branch"),
    };
    assert_eq!(error.span(), recursa::Span::new(7, 14).unwrap());
    assert_eq!(
        error.found(),
        Some("content:\"[a-zA-Z_][a-zA-Z0-9_]*\":priority=None")
    );
    assert_eq!(
        error.expected().collect::<Vec<_>>(),
        [
            "fixed:TABLE",
            "fixed:VIEW",
            "fixed:INDEX",
            "fixed:SCHEMA",
            "fixed:FUNCTION",
            "fixed:PROCEDURE",
        ],
    );
    assert_eq!(input.cursor(), 0, "failed FIRST-k dispatch is transactional");

    let nested_malformed = grammar::lex("CREATE unknown");
    let mut input = nested_malformed.input();
    let error = match grammar::NestedStatement::parse(&mut input) {
        Err(error) => error,
        Ok(_) => panic!("a direct semantic wrapper hid the nested LL(2) failure"),
    };
    assert_eq!(error.span(), recursa::Span::new(7, 14).unwrap());
    assert_eq!(
        error.expected().collect::<Vec<_>>(),
        [
            "fixed:TABLE",
            "fixed:VIEW",
            "fixed:INDEX",
            "fixed:SCHEMA",
            "fixed:FUNCTION",
            "fixed:PROCEDURE",
        ],
    );
    assert_eq!(input.cursor(), 0);

    let column = grammar::lex("value AS alias");
    let mut input = column.input();
    let parsed = grammar::Column::parse(&mut input)
        .expect("static-plus-caller FOLLOW works through the optional alias");
    assert!(input.is_eof());
    assert_eq!(parsed.root().value().name.text(), "value");
    assert_eq!(
        parsed
            .root()
            .value()
            .as_alias
            .as_ref()
            .unwrap()
            .alias
            .text(),
        "alias"
    );

    let nullable_static = grammar::lex("PRINT message");
    let mut input = nullable_static.input();
    let parsed = grammar::NullablePrefix::parse(&mut input)
        .expect("a nullable enum branch is selected by static FOLLOW");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.root().value().maybe_debug,
        grammar::MaybeDebug::Absent(_)
    ));
    assert_eq!(parsed.root().span(), Some(recursa::Span::new(0, 13).unwrap()));
    assert_eq!(
        parsed.root().span_elements().get(0).unwrap().span(),
        None,
        "the all-nullable predictive child has no fabricated span"
    );
    assert_eq!(
        parsed.root().span_elements().get(1).unwrap().span(),
        Some(recursa::Span::new(0, 13).unwrap()),
    );
    assert_eq!(
        parsed
            .root()
            .value()
            .tail
            .as_ref()
            .expect("PRINT starts the optional tail")
            .rest
            .text(),
        "message"
    );

    let nullable_caller = grammar::lex("");
    let mut input = nullable_caller.input();
    let parsed = grammar::NullablePrefix::parse(&mut input)
        .expect("a nullable enum branch is selected by caller EOF FOLLOW");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.root().value().maybe_debug,
        grammar::MaybeDebug::Absent(_)
    ));
    assert!(parsed.root().value().tail.is_none());
    assert_eq!(parsed.root().span(), None);
    assert_eq!(parsed.root().span_elements().get(0).unwrap().span(), None);
    assert_eq!(parsed.root().span_elements().get(1).unwrap().span(), None);

    let first_rectangle = grammar::lex("B C");
    let mut input = first_rectangle.input();
    let parsed = grammar::OverlappingRectangles::parse(&mut input)
        .expect("B C selects the first complete language");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.root().value(),
        grammar::OverlappingRectangles::First(_)
    ));

    let second_rectangle = grammar::lex("B D");
    let mut input = second_rectangle.input();
    let parsed = grammar::OverlappingRectangles::parse(&mut input)
        .expect("B D selects the second complete language");
    assert!(input.is_eof());
    assert!(matches!(
        parsed.root().value(),
        grammar::OverlappingRectangles::Second(_)
    ));

    println!("predictive-ok");
}
