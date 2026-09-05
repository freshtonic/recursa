//! Drives the same inputs through the `lr` parser: `parsers::lr::input` builds the input that
//! selects it, and the inherent `Statement::parse` and `parse_spanned` accept it beside the
//! default parser's input.

use recursa::{NodeView as _, Pretty as _};
use recursa_generated_named_parsers::grammar::{self, parsers::lr};

/// The inputs every parser of the grammar is driven through.
const INPUTS: &[&str] = &[
    "let x = 5;",
    "print(!!false);",
    "let z = 1 + !2 + x;",
    "",
    "let x = ;",
    "let x = 1 print(2);",
    "print(1 + 2",
];

fn main() {
    for source in INPUTS {
        let lexed = grammar::lex(source);
        let mut input = lr::input(&lexed);
        match grammar::Statement::parse(&mut input) {
            Ok(parsed) => println!(
                "ok {source:?} => {:?} cursor={} eof={}",
                parsed.into_ast(),
                input.cursor(),
                input.is_eof()
            ),
            Err(error) => {
                let expected: Vec<&'static str> = error.expected().collect();
                println!(
                    "err {source:?} => code={} kind={:?} span={:?} found={:?} cursor={} expected={expected:?}",
                    error.code(),
                    error.kind(),
                    error.span(),
                    error.found(),
                    input.cursor()
                );
                continue;
            }
        }
        let mut input = lr::input_with_builder(&lexed, ());
        let parsed = grammar::Statement::parse_spanned(&mut input)
            .expect("positive input parses with capture");
        println!("prov {source:?} => {}", parsed.debug_provenance());
        println!("pretty {source:?} => {:?}", parsed.render());
    }
    println!("lr-ok");
}
