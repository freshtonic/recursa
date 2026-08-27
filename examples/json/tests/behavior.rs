use recursa_example_json::{ParseFailure, grammar, parse};

#[test]
fn nested_json_parses_into_the_declared_semantic_shape() {
    let value = parse(r#"{"answer":[true,null,42]}"#).expect("representative JSON parses");

    let grammar::Value::Object(object) = value else {
        panic!("root is an object");
    };
    assert_eq!(object.members.len(), 1);
    assert_eq!(object.members[0].key.text(), r#""answer""#);
    let grammar::Value::Array(array) = &object.members[0].value else {
        panic!("member value is an array");
    };
    assert_eq!(array.values.as_ref().map(recursa::Vec1::len), Some(3));
}

#[test]
fn containers_and_literals_preserve_explicit_semantic_cardinality() {
    let grammar::Value::Object(empty_object) = parse("{}").expect("empty object parses") else {
        panic!("value is an object");
    };
    assert!(empty_object.members.is_empty());

    let grammar::Value::Array(empty_array) = parse("[]").expect("empty array parses") else {
        panic!("value is an array");
    };
    assert!(empty_array.values.is_none());

    let grammar::Value::Array(nonempty_array) =
        parse(r#"["text",-12.5e+2]"#).expect("nonempty array parses")
    else {
        panic!("value is an array");
    };
    let values = nonempty_array.values.expect("nonempty array carries Vec1");
    assert_eq!(values.len(), 2);
    let grammar::Value::String(text) = &values[0] else {
        panic!("first value is a string");
    };
    assert_eq!(text.text(), r#""text""#);
    let grammar::Value::Number(number) = &values[1] else {
        panic!("second value is a number");
    };
    assert_eq!(number.text(), "-12.5e+2");
}

#[test]
fn invalid_or_trailing_json_is_rejected() {
    assert!(matches!(parse("@"), Err(ParseFailure::Lexical(_))));
    assert!(matches!(parse("{"), Err(ParseFailure::Syntax(_))));
    assert!(parse("null true").is_err());
}

#[cfg(feature = "explorer")]
#[test]
fn recursive_json_parses_through_the_erased_explorer_adapter() {
    let source = r#"{"answer":[true,null,{"nested":"value"}]}"#;
    let document = grammar::Value::explorer()
        .parse(source)
        .expect("recursive JSON parses through the adapter");
    assert_eq!(document.source(), Some(source));
    assert_eq!(
        document.root().span(),
        Some(recursa::Span::new(0, source.len()).unwrap())
    );
    assert_eq!(document.root().active_variant().unwrap().name(), "Object");
}
