//! Drives the container grammar and prints what the test compares between the two parser
//! styles: the Debug value of every positive parse with the cursor it leaves, where a root parse
//! stops when tokens follow it, the value of every entry point, and for every failing input the
//! error, the restored cursor and the number of heap allocations still live after the error is
//! dropped.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use recursa_generated_table_driven_containers::grammar;
use recursa::{HasSpan as _, NodeView as _, Pretty as _};

/// Counts live heap allocations so a failed parse can prove it dropped every partial vector and
/// box.
struct LiveCounting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: every allocation is forwarded to the system allocator unchanged; the counter is a
// side channel that never influences the returned pointer or layout.
unsafe impl GlobalAlloc for LiveCounting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for `layout`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
        // SAFETY: `pointer` came from `alloc` with this `layout`.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: LiveCounting = LiveCounting;

/// Positive inputs, one group per container shape.
const POSITIVE: &[&str] = &[
    // Let: erased group, envelope presence, Box, inlined Option<Box<Alias>>.
    "let if exists x = 1;",
    "let one if not exists x = 2 as y;",
    "let if exists as = only as temp;",
    // Print: bare presence, helper Option<Guard>.
    "print 1;",
    "print not x if y;",
    "print not !!only if [1, 2];",
    "print as if temp;",
    // Call: admitted keyword as callee, inlined Vec with trailing separator.
    "f();",
    "in(as, temp);",
    "f(1, 2,);",
    "out(only,);",
    // Define: Vec1 with separator, inlined Option<Mode>, helper Option<Box<Value>>.
    "fn f(x);",
    "fn as(in x, out only, temp) returns 1;",
    "fn g(x, y) returns [!1];",
    // With: inlined presence, inlined Vec1 with trailing separator.
    "with a;",
    "with recursive a | b |;",
    "with a | b;",
    // Block: inlined Vec without separator.
    "{}",
    "{ print 1; { let if exists x = 2; } f(); }",
    // Modifiers: three erased groups and three presences, two demoted to helpers.
    "mods x;",
    "mods one three four six x;",
    "mods two five x;",
    "mods one two three four five six x;",
    // Erased: five erased groups, the last demoted to a helper.
    "erased x;",
    "erased one five x;",
    "erased one two three four five x;",
    // Options: five valued unit optionals, the last demoted to a helper.
    "options;",
    "options as out temp;",
    "options as in out only temp;",
    "options temp;",
    // Trail: four erased groups and a trailing separator demoted to a helper.
    "trail a, b,;",
    "trail one four a;",
    "trail one two three four a, b;",
    // Boxes: inlined Vec<Box<Value>> with a separator.
    "boxes;",
    "boxes 1, as, [2];",
    // Marks: helper Vec<Bang> and helper Option<Vec1<Bang>>.
    "marks :;",
    "marks !! : !;",
    "marks : !!!;",
    // Filter: Vec1 without separator, repeated attachment with an erased group inside.
    "filter !;",
    "filter !! :1 :not 2;",
    "filter ! :not [as];",
    // Value shapes through Print: inlined Option<Vec1<Value>>, per-element attachment.
    "print [];",
    "print [[], [1, [2]]];",
    "print (1);",
    "print (1)|(as)|([]);",
    "print !(1)|(2);",
];

/// Inputs that fail; every one must restore the cursor and leak nothing.
const NEGATIVE: &[&str] = &[
    "",
    "let x = 1;",
    "let if exists x = ;",
    "let if exists x = 1 as;",
    "print;",
    "print 1 if;",
    "f(1,,);",
    "f(1",
    "fn f();",
    "fn f(x) returns;",
    "with;",
    "with recursive a |",
    "{ print 1; { let if exists x = 2; } f() }",
    "mods one one x;",
    "erased five one x;",
    "options temp as;",
    "trail a,,;",
    "boxes 1 2;",
    "marks !!;",
    "filter :1;",
    "filter ! :not;",
    "print [1,];",
    "print (1)|;",
    "print [;",
];

/// Drives every input through one parser: `$tag` prefixes each printed line and `$input`
/// builds that parser's input over a lexing result. The test compares the lines of the two
/// parsers of the one generation.
macro_rules! drive {
    ($tag:expr, $input:expr) => {{
        let tag: &str = $tag;
        // Prints the provenance of one positive input parsed with capture requested: the
        // occurrence tree, the root's elements and the Pretty rendering. The comparison
        // requires all three to be identical between the parsers.
        let provenance = |source: &str| {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Statement::parse_spanned(&mut input).expect("positive input parses with capture");
            println!("{tag} prov {source:?} => {}", parsed.debug_provenance());
            let elements: Vec<String> = parsed
                .root()
                .span_elements()
                .map(|element| {
                    format!(
                        "{}:{:?}@{}",
                        element.name().unwrap_or("?"),
                        element.role(),
                        span_text(element.span())
                    )
                })
                .collect();
            println!("{tag} elements {source:?} => [{}]", elements.join(" "));
            println!("{tag} pretty {source:?} => {:?}", parsed.render());
        };
        for source in POSITIVE {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Statement::parse(&mut input)
                .unwrap_or_else(|error| panic!("positive input {source:?} failed: {error}"));
            println!(
                "{tag} ok {source:?} => {:?} cursor={} eof={}",
                parsed.into_ast(),
                input.cursor(),
                input.is_eof()
            );
            assert!(input.is_eof(), "a positive input is consumed whole");
            provenance(source);
        }

        let source = "with a | b print 1; { }";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let first = grammar::Statement::parse(&mut input).expect_err("a list without its terminator fails");
        println!("{tag} prefix first code={} found={:?} cursor={}", first.code(), first.found(), input.cursor());
        let source = "f(1) ; {} print 2;";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let first = grammar::Statement::parse(&mut input).expect("first root parses");
        println!("{tag} prefix second {:?} cursor={}", first.into_ast(), input.cursor());
        let second = grammar::Statement::parse(&mut input).expect("second root parses");
        println!("{tag} prefix third {:?} cursor={}", second.into_ast(), input.cursor());
        let third = grammar::Statement::parse(&mut input).expect("third root parses");
        println!("{tag} prefix fourth {:?} cursor={} eof={}", third.into_ast(), input.cursor(), input.is_eof());

        let source = "[1, [as, only]]";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let value = grammar::Value::parse(&mut input).expect("a value parses from its own entry");
        println!("{tag} entry {:?} cursor={}", value.into_ast(), input.cursor());
        let source = "(in x, out temp)";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let params = grammar::Params::parse(&mut input).expect("a parameter list parses from its own entry");
        println!("{tag} entry {:?} cursor={}", params.into_ast(), input.cursor());
        let source = "only";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let name = grammar::Name::parse(&mut input).expect("an admitted keyword parses as a name");
        println!("{tag} entry {:?} cursor={}", name.into_ast(), input.cursor());
        let source = "as x";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let alias = grammar::Alias::parse(&mut input).expect("an alias parses from its own entry");
        println!("{tag} entry {:?} cursor={}", alias.into_ast(), input.cursor());

        // A struct root ending in a content token accepts the prefix; a repetition tail and a
        // one-token alternative reject the token after them; a closing token accepts (#118).
        for (source, expect) in [("x y", "Param"), ("(1) x", "Value"), ("[1] x", "Value"), ("x y", "Value")] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let result = match expect {
                "Param" => grammar::Param::parse(&mut input).map(|parsed| format!("{:?}", parsed.into_ast())),
                _ => grammar::Value::parse(&mut input).map(|parsed| format!("{:?}", parsed.into_ast())),
            };
            match result {
                Ok(value) => println!("{tag} trailing {expect} {source:?} => ok {value} cursor={} eof={}", input.cursor(), input.is_eof()),
                Err(error) => println!(
                    "{tag} trailing {expect} {source:?} => code={} kind={:?} span={:?} found={:?} cursor={}",
                    error.code(),
                    error.kind(),
                    error.span(),
                    error.found(),
                    input.cursor()
                ),
            }
        }

        for source in NEGATIVE {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            // The recursive-descent parser interns composed FOLLOW sets on first use and leaks
            // those records by design, so the live count is taken on a warm second parse.
            drop(grammar::Statement::parse(&mut input).expect_err("failing input fails"));
            let baseline = LIVE.load(Ordering::SeqCst);
            let error = grammar::Statement::parse(&mut input).expect_err("failing input fails");
            let (code, kind, span, found) = (error.code(), error.kind(), error.span(), error.found());
            drop(error);
            let leaked = LIVE.load(Ordering::SeqCst) - baseline;
            println!(
                "{tag} err {source:?} => code={code} kind={kind:?} span={span:?} found={found:?} cursor={} leaked={leaked}",
                input.cursor()
            );
            assert_eq!(leaked, 0, "a failed parse must drop every partial value");
            assert_eq!(input.cursor(), 0, "a failed parse restores the cursor");
        }
    }};
}

/// The default parser's input.
fn rd_input<'input>(lexed: &grammar::LexResult<'input>) -> grammar::Input<'input> {
    lexed.input()
}

fn main() {
    drive!("rd", rd_input);
    drive!("lr", grammar::parsers::lr::input);
    println!("table-driven-containers-ok");
}


/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<recursa::Span>) -> String {
    span.map_or_else(|| "-".to_owned(), |span| format!("{}..{}", span.start(), span.end()))
}
