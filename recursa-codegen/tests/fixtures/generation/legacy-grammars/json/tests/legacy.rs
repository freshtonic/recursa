use recursa_legacy_json::json;

fn parse(source: &str) -> Result<json::Value<'_>, recursa::ParseError> {
    let lexed = json::lex(source);
    assert_eq!(lexed.errors().count(), 0, "lex errors in {source:?}");
    let mut input = lexed.input();
    let parsed = json::Value::parse(&mut input)?;
    assert!(input.is_eof(), "accepted parse must consume explicit EOF");
    Ok(parsed.into_ast())
}

fn parsed(source: &str) -> json::Value<'_> {
    parse(source).unwrap_or_else(|error| panic!("{source:?} did not parse: {error}"))
}

fn assert_single_token(source: &str, kind: json::TokenKind) {
    let lexed = json::lex(source);
    assert_eq!(lexed.errors().count(), 0, "lex errors in {source:?}");
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens.len(), 1, "tokens for {source:?}");
    assert_eq!(tokens[0].kind(), kind);
    assert_eq!(tokens[0].text(), source);
    assert_eq!(
        tokens[0].span(),
        recursa::Span::new(0, source.len()).unwrap()
    );
}

#[test]
fn null_literal() {
    assert!(matches!(parsed("null"), json::Value::Null));
}

#[test]
fn true_literal() {
    assert!(matches!(
        parsed("true"),
        json::Value::Bool(json::Bool::True)
    ));
}

#[test]
fn false_literal() {
    assert!(matches!(
        parsed("false"),
        json::Value::Bool(json::Bool::False)
    ));
}

#[test]
fn number_literal() {
    let json::Value::Number(number) = parsed("42") else {
        panic!("expected number")
    };
    assert_eq!(number.text(), "42");
}

#[test]
fn string_literal() {
    let json::Value::String(string) = parsed(r#""hello""#) else {
        panic!("expected string")
    };
    assert_eq!(string.text(), r#""hello""#);
}

#[test]
fn empty_array() {
    let json::Value::Array(array) = parsed("[]") else {
        panic!("expected array")
    };
    assert!(array.values.is_empty());
}

#[test]
fn singleton_array() {
    let json::Value::Array(array) = parsed("[1]") else {
        panic!("expected array")
    };
    assert_eq!(array.values.len(), 1);
    assert!(matches!(array.values[0], json::Value::Number(_)));
}

#[test]
fn array_of_three_numbers() {
    let json::Value::Array(array) = parsed("[1, 2, 3]") else {
        panic!("expected array")
    };
    assert_eq!(array.values.len(), 3);
    assert!(
        array
            .values
            .iter()
            .all(|value| matches!(value, json::Value::Number(_)))
    );
}

#[test]
fn array_of_mixed_types() {
    let json::Value::Array(array) = parsed("[true, null, false]") else {
        panic!("expected array")
    };
    assert!(matches!(
        array.values[0],
        json::Value::Bool(json::Bool::True)
    ));
    assert!(matches!(array.values[1], json::Value::Null));
    assert!(matches!(
        array.values[2],
        json::Value::Bool(json::Bool::False)
    ));
}

#[test]
fn empty_object() {
    let json::Value::Object(object) = parsed("{}") else {
        panic!("expected object")
    };
    assert!(object.members.is_empty());
}

#[test]
fn single_member_object() {
    let json::Value::Object(object) = parsed(r#"{"a": 1}"#) else {
        panic!("expected object")
    };
    assert_eq!(object.members.len(), 1);
    assert_eq!(object.members[0].key.text(), r#""a""#);
    assert!(matches!(object.members[0].value, json::Value::Number(_)));
}

#[test]
fn two_member_object() {
    let json::Value::Object(object) = parsed(r#"{"a": 1, "b": 2}"#) else {
        panic!("expected object")
    };
    assert_eq!(object.members.len(), 2);
    assert_eq!(object.members[0].key.text(), r#""a""#);
    assert_eq!(object.members[1].key.text(), r#""b""#);
}

#[test]
fn array_of_arrays() {
    let json::Value::Array(array) = parsed("[[1], [2]]") else {
        panic!("expected array")
    };
    assert_eq!(array.values.len(), 2);
    assert!(matches!(array.values[0], json::Value::Array(_)));
    assert!(matches!(array.values[1], json::Value::Array(_)));
}

#[test]
fn object_containing_array() {
    let json::Value::Object(object) = parsed(r#"{"a": [1, 2]}"#) else {
        panic!("expected object")
    };
    let json::Value::Array(array) = &object.members[0].value else {
        panic!("expected nested array")
    };
    assert_eq!(array.values.len(), 2);
}

#[test]
fn array_containing_object() {
    let json::Value::Array(array) = parsed(r#"[{"a": 1}]"#) else {
        panic!("expected array")
    };
    assert!(matches!(array.values[0], json::Value::Object(_)));
}

#[test]
fn realistic_nested_input() {
    let value = parsed(
        r#"{
            "name": "spike",
            "tags": ["parser", "pratt", "ll1"],
            "depth": 3
        }"#,
    );
    let json::Value::Object(object) = value else {
        panic!("expected object")
    };
    assert_eq!(object.members.len(), 3);
    assert_eq!(object.members[0].key.text(), r#""name""#);
}

#[test]
fn string_handles_escapes_and_unicode() {
    for string in [
        r#""""#,
        r#""hello""#,
        r#""a\"b""#,
        r#""\\ \/ \b \f \n \r \t""#,
        r#""\u00e9 \uD83D\uDE00""#,
        "\"café 😀\"",
    ] {
        assert_single_token(string, json::TokenKind::String);
    }
}

#[test]
fn string_with_escapes_parses_as_a_value() {
    assert!(matches!(parsed(r#""café \"x\"""#), json::Value::String(_)));
    let json::Value::Object(object) = parsed(r#"{"kéy": "v\tw"}"#) else {
        panic!("expected object")
    };
    assert_eq!(object.members.len(), 1);
}

#[test]
fn number_covers_int_frac_and_exp() {
    for number in [
        "0", "-0", "42", "-42", "3.14", "-3.14", "0.5", "1e10", "1E10", "1e+10", "1e-10", "1.5e3",
        "-2.5E-8",
    ] {
        assert_single_token(number, json::TokenKind::Number);
    }
}

#[test]
fn number_rejects_leading_zero_as_single_token() {
    let lexed = json::lex("01");
    assert_eq!(lexed.errors().count(), 0);
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens.len(), 2);
    assert!(
        tokens
            .iter()
            .all(|token| token.kind() == json::TokenKind::Number)
    );
    assert_eq!(tokens[0].text(), "0");
    assert_eq!(tokens[1].text(), "1");
}

#[test]
fn number_with_exponent_parses_as_a_value() {
    assert!(matches!(parsed("-2.5E-8"), json::Value::Number(_)));
    let json::Value::Array(array) = parsed("[1e10, -0.5, 0]") else {
        panic!("expected array")
    };
    assert_eq!(array.values.len(), 3);
}

#[test]
fn string_value_node_captures_token_and_text() {
    let json::Value::String(string) = parsed(r#""hi""#) else {
        panic!("expected string")
    };
    assert_eq!(string.kind(), json::TokenKind::String);
    assert_eq!(string.text(), r#""hi""#);
}

#[test]
fn number_value_node_captures_token_and_text() {
    let json::Value::Number(number) = parsed("3.14") else {
        panic!("expected number")
    };
    assert_eq!(number.kind(), json::TokenKind::Number);
    assert_eq!(number.text(), "3.14");
}

#[test]
fn captured_text_reflects_the_actual_source() {
    let json::Value::String(string) = parsed(r#""a\nb é""#) else {
        panic!("expected string")
    };
    assert_eq!(string.text(), r#""a\nb é""#);
    let json::Value::Number(number) = parsed("-2.5E-8") else {
        panic!("expected number")
    };
    assert_eq!(number.text(), "-2.5E-8");
}
