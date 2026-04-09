use recursa::{Input, NoRules, Parse, ParseRules, Scan};

// Verify that derive macros are accessible through the recursa crate
#[derive(Scan)]
#[scan(pattern = "hello")]
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
fn recursa_reexports_scan_trait() {
    let mut input = Input::new("hello");
    let _h = <Hello as Parse>::parse(&mut input, &NoRules).unwrap();
    assert_eq!(input.cursor(), 5);
}

#[test]
fn recursa_reexports_parse_rules() {
    let mut input = Input::new("   hello");
    input.consume_ignored(MyRules::ignore_regex());
    assert_eq!(input.remaining(), "hello");
}

#[test]
fn recursa_reexports_parse_error() {
    use recursa::miette::Diagnostic;
    let err = recursa::ParseError::new("test", 0..4, "something");
    // Verify ParseError is accessible and implements Diagnostic
    let _: &dyn Diagnostic = &err;
}

// Verify bulk macros work through the recursa crate
recursa::keywords! {
    TestLet => "let",
}

#[test]
fn recursa_reexports_macros() {
    let mut input = Input::new("let");
    let _kw = <TestLet as Parse>::parse(&mut input, &NoRules).unwrap();
    assert_eq!(input.cursor(), 3);
}
