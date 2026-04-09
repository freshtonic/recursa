use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr};

pub fn derive_scan(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    match &input.data {
        Data::Struct(data) => {
            let pattern = get_scan_pattern(&input)?;
            let generics = &input.generics;
            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
            match &data.fields {
                Fields::Unit => derive_scan_unit_struct(
                    name,
                    &pattern,
                    impl_generics,
                    ty_generics,
                    where_clause,
                ),
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    derive_scan_tuple_struct(name, &pattern, generics)
                }
                _ => Err(syn::Error::new_spanned(
                    name,
                    "Scan can only be derived for unit structs or single-field tuple structs",
                )),
            }
        }
        Data::Enum(data) => derive_scan_enum(name, &input.generics, data),
        _ => Err(syn::Error::new_spanned(
            name,
            "Scan cannot be derived for unions",
        )),
    }
}

fn get_scan_pattern(input: &DeriveInput) -> syn::Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("scan") {
            let mut pattern = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("pattern") {
                    let value: LitStr = meta.value()?.parse()?;
                    pattern = Some(value.value());
                    Ok(())
                } else {
                    Err(meta.error("expected `pattern`"))
                }
            })?;
            return pattern
                .ok_or_else(|| syn::Error::new_spanned(attr, "missing `pattern` in #[scan(...)]"));
        }
    }
    Err(syn::Error::new_spanned(
        &input.ident,
        "missing #[scan(pattern = \"...\")] attribute",
    ))
}

/// Generates a `Parse` impl that delegates to `Scan` for a given type.
fn generate_parse_for_scan(
    name: &syn::Ident,
    impl_generics: &syn::ImplGenerics,
    ty_generics: &syn::TypeGenerics,
    where_clause: Option<&syn::WhereClause>,
    lt: &proc_macro2::TokenStream,
) -> TokenStream {
    quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            type Rules = ::recursa_core::NoRules;
            const IS_TERMINAL: bool = true;

            fn first_pattern() -> &'static str {
                <Self as ::recursa_core::Scan<#lt>>::PATTERN
            }

            fn peek(input: &::recursa_core::Input<#lt, ::recursa_core::NoRules>) -> bool {
                <Self as ::recursa_core::Scan<#lt>>::peek(input)
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, ::recursa_core::NoRules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                <Self as ::recursa_core::Scan<#lt>>::parse(input)
            }
        }
    }
}

fn derive_scan_unit_struct(
    name: &syn::Ident,
    pattern: &str,
    impl_generics: syn::ImplGenerics,
    ty_generics: syn::TypeGenerics,
    where_clause: Option<&syn::WhereClause>,
) -> syn::Result<TokenStream> {
    let anchored_pattern = format!(r"\A(?:{})", pattern);

    let lt = quote! { '_ };
    let parse_impl = generate_parse_for_scan(name, &impl_generics, &ty_generics, where_clause, &lt);

    Ok(quote! {
        impl #impl_generics ::recursa_core::Scan<'_> for #name #ty_generics #where_clause {
            const PATTERN: &'static str = #pattern;

            fn regex() -> &'static ::regex::Regex {
                static REGEX: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
                REGEX.get_or_init(|| ::regex::Regex::new(#anchored_pattern).unwrap())
            }

            fn from_match(_matched: &str) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                Ok(#name)
            }
        }

        #parse_impl
    })
}

fn derive_scan_tuple_struct(
    name: &syn::Ident,
    pattern: &str,
    generics: &syn::Generics,
) -> syn::Result<TokenStream> {
    let anchored_pattern = format!(r"\A(?:{})", pattern);

    // Extract the lifetime parameter (tuple Scan structs must have one)
    let lifetime = generics.lifetimes().next().ok_or_else(|| {
        syn::Error::new_spanned(name, "tuple Scan structs must have a lifetime parameter")
    })?;
    let lt = &lifetime.lifetime;

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let lt_tokens = quote! { #lt };
    let parse_impl =
        generate_parse_for_scan(name, &impl_generics, &ty_generics, where_clause, &lt_tokens);

    Ok(quote! {
        impl #impl_generics ::recursa_core::Scan<#lt> for #name #ty_generics #where_clause {
            const PATTERN: &'static str = #pattern;

            fn regex() -> &'static ::regex::Regex {
                static REGEX: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
                REGEX.get_or_init(|| ::regex::Regex::new(#anchored_pattern).unwrap())
            }

            fn from_match(matched: &#lt str) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                Ok(#name(matched))
            }
        }

        #parse_impl
    })
}

fn derive_scan_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Each variant must be a single-field newtype where the inner type implements Scan
    let mut variant_names = Vec::new();
    let mut variant_types = Vec::new();

    for variant in &data.variants {
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => {
                return Err(syn::Error::new_spanned(
                    &variant.ident,
                    "Scan enum variants must be single-field newtypes",
                ));
            }
        };
        variant_names.push(&variant.ident);
        variant_types.push(inner_type);
    }

    let variant_indices: Vec<_> = (0..variant_names.len())
        .map(|i| syn::Ident::new(&format!("_{i}"), proc_macro2::Span::call_site()))
        .collect();

    // Generate arms to find the longest matching capture group
    let match_arms_for_len = variant_indices.iter().enumerate().map(|(i, idx)| {
        quote! {
            if let Some(m) = captures.name(stringify!(#idx)) {
                if m.len() > best_len {
                    best_len = m.len();
                    best_index = Some(#i);
                }
            }
        }
    });

    // Generate dispatch arms that construct the correct variant
    let dispatch_arms = variant_names
        .iter()
        .zip(variant_types.iter())
        .enumerate()
        .map(|(i, (vname, vtype))| {
            let idx = &variant_indices[i];
            quote! {
                Some(#i) => {
                    let m = captures.name(stringify!(#idx)).unwrap();
                    let matched_str = &input.source()[input.cursor()..input.cursor() + m.len()];
                    let result = <#vtype as ::recursa_core::Scan>::from_match(matched_str)?;
                    input.advance(m.len());
                    Ok(#name::#vname(result))
                }
            }
        });

    // Build the combined pattern at runtime from variant PATTERN constants
    let pattern_parts = variant_types.iter().zip(variant_indices.iter()).map(|(vtype, idx)| {
        quote! {
            parts.push(format!("(?P<{}>{})", stringify!(#idx), <#vtype as ::recursa_core::Scan>::PATTERN));
        }
    });

    let lt_tokens = quote! { #lt };
    let parse_impl =
        generate_parse_for_scan(name, &impl_generics, &ty_generics, where_clause, &lt_tokens);

    Ok(quote! {
        impl #impl_generics ::recursa_core::Scan<#lt> for #name #ty_generics #where_clause {
            const PATTERN: &'static str = ""; // Combined pattern is built at runtime

            fn regex() -> &'static ::regex::Regex {
                static REGEX: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
                REGEX.get_or_init(|| {
                    let mut parts = Vec::new();
                    #(#pattern_parts)*
                    let combined = format!(r"\A(?:{})", parts.join("|"));
                    ::regex::Regex::new(&combined).unwrap()
                })
            }

            fn from_match(_matched: &#lt str) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                unimplemented!("use parse() for enum Scan types")
            }

            fn peek(input: &::recursa_core::Input<#lt, ::recursa_core::NoRules>) -> bool {
                Self::regex().is_match(input.remaining())
            }

            fn parse(input: &mut ::recursa_core::Input<#lt, ::recursa_core::NoRules>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let captures = match Self::regex().captures(input.remaining()) {
                    Some(c) => c,
                    None => {
                        return Err(::recursa_core::ParseError::new(
                            input.source().to_string(),
                            input.cursor()..input.cursor(),
                            stringify!(#name),
                        ));
                    }
                };

                // Find longest match (maximal munch), declaration order as tiebreaker
                let mut best_len = 0usize;
                let mut best_index: Option<usize> = None;
                #(#match_arms_for_len)*

                match best_index {
                    #(#dispatch_arms)*
                    _ => Err(::recursa_core::ParseError::new(
                        input.source().to_string(),
                        input.cursor()..input.cursor(),
                        stringify!(#name),
                    )),
                }
            }
        }

        #parse_impl
    })
}
