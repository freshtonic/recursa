use std::ops::ControlFlow;

use recursa::{Break, Input, Parse, ParseRules, TotalVisitor, Visit};

#[derive(Parse, Visit, Debug, Clone)]
#[parse(pattern = "let")]
struct LetKw;

#[derive(Parse, Visit, Debug, Clone)]
#[parse(pattern = "=")]
struct EqSign;

#[derive(Parse, Visit, Debug, Clone)]
#[parse(pattern = ";")]
struct Semi;

#[derive(Parse, Visit, Debug, Clone)]
#[parse(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident(String);

#[derive(Parse, Visit, Debug, Clone)]
#[parse(pattern = r"[0-9]+")]
struct IntLit(String);

struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

#[derive(Parse, Visit, Debug)]
#[parse(rules = Lang)]
struct LetStmt {
    let_kw: LetKw,
    name: Ident,
    eq: EqSign,
    value: IntLit,
    semi: Semi,
}

struct IdentCollector {
    idents: Vec<String>,
}

impl TotalVisitor for IdentCollector {
    type Error = ();

    fn total_enter<N>(&mut self, node: &N) -> ControlFlow<Break<()>> {
        if std::any::type_name::<N>() == std::any::type_name::<Ident>() {
            let ident = unsafe { &*(node as *const N as *const Ident) };
            self.idents.push(ident.0.clone());
        }
        ControlFlow::Continue(())
    }

    fn total_exit<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
        ControlFlow::Continue(())
    }
}

#[test]
fn visitor_collects_idents() {
    let mut input = Input::new("let x = 42;");
    let stmt = LetStmt::parse::<Lang>(&mut input).unwrap();
    let mut collector = IdentCollector { idents: vec![] };
    let _ = stmt.visit(&mut collector);
    assert_eq!(collector.idents, vec!["x"]);
}

#[test]
fn visitor_skip_children_prevents_descent() {
    struct SkipLetStmt;
    impl TotalVisitor for SkipLetStmt {
        type Error = ();
        fn total_enter<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
            if std::any::type_name::<N>() == std::any::type_name::<LetStmt>() {
                ControlFlow::Break(Break::SkipChildren)
            } else {
                ControlFlow::Continue(())
            }
        }
        fn total_exit<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
            ControlFlow::Continue(())
        }
    }

    let mut input = Input::new("let x = 42;");
    let stmt = LetStmt::parse::<Lang>(&mut input).unwrap();
    let mut skipper = SkipLetStmt;
    let result = stmt.visit(&mut skipper);
    assert!(matches!(result, ControlFlow::Continue(())));
}

#[test]
fn visitor_counts_all_nodes() {
    struct NodeCounter {
        count: usize,
    }
    impl TotalVisitor for NodeCounter {
        type Error = ();
        fn total_enter<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
            self.count += 1;
            ControlFlow::Continue(())
        }
        fn total_exit<N>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
            ControlFlow::Continue(())
        }
    }

    let mut input = Input::new("let x = 42;");
    let stmt = LetStmt::parse::<Lang>(&mut input).unwrap();
    let mut counter = NodeCounter { count: 0 };
    let _ = stmt.visit(&mut counter);
    // LetStmt + LetKw + Ident + String(x) + EqSign + IntLit + String(42) + Semi = 8
    assert_eq!(counter.count, 8);
}
