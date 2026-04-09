//! Core traits and types for the recursa parser framework.

mod error;
mod input;
mod macros;
mod parse;
mod rules;
mod scan;
pub mod seq;
pub mod visitor;

pub use error::ParseError;
pub use input::Input;
pub use parse::Parse;
pub use rules::{NoRules, ParseRules};
pub use scan::Scan;
pub use visitor::{AsNodeKey, Break, NodeKey, Visit, Visitor};

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
        let input = Input::new("hello world");
        assert_eq!(input.cursor(), 0);
        assert_eq!(input.remaining(), "hello world");
    }

    #[test]
    fn input_advance() {
        let mut input = Input::new("hello world");
        input.advance(5);
        assert_eq!(input.cursor(), 5);
        assert_eq!(input.remaining(), " world");
    }

    #[test]
    fn input_fork_does_not_affect_original() {
        let input = Input::new("hello world");
        let mut fork = input.fork();
        fork.advance(5);
        assert_eq!(input.cursor(), 0);
        assert_eq!(fork.cursor(), 5);
    }

    #[test]
    fn input_fork_commit() {
        let mut input = Input::new("hello world");
        let mut fork = input.fork();
        fork.advance(5);
        input.commit(fork);
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn input_source() {
        let input = Input::new("hello world");
        assert_eq!(input.source(), "hello world");
    }

    #[test]
    fn input_is_empty_at_end() {
        let mut input = Input::new("hi");
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
        let input = Input::new("test foo");
        assert!(<TestKeyword as Scan>::peek(&input));
    }

    #[test]
    fn scan_keyword_peek_fails() {
        let input = Input::new("foo bar");
        assert!(!<TestKeyword as Scan>::peek(&input));
    }

    #[test]
    fn scan_keyword_parse() {
        let mut input = Input::new("test foo");
        let _kw = <TestKeyword as Scan>::parse(&mut input).unwrap();
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn scan_ident_parse_captures() {
        let mut input = Input::new("hello world");
        let ident = <TestIdent as Scan>::parse(&mut input).unwrap();
        assert_eq!(ident.0, "hello");
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn scan_type_implements_parse() {
        // TestKeyword implements Scan, so it should also implement Parse via blanket impl
        let mut input = Input::new("test foo");
        let _kw = <TestKeyword as Parse>::parse(&mut input, &NoRules).unwrap();
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn scan_type_peek_through_parse() {
        let input = Input::new("test foo");
        assert!(<TestKeyword as Parse>::peek(&input, &NoRules));
    }

    #[test]
    fn scan_type_peek_through_parse_fails() {
        let input = Input::new("foo bar");
        assert!(!<TestKeyword as Parse>::peek(&input, &NoRules));
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

        fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
            static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            &CACHE
        }
    }

    #[test]
    fn input_consume_ignored_skips_whitespace() {
        let mut input = Input::new("   hello");
        input.consume_ignored(WhitespaceRules::ignore_regex());
        assert_eq!(input.remaining(), "hello");
    }

    #[test]
    fn input_consume_ignored_noop_when_no_whitespace() {
        let mut input = Input::new("hello");
        input.consume_ignored(WhitespaceRules::ignore_regex());
        assert_eq!(input.remaining(), "hello");
    }

    #[test]
    fn input_consume_ignored_noop_for_no_rules() {
        let mut input = Input::new("   hello");
        input.consume_ignored(NoRules::ignore_regex());
        assert_eq!(input.remaining(), "   hello");
    }

    #[test]
    fn box_parse_delegates_to_inner() {
        let mut input = Input::new("test foo");
        let boxed = <Box<TestKeyword> as Parse>::parse(&mut input, &NoRules).unwrap();
        let _: Box<TestKeyword> = boxed;
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn box_peek_delegates_to_inner() {
        let input = Input::new("test foo");
        assert!(<Box<TestKeyword> as Parse>::peek(&input, &NoRules));
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
        let mut input = Input::new("test foo");
        let result = <Option<TestKeyword> as Parse>::parse(&mut input, &NoRules).unwrap();
        assert!(result.is_some());
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn option_parse_none_when_peek_fails() {
        let mut input = Input::new("foo bar");
        let result = <Option<TestKeyword> as Parse>::parse(&mut input, &NoRules).unwrap();
        assert!(result.is_none());
        assert_eq!(input.cursor(), 0); // no input consumed
    }

    #[test]
    fn option_peek_delegates() {
        let input = Input::new("test foo");
        assert!(<Option<TestKeyword> as Parse>::peek(&input, &NoRules));

        let input2 = Input::new("foo bar");
        assert!(!<Option<TestKeyword> as Parse>::peek(&input2, &NoRules));
    }

    #[test]
    fn option_first_pattern_delegates() {
        assert_eq!(
            <Option<TestKeyword> as Parse>::first_pattern(),
            <TestKeyword as Parse>::first_pattern()
        );
    }

    use std::any::TypeId;
    use crate::visitor::{Break, NodeKey};

    #[test]
    fn node_key_equality() {
        let x = 42u32;
        let k1 = NodeKey::new(&x);
        let k2 = NodeKey::new(&x);
        assert_eq!(k1, k2);
    }

    #[test]
    fn node_key_different_nodes() {
        let x = 42u32;
        let y = 42u32;
        let k1 = NodeKey::new(&x);
        let k2 = NodeKey::new(&y);
        assert_ne!(k1, k2); // different addresses
    }

    #[test]
    fn node_key_get_as() {
        let x = 42u32;
        let k = NodeKey::new(&x);
        assert_eq!(k.get_as::<u32>(), Some(&42));
        assert_eq!(k.get_as::<i32>(), None); // wrong type
    }

    #[test]
    fn node_key_hashable() {
        use std::collections::HashMap;
        let x = 42u32;
        let k = NodeKey::new(&x);
        let mut map = HashMap::new();
        map.insert(k, "found");
        assert_eq!(map.get(&k), Some(&"found"));
    }

    #[test]
    fn break_variants() {
        let _skip: Break<String> = Break::SkipChildren;
        let _fin: Break<String> = Break::Finished;
        let _err: Break<String> = Break::Err("oops".to_string());
    }

    use std::ops::ControlFlow;
    use crate::visitor::{Visit, Visitor};

    // A simple manual Visit impl for testing
    struct Leaf(i32);

    impl AsNodeKey for Leaf {}

    impl Visit for Leaf {
        fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
            match visitor.enter(self) {
                ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
                other => return other,
            }
            visitor.exit(self)
        }
    }

    struct Counter {
        enter_count: usize,
        exit_count: usize,
    }

    impl Visitor for Counter {
        type Error = ();

        fn enter<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
            self.enter_count += 1;
            ControlFlow::Continue(())
        }

        fn exit<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
            self.exit_count += 1;
            ControlFlow::Continue(())
        }
    }

    #[test]
    fn visitor_enter_exit_called() {
        let leaf = Leaf(42);
        let mut counter = Counter { enter_count: 0, exit_count: 0 };
        leaf.visit(&mut counter);
        assert_eq!(counter.enter_count, 1);
        assert_eq!(counter.exit_count, 1);
    }

    #[test]
    fn visitor_downcast_in_enter() {
        struct TypeChecker { found_leaf: bool }
        impl Visitor for TypeChecker {
            type Error = ();
            fn enter<N: Visit>(&mut self, node: &N) -> ControlFlow<Break<Self::Error>> {
                if let Some(leaf) = node.downcast_ref::<Leaf>() {
                    self.found_leaf = true;
                    assert_eq!(leaf.0, 42);
                }
                ControlFlow::Continue(())
            }
        }

        let leaf = Leaf(42);
        let mut checker = TypeChecker { found_leaf: false };
        leaf.visit(&mut checker);
        assert!(checker.found_leaf);
    }

    #[test]
    fn visitor_skip_children() {
        // SkipChildren should not propagate up as an error
        struct Skipper;
        impl Visitor for Skipper {
            type Error = ();
            fn enter<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
                ControlFlow::Break(Break::SkipChildren)
            }
            fn exit<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
                ControlFlow::Continue(())
            }
        }

        let leaf = Leaf(42);
        let mut skipper = Skipper;
        let result = leaf.visit(&mut skipper);
        assert!(matches!(result, ControlFlow::Continue(())));
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
