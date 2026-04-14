//! Expansion logic for `#[railroad]`.
//!
//! The macro parses the annotated item, builds a layout tree from
//! `recursa_diagram_core::layout`, renders it to SVG via
//! `recursa_diagram_core::render`, and emits the original item with the SVG
//! attached as a doc comment.

use proc_macro2::TokenStream;
use quote::quote;
use recursa_diagram_core::layout::{Choice, NonTerminal, OneOrMore, Optional, Sequence, Terminal};
use recursa_diagram_core::{layout::Node, render};
use syn::parse::Parser;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, Fields, GenericArgument, Ident, Lit, LitStr,
    MetaNameValue, PathArguments, Type, parse2, punctuated::Punctuated, token::Comma,
};

pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let attrs = parse_type_attrs(attr)?;

    let node = build_node(&input, &attrs)?;
    let svg = render(&node);
    let doc = format!("\n\n{svg}\n\n");

    let body = strip_body(&input);

    Ok(quote! {
        #[doc = #doc]
        #body
    })
}

#[derive(Debug, Default)]
struct TypeAttrs {
    label: Option<String>,
}

fn parse_type_attrs(attr: TokenStream) -> syn::Result<TypeAttrs> {
    let mut out = TypeAttrs::default();
    if attr.is_empty() {
        return Ok(out);
    }
    let nvs = Punctuated::<MetaNameValue, Comma>::parse_terminated.parse2(attr)?;
    for nv in nvs {
        if nv.path.is_ident("label") {
            if out.label.is_some() {
                return Err(syn::Error::new_spanned(nv.path, "duplicate `label`"));
            }
            if let Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) = nv.value
            {
                out.label = Some(s.value());
            } else {
                return Err(syn::Error::new_spanned(nv.value, "expected string literal"));
            }
        } else {
            return Err(syn::Error::new_spanned(nv.path, "unknown attribute key"));
        }
    }
    Ok(out)
}

fn build_node(input: &DeriveInput, attrs: &TypeAttrs) -> syn::Result<Node> {
    if let Some(label) = &attrs.label {
        return Ok(Node::Terminal(Terminal::new(label)));
    }
    match &input.data {
        Data::Struct(s) => match &s.fields {
            // Bare `#[railroad]` on `struct Foo;` has nothing structural to
            // render, so fall back to a NonTerminal bearing the type name.
            // This also gives the user a visible label in the rendered diagram.
            Fields::Unit => Ok(Node::NonTerminal(NonTerminal::new(
                input.ident.to_string(),
                Some(format!("{}.html", input.ident)),
            ))),
            _ => build_from_fields(&s.fields),
        },
        Data::Enum(e) => {
            if e.variants.is_empty() {
                return Err(syn::Error::new_spanned(
                    input,
                    "empty enum has nothing to render",
                ));
            }
            // Per CLAUDE.md, recursa enum variants are single-field tuple
            // variants wrapping a `Parse`-implementing type. We recognise that
            // shape by recursing into the inner type. Anything else (unit,
            // multi-field tuple, named) is non-conforming, but we still produce
            // a defensive `NonTerminal(VariantIdent)` rather than failing.
            let children: Vec<Node> = e
                .variants
                .iter()
                .map(|v| match &v.fields {
                    Fields::Unnamed(u) if u.unnamed.len() == 1 => recognize(&u.unnamed[0].ty),
                    _ => Node::NonTerminal(NonTerminal::new(v.ident.to_string(), None)),
                })
                .collect();
            // Default to the first declared variant; we have no semantic
            // information to choose otherwise.
            Ok(Node::Choice(Choice::new(0, children)))
        }
        Data::Union(_) => Err(syn::Error::new_spanned(input, "unions are not supported")),
    }
}

fn build_from_fields(fields: &Fields) -> syn::Result<Node> {
    let iter: Box<dyn Iterator<Item = &syn::Field>> = match fields {
        Fields::Named(n) => Box::new(n.named.iter()),
        Fields::Unnamed(u) => Box::new(u.unnamed.iter()),
        Fields::Unit => unreachable!("Fields::Unit handled in build_node"),
    };
    let mut children = Vec::new();
    for field in iter {
        let field_attrs = parse_field_attrs(&field.attrs)?;
        if field_attrs.skip {
            continue;
        }
        children.push(node_for_field_type(&field.ty, &field_attrs));
    }
    Ok(Node::Sequence(Sequence::new(children)))
}

#[derive(Default)]
struct FieldAttrs {
    label: Option<String>,
    skip: bool,
}

fn parse_field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut out = FieldAttrs::default();
    for a in attrs {
        if !a.path().is_ident("railroad") {
            continue;
        }
        a.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                if out.label.is_some() {
                    return Err(meta.error("duplicate `label`"));
                }
                let s: LitStr = meta.value()?.parse()?;
                out.label = Some(s.value());
            } else if meta.path.is_ident("skip") {
                if out.skip {
                    return Err(meta.error("duplicate `skip`"));
                }
                out.skip = true;
            } else {
                return Err(meta.error("unknown key"));
            }
            Ok(())
        })?;
    }
    if out.skip && out.label.is_some() {
        return Err(syn::Error::new_spanned(
            &attrs[0],
            "`skip` and `label` are mutually exclusive",
        ));
    }
    Ok(out)
}

fn node_for_field_type(ty: &Type, field_attrs: &FieldAttrs) -> Node {
    if let Some(label) = &field_attrs.label {
        return Node::NonTerminal(NonTerminal::new(label, None));
    }
    recognize(ty)
}

fn recognize(ty: &Type) -> Node {
    // Direct reference to a `keyword::*` type (no PhantomData wrapper). Common
    // shape inside enum variants like `JoinType::Left(keyword::Left)`. We
    // render these as terminals with the uppercased last-segment ident, same
    // convention as the `PhantomData<keyword::T>` case below.
    if is_keyword_path(ty) {
        let label = type_label(ty).to_uppercase();
        return Node::Terminal(Terminal::new(label));
    }
    if let Some((ident, args)) = outer_generic(ty) {
        match ident.to_string().as_str() {
            "Option" if args.len() == 1 => {
                return Node::Optional(Optional::new(recognize(&args[0])));
            }
            "Seq" | "Punctuated" if args.len() == 2 => {
                let child = recognize(&args[0]);
                let sep = recognize(&args[1]);
                return Node::OneOrMore(OneOrMore::new(child, Some(sep)));
            }
            "Vec" if args.len() == 1 => {
                return Node::OneOrMore(OneOrMore::new(recognize(&args[0]), None));
            }
            "Surrounded" if args.len() == 3 => {
                return Node::Sequence(Sequence::new(vec![
                    recognize(&args[0]),
                    recognize(&args[1]),
                    recognize(&args[2]),
                ]));
            }
            "Box" | "Rc" | "Arc" if args.len() == 1 => {
                return recognize(&args[0]);
            }
            // `PhantomData<T>` is the convention pg-sql uses for keyword
            // markers: a parsed-and-discarded token that carries no runtime
            // data. Render it as a literal terminal box. We uppercase the
            // type's last-segment ident because keyword types use TitleCase
            // identifiers (e.g. `keyword::Drop`) whereas the SQL keyword
            // they represent is uppercase (`DROP`). The convention is
            // codebase-specific; if a non-keyword `PhantomData<T>` ever
            // appears its terminal label will still be readable, just
            // shoutier than expected.
            "PhantomData" if args.len() == 1 => {
                let label = type_label(&args[0]).to_uppercase();
                return Node::Terminal(Terminal::new(label));
            }
            _ => {}
        }
    }
    let name = type_label(ty);
    let href = type_href(ty);
    Node::NonTerminal(NonTerminal::new(name, href))
}

/// Detect a type path whose second-to-last segment is `keyword`, e.g.
/// `keyword::Left`, `pg_sql::keyword::Drop`, or `crate::keyword::Where`.
/// Used by `recognize` to render keyword references as literal terminals.
fn is_keyword_path(ty: &Type) -> bool {
    let Type::Path(p) = ty else {
        return false;
    };
    let segs = &p.path.segments;
    if segs.len() < 2 {
        return false;
    }
    segs[segs.len() - 2].ident == "keyword"
}

fn outer_generic(ty: &Type) -> Option<(&Ident, Vec<Type>)> {
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && let PathArguments::AngleBracketed(ab) = &seg.arguments
    {
        let args: Vec<_> = ab
            .args
            .iter()
            .filter_map(|a| {
                if let GenericArgument::Type(t) = a {
                    Some(t.clone())
                } else {
                    None
                }
            })
            .collect();
        return Some((&seg.ident, args));
    }
    None
}

fn type_label(ty: &Type) -> String {
    if let Type::Path(p) = ty
        && let Some(last) = p.path.segments.last()
    {
        return last.ident.to_string();
    }
    quote!(#ty).to_string()
}

// Best-effort relative href for a type-path reference. We strip a single
// leading `crate::` segment and emit the remaining path with `/` separators
// followed by `.html`. For a same-module bare reference like `Foo` this yields
// `Foo.html`, which rustdoc resolves against the current page. For a qualified
// path like `crate::ast::Thing` this yields `ast/Thing.html`, which is correct
// when the diagram is rendered inside a doc page at the crate root but wrong
// when the caller lives in a nested module — proc macros lack the module
// context to do better. Phase 5's smoke test will reveal whether rustdoc
// accepts these links; if not, we'll need a configurable path-prefix attribute.
//
// Returns `None` for:
// - non-path types (references, tuples, slices, etc.)
// - absolute paths (`::std::vec::Vec`) — would link out of the current crate
// - paths starting with `super::` or `self::` — rustdoc has no such directories
//   and we can't resolve them without the calling module's context
fn type_href(ty: &Type) -> Option<String> {
    let Type::Path(p) = ty else {
        return None;
    };
    if p.path.leading_colon.is_some() || p.path.segments.is_empty() {
        return None;
    }
    let mut segments: Vec<String> = p
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    if matches!(segments.first().map(String::as_str), Some("super" | "self")) {
        return None;
    }
    if segments.first().map(String::as_str) == Some("crate") {
        segments.remove(0);
    }
    if segments.is_empty() {
        return None;
    }
    Some(format!("{}.html", segments.join("/")))
}

fn strip_body(input: &DeriveInput) -> TokenStream {
    let mut clone = input.clone();
    clone.attrs.retain(|a| !a.path().is_ident("railroad"));
    quote! { #clone }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn node_for(tokens: TokenStream) -> Node {
        let input: DeriveInput = parse2(tokens).unwrap();
        build_node(&input, &TypeAttrs::default()).unwrap()
    }

    #[test]
    fn named_struct_fields_render_as_sequence() {
        let node = node_for(quote! {
            pub struct ArgList { name: Ident, comma: Comma, value: Ident }
        });
        match node {
            Node::Sequence(seq) => assert_eq!(seq.children.len(), 3),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn tuple_struct_fields_render_as_sequence() {
        let node = node_for(quote! { pub struct Pair(Ident, Ident); });
        match node {
            Node::Sequence(seq) => assert_eq!(seq.children.len(), 2),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn unit_struct_falls_back_to_non_terminal_with_ident() {
        let node = node_for(quote! { pub struct Empty; });
        match node {
            Node::NonTerminal(nt) => {
                assert_eq!(nt.text, "Empty");
                assert_eq!(nt.href.as_deref(), Some("Empty.html"));
            }
            other => panic!("expected NonTerminal, got {other:?}"),
        }
    }

    #[test]
    fn vec_renders_as_one_or_more_without_separator() {
        let node = node_for(quote! { pub struct S { xs: Vec<Foo> } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::OneOrMore(om) => assert!(om.separator.is_none()),
                other => panic!("expected OneOrMore, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn punctuated_renders_as_one_or_more_with_separator() {
        let node = node_for(quote! { pub struct S { xs: Punctuated<Foo, Comma> } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::OneOrMore(om) => assert!(om.separator.is_some()),
                other => panic!("expected OneOrMore, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_type_label_is_rejected() {
        let attr: TokenStream = quote! { label = "A", label = "B" };
        let err = parse_type_attrs(attr).expect_err("expected duplicate-label error");
        assert!(
            err.to_string().contains("duplicate"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn duplicate_field_label_is_rejected() {
        // We can't easily call parse_field_attrs through build_node because
        // duplicate label will fail before any node is built; assert via the
        // expand entry instead.
        let item: TokenStream = quote! {
            pub struct S {
                #[railroad(label = "A", label = "B")]
                f: Foo,
            }
        };
        let err = expand(quote! {}, item).expect_err("expected duplicate-label error");
        assert!(
            err.to_string().contains("duplicate"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn option_renders_as_optional() {
        let node = node_for(quote! { pub struct S { x: Option<Foo> } });
        match node {
            Node::Sequence(seq) => {
                assert_eq!(seq.children.len(), 1);
                assert!(matches!(seq.children[0], Node::Optional(_)));
            }
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn seq_renders_as_one_or_more() {
        let node = node_for(quote! { pub struct S { xs: Seq<Foo, Comma> } });
        match node {
            Node::Sequence(seq) => assert!(matches!(seq.children[0], Node::OneOrMore(_))),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn surrounded_renders_as_sequence_of_three() {
        let node = node_for(quote! { pub struct S { g: Surrounded<LParen, Foo, RParen> } });
        match node {
            Node::Sequence(outer) => match &outer.children[0] {
                Node::Sequence(inner) => assert_eq!(inner.children.len(), 3),
                other => panic!("expected inner Sequence, got {other:?}"),
            },
            other => panic!("expected outer Sequence, got {other:?}"),
        }
    }

    #[test]
    fn box_is_transparent() {
        let node = node_for(quote! { pub struct S { b: Box<Foo> } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => assert_eq!(nt.text, "Foo"),
                other => panic!("expected unwrapped Foo, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn direct_keyword_path_renders_as_uppercased_terminal() {
        // Common shape inside enum variants like `JoinType::Left(keyword::Left)`.
        // No PhantomData wrapper — recognize must detect the `keyword::` prefix
        // on the type path itself.
        let node = node_for(quote! {
            pub enum JoinType {
                Left(keyword::Left),
                Right(keyword::Right),
            }
        });
        match node {
            Node::Choice(ch) => {
                assert_eq!(ch.children.len(), 2);
                match &ch.children[0] {
                    Node::Terminal(t) => assert_eq!(t.text, "LEFT"),
                    other => panic!("expected Terminal, got {other:?}"),
                }
                match &ch.children[1] {
                    Node::Terminal(t) => assert_eq!(t.text, "RIGHT"),
                    other => panic!("expected Terminal, got {other:?}"),
                }
            }
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn keyword_path_in_struct_field_also_works() {
        let node = node_for(quote! {
            pub struct S { kw: keyword::Select }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::Terminal(t) => assert_eq!(t.text, "SELECT"),
                other => panic!("expected Terminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn non_keyword_path_is_not_uppercased() {
        // A type like `ast::expr::BinaryOp` should not be miscategorized as
        // a keyword just because it has a multi-segment path.
        let node = node_for(quote! {
            pub struct S { op: ast::expr::BinaryOp }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => assert_eq!(nt.text, "BinaryOp"),
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn phantom_data_renders_as_uppercased_terminal() {
        // PhantomData<T> in pg-sql means "T is a keyword marker". Render it
        // as a literal terminal with the ident uppercased so `DROP`/`TABLE`
        // appear as terminals instead of opaque PhantomData boxes.
        let node = node_for(quote! {
            pub struct S { _drop: PhantomData<keyword::Drop> }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::Terminal(t) => assert_eq!(t.text, "DROP"),
                other => panic!("expected Terminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn enum_renders_as_choice() {
        let node = node_for(quote! {
            pub enum Stmt {
                Select(SelectStmt),
                Insert(InsertStmt),
                Update(UpdateStmt),
            }
        });
        match node {
            Node::Choice(ch) => {
                assert_eq!(ch.children.len(), 3);
                assert_eq!(ch.default_idx, 0);
            }
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn enum_variant_unwraps_inner_type() {
        let node = node_for(quote! {
            pub enum E {
                Wrapped(Box<Foo>),
            }
        });
        match node {
            Node::Choice(ch) => match &ch.children[0] {
                Node::NonTerminal(nt) => assert_eq!(nt.text, "Foo"),
                other => panic!("expected NonTerminal Foo, got {other:?}"),
            },
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn unit_variant_falls_back_to_variant_name() {
        let node = node_for(quote! {
            pub enum E { Bare }
        });
        match node {
            Node::Choice(ch) => match &ch.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "Bare");
                    assert!(nt.href.is_none());
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn empty_enum_is_rejected() {
        let input: DeriveInput = parse2(quote! { pub enum Empty {} }).unwrap();
        let err = build_node(&input, &TypeAttrs::default()).expect_err("expected empty-enum error");
        assert!(
            err.to_string().contains("empty enum"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn field_skip_removes_from_sequence() {
        let node = node_for(quote! {
            pub struct S {
                a: Ident,
                #[railroad(skip)]
                b: Ident,
                c: Ident,
            }
        });
        match node {
            Node::Sequence(seq) => assert_eq!(seq.children.len(), 2),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    fn parse_type(tokens: TokenStream) -> Type {
        parse2(tokens).unwrap()
    }

    #[test]
    fn single_segment_type_href() {
        assert_eq!(
            type_href(&parse_type(quote!(Foo))).as_deref(),
            Some("Foo.html")
        );
    }

    #[test]
    fn crate_prefixed_type_href() {
        assert_eq!(
            type_href(&parse_type(quote!(crate::Foo))).as_deref(),
            Some("Foo.html")
        );
    }

    #[test]
    fn qualified_path_becomes_relative_href() {
        assert_eq!(
            type_href(&parse_type(quote!(crate::ast::Thing))).as_deref(),
            Some("ast/Thing.html")
        );
        assert_eq!(
            type_href(&parse_type(quote!(crate::ast::other::Thing))).as_deref(),
            Some("ast/other/Thing.html")
        );
    }

    #[test]
    fn non_path_type_returns_none() {
        assert_eq!(type_href(&parse_type(quote!(&'a Foo))), None);
        assert_eq!(type_href(&parse_type(quote!((Foo, Bar)))), None);
        // Bare path still produces Some.
        assert!(type_href(&parse_type(quote!(Foo))).is_some());
    }

    #[test]
    fn super_and_self_paths_return_none() {
        // rustdoc has no `super` or `self` directories; better to omit the
        // link than emit a guaranteed 404.
        assert_eq!(type_href(&parse_type(quote!(super::Foo))), None);
        assert_eq!(type_href(&parse_type(quote!(self::Foo))), None);
        assert_eq!(type_href(&parse_type(quote!(super::ast::Foo))), None);
    }

    #[test]
    fn absolute_paths_return_none() {
        // `::std::vec::Vec` would link out of the current crate; omit.
        assert_eq!(type_href(&parse_type(quote!(::std::vec::Vec))), None);
        assert_eq!(type_href(&parse_type(quote!(::core::option::Option))), None);
    }

    #[test]
    fn double_crate_strips_only_one() {
        // Pathological — `skip_while` would have stripped both. Single strip
        // matches the comment on type_href.
        assert_eq!(
            type_href(&parse_type(quote!(crate::crate_::Foo))).as_deref(),
            Some("crate_/Foo.html")
        );
    }

    #[test]
    fn field_label_overrides_type_name() {
        let node = node_for(quote! {
            pub struct S {
                #[railroad(label = "SELECT")]
                kw: SelectKw,
            }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "SELECT");
                    // A label short-circuits recognize entirely, so no href
                    // should be derived from the underlying type. Pinning this
                    // distinguishes the label path from incidental NonTerminal
                    // construction with a coincidentally-matching text.
                    assert!(nt.href.is_none(), "label should suppress href");
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn split_attribute_label_skip_conflict_is_rejected() {
        // The conflict check fires after both attributes have been folded
        // into the same FieldAttrs. Verify it works when label and skip
        // arrive in separate `#[railroad(...)]` attrs, not just the same one.
        let item: TokenStream = quote! {
            pub struct S {
                #[railroad(skip)]
                #[railroad(label = "X")]
                f: Foo,
            }
        };
        let err = expand(quote! {}, item).expect_err("expected conflict error");
        assert!(
            err.to_string().contains("mutually exclusive"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn multi_field_tuple_variant_falls_back_to_variant_name() {
        // Non-conforming per CLAUDE.md (variants must be single-field tuples)
        // but the macro defends against it with a NonTerminal fallback.
        let node = node_for(quote! {
            pub enum E {
                Pair(Foo, Bar),
            }
        });
        match node {
            Node::Choice(ch) => match &ch.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "Pair");
                    assert!(nt.href.is_none());
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Choice, got {other:?}"),
        }
    }
}
