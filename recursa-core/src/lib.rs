//! Core traits and types for the recursa parser framework.

mod error;
pub mod fmt;
mod input;
mod macros;
mod parse;
mod rules;
pub mod seq;
pub mod surrounded;
pub mod visitor;

pub use error::ParseError;
pub use fmt::{FormatTokens, TokenText};
pub use input::Input;
pub use parse::Parse;
pub use rules::{NoRules, ParseRules};
pub use visitor::{AsNodeKey, Break, NodeKey, TotalVisitor, Visit, Visitor};

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

    struct TestKeyword;

    impl<'input> Parse<'input> for TestKeyword {
        fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
            input.remaining().starts_with("test")
        }

        fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
            if input.remaining().starts_with("test") {
                input.advance(4);
                Ok(TestKeyword)
            } else {
                Err(ParseError::new(
                    input.source().to_string(),
                    input.cursor()..input.cursor(),
                    "test",
                ))
            }
        }
    }

    struct TestIdent<'input>(&'input str);

    impl<'input> Parse<'input> for TestIdent<'input> {
        fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
            let r = input.remaining();
            r.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        }

        fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
            let r = input.remaining();
            let len = r
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            if len == 0 {
                return Err(ParseError::new(
                    input.source().to_string(),
                    input.cursor()..input.cursor(),
                    "identifier",
                ));
            }
            let start = input.cursor();
            let matched = &input.source()[start..start + len];
            input.advance(len);
            Ok(TestIdent(matched))
        }
    }

    #[test]
    fn parse_keyword_peek() {
        let input = Input::new("test foo");
        assert!(TestKeyword::peek::<NoRules>(&input));
    }

    #[test]
    fn parse_keyword_peek_fails() {
        let input = Input::new("foo bar");
        assert!(!TestKeyword::peek::<NoRules>(&input));
    }

    #[test]
    fn parse_keyword_parse() {
        let mut input = Input::new("test foo");
        let _kw = TestKeyword::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn parse_ident_parse_captures() {
        let mut input = Input::new("hello world");
        let ident = TestIdent::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(ident.0, "hello");
        assert_eq!(input.cursor(), 5);
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
        WhitespaceRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "hello");
    }

    #[test]
    fn input_consume_ignored_noop_when_no_whitespace() {
        let mut input = Input::new("hello");
        WhitespaceRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "hello");
    }

    #[test]
    fn input_consume_ignored_noop_for_no_rules() {
        let mut input = Input::new("   hello");
        NoRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "   hello");
    }

    #[test]
    fn box_parse_delegates_to_inner() {
        let mut input = Input::new("test foo");
        let boxed = Box::<TestKeyword>::parse::<NoRules>(&mut input).unwrap();
        let _: Box<TestKeyword> = boxed;
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn box_peek_delegates_to_inner() {
        let input = Input::new("test foo");
        assert!(Box::<TestKeyword>::peek::<NoRules>(&input));
    }

    #[test]
    fn option_parse_some_when_peek_matches() {
        let mut input = Input::new("test foo");
        let result = Option::<TestKeyword>::parse::<NoRules>(&mut input).unwrap();
        assert!(result.is_some());
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn option_parse_none_when_peek_fails() {
        let mut input = Input::new("foo bar");
        let result = Option::<TestKeyword>::parse::<NoRules>(&mut input).unwrap();
        assert!(result.is_none());
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn option_peek_delegates() {
        let input = Input::new("test foo");
        assert!(Option::<TestKeyword>::peek::<NoRules>(&input));

        let input2 = Input::new("foo bar");
        assert!(!Option::<TestKeyword>::peek::<NoRules>(&input2));
    }

    #[test]
    fn vec_parse_zero_or_more() {
        let mut input = Input::new("testtesttest foo");
        let items = Vec::<TestKeyword>::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(input.remaining(), " foo");
    }

    #[test]
    fn vec_parse_zero_or_more_with_whitespace_rules() {
        let mut input = Input::new("test test test foo");
        let items = Vec::<TestKeyword>::parse::<WhitespaceRules>(&mut input).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(input.remaining(), " foo");
    }

    #[test]
    fn vec_parse_empty() {
        let mut input = Input::new("foo bar");
        let items = Vec::<TestKeyword>::parse::<NoRules>(&mut input).unwrap();
        assert_eq!(items.len(), 0);
        assert_eq!(input.cursor(), 0);
    }

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

    use crate::visitor::{TotalVisitor, Visit, Visitor};
    use std::ops::ControlFlow;

    // A simple manual Visit impl for testing
    struct Leaf(i32);

    impl AsNodeKey for Leaf {}

    impl Visit for Leaf {
        fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
            match visitor.total_enter(self) {
                ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
                other => return other,
            }
            visitor.total_exit(self)
        }
    }

    /// A simple TotalVisitor that counts all enter/exit calls.
    struct Counter {
        enter_count: usize,
        exit_count: usize,
    }

    impl TotalVisitor for Counter {
        type Error = ();

        fn total_enter<N>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
            self.enter_count += 1;
            ControlFlow::Continue(())
        }

        fn total_exit<N>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
            self.exit_count += 1;
            ControlFlow::Continue(())
        }
    }

    #[test]
    fn visitor_enter_exit_called() {
        let leaf = Leaf(42);
        let mut counter = Counter {
            enter_count: 0,
            exit_count: 0,
        };
        let _ = leaf.visit(&mut counter);
        assert_eq!(counter.enter_count, 1);
        assert_eq!(counter.exit_count, 1);
    }

    #[test]
    fn visitor_downcast_in_enter() {
        /// A TotalVisitor that dispatches to Visitor<Leaf> for type-safe access.
        struct TypeChecker {
            found_leaf: bool,
        }

        impl Visitor<Leaf> for TypeChecker {
            type Error = ();
            fn enter(&mut self, leaf: &Leaf) -> ControlFlow<Break<()>> {
                self.found_leaf = true;
                assert_eq!(leaf.0, 42);
                ControlFlow::Continue(())
            }
        }

        impl TotalVisitor for TypeChecker {
            type Error = ();

            fn total_enter<N>(&mut self, node: &N) -> ControlFlow<Break<()>> {
                if ::std::any::type_name::<N>() == ::std::any::type_name::<Leaf>() {
                    let node = unsafe { &*(node as *const N as *const Leaf) };
                    return <Self as Visitor<Leaf>>::enter(self, node);
                }
                ControlFlow::Continue(())
            }

            fn total_exit<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
                ControlFlow::Continue(())
            }
        }

        let leaf = Leaf(42);
        let mut checker = TypeChecker { found_leaf: false };
        let _ = leaf.visit(&mut checker);
        assert!(checker.found_leaf);
    }

    #[test]
    fn visitor_skip_children() {
        struct Skipper;

        impl TotalVisitor for Skipper {
            type Error = ();

            fn total_enter<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
                ControlFlow::Break(Break::SkipChildren)
            }

            fn total_exit<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
                ControlFlow::Continue(())
            }
        }

        let leaf = Leaf(42);
        let mut skipper = Skipper;
        let result = leaf.visit(&mut skipper);
        assert!(matches!(result, ControlFlow::Continue(())));
    }

    #[test]
    fn visit_box_delegates() {
        let boxed = Box::new(Leaf(99));
        let mut counter = Counter {
            enter_count: 0,
            exit_count: 0,
        };
        let _ = boxed.visit(&mut counter);
        assert_eq!(counter.enter_count, 1); // Leaf's enter, not Box's
        assert_eq!(counter.exit_count, 1);
    }

    #[test]
    fn visit_option_some() {
        let opt = Some(Leaf(1));
        let mut counter = Counter {
            enter_count: 0,
            exit_count: 0,
        };
        let _ = opt.visit(&mut counter);
        assert_eq!(counter.enter_count, 1);
    }

    #[test]
    fn visit_option_none() {
        let opt: Option<Leaf> = None;
        let mut counter = Counter {
            enter_count: 0,
            exit_count: 0,
        };
        let _ = opt.visit(&mut counter);
        assert_eq!(counter.enter_count, 0);
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
