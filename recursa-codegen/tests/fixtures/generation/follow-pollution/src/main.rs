//! Drives every root through the sole LR parser surface: `lexed.input()`, the inherent `parse`,
//! and `parse_spanned`.

use recursa::{NodeView as _, Pretty as _};
use recursa_generated_follow_pollution::grammar;

/// The `Statement` inputs: the shared-helper shape.
pub const STATEMENTS: &[&str] = &[
    "a 1 c", "a 1 d", "a c", "b 1 d", "b 1 c", "b d", "a 1", "b 1 x",
];

/// The `Expr` inputs: a root that continues past the wrapper or ends in `ColId`, and the
/// parenthesised query that the unit-rule pair of #143 lets an `Expr` derive.
pub const EXPRS: &[&str] = &["foo", "foo()", "foo(", "foo bar", "((1))"];

/// The `FuncName` inputs: a root that can end only in the later-declared wrapper.
pub const FUNC_NAMES: &[&str] = &["foo.bar", "foo", "foo."];

/// The `SelectClause` inputs: gram.y's motivating example for factoring `select_with_parens`.
pub const SELECT_CLAUSES: &[&str] = &["(((1)) union 1)", "1"];

/// The `ParenContent` inputs: the type that derives either side of the unit-rule pair, so its
/// start state is where the two compete on the end marker alone.
pub const PAREN_CONTENTS: &[&str] = &["(1)", "((1), (1))", "1", "foo"];

/// Parses `source` as `$root` and prints what happened.
macro_rules! drive {
    ($root:ident, $label:literal, $source:expr) => {{
        let source = $source;
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        match grammar::$root::parse(&mut input) {
            Ok(parsed) => {
                println!(
                    "ok {} {source:?} => {:?} cursor={} eof={}",
                    $label,
                    parsed.into_ast(),
                    input.cursor(),
                    input.is_eof()
                );
                let mut input = lexed.input();
                let parsed = grammar::$root::parse_spanned(&mut input)
                    .expect("positive input parses with capture");
                println!("prov {} {source:?} => {}", $label, parsed.debug_provenance());
                println!("pretty {} {source:?} => {:?}", $label, parsed.render());
            }
            Err(error) => {
                let expected: Vec<&'static str> = error.expected().collect();
                println!(
                    "err {} {source:?} => code={} kind={:?} span={:?} found={:?} cursor={} expected={expected:?}",
                    $label,
                    error.code(),
                    error.kind(),
                    error.span(),
                    error.found(),
                    input.cursor()
                );
            }
        }
    }};
}

fn main() {
    for source in STATEMENTS {
        drive!(Statement, "Statement", *source);
    }
    for source in EXPRS {
        drive!(Expr, "Expr", *source);
    }
    for source in FUNC_NAMES {
        drive!(FuncName, "FuncName", *source);
    }
    for source in SELECT_CLAUSES {
        drive!(SelectClause, "SelectClause", *source);
    }
    for source in PAREN_CONTENTS {
        drive!(ParenContent, "ParenContent", *source);
    }
    println!("follow-pollution-ok");
}
