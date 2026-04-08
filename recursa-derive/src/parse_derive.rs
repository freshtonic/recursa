use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type};

pub fn derive_parse(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    let rules_type = get_rules_type(&input)?;

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                derive_parse_struct(name, &input.generics, &rules_type, fields)
            }
            _ => Err(syn::Error::new_spanned(
                name,
                "Parse can only be derived for structs with named fields",
            )),
        },
        Data::Enum(data) => derive_parse_enum(name, &input.generics, &rules_type, data),
        _ => Err(syn::Error::new_spanned(
            name,
            "Parse can only be derived for structs and enums",
        )),
    }
}

fn get_rules_type(input: &DeriveInput) -> syn::Result<Type> {
    for attr in &input.attrs {
        if attr.path().is_ident("parse") {
            let mut rules = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rules") {
                    let value = meta.value()?;
                    let ty: Type = value.parse()?;
                    rules = Some(ty);
                    Ok(())
                } else {
                    Err(meta.error("expected `rules`"))
                }
            })?;
            if let Some(rules) = rules {
                return Ok(rules);
            }
        }
    }
    Err(syn::Error::new_spanned(
        &input.ident,
        "missing #[parse(rules = ...)] attribute",
    ))
}

fn derive_parse_struct(
    name: &syn::Ident,
    generics: &syn::Generics,
    rules_type: &Type,
    fields: &syn::FieldsNamed,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Determine the lifetime to use for Parse<'input>
    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let field_names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();
    let first_field_type = field_types.first().ok_or_else(|| {
        syn::Error::new_spanned(name, "Parse struct must have at least one field")
    })?;

    // Generate the parse body: consume_ignored + rebind + parse each field
    let parse_fields = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(name, ty)| {
            quote! {
                fork.consume_ignored();
                let #name = {
                    let mut rebound = fork.rebind::<<#ty as ::recursa_core::Parse>::Rules>();
                    let result = <#ty as ::recursa_core::Parse>::parse(&mut rebound)?;
                    fork.commit(rebound.rebind());
                    result
                };
            }
        });

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            type Rules = #rules_type;

            fn peek(input: &::recursa_core::Input<#lt, Self::Rules>) -> bool {
                let rebound = input.rebind::<<#first_field_type as ::recursa_core::Parse>::Rules>();
                <#first_field_type as ::recursa_core::Parse>::peek(&rebound)
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, Self::Rules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let mut fork = input.fork();
                #(#parse_fields)*
                input.commit(fork);
                Ok(Self { #(#field_names),* })
            }
        }
    })
}

fn derive_parse_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    rules_type: &Type,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    // Each variant must be a single-field newtype: Variant(InnerType)
    let mut peek_arms = Vec::new();
    let mut parse_arms = Vec::new();

    for variant in &data.variants {
        let variant_name = &variant.ident;
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => {
                return Err(syn::Error::new_spanned(
                    variant_name,
                    "Parse enum variants must be single-field newtypes, e.g. Variant(InnerType)",
                ));
            }
        };

        peek_arms.push(quote! {
            {
                let rebound = input.rebind::<<#inner_type as ::recursa_core::Parse>::Rules>();
                if <#inner_type as ::recursa_core::Parse>::peek(&rebound) {
                    return true;
                }
            }
        });

        parse_arms.push(quote! {
            {
                let rebound = fork.rebind::<<#inner_type as ::recursa_core::Parse>::Rules>();
                if <#inner_type as ::recursa_core::Parse>::peek(&rebound) {
                    let mut rebound = fork.rebind::<<#inner_type as ::recursa_core::Parse>::Rules>();
                    match <#inner_type as ::recursa_core::Parse>::parse(&mut rebound) {
                        Ok(inner) => {
                            input.commit(rebound.rebind());
                            return Ok(#name::#variant_name(inner));
                        }
                        Err(e) => errors.push(e),
                    }
                }
            }
        });
    }

    // Build error arms for when no variant matches via peek
    let variant_names: Vec<_> = data.variants.iter().map(|v| &v.ident).collect();
    let error_labels: Vec<String> = variant_names.iter().map(|v| v.to_string()).collect();

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            type Rules = #rules_type;

            fn peek(input: &::recursa_core::Input<#lt, Self::Rules>) -> bool {
                #(#peek_arms)*
                false
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, Self::Rules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let fork = input.fork();
                let mut errors = ::std::vec::Vec::new();
                #(#parse_arms)*
                // None matched -- collect errors for all variants
                if errors.is_empty() {
                    #(
                        errors.push(::recursa_core::ParseError::new(
                            fork.source().to_string(),
                            fork.cursor()..fork.cursor(),
                            #error_labels,
                        ));
                    )*
                }
                Err(::recursa_core::ParseError::merge(errors))
            }
        }
    })
}
