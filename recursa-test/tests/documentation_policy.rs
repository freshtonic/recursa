//! Stable syntax-aware policy for authored, documentation-hidden Rust API.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::ext::IdentExt as _;
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Item, Lit, Meta, Token, UseTree, Visibility};

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
    missing: Vec<MissingDocumentation>,
}

#[derive(Default)]
struct Inventory {
    hidden_types: BTreeSet<Vec<String>>,
    bindings: BTreeMap<Vec<String>, BTreeSet<Vec<String>>>,
}

struct CheckState<'inventory, 'visited> {
    inventory: &'inventory Inventory,
    visited: &'visited mut BTreeSet<(PathBuf, Vec<String>, bool)>,
}

impl Policy {
    fn check_crate(&mut self, path: &Path) {
        let mut inventory = Inventory::default();
        let mut visited = BTreeSet::new();
        inventory_file(path, &[], false, &mut inventory, &mut visited);
        visited.clear();
        self.check_file(
            path,
            &[],
            false,
            &mut CheckState {
                inventory: &inventory,
                visited: &mut visited,
            },
        );
    }

    fn check_file(
        &mut self,
        path: &Path,
        module: &[String],
        inherited_hidden: bool,
        state: &mut CheckState<'_, '_>,
    ) {
        let canonical = path.canonicalize().expect("canonical authored module path");
        if !state
            .visited
            .insert((canonical, module.to_vec(), inherited_hidden))
        {
            return;
        }
        let source = std::fs::read_to_string(path).expect("read authored Rust library source");
        let file = syn::parse_file(&source).expect("parse authored Rust library source");
        let module_dir = module_directory(path);
        self.check_items(
            path,
            &module_dir,
            module,
            &file.items,
            inherited_hidden,
            state,
        );
    }

    fn check_items(
        &mut self,
        source: &Path,
        module_dir: &Path,
        module_path: &[String],
        items: &[Item],
        inherited_hidden: bool,
        state: &mut CheckState<'_, '_>,
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
                    let mut child_module = module_path.to_vec();
                    child_module.push(module.ident.unraw().to_string());
                    if let Some((_, items)) = &module.content {
                        let child_module_dir = module_dir.join(module.ident.unraw().to_string());
                        self.check_items(
                            source,
                            &child_module_dir,
                            &child_module,
                            items,
                            hidden,
                            state,
                        );
                    } else if let Some(path) = resolve_module(source, module_dir, module) {
                        self.check_file(&path, &child_module, hidden, state);
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
                        hidden || impl_targets_hidden_type(module_path, item, state.inventory);
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
    let mut inventory = Inventory::default();
    inventory_source_items(&[], &file.items, false, &mut inventory);
    let mut policy = Policy::default();
    policy.check_items(
        Path::new("fixture.rs"),
        Path::new("."),
        &[],
        &file.items,
        false,
        &mut CheckState {
            inventory: &inventory,
            visited: &mut BTreeSet::new(),
        },
    );
    policy.missing
}

fn inventory_source_items(
    module: &[String],
    items: &[Item],
    inherited_hidden: bool,
    inventory: &mut Inventory,
) {
    inventory_items(module, items, inherited_hidden, inventory);
    for item in items {
        let Item::Mod(child) = item else { continue };
        if definitely_test_only(&child.attrs) {
            continue;
        }
        let Some((_, items)) = &child.content else {
            continue;
        };
        let mut child_module = module.to_vec();
        child_module.push(child.ident.unraw().to_string());
        inventory_source_items(
            &child_module,
            items,
            inherited_hidden || doc_hidden(&child.attrs),
            inventory,
        );
    }
}

fn inventory_file(
    path: &Path,
    module: &[String],
    inherited_hidden: bool,
    inventory: &mut Inventory,
    visited: &mut BTreeSet<(PathBuf, Vec<String>, bool)>,
) {
    let canonical = path.canonicalize().expect("canonical authored module path");
    if !visited.insert((canonical, module.to_vec(), inherited_hidden)) {
        return;
    }
    let source = std::fs::read_to_string(path).expect("read authored Rust library source");
    let file = syn::parse_file(&source).expect("parse authored Rust library source");
    inventory_items(module, &file.items, inherited_hidden, inventory);
    let module_dir = module_directory(path);
    for item in &file.items {
        let Item::Mod(child) = item else { continue };
        if definitely_test_only(&child.attrs) {
            continue;
        }
        let hidden = inherited_hidden || doc_hidden(&child.attrs);
        let mut child_module = module.to_vec();
        let child_name = child.ident.unraw().to_string();
        child_module.push(child_name.clone());
        if let Some((_, items)) = &child.content {
            let child_module_dir = module_dir.join(child_name);
            inventory_inline_modules(
                path,
                &child_module_dir,
                &child_module,
                items,
                hidden,
                inventory,
                visited,
            );
        } else if let Some(path) = resolve_module(path, &module_dir, child) {
            inventory_file(&path, &child_module, hidden, inventory, visited);
        }
    }
}

fn inventory_inline_modules(
    source: &Path,
    module_dir: &Path,
    module: &[String],
    items: &[Item],
    inherited_hidden: bool,
    inventory: &mut Inventory,
    visited: &mut BTreeSet<(PathBuf, Vec<String>, bool)>,
) {
    inventory_items(module, items, inherited_hidden, inventory);
    for item in items {
        let Item::Mod(child) = item else { continue };
        if definitely_test_only(&child.attrs) {
            continue;
        }
        let hidden = inherited_hidden || doc_hidden(&child.attrs);
        let mut child_module = module.to_vec();
        let child_name = child.ident.unraw().to_string();
        child_module.push(child_name.clone());
        if let Some((_, items)) = &child.content {
            let child_module_dir = module_dir.join(child_name);
            inventory_inline_modules(
                source,
                &child_module_dir,
                &child_module,
                items,
                hidden,
                inventory,
                visited,
            );
        } else if let Some(path) = resolve_module(source, module_dir, child) {
            inventory_file(&path, &child_module, hidden, inventory, visited);
        }
    }
}

fn inventory_items(
    module: &[String],
    items: &[Item],
    inherited_hidden: bool,
    inventory: &mut Inventory,
) {
    for item in items {
        let attrs = item_attrs(item);
        if definitely_test_only(attrs) {
            continue;
        }
        let hidden = inherited_hidden || doc_hidden(attrs);
        let ident = match item {
            Item::Struct(item) if hidden => Some(&item.ident),
            Item::Enum(item) if hidden => Some(&item.ident),
            Item::Union(item) if hidden => Some(&item.ident),
            _ => None,
        };
        if let Some(ident) = ident {
            let mut identity = module.to_vec();
            identity.push(ident.unraw().to_string());
            inventory.hidden_types.insert(identity);
        }
        if let Item::Use(item) = item {
            collect_use_bindings(module, &item.tree, &[], inventory);
        }
    }
}

fn collect_use_bindings(
    module: &[String],
    tree: &UseTree,
    prefix: &[String],
    inventory: &mut Inventory,
) {
    match tree {
        UseTree::Path(path) => {
            let mut target = prefix.to_vec();
            target.push(path.ident.unraw().to_string());
            collect_use_bindings(module, &path.tree, &target, inventory);
        }
        UseTree::Name(name) => {
            let name = name.ident.unraw().to_string();
            if name == "self" {
                if let Some(local) = prefix.last() {
                    insert_use_binding(module, local, prefix, inventory);
                }
                return;
            }
            let mut target = prefix.to_vec();
            target.push(name.clone());
            insert_use_binding(module, &name, &target, inventory);
        }
        UseTree::Rename(rename) => {
            let mut target = prefix.to_vec();
            if rename.ident != "self" {
                target.push(rename.ident.unraw().to_string());
            }
            insert_use_binding(
                module,
                &rename.rename.unraw().to_string(),
                &target,
                inventory,
            );
        }
        UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_bindings(module, tree, prefix, inventory);
            }
        }
        UseTree::Glob(_) => {}
    }
}

fn insert_use_binding(
    module: &[String],
    local: &str,
    target: &[String],
    inventory: &mut Inventory,
) {
    let mut binding = module.to_vec();
    binding.push(local.to_owned());
    if let Some(target) = normalize_segments(module, target) {
        inventory
            .bindings
            .entry(binding)
            .or_default()
            .insert(target);
    }
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
    module: &[String],
    item: &syn::ItemImpl,
    inventory: &Inventory,
) -> bool {
    let syn::Type::Path(path) = item.self_ty.as_ref() else {
        return false;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() {
        return false;
    }
    let segments = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>();
    let Some(identity) = normalize_segments(module, &segments) else {
        return false;
    };
    let mut pending = vec![identity];
    let mut seen = BTreeSet::new();
    while let Some(identity) = pending.pop() {
        if !seen.insert(identity.clone()) {
            continue;
        }
        if inventory.hidden_types.contains(&identity) {
            return true;
        }
        let binding_length = inventory
            .bindings
            .keys()
            .filter(|binding| identity.starts_with(binding.as_slice()))
            .map(Vec::len)
            .max();
        let Some(binding_length) = binding_length else {
            continue;
        };
        for (binding, targets) in inventory.bindings.iter().filter(|(binding, _)| {
            binding.len() == binding_length && identity.starts_with(binding.as_slice())
        }) {
            for target in targets {
                let mut resolved = target.clone();
                resolved.extend_from_slice(&identity[binding.len()..]);
                pending.push(resolved);
            }
        }
    }
    false
}

fn normalize_segments(module: &[String], segments: &[String]) -> Option<Vec<String>> {
    let (mut resolved, index) = match segments.first().map(String::as_str) {
        Some("crate") => (Vec::new(), 1),
        Some("self") => (module.to_vec(), 1),
        Some("super") => {
            let mut resolved = module.to_vec();
            let mut index = 0;
            while segments
                .get(index)
                .is_some_and(|segment| segment == "super")
            {
                resolved.pop()?;
                index += 1;
            }
            (resolved, index)
        }
        Some(_) => (module.to_vec(), 0),
        None => return None,
    };
    resolved.extend_from_slice(&segments[index..]);
    Some(resolved)
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
        policy.check_crate(&workspace.join(root));
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
fn cross_file_hidden_impls_resolve_qualified_reexports_without_name_collisions() {
    let package = tempfile::tempdir().expect("temporary documentation-policy crate");
    std::fs::write(
        package.path().join("lib.rs"),
        "mod hidden; mod api; mod sibling; mod ordinary;",
    )
    .expect("write fixture crate root");
    std::fs::write(
        package.path().join("hidden.rs"),
        r#"
        /// Matching-version wire type.
        #[doc(hidden)]
        pub struct Wire;
        "#,
    )
    .expect("write hidden type module");
    std::fs::write(
        package.path().join("api.rs"),
        r#"
        #[cfg(feature = "hidden-protocol")]
        pub use crate::hidden::Wire as ProtocolWire;
        #[cfg(not(feature = "hidden-protocol"))]
        pub use crate::ordinary::Wire as ProtocolWire;
        "#,
    )
    .expect("write re-export module");
    std::fs::write(
        package.path().join("ordinary.rs"),
        r#"
        pub struct Wire;
        impl Wire {
            pub fn same_name_but_not_hidden() {}
        }
        "#,
    )
    .expect("write same-name ordinary type module");
    let sibling = package.path().join("sibling.rs");
    std::fs::write(
        &sibling,
        r#"
        impl crate::api::ProtocolWire {
            pub fn undocumented_cross_file_method() {}
        }
        "#,
    )
    .expect("write undocumented sibling impl");

    let mut policy = Policy::default();
    policy.check_crate(&package.path().join("lib.rs"));
    assert_eq!(
        policy
            .missing
            .iter()
            .map(|item| item.item.as_str())
            .collect::<Vec<_>>(),
        ["undocumented_cross_file_method"],
    );

    std::fs::write(
        sibling,
        r#"
        impl crate::api::ProtocolWire {
            /// Decodes the matching-version wire representation.
            pub fn documented_cross_file_method() {}
        }
        "#,
    )
    .expect("write documented sibling impl");
    let mut policy = Policy::default();
    policy.check_crate(&package.path().join("lib.rs"));
    assert!(
        policy.missing.is_empty(),
        "documented hidden impl and unrelated same-name type must pass: {:#?}",
        policy.missing,
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
