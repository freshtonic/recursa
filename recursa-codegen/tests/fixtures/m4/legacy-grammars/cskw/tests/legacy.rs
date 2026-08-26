use recursa_legacy_cskw::cskw;

fn parse(source: &str) -> Result<cskw::Stmt<'_>, recursa::ParseError> {
    let lexed = cskw::lex(source);
    assert_eq!(lexed.errors().count(), 0, "lex errors in {source:?}");
    let mut input = lexed.input();
    let parsed = cskw::Stmt::parse(&mut input)?;
    assert!(input.is_eof(), "accepted parse must consume explicit EOF");
    Ok(parsed.into_ast())
}

fn parsed(source: &str) -> cskw::Stmt<'_> {
    parse(source).unwrap_or_else(|error| panic!("{source:?} did not parse: {error}"))
}

fn select<'visit, 'input>(
    statement: &'visit cskw::Stmt<'input>,
) -> &'visit cskw::SelectStmt<'input> {
    match statement {
        cskw::Stmt::Select(select) => select,
    }
}

#[test]
fn basic_select() {
    let statement = parsed("SELECT col FROM tab");
    let select = select(&statement);
    assert_eq!(select.cols.len(), 1);
    assert_eq!(select.cols[0].name.text(), "col");
    assert!(select.cols[0].as_alias.is_none());
    assert_eq!(select.table.text(), "tab");
}

#[test]
fn select_with_alias() {
    let statement = parsed("SELECT col AS alias FROM tab");
    let select = select(&statement);
    assert_eq!(select.cols.len(), 1);
    assert_eq!(select.cols[0].name.text(), "col");
    assert_eq!(
        select.cols[0]
            .as_alias
            .as_ref()
            .expect("alias is present")
            .alias
            .text(),
        "alias"
    );
}

#[test]
fn multiple_columns() {
    let statement = parsed("SELECT a, b, c FROM tab");
    let select = select(&statement);
    assert_eq!(select.cols.len(), 3);
    assert_eq!(
        select
            .cols
            .iter()
            .map(|column| column.name.text())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
}

#[test]
fn case_insensitive_keywords() {
    let statement = parsed("select col from tab");
    assert_eq!(select(&statement).cols[0].name.text(), "col");
    let lexed = cskw::lex("select col from tab");
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens[0].kind(), cskw::TokenKind::SELECT);
    assert_eq!(tokens[0].text(), "select");
    assert_eq!(tokens[2].kind(), cskw::TokenKind::FROM);
    assert_eq!(tokens[2].text(), "from");

    assert!(parse("sElEcT col fRoM tab").is_ok());
}

#[test]
fn as_can_be_a_column_name() {
    let statement = parsed("SELECT as FROM tab");
    let select = select(&statement);
    assert_eq!(select.cols[0].name.text(), "as");
    assert_eq!(select.cols[0].name.kind(), cskw::TokenKind::AS);
    assert!(select.cols[0].as_alias.is_none());
}

#[test]
fn as_can_be_a_table_name() {
    let statement = parsed("SELECT col FROM aS");
    let select = select(&statement);
    assert_eq!(select.table.text(), "aS");
    assert_eq!(select.table.kind(), cskw::TokenKind::AS);
}

#[test]
fn select_cannot_be_a_column_name() {
    assert!(parse("SELECT select FROM tab").is_err());
}

#[test]
fn from_cannot_be_a_column_name() {
    assert!(parse("SELECT from FROM tab").is_err());
}

#[test]
fn as_cannot_be_an_alias() {
    assert!(parse("SELECT col AS as FROM tab").is_err());
}

#[test]
fn keyword_cannot_be_a_table_name_for_reserved_words() {
    assert!(parse("SELECT col FROM SELECT").is_err());
}
