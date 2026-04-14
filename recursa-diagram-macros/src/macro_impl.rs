//! Expansion logic for `#[railroad]`.
//!
//! The macro parses the annotated item, builds a layout tree from
//! `recursa_diagram_core::layout`, renders it to SVG via
//! `recursa_diagram_core::render`, and emits the original item with the SVG
//! attached as a doc comment.

use proc_macro2::TokenStream;
use quote::quote;
use recursa_diagram_core::layout::{NonTerminal, Sequence, Terminal};
use recursa_diagram_core::{layout::Node, render};
use syn::parse::Parser;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, Fields, Lit, LitStr, MetaNameValue, Type, parse2,
    punctuated::Punctuated, token::Comma,
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

#[derive(Default)]
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
        Data::Struct(s) => build_from_fields(&s.fields),
        Data::Enum(_) => Ok(Node::NonTerminal(NonTerminal::new(
            input.ident.to_string(),
            None,
        ))),
        Data::Union(_) => Err(syn::Error::new_spanned(input, "unions are not supported")),
    }
}

fn build_from_fields(fields: &Fields) -> syn::Result<Node> {
    let iter: Box<dyn Iterator<Item = &syn::Field>> = match fields {
        Fields::Named(n) => Box::new(n.named.iter()),
        Fields::Unnamed(u) => Box::new(u.unnamed.iter()),
        Fields::Unit => return Ok(Node::Sequence(Sequence::new(vec![]))),
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
                let s: LitStr = meta.value()?.parse()?;
                out.label = Some(s.value());
            } else if meta.path.is_ident("skip") {
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
    let name = type_label(ty);
    let href = type_href(ty);
    Node::NonTerminal(NonTerminal::new(name, href))
}

fn type_label(ty: &Type) -> String {
    if let Type::Path(p) = ty
        && let Some(last) = p.path.segments.last()
    {
        return last.ident.to_string();
    }
    quote!(#ty).to_string()
}

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
    fn unit_struct_renders_as_empty_sequence() {
        let node = node_for(quote! { pub struct Empty; });
        match node {
            Node::Sequence(seq) => assert!(seq.children.is_empty()),
            other => panic!("expected empty Sequence, got {other:?}"),
        }
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
