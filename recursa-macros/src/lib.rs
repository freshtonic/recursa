//! Structural loaders for matching-version Recursa generated output.

use proc_macro::TokenStream;
use quote::quote;
use std::path::{Component, Path, PathBuf};
use syn::ext::IdentExt as _;
use syn::parse::Parse as _;
use syn::{DeriveInput, Type, parse_macro_input};

const PRIVATE_PROTOCOL: u32 = 1;

/// Marks a source declaration as a grammar Node and loads its generated body.
///
/// The derive deliberately interprets no fields or grammar helper attributes.
#[proc_macro_derive(Node, attributes(lex, node, tok, sep, parse, pratt, spanned))]
pub fn node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;
    let span = ident.span();
    let start = span.start();
    let file = span
        .local_file()
        .and_then(|path| path.canonicalize().ok())
        .unwrap_or_else(|| Path::new(&span.file()).to_path_buf());
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    let relative_file = manifest
        .as_deref()
        .and_then(|manifest| file.strip_prefix(manifest).ok())
        .unwrap_or(&file);
    let source_identity = normalized_source_identity(relative_file);
    let anchor = format!(
        "{}:{}:{}:{}",
        source_identity,
        start.line,
        start.column + 1,
        ident.unraw(),
    );
    let loader_anchor_text = format!("{:016x}", stable_hash(anchor.as_bytes()));
    let loader_anchor = syn::LitStr::new(&loader_anchor_text, proc_macro2::Span::call_site());
    quote! {
        __recursa_load_node!(#ident, #loader_anchor);
    }
    .into()
}

/// Loads the complete Generation selected by the build script.
///
/// Only `recursa-codegen` interprets this macro's grammar configuration.
#[proc_macro]
pub fn grammar(_input: TokenStream) -> TokenStream {
    let release = format!("release_{}", env!("CARGO_PKG_VERSION")).replace(['.', '-'], "_");
    let release = syn::Ident::new(&release, proc_macro2::Span::call_site());
    let protocol = syn::Ident::new(
        &format!("protocol_{PRIVATE_PROTOCOL}"),
        proc_macro2::Span::call_site(),
    );
    let capabilities = syn::Ident::new(
        &format!(
            "cap_core_d{}_e{}_a{}_s{}",
            u8::from(cfg!(feature = "diagrams")),
            u8::from(cfg!(feature = "explorer")),
            u8::from(cfg!(feature = "arbitrary")),
            u8::from(cfg!(feature = "serde")),
        ),
        proc_macro2::Span::call_site(),
    );
    let arbitrary_gate = if cfg!(feature = "arbitrary") {
        quote! {
            macro_rules! __recursa_if_arbitrary {
                ($($tokens:tt)*) => { $($tokens)* };
            }
        }
    } else {
        quote! {
            macro_rules! __recursa_if_arbitrary {
                ($($tokens:tt)*) => {};
            }
        }
    };
    let serde_gate = if cfg!(feature = "serde") {
        quote! {
            macro_rules! __recursa_if_serde {
                ($($tokens:tt)*) => { $($tokens)* };
            }
        }
    } else {
        quote! {
            macro_rules! __recursa_if_serde {
                ($($tokens:tt)*) => {};
            }
        }
    };
    quote! {
        #arbitrary_gate
        #serde_gate
        include!(concat!(env!("OUT_DIR"), "/recursa-selected.rs"));
        __recursa_root_handshake!(#release, #protocol, #capabilities);
    }
    .into()
}

/// Structural placeholder for token declarations consumed by the analyzer.
#[proc_macro]
pub fn tokens(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

struct TotalVisitorInput {
    dispatch: Vec<Type>,
    error: Type,
    facade: syn::Path,
}

fn total_visitor_input(input: &DeriveInput, attribute: &str) -> syn::Result<TotalVisitorInput> {
    let mut dispatch = None;
    let mut error = None;
    let mut facade = None;
    for attr in &input.attrs {
        if !attr.path().is_ident(attribute) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("dispatch") {
                let value = meta.value()?;
                let content;
                syn::bracketed!(content in value);
                dispatch = Some(
                    content
                        .parse_terminated(Type::parse, syn::Token![,])?
                        .into_iter()
                        .collect(),
                );
                Ok(())
            } else if meta.path.is_ident("error") {
                error = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("facade") {
                facade = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("expected `dispatch`, `error`, or `facade`"))
            }
        })?;
    }
    Ok(TotalVisitorInput {
        dispatch: dispatch.ok_or_else(|| {
            syn::Error::new_spanned(
                &input.ident,
                format!("missing `dispatch` in #[{attribute}(...)]"),
            )
        })?,
        error: error.ok_or_else(|| {
            syn::Error::new_spanned(
                &input.ident,
                format!("missing `error` in #[{attribute}(...)]"),
            )
        })?,
        facade: facade.unwrap_or_else(mapped_facade_path),
    })
}

fn mapped_facade_path() -> syn::Path {
    match proc_macro_crate::crate_name("recursa") {
        Ok(proc_macro_crate::FoundCrate::Itself) => syn::parse_quote!(crate),
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            syn::Path::from(syn::Ident::new(&name, proc_macro2::Span::call_site()))
        }
        Err(_) => syn::parse_quote!(recursa),
    }
}

#[proc_macro_derive(TotalVisitor, attributes(total_visitor))]
pub fn total_visitor(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    render_total_visitor(&input, false)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(TotalVisitorMut, attributes(total_visitor_mut))]
pub fn total_visitor_mut(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    render_total_visitor(&input, true)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn render_total_visitor(
    input: &DeriveInput,
    mutable: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    let attribute = if mutable {
        "total_visitor_mut"
    } else {
        "total_visitor"
    };
    let configuration = total_visitor_input(input, attribute)?;
    let name = &input.ident;
    let error = &configuration.error;
    let facade = &configuration.facade;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    if mutable {
        let enter = configuration.dispatch.iter().map(|ty| {
            quote! {
                if ::core::any::type_name::<N>() == ::core::any::type_name::<#ty>() {
                    // SAFETY: the accepted traversal contract uses type-name
                    // dispatch so borrowed AST lifetimes remain supported.
                    let node = unsafe { &mut *(node as *mut N as *mut #ty) };
                    return <Self as #facade::VisitorMut<#ty>>::enter_mut(self, node);
                }
            }
        });
        let exit = configuration.dispatch.iter().map(|ty| {
            quote! {
                if ::core::any::type_name::<N>() == ::core::any::type_name::<#ty>() {
                    // SAFETY: see `total_enter_mut` above.
                    let node = unsafe { &mut *(node as *mut N as *mut #ty) };
                    return <Self as #facade::VisitorMut<#ty>>::exit_mut(self, node);
                }
            }
        });
        Ok(quote! {
            impl #impl_generics #facade::TotalVisitorMut for #name #type_generics #where_clause {
                type Error = #error;
                fn total_enter_mut<N>(
                    &mut self,
                    node: &mut N,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>> {
                    #(#enter)*
                    ::core::ops::ControlFlow::Continue(())
                }
                fn total_exit_mut<N>(
                    &mut self,
                    node: &mut N,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>> {
                    #(#exit)*
                    ::core::ops::ControlFlow::Continue(())
                }
            }
        })
    } else {
        let enter = configuration.dispatch.iter().map(|ty| {
            quote! {
                if ::core::any::type_name::<N>() == ::core::any::type_name::<#ty>() {
                    // SAFETY: type-name equality identifies the same concrete
                    // monomorphized Node without requiring a `'static` bound.
                    let node = unsafe { &*(node as *const N as *const #ty) };
                    return <Self as #facade::Visitor<#ty>>::enter(self, node);
                }
            }
        });
        let exit = configuration.dispatch.iter().map(|ty| {
            quote! {
                if ::core::any::type_name::<N>() == ::core::any::type_name::<#ty>() {
                    // SAFETY: see `total_enter` above.
                    let node = unsafe { &*(node as *const N as *const #ty) };
                    return <Self as #facade::Visitor<#ty>>::exit(self, node);
                }
            }
        });
        Ok(quote! {
            impl #impl_generics #facade::TotalVisitor for #name #type_generics #where_clause {
                type Error = #error;
                fn total_enter<N>(
                    &mut self,
                    node: &N,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>> {
                    #(#enter)*
                    ::core::ops::ControlFlow::Continue(())
                }
                fn total_exit<N>(
                    &mut self,
                    node: &N,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>> {
                    #(#exit)*
                    ::core::ops::ControlFlow::Continue(())
                }
            }
        })
    }
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn normalized_source_identity(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
            Component::ParentDir => Some("..".to_owned()),
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
        })
        .collect::<Vec<_>>()
        .join("/")
}
