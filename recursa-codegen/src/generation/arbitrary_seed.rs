//! Arena-backed `Arbitrary` support: a flat, always-heap mirror ("seed") of every Node reachable
//! from an `arena_ast` grammar's Arbitrary-configured roots, plus mechanical materialization from
//! a sampled seed value into the real arena-backed AST.
//!
//! `arbitrary::Arbitrary::arbitrary(u: &mut Unstructured) -> Result<Self>` has no channel for an
//! allocator handle, so a type that needs a live `&'bump Bump` to construct itself (`ArenaBox`,
//! `ArenaVec`, `ArenaVec1`) cannot implement it directly. Rather than solve that by threading a
//! `Bump` through sampling itself (which would need a new allocator-aware trait plus careful
//! lifetime plumbing), this module generates a second, ordinary-heap-shaped Node type per Node —
//! identical in field shape, with `Box`/`Vec`/`Vec1` standing in for `ArenaBox`/`ArenaVec`/
//! `ArenaVec1` — and reuses the *already existing, unmodified* [`render_node_arbitrary`] Nested-AST
//! Arbitrary codegen against it unchanged. A second, purely mechanical `into_arena` function then
//! copies the sampled seed value into a freshly allocated arena, field by field, with no
//! interaction with `Unstructured` at all.
//!
//! For a non-`arena_ast` grammar, the primary AST already *is* this shape, so the "seed" module
//! just re-exports the primary Node types directly under the same flat names.
//!
//! Every seed type lives in one flat, grammar-root-anchored module (`__recursa_arbitrary_seed`),
//! named by each Node *application*'s own [`NodeApplicationParsePlan::parse_index`] — the same
//! flat, global numbering the generated arena family markers (`__RecursaArenaFamilyNode{N}` in
//! `table_driven.rs`) already use, and the same aggregation shape `__recursa_content_tokens`
//! already uses for content tokens. Keying by *application* rather than by a Node's own position in
//! [`EmissionPlan::nodes`] matters as soon as any Node has more than one materialized application
//! (generics): `parse_index` is the one identity already unique per concrete generated type,
//! not per Node family.

use super::*;

/// Renders the complete `__recursa_arbitrary_seed` module for one grammar.
///
/// Emits nothing when no Node configures `Arbitrary`. Otherwise emits one entry per
/// Arbitrary-configured Node application: for a non-`arena_ast` grammar, a type alias to the
/// primary type; for an `arena_ast` grammar, a real mirror struct/enum, its Arbitrary impl (via
/// the existing Nested-AST renderer, unmodified), and its `into_arena` materialization method.
pub(super) fn render_arbitrary_seed_module(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let entries = plan
        .nodes
        .iter()
        .filter(|node| {
            // A seed entry, wherever it names the primary type (the non-`arena_ast` re-export;
            // `into_arena`'s own return type), does so from this module's aggregate, grammar-root
            // location — unlike every *other* generated capability, which is spliced directly
            // into the primary type's own module via per-node inclusion, and so never has to name
            // it from outside. A Node declared without `pub` (a common pattern for grammar-private
            // helper types — see `recursa-codegen/tests/fixtures/generation/compound-nodes`)
            // cannot be named from here, so its seed entry is skipped entirely rather than
            // emitting a reference the compiler will reject.
            node.configured_traits
                .contains(&ConfiguredTraitRecipe::Arbitrary)
                && node.publicly_materialized
        })
        .flat_map(|node| {
            node.parse_applications
                .iter()
                .map(move |application| (node, application))
        })
        .map(|(node, application)| render_seed_entry(plan, node, application, facade));
    quote! {
        __recursa_if_arbitrary! {
            #[doc(hidden)]
            // A seed type mirrors the authored Node it stands for: the same variant names and the
            // same `zero_or_many!(boxed!(T))` field, as `Vec<Box<T>>`. Clippy skips
            // `enum_variant_names` and `vec_box` on an exported type, so the authored Node never
            // meets them; the mirror is crate-private and would. A mirror also owns what the
            // arena type only refers to, so a variant that is small in the authored enum can be
            // `large_enum_variant` here. Nothing downstream can edit a mirror, and it must keep
            // the Node's shape.
            #[allow(
                clippy::enum_variant_names,
                clippy::large_enum_variant,
                clippy::vec_box
            )]
            pub(crate) mod __recursa_arbitrary_seed {
                #(#entries)*
            }
        }
    }
}

/// Renders one Node application's seed entry: a type alias for a plain-heap grammar, or a real
/// mirror type, its Arbitrary impl, and its arena materialization method for an `arena_ast`
/// grammar.
fn render_seed_entry(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    facade: &Ident,
) -> proc_macro2::TokenStream {
    if !plan.arena_ast {
        let ident = seed_type_ident(application.parse_index);
        let primary = &application.static_ty;
        // A plain `use ... as ...;` cannot carry the primary type's own generic arguments (`use`
        // paths take no `<..>`), so this re-export is a type alias instead — transparent for both
        // the `Arbitrary` impl call and every other use site, without duplicating anything.
        return quote!(pub type #ident = #primary;);
    }
    let seed_node = seed_node_plan(plan, node, application);
    let definition = render_seed_type_definition(&seed_node, facade);
    let arbitrary_impl = render_node_arbitrary(plan, &seed_node);
    // The mirror receives the element type's declared rejection rules too, so a sampled list of
    // mirrors drops exactly the items the value form drops and materializes into an arena value
    // that parses back.
    let rejection_support = super::rejections::render_arbitrary_support(
        plan,
        &seed_node,
        &super::rejections::NestedNaming::Seed,
    )
    .unwrap_or_else(|error| quote!(::core::compile_error!(#error);));
    let materialize = render_seed_materialize(plan, application, &seed_node, facade);
    quote! {
        #definition
        #arbitrary_impl
        #rejection_support
        #materialize
    }
}

/// One flat identifier naming a Node application's seed/mirror type, stable across its authored
/// module — keyed by [`NodeApplicationParsePlan::parse_index`], the one identity already unique
/// per concrete generated type (see the module-level doc comment).
pub(super) fn seed_type_ident(parse_index: usize) -> Ident {
    format_ident!("__RecursaArbitrarySeed{parse_index}")
}

/// Absolute path to one Node application's seed/mirror type, anchored at the grammar's own root.
pub(super) fn seed_type_path(plan: &EmissionPlan, parse_index: usize) -> Type {
    // `module_anchor` is already a complete, directly usable path (see its other call sites in
    // `generated_output.rs`, e.g. `#module_anchor::View<..>`) — it must not be re-prefixed with a
    // fresh `crate`.
    let mut path = plan.module_anchor.clone();
    path.segments.push(syn::PathSegment {
        ident: format_ident!("__recursa_arbitrary_seed"),
        arguments: syn::PathArguments::None,
    });
    path.segments.push(syn::PathSegment {
        ident: seed_type_ident(parse_index),
        arguments: syn::PathArguments::None,
    });
    Type::Path(syn::TypePath { qself: None, path })
}

/// A type path with its final segment's generic arguments stripped — used only where the path
/// appears in *expression* position (a struct-literal or enum-variant constructor). Rust's
/// struct-literal syntax rejects `Type<'a> { .. }`; the omitted lifetime is inferred from the
/// enclosing function's declared return type instead.
fn strip_generic_args(ty: &Type) -> Type {
    let mut ty = ty.clone();
    if let Type::Path(type_path) = &mut ty
        && let Some(segment) = type_path.path.segments.last_mut()
    {
        segment.arguments = syn::PathArguments::None;
    }
    ty
}

/// Resolves a closed application to the stable seed-type index shared with explorer generation.
fn application_parse_index_of(plan: &EmissionPlan, target: MaterializedApplicationId) -> usize {
    explorer_application_parse_plan(&plan.nodes, target)
        .1
        .parse_index
}

/// Finds the canonical token plan used to qualify a seed field outside its authored module.
fn content_token_index_of(plan: &EmissionPlan, target: ContentTokenId) -> usize {
    plan.tokens
        .iter()
        .position(|candidate| candidate.semantic == target)
        .expect("Content always names a token present in the same plan")
}

/// Absolute path to one content token's generated type, anchored at the grammar's own root, at
/// the given lifetime — the same numbered private alias [`terminal_type`]'s `ContentToken` branch
/// names elsewhere, reconstructed here because a projection's own captured `terminal` type may
/// instead be whatever bare, module-local alias name (e.g. `WordText`) the user's field spelled,
/// which only resolves inside that field's own authored module — not inside the aggregate seed
/// module this function's caller lives in.
fn content_token_path(plan: &EmissionPlan, index: usize, lifetime: &syn::Lifetime) -> Type {
    let mut path = plan.module_anchor.clone();
    path.segments.push(syn::PathSegment {
        ident: format_ident!("__recursa_content_tokens"),
        arguments: syn::PathArguments::None,
    });
    path.segments.push(syn::PathSegment {
        ident: format_ident!("__RecursaContentToken{index}"),
        arguments: syn::PathArguments::AngleBracketed(syn::parse_quote!(<#lifetime>)),
    });
    Type::Path(syn::TypePath { qself: None, path })
}

/// Rewrites one semantic projection's terminal for seed use, leaving cardinality untouched. A
/// Node-typed terminal becomes a reference to that Node's own seed type; a content-token terminal
/// becomes its canonical, fully qualified type at `'static` — sound because content-token storage
/// (`ContentText`) is Owned-or-Borrowed, and an Owned instance carries no real borrow, so it is
/// constructible (and usable) at any lifetime the caller names.
fn rewrite_projection_for_seed(
    plan: &EmissionPlan,
    projection: &SemanticParseProjection,
) -> SemanticParseProjection {
    let mut rewritten = projection.clone();
    if let SemanticParseProjection::Direct {
        terminal,
        terminal_application,
        explorer_terminal,
        ..
    } = &mut rewritten
    {
        let static_lifetime: syn::Lifetime = syn::parse_quote!('static);
        let replaced = match (*terminal_application, &explorer_terminal) {
            (Some(target), _) => seed_type_path(plan, application_parse_index_of(plan, target)),
            (None, ExplorerTerminalPlan::Content(token)) => {
                content_token_path(plan, content_token_index_of(plan, *token), &static_lifetime)
            }
            (None, ExplorerTerminalPlan::Node(_)) => {
                unreachable!("a Node explorer_terminal always carries terminal_application")
            }
        };
        **terminal = replaced;
    }
    rewritten
}

/// Rewrites one parse element's semantic projection for seed use; fixed-token elements carry no
/// field and pass through unchanged.
fn rewrite_element_for_seed(plan: &EmissionPlan, element: &ElementParsePlan) -> ElementParsePlan {
    let mut rewritten = element.clone();
    if let ElementParseKind::Semantic { projection, .. } = &mut rewritten.kind {
        let replaced = rewrite_projection_for_seed(plan, projection);
        **projection = replaced;
    }
    rewritten
}

/// Rewrites one variant's elements for seed use.
fn rewrite_variant_for_seed(plan: &EmissionPlan, variant: &VariantParsePlan) -> VariantParsePlan {
    let mut rewritten = variant.clone();
    rewritten.elements = variant
        .elements
        .iter()
        .map(|element| rewrite_element_for_seed(plan, element))
        .collect();
    rewritten
}

/// Rewrites a Node's body for seed use, over the flat, ordered variant list [`trait_variants`]
/// already produces for every other Nested-AST trait renderer (including
/// [`render_node_arbitrary`] itself).
fn rewrite_body_for_seed(plan: &EmissionPlan, body: &NodeBodyPlan) -> NodeBodyPlan {
    match body {
        NodeBodyPlan::Struct { style, elements } => NodeBodyPlan::Struct {
            style: *style,
            elements: elements
                .iter()
                .map(|element| rewrite_element_for_seed(plan, element))
                .collect(),
        },
        NodeBodyPlan::Enum { .. } => NodeBodyPlan::Enum {
            variants: trait_variants(body)
                .iter()
                .map(|variant| rewrite_variant_for_seed(plan, variant))
                .collect(),
        },
    }
}

/// Builds the seed's own frozen plan from the primary Node's: a fresh, flat, lifetime-free
/// identity with every terminal type rewritten to its seed form. Cardinality (`FieldWrapper`s),
/// field identity, and construction style are untouched, so [`render_node_arbitrary`] — which only
/// ever reads terminal types through `render_wrapped_type` (always the heap-shaped renderer) — is
/// reused completely unmodified against this plan.
fn seed_node_plan(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> NodeParsePlan {
    let mut seed = node.clone();
    seed.ident = seed_type_ident(application.parse_index);
    // A materialized application is fully concrete — every generic parameter of the Node family is
    // already substituted into `application.ty`/`application.body` — so the seed, unlike the Node
    // family plan it was cloned from, carries no generics at all.
    seed.generics = syn::Generics::default();
    seed.body = rewrite_body_for_seed(plan, &application.body);
    seed
}

/// Semantic fields paired with their rendered seed (always-heap) types, in authored field order.
fn seed_field_types(facade: &Ident, elements: &[ElementParsePlan]) -> Vec<(FieldPlan, Type)> {
    let mut fields = elements
        .iter()
        .filter_map(|element| {
            let ElementParseKind::Semantic { field, projection } = &element.kind else {
                return None;
            };
            let ty = match projection.as_ref() {
                SemanticParseProjection::Direct {
                    terminal, wrappers, ..
                } => render_wrapped_type(facade, terminal, wrappers),
                SemanticParseProjection::FixedPresence { .. } => syn::parse_quote!(bool),
            };
            Some((field.clone(), ty))
        })
        .collect::<Vec<_>>();
    fields.sort_by_key(|(field, _)| field.index);
    fields
}

/// Renders one seed Node's own struct or enum type declaration.
fn render_seed_type_definition(
    seed_node: &NodeParsePlan,
    facade: &Ident,
) -> proc_macro2::TokenStream {
    let ident = &seed_node.ident;
    match &seed_node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = seed_field_types(facade, elements);
            let body = render_seed_fields(
                style,
                &fields,
                |field, ty| quote!(pub #field: #ty),
                |ty| quote!(pub #ty),
            );
            let terminator = matches!(
                style,
                ResolvedFieldStyle::Unnamed | ResolvedFieldStyle::Unit
            )
            .then(|| quote!(;));
            quote! {
                #[allow(dead_code)]
                pub struct #ident #body #terminator
            }
        }
        NodeBodyPlan::Enum { variants } => {
            let rendered_variants = variants.iter().map(|variant| {
                let variant_ident = &variant.ident;
                let fields = seed_field_types(facade, &variant.elements);
                let body = render_seed_fields(
                    &variant.style,
                    &fields,
                    |field, ty| quote!(#field: #ty),
                    |ty| quote!(#ty),
                );
                quote!(#variant_ident #body)
            });
            quote! {
                #[allow(dead_code)]
                pub enum #ident {
                    #(#rendered_variants),*
                }
            }
        }
    }
}

/// Renders one struct or variant's field list body (without any trailing terminator — a struct
/// caller adds its own `;` for the unnamed/unit shapes; an enum variant needs none).
fn render_seed_fields(
    style: &ResolvedFieldStyle,
    fields: &[(FieldPlan, Type)],
    named: impl Fn(&Ident, &Type) -> proc_macro2::TokenStream,
    unnamed: impl Fn(&Type) -> proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => {
            let entries = fields.iter().map(|(field, ty)| {
                let field_ident = field.ident.as_ref().expect("named field has an identifier");
                named(field_ident, ty)
            });
            quote!({ #(#entries),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let entries = fields.iter().map(|(_, ty)| unnamed(ty));
            quote!((#(#entries),*))
        }
        ResolvedFieldStyle::Unit => quote!(),
    }
}

/// One semantic field's cardinality and terminal identity, read from the *primary* (unmodified)
/// element list — the shape `into_arena` needs to know which wrapper conversions to apply.
struct MaterializeField {
    /// Authored field identity and position used to reconstruct its owning value.
    field: FieldPlan,
    /// Ordered container layers that must be rebuilt in the destination arena.
    wrappers: Vec<FieldWrapper>,
    /// Closed child application to materialize, absent for intrinsic token values.
    terminal_application: Option<MaterializedApplicationId>,
    /// Whether this field is a boolean presence value requiring no storage conversion.
    is_fixed_presence: bool,
}

/// Collects semantic fields in authored field order for seed-to-arena reconstruction.
fn materialize_fields(elements: &[ElementParsePlan]) -> Vec<MaterializeField> {
    let mut fields = elements
        .iter()
        .filter_map(|element| {
            let ElementParseKind::Semantic { field, projection } = &element.kind else {
                return None;
            };
            let (wrappers, terminal_application, is_fixed_presence) = match projection.as_ref() {
                SemanticParseProjection::Direct {
                    wrappers,
                    terminal_application,
                    ..
                } => (wrappers.clone(), *terminal_application, false),
                SemanticParseProjection::FixedPresence { .. } => (Vec::new(), None, true),
            };
            Some(MaterializeField {
                field: field.clone(),
                wrappers,
                terminal_application,
                is_fixed_presence,
            })
        })
        .collect::<Vec<_>>();
    fields.sort_by_key(|entry| entry.field.index);
    fields
}

/// A positional (tuple-style) field's synthetic binding name, shared between a match pattern that
/// destructures a seed value and the reconstruction expression built from those same bindings.
fn positional_binding(index: usize) -> Ident {
    format_ident!("__recursa_seed_v{index}")
}

/// The local binding a seed field materializes under, whether pattern-matched (enum variant) or
/// field-accessed (struct): the field's own name when named, else its positional binding.
fn binding_ident(field: &FieldPlan) -> Ident {
    field
        .ident
        .clone()
        .unwrap_or_else(|| positional_binding(field.index))
}

/// Converts one field's already-sampled seed value into its arena-backed equivalent: unwraps each
/// storage wrapper outermost-first, recursing into the arena's own container constructors, and at
/// the leaf either moves a content value through unchanged (content storage is Owned-or-Borrowed
/// and never depends on the arena) or calls the referenced Node's own `into_arena` method.
fn materialize_expr(
    facade: &Ident,
    wrappers: &[FieldWrapper],
    terminal_application: Option<MaterializedApplicationId>,
    value: proc_macro2::TokenStream,
    bump: &Ident,
) -> proc_macro2::TokenStream {
    let Some((head, rest)) = wrappers.split_first() else {
        return if terminal_application.is_some() {
            quote!((#value).__recursa_arbitrary_into_arena(#bump))
        } else {
            value
        };
    };
    match head {
        // An optional content value has nothing to convert: it moves through as it is. Emitting
        // the general form here would be `value.map(|inner| inner)`, which is
        // `clippy::map_identity` in a downstream crate that nobody can edit.
        FieldWrapper::Optional if rest.is_empty() && terminal_application.is_none() => value,
        FieldWrapper::Optional => {
            let inner = materialize_expr(
                facade,
                rest,
                terminal_application,
                quote!(__recursa_seed_inner),
                bump,
            );
            quote!((#value).map(|__recursa_seed_inner| #inner))
        }
        FieldWrapper::Boxed => {
            let inner = materialize_expr(facade, rest, terminal_application, quote!(*#value), bump);
            quote!(#facade::ArenaBox::new_in(#inner, #bump))
        }
        // The same holds for a list of content values: no element needs a conversion.
        FieldWrapper::ZeroOrMore if rest.is_empty() && terminal_application.is_none() => {
            quote!(#facade::ArenaVec::from_iter_in(#value, #bump))
        }
        FieldWrapper::ZeroOrMore => {
            let inner = materialize_expr(
                facade,
                rest,
                terminal_application,
                quote!(__recursa_seed_item),
                bump,
            );
            quote! {
                #facade::ArenaVec::from_iter_in(
                    (#value).into_iter().map(|__recursa_seed_item| #inner),
                    #bump,
                )
            }
        }
        FieldWrapper::OneOrMore => {
            let inner = materialize_expr(
                facade,
                rest,
                terminal_application,
                quote!(__recursa_seed_item),
                bump,
            );
            quote! {
                {
                    let mut __recursa_seed_values = (#value).into_vec().into_iter();
                    let __recursa_seed_item = __recursa_seed_values
                        .next()
                        .expect("OneOrMore is never empty");
                    let mut __recursa_seed_result = #facade::ArenaVec1::new_in(#inner, #bump);
                    for __recursa_seed_item in __recursa_seed_values {
                        __recursa_seed_result.push(#inner);
                    }
                    __recursa_seed_result
                }
            }
        }
    }
}

/// Converts one seed field into arena storage while retaining presence booleans directly.
fn materialize_field_expr(
    facade: &Ident,
    field: &MaterializeField,
    source: proc_macro2::TokenStream,
    bump: &Ident,
) -> proc_macro2::TokenStream {
    if field.is_fixed_presence {
        source
    } else {
        materialize_expr(
            facade,
            &field.wrappers,
            field.terminal_application,
            source,
            bump,
        )
    }
}

/// Shared struct/variant reconstruction: builds `target { .. }` / `target(..)` / `target`,
/// converting each field's source expression (supplied by the caller, since a struct reads
/// `self.field` while an enum variant reads a pattern-bound local) into its arena form.
fn render_seed_materialize_construction(
    facade: &Ident,
    style: &ResolvedFieldStyle,
    elements: &[ElementParsePlan],
    target: proc_macro2::TokenStream,
    bump: &Ident,
    field_value: impl Fn(&FieldPlan) -> proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let fields = materialize_fields(elements);
    match style {
        ResolvedFieldStyle::Named => {
            let entries = fields.iter().map(|entry| {
                let ident = entry
                    .field
                    .ident
                    .as_ref()
                    .expect("named field has an identifier");
                let source = field_value(&entry.field);
                let expr = materialize_field_expr(facade, entry, source, bump);
                quote!(#ident: #expr)
            });
            quote!(#target { #(#entries),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let entries = fields.iter().map(|entry| {
                let source = field_value(&entry.field);
                materialize_field_expr(facade, entry, source, bump)
            });
            quote!(#target(#(#entries),*))
        }
        ResolvedFieldStyle::Unit => quote!(#target),
    }
}

/// Builds the match pattern destructuring one seed enum variant into local bindings named exactly
/// as [`binding_ident`] would look them up.
fn render_variant_pattern(
    seed_ident: &Ident,
    variant_ident: &Ident,
    style: &ResolvedFieldStyle,
    fields: &[MaterializeField],
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => {
            let bindings = fields.iter().map(|entry| {
                let ident = entry
                    .field
                    .ident
                    .as_ref()
                    .expect("named field has an identifier");
                quote!(#ident)
            });
            quote!(#seed_ident::#variant_ident { #(#bindings),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let bindings = fields
                .iter()
                .map(|entry| positional_binding(entry.field.index));
            quote!(#seed_ident::#variant_ident(#(#bindings),*))
        }
        ResolvedFieldStyle::Unit => quote!(#seed_ident::#variant_ident),
    }
}

/// Matches one seed variant and rebuilds its corresponding concrete arena variant.
fn render_seed_materialize_variant_arm(
    facade: &Ident,
    seed_ident: &Ident,
    primary_ty: &Type,
    primary_variant: &VariantParsePlan,
    bump: &Ident,
) -> proc_macro2::TokenStream {
    let fields = materialize_fields(&primary_variant.elements);
    let pattern = render_variant_pattern(
        seed_ident,
        &primary_variant.ident,
        &primary_variant.style,
        &fields,
    );
    let variant_ident = &primary_variant.ident;
    let construction = render_seed_materialize_construction(
        facade,
        &primary_variant.style,
        &primary_variant.elements,
        quote!(#primary_ty::#variant_ident),
        bump,
        |field| {
            let ident = binding_ident(field);
            quote!(#ident)
        },
    );
    quote!(#pattern => #construction)
}

/// Renders one seed Node's `into_arena` materialization method: a purely mechanical, recursive
/// copy from the sampled seed value into a freshly arena-allocated primary value, with no
/// interaction with `Unstructured` at all — every random choice was already made while sampling
/// the seed.
fn render_seed_materialize(
    plan: &EmissionPlan,
    application: &NodeApplicationParsePlan,
    seed_node: &NodeParsePlan,
    facade: &Ident,
) -> proc_macro2::TokenStream {
    let _ = plan;
    let seed_ident = &seed_node.ident;
    // `application.ty` already embeds `'__recursa_input` as its generic argument (see
    // `emission_plan.rs`'s `materialized_application_type` call) — declaring the *same* lifetime
    // name on this method lets `application.ty` be reused verbatim as the return type, with no
    // rewriting needed.
    let bump_lifetime: syn::Lifetime = syn::parse_quote!('__recursa_input);
    let bump = format_ident!("__recursa_input_bump");
    let return_ty = &application.ty;
    // A Node with no borrowed content has no lifetime parameter, so its `application.ty` does not
    // name `'__recursa_input`. Declaring the lifetime for the bump reference alone is
    // `clippy::needless_lifetimes` in the downstream crate; the elided form says the same thing.
    let names_input_lifetime = quote!(#return_ty)
        .to_string()
        .contains(&bump_lifetime.ident.to_string());
    let (method_generics, bump_ty) = if names_input_lifetime {
        (
            quote!(<#bump_lifetime>),
            quote!(&#bump_lifetime #facade::__private::bumpalo::Bump),
        )
    } else {
        (quote!(), quote!(&#facade::__private::bumpalo::Bump))
    };
    let construction_ty = strip_generic_args(&application.ty);
    let body = match &application.body {
        NodeBodyPlan::Struct { style, elements } => render_seed_materialize_construction(
            facade,
            style,
            elements,
            quote!(#construction_ty),
            &bump,
            |field| match &field.ident {
                Some(ident) => quote!(self.#ident),
                None => {
                    let index = syn::Index::from(field.index);
                    quote!(self.#index)
                }
            },
        ),
        NodeBodyPlan::Enum { .. } => {
            let arms = trait_variants(&application.body)
                .into_iter()
                .map(|primary_variant| {
                    render_seed_materialize_variant_arm(
                        facade,
                        seed_ident,
                        &construction_ty,
                        &primary_variant,
                        &bump,
                    )
                });
            quote! {
                match self {
                    #(#arms,)*
                }
            }
        }
    };
    quote! {
        #[allow(dead_code)]
        impl #seed_ident {
            pub fn __recursa_arbitrary_into_arena #method_generics (
                self,
                #bump: #bump_ty,
            ) -> #return_ty {
                #body
            }
        }
    }
}
