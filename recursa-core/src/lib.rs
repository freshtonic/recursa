//! Core traits and types for the recursa parser framework.

mod error;
mod input;
mod rules;

pub use error::ParseError;
pub use input::Input;
pub use rules::{NoRules, ParseRules};

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
}
