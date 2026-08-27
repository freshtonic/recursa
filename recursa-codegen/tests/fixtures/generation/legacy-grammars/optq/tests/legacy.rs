use recursa_legacy_optq::optq;

fn parse(source: &str) -> Result<optq::Stmt<'_>, recursa::ParseError> {
    let lexed = optq::lex(source);
    assert_eq!(lexed.errors().count(), 0, "lex errors in {source:?}");
    let mut input = lexed.input();
    let parsed = optq::Stmt::parse(&mut input)?;
    assert!(input.is_eof(), "accepted parse must consume explicit EOF");
    Ok(parsed.into_ast())
}

fn parsed(source: &str) -> optq::Stmt<'_> {
    parse(source).unwrap_or_else(|error| panic!("{source:?} did not parse: {error}"))
}

fn shape<'visit, 'input>(statement: &'visit optq::Stmt<'input>) -> (&'static str, bool, &'visit str)
where
    'input: 'visit,
{
    match statement {
        optq::Stmt::Print(node) => ("print", node.debug.is_some(), node.rest.text()),
        optq::Stmt::Read(node) => ("read", node.debug.is_some(), node.rest.text()),
        optq::Stmt::Set(node) => ("set", node.debug.is_some(), node.rest.text()),
    }
}

#[test]
fn plain_print() {
    assert_eq!(shape(&parsed("PRINT hello")), ("print", false, "hello"));
}

#[test]
fn plain_read() {
    assert_eq!(shape(&parsed("READ buffer")), ("read", false, "buffer"));
}

#[test]
fn plain_set() {
    assert_eq!(shape(&parsed("SET x")), ("set", false, "x"));
}

#[test]
fn debug_print() {
    assert_eq!(
        shape(&parsed("DEBUG PRINT hello")),
        ("print", true, "hello")
    );
}

#[test]
fn debug_read() {
    assert_eq!(
        shape(&parsed("DEBUG READ buffer")),
        ("read", true, "buffer")
    );
}

#[test]
fn debug_set() {
    assert_eq!(shape(&parsed("DEBUG SET x")), ("set", true, "x"));
}

#[test]
fn debug_alone_errors() {
    assert!(parse("DEBUG").is_err());
}

#[test]
fn debug_then_unknown_command_errors() {
    assert!(parse("DEBUG FOO bar").is_err());
}

#[test]
fn unknown_leading_keyword_errors() {
    assert!(parse("FOO bar").is_err());
}
