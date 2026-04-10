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
        Data::Enum(data) => {
            if is_pratt(&input) {
                derive_parse_pratt_enum(name, &input.generics, &rules_type, data)
            } else {
                derive_parse_enum(name, &input.generics, &rules_type, data)
            }
        }
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
                } else if meta.path.is_ident("pratt") {
                    // Accepted but handled separately by is_pratt()
                    Ok(())
                } else {
                    Err(meta.error("expected `rules` or `pratt`"))
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

fn is_pratt(input: &DeriveInput) -> bool {
    input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("parse") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("pratt") {
                found = true;
            } else if meta.path.is_ident("rules") {
                // consume the value
                let _: Type = meta.value()?.parse()?;
            }
            Ok(())
        });
        found
    })
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

    // Build nested if-chain for first_pattern: walk consecutive terminal fields,
    // collecting their patterns joined with IGNORE separator.
    let first_pattern_body = {
        let mut body = quote! {};
        for ty in field_types.iter().rev() {
            body = quote! {
                parts.push(<#ty as ::recursa_core::Parse>::first_pattern().to_string());
                if <#ty as ::recursa_core::Parse>::IS_TERMINAL {
                    #body
                }
            };
        }
        body
    };

    // Generate the parse body: consume_ignored + parse each field
    let parse_fields = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(name, ty)| {
            quote! {
                <#rules_type as ::recursa_core::ParseRules>::consume_ignored(&mut fork);
                let #name = <#ty as ::recursa_core::Parse>::parse(&mut fork, &#rules_type)?;
            }
        });

    Ok(quote! {
        impl #impl_generics ::recursa_core::Parse<#lt> for #name #ty_generics #where_clause {
            const IS_TERMINAL: bool = false;

            fn first_pattern() -> &'static str {
                static PATTERN: ::std::sync::OnceLock<::std::string::String> = ::std::sync::OnceLock::new();
                PATTERN.get_or_init(|| {
                    let ignore = <#rules_type as ::recursa_core::ParseRules>::IGNORE;
                    let sep = if ignore.is_empty() {
                        ::std::string::String::new()
                    } else {
                        ::std::format!("(?:{})?", ignore)
                    };
                    let mut parts: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
                    #first_pattern_body
                    parts.join(&sep)
                })
            }

            fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#lt>, _rules: &R) -> bool {
                let mut peek_input = input.fork();
                <#rules_type as ::recursa_core::ParseRules>::consume_ignored(&mut peek_input);
                <#first_field_type as ::recursa_core::Parse>::peek(&peek_input, &#rules_type)
            }

            fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#lt>, _rules: &R) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
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

    // For the peek_regex function, we need a named lifetime (not '_')
    let fn_lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'__input", proc_macro2::Span::call_site()));
    // For the impl block, we can use '_ when no lifetime is present
    let impl_lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

    // Each variant must be a single-field newtype: Variant(InnerType)
    let mut variant_names = Vec::new();
    let mut inner_types = Vec::new();

    for variant in &data.variants {
        let vname = &variant.ident;
        let inner_type = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => {
                return Err(syn::Error::new_spanned(
                    vname,
                    "Parse enum variants must be single-field tuple variants, e.g. Variant(InnerType). Struct-like variants are not supported — wrap multiple fields in a separate struct that derives Parse.",
                ));
            }
        };

        variant_names.push(vname.clone());
        inner_types.push(inner_type.clone());
    }

    // Generate variant pattern expressions (used by both first_pattern and peek_regex)
    let variant_pattern_exprs: Vec<_> = inner_types
        .iter()
        .map(|ty| {
            quote! { <#ty as ::recursa_core::Parse>::first_pattern() }
        })
        .collect();

    // Generate named capture group names
    let group_names: Vec<String> = (0..inner_types.len()).map(|i| format!("_{i}")).collect();

    // Generate capture check arms for parse: find longest match
    let capture_check_arms: Vec<_> = group_names
        .iter()
        .enumerate()
        .map(|(i, group_name)| {
            let i_lit = syn::Index::from(i);
            quote! {
                if let ::std::option::Option::Some(m) = captures.name(#group_name) {
                    if m.len() > best_len {
                        best_len = m.len();
                        best_index = ::std::option::Option::Some(#i_lit);
                    }
                }
            }
        })
        .collect();

    // Generate match arms for dispatching to the correct variant
    let dispatch_arms: Vec<_> = inner_types
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let vname = &variant_names[i];
            let i_lit = syn::Index::from(i);
            quote! {
                ::std::option::Option::Some(#i_lit) => {
                    let inner = <#ty as ::recursa_core::Parse>::parse(&mut fork, &#rules_type)?;
                    input.commit(fork);
                    ::std::result::Result::Ok(#name::#vname(inner))
                }
            }
        })
        .collect();

    let error_labels: Vec<String> = variant_names.iter().map(|v| v.to_string()).collect();

    Ok(quote! {
        const _: () = {
            static PEEK_REGEX: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();

            fn peek_regex<#fn_lt>() -> &'static ::regex::Regex {
                PEEK_REGEX.get_or_init(|| {
                    let group_names: &[&str] = &[#(#group_names),*];
                    let variant_patterns: &[&str] = &[#(#variant_pattern_exprs),*];
                    let named_groups: ::std::vec::Vec<::std::string::String> = group_names
                        .iter()
                        .zip(variant_patterns.iter())
                        .map(|(name, pat)| ::std::format!("(?P<{}>{})", name, pat))
                        .collect();
                    let combined = ::std::format!(r"\A(?:{})", named_groups.join("|"));
                    ::regex::Regex::new(&combined).unwrap()
                })
            }

            impl #impl_generics ::recursa_core::Parse<#impl_lt> for #name #ty_generics #where_clause {
                const IS_TERMINAL: bool = false;

                fn first_pattern() -> &'static str {
                    static PATTERN: ::std::sync::OnceLock<::std::string::String> = ::std::sync::OnceLock::new();
                    PATTERN.get_or_init(|| {
                        let variant_patterns: &[&str] = &[#(#variant_pattern_exprs),*];
                        let groups: ::std::vec::Vec<::std::string::String> = variant_patterns
                            .iter()
                            .map(|p| ::std::format!("({})", p))
                            .collect();
                        groups.join("|")
                    })
                }

                fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#impl_lt>, _rules: &R) -> bool {
                    let mut peek_input = input.fork();
                    <#rules_type as ::recursa_core::ParseRules>::consume_ignored(&mut peek_input);
                    peek_regex().is_match(peek_input.remaining())
                }

                fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#impl_lt>, _rules: &R) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                    let regex = peek_regex();
                    let mut fork = input.fork();
                    <#rules_type as ::recursa_core::ParseRules>::consume_ignored(&mut fork);

                    let captures = match regex.captures(fork.remaining()) {
                        ::std::option::Option::Some(c) => c,
                        ::std::option::Option::None => {
                            let mut errors = ::std::vec::Vec::new();
                            #(
                                errors.push(::recursa_core::ParseError::new(
                                    fork.source().to_string(),
                                    fork.cursor()..fork.cursor(),
                                    #error_labels,
                                ));
                            )*
                            return ::std::result::Result::Err(::recursa_core::ParseError::merge(errors));
                        }
                    };

                    // Find longest match, declaration order tiebreaker
                    let mut best_len = 0usize;
                    let mut best_index: ::std::option::Option<usize> = ::std::option::Option::None;
                    #(#capture_check_arms)*

                    match best_index {
                        #(#dispatch_arms)*
                        _ => {
                            let mut errors = ::std::vec::Vec::new();
                            #(
                                errors.push(::recursa_core::ParseError::new(
                                    fork.source().to_string(),
                                    fork.cursor()..fork.cursor(),
                                    #error_labels,
                                ));
                            )*
                            ::std::result::Result::Err(::recursa_core::ParseError::merge(errors))
                        }
                    }
                }
            }
        };
    })
}

fn derive_parse_pratt_enum(
    name: &syn::Ident,
    generics: &syn::Generics,
    rules_type: &Type,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // For the inner parse_expr function, we need a named lifetime (not '_')
    let fn_lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'__input", proc_macro2::Span::call_site()));
    // For the impl block, we can use '_ when no lifetime is present
    let impl_lt = generics
        .lifetimes()
        .next()
        .map(|l| l.lifetime.clone())
        .unwrap_or_else(|| syn::Lifetime::new("'_", proc_macro2::Span::call_site()));

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
                    "Pratt enum variants must use tuple fields, not struct-like fields. Use e.g. Variant(Box<Self>, OpToken, Box<Self>) not Variant { left: Box<Self>, op: OpToken, right: Box<Self> }.",
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
                        "prefix variants must have exactly two fields (operator, operand)",
                    ));
                }
                prefix_variants.push((vname.clone(), fields[0].ty.clone(), bp));
            }
            PrattKind::Postfix { bp } => {
                if fields.len() < 2 {
                    return Err(syn::Error::new_spanned(
                        vname,
                        "postfix variants must have at least two fields (lhs, operator, ...)",
                    ));
                }
                let all_field_types: Vec<_> = fields.iter().map(|f| f.ty.clone()).collect();
                postfix_variants.push((vname.clone(), all_field_types, bp));
            }
            PrattKind::Infix { bp, right_assoc } => {
                if fields.len() != 3 {
                    return Err(syn::Error::new_spanned(
                        vname,
                        "infix variants must have exactly three fields (left, operator, right)",
                    ));
                }
                infix_variants.push((vname.clone(), fields[1].ty.clone(), bp, right_assoc));
            }
        }
    }

    // Collect types for first_pattern generation
    let atom_types: Vec<_> = atom_variants.iter().map(|(_, ty)| ty.clone()).collect();
    let prefix_op_types: Vec<_> = prefix_variants
        .iter()
        .map(|(_, op_ty, _)| op_ty.clone())
        .collect();

    // Generate atom peek arms (for the top-level peek)
    let atom_peek_arms = atom_variants.iter().map(|(_vname, ty)| {
        quote! {
            if <#ty as ::recursa_core::Parse>::peek(input, &#rules_type) {
                return true;
            }
        }
    });

    // Generate atom parse arms (for the nud position) -- break out of 'nud block
    let atom_parse_arms = atom_variants.iter().map(|(vname, ty)| {
        quote! {
            if <#ty as ::recursa_core::Parse>::peek(input, &#rules_type) {
                let inner = <#ty as ::recursa_core::Parse>::parse(input, &#rules_type)?;
                break 'nud #name::#vname(inner);
            }
        }
    });

    // Generate prefix parse arms (for the nud position) -- break out of 'nud block
    let prefix_parse_arms = prefix_variants.iter().map(|(vname, op_ty, bp)| {
        quote! {
            if <#op_ty as ::recursa_core::Parse>::peek(input, &#rules_type) {
                let op = <#op_ty as ::recursa_core::Parse>::parse(input, &#rules_type)?;
                let rhs = parse_expr(input, #bp)?;
                break 'nud #name::#vname(op, Box::new(rhs));
            }
        }
    });

    // Generate prefix peek arms (for the top-level peek)
    let prefix_peek_arms = prefix_variants.iter().map(|(_vname, op_ty, _bp)| {
        quote! {
            if <#op_ty as ::recursa_core::Parse>::peek(input, &#rules_type) {
                return true;
            }
        }
    });

    // Generate postfix check/parse arms (for the led loop, before infix)
    let postfix_arms = postfix_variants.iter().map(|(vname, field_types, bp)| {
        // field_types[0] = Box<Self> (lhs), field_types[1] = operator, field_types[2..] = remaining
        let op_ty = &field_types[1];
        let remaining_types = &field_types[2..];

        // Generate field bindings for operator + remaining fields
        let mut field_parses = Vec::new();
        let mut field_idents = Vec::new();

        // Parse the operator (field index 1)
        let op_ident = syn::Ident::new("__f1", proc_macro2::Span::call_site());
        field_parses.push(quote! {
            let #op_ident = <#op_ty as ::recursa_core::Parse>::parse(input, &#rules_type)?;
        });
        field_idents.push(op_ident);

        // Parse remaining fields (indices 2..)
        for (i, ty) in remaining_types.iter().enumerate() {
            let ident = syn::Ident::new(&format!("__f{}", i + 2), proc_macro2::Span::call_site());
            field_parses.push(quote! {
                <#rules_type as ::recursa_core::ParseRules>::consume_ignored(input);
                let #ident = <#ty as ::recursa_core::Parse>::parse(input, &#rules_type)?;
            });
            field_idents.push(ident);
        }

        let all_idents = &field_idents;

        quote! {
            {
                <#rules_type as ::recursa_core::ParseRules>::consume_ignored(input);
                if <#op_ty as ::recursa_core::Parse>::peek(input, &#rules_type) && #bp >= min_bp {
                    #(#field_parses)*
                    lhs = #name::#vname(Box::new(lhs), #(#all_idents),*);
                    continue;
                }
            }
        }
    });

    // Generate infix check/parse arms (for the led loop)
    let infix_arms = infix_variants.iter().map(|(vname, op_ty, bp, right_assoc)| {
        let right_bp: u32 = if *right_assoc { *bp } else { bp + 1 };
        quote! {
            {
                <#rules_type as ::recursa_core::ParseRules>::consume_ignored(input);
                if <#op_ty as ::recursa_core::Parse>::peek(input, &#rules_type) && #bp >= min_bp {
                    let op = <#op_ty as ::recursa_core::Parse>::parse(input, &#rules_type)?;
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
                <#rules_type as ::recursa_core::ParseRules>::consume_ignored(input);

                // Parse prefix or atom (nud position)
                let mut lhs = 'nud: {
                    // Try prefix operators first
                    #(#prefix_parse_arms)*

                    // Try atoms
                    #(#atom_parse_arms)*

                    return Err(::recursa_core::ParseError::new(
                        input.source().to_string(),
                        input.cursor()..input.cursor(),
                        stringify!(#name),
                    ));
                };

                // Led loop (postfix then infix)
                loop {
                    #(#postfix_arms)*
                    #(#infix_arms)*
                    break;
                }

                Ok(lhs)
            }

            impl #impl_generics ::recursa_core::Parse<#impl_lt> for #name #ty_generics #where_clause {
                const IS_TERMINAL: bool = false;

                fn first_pattern() -> &'static str {
                    static PATTERN: ::std::sync::OnceLock<::std::string::String> = ::std::sync::OnceLock::new();
                    PATTERN.get_or_init(|| {
                        let mut parts: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
                        // Atom variants
                        #(parts.push(::std::format!("({})", <#atom_types as ::recursa_core::Parse>::first_pattern()));)*
                        // Prefix operator variants
                        #(parts.push(::std::format!("({})", <#prefix_op_types as ::recursa_core::Parse>::first_pattern()));)*
                        // Infix operators NOT included
                        parts.join("|")
                    })
                }

                fn peek<R: ::recursa_core::ParseRules>(input: &::recursa_core::Input<#impl_lt>, _rules: &R) -> bool {
                    #(#atom_peek_arms)*
                    #(#prefix_peek_arms)*
                    false
                }

                fn parse<R: ::recursa_core::ParseRules>(input: &mut ::recursa_core::Input<#impl_lt>, _rules: &R) -> ::std::result::Result<Self, ::recursa_core::ParseError> {
                    parse_expr(input, 0)
                }
            }
        };
    })
}

enum PrattKind {
    Atom,
    Prefix { bp: u32 },
    Postfix { bp: u32 },
    Infix { bp: u32, right_assoc: bool },
}

fn parse_pratt_attrs(attrs: &[syn::Attribute]) -> syn::Result<PrattKind> {
    for attr in attrs {
        if attr.path().is_ident("parse") {
            let mut kind = None;
            let mut bp = None;
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
                    let value = meta.value()?;
                    let lit: syn::LitInt = value.parse()?;
                    bp = Some(lit.base10_parse::<u32>()?);
                } else if meta.path.is_ident("assoc") {
                    let value = meta.value()?;
                    let lit: syn::LitStr = value.parse()?;
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
