use recursa::{Input, NoRules, Parse, ParseRules};

#[derive(Parse)]
#[parse(pattern = "hello")]
struct Hello;

struct MyRules;
impl ParseRules for MyRules {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

#[test]
fn recursa_reexports_parse_trait() {
    let mut input = Input::new("hello");
    let _h = Hello::parse::<NoRules>(&mut input).unwrap();
    assert_eq!(input.cursor(), 5);
}

#[test]
fn recursa_reexports_parse_rules() {
    let mut input = Input::new("   hello");
    MyRules::consume_ignored(&mut input);
    assert_eq!(input.remaining(), "hello");
}

#[test]
fn recursa_reexports_parse_error() {
    use recursa::miette::Diagnostic;
    let err = recursa::ParseError::new("test", 0..4, "something");
    let _: &dyn Diagnostic = &err;
}

recursa::keywords! {
    TestLet => "let",
}

#[test]
fn recursa_reexports_macros() {
    let mut input = Input::new("let");
    let _kw = TestLet::parse::<NoRules>(&mut input).unwrap();
    assert_eq!(input.cursor(), 3);
}
