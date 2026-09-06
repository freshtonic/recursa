//! Drives every root through the `lr` parser: `parsers::lr::input` selects it, and the inherent
//! `parse` and `parse_spanned` accept that input beside the default parser's.

use recursa::{NodeView as _, Pretty as _};
use recursa_generated_follow_pollution::grammar::{self, parsers::lr};

/// The `Statement` inputs: the shared-helper shape.
const STATEMENTS: &[&str] = &["a 1 c", "a 1 d", "a c", "b 1 d", "b 1 c", "b d", "a 1", "b 1 x"];

/// The `Expr` inputs: a root that continues past the wrapper or ends in `ColId`.
const EXPRS: &[&str] = &["foo", "foo()", "foo(", "foo bar"];

/// The `FuncName` inputs: a root that can end only in the later-declared wrapper.
const FUNC_NAMES: &[&str] = &["foo.bar", "foo", "foo."];

/// Parses `source` as `$root` through the `lr` parser and prints what happened.
macro_rules! drive {
    ($root:ident, $label:literal, $source:expr) => {{
        let source = $source;
        let lexed = grammar::lex(source);
        let mut input = lr::input(&lexed);
        match grammar::$root::parse(&mut input) {
            Ok(parsed) => {
                println!(
                    "ok {} {source:?} => {:?} cursor={} eof={}",
                    $label,
                    parsed.into_ast(),
                    input.cursor(),
                    input.is_eof()
                );
                let mut input = lr::input_with_builder(&lexed, ());
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
    println!("lr-ok");
}
