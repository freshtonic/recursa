//! Drives every container shape through both renderers: the Pretty implementation of a detached
//! nested AST and the flat Pretty implementation this fixture adds.
//!
//! The inputs are the `flat-containers` sibling's, so both fixtures cover the same grammar shapes.
//! For each one this binary parses twice, prints the nested render and the flat render, and
//! asserts that the two are the same string. That equality is the milestone's contract: a Flat AST
//! renders exactly what a detached nested value of the same Node renders, canonical fixed-token
//! spellings and all, because neither representation carries occurrence provenance.
//!
//! It then proves the rendering round trip of [ADR 0008]: the flat render reparses, into the same
//! flat document (its second render is the first) and into the same nested semantic tree (its
//! `Debug` is the first's). Finally it drives `render_with` under a narrow width, `pretty_node`
//! from an interior handle, and the entry points of Nodes other than the grammar root.
//!
//! [ADR 0008]: ../../../../../../docs/adr/0008-keep-flat-asts-semantic-only.md

use recursa::{Pretty as _, PrettyConfig, PrettyContext};
use recursa_generated_flat_pretty::grammar;
use recursa_generated_flat_pretty::grammar::{FlatStatement, FlatValue};

/// Positive inputs, one group per container shape; copied from the `flat-containers` driver so
/// both fixtures cover the same shapes.
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
    // FunctionTail: the comma before `variadic` belongs to the optional tail, not the list.
    "vtail 1, 2, variadic 3;",
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
    // Value shapes through Print: inlined Option<Vec1<Value>>, per-element attachment, and a
    // list inside a list item, which is the case that makes the scratch stack nest.
    "print [];",
    "print [[], [1, [2]]];",
    "print [[1, [2, [3]]], []];",
    "print (1);",
    "print (1)|(as)|([]);",
    "print !(1)|(2);",
];

/// The input whose render breaks under a narrow width.
const WIDE: &str = "{ print 1; { let if exists x = 2; } f(); }";

fn main() {
    for source in POSITIVE {
        drive(source);
    }
    drive_width();
    drive_interior_handle();
    drive_entry_points();
    println!("flat-pretty-ok");
}

/// Renders one positive input through both representations and proves they agree.
fn drive(source: &str) {
    let nested = nested_render(source);
    println!("pretty {source:?} => {}", nested.rendered);
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Statement::parse_flat(&mut input)
        .unwrap_or_else(|error| panic!("positive input {source:?} failed flat: {error}"));
    assert!(input.is_eof(), "a positive flat input is consumed whole");
    let rendered = flat.render();
    println!("flat-pretty {source:?} => {rendered}");
    assert_eq!(
        rendered, nested.rendered,
        "a Flat AST renders what a detached nested AST renders: {source:?}",
    );

    // The rendered text reparses into the same flat document. Its render is the first render,
    // which is the rendering half of the round trip, and its nested reparse has the same `Debug`
    // as the original tree, which is the semantic half.
    let lexed = grammar::lex(&rendered);
    let mut input = lexed.input();
    let again = grammar::Statement::parse_flat(&mut input)
        .unwrap_or_else(|error| panic!("the flat render of {source:?} failed to reparse: {error}"));
    assert!(input.is_eof(), "a rendered statement is consumed whole");
    assert_eq!(
        again.render(),
        rendered,
        "flat rendering is idempotent: {source:?}",
    );
    let reparsed = nested_render(&rendered);
    assert_eq!(
        reparsed.debug, nested.debug,
        "the flat render reparses to the same semantic tree: {source:?}",
    );
}

/// One detached nested parse: what it renders and what semantic tree it is.
struct Nested {
    /// The render of the detached value, with no occurrence provenance.
    rendered: String,
    /// The `Debug` of the same value, which stands for its semantic identity.
    debug: String,
}

/// Parses one input and detaches its nested AST, as `Parsed::into_ast` does.
fn nested_render(source: &str) -> Nested {
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let parsed = grammar::Statement::parse(&mut input)
        .unwrap_or_else(|error| panic!("positive input {source:?} failed: {error}"));
    assert!(input.is_eof(), "a positive input is consumed whole");
    let ast = parsed.into_ast();
    Nested {
        rendered: ast.render(),
        debug: format!("{ast:?}"),
    }
}

/// Renders one input under a caller-selected width and proves the width is honored.
fn drive_width() {
    let config = PrettyConfig::new(16, 2);
    let lexed = grammar::lex(WIDE);
    let mut input = lexed.input();
    let flat = grammar::Statement::parse_flat(&mut input).expect("the wide input parses flat");
    let narrow = flat.render_with(config);
    println!("flat-pretty-width {WIDE:?} => {narrow:?}");
    assert!(
        narrow.contains('\n'),
        "a narrow width breaks the rendered lines: {narrow:?}",
    );
    assert!(
        !flat.render().contains('\n'),
        "the grammar's own width keeps this input on one line",
    );
    let lexed = grammar::lex(WIDE);
    let mut input = lexed.input();
    let nested = grammar::Statement::parse(&mut input).expect("the wide input parses");
    assert_eq!(
        narrow,
        nested.into_ast().render_with(config),
        "both representations honor one `PrettyConfig`",
    );
    // A broken render still reparses, which is what makes the width a layout choice rather than a
    // change of meaning.
    let lexed = grammar::lex(&narrow);
    let mut input = lexed.input();
    let reparsed = grammar::Statement::parse_flat(&mut input).expect("a broken render reparses");
    assert_eq!(reparsed.render(), flat.render());
}

/// Renders one interior Node out of a larger result through `pretty_node`.
fn drive_interior_handle() {
    let source = "print [1, [as, only]];";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Statement::parse_flat(&mut input).expect("the input parses flat");
    let FlatStatement::Print(print) = flat[flat.root()] else {
        panic!("this input is a print statement")
    };
    let value = flat[print].value;
    let mut context = PrettyContext::new();
    flat.pretty_node(value, &mut context)
        .expect("rendering into a structured sink cannot fail");
    println!("flat-pretty-node {source:?} => {}", context.finish());
    // The same Node, reached one level deeper, renders on its own too.
    let FlatValue::List(list) = flat[value] else {
        panic!("this value is a list")
    };
    let items = flat[list].items.expect("the list has items");
    let mut context = PrettyContext::new();
    flat.pretty_node(flat[items][1], &mut context)
        .expect("rendering into a structured sink cannot fail");
    println!("flat-pretty-node inner => {}", context.finish());
}

/// Renders Flat ASTs rooted at Nodes other than the grammar's root.
fn drive_entry_points() {
    let lexed = grammar::lex("[1, [as, only]]");
    let mut input = lexed.input();
    let value = grammar::Value::parse_flat(&mut input).expect("a value parses from its own entry");
    println!("flat-pretty-entry value => {}", value.render());

    let lexed = grammar::lex("(in x, out temp)");
    let mut input = lexed.input();
    let params = grammar::Params::parse_flat(&mut input).expect("a parameter list parses");
    println!("flat-pretty-entry params => {}", params.render());

    let lexed = grammar::lex("only");
    let mut input = lexed.input();
    let name = grammar::Name::parse_flat(&mut input).expect("an admitted keyword parses as a name");
    println!("flat-pretty-entry name => {}", name.render());
}
