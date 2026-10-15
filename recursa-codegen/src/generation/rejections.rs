//! Declared rejections: the plan and the generated list checks.
//!
//! A `#[reject(...)]` declaration on an `ast_node!` enum states which items a repetition of that
//! enum may not hold together: forbidden unordered pairs of labels, a `once` set of labels, or a
//! text rule that reads one variant's folded identifier text. A `#[classify(...)]` declaration
//! lets an item of one variant count as another label by the folded text of its identifier
//! field. Every repetition whose element type is that enum runs the rules as the list is built,
//! and the first item a rule refuses ends the parse. `docs/declared-rejections.md` is the design.
//!
//! This module owns the recording half: it reads the surviving declarations out of the resolved
//! grammar, assigns each label its tag and each text its index, resolves the field path of every
//! classification, and renders the static rule table and the key every list backend computes for
//! one item. Source-model lowering owns the authoring diagnostics, so everything here may assume
//! a readable, resolved declaration, except the shape of a classification's field path, which is
//! only known once the Nodes are planned.
//!
//! The check runs at the reduce of a list step, which is the one place that holds both the items
//! the list has passed and the extent of the item just added. It therefore changes no LR table: a
//! list step is a rule the lowering already emits.

use super::*;
use syn::Meta;

use crate::resolution::ResolvedNodeBody;
use crate::source_syntax::{ClassifySyntax, RejectCheckSyntax, RejectRuleSyntax};

/// The test one recorded rule makes, with every name resolved to a tag or a text index.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum RecordCheck {
    /// Forbidden unordered pairs of class tags, in written order.
    Pairs(Vec<(u32, u32)>),
    /// A set of class tags of which a list holds at most one item.
    Once(Vec<u32>),
    /// A text rule over the items of one variant.
    Text {
        /// `true` for `not_in`, `false` for `one_of`.
        negated: bool,
        /// Tag of the classified variant.
        variant: u32,
        /// Indexes of the rule's texts in the enum's text table.
        texts: Vec<u32>,
    },
}

/// One rejection rule of one enum, with its names resolved.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct RejectionRecord {
    /// The test the rule makes.
    check: RecordCheck,
    /// Opaque code carried to the consumer.
    code: Option<String>,
    /// Opaque message carried to the consumer.
    message: String,
    /// Opaque hint carried to the consumer.
    hint: Option<String>,
    /// Citation of the source that rejects the item.
    citation: String,
}

/// Where a classified identifier field keeps its text.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ClassifiedTerminal {
    /// The field is a content token.
    Content,
    /// The field is a token-wrapper enum: every variant holds exactly one content token.
    Wrapper {
        /// The wrapper Node.
        node: NodeId,
        /// The wrapper's variants, in plan order.
        variants: Vec<String>,
    },
}

/// One classification of one variant, with its field path resolved against the planned Nodes.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct Classification {
    /// Tag of the classified variant.
    variant: u32,
    /// The struct Node the variant holds.
    payload: NodeId,
    /// Name of the identifier field of the payload.
    field: String,
    /// Whether the field is held behind `boxed!`.
    field_boxed: bool,
    /// Where the field keeps its text.
    terminal: ClassifiedTerminal,
    /// The consumer's fold function, as written.
    fold: String,
    /// For each entry of the enum's text table: the class an item with that folded text counts as.
    classes: Vec<u32>,
}

/// Every declared rule and classification of one enum.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(super) struct NodeRejections {
    /// Rules in authored order.
    rules: Vec<RejectionRecord>,
    /// Classifications, one per classified variant.
    classifications: Vec<Classification>,
    /// The enum's text table: every text a classification or a text rule names, first
    /// appearance first.
    texts: Vec<String>,
}

impl NodeRejections {
    /// Reports whether a rule of this enum can refuse an item on its own, which only a text rule
    /// does. Such an enum needs the check at the first step of a list too.
    pub(super) fn refuses_single_items(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| matches!(rule.check, RecordCheck::Text { .. }))
    }
}

/// Every declared rejection rule of one grammar, keyed by the enum that declares it.
///
/// The rules of one enum keep their authored attribute order, because the first rule that refuses
/// an item decides the message. A variant's tag is its position in the Node plan's own variant
/// list, and a text label is numbered after the variants in order of first appearance, so the
/// plan, the generated key functions and the rule table always agree. The map is ordered, so the
/// plan's `Debug` rendering is stable and the generation identity it feeds is reproducible.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(super) struct RejectionPlan {
    /// Rules of one Node type.
    nodes: BTreeMap<NodeId, NodeRejections>,
}

impl RejectionPlan {
    /// Reports whether the grammar declares no rejection rule at all.
    ///
    /// A grammar that declares none pays nothing: no check is rendered and no parse of it keeps
    /// the LR extent stack.
    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the rules and classifications of one Node type.
    pub(super) fn rules(&self, node: NodeId) -> Option<&NodeRejections> {
        self.nodes.get(&node)
    }

    /// Reports whether any enum classifies a variant, so a flat parse reads the text pool.
    pub(super) fn reads_texts(&self) -> bool {
        self.nodes
            .values()
            .any(|node| !node.classifications.is_empty())
    }

    /// Reports whether a flat list check reads the pool of `node`: the enum with the rules, the
    /// payload of a classified variant, or the token wrapper of a classified field.
    pub(super) fn reads_pool(&self, node: NodeId) -> bool {
        self.nodes.contains_key(&node)
            || self.nodes.values().any(|rules| {
                rules.classifications.iter().any(|classification| {
                    classification.payload == node
                        || matches!(
                            &classification.terminal,
                            ClassifiedTerminal::Wrapper { node: wrapper, .. } if *wrapper == node
                        )
                })
            })
    }
}

/// Returns the semantic fields of one element sequence, with their projections.
fn semantic_fields(elements: &[ElementParsePlan]) -> Vec<(&FieldPlan, &SemanticParseProjection)> {
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { field, projection } => Some((field, projection.as_ref())),
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .collect()
}

/// Returns the single Node a tuple variant holds, when every wrapper around it is `boxed!`.
fn variant_payload(variant: &VariantParsePlan) -> Option<NodeId> {
    if variant.style != ResolvedFieldStyle::Unnamed {
        return None;
    }
    let fields = semantic_fields(&variant.elements);
    let [(_, projection)] = fields.as_slice() else {
        return None;
    };
    match projection {
        SemanticParseProjection::Direct {
            terminal_node: Some(node),
            wrappers,
            ..
        } if wrappers
            .iter()
            .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed)) =>
        {
            Some(*node)
        }
        _ => None,
    }
}

/// Resolves where one classification's identifier field keeps its text.
///
/// The path is the classified variant's single struct payload, then the named field of that
/// struct, which is a content token or a token-wrapper enum. Anything else is an authoring error
/// generation reports, because the shape is only known once the Nodes are planned.
fn resolve_classification(
    nodes: &[NodeParsePlan],
    enum_node: &NodeParsePlan,
    variant: &VariantParsePlan,
    field: &str,
) -> Result<(NodeId, bool, ClassifiedTerminal), String> {
    let shape = || {
        format!(
            "`#[classify({}.{field}, ...)]` on `{}`: `{}` must hold exactly one non-generic \
             struct Node, whose field `{field}` is a content token or a non-generic enum whose \
             every variant holds one content token",
            variant.ident, enum_node.ident, variant.ident,
        )
    };
    let payload = variant_payload(variant).ok_or_else(shape)?;
    let payload_plan = nodes
        .iter()
        .find(|candidate| candidate.node == payload)
        .ok_or_else(shape)?;
    let NodeBodyPlan::Struct { elements, .. } = &payload_plan.body else {
        return Err(shape());
    };
    if payload_plan.parse_applications.len() != 1 {
        return Err(shape());
    }
    let (_, projection) = semantic_fields(elements)
        .into_iter()
        .find(|(candidate, _)| {
            candidate
                .ident
                .as_ref()
                .is_some_and(|ident| ident.unraw() == field)
        })
        .ok_or_else(shape)?;
    let SemanticParseProjection::Direct {
        terminal_node,
        explorer_terminal,
        wrappers,
        ..
    } = projection
    else {
        return Err(shape());
    };
    if !wrappers
        .iter()
        .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed))
    {
        return Err(shape());
    }
    let field_boxed = !wrappers.is_empty();
    if matches!(explorer_terminal, ExplorerTerminalPlan::Content(_)) {
        return Ok((payload, field_boxed, ClassifiedTerminal::Content));
    }
    let wrapper = terminal_node.ok_or_else(shape)?;
    let wrapper_plan = nodes
        .iter()
        .find(|candidate| candidate.node == wrapper)
        .ok_or_else(shape)?;
    let NodeBodyPlan::Enum { variants } = &wrapper_plan.body else {
        return Err(shape());
    };
    if wrapper_plan.parse_applications.len() != 1 {
        return Err(shape());
    }
    let mut names = Vec::new();
    for wrapped in variants {
        let fields = semantic_fields(&wrapped.elements);
        let [(_, projection)] = fields.as_slice() else {
            return Err(shape());
        };
        match projection {
            SemanticParseProjection::Direct {
                explorer_terminal: ExplorerTerminalPlan::Content(_),
                wrappers,
                ..
            } if wrappers.is_empty() && wrapped.style == ResolvedFieldStyle::Unnamed => {
                names.push(wrapped.ident.unraw().to_string());
            }
            _ => return Err(shape()),
        }
    }
    Ok((
        payload,
        field_boxed,
        ClassifiedTerminal::Wrapper {
            node: wrapper,
            variants: names,
        },
    ))
}

/// Adds `text` to the text table when it is new, and returns its index.
fn intern_text(texts: &mut Vec<String>, text: &str) -> u32 {
    let index = texts
        .iter()
        .position(|candidate| candidate == text)
        .unwrap_or_else(|| {
            texts.push(text.to_owned());
            texts.len() - 1
        });
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// Builds the rules and classifications of one enum.
///
/// # Long-function contract
///
/// Rationale: labels, the text table, the rules and the classifications all number one another,
/// so they are built in one place where the numbering is visibly the same for all four.
/// Invariants: a variant's tag is its plan position; a text label follows the variants in order
/// of first appearance; the text table lists classification texts before rule texts.
/// Phases: reads the declarations, numbers labels, records rules, resolves classifications.
fn node_rejections(
    nodes: &[NodeParsePlan],
    enum_node: &NodeParsePlan,
    metas: &[Meta],
    variants: &[VariantParsePlan],
) -> Result<NodeRejections, String> {
    let names = variants
        .iter()
        .map(|variant| variant.ident.unraw().to_string())
        .collect::<Vec<_>>();
    let classifications = metas
        .iter()
        .filter(|meta| meta.path().is_ident("classify"))
        .filter_map(|meta| ClassifySyntax::from_meta(meta).ok())
        .collect::<Vec<_>>();
    let rules = metas
        .iter()
        .filter(|meta| meta.path().is_ident("reject"))
        .filter_map(|meta| RejectRuleSyntax::from_meta(meta).ok())
        .collect::<Vec<_>>();

    // Labels: the variants, then every text label in order of first appearance.
    let mut labels = names.clone();
    for classify in &classifications {
        for text in &classify.texts {
            if !labels.contains(&text.label) {
                labels.push(text.label.clone());
            }
        }
    }
    let label = |name: &str| {
        labels
            .iter()
            .position(|candidate| candidate == name)
            .and_then(|index| u32::try_from(index).ok())
    };
    let variant = |name: &str| {
        names
            .iter()
            .position(|candidate| candidate == name)
            .and_then(|index| u32::try_from(index).ok())
    };

    // The text table: classification texts first, then rule texts.
    let mut texts: Vec<String> = Vec::new();
    for classify in &classifications {
        for text in &classify.texts {
            intern_text(&mut texts, &text.text);
        }
    }
    let mut records = Vec::new();
    for rule in rules {
        // A name that no label carries is dropped here, because source-model lowering already
        // reported it and generation never runs on a rejected model.
        let check = match &rule.check {
            RejectCheckSyntax::Pairs(pairs) => {
                let pairs = pairs
                    .iter()
                    .filter_map(|pair| Some((label(&pair.left)?, label(&pair.right)?)))
                    .collect::<Vec<_>>();
                if pairs.is_empty() {
                    continue;
                }
                RecordCheck::Pairs(pairs)
            }
            RejectCheckSyntax::Once { labels: once, .. } => {
                let tags = once
                    .iter()
                    .filter_map(|name| label(name))
                    .collect::<Vec<_>>();
                if tags.is_empty() {
                    continue;
                }
                RecordCheck::Once(tags)
            }
            RejectCheckSyntax::Text {
                negated,
                variant: name,
                texts: rule_texts,
                ..
            } => {
                let Some(tag) = variant(name) else {
                    continue;
                };
                RecordCheck::Text {
                    negated: *negated,
                    variant: tag,
                    texts: rule_texts
                        .iter()
                        .map(|text| intern_text(&mut texts, text))
                        .collect(),
                }
            }
        };
        records.push(RejectionRecord {
            check,
            code: rule.code,
            message: rule.message,
            hint: rule.hint,
            citation: rule.citation,
        });
    }

    let mut resolved = Vec::new();
    for classify in classifications {
        let Some(tag) = variant(&classify.variant) else {
            continue;
        };
        let (payload, field_boxed, terminal) =
            resolve_classification(nodes, enum_node, &variants[tag as usize], &classify.field)?;
        let classes = texts
            .iter()
            .map(|text| {
                classify
                    .texts
                    .iter()
                    .find(|mapping| mapping.text == *text)
                    .and_then(|mapping| label(&mapping.label))
                    .unwrap_or(tag)
            })
            .collect();
        let fold = &classify.fold;
        resolved.push(Classification {
            variant: tag,
            payload,
            field: classify.field,
            field_boxed,
            terminal,
            fold: quote!(#fold).to_string(),
            classes,
        });
    }
    Ok(NodeRejections {
        rules: records,
        classifications: resolved,
        texts,
    })
}

/// Builds the complete rejection plan from the resolved grammar and the planned nodes.
pub(super) fn plan_rejections(
    resolved: &ResolvedSourceGrammar,
    nodes: &[NodeParsePlan],
) -> Result<RejectionPlan, String> {
    let mut plan = RejectionPlan::default();
    for node in &resolved.nodes {
        if !node
            .metas
            .iter()
            .any(|meta| meta.path().is_ident("reject") || meta.path().is_ident("classify"))
        {
            continue;
        }
        let ResolvedNodeBody::Enum { .. } = &node.body else {
            continue;
        };
        let Some(planned) = nodes.iter().find(|planned| planned.node == node.id) else {
            continue;
        };
        let NodeBodyPlan::Enum { variants } = &planned.body else {
            continue;
        };
        let rejections = node_rejections(nodes, planned, &node.metas, variants)?;
        if !rejections.rules.is_empty() {
            plan.nodes.insert(node.id, rejections);
        }
    }
    Ok(plan)
}

/// Renders one Node's static rule table under `table`.
pub(super) fn render_rule_table(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    table: &Ident,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let Some(rejections) = plan.rejections.rules(node.node) else {
        return quote!();
    };
    let site = node.ident.unraw().to_string();
    let optional = |value: &Option<String>| {
        value.as_ref().map_or_else(
            || quote!(::core::option::Option::None),
            |text| quote!(::core::option::Option::Some(#text)),
        )
    };
    let mut statics = Vec::new();
    let mut entries = Vec::new();
    for (index, rule) in rejections.rules.iter().enumerate() {
        let operands = format_ident!("{table}_OPERANDS_{index}");
        let code = optional(&rule.code);
        let message = &rule.message;
        let hint = optional(&rule.hint);
        let citation = &rule.citation;
        let note = quote!(#facade::RejectionNote::new(#site, #code, #message, #hint, #citation));
        match &rule.check {
            RecordCheck::Pairs(pairs) => {
                let count = pairs.len();
                let pairs = pairs
                    .iter()
                    .map(|(left, right)| quote!(#facade::RejectionPair::new(#left, #right)));
                statics.push(quote! {
                    static #operands: [#facade::RejectionPair; #count] = [#(#pairs),*];
                });
                entries.push(quote!(#facade::RejectionRule::new(&#operands, #note)));
            }
            RecordCheck::Once(classes) => {
                let count = classes.len();
                statics.push(quote!(static #operands: [u32; #count] = [#(#classes),*];));
                entries.push(quote!(#facade::RejectionRule::once(&#operands, #note)));
            }
            RecordCheck::Text {
                negated,
                variant,
                texts,
            } => {
                let count = texts.len();
                statics.push(quote!(static #operands: [u32; #count] = [#(#texts),*];));
                let constructor = if *negated {
                    quote!(text_not_in)
                } else {
                    quote!(text_in)
                };
                entries.push(quote! {
                    #facade::RejectionRule::#constructor(#variant, &#operands, #note)
                });
            }
        }
    }
    let count = entries.len();
    quote! {
        #(#statics)*
        static #table: [#facade::RejectionRule; #count] = [#(#entries),*];
    }
}

/// Renders the key of one classified item from its raw identifier text.
///
/// `raw` is an `Option<&str>`: the source text of the identifier, or `None` when a flat result
/// lacks it. The consumer's fold function turns it into the folded text, and the item counts as
/// the label its folded text maps to. An item whose text no declaration names keeps its own
/// variant as its class and has no text index.
fn render_classified_key(
    facade: &Ident,
    classification: &Classification,
    texts: &[String],
    raw: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String> {
    let variant = classification.variant;
    let fold: syn::Path = syn::parse_str(&classification.fold)
        .map_err(|error| format!("a classify fold path does not render back: {error}"))?;
    let arms =
        texts
            .iter()
            .zip(&classification.classes)
            .enumerate()
            .map(|(index, (text, class))| {
                let index = u32::try_from(index).unwrap_or(u32::MAX);
                quote!(#text => #facade::RejectionKey::new(#variant, #class, #index),)
            });
    Ok(quote! {
        match (#raw).and_then(|__recursa_raw| #fold(__recursa_raw)) {
            ::core::option::Option::Some(__recursa_folded) => {
                match ::core::convert::AsRef::<str>::as_ref(&__recursa_folded) {
                    #(#arms)*
                    _ => #facade::RejectionKey::plain(#variant),
                }
            }
            ::core::option::Option::None => #facade::RejectionKey::plain(#variant),
        }
    })
}

/// How a nested key names a Node it reaches through a classification's field path.
pub(super) enum NestedNaming<'a> {
    /// The value or arena form: the caller names a Node by its application path.
    Value(&'a dyn Fn(NodeId) -> Result<proc_macro2::TokenStream, String>),
    /// The `Arbitrary` seed mirror: a Node is named by its seed path.
    Seed,
}

/// Renders the source text of a classified field of a nested payload bound to
/// `__recursa_payload`, as an `Option<&str>`.
fn render_nested_text(
    plan: &EmissionPlan,
    classification: &Classification,
    naming: &NestedNaming<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let field = format_ident!("{}", classification.field);
    let access = if classification.field_boxed {
        quote!(&*__recursa_payload.#field)
    } else {
        quote!(&__recursa_payload.#field)
    };
    match &classification.terminal {
        ClassifiedTerminal::Content => Ok(quote! {
            ::core::option::Option::Some((#access).text())
        }),
        ClassifiedTerminal::Wrapper { node, variants } => {
            let path = match naming {
                NestedNaming::Value(name) => name(*node)?,
                NestedNaming::Seed => {
                    let wrapper = plan
                        .nodes
                        .iter()
                        .find(|candidate| candidate.node == *node)
                        .ok_or_else(|| {
                            "a classified wrapper has no frozen parse plan".to_owned()
                        })?;
                    let [application] = wrapper.parse_applications.as_slice() else {
                        return Err(format!(
                            "`{}` classifies through the generic wrapper `{}`",
                            classification.field, wrapper.ident
                        ));
                    };
                    let ty = super::arbitrary_seed::seed_type_path(plan, application.parse_index);
                    quote!(#ty)
                }
            };
            let arms = variants.iter().map(|variant| {
                let variant = format_ident!("{variant}");
                quote!(#path::#variant(__recursa_token) => __recursa_token.text(),)
            });
            Ok(quote! {
                ::core::option::Option::Some(match #access { #(#arms)* })
            })
        }
    }
}

/// Renders the match arms that compute one nested value's key.
///
/// A variant with no classification is matched by name, so the payload never takes part: its arm
/// is a wildcard pattern over the variant's own shape. A classified variant binds its payload and
/// reads the identifier text through the classification's field path.
fn render_nested_key_arms(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    type_path: &proc_macro2::TokenStream,
    naming: &NestedNaming<'_>,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let facade = &plan.facade;
    let NodeBodyPlan::Enum { variants } = &node.body else {
        return Ok(Vec::new());
    };
    let rejections = plan.rejections.rules(node.node);
    variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let tag = u32::try_from(index).unwrap_or(u32::MAX);
            let variant_ident = &variant.ident;
            let classification = rejections.and_then(|rejections| {
                rejections
                    .classifications
                    .iter()
                    .find(|classification| classification.variant == tag)
                    .map(|classification| (rejections, classification))
            });
            if let Some((rejections, classification)) = classification {
                let raw = render_nested_text(plan, classification, naming)?;
                let key = render_classified_key(facade, classification, &rejections.texts, raw)?;
                return Ok(quote!(#type_path::#variant_ident(__recursa_payload) => #key,));
            }
            let pattern = match variant.style {
                ResolvedFieldStyle::Named => quote!(#type_path::#variant_ident { .. }),
                ResolvedFieldStyle::Unnamed => quote!(#type_path::#variant_ident(..)),
                ResolvedFieldStyle::Unit => quote!(#type_path::#variant_ident),
            };
            Ok(quote!(#pattern => #facade::RejectionKey::plain(#tag),))
        })
        .collect()
}

/// Renders the function that reports one nested value's rejection key.
///
/// The variant tag is the variant's position in the Node plan's variant list, which is the
/// position the rule table used.
pub(super) fn render_key_function(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    ident: &Ident,
    value_type: &proc_macro2::TokenStream,
    type_path: &proc_macro2::TokenStream,
    naming: &NestedNaming<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let arms = render_nested_key_arms(plan, node, type_path, naming)?;
    let documentation = format!(
        " Reports the rejection key of one `{}` for its declared rejection rules.",
        node.ident,
    );
    Ok(quote! {
        #[doc = #documentation]
        fn #ident(value: &#value_type) -> #facade::RejectionKey {
            match value { #(#arms)* }
        }
    })
}

/// Reads one pooled flat value during a list check: the handle expression and the Node it names.
pub(super) type FlatPoolRead<'a> =
    &'a dyn Fn(NodeId, proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, String>;

/// Renders the flat key computation of one list check as a closure over `builder`.
///
/// A flat item is a handle into a pool, so a classified item reads its payload out of that
/// payload's pool, its identifier out of the wrapper's pool, and the identifier's text out of
/// the text pool. Nothing is materialized.
pub(super) fn render_flat_key_closure(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    flat_ty: &Type,
    flat_path: &proc_macro2::TokenStream,
    read_pool: FlatPoolRead<'_>,
    flat_path_of: &dyn Fn(NodeId) -> Result<proc_macro2::TokenStream, String>,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let NodeBodyPlan::Enum { variants } = &node.body else {
        return Ok(quote!());
    };
    let rejections = plan.rejections.rules(node.node);
    let mut arms = Vec::new();
    for (index, variant) in variants.iter().enumerate() {
        let tag = u32::try_from(index).unwrap_or(u32::MAX);
        let variant_ident = &variant.ident;
        let classification = rejections.and_then(|rejections| {
            rejections
                .classifications
                .iter()
                .find(|classification| classification.variant == tag)
                .map(|classification| (rejections, classification))
        });
        if let Some((rejections, classification)) = classification {
            let payload = read_pool(classification.payload, quote!(*__recursa_payload))?;
            let field = format_ident!("{}", classification.field);
            let text = match &classification.terminal {
                ClassifiedTerminal::Content => quote! {
                    builder
                        .texts()
                        .get(__recursa_struct.#field)
                        .map(|__recursa_text| __recursa_text.as_str())
                },
                ClassifiedTerminal::Wrapper {
                    node: wrapper,
                    variants: wrapped,
                } => {
                    let wrapper_value = read_pool(*wrapper, quote!(__recursa_struct.#field))?;
                    let wrapper_path = flat_path_of(*wrapper)?;
                    let arms = wrapped.iter().map(|variant| {
                        let variant = format_ident!("{variant}");
                        quote! {
                            #wrapper_path::#variant(__recursa_token) => builder
                                .texts()
                                .get(*__recursa_token)
                                .map(|__recursa_text| __recursa_text.as_str()),
                        }
                    });
                    quote!(match #wrapper_value { #(#arms)* })
                }
            };
            let key = render_classified_key(facade, classification, &rejections.texts, text)?;
            arms.push(quote! {
                #flat_path::#variant_ident(__recursa_payload) => {
                    let __recursa_struct = #payload;
                    #key
                }
            });
            continue;
        }
        let pattern = match variant.style {
            ResolvedFieldStyle::Named => quote!(#flat_path::#variant_ident { .. }),
            ResolvedFieldStyle::Unnamed => quote!(#flat_path::#variant_ident(..)),
            ResolvedFieldStyle::Unit => quote!(#flat_path::#variant_ident),
        };
        arms.push(quote!(#pattern => #facade::RejectionKey::plain(#tag),));
    }
    Ok(quote! {
        |__recursa_value: &#flat_ty| -> #facade::RejectionKey {
            match __recursa_value { #(#arms)* }
        }
    })
}

/// Renders the check one list step runs over the items the list already holds.
///
/// `passed` yields the key of every item in source order, and `added` is the key of the item this
/// step adds together with its extent. The runtime walk owns the semantics; this is the call.
pub(super) fn render_check(
    plan: &EmissionPlan,
    table: &Ident,
    passed: &proc_macro2::TokenStream,
    added: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    quote! {
        if let ::core::option::Option::Some(__recursa_rejection) =
            #facade::__private::first_declared_rejection(
                #passed.chain(::core::iter::once(#added)),
                &#table,
            )
        {
            return Err(reduction.reject(__recursa_rejection));
        }
    }
}

/// Renders the inherent items generated `Arbitrary` sampling needs from one Node with rules.
///
/// A sampled list has to parse back, so the sampler drops every item that a rule refuses, with
/// the same walk the parser runs. The walk needs two things from the element type: the key of one
/// value and the rule table. Both are rendered here, beside the Node's `Arbitrary`
/// implementation, so the arena form's seed mirror receives them from the same call.
pub(super) fn render_arbitrary_support(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    naming: &NestedNaming<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    if plan.rejections.rules(node.node).is_none() {
        return Ok(quote!());
    }
    let table = format_ident!("__RECURSA_REJECTION_RULES");
    let table_items = render_rule_table(plan, node, &table);
    let arms = render_nested_key_arms(plan, node, &quote!(Self), naming)?;
    let node_ident = &node.ident;
    let (impl_generics, type_generics, where_clause) = node.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #node_ident #type_generics #where_clause {
            /// Reports this value's rejection key for its declared rejection rules.
            #[doc(hidden)]
            pub fn __recursa_rejection_key(&self) -> #facade::RejectionKey {
                match self { #(#arms)* }
            }

            /// Returns this type's declared rejection rules, in authored order.
            #[doc(hidden)]
            pub fn __recursa_rejection_rules() -> &'static [#facade::RejectionRule] {
                #table_items
                &#table
            }
        }
    })
}

/// Renders the filter that drops every sampled item a rule refuses.
///
/// The wrappers are walked outermost first, exactly as the seed materializer walks them, so the
/// filter reaches the repetition wherever the authored type puts it. A repetition of an element
/// type with no rule, and a field with no repetition at all, render nothing and keep the plain
/// sampling call. A non-empty list whose every item a text rule refuses cannot be sampled, so the
/// sampler reports that the input bytes describe no value.
pub(super) fn render_arbitrary_filter(
    facade: &Ident,
    wrappers: &[FieldWrapper],
    rules: &proc_macro2::TokenStream,
    value: proc_macro2::TokenStream,
) -> Option<proc_macro2::TokenStream> {
    let (wrapper, rest) = wrappers.split_first()?;
    match wrapper {
        FieldWrapper::Boxed => {
            let inner = render_arbitrary_filter(facade, rest, rules, quote!(*#value))?;
            Some(quote!(Box::new(#inner)))
        }
        FieldWrapper::Optional => {
            let inner = render_arbitrary_filter(facade, rest, rules, quote!(__recursa_sampled))?;
            Some(quote! {
                match #value {
                    ::core::option::Option::Some(__recursa_sampled) => {
                        ::core::option::Option::Some(#inner)
                    }
                    ::core::option::Option::None => ::core::option::Option::None,
                }
            })
        }
        FieldWrapper::ZeroOrMore => Some(quote! {
            {
                let mut __recursa_items = #value;
                let mut __recursa_filter = #facade::__private::RejectionFilter::new(#rules);
                __recursa_items.retain(|__recursa_item| {
                    __recursa_filter.admits(__recursa_item.__recursa_rejection_key())
                });
                __recursa_items
            }
        }),
        FieldWrapper::OneOrMore => Some(quote! {
            {
                let mut __recursa_items = (#value).into_vec();
                let mut __recursa_filter = #facade::__private::RejectionFilter::new(#rules);
                __recursa_items.retain(|__recursa_item| {
                    __recursa_filter.admits(__recursa_item.__recursa_rejection_key())
                });
                let mut __recursa_kept = __recursa_items.into_iter();
                let __recursa_first = __recursa_kept
                    .next()
                    .ok_or(#facade::__private::arbitrary::Error::IncorrectFormat)?;
                let mut __recursa_list = #facade::Vec1::new(__recursa_first);
                for __recursa_item in __recursa_kept {
                    __recursa_list.push(__recursa_item);
                }
                __recursa_list
            }
        }),
    }
}
