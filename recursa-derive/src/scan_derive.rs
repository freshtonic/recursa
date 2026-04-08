use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr};

pub fn derive_scan(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let pattern = get_scan_pattern(&input)?;

    match &input.data {
        Data::Struct(data) => match &data.fields {
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
        },
        _ => Err(syn::Error::new_spanned(
            name,
            "Scan can only be derived for structs (enum Scan support is separate)",
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

fn derive_scan_unit_struct(
    name: &syn::Ident,
    pattern: &str,
    impl_generics: syn::ImplGenerics,
    ty_generics: syn::TypeGenerics,
    where_clause: Option<&syn::WhereClause>,
) -> syn::Result<TokenStream> {
    let anchored_pattern = format!(r"\A(?:{})", pattern);

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
    })
}

fn derive_scan_tuple_struct(
    _name: &syn::Ident,
    _pattern: &str,
    _generics: &syn::Generics,
) -> syn::Result<TokenStream> {
    // Placeholder -- implemented in Task 9
    Err(syn::Error::new_spanned(
        _name,
        "derive(Scan) for tuple structs is not yet implemented",
    ))
}
