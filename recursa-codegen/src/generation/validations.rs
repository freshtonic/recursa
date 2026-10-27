//! Reduce-time checks of one built Node: refused variants and consumer validation hooks
//! (`docs/declared-rejections.md`, "Refused variants and validation hooks").
//!
//! A declared rejection rule of a list reads the items a repetition holds. Some checks of a
//! `gram.y` action read one Node instead: the action refuses a form that its rule matched
//! (`frame start cannot be UNBOUNDED FOLLOWING`), or it compares values that no declaration can
//! name (`wrong number of parameters on left side of OVERLAPS expression`). Both run when the
//! reduce has built the Node, in the value, arena and flat reduces alike, and both end the parse
//! with a declared rejection.

use super::*;
use syn::Meta;

use crate::resolution::ResolvedNodeBody;
use crate::source_syntax::{RejectCheckSyntax, RejectRuleSyntax};

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
/// Every reduce-time check of one grammar, keyed by the Node that declares it.
///
/// The map is ordered, so the plan's `Debug` rendering is stable and the generation identity it
/// feeds is reproducible.
pub(super) struct ValidationPlan {
    /// The checks of each Node.
    nodes: BTreeMap<NodeId, NodeValidations>,
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
/// The checks of one Node: those of every value, and those of one variant only.
struct NodeValidations {
    /// Checks that run for every value of the Node, in authored order.
    node: Vec<ReduceCheck>,
    /// Checks that run for a value of one variant, in authored order.
    variants: BTreeMap<ResolvedVariantId, Vec<ReduceCheck>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// One reduce-time check.
enum ReduceCheck {
    /// `#[reject(variant(V), ...)]`: the variant is refused with this message data.
    Refuse {
        /// Opaque code carried to the consumer.
        code: Option<String>,
        /// Opaque message carried to the consumer.
        message: String,
        /// Opaque hint carried to the consumer.
        hint: Option<String>,
        /// Citation of the source that refuses the variant.
        citation: String,
    },
    /// `#[validate(path, flat = path)]`: consumer functions that may refuse the built value. The
    /// nested one takes `&Node`; the flat one takes `&FlatNode` and the result so far, as a
    /// `&FlatAst` rooted at the flat Node type.
    Hook {
        /// The function the value and arena reduces call with the built Node.
        nested: String,
        /// The function the flat reduce calls with the built flat Node and the builder.
        flat: Option<String>,
    },
}

impl ValidationPlan {
    /// Reports whether the grammar declares no reduce-time check, so no parse keeps extents for
    /// one.
    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Reports whether a declared rule refuses every value of one variant, so `Arbitrary` never
    /// samples it.
    pub(super) fn refuses_variant(&self, node: NodeId, variant: ResolvedVariantId) -> bool {
        self.nodes
            .get(&node)
            .and_then(|checks| checks.variants.get(&variant))
            .is_some_and(|checks| {
                checks
                    .iter()
                    .any(|check| matches!(check, ReduceCheck::Refuse { .. }))
            })
    }

    /// Reports whether some hook reads the flat stores, so the builder must lend them.
    pub(super) fn reads_flat_builder(&self) -> bool {
        self.nodes.values().any(|checks| {
            checks
                .node
                .iter()
                .chain(checks.variants.values().flatten())
                .any(|check| matches!(check, ReduceCheck::Hook { flat: Some(_), .. }))
        })
    }

    /// The checks one reduce of `node` runs: the Node's own, then those of `variant`.
    fn checks(
        &self,
        node: NodeId,
        variant: Option<ResolvedVariantId>,
    ) -> impl Iterator<Item = &ReduceCheck> {
        let checks = self.nodes.get(&node);
        checks
            .into_iter()
            .flat_map(|checks| checks.node.iter())
            .chain(
                checks
                    .zip(variant)
                    .and_then(|(checks, variant)| checks.variants.get(&variant))
                    .into_iter()
                    .flatten(),
            )
    }
}

/// Reads the `validate(...)` declarations of one Node or variant.
///
/// `validate(path)` names the function the value and arena reduces call. `flat = path` names the
/// one the flat reduce calls; a grammar that emits the Flat AST requires it, because the three
/// storages must accept and reject the same sources.
fn validate_hooks(metas: &[Meta], flat: bool, subject: &str) -> Result<Vec<ReduceCheck>, String> {
    let mut hooks = Vec::new();
    for meta in metas.iter().filter(|meta| meta.path().is_ident("validate")) {
        let Meta::List(list) = meta else {
            return Err(format!(
                "`#[validate]` on `{subject}` names the validating function: `#[validate(path)]`"
            ));
        };
        let entries = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
            .map_err(|error| format!("`#[validate(...)]` on `{subject}`: {error}"))?;
        let mut nested = None;
        let mut flat_hook = None;
        for entry in entries {
            match entry {
                Meta::Path(path) if nested.is_none() => {
                    nested = Some(path.to_token_stream().to_string());
                }
                Meta::NameValue(assignment)
                    if assignment.path.is_ident("flat") && flat_hook.is_none() =>
                {
                    let syn::Expr::Path(path) = &assignment.value else {
                        return Err(format!(
                            "`#[validate(..., flat = path)]` on `{subject}` names a function"
                        ));
                    };
                    flat_hook = Some(path.path.to_token_stream().to_string());
                }
                _ => {
                    return Err(format!(
                        "`#[validate(...)]` on `{subject}` takes one function path and an \
                         optional `flat = path`"
                    ));
                }
            }
        }
        let nested = nested.ok_or_else(|| {
            format!("`#[validate(...)]` on `{subject}` names the function the parse calls")
        })?;
        if flat && flat_hook.is_none() {
            return Err(format!(
                "`#[validate({nested})]` on `{subject}` names no `flat = path`; the grammar emits \
                 the Flat AST, and its flat parse must refuse the same sources"
            ));
        }
        hooks.push(ReduceCheck::Hook {
            nested,
            flat: flat.then_some(flat_hook).flatten(),
        });
    }
    Ok(hooks)
}

/// Plans every refused variant and validation hook of a grammar.
///
/// A `#[reject(variant(V), ...)]` declaration belongs to the enum, as every declared rule does,
/// and refuses the values of one variant. A `#[validate(...)]` declaration belongs to a Node or
/// to one variant of an enum.
pub(super) fn plan_validations(
    resolved: &ResolvedSourceGrammar,
    flat: bool,
) -> Result<ValidationPlan, String> {
    let mut plan = ValidationPlan::default();
    for node in &resolved.nodes {
        let mut checks = NodeValidations {
            node: validate_hooks(&node.metas, flat, &node.ident)?,
            variants: BTreeMap::new(),
        };
        if let ResolvedNodeBody::Enum { variants } = &node.body {
            for rule in node
                .metas
                .iter()
                .filter(|meta| meta.path().is_ident("reject"))
                .filter_map(|meta| RejectRuleSyntax::from_meta(meta).ok())
            {
                let RejectCheckSyntax::Variant { variant, .. } = &rule.check else {
                    continue;
                };
                // A name that no variant carries was reported by source-model lowering.
                let Some(variant) = variants
                    .iter()
                    .find(|candidate| candidate.ident == *variant)
                else {
                    continue;
                };
                checks
                    .variants
                    .entry(variant.id)
                    .or_default()
                    .push(ReduceCheck::Refuse {
                        code: rule.code,
                        message: rule.message,
                        hint: rule.hint,
                        citation: rule.citation,
                    });
            }
            for variant in variants {
                let hooks = validate_hooks(
                    &variant.metas,
                    flat,
                    &format!("{}::{}", node.ident, variant.ident),
                )?;
                if !hooks.is_empty() {
                    checks.variants.entry(variant.id).or_default().extend(hooks);
                }
            }
        }
        if !checks.node.is_empty() || !checks.variants.is_empty() {
            plan.nodes.insert(node.id, checks);
        }
    }
    Ok(plan)
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
/// The variants that generated `Arbitrary` sampling never chooses, by `#[omit(Arbitrary)]` on
/// the variant.
///
/// A refused variant is never sampled either, because it never parses back
/// ([`ValidationPlan::refuses_variant`]). An omitted variant parses, but its author knows that
/// most samples of it are values a hook refuses, so sampling it would spend the sample of every
/// Node that holds it. The arena form's seed mirror keeps the Node and variant identities, so it
/// reads the same set and makes the same choice.
pub(super) struct UnsampledVariants {
    /// The omitted variants, keyed by their enum.
    variants: BTreeSet<(NodeId, ResolvedVariantId)>,
}

impl UnsampledVariants {
    /// Reports whether no variant of the grammar is omitted from sampling.
    pub(super) fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }

    /// Reports whether sampling omits one variant.
    pub(super) fn omits(&self, node: NodeId, variant: ResolvedVariantId) -> bool {
        self.variants.contains(&(node, variant))
    }
}

/// Plans the variants that `#[omit(Arbitrary)]` keeps out of generated sampling.
///
/// Source-model lowering reports an `#[omit(...)]` on a variant that names another trait, so a
/// declaration that cannot be read here omits nothing.
pub(super) fn plan_unsampled_variants(resolved: &ResolvedSourceGrammar) -> UnsampledVariants {
    let mut variants = BTreeSet::new();
    for node in &resolved.nodes {
        let ResolvedNodeBody::Enum { variants: declared } = &node.body else {
            continue;
        };
        for variant in declared {
            if crate::source_syntax::variant_omits_arbitrary(&variant.metas).unwrap_or(false) {
                variants.insert((node.id, variant.id));
            }
        }
    }
    UnsampledVariants { variants }
}

/// How one reduce reaches the Node it has built.
pub(super) enum BuiltNode<'a> {
    /// The value or arena reduce: the local that holds the built Node.
    Nested(&'a Ident),
    /// The flat reduce: the local that holds the built flat Node and its flat type; `builder`
    /// is in scope.
    Flat(&'a Ident, &'a Type),
}

/// Renders the reduce-time checks of one reduce, after the Node is built and before it is
/// pushed or allocated.
///
/// `fields` gives, for each semantic field of the built variant in declaration order, the
/// right-hand-side symbols of its value, so a hook that names a field reports the extent of that
/// field, as `parser_errposition` does.
pub(super) fn render_reduce_checks(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    variant: Option<ResolvedVariantId>,
    fields: &[Option<(usize, usize)>],
    built: BuiltNode<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let site = node.ident.unraw().to_string();
    let mut rendered = Vec::new();
    for check in plan.validations.checks(node.node, variant) {
        match check {
            ReduceCheck::Refuse {
                code,
                message,
                hint,
                citation,
            } => {
                let optional = |value: &Option<String>| {
                    value.as_ref().map_or_else(
                        || quote!(::core::option::Option::None),
                        |text| quote!(::core::option::Option::Some(#text)),
                    )
                };
                let code = optional(code);
                let hint = optional(hint);
                // The refusal ends every reduce of the variant. The branch keeps the statements
                // after it reachable for the compiler, so the reduce stays one shape.
                rendered.push(quote! {
                    if ::core::hint::black_box(true) {
                        static __RECURSA_REFUSED_VARIANT: #facade::RejectionNote =
                            #facade::RejectionNote::new(#site, #code, #message, #hint, #citation);
                        return Err(reduction.reject(#facade::Rejection::declared(
                            &__RECURSA_REFUSED_VARIANT,
                            reduction.extent(),
                        )));
                    }
                });
            }
            ReduceCheck::Hook { nested, flat } => {
                let call = match &built {
                    BuiltNode::Nested(value) => {
                        let path = syn::parse_str::<syn::Path>(nested)
                            .map_err(|error| format!("validation hook `{nested}`: {error}"))?;
                        quote!(#path(&#value))
                    }
                    BuiltNode::Flat(value, ty) => {
                        let flat = flat.as_ref().ok_or_else(|| {
                            format!("validation hook `{nested}` of `{site}` has no flat function")
                        })?;
                        let path = syn::parse_str::<syn::Path>(flat)
                            .map_err(|error| format!("validation hook `{flat}`: {error}"))?;
                        quote! {
                            {
                                let __recursa_lent = builder.lend::<#ty>();
                                let __recursa_checked = #path(&#value, &__recursa_lent);
                                builder.restore(__recursa_lent);
                                __recursa_checked
                            }
                        }
                    }
                };
                let arms = fields.iter().enumerate().filter_map(|(index, extent)| {
                    let (start, len) = (*extent)?;
                    Some(quote! {
                        ::core::option::Option::Some(#index) => {
                            reduction.symbol_range_extent(#start, #len)
                        }
                    })
                });
                rendered.push(quote! {
                    if let ::core::result::Result::Err(__recursa_refusal) = #call {
                        let __recursa_span = match __recursa_refusal.field() {
                            #(#arms)*
                            _ => reduction.extent(),
                        };
                        return Err(reduction.reject(#facade::Rejection::declared(
                            __recursa_refusal.note(),
                            __recursa_span,
                        )));
                    }
                });
            }
        }
    }
    Ok(quote!(#(#rendered)*))
}

/// Finds, for each semantic element of one reduce in declaration order, the right-hand-side
/// symbols of its value: the one value-carrying symbol of its site, or the whole site.
pub(super) fn field_extents(
    lr: &TableDrivenRendering,
    rule: &crate::lr::Rule,
    elements: &[ElementParsePlan],
    forms: &[crate::lr::SiteForm],
) -> Vec<Option<(usize, usize)>> {
    elements
        .iter()
        .zip(forms)
        .filter(|(element, _)| matches!(element.kind, ElementParseKind::Semantic { .. }))
        .map(|(_, form)| {
            let (start, len) = (form.start as usize, form.len as usize);
            if len == 0 {
                return None;
            }
            let value = rule
                .rhs
                .get(start..start + len)?
                .iter()
                .position(|symbol| lr.symbol_carries_value(*symbol));
            Some(value.map_or((start, len), |offset| (start + offset, 1)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the hooks of one authored attribute list.
    fn hooks(attributes: &str, flat: bool) -> Result<Vec<ReduceCheck>, String> {
        let item: syn::ItemStruct = syn::parse_str(&format!("{attributes} struct Node;"))
            .expect("the test attributes parse");
        let metas = item
            .attrs
            .iter()
            .map(|attribute| attribute.meta.clone())
            .collect::<Vec<_>>();
        validate_hooks(&metas, flat, "Node")
    }

    #[test]
    fn a_hook_names_one_function_and_a_flat_grammar_names_its_flat_function() {
        assert_eq!(
            hooks("#[validate(crate::check)]", false).unwrap(),
            vec![ReduceCheck::Hook {
                nested: "crate :: check".to_owned(),
                flat: None,
            }],
        );
        assert_eq!(
            hooks("#[validate(crate::check, flat = crate::check_flat)]", true).unwrap(),
            vec![ReduceCheck::Hook {
                nested: "crate :: check".to_owned(),
                flat: Some("crate :: check_flat".to_owned()),
            }],
        );
        let missing = hooks("#[validate(crate::check)]", true).unwrap_err();
        assert!(missing.contains("names no `flat = path`"), "{missing}");
        let malformed = hooks("#[validate]", false).unwrap_err();
        assert!(
            malformed.contains("names the validating function"),
            "{malformed}"
        );
        let extra = hooks("#[validate(crate::a, crate::b)]", false).unwrap_err();
        assert!(extra.contains("takes one function path"), "{extra}");
    }
}
