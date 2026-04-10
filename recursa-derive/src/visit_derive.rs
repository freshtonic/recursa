use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// Parse the `#[visit(...)]` attribute to determine visit mode.
enum VisitMode {
    /// Normal: enter, visit children, exit.
    Normal,
    /// Terminal: enter and exit, but do NOT visit children.
    /// Use for data-carrying tokens (e.g., `StringLit(String)`).
    Terminal,
    /// Ignore: completely transparent to visitors. No enter/exit calls.
    /// Use for tokens that carry no information (keywords, punctuation).
    Ignore,
}

fn get_visit_mode(input: &DeriveInput) -> syn::Result<VisitMode> {
    for attr in &input.attrs {
        if attr.path().is_ident("visit") {
            let mut mode = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("terminal") {
                    mode = Some(VisitMode::Terminal);
                    Ok(())
                } else if meta.path.is_ident("ignore") {
                    mode = Some(VisitMode::Ignore);
                    Ok(())
                } else {
                    Err(meta.error("expected `terminal` or `ignore`"))
                }
            })?;
            if let Some(mode) = mode {
                return Ok(mode);
            }
        }
    }
    Ok(VisitMode::Normal)
}

pub fn derive_visit(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mode = get_visit_mode(&input)?;

    let visit_fn_body = match mode {
        VisitMode::Ignore => {
            // Completely transparent — no enter/exit
            quote! {
                ::std::ops::ControlFlow::Continue(())
            }
        }
        VisitMode::Terminal => {
            // Enter and exit, but no child traversal
            quote! {
                match ::recursa_core::Visitor::enter(visitor, self) {
                    ::std::ops::ControlFlow::Continue(()) | ::std::ops::ControlFlow::Break(::recursa_core::Break::SkipChildren) => {}
                    other => return other,
                }
                ::recursa_core::Visitor::exit(visitor, self)
            }
        }
        VisitMode::Normal => {
            let children_body = match &input.data {
                Data::Struct(data) => derive_visit_struct(data)?,
                Data::Enum(data) => derive_visit_enum(data)?,
                _ => {
                    return Err(syn::Error::new_spanned(
                        name,
                        "Visit can only be derived for structs and enums",
                    ));
                }
            };
            quote! {
                match ::recursa_core::Visitor::enter(visitor, self) {
                    ::std::ops::ControlFlow::Continue(()) => {
                        #children_body
                    }
                    ::std::ops::ControlFlow::Break(::recursa_core::Break::SkipChildren) => {}
                    other => return other,
                }
                ::recursa_core::Visitor::exit(visitor, self)
            }
        }
    };

    Ok(quote! {
        impl #impl_generics ::recursa_core::AsNodeKey for #name #ty_generics #where_clause {}

        impl #impl_generics ::recursa_core::Visit for #name #ty_generics #where_clause {
            fn visit<V: ::recursa_core::Visitor>(
                &self,
                visitor: &mut V,
            ) -> ::std::ops::ControlFlow<::recursa_core::Break<V::Error>> {
                #visit_fn_body
            }
        }
    })
}

fn derive_visit_struct(data: &syn::DataStruct) -> syn::Result<TokenStream> {
    let field_visits: Vec<_> = match &data.fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .filter(|f| {
                // Skip PhantomData fields (they have no data to visit)
                !is_phantom_data_type(&f.ty)
            })
            .map(|f| {
                let name = &f.ident;
                quote! { ::recursa_core::Visit::visit(&self.#name, visitor)?; }
            })
            .collect(),
        Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .enumerate()
            .filter(|(_, f)| !is_phantom_data_type(&f.ty))
            .map(|(i, _)| {
                let idx = syn::Index::from(i);
                quote! { ::recursa_core::Visit::visit(&self.#idx, visitor)?; }
            })
            .collect(),
        Fields::Unit => vec![],
    };

    Ok(quote! { #(#field_visits)* })
}

/// Check if a type is `PhantomData<...>` (no data to visit).
fn is_phantom_data_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident == "PhantomData";
    }
    false
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
