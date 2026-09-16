//! Drives every container shape through both construction backends: the nested parse of the
//! `table-driven-containers` sibling and the Flat AST parse this fixture adds.
//!
//! The inputs are the sibling's, so the two fixtures cover the same grammar shapes. For each one
//! this binary prints the nested value, a structural rendering walked by hand out of the Flat AST,
//! the root span, and the pool census that proves `Value` and `Name` are pooled. It also checks
//! invariant I1 (a pooled Node's pooled descendants have smaller handles), that every child's span
//! lies inside its parent's, that both backends accept and reject exactly the same inputs with the
//! same diagnostics, and that a failed flat parse restores the cursor and frees its stores.
//!
//! Every size line is a compile-time fact about the generated layout, as in step two.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use recursa::{FlatNode, Handle, List, List1, Span};
use recursa_generated_flat_containers::grammar;
use recursa_generated_flat_containers::grammar::{
    FlatAlias, FlatArgs, FlatAst, FlatBang, FlatBlock, FlatBoxes, FlatCall, FlatCallee, FlatDefine,
    FlatErased, FlatFilter, FlatFunctionTail, FlatGuard, FlatKwAs, FlatKwIn, FlatKwOnly, FlatKwOut,
    FlatKwTemp, FlatLet, FlatList, FlatMarks, FlatMode, FlatModifiers, FlatName, FlatNumber,
    FlatOptions, FlatParam, FlatParams, FlatPrint, FlatStatement, FlatTrail, FlatTrailingVariadic,
    FlatTuple, FlatValue, FlatWith,
};

/// Counts live heap allocations so a failed parse can prove it dropped every store it opened.
struct LiveCounting;

/// Live allocation count maintained by [`LiveCounting`].
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

/// The counting allocator this fixture installs.
#[global_allocator]
static ALLOCATOR: LiveCounting = LiveCounting;

/// Positive inputs, one group per container shape; copied from the `table-driven-containers`
/// driver so both fixtures cover the same shapes.
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

/// Inputs that fail; every one must restore the cursor and leak nothing in both backends.
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

/// Prints one flat type's size, and its name, so the sizes are diffable.
macro_rules! sizes {
    ($($ty:ty),* $(,)?) => {
        $(println!("lr size {} = {}", stringify!($ty), std::mem::size_of::<$ty>());)*
    };
}

fn main() {
    print_sizes();
    for source in POSITIVE {
        drive_positive(source);
    }
    drive_without_spans();
    drive_prefixes();
    drive_entry_points();
    for source in NEGATIVE {
        drive_negative(source);
    }
    println!("flat-containers-ok");
}

/// Prints the layout facts step two established, unchanged.
fn print_sizes() {
    // Every declaration of the grammar has a flat type in the same module.
    sizes!(
        grammar::FlatStatement,
        grammar::FlatLet,
        grammar::FlatAlias,
        grammar::FlatPrint,
        grammar::FlatGuard,
        grammar::FlatCall,
        grammar::FlatArgs,
        grammar::FlatFunctionTail,
        grammar::FlatTrailingVariadic,
        grammar::FlatDefine,
        grammar::FlatParams,
        grammar::FlatParam,
        grammar::FlatMode,
        grammar::FlatWith,
        grammar::FlatBlock,
        grammar::FlatModifiers,
        grammar::FlatErased,
        grammar::FlatOptions,
        grammar::FlatKwAs,
        grammar::FlatKwIn,
        grammar::FlatKwOut,
        grammar::FlatKwOnly,
        grammar::FlatKwTemp,
        grammar::FlatTrail,
        grammar::FlatBoxes,
        grammar::FlatMarks,
        grammar::FlatFilter,
        grammar::FlatValue,
        grammar::FlatList,
        grammar::FlatTuple,
        grammar::FlatBang,
        grammar::FlatName,
        grammar::FlatCallee,
        grammar::FlatNumber,
    );
    sizes!(grammar::FlatAst<'static, grammar::FlatStatement>);

    // A flat Node is `Copy`, so a handle-shaped child never moves out of its parent.
    let copied = <grammar::FlatBang as Clone>::clone(&grammar::FlatBang);
    let _ = copied;
    assert_eq!(grammar::FlatBang, copied);

    // Every flat Node carries a grammar-unique tag, which the shared pool checks on each read.
    println!(
        "lr tags statement={} value={} name={}",
        <grammar::FlatStatement as FlatNode>::TAG,
        <grammar::FlatValue as FlatNode>::TAG,
        <grammar::FlatName as FlatNode>::TAG,
    );

    // The owner is `Send + Sync` for every root Node type, proved without instantiating one.
    grammar::__recursa_assert_flat_ast_send_sync::<grammar::FlatStatement>();

    // The design's four- and eight-byte handle promises.
    println!(
        "lr optional handle={} optional list={}",
        std::mem::size_of::<Option<Handle<grammar::FlatValue>>>(),
        std::mem::size_of::<Option<List<Handle<grammar::FlatValue>>>>(),
    );
}

/// Parses one positive input through both backends and prints what each built.
fn drive_positive(source: &str) {
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let parsed = grammar::Statement::parse(&mut input)
        .unwrap_or_else(|error| panic!("positive input {source:?} failed: {error}"));
    println!(
        "lr ok {source:?} => {:?} cursor={} eof={}",
        parsed.into_ast(),
        input.cursor(),
        input.is_eof()
    );
    assert!(input.is_eof(), "a positive input is consumed whole");

    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Statement::parse_flat(&mut input)
        .unwrap_or_else(|error| panic!("positive input {source:?} failed flat: {error}"));
    assert!(input.is_eof(), "a positive flat input is consumed whole");
    println!(
        "lr flat {source:?} => {} cursor={}",
        statement(&flat, flat.root()),
        input.cursor()
    );
    let root = flat.span(flat.root());
    println!("lr flat-spans {source:?} => {}", span_text(root));
    assert!(
        flat.has_spans(),
        "this fixture enables the `spans` feature"
    );
    assert_eq!(
        root,
        Span::new(0, source.trim_end().len()).ok(),
        "the root of a fully consumed input spans the whole input: {source:?}"
    );
    let census = census(&flat, flat.root());
    println!(
        "lr flat-pool {source:?} => values={} names={}",
        census.values, census.names
    );
}

/// Parses one negative input through both backends and prints the one error line both produce.
fn drive_negative(source: &str) {
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    // Take the live count after a warm failure so it covers only the measured parse, and take it
    // before any rendering, which would allocate a string of its own.
    drop(grammar::Statement::parse(&mut input).expect_err("failing input fails"));
    let baseline = LIVE.load(Ordering::SeqCst);
    let error = grammar::Statement::parse(&mut input).expect_err("failing input fails");
    let reported = Reported::of(&error, input.cursor());
    drop(error);
    assert_eq!(
        LIVE.load(Ordering::SeqCst) - baseline,
        0,
        "a failed nested parse must drop every partial value"
    );
    assert_eq!(input.cursor(), 0, "a failed parse restores the cursor");
    let nested = reported.line();

    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    drop(flat_error(&mut input, source));
    let baseline = LIVE.load(Ordering::SeqCst);
    let error = flat_error(&mut input, source);
    let reported = Reported::of(&error, input.cursor());
    let span = error.span();
    drop(error);
    assert_eq!(
        LIVE.load(Ordering::SeqCst) - baseline,
        0,
        "a failed flat parse must drop every store it opened"
    );
    assert_eq!(
        input.cursor(),
        0,
        "a failed flat parse restores the cursor: {source:?}"
    );
    let flat = reported.line();
    assert_eq!(nested, flat, "both backends reject {source:?} alike");
    // The restored cursor is usable: the same input fails again at the same place.
    let again = flat_error(&mut input, source);
    assert_eq!(again.span(), span, "the retry repeats the error");
    assert_eq!(input.cursor(), 0, "the retry restores the cursor too");
    println!("lr err {source:?} => {flat}");
}

/// Parses without spans and proves the result retains none.
fn drive_without_spans() {
    let source = "print [1, [2]];";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Statement::parse_flat_without_spans(&mut input)
        .expect("the input parses without spans");
    println!(
        "lr flat-nospans {source:?} => {} has_spans={} root={}",
        statement(&flat, flat.root()),
        flat.has_spans(),
        span_text(flat.span(flat.root()))
    );
    assert!(!flat.has_spans());
    assert!(flat.span(flat.root()).is_none());
}

/// Parses a prefix with both backends and proves they stop at the same token.
fn drive_prefixes() {
    let source = "f(1) ; {} print 2;";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let nested = grammar::Statement::parse(&mut input).expect("the first root parses");
    let nested_cursor = input.cursor();
    println!(
        "lr prefix nested {:?} cursor={nested_cursor} eof={}",
        nested.into_ast(),
        input.is_eof()
    );
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Statement::parse_flat(&mut input).expect("the first flat root parses");
    println!(
        "lr prefix flat {} cursor={} eof={}",
        statement(&flat, flat.root()),
        input.cursor(),
        input.is_eof()
    );
    assert_eq!(
        input.cursor(),
        nested_cursor,
        "both backends accept the same prefix"
    );
    // The rest of the input still parses, root after root, from the same cursor.
    let second = grammar::Statement::parse_flat(&mut input).expect("the second flat root parses");
    println!(
        "lr prefix flat {} cursor={}",
        statement(&second, second.root()),
        input.cursor()
    );
}

/// Parses from the entry points of Nodes other than the grammar's root.
fn drive_entry_points() {
    let lexed = grammar::lex("[1, [as, only]]");
    let mut input = lexed.input();
    let parsed = grammar::Value::parse_flat(&mut input).expect("a value parses from its own entry");
    println!(
        "lr entry value {} root={}",
        value(&parsed, parsed.root()),
        span_text(parsed.span(parsed.root()))
    );

    let lexed = grammar::lex("(in x, out temp)");
    let mut input = lexed.input();
    let parsed = grammar::Params::parse_flat(&mut input).expect("a parameter list parses");
    println!("lr entry params {}", params(&parsed, parsed.root()));

    let lexed = grammar::lex("only");
    let mut input = lexed.input();
    let parsed =
        grammar::Name::parse_flat(&mut input).expect("an admitted keyword parses as a name");
    println!("lr entry name {}", name(&parsed, parsed.root()));

    let lexed = grammar::lex("as x");
    let mut input = lexed.input();
    let parsed = grammar::Alias::parse_flat(&mut input).expect("an alias parses from its own entry");
    println!("lr entry alias {}", alias(&parsed, parsed.root()));
}

/// Runs one failing flat parse and returns its error.
///
/// `Result::expect_err` needs the success type to be `Debug`, and a Flat AST is not, so the
/// failure is taken by pattern instead.
fn flat_error(
    input: &mut grammar::Input<'_>,
    source: &str,
) -> recursa::ParseError {
    match grammar::Statement::parse_flat(input) {
        Ok(_) => panic!("failing input {source:?} parsed flat"),
        Err(error) => error,
    }
}

/// One failure as both backends must report it, held without allocating.
///
/// Every field is `Copy`, so the failure can be measured for leaked allocations before it is
/// rendered.
#[derive(Clone, Copy)]
struct Reported {
    /// The diagnostic code.
    code: &'static str,
    /// The failure kind.
    kind: recursa::ParseErrorKind,
    /// The failing span.
    span: Span,
    /// The token the automaton could not act on.
    found: Option<&'static str>,
    /// The input cursor after the failure.
    cursor: usize,
}

impl Reported {
    /// Reads one error's reportable facts.
    fn of(error: &recursa::ParseError, cursor: usize) -> Self {
        Self {
            code: error.code(),
            kind: error.kind(),
            span: error.span(),
            found: error.found(),
            cursor,
        }
    }

    /// Renders the line both backends must produce.
    fn line(self) -> String {
        format!(
            "code={} kind={:?} span={:?} found={:?} cursor={}",
            self.code, self.kind, self.span, self.found, self.cursor,
        )
    }
}

/// Renders an optional span as `start..end` or `-`.
fn span_text(span: Option<Span>) -> String {
    span.map_or_else(
        || "-".to_owned(),
        |span| format!("{}..{}", span.start(), span.end()),
    )
}

// --- the structural rendering, walked by hand over this grammar's flat types -------------------
//
// Every function takes the result as `FlatAst<'_, R>` for any root Node `R`, because the
// generated storage implementations do not depend on which Node the parse started from. A handle
// is indexed to borrow the Node it names, a list handle to borrow its run of handles, and a text
// handle to borrow the text the pool owns.

/// Renders one optional child through `render`.
fn optional<T>(child: Option<T>, render: impl FnOnce(T) -> String) -> String {
    child.map_or_else(|| "-".to_owned(), render)
}

/// Renders one run of handles through `render`.
fn run<T: Copy>(items: &[T], render: impl Fn(T) -> String) -> String {
    let rendered = items.iter().copied().map(render).collect::<Vec<_>>();
    format!("[{}]", rendered.join(" "))
}

/// Renders one statement, which is this grammar's root.
fn statement<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatStatement>) -> String {
    match ast[handle] {
        FlatStatement::Let(inner) => let_(ast, inner),
        FlatStatement::Print(inner) => print(ast, inner),
        FlatStatement::Call(inner) => call(ast, inner),
        FlatStatement::Define(inner) => define(ast, inner),
        FlatStatement::With(inner) => with(ast, inner),
        FlatStatement::Block(inner) => block(ast, inner),
        FlatStatement::Modifiers(inner) => modifiers(ast, inner),
        FlatStatement::Erased(inner) => erased(ast, inner),
        FlatStatement::Options(inner) => options(ast, inner),
        FlatStatement::Trail(inner) => trail(ast, inner),
        FlatStatement::Boxes(inner) => boxes(ast, inner),
        FlatStatement::Marks(inner) => marks(ast, inner),
        FlatStatement::Filter(inner) => filter(ast, inner),
        FlatStatement::FunctionTail(inner) => function_tail(ast, inner),
    }
}

/// Renders a `Let`: an envelope presence, a boxed value and an inlined optional alias.
fn let_<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatLet>) -> String {
    let node = ast[handle];
    format!(
        "Let{{unless={} name={} value={} alias={}}}",
        node.unless,
        name(ast, node.name),
        value(ast, node.value),
        optional(node.alias, |inner| alias(ast, inner)),
    )
}

/// Renders an `Alias`.
fn alias<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatAlias>) -> String {
    format!("Alias{{name={}}}", name(ast, ast[handle].name))
}

/// Renders a `Print`: a bare presence and a helper optional guard.
fn print<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatPrint>) -> String {
    let node = ast[handle];
    format!(
        "Print{{negated={} value={} guard={}}}",
        node.negated,
        value(ast, node.value),
        optional(node.guard, |inner| guard(ast, inner)),
    )
}

/// Renders a `Guard`.
fn guard<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatGuard>) -> String {
    format!("Guard{{condition={}}}", value(ast, ast[handle].condition))
}

/// Renders a `Call`.
fn call<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatCall>) -> String {
    let node = ast[handle];
    format!(
        "Call{{callee={} args={}}}",
        callee(ast, node.callee),
        args(ast, node.args),
    )
}

/// Renders an `Args`: a zero-or-more list with a trailing separator.
fn args<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatArgs>) -> String {
    let values = ast[handle].values;
    format!(
        "Args{{values={}}}",
        run(&ast[values], |item| value(ast, item))
    )
}

/// Renders a `FunctionTail`: a one-or-more list and an optional tail that owns the same comma.
fn function_tail<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatFunctionTail>) -> String {
    let node = ast[handle];
    format!(
        "FunctionTail{{values={} trailing={}}}",
        run(&ast[node.values], |item| value(ast, item)),
        optional(node.trailing_variadic, |inner| trailing_variadic(ast, inner)),
    )
}

/// Renders a `TrailingVariadic`.
fn trailing_variadic<R: FlatNode>(
    ast: &FlatAst<'_, R>,
    handle: Handle<FlatTrailingVariadic>,
) -> String {
    format!(
        "TrailingVariadic{{value={}}}",
        value(ast, ast[handle].value)
    )
}

/// Renders a `Define`: a parameter list and a helper optional boxed value.
fn define<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatDefine>) -> String {
    let node = ast[handle];
    format!(
        "Define{{name={} params={} returns={}}}",
        name(ast, node.name),
        params(ast, node.params),
        optional(node.returns, |inner| value(ast, inner)),
    )
}

/// Renders a `Params`: a one-or-more list with a separator.
fn params<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatParams>) -> String {
    let list = ast[handle].params;
    format!("Params{}", run(&ast[list], |item| param(ast, item)))
}

/// Renders a `Param`: an inlined optional mode.
fn param<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatParam>) -> String {
    let node = ast[handle];
    format!(
        "Param{{mode={} name={}}}",
        optional(node.mode, |inner| mode(ast, inner)),
        name(ast, node.name),
    )
}

/// Renders a `Mode`, a fixed-token-only enum.
fn mode<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatMode>) -> String {
    match ast[handle] {
        FlatMode::In => "In".to_owned(),
        FlatMode::Out => "Out".to_owned(),
    }
}

/// Renders a `With`: an inlined presence and an inlined one-or-more list.
fn with<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatWith>) -> String {
    let node = ast[handle];
    format!(
        "With{{recursive={} names={}}}",
        node.recursive,
        run(&ast[node.names], |item| name(ast, item)),
    )
}

/// Renders a `Block`: an inlined zero-or-more list without a separator, recursive through
/// `Statement`.
fn block<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatBlock>) -> String {
    let list = ast[handle].statements;
    format!("Block{}", run(&ast[list], |item| statement(ast, item)))
}

/// Renders a `Modifiers`: three presences, two of them behind helpers.
fn modifiers<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatModifiers>) -> String {
    let node = ast[handle];
    format!(
        "Modifiers{{four={} five={} six={} name={}}}",
        node.four,
        node.five,
        node.six,
        name(ast, node.name),
    )
}

/// Renders an `Erased`, whose five optional groups leave no value behind.
fn erased<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatErased>) -> String {
    format!("Erased{{name={}}}", name(ast, ast[handle].name))
}

/// Renders an `Options`: five optionals of fixed-token unit Nodes, which the parent allocates.
fn options<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatOptions>) -> String {
    let node = ast[handle];
    format!(
        "Options{{a={} b={} c={} d={} e={}}}",
        optional(node.a, |inner| kw_as(ast, inner)),
        optional(node.b, |inner| kw_in(ast, inner)),
        optional(node.c, |inner| kw_out(ast, inner)),
        optional(node.d, |inner| kw_only(ast, inner)),
        optional(node.e, |inner| kw_temp(ast, inner)),
    )
}

/// Renders the `as` keyword Node, whose flat form is zero-size.
fn kw_as<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatKwAs>) -> String {
    assert_eq!(ast[handle], FlatKwAs);
    "KwAs".to_owned()
}

/// Renders the `in` keyword Node.
fn kw_in<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatKwIn>) -> String {
    assert_eq!(ast[handle], FlatKwIn);
    "KwIn".to_owned()
}

/// Renders the `out` keyword Node.
fn kw_out<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatKwOut>) -> String {
    assert_eq!(ast[handle], FlatKwOut);
    "KwOut".to_owned()
}

/// Renders the `only` keyword Node.
fn kw_only<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatKwOnly>) -> String {
    assert_eq!(ast[handle], FlatKwOnly);
    "KwOnly".to_owned()
}

/// Renders the `temp` keyword Node.
fn kw_temp<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatKwTemp>) -> String {
    assert_eq!(ast[handle], FlatKwTemp);
    "KwTemp".to_owned()
}

/// Renders a `Trail`: a one-or-more list behind a trailing-separator helper.
fn trail<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatTrail>) -> String {
    let list = ast[handle].names;
    format!("Trail{}", run(&ast[list], |item| name(ast, item)))
}

/// Renders a `Boxes`: an inlined zero-or-more list whose `boxed!` layer the flat form drops.
fn boxes<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatBoxes>) -> String {
    let list = ast[handle].values;
    format!("Boxes{}", run(&ast[list], |item| value(ast, item)))
}

/// Renders a `Marks`: a helper zero-or-more list and a helper optional one-or-more list.
fn marks<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatMarks>) -> String {
    let node = ast[handle];
    format!(
        "Marks{{marks={} tail={}}}",
        run(&ast[node.marks], |item| bang(ast, item)),
        optional(node.tail, |list: List1<Handle<FlatBang>>| run(
            &ast[list],
            |item| bang(ast, item)
        )),
    )
}

/// Renders a `Filter`: a one-or-more list and a repeated attachment with an erased group inside.
fn filter<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatFilter>) -> String {
    let node = ast[handle];
    format!(
        "Filter{{bangs={} filters={}}}",
        run(&ast[node.bangs], |item| bang(ast, item)),
        run(&ast[node.filters], |item| value(ast, item)),
    )
}

/// Renders a `Value`, the pooled enum.
fn value<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatValue>) -> String {
    match ast[handle] {
        FlatValue::Name { name: inner } => format!("Value::Name({})", name(ast, inner)),
        FlatValue::Number(inner) => format!("Value::Number({})", number(ast, inner)),
        FlatValue::Negated { mark, inner } => format!(
            "Value::Negated({} {})",
            bang(ast, mark),
            value(ast, inner)
        ),
        FlatValue::List(inner) => format!("Value::{}", list(ast, inner)),
        FlatValue::Tuple(inner) => format!("Value::{}", tuple(ast, inner)),
    }
}

/// Renders a `List`: an inlined optional one-or-more list.
fn list<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatList>) -> String {
    let items = ast[handle].items;
    format!(
        "List{{items={}}}",
        optional(items, |items: List1<Handle<FlatValue>>| run(
            &ast[items],
            |item| value(ast, item)
        )),
    )
}

/// Renders a `Tuple`: a one-or-more list whose every element carries its own attachment.
fn tuple<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatTuple>) -> String {
    let cells = ast[handle].cells;
    format!("Tuple{}", run(&ast[cells], |item| value(ast, item)))
}

/// Renders a `Bang`, a fixed-token unit Node.
fn bang<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatBang>) -> String {
    assert_eq!(ast[handle], FlatBang);
    "Bang".to_owned()
}

/// Renders a `Name`, whose text the text pool owns and which borrows the input.
fn name<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatName>) -> String {
    let text = ast[handle].text;
    format!("Name({:?})", ast[text].as_str())
}

/// Renders a `Callee`.
fn callee<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatCallee>) -> String {
    let text = ast[handle].text;
    format!("Callee({:?})", ast[text].as_str())
}

/// Renders a `Number`.
fn number<R: FlatNode>(ast: &FlatAst<'_, R>, handle: Handle<FlatNumber>) -> String {
    let text = ast[handle].text;
    format!("Number({:?})", ast[text].as_str())
}

// --- the pool census, invariant I1 and span containment ---------------------------------------

/// The number of Nodes each typed pool of one result holds.
struct Census {
    /// Nodes in the `FlatValue` pool.
    values: usize,
    /// Nodes in the `FlatName` pool.
    names: usize,
}

/// Counts each typed pool of one result and checks invariant I1 and span containment over it.
///
/// A pooled handle's raw value is its pool index plus one, so a pool of `n` Nodes hands out
/// exactly the handles `1..=n`. Collecting every reachable handle and finding that dense set is
/// therefore both the census and the proof that the Node is pooled rather than shared: a
/// shared-pool Node's handle is a word offset and leaves gaps.
fn census<R: FlatNode>(ast: &FlatAst<'_, R>, root: Handle<FlatStatement>) -> Census {
    let mut values = Vec::new();
    let mut names = Vec::new();
    collect(ast, root, &mut values, &mut names);
    Census {
        values: dense(&mut values, "FlatValue"),
        names: dense(&mut names, "FlatName"),
    }
}

/// Sorts `handles`, checks that they are the dense pool range `1..=len`, and returns the length.
fn dense(handles: &mut Vec<u32>, pool: &str) -> usize {
    handles.sort_unstable();
    handles.dedup();
    for (index, raw) in handles.iter().enumerate() {
        assert_eq!(
            *raw,
            index as u32 + 1,
            "{pool} is a typed pool, so its handles are the dense range 1..=n: {handles:?}"
        );
    }
    handles.len()
}

/// Collects every pooled handle reachable from one statement.
///
/// The walk descends through the shared-pool Nodes between two pooled ones, so a `Value` inside a
/// `List` inside a `Value` is found.
fn collect<R: FlatNode>(
    ast: &FlatAst<'_, R>,
    handle: Handle<FlatStatement>,
    values: &mut Vec<u32>,
    names: &mut Vec<u32>,
) {
    let value = |handle: Handle<FlatValue>, values: &mut Vec<u32>, names: &mut Vec<u32>| {
        collect_value(ast, handle, values, names);
    };
    match ast[handle] {
        FlatStatement::Let(inner) => {
            let node = ast[inner];
            names.push(ast[inner].name.raw().get());
            value(node.value, values, names);
            if let Some(alias) = node.alias {
                names.push(ast[alias].name.raw().get());
            }
        }
        FlatStatement::Print(inner) => {
            let node = ast[inner];
            value(node.value, values, names);
            if let Some(guard) = node.guard {
                value(ast[guard].condition, values, names);
            }
        }
        FlatStatement::Call(inner) => {
            let list = ast[ast[inner].args].values;
            for item in ast[list].to_vec() {
                value(item, values, names);
            }
        }
        FlatStatement::Define(inner) => {
            let node = ast[inner];
            names.push(node.name.raw().get());
            for item in ast[ast[node.params].params].to_vec() {
                names.push(ast[item].name.raw().get());
            }
            if let Some(returns) = node.returns {
                value(returns, values, names);
            }
        }
        FlatStatement::With(inner) => {
            for item in ast[ast[inner].names].to_vec() {
                names.push(item.raw().get());
            }
        }
        FlatStatement::Block(inner) => {
            for item in ast[ast[inner].statements].to_vec() {
                collect(ast, item, values, names);
            }
        }
        FlatStatement::Modifiers(inner) => names.push(ast[inner].name.raw().get()),
        FlatStatement::Erased(inner) => names.push(ast[inner].name.raw().get()),
        FlatStatement::Options(_) => {}
        FlatStatement::Trail(inner) => {
            for item in ast[ast[inner].names].to_vec() {
                names.push(item.raw().get());
            }
        }
        FlatStatement::Boxes(inner) => {
            for item in ast[ast[inner].values].to_vec() {
                value(item, values, names);
            }
        }
        FlatStatement::Marks(_) => {}
        FlatStatement::Filter(inner) => {
            for item in ast[ast[inner].filters].to_vec() {
                value(item, values, names);
            }
        }
        FlatStatement::FunctionTail(inner) => {
            let node = ast[inner];
            for item in ast[node.values].to_vec() {
                value(item, values, names);
            }
            if let Some(tail) = node.trailing_variadic {
                value(ast[tail].value, values, names);
            }
        }
    }
}

/// Collects one `Value` subtree and checks invariant I1 and span containment for it.
fn collect_value<R: FlatNode>(
    ast: &FlatAst<'_, R>,
    handle: Handle<FlatValue>,
    values: &mut Vec<u32>,
    names: &mut Vec<u32>,
) {
    values.push(handle.raw().get());
    let children: Vec<Handle<FlatValue>> = match ast[handle] {
        FlatValue::Name { name } => {
            names.push(name.raw().get());
            Vec::new()
        }
        FlatValue::Number(_) => Vec::new(),
        FlatValue::Negated { inner, .. } => vec![inner],
        FlatValue::List(inner) => match ast[inner].items {
            Some(items) => ast[items].to_vec(),
            None => Vec::new(),
        },
        FlatValue::Tuple(inner) => ast[ast[inner].cells].to_vec(),
    };
    for child in children {
        // Invariant I1: a pooled Node is reduced after its children, so a same-pool child always
        // has a smaller handle.
        assert!(
            child.raw() < handle.raw(),
            "I1: a pooled child must precede its parent, {child:?} under {handle:?}"
        );
        assert!(
            contains(ast.span(handle), ast.span(child)),
            "a child's span lies inside its parent's, {child:?} under {handle:?}"
        );
        collect_value(ast, child, values, names);
    }
}

/// Reports whether `outer` contains `inner`; two absent spans contain each other.
fn contains(outer: Option<Span>, inner: Option<Span>) -> bool {
    match (outer, inner) {
        (Some(outer), Some(inner)) => outer.start() <= inner.start() && inner.end() <= outer.end(),
        (_, None) => true,
        (None, Some(_)) => false,
    }
}
