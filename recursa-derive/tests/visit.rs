use std::ops::ControlFlow;

use recursa_core::{AsNodeKey, Break, TotalVisitor, Visit};
use recursa_derive::Visit;

// -- Leaf types (manual Visit impl for testing) --

struct Token;
impl AsNodeKey for Token {}
impl Visit for Token {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.total_enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.total_exit(self)
    }
}

// -- Derived Visit --

#[derive(Visit)]
struct TwoTokens {
    a: Token,
    b: Token,
}

#[derive(Visit)]
enum Choice {
    First(Token),
    Second(TwoTokens),
}

// -- Counter visitor --

struct Counter {
    enters: usize,
    exits: usize,
}

impl TotalVisitor for Counter {
    type Error = ();

    fn total_enter<N: 'static>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
        self.enters += 1;
        ControlFlow::Continue(())
    }

    fn total_exit<N: 'static>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
        self.exits += 1;
        ControlFlow::Continue(())
    }
}

#[test]
fn visit_struct_visits_fields() {
    let node = TwoTokens { a: Token, b: Token };
    let mut c = Counter {
        enters: 0,
        exits: 0,
    };
    let _ = node.visit(&mut c);
    // TwoTokens enter + Token a enter + Token b enter = 3
    assert_eq!(c.enters, 3);
    // TwoTokens exit + Token a exit + Token b exit = 3
    assert_eq!(c.exits, 3);
}

#[test]
fn visit_enum_visits_variant() {
    let node = Choice::First(Token);
    let mut c = Counter {
        enters: 0,
        exits: 0,
    };
    let _ = node.visit(&mut c);
    // Choice enter + Token enter = 2
    assert_eq!(c.enters, 2);
    // Choice exit + Token exit = 2
    assert_eq!(c.exits, 2);
}

#[test]
fn visit_enum_second_variant() {
    let node = Choice::Second(TwoTokens { a: Token, b: Token });
    let mut c = Counter {
        enters: 0,
        exits: 0,
    };
    let _ = node.visit(&mut c);
    // Choice enter + TwoTokens enter + Token a enter + Token b enter = 4
    assert_eq!(c.enters, 4);
    assert_eq!(c.exits, 4);
}

#[test]
fn visit_skip_children() {
    struct SkipTwoTokens;
    impl TotalVisitor for SkipTwoTokens {
        type Error = ();

        fn total_enter<N: 'static>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
            if std::any::TypeId::of::<N>() == std::any::TypeId::of::<TwoTokens>() {
                ControlFlow::Break(Break::SkipChildren)
            } else {
                ControlFlow::Continue(())
            }
        }

        fn total_exit<N: 'static>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
            ControlFlow::Continue(())
        }
    }

    let node = TwoTokens { a: Token, b: Token };
    let mut s = SkipTwoTokens;
    let result = node.visit(&mut s);
    // Should complete successfully (SkipChildren is not an error)
    assert!(matches!(result, ControlFlow::Continue(())));
}

// -- Test #[derive(TotalVisitor)] --

use recursa_core::Visitor;
use recursa_derive::TotalVisitor;

#[derive(TotalVisitor)]
#[total_visitor(dispatch = [TwoTokens], error = ())]
struct TwoTokensCounter {
    two_tokens_count: usize,
}

impl Visitor<TwoTokens> for TwoTokensCounter {
    type Error = ();
    fn enter(&mut self, _node: &TwoTokens) -> ControlFlow<Break<()>> {
        self.two_tokens_count += 1;
        ControlFlow::Continue(())
    }
}

#[test]
fn derive_total_visitor_dispatches_to_typed_visitor() {
    let node = Choice::Second(TwoTokens { a: Token, b: Token });
    let mut v = TwoTokensCounter {
        two_tokens_count: 0,
    };
    let _ = node.visit(&mut v);
    // Only TwoTokens triggers the counter, not Choice or Token
    assert_eq!(v.two_tokens_count, 1);
}

#[test]
fn derive_total_visitor_ignores_undispatched_types() {
    let node = Choice::First(Token);
    let mut v = TwoTokensCounter {
        two_tokens_count: 0,
    };
    let _ = node.visit(&mut v);
    // Token is not in the dispatch list, so count stays 0
    assert_eq!(v.two_tokens_count, 0);
}
