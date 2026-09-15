#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Parsing and representation-specific lowering for `ast_node!` declarations.
//!
//! This module owns the declaration-only macro language. It expands a recognized
//! invocation exactly once into ordinary Rust items so the established resolver
//! remains the single parser and name-resolution path for Node semantics.

use std::collections::BTreeSet;
use std::ops::Range;

use proc_macro2::Span;
use syn::ext::IdentExt as _;
use syn::spanned::Spanned as _;
use syn::visit::Visit as _;
use syn::visit_mut::VisitMut as _;
use syn::{Attribute, GenericParam, Item, ItemMacro, Type, Visibility, parse_quote_spanned};

use crate::source_syntax::{CargoCfgSnapshot, configured_attribute_metas, span_range};

/// Metadata retained when one declaration item replaces its enclosing macro invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeclarationOrigin {
    /// Authored range of the outer invocation used as the generated loader replacement anchor.
    pub(crate) invocation_range: Range<u32>,
    /// Exact concrete declaration produced for the selected nested representation.
    pub(crate) concrete_item: Item,
    /// Number of authored abstract container markers on each field in source order.
    pub(crate) field_marker_ranges: Vec<Vec<Range<u32>>>,
}

/// Result of expanding declarations in one logical module.
#[derive(Clone, Debug)]
pub(crate) struct DeclarationExpansion {
    /// Ordinary items, including the concrete replacements for declaration invocations.
    pub(crate) items: Vec<Item>,
    /// Declaration metadata keyed by the declared type name.
    pub(crate) origins: Vec<(String, DeclarationOrigin)>,
    /// Syntax failures attributed to their authored spans.
    pub(crate) errors: Vec<syn::Error>,
}

/// Parsed declaration items paired with per-field abstract marker ranges.
type ParsedDeclarations = Vec<(Item, Vec<Vec<Range<u32>>>)>;

/// Expands every recognized declaration invocation in one logical module.
#[cfg(test)]
pub(crate) fn expand_declarations(
    items: &[Item],
    facade: &str,
    arena_ast: bool,
    cfg: &CargoCfgSnapshot,
) -> DeclarationExpansion {
    expand_declarations_with_aliases(items, facade, arena_ast, cfg, &BTreeSet::new())
}

/// Expands declarations with macro aliases resolved through selected-tree re-exports.
pub(crate) fn expand_declarations_with_aliases(
    items: &[Item],
    facade: &str,
    arena_ast: bool,
    cfg: &CargoCfgSnapshot,
    additional_aliases: &BTreeSet<String>,
) -> DeclarationExpansion {
    let mut aliases = ast_node_aliases(items, facade, cfg);
    aliases.extend(additional_aliases.iter().cloned());
    let mut expanded = Vec::new();
    let mut origins = Vec::new();
    let mut errors = Vec::new();
    for item in items {
        let Item::Macro(item_macro) = item else {
            expanded.push(item.clone());
            continue;
        };
        match configured_attribute_metas(&item_macro.attrs, cfg) {
            Ok(None) => {
                expanded.push(item.clone());
                continue;
            }
            Err(error) => {
                errors.push(syn::Error::new(item_macro.span(), error.message));
                continue;
            }
            Ok(Some(_)) => {}
        }
        if let Some(ident) = &item_macro.ident {
            // `macro_rules!` has textual scope: only later invocations are shadowed.
            aliases.remove(&ident.unraw().to_string());
            expanded.push(item.clone());
            continue;
        }
        if !is_ast_node_invocation(item_macro, facade, &aliases) {
            expanded.push(item.clone());
            continue;
        }
        match parse_declaration_items(item_macro, facade, arena_ast, cfg) {
            Ok(items) => {
                let invocation_range = span_range(item_macro.mac.path.span());
                for (concrete_item, field_marker_ranges) in items {
                    let name =
                        item_name(&concrete_item).expect("declaration parser emits named items");
                    let mut emitted_item = concrete_item.clone();
                    prepare_emitted_item(&mut emitted_item, cfg);
                    origins.push((
                        name,
                        DeclarationOrigin {
                            invocation_range: invocation_range.clone(),
                            concrete_item: emitted_item,
                            field_marker_ranges,
                        },
                    ));
                    expanded.push(concrete_item);
                }
            }
            Err(error) => errors.push(error),
        }
    }
    DeclarationExpansion {
        items: expanded,
        origins,
        errors,
    }
}

/// Removes analyzer-owned attributes from the concrete Rust declaration payload.
fn prepare_emitted_item(item: &mut Item, cfg: &CargoCfgSnapshot) {
    let attrs = match item {
        Item::Struct(item) => &mut item.attrs,
        Item::Enum(item) => &mut item.attrs,
        _ => return,
    };
    let _ = attrs.pop();
    configure_emitted_attributes(attrs, cfg);
    match item {
        Item::Struct(item) => {
            let fields = std::mem::replace(&mut item.fields, syn::Fields::Unit);
            item.fields = configure_fields(fields, cfg);
        }
        Item::Enum(item) => {
            item.variants = std::mem::take(&mut item.variants)
                .into_iter()
                .filter_map(|mut variant| {
                    configure_emitted_attributes(&mut variant.attrs, cfg).then(|| {
                        let fields = std::mem::replace(&mut variant.fields, syn::Fields::Unit);
                        variant.fields = configure_fields(fields, cfg);
                        variant
                    })
                })
                .collect();
        }
        _ => {}
    }
}

/// Applies cfg selection and retains Rust attributes while consuming grammar annotations.
fn configure_emitted_attributes(attrs: &mut Vec<Attribute>, cfg: &CargoCfgSnapshot) -> bool {
    let Ok(Some(metas)) = configured_attribute_metas(attrs, cfg) else {
        attrs.clear();
        return false;
    };
    *attrs = metas
        .into_iter()
        .filter(|meta| {
            !matches!(
                meta.path().get_ident().map(ToString::to_string).as_deref(),
                Some(
                    "lex"
                        | "node"
                        | "tok"
                        | "presence"
                        | "sep"
                        | "parse"
                        | "pratt"
                        | "spanned"
                        | "pretty"
                        | "greedy"
                        | "omit"
                )
            )
        })
        .map(|meta| syn::parse_quote_spanned!(meta.span()=> #[#meta]))
        .collect();
    true
}

/// Filters cfg-inactive fields and configures each retained field attribute list.
fn configure_fields(fields: syn::Fields, cfg: &CargoCfgSnapshot) -> syn::Fields {
    match fields {
        syn::Fields::Named(mut fields) => {
            fields.named = fields
                .named
                .into_iter()
                .filter_map(|mut field| {
                    configure_emitted_attributes(&mut field.attrs, cfg).then_some(field)
                })
                .collect();
            syn::Fields::Named(fields)
        }
        syn::Fields::Unnamed(mut fields) => {
            fields.unnamed = fields
                .unnamed
                .into_iter()
                .filter_map(|mut field| {
                    configure_emitted_attributes(&mut field.attrs, cfg).then_some(field)
                })
                .collect();
            syn::Fields::Unnamed(fields)
        }
        syn::Fields::Unit => syn::Fields::Unit,
    }
}

/// Collects ordinary imports and renames of the configured facade's outer macro.
fn ast_node_aliases(items: &[Item], facade: &str, cfg: &CargoCfgSnapshot) -> BTreeSet<String> {
    let mut aliases = BTreeSet::new();
    for item in items {
        let Item::Use(item_use) = item else { continue };
        if !matches!(
            configured_attribute_metas(&item_use.attrs, cfg),
            Ok(Some(_))
        ) {
            continue;
        }
        collect_use_alias(&item_use.tree, &mut Vec::new(), facade, &mut aliases);
    }
    aliases
}

/// Recursively finds `use facade::ast_node [as local]` leaves.
fn collect_use_alias(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    facade: &str,
    aliases: &mut BTreeSet<String>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.unraw().to_string());
            collect_use_alias(&path.tree, prefix, facade, aliases);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            if prefix == &[facade.to_owned()] && name.ident == "ast_node" {
                aliases.insert("ast_node".to_owned());
            }
        }
        syn::UseTree::Rename(rename) => {
            if prefix == &[facade.to_owned()] && rename.ident == "ast_node" {
                aliases.insert(rename.rename.unraw().to_string());
            }
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_alias(item, prefix, facade, aliases);
            }
        }
        syn::UseTree::Glob(_) => {
            if prefix == &[facade.to_owned()] {
                aliases.insert("ast_node".to_owned());
            }
        }
    }
}

/// Reports whether a macro path is a qualified or imported `ast_node!` spelling.
fn is_ast_node_invocation(item: &ItemMacro, facade: &str, aliases: &BTreeSet<String>) -> bool {
    let segments = item
        .mac
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>();
    matches!(segments.as_slice(), [root, name] if root == facade && name == "ast_node")
        || matches!(segments.as_slice(), [name] if aliases.contains(name))
}

/// Parses an invocation and lowers its fixed inner marker syntax.
fn parse_declaration_items(
    invocation: &ItemMacro,
    facade: &str,
    arena_ast: bool,
    cfg: &CargoCfgSnapshot,
) -> syn::Result<ParsedDeclarations> {
    let file: syn::File = syn::parse2(invocation.mac.tokens.clone())?;
    if file.items.is_empty() {
        return Err(syn::Error::new(
            invocation.mac.tokens.span(),
            "`ast_node!` requires at least one struct or enum declaration",
        ));
    }
    let mut declarations = Vec::new();
    for mut item in file.items {
        if !matches!(item, Item::Struct(_) | Item::Enum(_)) {
            return Err(syn::Error::new(
                item.span(),
                "`ast_node!` accepts only struct and enum declarations",
            ));
        }
        if !configure_declaration_item(&mut item, cfg)? {
            continue;
        }
        let marker_ranges = active_item_fields(&item, cfg)
            .into_iter()
            .map(|field| type_marker_ranges(&field.ty))
            .collect();
        prepare_item(&mut item, facade, arena_ast)?;
        declarations.push((item, marker_ranges));
    }
    Ok(declarations)
}

/// Selects cfg-active declarations, variants, fields, and cfg_attr expansions before validation.
fn configure_declaration_item(item: &mut Item, cfg: &CargoCfgSnapshot) -> syn::Result<bool> {
    let attrs = match item {
        Item::Struct(item) => &mut item.attrs,
        Item::Enum(item) => &mut item.attrs,
        _ => return Ok(false),
    };
    if !configure_source_attributes(attrs, cfg)? {
        return Ok(false);
    }
    match item {
        Item::Struct(item) => {
            let fields = std::mem::replace(&mut item.fields, syn::Fields::Unit);
            item.fields = configure_source_fields(fields, cfg)?;
        }
        Item::Enum(item) => {
            let mut variants = syn::punctuated::Punctuated::new();
            for mut variant in std::mem::take(&mut item.variants) {
                if configure_source_attributes(&mut variant.attrs, cfg)? {
                    let fields = std::mem::replace(&mut variant.fields, syn::Fields::Unit);
                    variant.fields = configure_source_fields(fields, cfg)?;
                    variants.push(variant);
                }
            }
            item.variants = variants;
        }
        _ => {}
    }
    Ok(true)
}

/// Expands cfg_attr and reports whether one attributed declaration component is active.
fn configure_source_attributes(
    attrs: &mut Vec<Attribute>,
    cfg: &CargoCfgSnapshot,
) -> syn::Result<bool> {
    let configured = configured_attribute_metas(attrs, cfg).map_err(|error| {
        syn::Error::new(
            attrs.first().map_or_else(Span::call_site, Attribute::span),
            error.message,
        )
    })?;
    let Some(metas) = configured else {
        attrs.clear();
        return Ok(false);
    };
    *attrs = metas
        .into_iter()
        .map(|meta| syn::parse_quote_spanned!(meta.span()=> #[#meta]))
        .collect();
    Ok(true)
}

/// Filters cfg-inactive fields before declaration-language validation.
fn configure_source_fields(
    fields: syn::Fields,
    cfg: &CargoCfgSnapshot,
) -> syn::Result<syn::Fields> {
    Ok(match fields {
        syn::Fields::Named(mut fields) => {
            let mut active = syn::punctuated::Punctuated::new();
            for mut field in fields.named {
                if configure_source_attributes(&mut field.attrs, cfg)? {
                    active.push(field);
                }
            }
            fields.named = active;
            syn::Fields::Named(fields)
        }
        syn::Fields::Unnamed(mut fields) => {
            let mut active = syn::punctuated::Punctuated::new();
            for mut field in fields.unnamed {
                if configure_source_attributes(&mut field.attrs, cfg)? {
                    active.push(field);
                }
            }
            fields.unnamed = active;
            syn::Fields::Unnamed(fields)
        }
        syn::Fields::Unit => syn::Fields::Unit,
    })
}

/// Borrows every declaration field in the resolver's canonical flattened order.
fn active_item_fields<'item>(item: &'item Item, cfg: &CargoCfgSnapshot) -> Vec<&'item syn::Field> {
    match item {
        Item::Struct(item) => item
            .fields
            .iter()
            .filter(|field| matches!(configured_attribute_metas(&field.attrs, cfg), Ok(Some(_))))
            .collect(),
        Item::Enum(item) => item
            .variants
            .iter()
            .filter(|variant| {
                matches!(configured_attribute_metas(&variant.attrs, cfg), Ok(Some(_)))
            })
            .flat_map(|variant| {
                variant.fields.iter().filter(|field| {
                    matches!(configured_attribute_metas(&field.attrs, cfg), Ok(Some(_)))
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Counts fixed abstract container markers within one authored field type.
fn type_marker_ranges(ty: &Type) -> Vec<Range<u32>> {
    /// Collects exact authored ranges of declaration-only storage markers.
    struct Collector(Vec<Range<u32>>);
    impl<'ast> syn::visit::Visit<'ast> for Collector {
        fn visit_type(&mut self, ty: &'ast Type) {
            if let Type::Macro(node) = ty
                && node.mac.path.get_ident().is_some_and(|ident| {
                    matches!(
                        ident.to_string().as_str(),
                        "boxed" | "one_or_many" | "zero_or_many"
                    )
                })
            {
                self.0.push(span_range(node.span()));
                if let Ok(inner) = syn::parse2::<Type>(node.mac.tokens.clone()) {
                    self.visit_type(&inner);
                }
                return;
            }
            syn::visit::visit_type(self, ty);
        }
    }
    let mut collector = Collector(Vec::new());
    collector.visit_type(ty);
    collector.0
}

/// Applies declaration defaults, restrictions, and representation-specific containers.
fn prepare_item(item: &mut Item, facade: &str, arena_ast: bool) -> syn::Result<()> {
    let item_span = item.span();
    let facade_ident = syn::Ident::new(facade, item_span);
    validate_field_lifetimes(item)?;
    let (attrs, vis, generics) = match item {
        Item::Struct(item) => (&mut item.attrs, &mut item.vis, &mut item.generics),
        Item::Enum(item) => (&mut item.attrs, &mut item.vis, &mut item.generics),
        _ => unreachable!("declaration parser checked item kind"),
    };
    validate_generics(generics)?;
    if matches!(vis, Visibility::Inherited) {
        *vis = parse_quote_spanned!(item_span=> pub);
    }
    let derive: Attribute = syn::parse_quote_spanned!(item_span=> #[derive(#facade_ident::Node)]);
    attrs.push(derive);
    let mut lowerer = MarkerLowerer {
        facade,
        arena_ast,
        error: None,
    };
    lowerer.visit_item_mut(item);
    if let Some(error) = lowerer.error {
        return Err(error);
    }
    default_named_field_visibility(item);
    Ok(())
}

/// Rejects authored lifetimes in semantic fields without interpreting opaque bounds.
fn validate_field_lifetimes(item: &Item) -> syn::Result<()> {
    let mut finder = ExplicitLifetimeFinder::default();
    match item {
        Item::Struct(item) => {
            for field in &item.fields {
                finder.visit_type(&field.ty);
            }
        }
        Item::Enum(item) => {
            for variant in &item.variants {
                for field in &variant.fields {
                    finder.visit_type(&field.ty);
                }
            }
        }
        _ => {}
    }
    finder.first.map_or(Ok(()), |span| Err(syn::Error::new(span, "explicit lifetime arguments are not allowed in `ast_node!` declarations; generation supplies `'input`")))
}

/// Rejects declaration generic forms whose generated signature is ambiguous.
fn validate_generics(generics: &syn::Generics) -> syn::Result<()> {
    for parameter in &generics.params {
        match parameter {
            GenericParam::Lifetime(parameter) => {
                return Err(syn::Error::new(
                    parameter.span(),
                    "Node declarations cannot declare lifetime parameters; generation infers `'input`",
                ));
            }
            GenericParam::Const(parameter) => {
                return Err(syn::Error::new(
                    parameter.span(),
                    "const-generic Node declarations are not supported",
                ));
            }
            GenericParam::Type(parameter) if parameter.default.is_some() => {
                return Err(syn::Error::new(
                    parameter.span(),
                    "Node declaration type parameters cannot have defaults",
                ));
            }
            GenericParam::Type(_) => {}
        }
    }
    Ok(())
}

/// Gives named struct fields the declaration language's public default.
fn default_named_field_visibility(item: &mut Item) {
    let Item::Struct(item) = item else { return };
    for field in &mut item.fields {
        if matches!(field.vis, Visibility::Inherited) {
            field.vis = parse_quote_spanned!(field.span()=> pub);
        }
    }
}

/// Rewrites declaration-only type macros into the existing semantic container catalog.
struct MarkerLowerer<'a> {
    /// Configured facade used for arena and non-empty-vector paths.
    facade: &'a str,
    /// Whether generated nested storage uses arena-backed containers.
    arena_ast: bool,
    /// First marker syntax error, retained to avoid cascading diagnostics.
    error: Option<syn::Error>,
}

impl syn::visit_mut::VisitMut for MarkerLowerer<'_> {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Macro(type_macro) = ty {
            let Some(name) = type_macro
                .mac
                .path
                .get_ident()
                .map(|ident| ident.unraw().to_string())
            else {
                self.error = Some(syn::Error::new(
                    type_macro.span(),
                    "declaration type markers must be unqualified `boxed!`, `one_or_many!`, or `zero_or_many!`",
                ));
                return;
            };
            if matches!(name.as_str(), "boxed" | "one_or_many" | "zero_or_many") {
                let Ok(mut inner) = syn::parse2::<Type>(type_macro.mac.tokens.clone()) else {
                    self.error = Some(syn::Error::new(
                        type_macro.span(),
                        format!("`{name}!` requires exactly one type"),
                    ));
                    return;
                };
                self.visit_type_mut(&mut inner);
                let span = type_macro.span();
                let facade = syn::Ident::new(self.facade, span);
                *ty = match (name.as_str(), self.arena_ast) {
                    ("boxed", false) => parse_quote_spanned!(span=> Box<#inner>),
                    ("boxed", true) => {
                        parse_quote_spanned!(span=> #facade::ArenaBox<'input, #inner>)
                    }
                    ("one_or_many", false) => parse_quote_spanned!(span=> #facade::Vec1<#inner>),
                    ("one_or_many", true) => {
                        parse_quote_spanned!(span=> #facade::ArenaVec1<'input, #inner>)
                    }
                    ("zero_or_many", false) => parse_quote_spanned!(span=> Vec<#inner>),
                    ("zero_or_many", true) => {
                        parse_quote_spanned!(span=> #facade::ArenaVec<'input, #inner>)
                    }
                    _ => unreachable!(),
                };
                return;
            }
            self.error = Some(syn::Error::new(
                type_macro.span(),
                "unsupported declaration type macro; only `boxed!`, `one_or_many!`, and `zero_or_many!` are accepted",
            ));
            return;
        }
        syn::visit_mut::visit_type_mut(self, ty);
    }
}

/// Finds forbidden explicit lifetimes in authored semantic type syntax.
#[derive(Default)]
struct ExplicitLifetimeFinder {
    /// First authored lifetime location.
    first: Option<Span>,
}

impl<'ast> syn::visit::Visit<'ast> for ExplicitLifetimeFinder {
    fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
        if self.first.is_none() {
            self.first = Some(lifetime.span());
        }
    }

    fn visit_type_macro(&mut self, node: &'ast syn::TypeMacro) {
        if self.first.is_none()
            && let Ok(inner) = syn::parse2::<Type>(node.mac.tokens.clone())
        {
            let mut nested = Self::default();
            nested.visit_type(&inner);
            self.first = nested.first;
        }
    }
}

/// Returns the declared identifier for a supported item.
fn item_name(item: &Item) -> Option<String> {
    match item {
        Item::Struct(item) => Some(item.ident.unraw().to_string()),
        Item::Enum(item) => Some(item.ident.unraw().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use quote::ToTokens as _;

    use super::expand_declarations;
    use crate::source_syntax::CargoCfgSnapshot;

    /// Parses source items for isolated declaration expansion tests.
    fn items(source: &str) -> Vec<syn::Item> {
        syn::parse_file(source).unwrap().items
    }

    #[test]
    fn qualified_declarations_lower_markers_and_apply_public_defaults() {
        let expansion = expand_declarations(
            &items("recursa::ast_node! { struct Root { values: one_or_many!(boxed!(Leaf)) } }"),
            "recursa",
            false,
            &CargoCfgSnapshot::default(),
        );
        assert!(expansion.errors.is_empty());
        assert_eq!(
            expansion.origins[0].1.field_marker_ranges,
            vec![vec![43..69, 56..68]]
        );
        let rendered = expansion.items[0].to_token_stream().to_string();
        assert!(rendered.contains("pub struct Root"));
        assert!(rendered.contains("pub values : recursa :: Vec1 < Box < Leaf > >"));
        let emitted = expansion.origins[0]
            .1
            .concrete_item
            .to_token_stream()
            .to_string();
        assert!(!emitted.contains("recursa :: Node"));
    }

    #[test]
    fn imported_and_renamed_outer_macros_are_discovered_but_unrelated_bare_macros_are_not() {
        let expansion = expand_declarations(
            &items(
                "use recursa::ast_node as declare; declare! { enum Root { Unit } } ast_node! { nonsense }",
            ),
            "recursa",
            false,
            &CargoCfgSnapshot::default(),
        );
        assert!(expansion.errors.is_empty());
        assert_eq!(expansion.origins.len(), 1);
        assert!(matches!(expansion.items[2], syn::Item::Macro(_)));
    }

    #[test]
    fn facade_glob_import_discovers_ast_node_unless_a_local_macro_shadows_it() {
        let imported = expand_declarations(
            &items("use recursa::*; ast_node! { struct Root; }"),
            "recursa",
            false,
            &CargoCfgSnapshot::default(),
        );
        assert_eq!(imported.origins.len(), 1);

        let shadowed = expand_declarations(
            &items(
                "use recursa::*; macro_rules! ast_node { ($($t:tt)*) => {} } ast_node! { unrelated }",
            ),
            "recursa",
            false,
            &CargoCfgSnapshot::default(),
        );
        assert!(shadowed.origins.is_empty());
        assert!(shadowed.errors.is_empty());
    }

    #[test]
    fn inactive_local_macro_does_not_shadow_an_imported_declaration_macro() {
        let expansion = expand_declarations(
            &items(
                "use recursa::ast_node; #[cfg(any())] macro_rules! ast_node { ($($t:tt)*) => {} } ast_node! { struct Root; }",
            ),
            "recursa",
            false,
            &CargoCfgSnapshot::default(),
        );
        assert!(expansion.errors.is_empty());
        assert_eq!(
            expansion
                .origins
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Root"]
        );
    }

    #[test]
    fn local_macro_shadows_only_later_bare_invocations() {
        let expansion = expand_declarations(
            &items(
                "use recursa::ast_node; ast_node! { struct Before; } macro_rules! ast_node { ($($t:tt)*) => {} } ast_node! { unrelated tokens }",
            ),
            "recursa",
            false,
            &CargoCfgSnapshot::default(),
        );
        assert!(expansion.errors.is_empty());
        assert_eq!(
            expansion
                .origins
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Before"]
        );
        assert!(matches!(expansion.items.last(), Some(syn::Item::Macro(_))));
    }

    #[test]
    fn inactive_declarations_do_not_parse_their_invalid_body() {
        let expansion = expand_declarations(
            &items("#[cfg(windows)] recursa::ast_node! { invalid declaration tokens }"),
            "recursa",
            false,
            &CargoCfgSnapshot::default().with_flag("unix"),
        );
        assert!(expansion.errors.is_empty());
        assert!(expansion.origins.is_empty());
    }

    #[test]
    fn inactive_inner_items_and_fields_are_skipped_before_declaration_validation() {
        let expansion = expand_declarations(
            &items(
                "recursa::ast_node! { #[cfg(any())] struct Ignored<'a> { bad: &'a str } struct Root { #[cfg(any())] bad: unsupported!(Leaf) } }",
            ),
            "recursa",
            false,
            &CargoCfgSnapshot::default(),
        );
        assert!(expansion.errors.is_empty(), "{:?}", expansion.errors);
        assert_eq!(expansion.origins.len(), 1);
        let emitted = expansion.origins[0]
            .1
            .concrete_item
            .to_token_stream()
            .to_string();
        assert!(emitted.contains("struct Root"));
        assert!(!emitted.contains("bad"));
    }

    #[test]
    fn emitted_items_select_cfg_attr_and_remove_nested_grammar_attributes() {
        let expansion = expand_declarations(
            &items(
                "recursa::ast_node! { #[cfg_attr(unix, derive(Clone))] struct Root { #[cfg_attr(unix, tok(OPEN))] active: Leaf, #[cfg(windows)] inactive: Leaf } }",
            ),
            "recursa",
            false,
            &CargoCfgSnapshot::default().with_flag("unix"),
        );
        assert!(expansion.errors.is_empty());
        let emitted = expansion.origins[0]
            .1
            .concrete_item
            .to_token_stream()
            .to_string();
        assert!(emitted.contains("derive (Clone)"), "{emitted}");
        assert!(emitted.contains("active : Leaf"), "{emitted}");
        assert!(!emitted.contains("tok"), "{emitted}");
        assert!(!emitted.contains("inactive"), "{emitted}");
    }

    #[test]
    fn arena_markers_lower_to_arena_containers_with_the_generated_lifetime() {
        let expansion = expand_declarations(
            &items(
                "recursa::ast_node! { pub(crate) struct Root { pub(super) values: zero_or_many!(Leaf) } }",
            ),
            "recursa",
            true,
            &CargoCfgSnapshot::default(),
        );
        assert!(expansion.errors.is_empty());
        let rendered = expansion.items[0].to_token_stream().to_string();
        assert!(rendered.contains("pub (crate) struct Root"));
        assert!(rendered.contains("pub (super) values : recursa :: ArenaVec < 'input , Leaf >"));
    }

    #[test]
    fn explicit_lifetimes_and_unsupported_generic_forms_are_rejected() {
        for source in [
            "recursa::ast_node! { struct Root<'a> { value: Leaf } }",
            "recursa::ast_node! { struct Root<T = Leaf> { value: T } }",
            "recursa::ast_node! { struct Root<const N: usize> { value: Leaf } }",
            "recursa::ast_node! { struct Root { value: Leaf<'input> } }",
        ] {
            let expansion = expand_declarations(
                &items(source),
                "recursa",
                false,
                &CargoCfgSnapshot::default(),
            );
            assert_eq!(expansion.errors.len(), 1, "{source}");
        }
    }
}
