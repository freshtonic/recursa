#![allow(
    dead_code,
    reason = "private M3 resolution seam is consumed by later lowering tickets"
)]

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

use proc_macro2::Span;
use syn::ext::IdentExt as _;
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::{Attribute, GenericArgument, Item, Meta, Path, PathArguments, Token, Type, Visibility};

use crate::SourceId;
use crate::discovery::{DiscoveredModuleTree, ModuleInclusion};
use crate::lowering::{
    BuildInputs, DiscoveryInputs, FacadeMapping, ModuleId, NodeId, SelectedRootAnchor,
};
use crate::source_syntax::{CargoCfgSnapshot, GrammarConfigSyntax, configure_attributes};

macro_rules! resolution_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(u32);

        impl $name {
            fn next(length: usize) -> Self {
                Self(length.try_into().expect("resolution identity overflow"))
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

resolution_id!(SemanticUseId);
resolution_id!(ResolutionTraceId);
resolution_id!(BindingId);
resolution_id!(GenericParameterId);

#[derive(Clone, Debug)]
pub(crate) struct ResolutionInput<'cfg> {
    pub(crate) discovery_inputs: DiscoveryInputs,
    pub(crate) tree: DiscoveredModuleTree,
    pub(crate) cfg: &'cfg CargoCfgSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolutionReport {
    pub(crate) discovery_inputs: DiscoveryInputs,
    pub(crate) build_inputs: Option<BuildInputs>,
    pub(crate) diagnostics: Vec<ResolutionDiagnostic>,
    pub(crate) resolved: Option<ResolvedSourceGrammar>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedSourceGrammar {
    pub(crate) build_inputs: BuildInputs,
    pub(crate) tree: DiscoveredModuleTree,
    pub(crate) nodes: Vec<ResolvedNode>,
    pub(crate) generic_parameters: Vec<ResolvedGenericParameter>,
    pub(crate) uses: Vec<SemanticUse>,
    pub(crate) resolutions: Vec<SemanticResolution>,
    pub(crate) traces: Vec<ResolutionTrace>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NodeKey {
    pub(crate) segments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedNode {
    pub(crate) id: NodeId,
    pub(crate) module: ModuleId,
    pub(crate) source: SourceId,
    pub(crate) ident: String,
    pub(crate) key: NodeKey,
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedGenericParameter {
    pub(crate) id: GenericParameterId,
    pub(crate) node: NodeId,
    pub(crate) ident: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticUse {
    pub(crate) id: SemanticUseId,
    pub(crate) module: ModuleId,
    pub(crate) containing_node: NodeId,
    pub(crate) source: SourceId,
    pub(crate) range: Range<u32>,
    pub(crate) authored: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticResolution {
    pub(crate) use_site: SemanticUseId,
    pub(crate) authored: String,
    pub(crate) target: SemanticTarget,
    pub(crate) trace: ResolutionTraceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SemanticTarget {
    Node(NodeId),
    GenericParameter(GenericParameterId),
    Catalog(CatalogType),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogType {
    Option,
    Box,
    Vec,
    Vec1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolutionTrace {
    pub(crate) id: ResolutionTraceId,
    pub(crate) steps: Vec<ResolutionTraceStep>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResolutionTraceStep {
    Start(ModuleId),
    ContainingNode(NodeId),
    CurrentModule(ModuleId),
    ParentModule(ModuleId),
    Anchor(ModuleId),
    Module(ModuleId),
    Declaration(NodeId),
    GenericParameter(GenericParameterId),
    Prelude(CatalogType),
    Catalog(CatalogType),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ResolutionDiagnosticKind {
    MissingGrammarConfig,
    MultipleGrammarConfigs,
    InvalidGrammarConfig,
    InvalidConfiguration,
    DuplicateDeclaration,
    InvalidVisibility,
    UnsupportedDeclaration,
    Unresolved,
    Inaccessible,
    AboveAnchor,
    OutsideAnchor,
    ExternalPath,
    LeadingExternalPath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolutionDiagnostic {
    pub(crate) kind: ResolutionDiagnosticKind,
    pub(crate) path: PathBuf,
    pub(crate) range: Option<Range<u32>>,
    pub(crate) message: String,
}

#[derive(Clone, Debug)]
struct Binding {
    id: BindingId,
    module: ModuleId,
    name: String,
    kind: BindingKind,
    visibility: Visibility,
    source: SourceId,
    range: Range<u32>,
}

#[derive(Clone, Copy, Debug)]
enum BindingKind {
    Module(ModuleId),
    Node(NodeId),
    Unsupported,
    ExternalCrate,
}

#[derive(Clone, Debug)]
struct PendingNode {
    resolved: ResolvedNode,
    generic_parameters: BTreeMap<String, GenericParameterId>,
    fields: Vec<Type>,
}

struct Resolver<'cfg> {
    cfg: &'cfg CargoCfgSnapshot,
    build_inputs: BuildInputs,
    tree: DiscoveredModuleTree,
    items: BTreeMap<ModuleId, Vec<Item>>,
    bindings: Vec<Binding>,
    binding_by_name: BTreeMap<(ModuleId, String), BindingId>,
    nodes: Vec<PendingNode>,
    generic_parameters: Vec<ResolvedGenericParameter>,
    uses: Vec<SemanticUse>,
    resolutions: Vec<SemanticResolution>,
    traces: Vec<ResolutionTrace>,
    diagnostics: Vec<ResolutionDiagnostic>,
}

pub(crate) fn resolve_local_semantics(input: ResolutionInput<'_>) -> ResolutionReport {
    let discovery_inputs = input.discovery_inputs.clone();
    let items = input
        .tree
        .inclusions
        .iter()
        .map(|module| (module.id, module.items.clone()))
        .collect();
    let (config, mut diagnostics) = find_grammar_config(&input.tree, &items, input.cfg);
    let Some((anchor, facade)) = config else {
        return ResolutionReport {
            discovery_inputs,
            build_inputs: None,
            diagnostics,
            resolved: None,
        };
    };
    let build_inputs = BuildInputs {
        discovery: input.discovery_inputs,
        module_anchor: anchor.clone(),
        facade: facade.clone(),
    };
    let mut resolver = Resolver {
        cfg: input.cfg,
        build_inputs,
        tree: input.tree,
        items,
        bindings: Vec::new(),
        binding_by_name: BTreeMap::new(),
        nodes: Vec::new(),
        generic_parameters: Vec::new(),
        uses: Vec::new(),
        resolutions: Vec::new(),
        traces: Vec::new(),
        diagnostics: Vec::new(),
    };
    resolver.diagnostics.append(&mut diagnostics);
    resolver.build_symbols();
    resolver.validate_visibility_scopes();
    resolver.resolve_fields();
    resolver.finish()
}

impl Resolver<'_> {
    fn finish(self) -> ResolutionReport {
        let resolved = self.diagnostics.is_empty().then(|| ResolvedSourceGrammar {
            build_inputs: self.build_inputs.clone(),
            tree: self.tree,
            nodes: self.nodes.into_iter().map(|node| node.resolved).collect(),
            generic_parameters: self.generic_parameters,
            uses: self.uses,
            resolutions: self.resolutions,
            traces: self.traces,
        });
        ResolutionReport {
            discovery_inputs: self.build_inputs.discovery.clone(),
            build_inputs: Some(self.build_inputs),
            diagnostics: self.diagnostics,
            resolved,
        }
    }

    fn build_symbols(&mut self) {
        let inclusions = self.tree.inclusions.clone();
        for inclusion in inclusions.iter().filter(|module| module.parent.is_some()) {
            let parent = inclusion.parent.expect("filtered module has parent");
            let name = inclusion
                .logical_path
                .last()
                .expect("non-root module has a name")
                .clone();
            self.insert_binding(Binding {
                id: BindingId::next(self.bindings.len()),
                module: parent,
                name,
                kind: BindingKind::Module(inclusion.id),
                visibility: inclusion.visibility.clone(),
                source: inclusion.declaration_source,
                range: inclusion.declaration_range.clone(),
            });
        }

        for inclusion in inclusions {
            let Some(items) = self.items.get(&inclusion.id).cloned() else {
                continue;
            };
            for item in items {
                let configured = match configured_metas(item_attrs(&item), self.cfg) {
                    Ok(Some(metas)) => metas,
                    Ok(None) => continue,
                    Err((range, message)) => {
                        self.push_site_diagnostic(
                            ResolutionDiagnosticKind::InvalidConfiguration,
                            inclusion.source,
                            Some(range),
                            message,
                        );
                        continue;
                    }
                };
                let Some((ident, visibility, generics, may_be_node)) = item_declaration(&item)
                else {
                    if let Item::ExternCrate(item) = item {
                        let name = item
                            .rename
                            .as_ref()
                            .map_or_else(|| item.ident.unraw(), |(_, ident)| ident.unraw())
                            .to_string();
                        self.insert_binding(Binding {
                            id: BindingId::next(self.bindings.len()),
                            module: inclusion.id,
                            name,
                            kind: BindingKind::ExternalCrate,
                            visibility: Visibility::Inherited,
                            source: inclusion.source,
                            range: span_range(item.ident.span()),
                        });
                    }
                    continue;
                };
                let name = ident.unraw().to_string();
                let is_node =
                    may_be_node && has_node_derive(&configured, &self.build_inputs.facade.ident);
                let kind = if is_node {
                    let id = NodeId::next(self.nodes.len());
                    let generic_parameters = generics
                        .params
                        .iter()
                        .filter_map(|parameter| {
                            let syn::GenericParam::Type(parameter) = parameter else {
                                return None;
                            };
                            let parameter_id =
                                GenericParameterId::next(self.generic_parameters.len());
                            let ident = parameter.ident.unraw().to_string();
                            self.generic_parameters.push(ResolvedGenericParameter {
                                id: parameter_id,
                                node: id,
                                ident: ident.clone(),
                            });
                            Some((ident, parameter_id))
                        })
                        .collect();
                    let mut key = self.build_inputs.module_anchor.segments.clone();
                    key.extend(inclusion.logical_path.iter().cloned());
                    key.push(name.clone());
                    self.nodes.push(PendingNode {
                        resolved: ResolvedNode {
                            id,
                            module: inclusion.id,
                            source: inclusion.source,
                            ident: name.clone(),
                            key: NodeKey { segments: key },
                            range: span_range(ident.span()),
                        },
                        generic_parameters,
                        fields: configured_node_fields(
                            &item,
                            self.cfg,
                            &mut self.diagnostics,
                            &self.tree,
                            inclusion.source,
                        ),
                    });
                    BindingKind::Node(id)
                } else {
                    BindingKind::Unsupported
                };
                self.insert_binding(Binding {
                    id: BindingId::next(self.bindings.len()),
                    module: inclusion.id,
                    name,
                    kind,
                    visibility,
                    source: inclusion.source,
                    range: span_range(ident.span()),
                });
            }
        }
    }

    fn insert_binding(&mut self, binding: Binding) {
        let key = (binding.module, binding.name.clone());
        if self.binding_by_name.contains_key(&key) {
            self.push_site_diagnostic(
                ResolutionDiagnosticKind::DuplicateDeclaration,
                binding.source,
                Some(binding.range),
                format!(
                    "duplicate active type-namespace declaration `{}`",
                    binding.name
                ),
            );
            return;
        }
        self.binding_by_name.insert(key, binding.id);
        self.bindings.push(binding);
    }

    fn validate_visibility_scopes(&mut self) {
        let bindings = self.bindings.clone();
        for binding in bindings {
            if self.visibility_scope(&binding).is_none() {
                self.push_site_diagnostic(
                    ResolutionDiagnosticKind::InvalidVisibility,
                    binding.source,
                    Some(binding.range),
                    format!(
                        "visibility for `{}` is not an enclosing selected module",
                        binding.name
                    ),
                );
            }
        }
    }

    fn resolve_fields(&mut self) {
        let nodes = self.nodes.clone();
        for node in nodes {
            for ty in &node.fields {
                self.resolve_type(
                    ty,
                    node.resolved.module,
                    node.resolved.id,
                    node.resolved.source,
                );
            }
        }
    }

    fn resolve_type(&mut self, ty: &Type, module: ModuleId, node: NodeId, source: SourceId) {
        match ty {
            Type::Path(type_path) => {
                let path = &type_path.path;
                let authored = path_text(path);
                let use_id = SemanticUseId::next(self.uses.len());
                let range = span_range(path.span());
                self.uses.push(SemanticUse {
                    id: use_id,
                    module,
                    containing_node: node,
                    source,
                    range: range.clone(),
                    authored: authored.clone(),
                });
                let outcome = if type_path.qself.is_some() {
                    Err((
                        ResolutionDiagnosticKind::ExternalPath,
                        "qualified semantic paths are outside the local resolver".to_owned(),
                    ))
                } else {
                    self.resolve_path(path, module, node)
                };
                match outcome {
                    Ok((target, steps)) => {
                        let trace = ResolutionTraceId::next(self.traces.len());
                        self.traces.push(ResolutionTrace { id: trace, steps });
                        self.resolutions.push(SemanticResolution {
                            use_site: use_id,
                            authored,
                            target,
                            trace,
                        });
                    }
                    Err((kind, message)) => {
                        self.push_site_diagnostic(kind, source, Some(range), message)
                    }
                }
                for segment in &path.segments {
                    if let PathArguments::AngleBracketed(arguments) = &segment.arguments {
                        for argument in &arguments.args {
                            if let GenericArgument::Type(argument) = argument {
                                self.resolve_type(argument, module, node, source);
                            }
                        }
                    }
                }
            }
            Type::Group(group) => self.resolve_type(&group.elem, module, node, source),
            Type::Paren(paren) => self.resolve_type(&paren.elem, module, node, source),
            Type::Reference(reference) => self.resolve_type(&reference.elem, module, node, source),
            Type::Tuple(tuple) => {
                for element in &tuple.elems {
                    self.resolve_type(element, module, node, source);
                }
            }
            _ => {}
        }
    }

    fn resolve_path(
        &self,
        path: &Path,
        use_module: ModuleId,
        containing_node: NodeId,
    ) -> Result<(SemanticTarget, Vec<ResolutionTraceStep>), (ResolutionDiagnosticKind, String)>
    {
        if path.leading_colon.is_some() {
            return Err((
                ResolutionDiagnosticKind::LeadingExternalPath,
                "leading `::` semantic paths use the unsupported extern prelude".to_owned(),
            ));
        }
        let names = path
            .segments
            .iter()
            .map(|segment| segment.ident.unraw().to_string())
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Err((
                ResolutionDiagnosticKind::Unresolved,
                "empty semantic path".to_owned(),
            ));
        }
        let root = self.root_module();
        if names[0] == "Self" {
            if names.len() != 1 {
                return Err((
                    ResolutionDiagnosticKind::UnsupportedDeclaration,
                    "associated paths through `Self` are unsupported".to_owned(),
                ));
            }
            return Ok((
                SemanticTarget::Node(containing_node),
                vec![
                    ResolutionTraceStep::Start(use_module),
                    ResolutionTraceStep::ContainingNode(containing_node),
                ],
            ));
        }
        if !matches!(names[0].as_str(), "self" | "super" | "crate")
            && let Some(parameter) = self.generic_parameter(containing_node, &names[0])
        {
            if names.len() != 1 {
                return Err((
                    ResolutionDiagnosticKind::UnsupportedDeclaration,
                    "associated paths through a generic parameter are unsupported".to_owned(),
                ));
            }
            return Ok((
                SemanticTarget::GenericParameter(parameter),
                vec![
                    ResolutionTraceStep::Start(use_module),
                    ResolutionTraceStep::GenericParameter(parameter),
                ],
            ));
        }

        let mut steps = vec![ResolutionTraceStep::Start(use_module)];
        let (module, consumed, explicit_local) = match names[0].as_str() {
            "self" => {
                steps.push(ResolutionTraceStep::CurrentModule(use_module));
                (use_module, 1, true)
            }
            "super" => {
                let mut module = use_module;
                let mut consumed = 0;
                while names.get(consumed).is_some_and(|name| name == "super") {
                    let Some(parent) = self.module(module).parent else {
                        return Err((
                            ResolutionDiagnosticKind::AboveAnchor,
                            "`super` traverses above the selected root".to_owned(),
                        ));
                    };
                    module = parent;
                    steps.push(ResolutionTraceStep::ParentModule(module));
                    consumed += 1;
                }
                (module, consumed, true)
            }
            "crate" => {
                let anchor_end = 1 + self.build_inputs.module_anchor.segments.len();
                if names.len() < anchor_end
                    || names[1..anchor_end] != self.build_inputs.module_anchor.segments
                {
                    return Err((
                        ResolutionDiagnosticKind::OutsideAnchor,
                        "crate-rooted semantic path does not enter the selected anchor".to_owned(),
                    ));
                }
                steps.push(ResolutionTraceStep::Anchor(root));
                (root, anchor_end, true)
            }
            _ => (use_module, 0, false),
        };
        if consumed == names.len() {
            return Err((
                ResolutionDiagnosticKind::UnsupportedDeclaration,
                "a module cannot be used as a semantic type".to_owned(),
            ));
        }

        if !explicit_local {
            if let Some(binding) = self.lookup(module, &names[0]) {
                return self.resolve_from_binding(binding, &names, 1, use_module, steps);
            }
            if let Some(catalog) = catalog_path(&names, &self.build_inputs.facade.ident) {
                let step = if names.len() == 1 {
                    ResolutionTraceStep::Prelude(catalog)
                } else {
                    ResolutionTraceStep::Catalog(catalog)
                };
                steps.push(step);
                return Ok((SemanticTarget::Catalog(catalog), steps));
            }
            return Err(if names.len() > 1 {
                (
                    ResolutionDiagnosticKind::ExternalPath,
                    format!(
                        "external semantic path `{}` is outside the closed catalog",
                        path_text(path)
                    ),
                )
            } else {
                (
                    ResolutionDiagnosticKind::Unresolved,
                    format!("unresolved semantic name `{}`", names[0]),
                )
            });
        }

        let Some(binding) = self.lookup(module, &names[consumed]) else {
            return Err((
                ResolutionDiagnosticKind::Unresolved,
                format!("unresolved selected-tree path `{}`", path_text(path)),
            ));
        };
        self.resolve_from_binding(binding, &names, consumed + 1, use_module, steps)
    }

    fn resolve_from_binding<'resolver>(
        &'resolver self,
        mut binding: &'resolver Binding,
        names: &[String],
        mut next: usize,
        use_module: ModuleId,
        mut steps: Vec<ResolutionTraceStep>,
    ) -> Result<(SemanticTarget, Vec<ResolutionTraceStep>), (ResolutionDiagnosticKind, String)>
    {
        loop {
            if !self.binding_visible(binding, use_module) {
                return Err((
                    ResolutionDiagnosticKind::Inaccessible,
                    format!("`{}` is not visible from this logical module", binding.name),
                ));
            }
            if next == names.len() {
                return match binding.kind {
                    BindingKind::Node(node) => {
                        steps.push(ResolutionTraceStep::Declaration(node));
                        Ok((SemanticTarget::Node(node), steps))
                    }
                    BindingKind::ExternalCrate => Err((
                        ResolutionDiagnosticKind::ExternalPath,
                        "extern-crate-dependent semantic lookup is unsupported".to_owned(),
                    )),
                    BindingKind::Module(_) | BindingKind::Unsupported => Err((
                        ResolutionDiagnosticKind::UnsupportedDeclaration,
                        format!(
                            "`{}` is not a supported grammar-semantic declaration",
                            binding.name
                        ),
                    )),
                };
            }
            let BindingKind::Module(child) = binding.kind else {
                return Err((
                    match binding.kind {
                        BindingKind::ExternalCrate => ResolutionDiagnosticKind::ExternalPath,
                        _ => ResolutionDiagnosticKind::UnsupportedDeclaration,
                    },
                    format!("`{}` is not a selected-tree module", binding.name),
                ));
            };
            steps.push(ResolutionTraceStep::Module(child));
            let Some(next_binding) = self.lookup(child, &names[next]) else {
                return Err((
                    ResolutionDiagnosticKind::Unresolved,
                    format!(
                        "module `{}` has no semantic name `{}`",
                        binding.name, names[next]
                    ),
                ));
            };
            binding = next_binding;
            next += 1;
        }
    }

    fn lookup(&self, module: ModuleId, name: &str) -> Option<&Binding> {
        self.binding_by_name
            .get(&(module, name.to_owned()))
            .map(|id| &self.bindings[id.index()])
    }

    fn generic_parameter(&self, node: NodeId, name: &str) -> Option<GenericParameterId> {
        self.nodes[node.index()]
            .generic_parameters
            .get(name)
            .copied()
    }

    fn binding_visible(&self, binding: &Binding, use_module: ModuleId) -> bool {
        self.visibility_scope(binding)
            .is_some_and(|scope| self.is_descendant(use_module, scope))
    }

    fn visibility_scope(&self, binding: &Binding) -> Option<ModuleId> {
        match &binding.visibility {
            Visibility::Public(_) => Some(self.root_module()),
            Visibility::Inherited => Some(binding.module),
            Visibility::Restricted(restricted) => {
                let names = restricted
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.unraw().to_string())
                    .collect::<Vec<_>>();
                if names == ["crate"] {
                    return Some(self.root_module());
                }
                if names == ["self"] {
                    return Some(binding.module);
                }
                if names.iter().all(|name| name == "super") {
                    let logical = &self.module(binding.module).logical_path;
                    let full_depth = self.build_inputs.module_anchor.segments.len() + logical.len();
                    if names.len() > full_depth {
                        return None;
                    }
                    let target_depth = full_depth - names.len();
                    if target_depth <= self.build_inputs.module_anchor.segments.len() {
                        return Some(self.root_module());
                    }
                    let logical_depth =
                        target_depth - self.build_inputs.module_anchor.segments.len();
                    return self.module_by_logical_path(&logical[..logical_depth]);
                }
                let scope = if names.first().is_some_and(|name| name == "crate") {
                    let restriction = &names[1..];
                    let mut declaring = self.build_inputs.module_anchor.segments.clone();
                    declaring.extend(self.module(binding.module).logical_path.iter().cloned());
                    if !declaring.starts_with(restriction) {
                        return None;
                    }
                    if restriction.len() <= self.build_inputs.module_anchor.segments.len() {
                        self.root_module()
                    } else {
                        self.module_by_logical_path(
                            &restriction[self.build_inputs.module_anchor.segments.len()..],
                        )?
                    }
                } else {
                    return None;
                };
                self.is_descendant(binding.module, scope).then_some(scope)
            }
        }
    }

    fn root_module(&self) -> ModuleId {
        self.tree
            .inclusions
            .iter()
            .find(|module| module.parent.is_none())
            .expect("discovered tree has a root")
            .id
    }

    fn module(&self, id: ModuleId) -> &ModuleInclusion {
        &self.tree.inclusions[id.index()]
    }

    fn module_by_logical_path(&self, path: &[String]) -> Option<ModuleId> {
        self.tree
            .inclusions
            .iter()
            .find(|module| module.logical_path == path)
            .map(|module| module.id)
    }

    fn is_descendant(&self, mut module: ModuleId, ancestor: ModuleId) -> bool {
        loop {
            if module == ancestor {
                return true;
            }
            let Some(parent) = self.module(module).parent else {
                return false;
            };
            module = parent;
        }
    }

    fn push_site_diagnostic(
        &mut self,
        kind: ResolutionDiagnosticKind,
        source: SourceId,
        range: Option<Range<u32>>,
        message: String,
    ) {
        self.diagnostics.push(ResolutionDiagnostic {
            kind,
            path: self.tree.files[source.index()].path.clone(),
            range,
            message,
        });
    }
}

fn find_grammar_config(
    tree: &DiscoveredModuleTree,
    items: &BTreeMap<ModuleId, Vec<Item>>,
    cfg: &CargoCfgSnapshot,
) -> (
    Option<(SelectedRootAnchor, FacadeMapping)>,
    Vec<ResolutionDiagnostic>,
) {
    let mut diagnostics = Vec::new();
    let mut configurations = Vec::new();
    for inclusion in tree
        .inclusions
        .iter()
        .filter(|module| module.parent.is_none())
    {
        for item in &items[&inclusion.id] {
            let Item::Macro(item) = item else { continue };
            if item.mac.path.leading_colon.is_some()
                || item.mac.path.segments.len() != 2
                || !item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "grammar")
            {
                continue;
            }
            let configured = match configured_metas(&item.attrs, cfg) {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err((range, message)) => {
                    diagnostics.push(ResolutionDiagnostic {
                        kind: ResolutionDiagnosticKind::InvalidConfiguration,
                        path: tree.files[inclusion.source.index()].path.clone(),
                        range: Some(range),
                        message,
                    });
                    false
                }
            };
            if !configured {
                continue;
            }
            let macro_root = item.mac.path.segments[0].ident.unraw().to_string();
            match syn::parse2::<GrammarConfigSyntax>(item.mac.tokens.clone()) {
                Ok(syntax) if syntax.facade.unraw() == macro_root => {
                    configurations.push((inclusion, item, syntax));
                }
                Ok(syntax) if syntax.facade_explicit || macro_root == "recursa" => {
                    diagnostics.push(ResolutionDiagnostic {
                        kind: ResolutionDiagnosticKind::InvalidGrammarConfig,
                        path: tree.files[inclusion.source.index()].path.clone(),
                        range: Some(span_range(item.mac.path.span())),
                        message: "grammar invocation must use the configured facade mapping"
                            .to_owned(),
                    });
                }
                Ok(_) => {}
                Err(_) if macro_root != "recursa" => {}
                Err(error) => diagnostics.push(ResolutionDiagnostic {
                    kind: ResolutionDiagnosticKind::InvalidGrammarConfig,
                    path: tree.files[inclusion.source.index()].path.clone(),
                    range: Some(span_range(error.span())),
                    message: error.to_string(),
                }),
            }
        }
    }
    if configurations.is_empty() {
        if !diagnostics.is_empty() {
            return (None, diagnostics);
        }
        diagnostics.push(ResolutionDiagnostic {
            kind: ResolutionDiagnosticKind::MissingGrammarConfig,
            path: tree.files[tree.inclusions[0].source.index()].path.clone(),
            range: None,
            message: "expected one active grammar configuration".to_owned(),
        });
        return (None, diagnostics);
    }
    if configurations.len() > 1 {
        for (inclusion, item, _) in configurations.iter().skip(1) {
            diagnostics.push(ResolutionDiagnostic {
                kind: ResolutionDiagnosticKind::MultipleGrammarConfigs,
                path: tree.files[inclusion.source.index()].path.clone(),
                range: Some(span_range(item.mac.path.span())),
                message: "multiple active grammar configurations".to_owned(),
            });
        }
        return (None, diagnostics);
    }
    let (inclusion, _, syntax) = &configurations[0];
    let anchor = match selected_anchor(&syntax.module) {
        Some(anchor) => anchor,
        None => {
            diagnostics.push(ResolutionDiagnostic {
                kind: ResolutionDiagnosticKind::InvalidGrammarConfig,
                path: tree.files[inclusion.source.index()].path.clone(),
                range: Some(span_range(syntax.module.span())),
                message: "module anchor must be `crate` or a path below `crate`".to_owned(),
            });
            return (None, diagnostics);
        }
    };
    let facade = syntax.facade.unraw().to_string();
    (Some((anchor, FacadeMapping { ident: facade })), diagnostics)
}

fn selected_anchor(path: &Path) -> Option<SelectedRootAnchor> {
    if path.leading_colon.is_some()
        || path.segments.first()?.ident != "crate"
        || path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return None;
    }
    Some(SelectedRootAnchor {
        segments: path
            .segments
            .iter()
            .skip(1)
            .map(|segment| segment.ident.unraw().to_string())
            .collect(),
    })
}

fn configured_metas(
    attributes: &[Attribute],
    cfg: &CargoCfgSnapshot,
) -> Result<Option<Vec<Meta>>, (Range<u32>, String)> {
    configure_attributes(attributes, cfg)
        .map(|configured| configured.map(|configured| configured.metas))
        .map_err(|error| (error.range, error.message))
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
        Item::Verbatim(_) => &[],
        _ => &[],
    }
}

fn item_declaration(item: &Item) -> Option<(syn::Ident, Visibility, syn::Generics, bool)> {
    match item {
        Item::Struct(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            true,
        )),
        Item::Enum(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            true,
        )),
        Item::Union(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        Item::Trait(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        Item::TraitAlias(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        Item::Type(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        _ => None,
    }
}

fn configured_node_fields(
    item: &Item,
    cfg: &CargoCfgSnapshot,
    diagnostics: &mut Vec<ResolutionDiagnostic>,
    tree: &DiscoveredModuleTree,
    source: SourceId,
) -> Vec<Type> {
    let fields = match item {
        Item::Struct(item) => item.fields.iter().collect::<Vec<_>>(),
        Item::Enum(item) => item
            .variants
            .iter()
            .filter_map(|variant| match configured_metas(&variant.attrs, cfg) {
                Ok(Some(_)) => Some(variant.fields.iter().collect::<Vec<_>>()),
                Ok(None) => None,
                Err((range, message)) => {
                    diagnostics.push(ResolutionDiagnostic {
                        kind: ResolutionDiagnosticKind::InvalidConfiguration,
                        path: tree.files[source.index()].path.clone(),
                        range: Some(range),
                        message,
                    });
                    None
                }
            })
            .flatten()
            .collect(),
        _ => Vec::new(),
    };
    fields
        .into_iter()
        .filter_map(|field| match configured_metas(&field.attrs, cfg) {
            Ok(Some(_)) => Some(field.ty.clone()),
            Ok(None) => None,
            Err((range, message)) => {
                diagnostics.push(ResolutionDiagnostic {
                    kind: ResolutionDiagnosticKind::InvalidConfiguration,
                    path: tree.files[source.index()].path.clone(),
                    range: Some(range),
                    message,
                });
                None
            }
        })
        .collect()
}

fn has_node_derive(metas: &[Meta], facade: &str) -> bool {
    metas
        .iter()
        .filter_map(|meta| {
            let Meta::List(list) = meta else { return None };
            list.path.is_ident("derive").then_some(list)
        })
        .any(|list| {
            Punctuated::<Path, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .is_ok_and(|paths| {
                    paths.iter().any(|path| {
                        path.leading_colon.is_none()
                            && path.segments.len() == 2
                            && path.segments[0].ident.unraw() == facade
                            && path.segments[1].ident == "Node"
                    })
                })
        })
}

fn catalog_path(names: &[String], facade: &str) -> Option<CatalogType> {
    match names {
        [name] if name == "Option" => Some(CatalogType::Option),
        [name] if name == "Box" => Some(CatalogType::Box),
        [name] if name == "Vec" => Some(CatalogType::Vec),
        [root, module, name]
            if (root == "core" || root == "std") && module == "option" && name == "Option" =>
        {
            Some(CatalogType::Option)
        }
        [root, module, name]
            if (root == "alloc" || root == "std") && module == "boxed" && name == "Box" =>
        {
            Some(CatalogType::Box)
        }
        [root, module, name]
            if (root == "alloc" || root == "std") && module == "vec" && name == "Vec" =>
        {
            Some(CatalogType::Vec)
        }
        [root, name] if root == facade && name == "Vec1" => Some(CatalogType::Vec1),
        _ => None,
    }
}

fn path_text(path: &Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn span_range(span: Span) -> Range<u32> {
    let range = span.byte_range();
    range.start.try_into().unwrap_or(u32::MAX)..range.end.try_into().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::{
        CatalogType, ResolutionDiagnosticKind, ResolutionInput, SemanticTarget,
        resolve_local_semantics,
    };
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::RustEdition;
    use crate::source_syntax::CargoCfgSnapshot;

    fn write(package: &Path, relative: &str, source: &str) {
        let path = package.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file parent")).unwrap();
        fs::write(path, source).unwrap();
    }

    fn resolve_with_cfg(
        package: &TempDir,
        source: &str,
        cfg: &CargoCfgSnapshot,
    ) -> super::ResolutionReport {
        write(package.path(), "grammar/root.rs", source);
        let discovery = discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new("grammar/root.rs"),
            edition: RustEdition::Rust2024,
            cfg,
        });
        assert!(discovery.diagnostics.is_empty());
        resolve_local_semantics(ResolutionInput {
            discovery_inputs: discovery.discovery_inputs,
            tree: discovery.tree.unwrap(),
            cfg,
        })
    }

    fn resolve(package: &TempDir, source: &str) -> super::ResolutionReport {
        resolve_with_cfg(package, source, &CargoCfgSnapshot::default())
    }

    #[test]
    fn resolves_anchored_local_paths_self_and_the_closed_catalog() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
my_recursa::grammar! {
    module = crate::sql,
    facade = my_recursa,
}

#[derive(my_recursa::Node)]
pub(super) struct Root {
    local: Root,
    relative: branch::Branch,
    explicit_self: self::branch::Branch,
    anchored: crate::sql::branch::Branch,
    optional: Option<branch::Branch>,
    boxed: std::boxed::Box<branch::Branch>,
    repeated: alloc::vec::Vec<branch::Branch>,
    nonempty: my_recursa::Vec1<branch::Branch>,
}

mod branch {
    #[derive(my_recursa::Node)]
    pub struct Branch {
        this: Self,
        root: super::Root,
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty());
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.build_inputs.module_anchor.segments, ["sql"]);
        assert_eq!(resolved.build_inputs.facade.ident, "my_recursa");
        assert_eq!(
            resolved
                .nodes
                .iter()
                .map(|node| node.key.segments.join("::"))
                .collect::<Vec<_>>(),
            ["sql::Root", "sql::branch::Branch"],
        );
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "Option"
                && resolution.target == SemanticTarget::Catalog(CatalogType::Option)
        }));
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "my_recursa::Vec1"
                && resolution.target == SemanticTarget::Catalog(CatalogType::Vec1)
        }));
        assert!(
            resolved
                .resolutions
                .iter()
                .all(|resolution| { !resolved.traces[resolution.trace.index()].steps.is_empty() })
        );
    }

    #[test]
    fn repeated_physical_sources_have_distinct_logical_node_keys() {
        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/shared.rs",
            r#"
#[derive(recursa::Node)]
pub struct Shared { helper: Helper }
#[derive(recursa::Node)]
struct Helper { again: Self }
"#,
        );
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
#[path = "shared.rs"] mod left;
#[path = "shared.rs"] mod right;
#[derive(recursa::Node)]
struct Root { left: left::Shared, right: right::Shared }
"#,
        );

        assert!(report.diagnostics.is_empty());
        let resolved = report.resolved.unwrap();
        let shared = resolved
            .nodes
            .iter()
            .filter(|node| node.ident == "Shared")
            .collect::<Vec<_>>();
        assert_eq!(shared.len(), 2);
        assert_ne!(shared[0].id, shared[1].id);
        assert_ne!(shared[0].module, shared[1].module);
        assert_ne!(shared[0].key, shared[1].key);
        assert_eq!(
            resolved.tree.files[shared[0].source.index()].path,
            resolved.tree.files[shared[1].source.index()].path,
        );
    }

    #[test]
    fn resolves_restricted_visibility_raw_names_and_multiple_parent_steps() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }

#[derive(recursa::Node)]
struct Root { keyword: r#type::r#match }

mod r#type {
    #[derive(recursa::Node)]
    pub struct r#match { root: super::Root }
}

mod left {
    #[derive(recursa::Node)]
    pub(super) struct Shared { root: super::Root }

    pub mod nested {
        #[derive(recursa::Node)]
        pub(in crate::sql::left) struct Restricted {
            root: super::super::Root,
        }
    }

    #[derive(recursa::Node)]
    struct Left { restricted: self::nested::Restricted }
}

mod right {
    #[derive(recursa::Node)]
    struct Right { shared: super::left::Shared }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        assert!(resolved.nodes.iter().any(|node| {
            node.ident == "match" && node.key.segments.join("::") == "sql::type::match"
        }));
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "super::super::Root"
                && matches!(resolution.target, SemanticTarget::Node(_))
        }));
    }

    #[test]
    fn applies_configuration_to_nodes_variants_and_fields_before_resolution() {
        let package = TempDir::new().unwrap();
        let cfg = CargoCfgSnapshot::default().with_flag("unix");
        let report = resolve_with_cfg(
            &package,
            r#"
recursa::grammar! { module = crate }
#[cfg(windows)]
struct Option;

#[cfg_attr(unix, derive(recursa::Node))]
enum Root {
    Present(Option<Leaf>),
    #[cfg(windows)]
    Inactive(dep::Missing),
}

#[derive(recursa::Node)]
struct Leaf {
    #[cfg(unix)]
    this: Self,
    #[cfg(windows)]
    missing: dep::Missing,
}
"#,
            &cfg,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.nodes.len(), 2);
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "Option"
                && resolution.target == SemanticTarget::Catalog(CatalogType::Option)
        }));
        assert!(
            !resolved
                .uses
                .iter()
                .any(|use_site| use_site.authored.contains("dep"))
        );
    }

    #[test]
    fn enforces_visibility_from_sibling_logical_modules() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod left {
    #[derive(recursa::Node)]
    struct Private { this: Self }
    pub mod nested {
        #[derive(recursa::Node)]
        pub(in crate::sql::left) struct Restricted { this: Self }
    }
}
mod right {
    #[derive(recursa::Node)]
    struct Right {
        private: super::left::Private,
        restricted: super::left::nested::Restricted,
    }
}
"#,
        );

        assert!(report.resolved.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                ResolutionDiagnosticKind::Inaccessible,
                ResolutionDiagnosticKind::Inaccessible,
            ],
        );
    }

    #[test]
    fn rejects_shadowed_catalog_visibility_and_external_paths_before_lowering() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
struct Vec;
mod left {
    #[derive(recursa::Node)]
    struct Private { this: Self }
}
#[derive(recursa::Node)]
struct Root {
    shadowed: Vec,
    inaccessible: left::Private,
    above: super::Root,
    outside: crate::outside::Thing,
    external: dep::Thing,
    absolute: ::std::vec::Vec,
}
"#,
        );

        assert!(report.resolved.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                ResolutionDiagnosticKind::UnsupportedDeclaration,
                ResolutionDiagnosticKind::Inaccessible,
                ResolutionDiagnosticKind::AboveAnchor,
                ResolutionDiagnosticKind::OutsideAnchor,
                ResolutionDiagnosticKind::ExternalPath,
                ResolutionDiagnosticKind::LeadingExternalPath,
            ],
        );
        let build_inputs = report
            .build_inputs
            .expect("valid anchor facts survive semantic failure");
        assert_eq!(build_inputs.module_anchor.segments, ["sql"]);
        assert_eq!(build_inputs.facade.ident, "recursa");
        assert_eq!(
            report.discovery_inputs.selected_root,
            PathBuf::from("grammar/root.rs")
        );
    }

    #[test]
    fn generic_parameters_shadow_catalog_names_and_keep_typed_traces() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)]
struct Root<Vec> { value: Vec }
"#,
        );

        assert!(report.diagnostics.is_empty());
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.generic_parameters.len(), 1);
        assert_eq!(
            resolved.resolutions[0].target,
            SemanticTarget::GenericParameter(resolved.generic_parameters[0].id),
        );
    }

    #[test]
    fn collects_nested_failures_and_ignores_descendant_grammar_named_macros() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod child {
    recursa::grammar! { module = crate::not_the_root }
    foo::grammar! { arbitrary tokens }
}
#[derive(recursa::Node)]
struct Root { nested: Unknown<AlsoMissing> }
"#,
        );

        assert!(report.resolved.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                ResolutionDiagnosticKind::Unresolved,
                ResolutionDiagnosticKind::Unresolved,
            ],
        );
        assert_eq!(report.build_inputs.unwrap().module_anchor.segments, ["sql"],);
    }
}
