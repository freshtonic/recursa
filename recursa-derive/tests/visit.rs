use std::ops::ControlFlow;

use recursa_core::{AsNodeKey, Break, Visit, Visitor};
use recursa_derive::Visit;

// -- Leaf types (manual Visit impl for testing) --

struct Token;
impl AsNodeKey for Token {}
impl Visit for Token {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
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

impl Visitor for Counter {
    type Error = ();

    fn enter<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
        self.enters += 1;
        ControlFlow::Continue(())
    }

    fn exit<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
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
    impl Visitor for SkipTwoTokens {
        type Error = ();

        fn enter<N: Visit>(&mut self, node: &N) -> ControlFlow<Break<()>> {
            if node.is::<TwoTokens>() {
                ControlFlow::Break(Break::SkipChildren)
            } else {
                ControlFlow::Continue(())
            }
        }
    }

    let node = TwoTokens { a: Token, b: Token };
    let mut s = SkipTwoTokens;
    let result = node.visit(&mut s);
    // Should complete successfully (SkipChildren is not an error)
    assert!(matches!(result, ControlFlow::Continue(())));
}
