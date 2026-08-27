//! Stable syntax-aware policy for authored, documentation-hidden Rust API.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Item, Lit, Meta, Token, Visibility};

const LIBRARY_ROOTS: &[&str] = &[
    "recursa/src/lib.rs",
    "recursa-core/src/lib.rs",
    "recursa-codegen/src/lib.rs",
    "recursa-macros/src/lib.rs",
    "recursa-test/src/lib.rs",
    "recursa-explorer/src/lib.rs",
];

#[derive(Debug, Eq, PartialEq)]
struct MissingDocumentation {
    source: String,
    item: String,
}

#[derive(Default)]
struct Policy {
    visited: BTreeSet<(PathBuf, bool)>,
    hidden_types: BTreeSet<(PathBuf, String)>,
    missing: Vec<MissingDocumentation>,
}

impl Policy {
    fn check_library(&mut self, path: &Path, inherited_hidden: bool) {
        let source = std::fs::read_to_string(path).expect("read authored Rust library source");
        let file = syn::parse_file(&source).expect("parse authored Rust library source");
        self.check_file(path, &file, inherited_hidden);
    }

    fn check_file(&mut self, path: &Path, file: &syn::File, inherited_hidden: bool) {
        let canonical = path.canonicalize().expect("canonical authored module path");
        if !self.visited.insert((canonical, inherited_hidden)) {
            return;
        }
        self.collect_hidden_types(path, &file.items, inherited_hidden);
        let module_dir = module_directory(path);
        self.check_items(path, &module_dir, &file.items, inherited_hidden);
    }

    fn collect_hidden_types(&mut self, source: &Path, items: &[Item], inherited_hidden: bool) {
        for item in items {
            if definitely_test_only(item_attrs(item)) {
                continue;
            }
            let hidden = inherited_hidden || doc_hidden(item_attrs(item));
            match item {
                Item::Struct(item) if hidden => {
                    self.hidden_types
                        .insert((source.to_owned(), item.ident.to_string()));
                }
                Item::Enum(item) if hidden => {
                    self.hidden_types
                        .insert((source.to_owned(), item.ident.to_string()));
                }
                Item::Union(item) if hidden => {
                    self.hidden_types
                        .insert((source.to_owned(), item.ident.to_string()));
                }
                Item::Mod(item) => {
                    if let Some((_, items)) = &item.content {
                        self.collect_hidden_types(source, items, hidden);
                    }
                }
                _ => {}
            }
        }
    }

    fn check_items(
        &mut self,
        source: &Path,
        module_dir: &Path,
        items: &[Item],
        inherited_hidden: bool,
    ) {
        for item in items {
            let attrs = item_attrs(item);
            if definitely_test_only(attrs) {
                continue;
            }
            let hidden = inherited_hidden || doc_hidden(attrs);
            let public = item_visibility(item).is_some_and(is_public) || exported_macro(item);
            if hidden && public {
                self.require_docs(source, item_name(item), attrs);
            }
            match item {
                Item::Mod(module) => {
                    if let Some((_, items)) = &module.content {
                        self.check_items(source, module_dir, items, hidden);
                    } else if let Some(path) = resolve_module(source, module_dir, module) {
                        self.check_library(&path, hidden);
                    }
                }
                Item::Struct(item) if hidden && public => {
                    for (index, field) in item.fields.iter().enumerate() {
                        if is_public(&field.vis) && !definitely_test_only(&field.attrs) {
                            self.require_docs(
                                source,
                                field
                                    .ident
                                    .as_ref()
                                    .map_or_else(|| format!("field {index}"), ToString::to_string),
                                &field.attrs,
                            );
                        }
                    }
                }
                Item::Enum(item) if hidden && public => {
                    for variant in &item.variants {
                        if definitely_test_only(&variant.attrs) {
                            continue;
                        }
                        self.require_docs(source, variant.ident.to_string(), &variant.attrs);
                        for (index, field) in variant.fields.iter().enumerate() {
                            if definitely_test_only(&field.attrs) {
                                continue;
                            }
                            self.require_docs(
                                source,
                                field
                                    .ident
                                    .as_ref()
                                    .map_or_else(|| format!("field {index}"), ToString::to_string),
                                &field.attrs,
                            );
                        }
                    }
                }
                Item::Union(item) if hidden && public => {
                    for field in &item.fields.named {
                        if is_public(&field.vis) && !definitely_test_only(&field.attrs) {
                            self.require_docs(
                                source,
                                field.ident.as_ref().expect("named union field").to_string(),
                                &field.attrs,
                            );
                        }
                    }
                }
                Item::Trait(item) if hidden && public => {
                    for child in &item.items {
                        let attrs = trait_item_attrs(child);
                        if !definitely_test_only(attrs) {
                            self.require_docs(source, trait_item_name(child), attrs);
                        }
                    }
                }
                Item::Impl(item) => {
                    let hidden_impl =
                        hidden || impl_targets_hidden_type(source, item, &self.hidden_types);
                    if hidden_impl {
                        for child in &item.items {
                            let attrs = impl_item_attrs(child);
                            if impl_item_public(child) && !definitely_test_only(attrs) {
                                self.require_docs(source, impl_item_name(child), attrs);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn require_docs(&mut self, source: &Path, item: String, attrs: &[Attribute]) {
        if !has_descriptive_docs(attrs) {
            self.missing.push(MissingDocumentation {
                source: source.display().to_string(),
                item,
            });
        }
    }
}

fn check_source(source: &str) -> Vec<MissingDocumentation> {
    let file = syn::parse_file(source).expect("parse policy fixture");
    let mut policy = Policy::default();
    policy.collect_hidden_types(Path::new("fixture.rs"), &file.items, false);
    policy.check_items(Path::new("fixture.rs"), Path::new("."), &file.items, false);
    policy.missing
}

fn doc_hidden(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        attribute.path().is_ident("doc")
            && match &attribute.meta {
                Meta::List(list) => list
                    .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                    .is_ok_and(|items| items.iter().any(|meta| meta.path().is_ident("hidden"))),
                Meta::Path(_) | Meta::NameValue(_) => false,
            }
    })
}

fn has_descriptive_docs(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attribute| match &attribute.meta {
        Meta::NameValue(value) if value.path.is_ident("doc") => {
            matches!(&value.value, Expr::Lit(literal) if matches!(&literal.lit, Lit::Str(text) if !text.value().trim().is_empty()))
        }
        Meta::Path(_) | Meta::List(_) | Meta::NameValue(_) => false,
    })
}

fn definitely_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && match &attribute.meta {
                Meta::List(list) => syn::parse2::<Meta>(list.tokens.clone())
                    .is_ok_and(|meta| cfg_requires_test(&meta)),
                Meta::Path(_) | Meta::NameValue(_) => false,
            }
    })
}

fn cfg_requires_test(meta: &Meta) -> bool {
    if meta.path().is_ident("test") {
        return true;
    }
    let Meta::List(list) = meta else {
        return false;
    };
    let Ok(items) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) else {
        return false;
    };
    if list.path.is_ident("all") {
        items.iter().any(cfg_requires_test)
    } else if list.path.is_ident("any") {
        !items.is_empty() && items.iter().all(cfg_requires_test)
    } else {
        false
    }
}

fn resolve_module(source: &Path, module_dir: &Path, module: &syn::ItemMod) -> Option<PathBuf> {
    if definitely_test_only(&module.attrs) {
        return None;
    }
    if let Some(path) = module.attrs.iter().find_map(path_attribute) {
        return Some(source.parent().expect("module source parent").join(path));
    }
    let name = module.ident.to_string();
    let direct = module_dir.join(format!("{name}.rs"));
    if direct.is_file() {
        Some(direct)
    } else {
        let nested = module_dir.join(&name).join("mod.rs");
        nested.is_file().then_some(nested)
    }
}

fn path_attribute(attribute: &Attribute) -> Option<String> {
    let Meta::NameValue(value) = &attribute.meta else {
        return None;
    };
    if !value.path.is_ident("path") {
        return None;
    }
    match &value.value {
        Expr::Lit(literal) => match &literal.lit {
            Lit::Str(path) => Some(path.value()),
            _ => None,
        },
        _ => None,
    }
}

fn module_directory(path: &Path) -> PathBuf {
    let parent = path.parent().expect("library module parent");
    if path
        .file_name()
        .is_some_and(|name| name == "lib.rs" || name == "mod.rs")
    {
        parent.to_owned()
    } else {
        parent.join(path.file_stem().expect("module stem"))
    }
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn exported_macro(item: &Item) -> bool {
    matches!(item, Item::Macro(item) if item.attrs.iter().any(|attribute| attribute.path().is_ident("macro_export")))
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn item_visibility(item: &Item) -> Option<&Visibility> {
    match item {
        Item::Const(item) => Some(&item.vis),
        Item::Enum(item) => Some(&item.vis),
        Item::ExternCrate(item) => Some(&item.vis),
        Item::Fn(item) => Some(&item.vis),
        Item::Mod(item) => Some(&item.vis),
        Item::Static(item) => Some(&item.vis),
        Item::Struct(item) => Some(&item.vis),
        Item::Trait(item) => Some(&item.vis),
        Item::TraitAlias(item) => Some(&item.vis),
        Item::Type(item) => Some(&item.vis),
        Item::Union(item) => Some(&item.vis),
        // A public re-export inherits the target item's documentation rather than defining a
        // second independently documentable ABI item.
        Item::Use(_) => None,
        Item::Impl(_) | Item::Macro(_) | Item::ForeignMod(_) | _ => None,
    }
}

fn item_name(item: &Item) -> String {
    match item {
        Item::Const(item) => item.ident.to_string(),
        Item::Enum(item) => item.ident.to_string(),
        Item::ExternCrate(item) => item.ident.to_string(),
        Item::Fn(item) => item.sig.ident.to_string(),
        Item::Mod(item) => item.ident.to_string(),
        Item::Static(item) => item.ident.to_string(),
        Item::Struct(item) => item.ident.to_string(),
        Item::Trait(item) => item.ident.to_string(),
        Item::TraitAlias(item) => item.ident.to_string(),
        Item::Type(item) => item.ident.to_string(),
        Item::Union(item) => item.ident.to_string(),
        Item::Use(_) => "use declaration".to_owned(),
        Item::Macro(item) => item
            .ident
            .as_ref()
            .map_or_else(|| "macro invocation".to_owned(), ToString::to_string),
        _ => "item".to_owned(),
    }
}

fn impl_targets_hidden_type(
    source: &Path,
    item: &syn::ItemImpl,
    hidden_types: &BTreeSet<(PathBuf, String)>,
) -> bool {
    let syn::Type::Path(path) = item.self_ty.as_ref() else {
        return false;
    };
    path.path.segments.last().is_some_and(|segment| {
        hidden_types.contains(&(source.to_owned(), segment.ident.to_string()))
    })
}

fn impl_item_public(item: &syn::ImplItem) -> bool {
    match item {
        syn::ImplItem::Const(item) => is_public(&item.vis),
        syn::ImplItem::Fn(item) => is_public(&item.vis),
        syn::ImplItem::Type(item) => is_public(&item.vis),
        _ => false,
    }
}

fn impl_item_attrs(item: &syn::ImplItem) -> &[Attribute] {
    match item {
        syn::ImplItem::Const(item) => &item.attrs,
        syn::ImplItem::Fn(item) => &item.attrs,
        syn::ImplItem::Type(item) => &item.attrs,
        syn::ImplItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn impl_item_name(item: &syn::ImplItem) -> String {
    match item {
        syn::ImplItem::Const(item) => item.ident.to_string(),
        syn::ImplItem::Fn(item) => item.sig.ident.to_string(),
        syn::ImplItem::Type(item) => item.ident.to_string(),
        _ => "associated item".to_owned(),
    }
}

fn trait_item_attrs(item: &syn::TraitItem) -> &[Attribute] {
    match item {
        syn::TraitItem::Const(item) => &item.attrs,
        syn::TraitItem::Fn(item) => &item.attrs,
        syn::TraitItem::Type(item) => &item.attrs,
        syn::TraitItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn trait_item_name(item: &syn::TraitItem) -> String {
    match item {
        syn::TraitItem::Const(item) => item.ident.to_string(),
        syn::TraitItem::Fn(item) => item.sig.ident.to_string(),
        syn::TraitItem::Type(item) => item.ident.to_string(),
        _ => "trait item".to_owned(),
    }
}

#[test]
fn authored_library_graph_has_no_undocumented_hidden_abi() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let mut policy = Policy::default();
    for root in LIBRARY_ROOTS {
        policy.check_library(&workspace.join(root), false);
    }
    assert!(
        policy.missing.is_empty(),
        "undocumented #[doc(hidden)] authored ABI:\n{:#?}",
        policy.missing,
    );
}

#[test]
fn undocumented_hidden_and_feature_gated_items_fail() {
    let missing = check_source(
        r#"
        #[doc(hidden)]
        pub struct Hidden;
        #[cfg(feature = "optional")]
        #[doc(hidden)]
        pub fn gated() {}
        #[doc(hidden)]
        #[macro_export]
        macro_rules! hidden_macro { () => {}; }
        "#,
    );
    assert_eq!(
        missing
            .iter()
            .map(|item| item.item.as_str())
            .collect::<Vec<_>>(),
        ["Hidden", "gated", "hidden_macro"],
    );
}

#[test]
fn hidden_descendants_require_their_own_documentation() {
    let missing = check_source(
        r#"
        /// Hidden protocol.
        #[doc(hidden)]
        pub mod protocol {
            pub struct Message {
                pub payload: String,
            }
        }
        /// Hidden wire type.
        #[doc(hidden)]
        pub struct Wire;
        impl Wire {
            pub fn decode() {}
        }
        "#,
    );
    assert_eq!(
        missing
            .iter()
            .map(|item| item.item.as_str())
            .collect::<Vec<_>>(),
        ["Message", "payload", "decode"],
    );
}

#[test]
fn documented_test_only_and_macro_generated_items_pass() {
    let missing = check_source(
        r#"
        /// Hidden protocol.
        #[doc(hidden)]
        pub mod protocol {
            /// Protocol message.
            pub struct Message {
                /// Wire payload.
                pub payload: String,
                #[cfg(test)]
                pub test_payload: String,
            }
            impl Message {
                #[cfg(test)]
                pub fn test_helper() {}
            }
        }
        #[cfg(test)]
        #[doc(hidden)]
        pub fn test_helper() {}
        macro_rules! generated {
            () => { #[doc(hidden)] pub fn expanded_without_docs() {} };
        }
        "#,
    );
    assert!(
        missing.is_empty(),
        "unexpected policy failures: {missing:#?}"
    );
}
