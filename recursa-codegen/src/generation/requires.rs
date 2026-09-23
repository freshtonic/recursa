//! Declared build-configuration requirements: the plan and the generated scan.
//!
//! A `#[config(since = ...)]` gate both removes its element from a build that excludes it and
//! records that the element needs that configuration. The removal half runs during resolution,
//! beside the `cfg` it replaces. This module owns the recording half: it reads the surviving
//! gates out of the resolved grammar, computes one static ceiling per Node type, and renders
//! the `Requires` implementation that walks a value in authored source order.
//!
//! The walk uses the same occurrence arithmetic as the generated views: element `e` of the
//! active shape is `cursor.child(e)`, a stored semantic value is one further level down, and
//! repetition item `i` is `cursor.child(e).child(i)`. See `node_views.rs`.

use super::*;
use syn::Meta;

use crate::resolution::ResolvedNodeBody;
use crate::source_syntax::ConfigGate;

/// One requirement recorded by an authored gate.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(super) struct RequirementRecord {
    /// Ordinal of the lowest configuration that admits the construct.
    configuration: u16,
    /// Whether the requirement belongs to the absence of the value.
    absent: bool,
    /// Opaque code carried to the consumer.
    code: Option<String>,
    /// Opaque message carried to the consumer.
    message: Option<String>,
    /// Opaque hint carried to the consumer.
    hint: Option<String>,
    /// Citation of the source that justifies the gate.
    citation: Option<String>,
}

/// Identifies one field by its owner and its construction position.
type FieldKey = (NodeId, Option<ResolvedVariantId>, usize);

/// Every declared requirement of one grammar, plus the ceilings that make a scan cheap.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(super) struct RequirementPlan {
    /// Declared configuration names in ascending order.
    pub(super) configurations: Vec<String>,
    /// Ordinal of the configuration this build states.
    pub(super) active: u16,
    /// Requirement of one whole Node type.
    nodes: BTreeMap<NodeId, RequirementRecord>,
    /// Requirement of one enum variant.
    variants: BTreeMap<ResolvedVariantId, RequirementRecord>,
    /// Requirement of one field, keyed by owner and construction position.
    fields: BTreeMap<FieldKey, RequirementRecord>,
    /// Highest requirement reachable through each Node type.
    ceilings: BTreeMap<NodeId, u16>,
}

impl RequirementPlan {
    /// Reports whether the grammar declares a configuration list at all.
    pub(super) fn is_declared(&self) -> bool {
        !self.configurations.is_empty()
    }

    /// Returns the highest requirement reachable through one Node type.
    fn ceiling(&self, node: NodeId) -> u16 {
        self.ceilings.get(&node).copied().unwrap_or(0)
    }

    /// Returns the highest requirement anywhere in the grammar.
    fn maximum(&self) -> u16 {
        self.nodes
            .values()
            .chain(self.variants.values())
            .chain(self.fields.values())
            .map(|record| record.configuration)
            .max()
            .unwrap_or(0)
    }
}

/// Reads one `config(...)` meta list into a requirement, when it records one.
///
/// Only a `since` bound records. A `before` bound removes its element from every later build,
/// so inside a build where the element survives it demands nothing.
fn record_from_metas(metas: &[Meta], names: &[String]) -> Option<RequirementRecord> {
    metas
        .iter()
        .filter(|meta| meta.path().is_ident("config"))
        .find_map(|meta| {
            let gate = ConfigGate::from_meta(meta).ok()?;
            let since = gate.since.as_ref()?;
            let configuration = names.iter().position(|name| name == since)?;
            Some(RequirementRecord {
                configuration: u16::try_from(configuration).ok()?,
                absent: gate.on_absent,
                code: gate.code,
                message: gate.message,
                hint: gate.hint,
                citation: gate.citation,
            })
        })
}

/// Builds the complete requirement plan from the resolved grammar and the planned nodes.
pub(super) fn plan_requirements(
    resolved: &ResolvedSourceGrammar,
    nodes: &[NodeParsePlan],
) -> RequirementPlan {
    let names = resolved
        .build_inputs
        .configurations
        .iter()
        .map(|entry| entry.name.unraw().to_string())
        .collect::<Vec<_>>();
    let mut plan = RequirementPlan {
        configurations: names.clone(),
        active: u16::try_from(resolved.build_inputs.configuration_active).unwrap_or(0),
        ..RequirementPlan::default()
    };
    if names.is_empty() {
        return plan;
    }
    for node in &resolved.nodes {
        if let Some(record) = record_from_metas(&node.metas, &names) {
            plan.nodes.insert(node.id, record);
        }
        match &node.body {
            ResolvedNodeBody::Struct { .. } => {
                for (index, field) in node.fields.iter().enumerate() {
                    if let Some(record) = record_from_metas(&field.metas, &names) {
                        plan.fields.insert((node.id, None, index), record);
                    }
                }
            }
            ResolvedNodeBody::Enum { variants } => {
                for variant in variants {
                    if let Some(record) = record_from_metas(&variant.metas, &names) {
                        plan.variants.insert(variant.id, record);
                    }
                    for (index, id) in variant.fields.iter().enumerate() {
                        let Some(field) = node.fields.iter().find(|field| field.id == *id) else {
                            continue;
                        };
                        if let Some(record) = record_from_metas(&field.metas, &names) {
                            plan.fields
                                .insert((node.id, Some(variant.id), index), record);
                        }
                    }
                }
            }
        }
    }
    compute_ceilings(&mut plan, nodes);
    plan
}

/// Computes, by fixpoint over the reachable type graph, the highest requirement each Node type
/// can hold.
///
/// A scan enters a subtree only when its ceiling can still change the answer, so a grammar
/// region with no gate costs one comparison. A generic Node takes the grammar's maximum,
/// because its type arguments are not known here; that loses pruning, never soundness.
fn compute_ceilings(plan: &mut RequirementPlan, nodes: &[NodeParsePlan]) {
    let maximum = plan.maximum();
    let mut own = BTreeMap::new();
    let mut edges: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for node in nodes {
        let mut floor = plan
            .nodes
            .get(&node.node)
            .map_or(0, |record| record.configuration);
        let mut generic = node.generics.type_params().next().is_some();
        let mut targets = BTreeSet::new();
        let shapes: Vec<(Option<ResolvedVariantId>, &[ElementParsePlan])> = match &node.body {
            NodeBodyPlan::Struct { elements, .. } => vec![(None, elements.as_slice())],
            NodeBodyPlan::Enum { variants } => variants
                .iter()
                .map(|variant| (Some(variant.resolved_variant), variant.elements.as_slice()))
                .collect(),
        };
        for (variant, elements) in shapes {
            if let Some(variant) = variant
                && let Some(record) = plan.variants.get(&variant)
            {
                floor = floor.max(record.configuration);
            }
            for element in elements {
                let ElementParseKind::Semantic { field, projection } = &element.kind else {
                    continue;
                };
                if let Some(record) = plan.fields.get(&(node.node, variant, field.index)) {
                    floor = floor.max(record.configuration);
                }
                if let SemanticParseProjection::Direct {
                    terminal,
                    terminal_node,
                    ..
                } = projection.as_ref()
                {
                    if let Some(target) = terminal_node {
                        targets.insert(*target);
                    } else if type_depends_on_node_type_parameters(terminal, node) {
                        generic = true;
                    }
                }
            }
        }
        own.insert(node.node, if generic { maximum } else { floor });
        edges.insert(node.node, targets);
    }
    let mut ceilings = own;
    loop {
        let mut changed = false;
        for (node, targets) in &edges {
            let reached = targets
                .iter()
                .map(|target| ceilings.get(target).copied().unwrap_or(0))
                .max()
                .unwrap_or(0);
            let current = ceilings.get(node).copied().unwrap_or(0);
            if reached > current {
                ceilings.insert(*node, reached);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    plan.ceilings = ceilings;
}

/// Collects the static notes of one Node while its scan body is rendered.
struct NoteBuilder<'plan> {
    /// Facade path used by every generated reference.
    facade: &'plan Ident,
    /// Identifier of the per-Node note table.
    table: Ident,
    /// Declared configuration names, used to inline each note's name.
    names: &'plan [String],
    /// Authored Node name, the prefix of every site.
    owner: String,
    /// Note constructors in emission order.
    notes: Vec<proc_macro2::TokenStream>,
}

impl NoteBuilder<'_> {
    /// Emits one static note and the call that records it at `cursor`.
    fn record(
        &mut self,
        record: &RequirementRecord,
        suffix: &str,
        cursor: &proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        let facade = self.facade;
        let index = self.notes.len();
        let ordinal = record.configuration;
        let name = self
            .names
            .get(usize::from(ordinal))
            .cloned()
            .unwrap_or_default();
        let site = format!("{}{suffix}", self.owner);
        let absent = record.absent;
        let optional = |value: &Option<String>| {
            value.as_ref().map_or_else(
                || quote!(::core::option::Option::None),
                |text| quote!(::core::option::Option::Some(#text)),
            )
        };
        let code = optional(&record.code);
        let message = optional(&record.message);
        let hint = optional(&record.hint);
        let citation = optional(&record.citation);
        self.notes.push(quote! {
            #facade::RequirementNote::new(
                #ordinal, #name, #site, #absent, #code, #message, #hint, #citation,
            )
        });
        let table = &self.table;
        quote!(scan.record(&#table[#index], #cursor);)
    }
}

/// Renders one Node's configured `Requires` implementation.
pub(super) fn render_node_requires(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let requirements = &plan.requirements;
    let node_ident = &node.ident;
    let mut builder = NoteBuilder {
        facade,
        table: format_ident!("__RECURSA_REQUIREMENTS_{node_index}"),
        names: &requirements.configurations,
        owner: node_ident.unraw().to_string(),
        notes: Vec::new(),
    };
    let cursor = quote!(cursor);
    let own = requirements
        .nodes
        .get(&node.node)
        .map(|record| builder.record(record, "", &cursor));
    let body = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let mut steps = Vec::new();
            for (index, element) in elements.iter().enumerate() {
                steps.push(render_element(
                    plan,
                    node,
                    &mut builder,
                    None,
                    index,
                    element,
                    None,
                ));
            }
            quote!(#own #(#steps)*)
        }
        NodeBodyPlan::Enum { variants } => {
            let mut arms = Vec::new();
            for variant in variants {
                let (pattern, bindings) = variant_bound_pattern(node_ident, variant, "requires");
                let fields = semantic_fields(&variant.elements);
                let site = format!("::{}", variant.ident);
                let variant_note = requirements
                    .variants
                    .get(&variant.resolved_variant)
                    .map(|record| builder.record(record, &site, &cursor));
                let mut steps = Vec::new();
                for (index, element) in variant.elements.iter().enumerate() {
                    let binding = match &element.kind {
                        ElementParseKind::Semantic { field, .. } => fields
                            .iter()
                            .position(|candidate| candidate.index == field.index)
                            .map(|position| bindings[position].clone()),
                        ElementParseKind::Fixed { .. }
                        | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
                    };
                    steps.push(render_element(
                        plan,
                        node,
                        &mut builder,
                        Some(variant.resolved_variant),
                        index,
                        element,
                        binding.as_ref(),
                    ));
                }
                arms.push(quote!(#pattern => { #variant_note #(#steps)* }));
            }
            quote! {
                #own
                match self { #(#arms)* }
            }
        }
    };
    let (impl_generics, type_generics, where_clause) = requires_impl_parts(facade, node);
    let ceiling = requirements.ceiling(node.node);
    let table = &builder.table;
    let count = builder.notes.len();
    let notes = &builder.notes;
    let declaration = (count != 0).then(|| {
        quote! {
            static #table: [#facade::RequirementNote; #count] = [#(#notes),*];
        }
    });
    quote! {
        #declaration
        impl #impl_generics #facade::Requires for #node_ident #type_generics #where_clause {
            const REQUIREMENT_CEILING: #facade::Configuration =
                #facade::Configuration::from_ordinal(#ceiling);

            fn scan_requirements(
                &self,
                cursor: #facade::__private::ViewCursor<'_>,
                scan: &mut #facade::RequirementScan,
            ) {
                if scan.finished() {
                    return;
                }
                let _ = &cursor;
                #body
            }
        }
    }
}

/// Renders the scan of one planned element: its own gate, then its children.
fn render_element(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    builder: &mut NoteBuilder<'_>,
    variant: Option<ResolvedVariantId>,
    index: usize,
    element: &ElementParsePlan,
    binding: Option<&Ident>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let ElementParseKind::Semantic { field, projection } = &element.kind else {
        return quote!();
    };
    let site = format!(".{}", element.metadata_name);
    let record = plan
        .requirements
        .fields
        .get(&(node.node, variant, field.index))
        .cloned();
    let bound = binding.is_some();
    let value = binding.map_or_else(
        || field_access(quote!(self), field.ident.as_ref(), field.index),
        |binding| quote!(#binding),
    );
    let slot = quote!(cursor.child(#index));
    match projection.as_ref() {
        SemanticParseProjection::FixedPresence { .. } => {
            let Some(record) = record else {
                return quote!();
            };
            let held = if bound { quote!(*#value) } else { quote!(#value) };
            let call = builder.record(&record, &site, &slot);
            if record.absent {
                quote!(if !#held { #call })
            } else {
                quote!(if #held { #call })
            }
        }
        SemanticParseProjection::Direct {
            terminal,
            terminal_node,
            wrappers,
            attachment,
            ..
        } => {
            let semantic_child = attachment.as_ref().map(|attachment| attachment.before.len());
            let descend = |cursor: proc_macro2::TokenStream| {
                semantic_child
                    .map_or_else(|| cursor.clone(), |child| quote!((#cursor).child(#child)))
            };
            let recursive =
                terminal_node.is_some() || type_depends_on_node_type_parameters(terminal, node);
            let terminal_type = &**terminal;
            let guarded = |value: proc_macro2::TokenStream, cursor: proc_macro2::TokenStream| {
                if recursive {
                    quote! {
                        if !scan.skips(
                            <#terminal_type as #facade::Requires>::REQUIREMENT_CEILING
                        ) {
                            #facade::Requires::scan_requirements(#value, #cursor, scan);
                        }
                    }
                } else {
                    quote!()
                }
            };
            if let Some(repeat) = wrappers.iter().position(|wrapper| {
                matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
            }) {
                let outer = &wrappers[..repeat];
                let inner = &wrappers[repeat + 1..];
                let optional_outer = outer
                    .iter()
                    .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));
                let collection = if optional_outer {
                    render_optional_view_reference(value, outer, bound)
                } else {
                    render_terminal_reference(value, outer, bound)
                };
                let item = render_terminal_reference(quote!(__recursa_item), inner, true);
                let walk = guarded(item, descend(quote!((#slot).child(__recursa_index))));
                // An optional repetition is empty when it is absent as well, so the two
                // shapes share one emptiness test.
                let items = if optional_outer {
                    quote!(#collection.map_or(&[][..], |values| values.as_slice()))
                } else {
                    quote!((#collection).as_slice())
                };
                let note = record.as_ref().map(|record| {
                    let call = builder.record(record, &site, &slot);
                    if record.absent {
                        quote!(if __recursa_items.is_empty() { #call })
                    } else {
                        quote!(if !__recursa_items.is_empty() { #call })
                    }
                });
                quote! {
                    {
                        let __recursa_items = #items;
                        #note
                        for (__recursa_index, __recursa_item)
                            in __recursa_items.iter().enumerate()
                        {
                            #walk
                        }
                    }
                }
            } else if wrappers
                .iter()
                .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
            {
                let reference = render_optional_view_reference(value, wrappers, bound);
                let walk = guarded(quote!(__recursa_value), descend(quote!((#slot).child(0usize))));
                let present = record
                    .as_ref()
                    .filter(|record| !record.absent)
                    .map(|record| builder.record(record, &site, &slot));
                let missing = record
                    .as_ref()
                    .filter(|record| record.absent)
                    .map(|record| builder.record(record, &site, &quote!(cursor)));
                quote! {
                    match #reference {
                        Some(__recursa_value) => { #present #walk }
                        None => { #missing }
                    }
                }
            } else {
                let reference = render_terminal_reference(value, wrappers, bound);
                let note = record
                    .as_ref()
                    .map(|record| builder.record(record, &site, &slot));
                let walk = guarded(reference, descend(quote!((#slot).child(0usize))));
                quote!(#note #walk)
            }
        }
    }
}

/// Prepares `Requires` generics without restating bounds on concrete field types.
fn requires_impl_parts(
    facade: &Ident,
    node: &NodeParsePlan,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let mut generics = node.generics.clone();
    for field_type in semantic_field_types(facade, node)
        .into_iter()
        .filter(|field_type| type_depends_on_node_type_parameters(field_type, node))
    {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #facade::Requires));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    (
        quote!(#impl_generics),
        quote!(#type_generics),
        quote!(#where_clause),
    )
}

/// Renders the grammar-root configuration enum.
pub(super) fn render_configurations(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let requirements = &plan.requirements;
    if !requirements.is_declared() {
        return quote!();
    }
    let names = &requirements.configurations;
    let variants = names
        .iter()
        .map(|name| configuration_variant(name))
        .collect::<Vec<_>>();
    let documented = names.iter().zip(&variants).map(|(name, variant)| {
        let documentation = format!("The `{name}` configuration.");
        quote!(#[doc = #documentation] #variant)
    });
    let ordinals = (0..names.len())
        .map(|index| u16::try_from(index).unwrap_or(u16::MAX))
        .collect::<Vec<_>>();
    let from_arms = variants
        .iter()
        .zip(&ordinals)
        .map(|(variant, ordinal)| quote!(#ordinal => ::core::option::Option::Some(Self::#variant),));
    let ordinal_arms = variants
        .iter()
        .zip(&ordinals)
        .map(|(variant, ordinal)| quote!(Self::#variant => #ordinal,));
    let name_arms = variants
        .iter()
        .zip(names.iter())
        .map(|(variant, name)| quote!(Self::#variant => #name,));
    let active = usize::from(requirements.active);
    let count = names.len();
    quote! {
        /// One named configuration of this grammar, ordered as the grammar declares them.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum Configuration {
            #(#documented,)*
        }

        impl Configuration {
            /// Every declared configuration, in ascending order.
            pub const ALL: [Self; #count] = [#(Self::#variants),*];

            /// The configuration that this build states.
            pub const BUILD: Self = Self::ALL[#active];

            /// The declared name of this configuration.
            pub const fn name(self) -> &'static str {
                match self { #(#name_arms)* }
            }

            /// The grammar-independent ordinal of this configuration.
            pub const fn id(self) -> #facade::Configuration {
                #facade::Configuration::from_ordinal(match self { #(#ordinal_arms)* })
            }

            /// The configuration at one grammar-independent ordinal.
            pub const fn from_id(id: #facade::Configuration) -> ::core::option::Option<Self> {
                match id.ordinal() {
                    #(#from_arms)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    }
}

/// Converts an authored configuration name into its generated variant identifier.
fn configuration_variant(name: &str) -> Ident {
    let mut variant = String::with_capacity(name.len());
    let mut capitalize = true;
    for character in name.chars() {
        if character == '_' || character == '-' {
            capitalize = true;
            continue;
        }
        if capitalize {
            variant.extend(character.to_uppercase());
            capitalize = false;
        } else {
            variant.push(character);
        }
    }
    if variant
        .chars()
        .next()
        .is_none_or(|character| !character.is_alphabetic())
    {
        variant.insert(0, 'C');
    }
    format_ident!("{variant}")
}
