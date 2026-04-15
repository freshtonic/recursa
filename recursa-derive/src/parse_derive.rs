use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Type};

/// Parsed `#[parse(...)]` attribute on the top-level derive input.
struct ParseAttrs {
    rules: Option<Type>,
    pattern: Option<String>,
    case_insensitive: bool,
    postcondition: Option<syn::Path>,
    pratt: bool,
}

fn parse_top_attrs(input: &DeriveInput) -> syn::Result<ParseAttrs> {
    let mut out = ParseAttrs {
        rules: None,
        pattern: None,
        case_insensitive: false,
        postcondition: None,
        pratt: false,
    };
    for attr in &input.attrs {
        if !attr.path().is_ident("parse") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rules") {
                let ty: Type = meta.value()?.parse()?;
                out.rules = Some(ty);
            } else if meta.path.is_ident("pattern") {
                let lit: LitStr = meta.value()?.parse()?;
                out.pattern = Some(lit.value());
            } else if meta.path.is_ident("case_insensitive") {
                out.case_insensitive = true;
            } else if meta.path.is_ident("postcondition") {
                let path: syn::Path = meta.value()?.parse()?;
                out.postcondition = Some(path);
            } else if meta.path.is_ident("pratt") {
                out.pratt = true;
            } else {
                return Err(meta.error(
                    "expected `rules`, `pattern`, `case_insensitive`, `postcondition`, or `pratt`",
                ));
            }
            Ok(())
        })?;
    }
    if out.case_insensitive
        && let Some(p) = &out.pattern
    {
        out.pattern = Some(format!("(?i:{})", p));
    }
    Ok(out)
}

/// Rules type expression. If no `rules = ...` given, falls back to `NoRules`.
fn rules_ty(attrs: &ParseAttrs) -> TokenStream {
    match &attrs.rules {
        Some(ty) => quote! { #ty },
        None => quote! { ::recursa_core::NoRules },
    }
}

pub fn derive_parse(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let attrs = parse_top_attrs(&input)?;

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unit => {
                let pattern = attrs.pattern.as_ref().ok_or_else(|| {
                    syn::Error::new_spanned(
                        name,
                        "unit struct Parse derive requires #[parse(pattern = \"...\")]",
                    )
                })?;
                derive_scanner_unit(name, pattern, &attrs.postcondition)
            }
            Fields::Unnamed(fields) if attrs.pattern.is_some() => {
                if fields.unnamed.len() != 1 {
                    return Err(syn::Error::new_spanned(
                        name,
                        "scanner-style tuple struct must have exactly one field",
                    ));
                }
                derive_scanner_tuple(
                    name,
                    attrs.pattern.as_ref().unwrap(),
                    &input.generics,
                    &fields.unnamed[0].ty,
                    &attrs.postcondition,
                )
            }
            Fields::Named(fields) => derive_named_struct(name, &input.generics, &attrs, fields),
            Fields::Unnamed(fields) => derive_tuple_struct(name, &input.generics, &attrs, fields),
        },
        Data::Enum(data) => {
            if attrs.pratt {
                derive_pratt_enum(name, &input.generics, &attrs, data)
            } else {
                derive_choice_enum(name, &input.generics, &attrs, data, &attrs.postcondition)
            }
        }
        _ => Err(syn::Error::new_spanned(
            name,
            "Parse can only be derived for structs and enums",
        )),
    }
}

// ---------------------------------------------------------------------------
// Scanner-style derives (unit / tuple struct with #[parse(pattern = ...)])
// ---------------------------------------------------------------------------

fn scanner_regex_init(anchored: &str) -> TokenStream {
    quote! {
        static REGEX_CELL: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
        let regex = REGEX_CELL.get_or_init(|| ::regex::Regex::new(#anchored).unwrap());
    }
}

fn scanner_parse_body(
    pattern: &str,
    anchored: &str,
    construct: TokenStream,
    postcondition: &Option<syn::Path>,
) -> TokenStream {
    let init = scanner_regex_init(anchored);
    let postcheck = postcondition.as_ref().map(|p| quote! { #p(&result)?; });
    quote! {
        #init
        match regex.find(input.remaining()) {
            ::std::option::Option::Some(m) if m.start() == 0 => {
                let matched = &input.source()[input.cursor()..input.cursor() + m.len()];
                let result = #construct;
                #postcheck
                input.advance(m.len());
                ::std::result::Result::Ok(result)
            }
            _ => ::std::result::Result::Err(::recursa_core::ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                #pattern,
            )),
        }
    }
}

fn scanner_peek_body(anchored: &str, postcondition: &Option<syn::Path>) -> TokenStream {
    if postcondition.is_some() {
        quote! {
            let mut fork = input.fork();
            <Self as ::recursa_core::Parse>::parse::<R>(&mut fork).is_ok()
        }
    } else {
        let init = scanner_regex_init(anchored);
        quote! {
            #init
            regex.find(input.remaining()).is_some_and(|m| m.start() == 0)
        }
    }
}

fn derive_scanner_unit(
    name: &syn::Ident,
    pattern: &str,
    postcondition: &Option<syn::Path>,
) -> syn::Result<TokenStream> {
    let anchored = format!(r"\A(?:{})", pattern);
    let peek_body = scanner_peek_body(&anchored, postcondition);
    let parse_body = scanner_parse_body(
        pattern,
        &anchored,
        quote! { { let _ = matched; #name } },
        postcondition,
    );

    Ok(quote! {
        impl<'input> ::recursa_core::Parse<'input> for #name {
            fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<'input>) -> bool {
                #peek_body
            }

            fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<'input>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                #parse_body
            }
        }
    })
}

/// Detects whether a scanner-tuple field type is `Cow<...>` (any path ending
/// in `Cow`). Used to decide whether to wrap the matched slice in
/// `Cow::Borrowed` or pass it through as a `&str`.
fn is_cow_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty
        && let Some(seg) = type_path.path.segments.last()
    {
        return seg.ident == "Cow";
    }
    false
}

fn derive_scanner_tuple(
    name: &syn::Ident,
    pattern: &str,
    generics: &syn::Generics,
    field_ty: &Type,
    postcondition: &Option<syn::Path>,
) -> syn::Result<TokenStream> {
    let anchored = format!(r"\A(?:{})", pattern);
    let peek_body = scanner_peek_body(&anchored, postcondition);

    if let Some(lifetime) = generics.lifetimes().next() {
        let lt = &lifetime.lifetime;
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let construct = if is_cow_type(field_ty) {
            quote! { #name(::std::borrow::Cow::Borrowed(matched)) }
        } else {
            quote! { #name(matched) }
        };
        let parse_body = scanner_parse_body(pattern, &anchored, construct, postcondition);

        Ok(quote! {
            impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
                fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#lt>) -> bool {
                    #peek_body
                }

                fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#lt>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                    #parse_body
                }
            }
        })
    } else {
        let parse_body = scanner_parse_body(
            pattern,
            &anchored,
            quote! { #name(matched.to_string()) },
            postcondition,
        );

        Ok(quote! {
            impl<'input> ::recursa_core::Parse<'input> for #name {
                fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<'input>) -> bool {
                    #peek_body
                }

                fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<'input>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                    #parse_body
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Struct derives (sequence parse)
// ---------------------------------------------------------------------------

/// Returns `true` if `ty` is syntactically `Box<Name>` or `Box<Name<...>>`
/// where `Name` matches the given enum identifier. Used by the Pratt
/// postfix derive to recognize recursive self-fields that should be parsed
/// through `parse_expr` with a caller-supplied `min_bp`.
fn is_box_of_self(ty: &Type, name: &syn::Ident) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    let Some(syn::GenericArgument::Type(Type::Path(inner))) = args.args.first() else {
        return false;
    };
    inner.path.segments.last().is_some_and(|s| s.ident == *name)
}

fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident == "Option";
    }
    false
}

/// Generate peek body for a struct: walk leading Option fields, then the first required.
fn generate_struct_peek_body(field_types: &[&Type]) -> TokenStream {
    let first_required = field_types.iter().position(|ty| !is_option_type(ty));
    match first_required {
        Some(0) => {
            let first = &field_types[0];
            quote! { <#first as ::recursa_core::Parse>::peek::<R>(&peek_input) }
        }
        Some(idx) => {
            let option_checks: Vec<_> = field_types[..idx]
                .iter()
                .map(|ty| {
                    quote! {
                        if <#ty as ::recursa_core::Parse>::peek::<R>(&peek_input) {
                            return true;
                        }
                    }
                })
                .collect();
            let required = &field_types[idx];
            quote! {
                #(#option_checks)*
                <#required as ::recursa_core::Parse>::peek::<R>(&peek_input)
            }
        }
        None => {
            let checks: Vec<_> = field_types
                .iter()
                .map(|ty| {
                    quote! {
                        if <#ty as ::recursa_core::Parse>::peek::<R>(&peek_input) {
                            return true;
                        }
                    }
                })
                .collect();
            quote! {
                #(#checks)*
                false
            }
        }
    }
}

fn derive_named_struct(
    name: &syn::Ident,
    generics: &syn::Generics,
    attrs: &ParseAttrs,
    fields: &syn::FieldsNamed,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let field_names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();
    if field_types.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "Parse struct must have at least one field",
        ));
    }
    let rules = rules_ty(attrs);
    let peek_body = generate_struct_peek_body(&field_types);
    let parse_fields = field_names.iter().zip(field_types.iter()).map(|(n, ty)| {
        quote! {
            <#rules as ::recursa_core::ParseRules>::consume_ignored(&mut fork);
            let #n = <#ty as ::recursa_core::Parse>::parse::<#rules>(&mut fork)?;
        }
    });

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#lt>) -> bool {
                let mut peek_input = input.fork();
                <#rules as ::recursa_core::ParseRules>::consume_ignored(&mut peek_input);
                #peek_body
            }

            fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#lt>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let mut fork = input.fork();
                #(#parse_fields)*
                input.commit(fork);
                Ok(Self { #(#field_names),* })
            }
        }
    })
}

fn derive_tuple_struct(
    name: &syn::Ident,
    generics: &syn::Generics,
    attrs: &ParseAttrs,
    fields: &syn::FieldsUnnamed,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
    if field_types.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "Parse tuple struct must have at least one field",
        ));
    }
    let rules = rules_ty(attrs);
    let peek_body = generate_struct_peek_body(&field_types);
    let field_bindings: Vec<_> = (0..field_types.len())
        .map(|i| syn::Ident::new(&format!("__f{i}"), proc_macro2::Span::call_site()))
        .collect();

    let parse_fields = field_bindings
        .iter()
        .zip(field_types.iter())
        .map(|(b, ty)| {
            quote! {
                <#rules as ::recursa_core::ParseRules>::consume_ignored(&mut fork);
                let #b = <#ty as ::recursa_core::Parse>::parse::<#rules>(&mut fork)?;
            }
        });

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#lt>) -> bool {
                let mut peek_input = input.fork();
                <#rules as ::recursa_core::ParseRules>::consume_ignored(&mut peek_input);
                #peek_body
            }

            fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#lt>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                let mut fork = input.fork();
                #(#parse_fields)*
                input.commit(fork);
                Ok(Self(#(#field_bindings),*))
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Enum derives (choice)
// ---------------------------------------------------------------------------

fn derive_choice_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    attrs: &ParseAttrs,
    data: &syn::DataEnum,
    postcondition: &Option<syn::Path>,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let impl_lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    let mut variant_names = Vec::new();
    let mut inner_types = Vec::new();

    for variant in &data.variants {
        let vname = &variant.ident;
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => {
                return Err(syn::Error::new_spanned(
                    vname,
                    "Parse enum variants must be single-field tuple variants, e.g. Variant(InnerType).",
                ));
            }
        };
        variant_names.push(vname.clone());
        inner_types.push(inner_type.clone());
    }

    let rules = rules_ty(attrs);
    let error_labels: Vec<String> = variant_names.iter().map(|v| v.to_string()).collect();

    let peek_arms = inner_types.iter().map(|ty| {
        quote! {
            if <#ty as ::recursa_core::Parse>::peek::<R>(&peek_input) {
                return true;
            }
        }
    });

    // For each variant: peek, then fork-and-try parse. If parse fails after a
    // successful peek, fall through to the next variant (mirrors Option<T>'s
    // blanket impl and is the only way to make disambiguation work when multiple
    // variants share a first-token peek).
    let parse_arms = inner_types
        .iter()
        .zip(variant_names.iter())
        .map(|(ty, vname)| {
            quote! {
                if <#ty as ::recursa_core::Parse>::peek::<#rules>(input) {
                    let mut fork = input.fork();
                    if let ::std::result::Result::Ok(inner) =
                        <#ty as ::recursa_core::Parse>::parse::<#rules>(&mut fork)
                    {
                        input.commit(fork);
                        return ::std::result::Result::Ok(#name::#vname(inner));
                    }
                }
            }
        });

    // Build parse arms that capture the result for postcondition, or return directly.
    let parse_arms_vec: Vec<_> = parse_arms.collect();
    let peek_arms_vec: Vec<_> = peek_arms.collect();

    let (peek_impl, parse_impl) = if let Some(pc) = postcondition {
        // Peek: fork-and-run full parse (includes postcondition check).
        let peek_impl = quote! {
            fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#impl_lt>) -> bool {
                let mut fork = input.fork();
                <Self as ::recursa_core::Parse>::parse::<R>(&mut fork).is_ok()
            }
        };
        // Parse: try each variant, capture result, run postcondition.
        let capture_parse_arms = inner_types
            .iter()
            .zip(variant_names.iter())
            .map(|(ty, vname)| {
                quote! {
                    if <#ty as ::recursa_core::Parse>::peek::<#rules>(input) {
                        let mut fork = input.fork();
                        if let ::std::result::Result::Ok(inner) =
                            <#ty as ::recursa_core::Parse>::parse::<#rules>(&mut fork)
                        {
                            let result = #name::#vname(inner);
                            if #pc(&result).is_ok() {
                                input.commit(fork);
                                return ::std::result::Result::Ok(result);
                            }
                        }
                    }
                }
            });
        let parse_impl = quote! {
            fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#impl_lt>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                <#rules as ::recursa_core::ParseRules>::consume_ignored(input);
                #(#capture_parse_arms)*
                let mut errors = ::std::vec::Vec::new();
                #(
                    errors.push(::recursa_core::ParseError::new(
                        input.source().to_string(),
                        input.cursor()..input.cursor(),
                        #error_labels,
                    ));
                )*
                ::std::result::Result::Err(::recursa_core::ParseError::merge(errors))
            }
        };
        (peek_impl, parse_impl)
    } else {
        let peek_impl = quote! {
            fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#impl_lt>) -> bool {
                let mut peek_input = input.fork();
                <#rules as ::recursa_core::ParseRules>::consume_ignored(&mut peek_input);
                #(#peek_arms_vec)*
                false
            }
        };
        let parse_impl = quote! {
            fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#impl_lt>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                <#rules as ::recursa_core::ParseRules>::consume_ignored(input);
                #(#parse_arms_vec)*
                let mut errors = ::std::vec::Vec::new();
                #(
                    errors.push(::recursa_core::ParseError::new(
                        input.source().to_string(),
                        input.cursor()..input.cursor(),
                        #error_labels,
                    ));
                )*
                ::std::result::Result::Err(::recursa_core::ParseError::merge(errors))
            }
        };
        (peek_impl, parse_impl)
    };

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#impl_lt> for #name #ty_generics #where_clause {
            #peek_impl
            #parse_impl
        }
    })
}

// ---------------------------------------------------------------------------
// Pratt enum derive
// ---------------------------------------------------------------------------

fn derive_pratt_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    attrs: &ParseAttrs,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let fn_lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'__input", proc_macro2::Span::call_site()));
    let impl_lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));
    let rules = rules_ty(attrs);

    let mut atom_variants = Vec::new();
    let mut prefix_variants = Vec::new();
    let mut postfix_variants = Vec::new();
    let mut infix_variants = Vec::new();

    for variant in &data.variants {
        let vname = &variant.ident;
        let kind = parse_pratt_attrs(&variant.attrs)?;
        let fields: Vec<_> = match &variant.fields {
            Fields::Unnamed(f) => f.unnamed.iter().collect(),
            _ => {
                return Err(syn::Error::new_spanned(
                    vname,
                    "Pratt enum variants must use tuple fields",
                ));
            }
        };

        match kind {
            PrattKind::Atom => {
                if fields.len() != 1 {
                    return Err(syn::Error::new_spanned(
                        vname,
                        "atom variants must have exactly one field",
                    ));
                }
                atom_variants.push((vname.clone(), fields[0].ty.clone()));
            }
            PrattKind::Prefix { bp } => {
                if fields.len() != 2 {
                    return Err(syn::Error::new_spanned(
                        vname,
                        "prefix variants must have exactly two fields",
                    ));
                }
                prefix_variants.push((vname.clone(), fields[0].ty.clone(), bp));
            }
            PrattKind::Postfix { bp, inner_bp } => {
                if fields.len() < 2 {
                    return Err(syn::Error::new_spanned(
                        vname,
                        "postfix variants must have at least two fields",
                    ));
                }
                let all_field_types: Vec<_> = fields.iter().map(|f| f.ty.clone()).collect();
                postfix_variants.push((vname.clone(), all_field_types, bp, inner_bp));
            }
            PrattKind::Infix { bp, right_assoc } => {
                if fields.len() != 3 {
                    return Err(syn::Error::new_spanned(
                        vname,
                        "infix variants must have exactly three fields",
                    ));
                }
                infix_variants.push((vname.clone(), fields[1].ty.clone(), bp, right_assoc));
            }
        }
    }

    // Atom try-in-order arms for nud. Fork-and-try each atom so that a failing
    // parse (after peek success) falls through to the next atom.
    let atom_try_arms = atom_variants.iter().map(|(vname, ty)| {
        quote! {
            if <#ty as ::recursa_core::Parse>::peek::<#rules>(input) {
                let mut fork = input.fork();
                if let ::std::result::Result::Ok(inner) =
                    <#ty as ::recursa_core::Parse>::parse::<#rules>(&mut fork)
                {
                    input.commit(fork);
                    break 'nud #name::#vname(inner);
                }
            }
        }
    });

    let prefix_parse_arms = prefix_variants.iter().map(|(vname, op_ty, bp)| {
        quote! {
            if <#op_ty as ::recursa_core::Parse>::peek::<#rules>(input) {
                let op = <#op_ty as ::recursa_core::Parse>::parse::<#rules>(input)?;
                let rhs = parse_expr(input, #bp)?;
                break 'nud #name::#vname(op, Box::new(rhs));
            }
        }
    });

    // For the top-level peek, we also try each atom/prefix in declaration order.
    let atom_peek_arms = atom_variants.iter().map(|(_, ty)| {
        quote! {
            if <#ty as ::recursa_core::Parse>::peek::<R>(input) {
                return true;
            }
        }
    });
    let prefix_peek_arms = prefix_variants.iter().map(|(_, op_ty, _)| {
        quote! {
            if <#op_ty as ::recursa_core::Parse>::peek::<R>(input) {
                return true;
            }
        }
    });

    let postfix_arms = postfix_variants
        .iter()
        .map(|(vname, field_types, bp, inner_bp)| {
            let op_ty = &field_types[1];
            let remaining_types = &field_types[2..];

            // Build the sequence of field parses that runs inside a fork. Each
            // parse uses `?` against the fork's IIFE result, so any failure
            // falls through to the next postfix/infix arm rather than aborting
            // `parse_expr`. This mirrors the atom try-in-order behavior and
            // allows postfix variants with overlapping first-token peeks
            // (e.g. `NOT IN` vs `NOT BETWEEN`) to disambiguate by trying each
            // in declaration order.
            let mut field_parses = Vec::new();
            let mut field_idents = Vec::new();
            let op_ident = syn::Ident::new("__f1", proc_macro2::Span::call_site());
            field_parses.push(quote! {
                let #op_ident = <#op_ty as ::recursa_core::Parse>::parse::<#rules>(&mut fork)?;
            });
            field_idents.push(op_ident);
            for (i, ty) in remaining_types.iter().enumerate() {
                let ident =
                    syn::Ident::new(&format!("__f{}", i + 2), proc_macro2::Span::call_site());
                // If this field is `Box<Self>` and the variant specifies `inner_bp`,
                // recurse via `parse_expr(&mut fork, inner_bp)` instead of the
                // plain `Parse::parse`, which would re-enter at `min_bp = 0` and
                // greedily consume infix operators that should belong to the
                // outer postfix.
                let parse_call = if let Some(ibp) = inner_bp
                    && is_box_of_self(ty, name)
                {
                    quote! {
                        let #ident = ::std::boxed::Box::new(parse_expr(&mut fork, #ibp)?);
                    }
                } else {
                    quote! {
                        let #ident = <#ty as ::recursa_core::Parse>::parse::<#rules>(&mut fork)?;
                    }
                };
                field_parses.push(quote! {
                    <#rules as ::recursa_core::ParseRules>::consume_ignored(&mut fork);
                    #parse_call
                });
                field_idents.push(ident);
            }
            let all_idents = &field_idents;
            quote! {
                {
                    <#rules as ::recursa_core::ParseRules>::consume_ignored(input);
                    if <#op_ty as ::recursa_core::Parse>::peek::<#rules>(input) && #bp >= min_bp {
                        let attempt: ::std::result::Result<_, ::recursa_core::ParseError> = (|| {
                            let mut fork = input.fork();
                            #(#field_parses)*
                            ::std::result::Result::Ok((fork, #(#all_idents),*))
                        })();
                        if let ::std::result::Result::Ok((fork, #(#all_idents),*)) = attempt {
                            input.commit(fork);
                            lhs = #name::#vname(Box::new(lhs), #(#all_idents),*);
                            continue;
                        }
                    }
                }
            }
        });

    let infix_arms = infix_variants
        .iter()
        .map(|(vname, op_ty, bp, right_assoc)| {
            let right_bp: u32 = if *right_assoc { *bp } else { bp + 1 };
            quote! {
                {
                    <#rules as ::recursa_core::ParseRules>::consume_ignored(input);
                    if <#op_ty as ::recursa_core::Parse>::peek::<#rules>(input) && #bp >= min_bp {
                        let op = <#op_ty as ::recursa_core::Parse>::parse::<#rules>(input)?;
                        let rhs = parse_expr(input, #right_bp)?;
                        lhs = #name::#vname(Box::new(lhs), op, Box::new(rhs));
                        continue;
                    }
                }
            }
        });

    Ok(quote! {
        const _: () = {
            fn parse_expr<#fn_lt>(
                input: &mut ::recursa_core::Input<#fn_lt>,
                min_bp: u32,
            ) -> ::std::result::Result<#name #ty_generics, ::recursa_core::ParseError> {
                <#rules as ::recursa_core::ParseRules>::consume_ignored(input);

                let mut lhs = 'nud: {
                    #(#prefix_parse_arms)*
                    #(#atom_try_arms)*
                    return Err(::recursa_core::ParseError::new(
                        input.source().to_string(),
                        input.cursor()..input.cursor(),
                        stringify!(#name),
                    ));
                };

                loop {
                    #(#postfix_arms)*
                    #(#infix_arms)*
                    break;
                }

                Ok(lhs)
            }

            impl #impl_generics ::recursa_core::Parse<#impl_lt> for #name #ty_generics #where_clause {
                fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#impl_lt>) -> bool {
                    #(#atom_peek_arms)*
                    #(#prefix_peek_arms)*
                    false
                }

                fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#impl_lt>) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                    parse_expr(input, 0)
                }
            }
        };
    })
}

enum PrattKind {
    Atom,
    Prefix { bp: u32 },
    Postfix { bp: u32, inner_bp: Option<u32> },
    Infix { bp: u32, right_assoc: bool },
}

fn parse_pratt_attrs(attrs: &[syn::Attribute]) -> syn::Result<PrattKind> {
    for attr in attrs {
        if attr.path().is_ident("parse") {
            let mut kind = None;
            let mut bp = None;
            let mut inner_bp: Option<u32> = None;
            let mut right_assoc = false;

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("atom") {
                    kind = Some("atom");
                } else if meta.path.is_ident("prefix") {
                    kind = Some("prefix");
                } else if meta.path.is_ident("postfix") {
                    kind = Some("postfix");
                } else if meta.path.is_ident("infix") {
                    kind = Some("infix");
                } else if meta.path.is_ident("bp") {
                    let lit: syn::LitInt = meta.value()?.parse()?;
                    bp = Some(lit.base10_parse::<u32>()?);
                } else if meta.path.is_ident("inner_bp") {
                    let lit: syn::LitInt = meta.value()?.parse()?;
                    inner_bp = Some(lit.base10_parse::<u32>()?);
                } else if meta.path.is_ident("assoc") {
                    let lit: syn::LitStr = meta.value()?.parse()?;
                    if lit.value() == "right" {
                        right_assoc = true;
                    }
                }
                Ok(())
            })?;

            return match kind {
                Some("atom") => Ok(PrattKind::Atom),
                Some("prefix") => Ok(PrattKind::Prefix {
                    bp: bp.ok_or_else(|| syn::Error::new_spanned(attr, "prefix requires bp"))?,
                }),
                Some("postfix") => Ok(PrattKind::Postfix {
                    bp: bp.ok_or_else(|| syn::Error::new_spanned(attr, "postfix requires bp"))?,
                    inner_bp,
                }),
                Some("infix") => Ok(PrattKind::Infix {
                    bp: bp.ok_or_else(|| syn::Error::new_spanned(attr, "infix requires bp"))?,
                    right_assoc,
                }),
                _ => Err(syn::Error::new_spanned(
                    attr,
                    "expected atom, prefix, postfix, or infix",
                )),
            };
        }
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        "pratt enum variant missing #[parse(atom|prefix|postfix|infix, ...)] attribute",
    ))
}
