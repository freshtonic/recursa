//! Expansion logic for `#[railroad]`.
//!
//! The macro parses the annotated item, builds a layout tree from
//! `recursa_diagram_core::layout`, renders it to SVG via
//! `recursa_diagram_core::render`, and emits the original item with the SVG
//! attached as a doc comment.

use proc_macro2::TokenStream;
use quote::quote;
use recursa_diagram_core::layout::{NonTerminal, Terminal};
use recursa_diagram_core::{layout::Node, render};
use syn::parse::Parser;
use syn::{
    DeriveInput, Expr, ExprLit, Lit, MetaNameValue, parse2, punctuated::Punctuated, token::Comma,
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
    Ok(Node::NonTerminal(NonTerminal::new(
        input.ident.to_string(),
        None,
    )))
}

fn strip_body(input: &DeriveInput) -> TokenStream {
    let mut clone = input.clone();
    clone.attrs.retain(|a| !a.path().is_ident("railroad"));
    quote! { #clone }
}
