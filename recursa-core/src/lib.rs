//! Core traits and types for the recursa parser framework.

mod error;
mod input;
mod macros;
mod parse;
mod rules;
mod scan;
pub mod seq;

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

    impl_parse_for_scan!(TestKeyword);

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

    impl_parse_for_scan!(TestIdent<'input>);

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

    #[test]
    fn scan_type_is_terminal() {
        let is_terminal = <TestKeyword as Parse>::IS_TERMINAL;
        assert!(is_terminal);
    }

    #[test]
    fn scan_type_first_pattern() {
        assert_eq!(<TestKeyword as Parse>::first_pattern(), "test");
    }

    #[test]
    fn scan_ident_first_pattern() {
        assert_eq!(
            <TestIdent as Parse>::first_pattern(),
            r"[a-zA-Z_][a-zA-Z0-9_]*"
        );
    }

    struct WhitespaceRules;

    impl ParseRules for WhitespaceRules {
        const IGNORE: &'static str = r"\s+";
    }

    #[test]
    fn input_consume_ignored_skips_whitespace() {
        let mut input = Input::<WhitespaceRules>::new("   hello");
        input.consume_ignored();
        assert_eq!(input.remaining(), "hello");
    }

    #[test]
    fn input_consume_ignored_noop_when_no_whitespace() {
        let mut input = Input::<WhitespaceRules>::new("hello");
        input.consume_ignored();
        assert_eq!(input.remaining(), "hello");
    }

    #[test]
    fn input_consume_ignored_noop_for_no_rules() {
        let mut input = Input::<NoRules>::new("   hello");
        input.consume_ignored();
        assert_eq!(input.remaining(), "   hello");
    }

    #[test]
    fn input_rebind_preserves_cursor() {
        let mut input = Input::<WhitespaceRules>::new("hello world");
        input.advance(6);
        let rebound: Input<'_, NoRules> = input.rebind();
        assert_eq!(rebound.cursor(), 6);
        assert_eq!(rebound.remaining(), "world");
        assert_eq!(rebound.source(), "hello world");
    }

    #[test]
    fn box_parse_delegates_to_inner() {
        let mut input = Input::<NoRules>::new("test foo");
        let boxed = <Box<TestKeyword> as Parse>::parse(&mut input).unwrap();
        let _: Box<TestKeyword> = boxed;
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn box_peek_delegates_to_inner() {
        let input = Input::<NoRules>::new("test foo");
        assert!(<Box<TestKeyword> as Parse>::peek(&input));
    }

    #[test]
    fn box_is_terminal_delegates() {
        const { assert!(<Box<TestKeyword> as Parse>::IS_TERMINAL) };
    }

    #[test]
    fn box_first_pattern_delegates() {
        assert_eq!(
            <Box<TestKeyword> as Parse>::first_pattern(),
            <TestKeyword as Parse>::first_pattern()
        );
    }

    #[test]
    fn option_parse_some_when_peek_matches() {
        let mut input = Input::<NoRules>::new("test foo");
        let result = <Option<TestKeyword> as Parse>::parse(&mut input).unwrap();
        assert!(result.is_some());
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn option_parse_none_when_peek_fails() {
        let mut input = Input::<NoRules>::new("foo bar");
        let result = <Option<TestKeyword> as Parse>::parse(&mut input).unwrap();
        assert!(result.is_none());
        assert_eq!(input.cursor(), 0); // no input consumed
    }

    #[test]
    fn option_peek_delegates() {
        let input = Input::<NoRules>::new("test foo");
        assert!(<Option<TestKeyword> as Parse>::peek(&input));

        let input2 = Input::<NoRules>::new("foo bar");
        assert!(!<Option<TestKeyword> as Parse>::peek(&input2));
    }

    #[test]
    fn option_first_pattern_delegates() {
        assert_eq!(
            <Option<TestKeyword> as Parse>::first_pattern(),
            <TestKeyword as Parse>::first_pattern()
        );
    }

    #[test]
    fn input_rebind_roundtrip() {
        let mut input = Input::<WhitespaceRules>::new("  hello");
        input.consume_ignored();
        let mut rebound: Input<'_, NoRules> = input.rebind();
        let _kw = <TestIdent as Scan>::parse(&mut rebound).unwrap();
        // Commit the rebound back via rebind
        let back: Input<'_, WhitespaceRules> = rebound.rebind();
        input.commit(back);
        assert_eq!(input.cursor(), 7);
        assert!(input.is_empty());
    }

    use crate::seq::Seq;

    #[test]
    fn seq_empty() {
        let seq: Seq<i32, ()> = Seq::empty();
        assert_eq!(seq.len(), 0);
        assert!(seq.is_empty());
        let elements: &[i32] = &seq;
        assert!(elements.is_empty());
    }

    #[test]
    fn seq_from_pairs() {
        let pairs = vec![(1, Some(())), (2, Some(())), (3, None)];
        let seq: Seq<i32, ()> = Seq::from_pairs(pairs);
        assert_eq!(seq.len(), 3);
        let elements: &[i32] = &seq;
        assert_eq!(elements, &[1, 2, 3]);
    }

    #[test]
    fn seq_pairs_accessible() {
        let pairs = vec![(1, Some(',')), (2, None)];
        let seq: Seq<i32, char> = Seq::from_pairs(pairs);
        let raw_pairs = seq.pairs();
        assert_eq!(raw_pairs.len(), 2);
        assert_eq!(raw_pairs[0], (1, Some(',')));
        assert_eq!(raw_pairs[1], (2, None));
    }
}
