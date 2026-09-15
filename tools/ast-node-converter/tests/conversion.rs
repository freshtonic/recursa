use recursa_ast_node_converter::{
    convert_embedded_sources, convert_format_template, convert_source, format_markdown,
    format_source,
};

#[test]
fn converts_representative_declarations_without_touching_surrounding_rust() {
    let source = r#"//! module docs
#[derive(recursa::Node, Debug, ThirdParty)]
#[helper(answer = "yes")]
#[node(omit(Visit))]
pub enum Expr<'input, T> where T: Clone {
    /// Recursive arm.
    Many(Vec<std::boxed::Box<Expr<'input, T>>>),
    Some(Option<recursa::Vec1<Name<'input>>>),
    UserBox(Box<T>),
}
impl<'input, T> Expr<'input, T> { fn handwritten(&self) {} }
"#;
    let (actual, report) = convert_source(source).unwrap();
    assert!(actual.contains("recursa::ast_node! {"));
    assert!(actual.contains("#[derive(Debug, ThirdParty)]"));
    assert!(actual.contains("#[omit(Visit)]"));
    assert!(actual.contains("zero_or_many!(boxed!(Expr < T >))"));
    assert!(actual.contains("Option < one_or_many ! (Name) >"));
    assert!(actual.contains("UserBox(boxed!(T))"));
    assert!(actual.contains("impl<'input, T> Expr<'input, T>"));
    assert!(!actual.contains("Expr<>"));
    assert_eq!(report.declarations, 1);
    assert_eq!(convert_source(&actual).unwrap().0, actual);
}

#[test]
fn converts_arena_storage_constructors() {
    let source = "#[derive(Node)]\nenum X<'input> { A(recursa::ArenaBox<'input, Y<'input>>), B(recursa::ArenaVec<'input, Y<'input>>) }";
    let (actual, report) = convert_source(source).unwrap();
    assert!(actual.contains("boxed!(Y)"));
    assert!(actual.contains("zero_or_many!(Y)"));
    assert_eq!(report.containers, 2);
}

#[test]
fn removes_multiple_declaration_lifetimes_but_preserves_handwritten_signatures() {
    let source = "#[derive(Node)]\nstruct Pair<'left, 'right>(Left<'left>, Right<'right>);\nimpl<'left, 'right> Pair<'left, 'right> {}";
    let (actual, report) = convert_source(source).unwrap();
    assert!(actual.contains("struct Pair(Left, Right)"));
    assert!(actual.contains("impl<'left, 'right> Pair<'left, 'right> {}"));
    assert_eq!(report.lifetimes, 4);
}

#[test]
fn supports_stdin_sized_whole_source_conversion_and_multiple_nodes() {
    let source = "#[derive(Node)]\nstruct A;\n\n#[derive(recursa::Node)]\nstruct B(Vec<C>);\n";
    let (actual, report) = convert_source(source).unwrap();
    assert_eq!(actual.matches("recursa::ast_node!").count(), 2);
    assert!(actual.contains("zero_or_many!(C)"));
    assert_eq!(report.declarations, 2);
}

#[test]
fn keeps_the_derive_facade_for_the_outer_macro() {
    let (actual, _) = convert_source("#[derive(my_recursa::Node)]\nstruct A;").unwrap();
    assert!(actual.starts_with("my_recursa::ast_node!"));
}

#[test]
fn reports_interior_type_comments_instead_of_discarding_them() {
    let (actual, report) =
        convert_source("#[derive(Node)]\nstruct A(Vec</* retain */ B>);").unwrap();
    assert!(actual.contains("Vec</* retain */ B>"));
    assert_eq!(report.unsupported.len(), 1);
}

#[test]
fn converts_complete_rust_sources_inside_host_string_literals() {
    let host = r###"fn fixture() { let source = r#"#[derive(recursa::Node)]
struct Root<'input>(Vec<Name<'input>>);"#; }"###;
    let (actual, report) = convert_embedded_sources(host).unwrap();
    assert!(actual.contains("recursa::ast_node!"));
    assert!(actual.contains("zero_or_many!(Name)"));
    assert_eq!(report.declarations, 1);
}

#[test]
fn converts_format_string_grammar_templates() {
    let host = r###"fn fixture(invalid_variant: &str) { let source = format!(r#"
recursa::grammar! {{ module = crate }}
#[derive(recursa::Node)]
enum Expr<'input> {{ {invalid_variant} }}
"#); }"###;
    let template = "\nrecursa::grammar! {{ module = crate }}\n#[derive(recursa::Node)]\nenum Expr<'input> {{ {invalid_variant} }}\n";
    convert_format_template(template).unwrap();
    let (actual, report) = convert_embedded_sources(host).unwrap();
    assert!(actual.contains("recursa::ast_node! {{"), "{actual}");
    assert!(actual.contains("enum Expr {{"), "{actual}");
    assert!(actual.contains("{invalid_variant}"), "{actual}");
    assert_eq!(report.declarations, 1);
}

#[test]
fn preserves_unknown_format_slots_and_is_idempotent() {
    let host = r###"fn fixture(second_priority: u8) { let source = format!(r#"
recursa::grammar! {{ module = crate }}
recursa::tokens! {{ matchers {{ Line => physical_line(prefix = r"@", priority = {second_priority}) }} }}
recursa::ast_node! {{ struct Root; }}
"#); }"###;
    let (once, _) = convert_embedded_sources(host).unwrap();
    assert!(once.contains("priority = {second_priority}"), "{once}");
    assert!(!once.contains("priority = {{second_priority}}"), "{once}");
    assert_eq!(convert_embedded_sources(&once).unwrap().0, once);
}

#[test]
fn formats_declaration_bodies_through_temporary_modules() {
    let source = "recursa::ast_node! {\nstruct Root{value:Option<one_or_many!(Name)>}\n}\n";
    let actual = format_source(source).unwrap();
    assert!(actual.contains("recursa::ast_node! {\n    struct Root {"));
    assert!(actual.contains("value: Option<one_or_many!(Name)>,"));
}

#[test]
fn formats_ast_node_rust_fences_without_touching_other_fences() {
    let markdown = "Before\n```rust,ignore\nrecursa::ast_node! { struct Root{value:Name} }\n```\n```rust\nfn untouched( ) {}\n```\n";
    let actual = format_markdown(markdown).unwrap();
    assert!(actual.contains("    struct Root {\n        value: Name,"));
    assert!(actual.contains("fn untouched( ) {}"));
}
