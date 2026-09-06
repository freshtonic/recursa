//! Drives the precedence grammar and prints what the test compares between the parser styles
//! and between the recursive-descent grammar with and without its `precedence` declaration:
//! the Debug value of every positive parse with the cursor it leaves, its provenance, and for
//! every failing input the error, the restored cursor and the number of heap allocations still
//! live after the error is dropped.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use recursa_generated_precedence::grammar;
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

/// The positive inputs both parsers accept, one or more per gram.y family.
const ACCEPTED: &[&str] = &[
    // `%nonassoc ESCAPE` above `LIKE`: the escape clause, after a tighter operand too.
    "select a like b escape 'c';",
    "select a like b + c escape 'd';",
    "select a like b escape 'c' + d;",
    "select a like b escape 'c' like d;",
    "select a escape;",
    "select a like b value;",
    // `VALUE` at IDENT's level above `IS`, the bare `JSON` alternative `%prec UNBOUNDED`.
    "select a is json;",
    "select a is json x;",
    "select a value;",
    // `%nonassoc UNBOUNDED NESTED` below `PATH`.
    "columns ( nested 'p' );",
    "columns ( nested path 'p' );",
    "columns ( unbounded path );",
    // `BETWEEN` as a column name before a window frame.
    "rows unbounded preceding;",
    "rows between unbounded preceding and current row;",
    "rows between - 1 preceding and current row;",
    "rows between preceding;",
    "rows current row;",
    // `%prec UMINUS` below `'.'`.
    "select ( select x );",
    "select ( select x ) . y;",
    "select ( select x ) . y . z;",
    "select ( select x ) + 1;",
    "select - ( select x ) . y;",
    "select x . y;",
    "select ( x ) . y;",
    // Literal `LOCAL` / `SESSION`.
    "set session characteristics;",
    "set local characteristics = 1;",
    "set session x = 1;",
    "set x = 1;",
    "set characteristics = 1;",
];

/// Inputs both parsers reject.
const REJECTED: &[&str] = &[
    "",
    "select a like;",
    "select ( select x ) .;",
    "rows between unbounded preceding;",
    "columns ( nested );",
    "set session;",
    // `BETWEEN` (5) below `'-'` (10): the operator is shifted into the frame extent's first
    // bound, which then wants its `AND`, so `between` never becomes a column name here.
    "rows between - 1 preceding;",
];

/// The inputs the two parser styles part on, where the declared precedence decides the
/// keyword before the token that recursive descent decides it by is read. The driver asserts
/// nothing about the direction: it prints what each parser makes of each input, accepted or
/// rejected, and the integration test pins both lines. In every one of them the table-driven
/// outcome is PostgreSQL's, because it is what the gram.y declaration of the family says.
const PARTING: &[&str] = &[
    // `%nonassoc ESCAPE` above `LIKE`: the escape clause is shifted, so a bare `escape`
    // before `;` is no label. Recursive descent reads the token after `ESCAPE` first.
    "select a like b escape;",
    // `VALUE` at IDENT's level: both parsers accept, and the trees differ. The declaration
    // shifts `VALUE` into the `IS JSON` type; recursive descent returns it to the caller,
    // which takes it as the bare label.
    "select a is json value;",
    // The same shift, which then leaves the second `value` for the label.
    "select a is json value value;",
    // `%nonassoc NESTED` below `PATH`: `PATH` is shifted into the nested column, which then
    // wants its path string, so `nested path` is no pair of column names.
    "columns ( nested path );",
    // `BETWEEN` (5) below `'+'` (10): no frame bound starts with `'+'`, so the only action
    // reduces `between` to a column name. Recursive descent commits to the frame extent.
    "rows between + 1 preceding;",
    // `SESSION` below `CHARACTERISTICS`: the transaction form is shifted, so `characteristics`
    // is no assignable name after `session`.
    "set session characteristics = 1;",
];

/// Drives every input through one parser: `$tag` prefixes each printed line and `$input`
/// builds that parser's input over a lexing result. The test compares the lines of the two
/// parsers of the one generation.
macro_rules! drive {
    ($tag:expr, $input:expr) => {{
        let tag: &str = $tag;
        let provenance = |source: &str| {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Stmt::parse_spanned(&mut input).expect("positive input parses with capture");
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
        for source in ACCEPTED {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Stmt::parse(&mut input).expect("positive input parses");
            println!(
                "{tag} ok {source:?} => {:?} cursor={} eof={}",
                parsed.into_ast(),
                input.cursor(),
                input.is_eof()
            );
            provenance(source);
        }

        let failure = |source: &str| {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            // The recursive-descent parser interns composed FOLLOW sets on first use and leaks
            // those records by design, so the live count is taken on a warm second parse.
            drop(grammar::Stmt::parse(&mut input).expect_err("failing input fails"));
            let baseline = LIVE.load(Ordering::SeqCst);
            let error = grammar::Stmt::parse(&mut input).expect_err("failing input fails");
            let (code, kind, span, found) = (error.code(), error.kind(), error.span(), error.found());
            drop(error);
            let leaked = LIVE.load(Ordering::SeqCst) - baseline;
            let cursor = input.cursor();
            assert_eq!(leaked, 0, "a failed parse must drop every partial value");
            assert_eq!(cursor, 0, "a failed parse restores the cursor");
            format!("code={code} kind={kind:?} span={span:?} found={found:?} cursor={cursor} leaked={leaked}")
        };
        for source in REJECTED {
            println!("{tag} err {source:?} => {}", failure(source));
        }
        for source in PARTING {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            match grammar::Stmt::parse(&mut input) {
                Ok(parsed) => println!("{tag} parts ok {source:?} => {:?}", parsed.into_ast()),
                Err(_) => println!("{tag} parts err {source:?} => {}", failure(source)),
            }
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
    println!("precedence-ok");
}

/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<recursa::Span>) -> String {
    span.map_or_else(|| "-".to_owned(), |span| format!("{}..{}", span.start(), span.end()))
}
