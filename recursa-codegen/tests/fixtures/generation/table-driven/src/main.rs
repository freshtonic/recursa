//! Drives the tracer grammar and prints what the test compares between the two parser styles:
//! the Debug value of every positive parse with the cursor it leaves, where a root parse stops
//! when tokens follow it, and for every failing input the error with its expected set, the
//! restored cursor and the number of heap allocations still live after the error is dropped.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use recursa_generated_table_driven::grammar;

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

fn main() {
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
        let mut input = lexed.input();
        let parsed = grammar::Statement::parse(&mut input).expect("positive input parses");
        println!(
            "ok {source:?} => {:?} cursor={} eof={}",
            parsed.into_ast(),
            input.cursor(),
            input.is_eof()
        );
    }

    let source = "let x = 5; print(x); true";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let first = grammar::Statement::parse(&mut input).expect("first root parses");
    println!("prefix first {:?} cursor={}", first.into_ast(), input.cursor());
    let second = grammar::Statement::parse(&mut input).expect("second root parses");
    println!(
        "prefix second {:?} cursor={} eof={}",
        second.into_ast(),
        input.cursor(),
        input.is_eof()
    );
    let third = grammar::Statement::parse(&mut input).expect_err("a bare `true` is no statement");
    println!(
        "prefix third code={} found={:?} cursor={}",
        third.code(),
        third.found(),
        input.cursor()
    );

    let source = "!!x";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let value = grammar::Value::parse(&mut input).expect("a value parses from its own entry");
    println!("entry {:?} cursor={}", value.into_ast(), input.cursor());
    let source = "count";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let name = grammar::Name::parse(&mut input).expect("a content token parses from its own entry");
    println!("entry {:?} cursor={}", name.into_ast(), input.cursor());

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
        let mut input = lexed.input();
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
            "err {source:?} => code={code} kind={kind:?} span={span:?} found={found:?} cursor={} leaked={leaked} expected={expected:?}",
            input.cursor()
        );
        assert_eq!(leaked, 0, "a failed parse must drop every partial value");
        assert_eq!(input.cursor(), 0, "a failed parse restores the cursor");
    }
    println!("table-driven-ok");
}
