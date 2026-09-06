//! Drives the lookahead-filter grammar and prints what the test compares between the parser
//! styles and between the grammar with and without its `lookahead` declaration: the Debug value
//! of every positive parse with the cursor it leaves, where a root parse stops when tokens
//! follow it, the kinds a lexing result holds, and for every failing input the error, the
//! restored cursor and the number of heap allocations still live after the error is dropped.
//! The `expr` lines drive gram.y's `a_expr` shape, where prefix `NOT` needs its `NOT_LA` twin.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use recursa_generated_lookahead::grammar;
use recursa::{HasSpan as _, NodeView as _, Pretty as _};

/// Counts live heap allocations so a failed parse can prove it dropped every boxed partial
/// value.
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
            let parsed = grammar::Rule::parse_spanned(&mut input).expect("positive input parses with capture");
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
            "a not between 1 and 2;",
            "a not b;",
            "a not not b;",
            "not a not in (3);",
            "a not b not like c;",
            "x between 1 and 2;",
            "not not a;",
        ] {
            let lexed = grammar::lex(source);
            let kinds = lexed
                .tokens()
                .map(|token| token.kind().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let mut input = $input(&lexed);
            let parsed = grammar::Rule::parse(&mut input).expect("positive input parses");
            println!(
                "{tag} ok {source:?} => {:?} cursor={} eof={} kinds=[{kinds}]",
                parsed.into_ast(),
                input.cursor(),
                input.is_eof()
            );
            provenance(source);
        }

        // gram.y's `a_expr` shape: prefix `NOT` beside the tests, over names that admit `like`
        // and `between`, so `NOT` before a trigger is read through the prefix rule's twin.
        let expr_provenance = |source: &str| {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::ExprRule::parse_spanned(&mut input).expect("positive expression parses with capture");
            println!("{tag} expr prov {source:?} => {}", parsed.debug_provenance());
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
            println!("{tag} expr elements {source:?} => [{}]", elements.join(" "));
            println!("{tag} expr pretty {source:?} => {:?}", parsed.render());
        };
        for source in [
            "not a not like b;",
            "a not between 1 and 2;",
            "not not a;",
            "not a like b;",
            "a not in (3);",
            "not like like b;",
            "not between;",
            "a not like between;",
            "like not between 1 and 2;",
            "not (a like b) like c;",
            "a between 1 and 2;",
            "not not like not between 1 and 2;",
        ] {
            let lexed = grammar::lex(source);
            let kinds = lexed
                .tokens()
                .map(|token| token.kind().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let mut input = $input(&lexed);
            let parsed = grammar::ExprRule::parse(&mut input).expect("positive expression parses");
            println!(
                "{tag} expr ok {source:?} => {:?} cursor={} eof={} kinds=[{kinds}]",
                parsed.into_ast(),
                input.cursor(),
                input.is_eof()
            );
            expr_provenance(source);
        }
        let source = "not between";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let expr = grammar::Expr::parse(&mut input).expect("an expression parses from its own entry");
        println!("{tag} expr entry {:?} cursor={}", expr.into_ast(), input.cursor());
        for source in ["not in;", "a not;", "not;", "a not like;", "not between like;", "a not between 1;"] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            drop(grammar::ExprRule::parse(&mut input).expect_err("failing expression fails"));
            let baseline = LIVE.load(Ordering::SeqCst);
            let error = grammar::ExprRule::parse(&mut input).expect_err("failing expression fails");
            let (code, kind, span, found) = (error.code(), error.kind(), error.span(), error.found());
            drop(error);
            let leaked = LIVE.load(Ordering::SeqCst) - baseline;
            println!(
                "{tag} expr err {source:?} => code={code} kind={kind:?} span={span:?} found={found:?} cursor={} leaked={leaked}",
                input.cursor()
            );
            assert_eq!(leaked, 0, "a failed parse must drop every partial value");
            assert_eq!(input.cursor(), 0, "a failed parse restores the cursor");
        }

        // merged-kind-evidence-start: the integration test removes this block together with the
        // `lookahead` declaration, without which the merged kind does not exist.
        // The merged kind exists, spells like the token it merges, and is never lexed.
        let merged = grammar::TokenKind::NOT_LA;
        println!("{tag} merged kind {merged:?} displays as {merged} like {}", grammar::TokenKind::NOT);
        let lexed = grammar::lex("not not_la between NOT_LA in like");
        let lexed_kinds = lexed.tokens().map(|token| token.kind()).collect::<Vec<_>>();
        assert!(
            !lexed_kinds.contains(&merged),
            "a lexing result never holds the merged kind: {lexed_kinds:?}"
        );
        println!("{tag} lexed {:?}", lexed_kinds);
        // merged-kind-evidence-end

        let source = "a not b; c not between 1 and 2; between";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let first = grammar::Rule::parse(&mut input).expect("first root parses");
        println!("{tag} prefix first {:?} cursor={}", first.into_ast(), input.cursor());
        let second = grammar::Rule::parse(&mut input).expect("second root parses");
        println!(
            "{tag} prefix second {:?} cursor={} eof={}",
            second.into_ast(),
            input.cursor(),
            input.is_eof()
        );
        let third = grammar::Rule::parse(&mut input).expect_err("a bare `between` is no rule");
        println!(
            "{tag} prefix third code={} found={:?} cursor={}",
            third.code(),
            third.found(),
            input.cursor()
        );

        let source = "a not in (1)";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let test = grammar::NotIn::parse(&mut input).expect("a test parses from its own entry");
        println!("{tag} entry {:?} cursor={}", test.into_ast(), input.cursor());
        let source = "a not between";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let subject = grammar::Subject::parse(&mut input).expect("a subject parses from its own entry");
        println!("{tag} entry {:?} cursor={}", subject.into_ast(), input.cursor());
        let source = "not not a";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let cond = grammar::Cond::parse(&mut input).expect("a condition parses from its own entry");
        println!("{tag} entry {:?} cursor={}", cond.into_ast(), input.cursor());
        let source = "not";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let mark = grammar::NotMark::parse(&mut input).expect("a bare mark parses from its own entry");
        println!("{tag} entry {:?} cursor={}", mark.into_ast(), input.cursor());

        for source in [
            "",
            "a not;",
            "a not between;",
            "a not between 1 b;",
            "not;",
            "a not in 3);",
            "a between not 1 and 2;",
            "a not between 1 and 2",
            "a b;",
            "; a;",
        ] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            // The recursive-descent parser interns composed FOLLOW sets on first use and leaks
            // those records by design, so the live count is taken on a warm second parse.
            drop(grammar::Rule::parse(&mut input).expect_err("failing input fails"));
            let baseline = LIVE.load(Ordering::SeqCst);
            let error = grammar::Rule::parse(&mut input).expect_err("failing input fails");
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
    // lr-parser-start: the integration test removes this block when it generates the
    // grammar with the recursive-descent parser alone.
    drive!("lr", grammar::parsers::lr::input);
    // lr-parser-end
    println!("lookahead-ok");
}


/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<recursa::Span>) -> String {
    span.map_or_else(|| "-".to_owned(), |span| format!("{}..{}", span.start(), span.end()))
}
