use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Lit, Type};

/// Check if a type is `Option<...>`.
fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident == "Option";
    }
    false
}

pub fn derive_format_tokens(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let struct_attrs = parse_struct_attrs(&input)?;

    let body = match &input.data {
        Data::Struct(data) => derive_struct(data, &struct_attrs)?,
        Data::Enum(data) => derive_enum(data)?,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "FormatTokens can only be derived for structs and enums",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics ::recursa_core::FormatTokens for #name #ty_generics #where_clause {
            fn format_tokens(&self, tokens: &mut Vec<::recursa_core::fmt::Token>) {
                #body
            }
        }
    })
}

// -- Attribute parsing --

#[derive(Default)]
struct StructAttrs {
    group: Option<GroupKindAttr>,
}

#[derive(Default)]
struct FieldAttrs {
    group: Option<GroupKindAttr>,
    break_: Option<BreakAttr>,
    indent: bool,
}

#[derive(Clone)]
enum GroupKindAttr {
    Consistent,
    Inconsistent,
}

struct BreakAttr {
    flat: String,
    broken: String,
}

fn parse_struct_attrs(input: &DeriveInput) -> syn::Result<StructAttrs> {
    let mut attrs = StructAttrs::default();
    for attr in &input.attrs {
        if attr.path().is_ident("format_tokens") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("group") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let kind: syn::Ident = content.parse()?;
                    attrs.group = Some(parse_group_kind(&kind)?);
                    Ok(())
                } else {
                    Err(meta.error("expected `group(consistent)` or `group(inconsistent)`"))
                }
            })?;
        }
    }
    Ok(attrs)
}

fn parse_field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
    let mut attrs = FieldAttrs::default();
    for attr in &field.attrs {
        if attr.path().is_ident("format_tokens") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("group") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let kind: syn::Ident = content.parse()?;
                    attrs.group = Some(parse_group_kind(&kind)?);
                    Ok(())
                } else if meta.path.is_ident("break") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let mut flat = None;
                    let mut broken = None;
                    while !content.is_empty() {
                        let key: syn::Ident = content.parse()?;
                        let _: syn::Token![=] = content.parse()?;
                        let value: Lit = content.parse()?;
                        let s = match &value {
                            Lit::Str(s) => s.value(),
                            _ => {
                                return Err(syn::Error::new_spanned(
                                    value,
                                    "expected string literal",
                                ));
                            }
                        };
                        if key == "flat" {
                            flat = Some(s);
                        } else if key == "broken" {
                            broken = Some(s);
                        } else {
                            return Err(syn::Error::new_spanned(
                                key,
                                "expected `flat` or `broken`",
                            ));
                        }
                        let _ = content.parse::<syn::Token![,]>();
                    }
                    attrs.break_ = Some(BreakAttr {
                        flat: flat.unwrap_or_else(|| " ".to_string()),
                        broken: broken.unwrap_or_else(|| "\n".to_string()),
                    });
                    Ok(())
                } else if meta.path.is_ident("indent") {
                    attrs.indent = true;
                    Ok(())
                } else {
                    Err(meta.error("expected `group(...)`, `break(...)`, or `indent`"))
                }
            })?;
        }
    }
    Ok(attrs)
}

fn parse_group_kind(ident: &syn::Ident) -> syn::Result<GroupKindAttr> {
    if ident == "consistent" {
        Ok(GroupKindAttr::Consistent)
    } else if ident == "inconsistent" {
        Ok(GroupKindAttr::Inconsistent)
    } else {
        Err(syn::Error::new_spanned(
            ident,
            "expected `consistent` or `inconsistent`",
        ))
    }
}

fn group_kind_tokens(kind: &GroupKindAttr) -> TokenStream {
    match kind {
        GroupKindAttr::Consistent => {
            quote! { ::recursa_core::fmt::GroupKind::Consistent }
        }
        GroupKindAttr::Inconsistent => {
            quote! { ::recursa_core::fmt::GroupKind::Inconsistent }
        }
    }
}

// -- Code generation --

fn derive_struct(data: &syn::DataStruct, struct_attrs: &StructAttrs) -> syn::Result<TokenStream> {
    let field_emissions = match &data.fields {
        Fields::Named(fields) => {
            let mut emissions = Vec::new();
            for f in &fields.named {
                let name = &f.ident;
                let attrs = parse_field_attrs(f)?;
                let is_option = is_option_type(&f.ty);
                emissions.push(emit_field(quote! { self.#name }, &attrs, is_option));
            }
            emissions
        }
        Fields::Unnamed(fields) => {
            let mut emissions = Vec::new();
            for (i, f) in fields.unnamed.iter().enumerate() {
                let idx = syn::Index::from(i);
                let attrs = parse_field_attrs(f)?;
                let is_option = is_option_type(&f.ty);
                emissions.push(emit_field(quote! { self.#idx }, &attrs, is_option));
            }
            emissions
        }
        Fields::Unit => vec![],
    };

    let inner = quote! { #(#field_emissions)* };

    if let Some(group) = &struct_attrs.group {
        let kind = group_kind_tokens(group);
        Ok(quote! {
            tokens.push(::recursa_core::fmt::Token::Begin(#kind));
            #inner
            tokens.push(::recursa_core::fmt::Token::End);
        })
    } else {
        Ok(inner)
    }
}

fn derive_enum(data: &syn::DataEnum) -> syn::Result<TokenStream> {
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
                    let calls: Vec<_> = bindings
                        .iter()
                        .map(
                            |b| quote! { ::recursa_core::FormatTokens::format_tokens(#b, tokens); },
                        )
                        .collect();
                    quote! {
                        Self::#vname(#(#bindings),*) => { #(#calls)* }
                    }
                }
                Fields::Named(fields) => {
                    let names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
                    let calls: Vec<_> = names
                        .iter()
                        .map(
                            |n| quote! { ::recursa_core::FormatTokens::format_tokens(#n, tokens); },
                        )
                        .collect();
                    quote! {
                        Self::#vname { #(#names),* } => { #(#calls)* }
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

fn emit_field(access: TokenStream, attrs: &FieldAttrs, is_option: bool) -> TokenStream {
    let has_attrs = attrs.break_.is_some() || attrs.indent || attrs.group.is_some();

    // For Option fields with formatting attributes, wrap the entire
    // attributed emission in an `if let Some` check so breaks/indents
    // are not emitted when the Option is None.
    if is_option && has_attrs {
        let inner_emit = emit_field_inner(quote! { *__inner }, attrs);
        quote! {
            if let ::std::option::Option::Some(__inner) = &#access {
                #inner_emit
            }
        }
    } else if has_attrs {
        emit_field_inner(access, attrs)
    } else {
        // No attributes — just delegate
        quote! {
            ::recursa_core::FormatTokens::format_tokens(&#access, tokens);
        }
    }
}

/// Emit a field with formatting attributes applied.
/// `access` is the expression to call format_tokens on.
fn emit_field_inner(access: TokenStream, attrs: &FieldAttrs) -> TokenStream {
    let core_emit = quote! {
        ::recursa_core::FormatTokens::format_tokens(&#access, tokens);
    };

    // Nesting order: group → indent → break → content → dedent → end
    //
    // Indent must be set before Break fires so the broken newline
    // renders at the correct indentation level.

    // Innermost: content with break prepended
    let with_break = if let Some(brk) = &attrs.break_ {
        let flat = &brk.flat;
        let broken = &brk.broken;
        quote! {
            tokens.push(::recursa_core::fmt::Token::Break {
                flat: #flat.to_string(),
                broken: #broken.to_string(),
            });
            #core_emit
        }
    } else {
        core_emit
    };

    // Wrap with indent
    let with_indent = if attrs.indent {
        quote! {
            tokens.push(::recursa_core::fmt::Token::Indent);
            #with_break
            tokens.push(::recursa_core::fmt::Token::Dedent);
        }
    } else {
        with_break
    };

    // Wrap with group (outermost)
    if let Some(group) = &attrs.group {
        let kind = group_kind_tokens(group);
        quote! {
            tokens.push(::recursa_core::fmt::Token::Begin(#kind));
            #with_indent
            tokens.push(::recursa_core::fmt::Token::End);
        }
    } else {
        with_indent
    }
}
