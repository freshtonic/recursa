use recursa_legacy_sql::sql;

fn parse(source: &str) -> Result<sql::Statement<'_>, recursa::ParseError> {
    let lexed = sql::lex(source);
    assert_eq!(lexed.errors().count(), 0, "lex errors in {source:?}");
    let mut input = lexed.input();
    let parsed = sql::Statement::parse(&mut input)?;
    assert!(input.is_eof(), "accepted parse must consume explicit EOF");
    Ok(parsed.into_ast())
}

fn parsed(source: &str) -> sql::Statement<'_> {
    parse(source).unwrap_or_else(|error| panic!("{source:?} did not parse: {error}"))
}

fn variant_and_rest<'visit, 'input>(
    statement: &'visit sql::Statement<'input>,
) -> (&'static str, &'visit str)
where
    'input: 'visit,
{
    match statement {
        sql::Statement::CreateTable(node) => ("create-table", node.rest.text()),
        sql::Statement::CreateView(node) => ("create-view", node.rest.text()),
        sql::Statement::CreateIndex(node) => ("create-index", node.rest.text()),
        sql::Statement::CreateSchema(node) => ("create-schema", node.rest.text()),
        sql::Statement::CreateFunction(node) => ("create-function", node.rest.text()),
        sql::Statement::CreateProcedure(node) => ("create-procedure", node.rest.text()),
        sql::Statement::DropTable(node) => ("drop-table", node.rest.text()),
        sql::Statement::DropView(node) => ("drop-view", node.rest.text()),
        sql::Statement::DropIndex(node) => ("drop-index", node.rest.text()),
        sql::Statement::DropSchema(node) => ("drop-schema", node.rest.text()),
        sql::Statement::DropFunction(node) => ("drop-function", node.rest.text()),
        sql::Statement::DropProcedure(node) => ("drop-procedure", node.rest.text()),
    }
}

#[test]
fn create_table() {
    assert_eq!(
        variant_and_rest(&parsed("CREATE TABLE foo")),
        ("create-table", "foo")
    );
}

#[test]
fn create_view() {
    assert_eq!(
        variant_and_rest(&parsed("CREATE VIEW bar")),
        ("create-view", "bar")
    );
}

#[test]
fn create_index() {
    assert_eq!(
        variant_and_rest(&parsed("CREATE INDEX idx_users")),
        ("create-index", "idx_users")
    );
}

#[test]
fn create_schema() {
    assert_eq!(
        variant_and_rest(&parsed("CREATE SCHEMA myschema")),
        ("create-schema", "myschema")
    );
}

#[test]
fn create_function() {
    assert_eq!(
        variant_and_rest(&parsed("CREATE FUNCTION add_one")),
        ("create-function", "add_one")
    );
}

#[test]
fn create_procedure() {
    assert_eq!(
        variant_and_rest(&parsed("CREATE PROCEDURE p")),
        ("create-procedure", "p")
    );
}

#[test]
fn drop_table() {
    assert_eq!(
        variant_and_rest(&parsed("DROP TABLE foo")),
        ("drop-table", "foo")
    );
}

#[test]
fn drop_view() {
    assert_eq!(
        variant_and_rest(&parsed("DROP VIEW bar")),
        ("drop-view", "bar")
    );
}

#[test]
fn drop_index() {
    assert_eq!(
        variant_and_rest(&parsed("DROP INDEX idx_users")),
        ("drop-index", "idx_users")
    );
}

#[test]
fn drop_schema() {
    assert_eq!(
        variant_and_rest(&parsed("DROP SCHEMA myschema")),
        ("drop-schema", "myschema")
    );
}

#[test]
fn drop_function() {
    assert_eq!(
        variant_and_rest(&parsed("DROP FUNCTION add_one")),
        ("drop-function", "add_one")
    );
}

#[test]
fn drop_procedure() {
    assert_eq!(
        variant_and_rest(&parsed("DROP PROCEDURE p")),
        ("drop-procedure", "p")
    );
}

#[test]
fn create_table_carries_both_keywords() {
    let statement = parsed("CREATE TABLE exact_name");
    assert_eq!(variant_and_rest(&statement), ("create-table", "exact_name"));
    let lexed = sql::lex("CREATE TABLE exact_name");
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens[0].kind(), sql::TokenKind::CREATE);
    assert_eq!(tokens[0].text(), "CREATE");
    assert_eq!(tokens[1].kind(), sql::TokenKind::TABLE);
    assert_eq!(tokens[1].text(), "TABLE");
}

#[test]
fn drop_table_carries_both_keywords() {
    let statement = parsed("DROP TABLE exact_name");
    assert_eq!(variant_and_rest(&statement), ("drop-table", "exact_name"));
    let lexed = sql::lex("DROP TABLE exact_name");
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(tokens[0].kind(), sql::TokenKind::DROP);
    assert_eq!(tokens[0].text(), "DROP");
    assert_eq!(tokens[1].kind(), sql::TokenKind::TABLE);
    assert_eq!(tokens[1].text(), "TABLE");
}

#[test]
fn missing_leading_keyword_errors() {
    assert!(parse("TABLE foo").is_err());
}

#[test]
fn unknown_create_body_keyword_errors() {
    assert!(parse("CREATE WIDGET foo").is_err());
}

#[test]
fn unknown_drop_body_keyword_errors() {
    assert!(parse("DROP WIDGET foo").is_err());
}

#[test]
fn tokenize_recognises_all_keywords() {
    let lexed = sql::lex("CREATE DROP TABLE VIEW INDEX SCHEMA FUNCTION PROCEDURE thingy");
    assert_eq!(lexed.errors().count(), 0);
    let tokens = lexed.tokens().collect::<Vec<_>>();
    assert_eq!(
        tokens.iter().map(|token| token.kind()).collect::<Vec<_>>(),
        [
            sql::TokenKind::CREATE,
            sql::TokenKind::DROP,
            sql::TokenKind::TABLE,
            sql::TokenKind::VIEW,
            sql::TokenKind::INDEX,
            sql::TokenKind::SCHEMA,
            sql::TokenKind::FUNCTION,
            sql::TokenKind::PROCEDURE,
            sql::TokenKind::Rest,
        ]
    );
    assert_eq!(tokens.last().unwrap().text(), "thingy");
}
