//! Drives the precedence grammar and prints the sole LR parser's behavior with the authored
//! `precedence` declaration:
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

/// Positive inputs the parser accepts, one or more per gram.y family.
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

/// Inputs the parser rejects.
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

/// Inputs whose LR action is decided by the declared precedence. The integration test pins
/// the accepted or rejected result; each matches PostgreSQL's `gram.y` declaration.
const PARTING: &[&str] = &[
    // `%nonassoc ESCAPE` above `LIKE`: the escape clause is shifted, so a bare `escape`
    // before `;` is no label.
    "select a like b escape;",
    // `VALUE` at IDENT's level shifts into the `IS JSON` type.
    "select a is json value;",
    // The same shift, which then leaves the second `value` for the label.
    "select a is json value value;",
    // `%nonassoc NESTED` below `PATH`: `PATH` is shifted into the nested column, which then
    // wants its path string, so `nested path` is no pair of column names.
    "columns ( nested path );",
    // `BETWEEN` (5) below `'+'` (10): no frame bound starts with `'+'`, so the action
    // reduces `between` to a column name.
    "rows between + 1 preceding;",
    // `SESSION` below `CHARACTERISTICS`: the transaction form is shifted, so `characteristics`
    // is no assignable name after `session`.
    "set session characteristics = 1;",
];

/// Drives every input through Recursa's parser: `$tag` prefixes each printed line and `$input`
/// builds its input over a lexing result.
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
            // The first failure exercises partial-value cleanup before the checked parse.
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

/// Builds the sole generated parser input.
fn parser_input<'input>(lexed: &grammar::LexResult<'input>) -> grammar::Input<'input> {
    lexed.input()
}

fn main() {
    drive!("lr", parser_input);
    println!("precedence-ok");
}

/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<recursa::Span>) -> String {
    span.map_or_else(|| "-".to_owned(), |span| format!("{}..{}", span.start(), span.end()))
}
