use recursa_generated_greedy::grammar;

fn outcome<T: std::fmt::Debug>(result: Result<T, recursa::ParseError>) -> String {
    match result {
        Ok(value) => format!("ok {value:?}"),
        Err(error) => format!("err {}", error.code()),
    }
}

fn main() {
    for source in ["", "SELECT", "SELECT SELECT", "SELECT SELECT SELECT"] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        println!(
            "query {source:?}: {}",
            outcome(grammar::Query::parse(&mut input).map(|parsed| parsed.into_ast()))
        );
    }
    for source in ["1 !", "1 ! !", "1"] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        println!(
            "stmt {source:?}: {}",
            outcome(grammar::Stmt::parse(&mut input).map(|parsed| parsed.into_ast()))
        );
    }
}
