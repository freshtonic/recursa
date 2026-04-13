use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Path};

/// Parse `#[total_visitor(dispatch = [Type1, Type2, ...], error = ErrorType)]`.
struct TotalVisitorAttrs {
    dispatch: Vec<Path>,
    error: syn::Type,
}

fn parse_total_visitor_attrs(input: &DeriveInput) -> syn::Result<TotalVisitorAttrs> {
    let mut dispatch = Vec::new();
    let mut error: Option<syn::Type> = None;

    for attr in &input.attrs {
        if attr.path().is_ident("total_visitor") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("dispatch") {
                    let _eq: syn::Token![=] = meta.input.parse()?;
                    let content;
                    syn::bracketed!(content in meta.input);
                    let types = content.parse_terminated(Path::parse_mod_style, syn::Token![,])?;
                    dispatch = types.into_iter().collect();
                    Ok(())
                } else if meta.path.is_ident("error") {
                    let value = meta.value()?;
                    error = Some(value.parse::<syn::Type>()?);
                    Ok(())
                } else {
                    Err(meta.error("expected `dispatch` or `error`"))
                }
            })?;
        }
    }

    let error = error.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "missing `error` in #[total_visitor(...)]")
    })?;

    if dispatch.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "missing `dispatch` in #[total_visitor(...)]",
        ));
    }

    Ok(TotalVisitorAttrs { dispatch, error })
}

pub fn derive_total_visitor(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let attrs = parse_total_visitor_attrs(&input)?;
    let error_type = &attrs.error;

    // Generate TypeId-based dispatch arms for total_enter
    let enter_arms: Vec<_> = attrs
        .dispatch
        .iter()
        .map(|ty| {
            quote! {
                if ::std::any::TypeId::of::<N>() == ::std::any::TypeId::of::<#ty>() {
                    // SAFETY: TypeId match guarantees N is #ty
                    let node = unsafe { &*(node as *const N as *const #ty) };
                    return <Self as ::recursa_core::Visitor<#ty>>::enter(self, node);
                }
            }
        })
        .collect();

    // Generate TypeId-based dispatch arms for total_exit
    let exit_arms: Vec<_> = attrs
        .dispatch
        .iter()
        .map(|ty| {
            quote! {
                if ::std::any::TypeId::of::<N>() == ::std::any::TypeId::of::<#ty>() {
                    let node = unsafe { &*(node as *const N as *const #ty) };
                    return <Self as ::recursa_core::Visitor<#ty>>::exit(self, node);
                }
            }
        })
        .collect();

    Ok(quote! {
        impl #impl_generics ::recursa_core::TotalVisitor for #name #ty_generics #where_clause {
            type Error = #error_type;

            fn total_enter<N: 'static>(&mut self, node: &N) -> ::std::ops::ControlFlow<::recursa_core::Break<Self::Error>> {
                #(#enter_arms)*
                ::std::ops::ControlFlow::Continue(())
            }

            fn total_exit<N: 'static>(&mut self, node: &N) -> ::std::ops::ControlFlow<::recursa_core::Break<Self::Error>> {
                #(#exit_arms)*
                ::std::ops::ControlFlow::Continue(())
            }
        }
    })
}
