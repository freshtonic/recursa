//! Core traits and types for the recursa parser framework.

mod error;
mod input;
mod parse;
mod rules;
mod scan;

pub use error::ParseError;
pub use input::Input;
pub use parse::Parse;
pub use rules::{NoRules, ParseRules};
pub use scan::Scan;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rules_ignore_is_empty() {
        assert_eq!(<NoRules as ParseRules>::IGNORE, "");
    }

    use miette::Diagnostic;

    #[test]
    fn parse_error_is_diagnostic() {
        let err = ParseError::new("let 123 = foo;", 4..7, "identifier");
        // Verify it implements Diagnostic (miette)
        let _: &dyn Diagnostic = &err;
        assert_eq!(err.expected(), "identifier");
    }

    #[test]
    fn parse_error_with_context() {
        let inner = ParseError::new("let 123 = foo;", 4..7, "identifier");
        let outer = inner.with_context("let binding", 0..3);
        // The related errors should contain the context
        let related: Vec<_> = outer.related().into_iter().flatten().collect();
        assert_eq!(related.len(), 1);
    }

    #[test]
    fn parse_error_with_help() {
        let err = ParseError::new("let 123 = foo;", 4..7, "identifier")
            .with_help("variable names must start with a letter or underscore");
        assert!(err.help().is_some());
    }

    #[test]
    fn parse_error_merge_combines_expected() {
        let e1 = ParseError::new("foo", 0..3, "integer");
        let e2 = ParseError::new("foo", 0..3, "string");
        let merged = ParseError::merge(vec![e1, e2]);
        assert_eq!(merged.expected(), "one of: integer, string");
    }

    #[test]
    fn parse_error_display_without_found() {
        let err = ParseError::new("abc", 0..3, "digit");
        assert_eq!(format!("{err}"), "expected digit");
    }

    #[test]
    fn parse_error_display_with_found() {
        let err = ParseError::new("abc", 0..3, "digit").with_found("letter");
        assert_eq!(format!("{err}"), "expected digit but found letter");
    }

    #[test]
    fn input_starts_at_zero() {
        let input = Input::<NoRules>::new("hello world");
        assert_eq!(input.cursor(), 0);
        assert_eq!(input.remaining(), "hello world");
    }

    #[test]
    fn input_advance() {
        let mut input = Input::<NoRules>::new("hello world");
        input.advance(5);
        assert_eq!(input.cursor(), 5);
        assert_eq!(input.remaining(), " world");
    }

    #[test]
    fn input_fork_does_not_affect_original() {
        let input = Input::<NoRules>::new("hello world");
        let mut fork = input.fork();
        fork.advance(5);
        assert_eq!(input.cursor(), 0);
        assert_eq!(fork.cursor(), 5);
    }

    #[test]
    fn input_fork_commit() {
        let mut input = Input::<NoRules>::new("hello world");
        let mut fork = input.fork();
        fork.advance(5);
        input.commit(fork);
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn input_source() {
        let input = Input::<NoRules>::new("hello world");
        assert_eq!(input.source(), "hello world");
    }

    #[test]
    fn input_is_empty_at_end() {
        let mut input = Input::<NoRules>::new("hi");
        assert!(!input.is_empty());
        input.advance(2);
        assert!(input.is_empty());
    }

    use regex::Regex;
    use std::sync::OnceLock;

    struct TestKeyword;

    impl Scan<'_> for TestKeyword {
        const PATTERN: &'static str = r"test";

        fn regex() -> &'static Regex {
            static REGEX: OnceLock<Regex> = OnceLock::new();
            REGEX.get_or_init(|| Regex::new(r"\Atest").unwrap())
        }

        fn from_match(_matched: &str) -> Result<Self, ParseError> {
            Ok(TestKeyword)
        }
    }

    struct TestIdent<'input>(&'input str);

    impl<'input> Scan<'input> for TestIdent<'input> {
        const PATTERN: &'static str = r"[a-zA-Z_][a-zA-Z0-9_]*";

        fn regex() -> &'static Regex {
            static REGEX: OnceLock<Regex> = OnceLock::new();
            REGEX.get_or_init(|| Regex::new(r"\A[a-zA-Z_][a-zA-Z0-9_]*").unwrap())
        }

        fn from_match(matched: &'input str) -> Result<Self, ParseError> {
            Ok(TestIdent(matched))
        }
    }

    #[test]
    fn scan_keyword_peek() {
        let input = Input::<NoRules>::new("test foo");
        assert!(<TestKeyword as Scan>::peek(&input));
    }

    #[test]
    fn scan_keyword_peek_fails() {
        let input = Input::<NoRules>::new("foo bar");
        assert!(!<TestKeyword as Scan>::peek(&input));
    }

    #[test]
    fn scan_keyword_parse() {
        let mut input = Input::<NoRules>::new("test foo");
        let _kw = <TestKeyword as Scan>::parse(&mut input).unwrap();
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn scan_ident_parse_captures() {
        let mut input = Input::<NoRules>::new("hello world");
        let ident = <TestIdent as Scan>::parse(&mut input).unwrap();
        assert_eq!(ident.0, "hello");
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn scan_type_implements_parse() {
        // TestKeyword implements Scan, so it should also implement Parse
        let mut input = Input::<NoRules>::new("test foo");
        let _kw = <TestKeyword as Parse>::parse(&mut input).unwrap();
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn scan_type_peek_through_parse() {
        let input = Input::<NoRules>::new("test foo");
        assert!(<TestKeyword as Parse>::peek(&input));
    }

    #[test]
    fn scan_type_peek_through_parse_fails() {
        let input = Input::<NoRules>::new("foo bar");
        assert!(!<TestKeyword as Parse>::peek(&input));
    }
}
