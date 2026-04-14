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
            _ => {}
        }
    }
    let name = type_label(ty);
    let href = type_href(ty);
    Node::NonTerminal(NonTerminal::new(name, href))
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

// TODO: hrefs are currently fabricated as `{TypeName}.html`, assuming the
// diagram embeds in sibling rustdoc pages. Make this configurable when we
// support embedding in mdBook or external docs.
fn type_href(ty: &Type) -> Option<String> {
    Some(format!("{}.html", type_label(ty)))
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
}
