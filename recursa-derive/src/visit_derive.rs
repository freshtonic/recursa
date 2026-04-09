use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn derive_visit(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let visit_body = match &input.data {
        Data::Struct(data) => derive_visit_struct(data)?,
        Data::Enum(data) => derive_visit_enum(data)?,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "Visit can only be derived for structs and enums",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics ::recursa_core::AsNodeKey for #name #ty_generics #where_clause {}

        impl #impl_generics ::recursa_core::Visit for #name #ty_generics #where_clause {
            fn visit<V: ::recursa_core::Visitor>(
                &self,
                visitor: &mut V,
            ) -> ::std::ops::ControlFlow<::recursa_core::Break<V::Error>> {
                match ::recursa_core::Visitor::enter(visitor, self) {
                    ::std::ops::ControlFlow::Continue(()) => {
                        #visit_body
                    }
                    ::std::ops::ControlFlow::Break(::recursa_core::Break::SkipChildren) => {}
                    other => return other,
                }
                ::recursa_core::Visitor::exit(visitor, self)
            }
        }
    })
}

fn derive_visit_struct(data: &syn::DataStruct) -> syn::Result<TokenStream> {
    let field_visits: Vec<_> = match &data.fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .map(|f| {
                let name = &f.ident;
                quote! { ::recursa_core::Visit::visit(&self.#name, visitor)?; }
            })
            .collect(),
        Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let idx = syn::Index::from(i);
                quote! { ::recursa_core::Visit::visit(&self.#idx, visitor)?; }
            })
            .collect(),
        Fields::Unit => vec![],
    };

    Ok(quote! { #(#field_visits)* })
}

fn derive_visit_enum(data: &syn::DataEnum) -> syn::Result<TokenStream> {
    let match_arms: Vec<_> = data
        .variants
        .iter()
        .map(|variant| {
            let vname = &variant.ident;
            match &variant.fields {
                Fields::Unnamed(fields) => {
                    let bindings: Vec<_> = (0..fields.unnamed.len())
                        .map(|i| {
                            syn::Ident::new(&format!("__f{i}"), proc_macro2::Span::call_site())
                        })
                        .collect();
                    let visits: Vec<_> = bindings
                        .iter()
                        .map(|b| {
                            quote! { ::recursa_core::Visit::visit(#b, visitor)?; }
                        })
                        .collect();
                    quote! {
                        Self::#vname(#(#bindings),*) => { #(#visits)* }
                    }
                }
                Fields::Named(fields) => {
                    let names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
                    let visits: Vec<_> = names
                        .iter()
                        .map(|n| {
                            quote! { ::recursa_core::Visit::visit(#n, visitor)?; }
                        })
                        .collect();
                    quote! {
                        Self::#vname { #(#names),* } => { #(#visits)* }
                    }
                }
                Fields::Unit => {
                    quote! { Self::#vname => {} }
                }
            }
        })
        .collect();

    Ok(quote! {
        match self {
            #(#match_arms)*
        }
    })
}
