//! Drives the JSON example grammar (copied from `examples/json` by the test) and prints what the
//! test checks through the sole LR parser: a structural rendering of every positive parse
//! (the example derives no `Debug`) with the cursor it leaves, where a root parse stops when tokens follow it, the value of every
//! entry point, and for every failing input the error, the restored cursor and the number of
//! heap allocations still live after the error is dropped.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use recursa::{HasSpan as _, NodeView as _, Pretty as _};
use recursa_generated_table_driven_json::grammar;

/// Counts live heap allocations so a failed parse can prove it dropped every partial vector.
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

/// Renders a value structurally: containers with their cardinality, literals with their text.
fn render(value: &grammar::Value<'_>) -> String {
    match value {
        grammar::Value::Object(object) => format!("object{{{}}}", render_members(object)),
        grammar::Value::Array(array) => match &array.values {
            None => "array(none)".to_owned(),
            Some(values) => format!(
                "array[{}]",
                values.iter().map(render).collect::<Vec<_>>().join(" ")
            ),
        },
        grammar::Value::String(text) => format!("string({})", text.text()),
        grammar::Value::Number(text) => format!("number({})", text.text()),
        grammar::Value::Bool(boolean) => format!("bool({})", render_bool(boolean)),
        grammar::Value::Null => "null".to_owned(),
    }
}

/// Renders an object's members as `key: value` pairs.
fn render_members(object: &grammar::Object<'_>) -> String {
    object
        .members
        .iter()
        .map(render_member)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders one member as `key: value`.
fn render_member(member: &grammar::Member<'_>) -> String {
    format!("{}: {}", member.key.text(), render(&member.value))
}

/// Renders a boolean literal.
fn render_bool(boolean: &grammar::Bool) -> &'static str {
    match boolean {
        grammar::Bool::True => "true",
        grammar::Bool::False => "false",
    }
}

/// Positive inputs: the example's own test inputs and a few deeper shapes.
const POSITIVE: &[&str] = &[
    r#"{"answer":[true,null,42]}"#,
    "{}",
    "[]",
    r#"["text",-12.5e+2]"#,
    r#"{"a":{"b":[[],{}]},"c":"d"}"#,
    "null",
    "true",
    "false",
    r#""str\"qé""#,
    "-0",
    "[1,2,3]",
    r#"{"k":1,"k":2}"#,
    r#" [ { "x" : [ ] } ] "#,
    "[[[[1]]]]",
    r#"{"answer":[true,null,{"nested":"value"}]}"#,
];

/// Inputs that fail; every one must restore the cursor and leak nothing.
const NEGATIVE: &[&str] = &[
    "",
    "{",
    "[1,]",
    r#"{"a"}"#,
    "[,]",
    r#"{"a":}"#,
    "]",
    r#"{"a":1,}"#,
    "[1 2]",
    r#"{"a" 1}"#,
    "{1:2}",
    "[",
    "@",
    r#"{"a":[1,{"b":}]}"#,
    "[1,[2,[3,]]]",
];

/// Drives every input through one parser: `$tag` prefixes each printed line and `$input`
/// builds that parser's input over a lexing result. The test compares the lines of the two
/// parsers of the one generation.
macro_rules! drive {
    ($tag:expr, $input:expr) => {{
        let tag: &str = $tag;
        // Prints the provenance of one positive input parsed with capture requested: the
        // occurrence tree, the root's elements and the Pretty rendering.
        let provenance = |source: &str| {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Value::parse_spanned(&mut input).expect("positive input parses with capture");
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
            let parsed = grammar::Value::parse(&mut input)
                .unwrap_or_else(|error| panic!("positive input {source:?} failed: {error}"));
            println!(
                "{tag} ok {source:?} => {} cursor={} eof={}",
                render(&parsed.into_ast()),
                input.cursor(),
                input.is_eof()
            );
            assert!(input.is_eof(), "a positive input is consumed whole");
            provenance(source);
        }

        // Roots that end in a closing token accept their prefix. A root that ends in a keyword
        // or content token has a distinct strict-boundary behavior.
        let source = r#"{} [] {"a":[null]} [true]"#;
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        for ordinal in ["first", "second", "third", "fourth"] {
            let value = grammar::Value::parse(&mut input).expect("each root parses");
            println!("{tag} prefix {ordinal} {} cursor={} eof={}", render(&value.into_ast()), input.cursor(), input.is_eof());
        }
        let source = "[1] ,";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let value = grammar::Value::parse(&mut input).expect("the array parses");
        println!("{tag} prefix fifth {} cursor={}", render(&value.into_ast()), input.cursor());
        let Err(error) = grammar::Value::parse(&mut input) else {
            panic!("a bare comma is no value");
        };
        println!("{tag} prefix sixth code={} found={:?} cursor={}", error.code(), error.found(), input.cursor());

        // A root whose selected alternative is one token long (a keyword or a content token)
        // has a different boundary from a root ending in a closing token (#118).
        for source in ["null,", "null true", "42 ]", "\"s\" {", "true"] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            match grammar::Value::parse(&mut input) {
                Ok(value) => println!("{tag} trailing {source:?} => ok {} cursor={} eof={}", render(&value.into_ast()), input.cursor(), input.is_eof()),
                Err(error) => println!(
                    "{tag} trailing {source:?} => code={} kind={:?} span={:?} found={:?} cursor={}",
                    error.code(),
                    error.kind(),
                    error.span(),
                    error.found(),
                    input.cursor()
                ),
            }
        }

        let source = r#"{"a":1}"#;
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let object = grammar::Object::parse(&mut input).expect("an object parses from its own entry");
        println!("{tag} entry object{{{}}} cursor={}", render_members(&object.into_ast()), input.cursor());
        let source = "[1]";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let array = grammar::Array::parse(&mut input).expect("an array parses from its own entry");
        println!(
            "{tag} entry {} cursor={}",
            render(&grammar::Value::Array(array.into_ast())),
            input.cursor()
        );
        let source = r#""k":true"#;
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let member = grammar::Member::parse(&mut input).expect("a member parses from its own entry");
        println!("{tag} entry {} cursor={}", render_member(&member.into_ast()), input.cursor());
        let source = "false";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let boolean = grammar::Bool::parse(&mut input).expect("a boolean parses from its own entry");
        println!("{tag} entry {} cursor={}", render_bool(&boolean.into_ast()), input.cursor());

        for source in NEGATIVE {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            // Take the live count after a warm failure so it covers only this parse's values.
            assert!(grammar::Value::parse(&mut input).is_err(), "failing input {source:?} fails");
            let baseline = LIVE.load(Ordering::SeqCst);
            let Err(error) = grammar::Value::parse(&mut input) else {
                panic!("failing input {source:?} fails");
            };
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

/// Builds the sole generated parser input.
fn parser_input<'input>(lexed: &grammar::LexResult<'input>) -> grammar::Input<'input> {
    lexed.input()
}

fn main() {
    drive!("lr", parser_input);
    println!("table-driven-json-ok");
}

/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<recursa::Span>) -> String {
    span.map_or_else(
        || "-".to_owned(),
        |span| format!("{}..{}", span.start(), span.end()),
    )
}
