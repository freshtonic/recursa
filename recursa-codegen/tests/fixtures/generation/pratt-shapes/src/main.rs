//! Drives the Pratt shapes grammar and prints what the test checks through the sole LR parser:
//! the Debug value of every positive parse with the cursor it leaves, its provenance,
//! elements and Pretty rendering, and for every failing input the error and the restored cursor.

use recursa::{HasSpan as _, NodeView as _, Pretty as _};
use recursa_generated_pratt_shapes::grammar;

/// The inputs the parser accepts; each prints its value, cursor, provenance, elements and
/// Pretty rendering. An input that stops before its end prints `eof=false` with the cursor.
const POSITIVE: &[&str] = &[
    "a like b",
    "a like b escape c",
    "a like b escape c + d",
    "a like b + c escape d",
    "a like b escape c like d",
    "a like b escape c and d",
    "a and b like c escape d",
    "a + b like c * d escape e + f",
    "a like b not like c escape d",
    "a not like b escape c",
    "a not like b + c escape d * e",
    "a not like b like c escape d",
    "a not like b not like c escape d",
    "(a like b) escape c",
    "a like b escape c escape d",
    "@a",
    "@ a + b",
    "@ a like b",
    "@ @ a * b",
    "a ~@ b",
    "a ~@ b + c",
    "a + b ~@ c",
    "a ~@ b ~@ c",
    "a ~@ @ b",
    "a like @ b escape @ c + d",
    "@ (a like b) and c",
    "xmlnamespaces(a as b)",
    "xmlnamespaces(a + b like c as d)",
    "xmlnamespaces(default a)",
    "xmlnamespaces(default a + b)",
    "xmlnamespaces(default default)",
    "xmlnamespaces((default) as b)",
    "xmlnamespaces(default ~@ a)",
    "a like default",
    "index (a)",
    "index (a + b)",
    "index ((a))",
    "index ((a + (select b)))",
    "index (a like b)",
    "(select a) + b",
];

/// The inputs the parser rejects; each prints the error and asserts the restored cursor.
const FAILING: &[&str] = &[
    "",
    "a like",
    "a like b escape",
    "escape a",
    "a like b escape + c",
    "a like b escape (c",
    "a not like escape c",
    "a like (b escape c)",
    "@",
    "a ~@",
    "~@ like a",
    "a @ @",
    "xmlnamespaces(default a as b)",
    "xmlnamespaces(default ~@ a as b)",
    "xmlnamespaces(default)",
    "xmlnamespaces(a)",
    "index (select a)",
    "index ((select a))",
    "index (a + (select b))",
    "index (a and b)",
    "index",
];

/// Drives every input through the parser: `$tag` prefixes each printed line and `$input` builds
/// its input over a lexing result.
macro_rules! drive {
    ($tag:expr, $input:expr) => {{
        let tag: &str = $tag;
        // Prints the provenance of one positive input parsed with capture requested: the
        // occurrence tree, the root's elements and the Pretty rendering.
        let provenance = |source: &str| {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let parsed = grammar::Expr::parse_spanned(&mut input)
                .expect("positive input parses with capture");
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
            let parsed = grammar::Expr::parse(&mut input).expect("positive input parses");
            println!(
                "{tag} ok {source:?} => {:?} cursor={} eof={}",
                parsed.into_ast(),
                input.cursor(),
                input.is_eof()
            );
            provenance(source);
        }

        for source in FAILING {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let error = grammar::Expr::parse(&mut input).expect_err("failing input fails");
            println!(
                "{tag} err {source:?} => code={} kind={:?} span={:?} found={:?} cursor={}",
                error.code(),
                error.kind(),
                error.span(),
                error.found(),
                input.cursor()
            );
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
    println!("pratt-shapes-ok");
}

/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<recursa::Span>) -> String {
    span.map_or_else(
        || "-".to_owned(),
        |span| format!("{}..{}", span.start(), span.end()),
    )
}
