//! Drives the tracer grammar and prints what the test compares between the two parser styles:
//! the Debug value of every positive parse with the cursor it leaves, where a root parse stops
//! when tokens follow it, and for every failing input the error with its expected set, the
//! restored cursor and the number of heap allocations still live after the error is dropped.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use recursa_generated_table_driven::grammar;
use recursa::{HasSpan as _, NodeView as _, Pretty as _};

/// Counts live heap allocations so a failed parse can prove it dropped every boxed partial
/// value.
struct LiveCounting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

/// Counts every heap allocation, so a capture-off parse can be compared with a capturing one.
static ALLOCATIONS: AtomicIsize = AtomicIsize::new(0);

// SAFETY: every allocation is forwarded to the system allocator unchanged; the counter is a
// side channel that never influences the returned pointer or layout.
unsafe impl GlobalAlloc for LiveCounting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(1, Ordering::SeqCst);
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
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
        for source in [
            "let x = 5;",
            "print(!x);",
            "let flag = true;",
            "print(!!false);",
            "let y = name_1;",
            "print(42);",
            "let z = 1 + !2 + x;",
        ] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Statement::parse(&mut input).expect("positive input parses");
            println!(
                "{tag} ok {source:?} => {:?} cursor={} eof={}",
                parsed.into_ast(),
                input.cursor(),
                input.is_eof()
            );
            provenance(source);
        }

        // Capture off allocates nothing for provenance: a parse with the default request allocates
        // strictly less than the same parse with capture requested, whose occurrence tree is heap
        // allocated in both styles. (The recursive-descent parser interns FOLLOW sets on first use,
        // so both counts are taken on warm parses.)
        let lexed = grammar::lex("print(!!false);");
        let allocations = |spanned: bool| {
            let mut input = $input(&lexed);
            let before = ALLOCATIONS.load(Ordering::SeqCst);
            let parsed = if spanned {
                grammar::Statement::parse_spanned(&mut input)
            } else {
                grammar::Statement::parse(&mut input)
            };
            let count = ALLOCATIONS.load(Ordering::SeqCst) - before;
            drop(parsed.expect("positive input parses"));
            count
        };
        let warm_up = (allocations(false), allocations(true));
        drop(warm_up);
        let (off, on) = (allocations(false), allocations(true));
        assert!(off < on, "capture off allocated {off}, capture on {on}");
        println!("{tag} capture off allocates less than capture on: {}", off < on);

        let source = "let x = 5; print(x); true";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let first = grammar::Statement::parse(&mut input).expect("first root parses");
        println!("{tag} prefix first {:?} cursor={}", first.into_ast(), input.cursor());
        let second = grammar::Statement::parse(&mut input).expect("second root parses");
        println!(
            "{tag} prefix second {:?} cursor={} eof={}",
            second.into_ast(),
            input.cursor(),
            input.is_eof()
        );
        let third = grammar::Statement::parse(&mut input).expect_err("a bare `true` is no statement");
        println!(
            "{tag} prefix third code={} found={:?} cursor={}",
            third.code(),
            third.found(),
            input.cursor()
        );

        // A root decided by its FOLLOW rejects the token after it: an alternative one token long
        // (`true`, `5`, `x`), also behind a nested alternative (`!!x`, `5 + 6`); a content-token
        // root and a root ending in a fixed token accept the prefix (#118).
        for source in ["true x", "5 x", "!!x y", "5 + 6 x", "5 + !6 ;"] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            match grammar::Value::parse(&mut input) {
                Ok(value) => println!("{tag} trailing {source:?} => ok {:?} cursor={}", value.into_ast(), input.cursor()),
                Err(error) => {
                    let expected: Vec<&'static str> = error.expected().collect();
                    println!(
                        "{tag} trailing {source:?} => code={} kind={:?} span={:?} found={:?} cursor={} expected={expected:?}",
                        error.code(),
                        error.kind(),
                        error.span(),
                        error.found(),
                        input.cursor()
                    );
                }
            }
        }
        let lexed = grammar::lex("count x");
        let mut input = $input(&lexed);
        let name = grammar::Name::parse(&mut input).expect("a content-token root accepts the prefix");
        println!("{tag} trailing \"count x\" => ok {:?} cursor={}", name.into_ast(), input.cursor());

        let source = "!!x";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let value = grammar::Value::parse(&mut input).expect("a value parses from its own entry");
        println!("{tag} entry {:?} cursor={}", value.into_ast(), input.cursor());
        let source = "count";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let name = grammar::Name::parse(&mut input).expect("a content token parses from its own entry");
        println!("{tag} entry {:?} cursor={}", name.into_ast(), input.cursor());

        for source in [
            "",
            "let x = ;",
            "let 5 = x;",
            "print(x",
            "print(x) let y = 1;",
            "let x = !;",
            "let x = !!!!!!!!(1);",
            "= 1;",
            "let x = 1 print(2);",
            "let x = 1 +;",
            "print(1 + 2",
        ] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            // The recursive-descent parser interns composed FOLLOW sets on first use and leaks
            // those records by design, so the live count is taken on a warm second parse.
            drop(grammar::Statement::parse(&mut input).expect_err("failing input fails"));
            // The expected names are `'static`; the vector that collects them is allocated before
            // the baseline so that it is not counted as a leak of the error.
            let mut expected: Vec<&'static str> = Vec::with_capacity(32);
            let baseline = LIVE.load(Ordering::SeqCst);
            let error = grammar::Statement::parse(&mut input).expect_err("failing input fails");
            let (code, kind, span, found) = (error.code(), error.kind(), error.span(), error.found());
            expected.extend(error.expected());
            drop(error);
            let leaked = LIVE.load(Ordering::SeqCst) - baseline;
            println!(
                "{tag} err {source:?} => code={code} kind={kind:?} span={span:?} found={found:?} cursor={} leaked={leaked} expected={expected:?}",
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
    println!("table-driven-ok");
}


/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<recursa::Span>) -> String {
    span.map_or_else(|| "-".to_owned(), |span| format!("{}..{}", span.start(), span.end()))
}
