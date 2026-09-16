//! End-to-end proof of the tiered flat AST milestone through a generated grammar: the `flat`
//! grammar-root flag, the `#[flat(...)]` declaration attribute, one flat type per declaration, the
//! `FlatAst` owner, the layout assertions the design requires, and the flat reduce actions and
//! `parse_flat` entry points that build one.
//!
//! The fixture binary drives every container shape of the `table-driven-containers` grammar
//! through both construction backends and prints what each built; the assertions below pin the
//! structural rendering of each container shape and the facts that only flat construction can
//! show. The negative cases mutate the grammar string and assert the stable diagnostic the build
//! must produce.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// The generation fixture this test scaffolds, runs, and mutates.
const FIXTURE: &str = "tests/fixtures/generation/flat-containers";

/// The grammar-root line the negative cases replace to drop or keep the `flat` flag.
const FLAT_FLAG: &str = "    flat,\n";

#[test]
fn the_flat_containers_fixture_emits_a_flat_type_for_every_declaration_and_one_owner() {
    let package = scaffold(FIXTURE, None);
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("flat-containers-ok\n"), "{stdout}");
    let lines = parser_lines(&stdout);

    // A unit fixed-token Node carries no field at all, so its flat form is a zero-size type.
    assert!(
        lines.contains("size grammar::FlatBang = 0\n"),
        "a unit Node has no flat field\n{lines}"
    );

    // One handle per child: a struct with one Node child is four bytes.
    for expected in [
        "size grammar::FlatAlias = 4\n",
        "size grammar::FlatGuard = 4\n",
        "size grammar::FlatBlock = 8\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // A content token becomes a four-byte text handle, not inline text.
    assert!(
        lines.contains("size grammar::FlatName = 4\n"),
        "a content token becomes one text handle\n{lines}"
    );

    // `Value` declares `max_size = 16`; its widest variant holds two handles plus a tag.
    assert!(
        lines.contains("size grammar::FlatValue = 12\n"),
        "the pooled enum stays inside its declared bound\n{lines}"
    );

    // The design's handle promises, checked in the generated code and printed here.
    assert!(
        lines.contains("optional handle=4 optional list=8\n"),
        "{lines}"
    );

    // Every flat type carries a distinct tag derived from its Node key.
    let tags = lines
        .lines()
        .find(|line| line.starts_with("tags "))
        .expect("the fixture prints three tags");
    let values = tags
        .split_whitespace()
        .skip(1)
        .map(|entry| entry.split_once('=').expect("a tag entry").1)
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 3, "{tags}");
    assert!(
        values[0] != values[1] && values[1] != values[2] && values[0] != values[2],
        "{tags}"
    );
}

#[test]
fn parse_flat_builds_every_container_shape_out_of_pools_lists_and_text_handles() {
    let package = scaffold(FIXTURE, None);
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("flat-containers-ok\n"), "{stdout}");
    let lines = parser_lines(&stdout);

    // Each container shape, as the flat result holds it. The driver walks the result by hand out
    // of handles, list runs and text handles, so every line is evidence about the stores rather
    // than about a `Debug` implementation.
    for expected in [
        // An empty zero-or-more list is an empty run; an absent optional list is `None`. The two
        // are distinct values.
        "flat \"f();\" => Call{callee=Callee(\"f\") args=Args{values=[]}} cursor=4\n",
        "flat \"boxes;\" => Boxes[] cursor=2\n",
        "flat \"print [];\" => Print{negated=false value=Value::List{items=-} guard=-} cursor=4\n",
        // A one-or-more list with a separator, and the same list with a trailing separator.
        "flat \"with a | b;\" => With{recursive=false names=[Name(\"a\") Name(\"b\")]} cursor=5\n",
        "flat \"with recursive a | b |;\" => With{recursive=true names=[Name(\"a\") Name(\"b\")]} cursor=7\n",
        "flat \"trail a, b,;\" => Trail[Name(\"a\") Name(\"b\")] cursor=6\n",
        // A zero-or-more list with a trailing separator.
        "flat \"f(1, 2,);\" => Call{callee=Callee(\"f\") args=Args{values=[Value::Number(Number(\"1\")) Value::Number(Number(\"2\"))]}} cursor=8\n",
        // `Option<one_or_many!>` absent and present, both behind helper nonterminals.
        "flat \"marks :;\" => Marks{marks=[] tail=-} cursor=3\n",
        "flat \"marks : !!!;\" => Marks{marks=[] tail=[Bang Bang Bang]} cursor=6\n",
        // Presences that are true and false, inlined and behind helpers.
        "flat \"mods one three four six x;\" => Modifiers{four=true five=false six=true name=Name(\"x\")} cursor=7\n",
        "flat \"mods two five x;\" => Modifiers{four=false five=true six=false name=Name(\"x\")} cursor=5\n",
        // An envelope presence, a `boxed!` child the flat form stores as a plain handle, and an
        // inlined optional Node.
        "flat \"let one if not exists x = 2 as y;\" => Let{unless=true name=Name(\"x\") value=Value::Number(Number(\"2\")) alias=Alias{name=Name(\"y\")}} cursor=11\n",
        // Erased optional groups leave no value at all, however many of them occur.
        "flat \"erased x;\" => Erased{name=Name(\"x\")} cursor=3\n",
        "flat \"erased one two three four five x;\" => Erased{name=Name(\"x\")} cursor=8\n",
        // Fixed-token unit Nodes, which the parent allocates because they carry no value on the
        // value stack.
        "flat \"options as out temp;\" => Options{a=KwAs b=- c=KwOut d=- e=KwTemp} cursor=5\n",
        "flat \"options as in out only temp;\" => Options{a=KwAs b=KwIn c=KwOut d=KwOnly e=KwTemp} cursor=7\n",
        "flat \"filter !;\" => Filter{bangs=[Bang] filters=[]} cursor=3\n",
        // A list inside a list item: the scratch stack nests and the inner list closes first.
        "flat \"print [[], [1, [2]]];\" => Print{negated=false value=Value::List{items=[Value::List{items=-} Value::List{items=[Value::Number(Number(\"1\")) Value::List{items=[Value::Number(Number(\"2\"))]}]}]} guard=-} cursor=14\n",
        // Content tokens keep their text in the text pool, borrowed from the input, including a
        // keyword an admission set reclaims as a name.
        "flat \"let if exists as = only as temp;\" => Let{unless=false name=Name(\"as\") value=Value::Name(Name(\"only\")) alias=Alias{name=Name(\"temp\")}} cursor=9\n",
        // A recursive Node through a nested statement list.
        "flat \"{ print 1; { let if exists x = 2; } f(); }\" => Block[Print{negated=false value=Value::Number(Number(\"1\")) guard=-} Block[Let{unless=false name=Name(\"x\") value=Value::Number(Number(\"2\")) alias=-}] Call{callee=Callee(\"f\") args=Args{values=[]}}] cursor=18\n",
    ] {
        assert!(
            lines.contains(expected),
            "the flat result must render as\n{expected}\n{lines}"
        );
    }

    // `Value` and `Name` are the pooled declarations. The driver asserts that the reachable
    // handles of each are the dense range `1..=n`, which only a typed pool hands out, and prints
    // the census it counted.
    for expected in [
        "flat-pool \"print [[1, [2, [3]]], []];\" => values=8 names=0\n",
        "flat-pool \"let if exists as = only as temp;\" => values=1 names=3\n",
        "flat-pool \"options as in out only temp;\" => values=0 names=0\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // With the `spans` feature the root of a fully consumed input spans the whole input. The
    // driver also asserts that for every input, and that every child's span lies inside its
    // parent's.
    assert!(
        lines.contains("flat-spans \"print [[], [1, [2]]];\" => 0..21\n"),
        "{lines}"
    );

    // `parse_flat_without_spans` retains no span at all.
    assert!(
        lines.contains("has_spans=false root=-\n"),
        "a flat parse without spans retains no span\n{lines}"
    );

    // Prefix acceptance: an input with trailing unconsumed text stops at the same cursor in both
    // backends, which the driver asserts; these lines record the agreed cursor.
    assert!(
        lines.contains("prefix flat Call{callee=Callee(\"f\") args=Args{values=[Value::Number(Number(\"1\"))]}} cursor=5 eof=false\n"),
        "{lines}"
    );
    assert!(
        lines.contains("prefix nested Call(Call { callee: Callee { text:"),
        "{lines}"
    );

    // Every rejected input fails the same way in both backends: the driver compares the two
    // rendered error lines, asserts both restore the cursor, and parses again after the failure,
    // so one line per input is enough here.
    let failures = lines
        .lines()
        .filter(|line| line.starts_with("err "))
        .count();
    assert_eq!(failures, 24, "every rejected input reports once\n{lines}");
    assert!(
        lines.contains("err \"print [1,];\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 9, end: 10 } found=Some(\"fixed:]\") cursor=0\n"),
        "{lines}"
    );

    // Entry points other than the grammar root build a Flat AST rooted at their own Node.
    for expected in [
        "entry value Value::List{items=[Value::Number(Number(\"1\")) Value::List{items=[Value::Name(Name(\"as\")) Value::Name(Name(\"only\"))]}]} root=0..15\n",
        "entry params Params[Param{mode=In name=Name(\"x\")} Param{mode=Out name=Name(\"temp\")}]\n",
        "entry name Name(\"only\")\n",
        "entry alias Alias{name=Name(\"x\")}\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }
}

#[test]
fn the_generated_traversal_reports_every_child_in_source_order_and_proves_post_order() {
    // Step four's surface: `NodeRef`, `children`, `root_ref`, `walk`, `check_post_order`, and the
    // owner's `pool`, `lists` and `texts` accessors. The driver prints one walk line per positive
    // input, built only out of `NodeRef` names, so each line is the same tree the structural line
    // above renders and names it by Node application instead of by field.
    let package = scaffold(FIXTURE, None);
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("flat-containers-ok\n"), "{stdout}");
    let lines = parser_lines(&stdout);

    // One line per container shape, for the inputs the structural test pins.
    for expected in [
        // An empty list, an empty run and an absent optional list all report no child at all.
        "flat-walk \"f();\" => Statement(Call(Callee Args))\n",
        "flat-walk \"boxes;\" => Statement(Boxes)\n",
        "flat-walk \"print [];\" => Statement(Print(Value(List)))\n",
        // A list reports one child per item, however the separator is written.
        "flat-walk \"with a | b;\" => Statement(With(Name Name))\n",
        "flat-walk \"with recursive a | b |;\" => Statement(With(Name Name))\n",
        "flat-walk \"trail a, b,;\" => Statement(Trail(Name Name))\n",
        "flat-walk \"f(1, 2,);\" => Statement(Call(Callee Args(Value(Number) Value(Number))))\n",
        // An absent `Option<one_or_many!>` reports nothing and a present one reports each item.
        "flat-walk \"marks :;\" => Statement(Marks)\n",
        "flat-walk \"marks : !!!;\" => Statement(Marks(Bang Bang Bang))\n",
        // A presence is data, not a child, so a true one adds nothing to the walk.
        "flat-walk \"mods one three four six x;\" => Statement(Modifiers(Name))\n",
        "flat-walk \"mods two five x;\" => Statement(Modifiers(Name))\n",
        // A `boxed!` child is one handle, and an inlined optional Node is one child.
        "flat-walk \"let one if not exists x = 2 as y;\" \
         => Statement(Let(Name Value(Number) Alias(Name)))\n",
        // An erased optional group leaves no Node, so it leaves no child either.
        "flat-walk \"erased x;\" => Statement(Erased(Name))\n",
        "flat-walk \"erased one two three four five x;\" => Statement(Erased(Name))\n",
        // A fixed-token unit Node the parent allocated is still a child.
        "flat-walk \"options as out temp;\" => Statement(Options(KwAs KwOut KwTemp))\n",
        "flat-walk \"options as in out only temp;\" \
         => Statement(Options(KwAs KwIn KwOut KwOnly KwTemp))\n",
        "flat-walk \"filter !;\" => Statement(Filter(Bang))\n",
        // A list inside a list item nests exactly as the store holds it.
        "flat-walk \"print [[], [1, [2]]];\" \
         => Statement(Print(Value(List(Value(List) Value(List(Value(Number) \
         Value(List(Value(Number)))))))))\n",
        // A content token is data: `Name` is a child of `Let`, its text is not a child of `Name`.
        "flat-walk \"let if exists as = only as temp;\" \
         => Statement(Let(Name Value(Name) Alias(Name)))\n",
        // A recursive Node through a nested statement list.
        "flat-walk \"{ print 1; { let if exists x = 2; } f(); }\" \
         => Statement(Block(Statement(Print(Value(Number))) \
         Statement(Block(Statement(Let(Name Value(Number))))) Statement(Call(Callee Args))))\n",
    ] {
        assert!(
            lines.contains(expected),
            "the walk must report\n{expected}\n{lines}"
        );
    }

    // Invariant I1 holds for every fresh parse, which the generated checker confirms per input.
    let checked = lines
        .lines()
        .filter(|line| line.starts_with("flat-post-order "))
        .collect::<Vec<_>>();
    assert_eq!(
        checked.len(),
        48,
        "every positive input runs the post-order checker\n{lines}"
    );
    assert!(
        checked.iter().all(|line| line.ends_with("=> ok")),
        "a fresh parse leaves every typed pool in post-order\n{lines}"
    );

    // The pool census, this time from `pool::<T>()` rather than from a reachability walk. The
    // driver asserts the two agree for every input; these lines record what the pools hold.
    for expected in [
        "flat-pool-len \"print [[1, [2, [3]]], []];\" => values=8 names=0\n",
        "flat-pool-len \"let if exists as = only as temp;\" => values=1 names=3\n",
        "flat-pool-len \"options as in out only temp;\" => values=0 names=0\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // `children` without a walk reports only the direct children of one Node.
    assert!(
        lines.contains("flat-children \"print [1, [2]];\" => [Print]\n"),
        "{lines}"
    );

    // The list store and the text pool through the owner's read-only accessors: two numbers, and
    // one reserved slot plus a run of two and a run of one.
    assert!(
        lines.contains("flat-stores \"print [1, [2]];\" => texts=2 lists=4\n"),
        "{lines}"
    );

    // `enter` returns `false` at the first `Value`, so the walk stops there and still leaves it.
    assert!(
        lines.contains("flat-walk-skip \"print [1, [2]];\" => Statement(Print(Value))\n"),
        "{lines}"
    );

    // A result rooted at a Node other than the grammar root names its own root variant.
    assert!(
        lines.contains(
            "entry value-walk Value(List(Value(Number) Value(List(Value(Name) Value(Name)))))\n"
        ),
        "{lines}"
    );
}

#[test]
fn a_grammar_with_both_arena_ast_and_flat_builds_either_representation() {
    // The two backends own separate stores: an arena AST allocates in the bump arena and a Flat
    // AST in its own pools, list store and text pool. This grammar selects both, so the generated
    // reducer generics must compile for each and both entry points must run.
    let package = scaffold(FIXTURE, Some(ARENA_FLAT_GRAMMAR));
    fs::write(package.path().join("src/main.rs"), ARENA_FLAT_DRIVER)
        .expect("write the arena and flat consumer");
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("arena-flat-ok\n"), "{stdout}");
    assert!(
        stdout.contains("arena Root { negated: true, name: Name { text:"),
        "{stdout}"
    );
    assert!(
        stdout.contains("flat negated=true name=\"x\" value=List(FlatList#"),
        "{stdout}"
    );
    assert!(
        stdout.contains("span=Some(Span { start: 0, end: 20 })"),
        "the flat root keeps its span with `arena_ast` on\n{stdout}"
    );
}

/// A grammar that selects `arena_ast` and `flat` together, with one pooled Node.
const ARENA_FLAT_GRAMMAR: &str = r#"//! A grammar with both `flat` and `arena_ast`.

recursa::grammar! {
    module = crate::grammar,
    flat,
    arena_ast,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords { LET => "let", NOT => "not" }
    punctuation {
        EQ => "=",
        SEMI => ";",
        COMMA => ",",
        LBRACKET => "[",
        RBRACKET => "]",
        BANG => "!",
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(LET, this, SEMI)]
    pub struct Root {
        #[presence(NOT)]
        pub negated: bool,
        pub name: Name,
        #[tok(EQ)]
        pub value: boxed!(Value),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[flat(pool)]
    pub enum Value {
        Name { name: Name },
        Negated { mark: Bang, inner: boxed!(Value) },
        List(List),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(LBRACKET, this, RBRACKET)]
    pub struct List {
        #[sep(COMMA)]
        pub items: zero_or_many!(Value),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(BANG)]
    pub struct Bang;
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}
"#;

/// The consumer that parses one input through each backend of the `arena_ast` and `flat` grammar.
const ARENA_FLAT_DRIVER: &str = r#"//! Proves a grammar with `arena_ast` and `flat` builds both representations.

use recursa::Handle;
use recursa_generated_flat_containers::grammar;

fn main() {
    let source = "let not x = [a, !b];";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let parsed = grammar::Root::parse(&mut input).expect("the arena parse succeeds");
    println!("arena {:?}", parsed.ast());

    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Root::parse_flat(&mut input).expect("the flat parse succeeds");
    let root = flat[flat.root()];
    let value: Handle<grammar::FlatValue> = root.value;
    println!(
        "flat negated={} name={:?} value={:?} span={:?}",
        root.negated,
        flat[flat[root.name].text].as_str(),
        flat[value],
        flat.span(flat.root()),
    );
    println!("arena-flat-ok");
}
"#;

#[test]
fn a_child_the_grammar_root_cannot_name_reaches_the_traversal_as_an_opaque_reference() {
    // `NodeRef` holds a handle per variant, so it cannot hold the handle of a Node the grammar
    // root cannot name. A Node in a private module is exactly that case: `children` reports its
    // site as `NodeRef::Opaque` and stops there, and the variant exists only for a grammar that
    // has such a child. Every other grammar of this file omits it.
    let package = scaffold(FIXTURE, Some(OPAQUE_FLAT_GRAMMAR));
    fs::write(package.path().join("src/main.rs"), OPAQUE_FLAT_DRIVER)
        .expect("write the opaque consumer");
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("opaque-flat-ok\n"), "{stdout}");
    assert!(
        stdout.contains("opaque children=[\"Name\", \"Opaque\", \"Opaque\"] below=0\n"),
        "a non-public child reports one opaque reference per site and no child of its own\n\
         {stdout}"
    );
}

/// A grammar whose helper Node lives in a private module, so the grammar root cannot name it.
const OPAQUE_FLAT_GRAMMAR: &str = r#"//! A Node in a private module, with `flat` on.

recursa::grammar! {
    module = crate::grammar,
    flat,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords { LET => "let" }
    punctuation { SEMI => ";", COMMA => "," }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(LET, this, SEMI)]
    pub struct Root {
        pub name: Name,
        #[sep(COMMA)]
        pub hidden: zero_or_many!(hidden::Hidden),
    }
}

pub mod visible {
    recursa::ast_node! {
        #[derive(Debug)]
        pub struct Seen {
            #[lex(pattern = r"[0-9]+")]
            pub text: SeenText,
        }
    }
}

mod hidden {
    recursa::ast_node! {
        #[derive(Debug)]
        pub struct Hidden {
            pub seen: super::visible::Seen,
        }
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}
"#;

/// The consumer that traverses a result whose second child is a Node in a private module.
const OPAQUE_FLAT_DRIVER: &str = r#"//! Proves a non-public child reaches the traversal as `NodeRef::Opaque`.

use recursa_generated_flat_containers::grammar;

fn main() {
    let source = "let x 1, 2;";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Root::parse_flat(&mut input).expect("the flat parse succeeds");
    let mut names = Vec::new();
    grammar::children(&flat, flat.root_ref(), &mut |child| names.push(child.name()));
    let mut below = 0;
    grammar::children(&flat, grammar::NodeRef::Opaque, &mut |_| below += 1);
    println!("opaque children={names:?} below={below}");
    println!("opaque-flat-ok");
}
"#;

#[test]
fn a_generic_pooled_node_gets_one_pool_and_one_flat_entry_point_per_application() {
    // A generic Node has one materialized application per closed argument list, so a pooled one
    // has one typed pool per application and one flat reduce action and entry point on each
    // application's own flat type.
    let package = scaffold(FIXTURE, Some(GENERIC_FLAT_GRAMMAR));
    fs::write(package.path().join("src/main.rs"), GENERIC_FLAT_DRIVER)
        .expect("write the generic consumer");
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("generic-flat-ok\n"), "{stdout}");
    assert!(
        stdout.contains("names=[\"a\", \"b\"] numbers=[\"1\", \"2\"]\n"),
        "each application stores its own element type\n{stdout}"
    );
    // Each application's pool is indexed from one, which is what makes the two pools separate.
    assert!(
        stdout.contains("handles names=1 numbers=1\n"),
        "each application has its own typed pool\n{stdout}"
    );
    assert!(
        stdout.contains("entry len=2\n"),
        "a generic application has its own flat entry point\n{stdout}"
    );
}

/// A grammar whose pooled Node is generic and materialized at two arguments.
const GENERIC_FLAT_GRAMMAR: &str = r#"//! A generic Node materialized at two arguments, with `flat` on.

recursa::grammar! {
    module = crate::grammar,
    flat,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords { PAIR => "pair" }
    punctuation { COMMA => ",", LBRACKET => "[", RBRACKET => "]", SEMI => ";" }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[tok(PAIR, this, SEMI)]
    pub struct Root {
        pub names: Holder<Name>,
        pub numbers: Holder<Number>,
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    #[flat(pool)]
    #[tok(LBRACKET, this, RBRACKET)]
    pub struct Holder<T> {
        #[sep(COMMA)]
        pub values: one_or_many!(T),
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Name {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        pub text: NameText,
    }
}

recursa::ast_node! {
    #[derive(Debug)]
    pub struct Number {
        #[lex(pattern = r"[0-9]+")]
        pub text: NumberText,
    }
}
"#;

/// The consumer that parses one input of the generic grammar into a Flat AST.
const GENERIC_FLAT_DRIVER: &str = r#"//! Proves flat construction works for a generic Node materialized twice.

use recursa_generated_flat_containers::grammar;

fn main() {
    let source = "pair [a, b] [1, 2];";
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let flat = grammar::Root::parse_flat(&mut input).expect("the flat parse succeeds");
    let root = flat[flat.root()];
    let names = flat[root.names];
    let numbers = flat[root.numbers];
    println!(
        "names={:?} numbers={:?}",
        flat[names.values]
            .iter()
            .map(|handle| flat[flat[*handle].text].as_str())
            .collect::<Vec<_>>(),
        flat[numbers.values]
            .iter()
            .map(|handle| flat[flat[*handle].text].as_str())
            .collect::<Vec<_>>(),
    );
    println!(
        "handles names={} numbers={}",
        root.names.raw(),
        root.numbers.raw()
    );
    println!(
        "spans root={:?} names={:?}",
        flat.span(flat.root()),
        flat.span(root.names)
    );

    let lexed = grammar::lex("[x, y]");
    let mut input = lexed.input();
    let holder = grammar::Holder::<grammar::Name>::parse_flat(&mut input)
        .expect("the generic entry point parses");
    println!("entry len={}", holder[holder[holder.root()].values].len());
    println!("generic-flat-ok");
}
"#;

#[test]
fn a_flat_declaration_attribute_without_the_root_flag_is_rejected() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(FIXTURE, Some(&grammar.replacen(FLAT_FLAG, "", 1)));
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("error[RCA3142] lowering at src/grammar.rs:")
            && stderr.contains("`#[flat(...)]` requires the `flat` flag in the grammar root"),
        "stderr:\n{stderr}",
    );
}

#[test]
fn an_unknown_flat_setting_is_rejected_at_the_declaration() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(
        FIXTURE,
        Some(&grammar.replacen(
            "#[flat(pool, max_size = 16)]",
            "#[flat(pool, packed, max_size = 16)]",
            1,
        )),
    );
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("error[RCA3141] lowering at src/grammar.rs:")
            && stderr.contains("unsupported flat setting `packed`"),
        "stderr:\n{stderr}",
    );
}

#[test]
fn a_flat_attribute_on_an_enum_variant_is_rejected() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(
        FIXTURE,
        Some(&grammar.replacen(
            "        Number(Number),",
            "        #[flat(pool)]\n        Number(Number),",
            1,
        )),
    );
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("error[RCA3141] lowering at src/grammar.rs:")
            && stderr.contains(
                "`#[flat(...)]` belongs on a whole struct or enum declaration, not on an enum \
                 variant",
            ),
        "stderr:\n{stderr}",
    );
}

#[test]
fn an_exceeded_max_size_bound_fails_the_generated_build() {
    let grammar = fs::read_to_string(fixture_grammar()).expect("read the flat grammar");
    let package = scaffold(
        FIXTURE,
        Some(&grammar.replacen(
            "#[flat(pool, max_size = 16)]",
            "#[flat(pool, max_size = 4)]",
            1,
        )),
    );
    let output = check_fixture(&package);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "flat Node `FlatValue` exceeds its declared `#[flat(pool, max_size = 4)]` bound",
        ),
        "stderr:\n{stderr}",
    );
}

/// Returns the fixture's authored grammar source path.
fn fixture_grammar() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE)
        .join("src/grammar.rs")
}

/// Copies the fixture package into a temporary directory; `grammar` replaces its grammar file.
fn scaffold(fixture: &str, grammar: Option<&str>) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(fixture);
    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative, relative);
    }
    copy(&fixture, package.path(), "Cargo.core.lock", "Cargo.lock");
    if let Some(grammar) = grammar {
        fs::write(package.path().join("src/grammar.rs"), grammar).expect("write the grammar");
    }
    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generated grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generated grammar fixture manifest");
    package
}

/// Selects the fixture's tagged lines and terminates each with a newline for whole-line matching.
fn parser_lines(stdout: &str) -> String {
    let mut parser = String::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("lr ") {
            parser.push_str(rest);
            parser.push('\n');
        }
    }
    parser
}

/// Copies one fixture file into the scaffolded package.
fn copy(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy generated grammar fixture file");
}

/// Renders a path for a TOML string literal.
fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

/// Builds and runs the scaffolded fixture binary.
fn run_fixture(package: &TempDir) -> Output {
    cargo(package, "run")
}

/// Builds the scaffolded fixture without running it, for a case that must fail to compile.
fn check_fixture(package: &TempDir) -> Output {
    cargo(package, "check")
}

/// Runs one Cargo subcommand inside the scaffolded package.
fn cargo(package: &TempDir, subcommand: &str) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg(subcommand)
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", package.path().join("target"))
        .output()
        .expect("run generated grammar fixture")
}

/// Fails with both streams when the fixture did not run.
fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
