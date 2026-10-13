//! Declared rejections: the plan and the two generated list checks.
//!
//! A `#[reject(pair(A, B), code = ..., message = ..., cite = ...)]` declaration on an `ast_node!`
//! enum names forbidden unordered pairs of its variants. Every repetition whose element type is
//! that enum runs the rules as the list is built, and the first item that completes a pair ends
//! the parse. `docs/declared-rejections.md` is the design.
//!
//! This module owns the recording half: it reads the surviving declarations out of the resolved
//! grammar, assigns each variant its tag, and renders the static rule table, the tag function and
//! the check that both list backends call. Source-model lowering owns the authoring diagnostics,
//! so everything here may assume a readable, resolved declaration.
//!
//! The check runs at the reduce of the list step, which is the one place that holds both the items
//! the list has passed and the extent of the item just added. It therefore changes no LR table: a
//! list step is a rule the lowering already emits.

use super::*;
use syn::Meta;

use crate::resolution::ResolvedNodeBody;
use crate::source_syntax::RejectRuleSyntax;

/// One rejection rule of one enum, with its variant names resolved to tags.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct RejectionRecord {
    /// Forbidden pairs as tag pairs, in written order.
    pairs: Vec<(u32, u32)>,
    /// Opaque code carried to the consumer.
    code: Option<String>,
    /// Opaque message carried to the consumer.
    message: String,
    /// Opaque hint carried to the consumer.
    hint: Option<String>,
    /// Citation of the source that rejects the pair.
    citation: String,
}

/// Every declared rejection rule of one grammar, keyed by the enum that declares it.
///
/// The rules of one enum keep their authored attribute order and the pairs of one rule keep their
/// written order, because the first pair a list completes decides the message. A tag is the
/// variant's position in the Node plan's own variant list, so the plan, the generated tag
/// function and the pair table always agree. The map is ordered, so the plan's `Debug` rendering
/// is stable and the generation identity it feeds is reproducible.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(super) struct RejectionPlan {
    /// Rules of one Node type, in authored order.
    nodes: BTreeMap<NodeId, Vec<RejectionRecord>>,
}

impl RejectionPlan {
    /// Reports whether the grammar declares no rejection rule at all.
    ///
    /// A grammar that declares none pays nothing: no check is rendered and no parse of it keeps
    /// the LR extent stack.
    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the rules of one Node type, in authored order.
    pub(super) fn rules(&self, node: NodeId) -> Option<&[RejectionRecord]> {
        self.nodes.get(&node).map(Vec::as_slice)
    }
}

/// Reads one Node's `reject(...)` metas into its rules, with pair names resolved to tags.
///
/// A name that no variant carries is dropped here, because source-model lowering already
/// reported it and generation never runs on a rejected model.
fn records_from_metas(metas: &[Meta], variants: &[String]) -> Vec<RejectionRecord> {
    metas
        .iter()
        .filter(|meta| meta.path().is_ident("reject"))
        .filter_map(|meta| RejectRuleSyntax::from_meta(meta).ok())
        .filter_map(|rule| {
            let tag = |name: &String| {
                variants
                    .iter()
                    .position(|variant| variant == name)
                    .and_then(|index| u32::try_from(index).ok())
            };
            let pairs = rule
                .pairs
                .iter()
                .filter_map(|pair| Some((tag(&pair.left)?, tag(&pair.right)?)))
                .collect::<Vec<_>>();
            (!pairs.is_empty()).then_some(RejectionRecord {
                pairs,
                code: rule.code,
                message: rule.message,
                hint: rule.hint,
                citation: rule.citation,
            })
        })
        .collect()
}

/// Builds the complete rejection plan from the resolved grammar and the planned nodes.
pub(super) fn plan_rejections(
    resolved: &ResolvedSourceGrammar,
    nodes: &[NodeParsePlan],
) -> RejectionPlan {
    let mut plan = RejectionPlan::default();
    for node in &resolved.nodes {
        if !node.metas.iter().any(|meta| meta.path().is_ident("reject")) {
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
        let names = variants
            .iter()
            .map(|variant| variant.ident.unraw().to_string())
            .collect::<Vec<_>>();
        let records = records_from_metas(&node.metas, &names);
        if !records.is_empty() {
            plan.nodes.insert(node.id, records);
        }
    }
    plan
}

/// Renders one Node's static rule table under `table`.
pub(super) fn render_rule_table(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    table: &Ident,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let Some(rules) = plan.rejections.rules(node.node) else {
        return quote!();
    };
    let site = node.ident.unraw().to_string();
    let optional = |value: &Option<String>| {
        value.as_ref().map_or_else(
            || quote!(::core::option::Option::None),
            |text| quote!(::core::option::Option::Some(#text)),
        )
    };
    let mut pair_tables = Vec::new();
    let mut entries = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let pairs_ident = format_ident!("{table}_PAIRS_{index}");
        let count = rule.pairs.len();
        let pairs = rule
            .pairs
            .iter()
            .map(|(left, right)| quote!(#facade::RejectionPair::new(#left, #right)));
        pair_tables.push(quote! {
            static #pairs_ident: [#facade::RejectionPair; #count] = [#(#pairs),*];
        });
        let code = optional(&rule.code);
        let message = &rule.message;
        let hint = optional(&rule.hint);
        let citation = &rule.citation;
        entries.push(quote! {
            #facade::RejectionRule::new(
                &#pairs_ident,
                #facade::RejectionNote::new(#site, #code, #message, #hint, #citation),
            )
        });
    }
    let count = entries.len();
    quote! {
        #(#pair_tables)*
        static #table: [#facade::RejectionRule; #count] = [#(#entries),*];
    }
}

/// Renders the function that reports one value's variant tag.
///
/// A variant matches by name, so the payload never takes part: every arm is a wildcard pattern
/// over the variant's own shape. The tag is the variant's position in the Node plan's variant
/// list, which is the position the pair table used.
pub(super) fn render_tag_function(
    node: &NodeParsePlan,
    ident: &Ident,
    value_type: &proc_macro2::TokenStream,
    type_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let NodeBodyPlan::Enum { variants } = &node.body else {
        return quote!();
    };
    let arms = variants.iter().enumerate().map(|(index, variant)| {
        let tag = u32::try_from(index).unwrap_or(u32::MAX);
        let variant_ident = &variant.ident;
        let pattern = match variant.style {
            ResolvedFieldStyle::Named => quote!(#type_path::#variant_ident { .. }),
            ResolvedFieldStyle::Unnamed => quote!(#type_path::#variant_ident(..)),
            ResolvedFieldStyle::Unit => quote!(#type_path::#variant_ident),
        };
        quote!(#pattern => #tag,)
    });
    let documentation = format!(
        " Reports the variant tag of one `{}` for its declared rejection rules.",
        node.ident,
    );
    quote! {
        #[doc = #documentation]
        fn #ident(value: &#value_type) -> u32 {
            match value { #(#arms)* }
        }
    }
}

/// Renders the check one list step runs over the items the list already holds.
///
/// `passed` yields the tag of every item in source order, and `added` is the tag of the item this
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
/// A sampled list has to parse back, so the sampler drops every item that would complete a
/// forbidden pair, with the same walk the parser runs. The walk needs two things from the element
/// type: the tag of one value and the rule table. Both are rendered here, beside the Node's
/// `Arbitrary` implementation, so the arena form's seed mirror receives them from the same call.
pub(super) fn render_arbitrary_support(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    if plan.rejections.rules(node.node).is_none() {
        return quote!();
    }
    let NodeBodyPlan::Enum { variants } = &node.body else {
        return quote!();
    };
    let table = format_ident!("__RECURSA_REJECTION_RULES");
    let table_items = render_rule_table(plan, node, &table);
    let arms = variants.iter().enumerate().map(|(index, variant)| {
        let tag = u32::try_from(index).unwrap_or(u32::MAX);
        let variant_ident = &variant.ident;
        let pattern = match variant.style {
            ResolvedFieldStyle::Named => quote!(Self::#variant_ident { .. }),
            ResolvedFieldStyle::Unnamed => quote!(Self::#variant_ident(..)),
            ResolvedFieldStyle::Unit => quote!(Self::#variant_ident),
        };
        quote!(#pattern => #tag,)
    });
    let node_ident = &node.ident;
    let (impl_generics, type_generics, where_clause) = node.generics.split_for_impl();
    quote! {
        impl #impl_generics #node_ident #type_generics #where_clause {
            /// Reports this value's variant tag for its declared rejection rules.
            #[doc(hidden)]
            pub fn __recursa_rejection_tag(&self) -> u32 {
                match self { #(#arms)* }
            }

            /// Returns this type's declared rejection rules, in authored order.
            #[doc(hidden)]
            pub fn __recursa_rejection_rules() -> &'static [#facade::RejectionRule] {
                #table_items
                &#table
            }
        }
    }
}

/// Renders the filter that drops every sampled item which would complete a forbidden pair.
///
/// The wrappers are walked outermost first, exactly as the seed materializer walks them, so the
/// filter reaches the repetition wherever the authored type puts it. A repetition of an element
/// type with no rule, and a field with no repetition at all, render nothing and keep the plain
/// sampling call.
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
            Some(quote!((#value).map(|__recursa_sampled| #inner)))
        }
        FieldWrapper::ZeroOrMore => Some(quote! {
            {
                let mut __recursa_items = #value;
                let mut __recursa_filter = #facade::__private::RejectionFilter::new(#rules);
                __recursa_items.retain(|__recursa_item| {
                    __recursa_filter.admits(__recursa_item.__recursa_rejection_tag())
                });
                __recursa_items
            }
        }),
        FieldWrapper::OneOrMore => Some(quote! {
            {
                let mut __recursa_items = (#value).into_vec();
                let mut __recursa_filter = #facade::__private::RejectionFilter::new(#rules);
                __recursa_items.retain(|__recursa_item| {
                    __recursa_filter.admits(__recursa_item.__recursa_rejection_tag())
                });
                let mut __recursa_kept = __recursa_items.into_iter();
                // The first item of a list completes no pair, because a pair never names one
                // variant twice, so the filter always keeps it.
                let __recursa_first = __recursa_kept
                    .next()
                    .expect("the first item of a sampled list completes no forbidden pair");
                let mut __recursa_list = #facade::Vec1::new(__recursa_first);
                for __recursa_item in __recursa_kept {
                    __recursa_list.push(__recursa_item);
                }
                __recursa_list
            }
        }),
    }
}
