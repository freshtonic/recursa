//! Pretty planning, authoritative validation, and structured rendering.
//!
//! Pretty plan construction and emission are intentionally co-located: spacing, provenance,
//! Pratt parenthesization, repetition, and attachment layout form one behavioral contract.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    macro_rules! focused_case {
        ($name:ident) => {
            #[test]
            fn $name() {
                test_support::$name();
            }
        };
    }
    focused_case!(authored_pretty_authority_rejects_emitted_declaration_corruption);
    focused_case!(pretty_pratt_recursion_uses_typed_node_identity_not_the_final_path_segment);
    focused_case!(pretty_pratt_grouping_follows_a_unique_authored_helper_path);
    focused_case!(pretty_pratt_variants_are_typechecked_in_isolated_helpers);
    focused_case!(every_typed_pretty_pratt_decision_is_part_of_family_authority);
    focused_case!(recursive_concrete_pretty_impls_do_not_publish_cyclic_child_bounds);
    focused_case!(every_local_pretty_hint_dimension_is_part_of_family_authority);
    focused_case!(ascii_insensitive_word_separators_share_canonical_pretty_family_authority);
    focused_case!(generic_top_level_breaks_keep_typed_family_and_closed_application_authority);
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete optional rendering projection for tokens and nodes.
pub(super) struct PrettyEmissionPlan {
    /// Optional token rendering behavior in semantic token order.
    pub(super) tokens: Vec<Option<PrettyTokenBehavior>>,
    /// Optional Node rendering behavior in parse-plan order.
    pub(super) nodes: Vec<Option<PrettyNodeBehavior>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source-text strategy for a Pretty-enabled token.
pub(super) enum PrettyTokenBehavior {
    /// Uses matched source when present and the stored value otherwise.
    MatchedOrStoredText,
}

/// Reports whether locally configured Node rendering can require private token rendering.
pub(super) fn content_token_pretty_helper_enabled(plan: &EmissionPlan) -> bool {
    plan.pretty.nodes.iter().any(Option::is_some)
}

/// Renders private wrapper traversal for content tokens that intentionally omit public `Pretty`.
pub(super) fn render_content_token_pretty_helper(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    if !content_token_pretty_helper_enabled(plan) {
        return proc_macro2::TokenStream::new();
    }
    let facade = &plan.facade;
    quote! {
        #[doc(hidden)]
        pub(crate) trait __RecursaPrettyWith {
            fn __recursa_pretty_with(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result;
        }

        impl<T> __RecursaPrettyWith for Box<T>
        where
            T: __RecursaPrettyWith,
        {
            fn __recursa_pretty_with(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                (**self).__recursa_pretty_with(context)
            }
        }

        impl<T> __RecursaPrettyWith for Option<T>
        where
            T: __RecursaPrettyWith,
        {
            fn __recursa_pretty_with(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                if let Some(value) = self {
                    value.__recursa_pretty_with(context)?;
                }
                Ok(())
            }
        }

        impl<T> __RecursaPrettyWith for Vec<T>
        where
            T: __RecursaPrettyWith,
        {
            fn __recursa_pretty_with(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                for value in self {
                    value.__recursa_pretty_with(context)?;
                }
                Ok(())
            }
        }

        impl<T> __RecursaPrettyWith for #facade::Vec1<T>
        where
            T: __RecursaPrettyWith,
        {
            fn __recursa_pretty_with(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                for value in self.iter() {
                    value.__recursa_pretty_with(context)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Layout, element, and Pratt behavior shared by a node family.
pub(super) struct PrettyNodeBehavior {
    /// Node declaration owning this rendering behavior.
    pub(super) node: NodeId,
    /// Materialized applications sharing the behavior.
    pub(super) applications: Vec<MaterializedApplicationId>,
    /// Node-level grouping and break hints.
    pub(super) pretty: PrettyHints,
    /// Variant-local hints in source variant order.
    pub(super) variant_pretty: Vec<PrettyHints>,
    /// Linear or delimited layout for each variant shape.
    pub(super) variant_layouts: Vec<PrettySequenceLayout>,
    /// Element rendering plans in variant and source-element order.
    pub(super) variants: Vec<Vec<PrettyElementBehavior>>,
    /// Pratt-specific grouping and parenthesization decisions.
    pub(super) pratt: Option<PrettyPrattBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grouping and variant-specific rendering rules for a Pratt node.
pub(super) struct PrettyPrattBehavior {
    /// Authored delimiter behavior for explicit grouping variants.
    pub(super) grouping: Option<PrettyPrattGrouping>,
    /// Parenthesization and operand plans in Pratt variant order.
    pub(super) variants: Vec<PrettyPrattVariantBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Fixed syntax surrounding an explicit recursive grouping.
pub(super) struct PrettyPrattGrouping {
    /// Fixed syntax rendered before the recursively grouped expression.
    pub(super) before: Vec<PrettyAttachedFixedBehavior>,
    /// Fixed syntax rendered after the recursively grouped expression.
    pub(super) after: Vec<PrettyAttachedFixedBehavior>,
}

#[derive(Clone, Debug, Default)]
/// Compact syntax graph used to prove an indirect authored Pratt grouping path.
pub(super) struct PrettyPrattGroupingSyntax {
    /// Structural helper bodies keyed by generated Node identity; Pratt bodies are absent.
    nodes: BTreeMap<NodeId, Vec<Vec<PrettyPrattGroupingElement>>>,
}

#[derive(Clone, Debug)]
/// One fixed, optional, or semantic step in a grouping-proof sequence.
enum PrettyPrattGroupingElement {
    /// Required fixed syntax retained in canonical Pretty form.
    Fixed(PrettyAttachedFixedBehavior),
    /// Syntax that can be omitted from the selected grouping derivation.
    Optional,
    /// A stored semantic value and its exact storage/attachment shape.
    Semantic {
        /// Generated terminal Node, when the field stores one.
        terminal_node: Option<NodeId>,
        /// Storage wrappers from the field outward to its terminal.
        wrappers: Vec<FieldWrapper>,
        /// Required fixed syntax before the value, absent when any entry is optional.
        before: Option<Vec<PrettyAttachedFixedBehavior>>,
        /// Required fixed syntax after the value, absent when any entry is optional.
        after: Option<Vec<PrettyAttachedFixedBehavior>>,
        /// Whether every attached entry can disappear with the semantic value.
        attachment_can_be_empty: bool,
    },
    /// A required semantic projection that cannot transparently forward the owner.
    Opaque,
}

#[derive(Clone, Debug, Default)]
/// Empty and single-owner derivations for one grouping syntax fragment.
struct PrettyPrattGroupingProof {
    /// Whether the fragment can contribute no required syntax.
    empty: bool,
    /// Authored single-owner paths, capped at two to distinguish ambiguity.
    witnesses: Vec<PrettyPrattGroupingWitness>,
    /// Whether a non-owner helper cycle made the finite proof inconclusive.
    cyclic: bool,
}

#[derive(Clone, Debug)]
/// One grouping derivation and whether it traverses an authored helper Node.
struct PrettyPrattGroupingWitness {
    /// Fixed syntax synthesized around the precedence-sensitive expression.
    grouping: PrettyPrattGrouping,
    /// Whether the recursive owner is reached indirectly through a helper.
    delegated: bool,
}

impl PrettyPrattGroupingProof {
    /// Retains enough paths to distinguish a unique witness from ambiguity.
    fn push_witness(&mut self, witness: PrettyPrattGroupingWitness) {
        if self.witnesses.len() < 2 {
            self.witnesses.push(witness);
        }
    }
}

/// Run-local, cycle-safe analyzer for one owning Pratt family.
struct PrettyPrattGroupingAnalyzer<'a> {
    /// Structural syntax graph independently projected from source or frozen parse plans.
    syntax: &'a PrettyPrattGroupingSyntax,
    /// Completed helper summaries keyed by owner and helper Node identities.
    memo: BTreeMap<(NodeId, NodeId), PrettyPrattGroupingProof>,
    /// Active helper Nodes used to reject context-dependent recursive proofs.
    active: BTreeSet<NodeId>,
    /// Exact fixed-only helper expansions, including failed proofs.
    fixed_memo: BTreeMap<NodeId, Option<Vec<PrettyAttachedFixedBehavior>>>,
    /// Fixed-only helper Nodes active in the current depth-first proof.
    fixed_active: BTreeSet<NodeId>,
}

impl<'a> PrettyPrattGroupingAnalyzer<'a> {
    /// Creates one analyzer whose memo is shared by every atom in a Pratt family.
    fn new(syntax: &'a PrettyPrattGroupingSyntax) -> Self {
        Self {
            syntax,
            memo: BTreeMap::new(),
            active: BTreeSet::new(),
            fixed_memo: BTreeMap::new(),
            fixed_active: BTreeSet::new(),
        }
    }

    /// Proves exactly one required, delimiter-bounded path through an atom sequence.
    fn grouping(
        &mut self,
        owner: NodeId,
        elements: &[PrettyPrattGroupingElement],
    ) -> Option<PrettyPrattGroupingWitness> {
        let proof = self.elements(owner, elements);
        matches!(proof.witnesses.as_slice(), [witness]
        if !proof.cyclic
            && if witness.delegated {
                witness.grouping.before.len() == 1 && witness.grouping.after.len() == 1
            } else {
                !witness.grouping.before.is_empty() && !witness.grouping.after.is_empty()
            })
        .then(|| proof.witnesses[0].clone())
    }

    /// Derives empty and single-owner paths through one element sequence.
    fn elements(
        &mut self,
        owner: NodeId,
        elements: &[PrettyPrattGroupingElement],
    ) -> PrettyPrattGroupingProof {
        let empty = elements
            .iter()
            .all(|element| self.element_is_empty(owner, element));
        let mut proof = PrettyPrattGroupingProof {
            empty,
            ..PrettyPrattGroupingProof::default()
        };
        for (index, element) in elements.iter().enumerate() {
            let PrettyPrattGroupingElement::Semantic {
                terminal_node,
                wrappers,
                before: Some(attached_before),
                after: Some(attached_after),
                ..
            } = element
            else {
                continue;
            };
            if wrappers
                .iter()
                .any(|wrapper| matches!(wrapper, FieldWrapper::Optional | FieldWrapper::ZeroOrMore))
            {
                continue;
            }
            let Some(fixed_before) = self.fixed(owner, &elements[..index]) else {
                continue;
            };
            let Some(fixed_after) = self.fixed(owner, &elements[index + 1..]) else {
                continue;
            };
            let mut nested =
                terminal_node.map_or_else(PrettyPrattGroupingProof::default, |terminal| {
                    if terminal == owner {
                        PrettyPrattGroupingProof {
                            witnesses: vec![PrettyPrattGroupingWitness {
                                grouping: PrettyPrattGrouping {
                                    before: Vec::new(),
                                    after: Vec::new(),
                                },
                                delegated: false,
                            }],
                            ..PrettyPrattGroupingProof::default()
                        }
                    } else {
                        self.node(owner, terminal)
                    }
                });
            proof.cyclic |= nested.cyclic;
            for witness in nested.witnesses.drain(..) {
                let mut context_before = fixed_before.clone();
                context_before.extend(attached_before.iter().cloned());
                let mut context_after = attached_after.clone();
                context_after.extend(fixed_after.iter().cloned());
                let context_is_transparent = context_before.is_empty() && context_after.is_empty();
                let context_is_enclosure = !context_before.is_empty() && !context_after.is_empty();
                let witness_is_transparent =
                    witness.grouping.before.is_empty() && witness.grouping.after.is_empty();
                if context_is_transparent || (context_is_enclosure && witness_is_transparent) {
                    context_before.extend(witness.grouping.before);
                    let mut after = witness.grouping.after;
                    after.extend(context_after);
                    proof.push_witness(PrettyPrattGroupingWitness {
                        grouping: PrettyPrattGrouping {
                            before: context_before,
                            after,
                        },
                        delegated: witness.delegated,
                    });
                }
            }
        }
        proof
    }

    /// Extracts required fixed syntax while optional elements take their empty derivation.
    fn fixed(
        &mut self,
        owner: NodeId,
        elements: &[PrettyPrattGroupingElement],
    ) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        let mut fixed = Vec::new();
        for element in elements {
            match element {
                PrettyPrattGroupingElement::Fixed(token) => fixed.push(token.clone()),
                PrettyPrattGroupingElement::Optional => {}
                PrettyPrattGroupingElement::Semantic { .. }
                    if self.element_is_empty(owner, element) => {}
                PrettyPrattGroupingElement::Semantic { .. } => {
                    let nested = self.fixed_only_element(element)?;
                    if nested.is_empty() {
                        return None;
                    }
                    fixed.extend(nested);
                }
                PrettyPrattGroupingElement::Opaque => return None,
            }
        }
        Some(fixed)
    }

    /// Expands one required semantic marker into exact fixed Pretty syntax.
    fn fixed_only_element(
        &mut self,
        element: &PrettyPrattGroupingElement,
    ) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        let PrettyPrattGroupingElement::Semantic {
            terminal_node: Some(terminal),
            wrappers,
            before: Some(before),
            after: Some(after),
            ..
        } = element
        else {
            return None;
        };
        if wrappers
            .iter()
            .any(|wrapper| !matches!(wrapper, FieldWrapper::Boxed))
        {
            return None;
        }
        let mut fixed = before.clone();
        fixed.extend(self.fixed_only_node(*terminal)?);
        fixed.extend(after.iter().cloned());
        Some(fixed)
    }

    /// Expands a single-path, non-recursive fixed-only helper Node.
    fn fixed_only_node(&mut self, node: NodeId) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        if let Some(fixed) = self.fixed_memo.get(&node) {
            return fixed.clone();
        }
        if !self.fixed_active.insert(node) {
            return None;
        }
        let fixed = self
            .syntax
            .nodes
            .get(&node)
            .filter(|variants| variants.len() == 1)
            .map(|variants| variants[0].clone())
            .and_then(|elements| self.fixed_only_elements(&elements));
        self.fixed_active.remove(&node);
        self.fixed_memo.insert(node, fixed.clone());
        fixed
    }

    /// Expands one helper sequence containing only required fixed syntax.
    fn fixed_only_elements(
        &mut self,
        elements: &[PrettyPrattGroupingElement],
    ) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        let mut fixed = Vec::new();
        for element in elements {
            match element {
                PrettyPrattGroupingElement::Fixed(token) => fixed.push(token.clone()),
                PrettyPrattGroupingElement::Semantic { .. } => {
                    fixed.extend(self.fixed_only_element(element)?);
                }
                PrettyPrattGroupingElement::Optional | PrettyPrattGroupingElement::Opaque => {
                    return None;
                }
            }
        }
        Some(fixed)
    }

    /// Reports whether one non-path element can disappear without required syntax.
    fn element_is_empty(&mut self, owner: NodeId, element: &PrettyPrattGroupingElement) -> bool {
        match element {
            PrettyPrattGroupingElement::Optional => true,
            PrettyPrattGroupingElement::Semantic {
                terminal_node,
                wrappers,
                attachment_can_be_empty,
                ..
            } => {
                *attachment_can_be_empty
                    && (wrappers.iter().any(|wrapper| {
                        matches!(wrapper, FieldWrapper::Optional | FieldWrapper::ZeroOrMore)
                    }) || terminal_node.is_some_and(|terminal| {
                        if terminal == owner {
                            return false;
                        }
                        let proof = self.node(owner, terminal);
                        !proof.cyclic && proof.empty
                    }))
            }
            PrettyPrattGroupingElement::Fixed(_) | PrettyPrattGroupingElement::Opaque => false,
        }
    }

    /// Derives and memoizes one structural helper Node summary.
    fn node(&mut self, owner: NodeId, node: NodeId) -> PrettyPrattGroupingProof {
        if let Some(proof) = self.memo.get(&(owner, node)) {
            return proof.clone();
        }
        if !self.active.insert(node) {
            return PrettyPrattGroupingProof {
                cyclic: true,
                ..PrettyPrattGroupingProof::default()
            };
        }
        let mut proof = PrettyPrattGroupingProof::default();
        if let Some(variants) = self.syntax.nodes.get(&node) {
            for elements in variants {
                let variant = self.elements(owner, elements);
                proof.empty |= variant.empty;
                proof.cyclic |= variant.cyclic;
                for witness in variant.witnesses {
                    proof.push_witness(witness);
                }
            }
        }
        self.active.remove(&node);
        for witness in &mut proof.witnesses {
            witness.delegated = true;
        }
        if !proof.cyclic {
            self.memo.insert((owner, node), proof.clone());
        }
        proof
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Parenthesization and operands for one Pratt variant.
pub(super) struct PrettyPrattVariantBehavior {
    /// Entry/exit binding powers controlling inserted grouping.
    pub(super) parenthesization: PrettyPrattParenthesization,
    /// Ordinary or recursive operand behavior in semantic-field order.
    pub(super) operands: Vec<PrettyPrattOperandBehavior>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Binding-power interval governing inserted parentheses.
pub(super) struct PrettyPrattParenthesization {
    /// Left binding power required to enter this variant, when led.
    pub(super) entry_lbp: Option<u16>,
    /// Right binding power exposed after rendering this variant.
    pub(super) exit_rbp: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Rendering rule for an ordinary or recursively bound operand.
pub(super) enum PrettyPrattOperandBehavior {
    /// Renders through the field's normal Pretty implementation.
    Ordinary,
    /// Renders through the owning Pratt family's binding-power entry point.
    Recursive {
        /// Pratt Node recursively targeted by the operand.
        owner: NodeId,
        /// Concrete recursive applications accepted at this site.
        applications: Vec<MaterializedApplicationId>,
        /// Minimum binding power passed into recursive rendering.
        minimum: PrettyPrattMinimum,
        /// Binding-power context following the rendered operand.
        next: PrettyPrattNext,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Minimum binding power passed to recursive Pratt rendering.
pub(super) enum PrettyPrattMinimum {
    /// Allows every extender.
    Zero,
    /// Reuses the enclosing variant's inner binding context.
    Inner,
    /// Requires the stored concrete binding power.
    Binding(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Binding context exposed after rendering a Pratt operand.
pub(super) enum PrettyPrattNext {
    /// Exposes no following binding constraint.
    None,
    /// Reuses the enclosing variant's inner binding context.
    Inner,
    /// Exposes the stored concrete binding power.
    Binding(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Grouping strategy for an emitted element sequence.
pub(super) enum PrettySequenceLayout {
    /// Emits elements in the current document group.
    Linear,
    /// Groups a delimiter-bounded sequence for coordinated breaks.
    Delimited,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Rendering rule for fixed syntax or a semantic field.
pub(super) enum PrettyElementBehavior {
    /// Emits canonical authored syntax with mechanical spacing.
    Fixed {
        /// Canonical fixed-token text emitted for synthetic syntax.
        spelling: String,
        /// Mechanical spacing class between adjacent fixed syntax.
        spacing: PrettySpacing,
        /// Placement relative to the surrounding sequence.
        rendering: PrettyFixedRendering,
        /// Whether the layout may break immediately before the token.
        soft_break_before: bool,
        /// Whether the layout may break immediately after the token.
        soft_break_after: bool,
    },
    /// Emits one erased fixed group only when parsed provenance records it as present.
    OptionalFixedTokenGroup {
        /// Optional fixed syntax in source order.
        tokens: Vec<PrettyAttachedFixedBehavior>,
    },
    /// Emits one stored field value through its selected projection.
    Semantic {
        /// Selects direct value rendering or fixed-presence rendering.
        projection: PrettySemanticProjection,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies how Pretty renders one authored semantic field value.
pub(super) enum PrettySemanticProjection {
    /// Emits required syntax around an optional fixed sequence controlled by a boolean field.
    FixedPresence {
        /// Fixed syntax before the optional sequence.
        before: Vec<PrettyAttachedSyntaxBehavior>,
        /// Optional fixed syntax emitted when the field is true.
        sequence: Vec<PrettyAttachedFixedBehavior>,
        /// Fixed syntax after the optional sequence.
        after: Vec<PrettyAttachedSyntaxBehavior>,
        /// Field-local grouping and break hints.
        pretty: PrettyHints,
    },
    /// Emits a stored value with container and provenance behavior.
    Direct {
        /// Separator and line-breaking behavior for repeated values.
        repetition: Option<PrettyRepetitionBehavior>,
        /// Fixed syntax attached to the semantic value.
        attachment: Option<PrettyAttachmentBehavior>,
        /// Whether source mapping follows structured child provenance or an opaque span.
        provenance: PrettyProvenanceBehavior,
        /// Field-local grouping and break hints.
        pretty: PrettyHints,
        /// Always, optional, or non-empty emission condition.
        presence: PrettyPresence,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Separator and line-breaking rules for a repeated field.
pub(super) struct PrettyRepetitionBehavior {
    /// Canonical separator spelling, or `None` for adjacent items.
    pub(super) separator: Option<String>,
    /// Whether a separator follows the final item.
    pub(super) trailing_separator: bool,
    /// Whether every item boundary forces a line break.
    pub(super) hard_break: bool,
    /// Whether separators render before rather than after subsequent items.
    pub(super) leading_separator: bool,
    /// Whether non-leading separators require preceding whitespace.
    pub(super) space_before_separator: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Condition under which a semantic field contributes output.
pub(super) enum PrettyPresence {
    /// Field always contributes a document.
    Always,
    /// Field contributes output only when present.
    Optional,
    /// Collection contributes output only when non-empty.
    NonEmpty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source-map granularity for a semantic field.
pub(super) enum PrettyProvenanceBehavior {
    /// Preserves child-level source-map structure.
    Structured,
    /// Maps the field as one indivisible source span.
    Opaque,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Layout and fixed syntax surrounding a semantic value.
pub(super) struct PrettyAttachmentBehavior {
    /// Sequence grouping shared by attached fixed syntax and its value.
    pub(super) layout: PrettySequenceLayout,
    /// Fixed syntax attached before the semantic value.
    pub(super) before: Vec<PrettyAttachedSyntaxBehavior>,
    /// Fixed syntax attached after the semantic value.
    pub(super) after: Vec<PrettyAttachedSyntaxBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies required and erased-optional fixed syntax in Pretty attachments.
pub(super) enum PrettyAttachedSyntaxBehavior {
    /// Emits one required fixed token.
    Required(PrettyAttachedFixedBehavior),
    /// Emits one fixed sequence only when its parsed occurrence has a source span.
    Optional(Vec<PrettyAttachedFixedBehavior>),
}

/// Extracts required-only syntax for delimiter grouping behavior.
fn required_pretty_syntax(
    syntax: &[PrettyAttachedSyntaxBehavior],
) -> Option<Vec<PrettyAttachedFixedBehavior>> {
    syntax
        .iter()
        .map(|syntax| match syntax {
            PrettyAttachedSyntaxBehavior::Required(fixed) => Some(fixed.clone()),
            PrettyAttachedSyntaxBehavior::Optional(_) => None,
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Canonical spelling and spacing for one attached token.
pub(super) struct PrettyAttachedFixedBehavior {
    /// Canonical fixed spelling emitted by the attachment.
    pub(super) spelling: String,
    /// Mechanical spacing class for neighboring syntax.
    pub(super) spacing: PrettySpacing,
    /// Placement and break behavior within the attachment.
    pub(super) rendering: PrettyFixedRendering,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Mechanical placement class for fixed syntax.
pub(super) enum PrettyFixedRendering {
    /// Joins directly to neighboring tight syntax.
    TightSequence,
    /// Requires spacing before the token.
    Before,
    /// Requires spacing after the token.
    After,
    /// Requires spacing on both sides.
    Around,
    /// Opens a delimited group without trailing space.
    Opening,
    /// Behaves as word-like syntax for adjacency.
    Word,
    /// Behaves as punctuation for adjacency.
    Punctuation,
}

/// Projects configured rendering behavior from the frozen parse and token plans.
pub(super) fn emitted_pretty_plan(
    nodes: &[NodeParsePlan],
    tokens: &[TokenParsePlan],
) -> PrettyEmissionPlan {
    let grouping_syntax = emitted_pretty_grouping_syntax(nodes);
    PrettyEmissionPlan {
        tokens: tokens
            .iter()
            .map(|token| {
                token
                    .configured_traits
                    .contains(&ConfiguredTraitRecipe::Pretty)
                    .then_some(PrettyTokenBehavior::MatchedOrStoredText)
            })
            .collect(),
        nodes: nodes
            .iter()
            .map(|node| {
                node.configured_traits
                    .contains(&ConfiguredTraitRecipe::Pretty)
                    .then(|| emitted_pretty_node_behavior(node, &grouping_syntax))
            })
            .collect(),
    }
}

/// Projects frozen parse plans into the compact syntax graph used for grouping proofs.
fn emitted_pretty_grouping_syntax(nodes: &[NodeParsePlan]) -> PrettyPrattGroupingSyntax {
    let nodes = nodes
        .iter()
        .filter_map(|node| {
            let variants = match &node.body {
                NodeBodyPlan::Struct { elements, .. } => vec![elements.as_slice()],
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter()
                    .map(|variant| variant.elements.as_slice())
                    .collect(),
                NodeBodyPlan::Pratt(_) => return None,
            };
            let variants = variants
                .into_iter()
                .map(|elements| {
                    let behaviors = emitted_pretty_elements(elements);
                    elements
                        .iter()
                        .zip(&behaviors)
                        .map(|(element, behavior)| {
                            emitted_pretty_grouping_element(element, behavior)
                        })
                        .collect()
                })
                .collect();
            Some((node.node, variants))
        })
        .collect();
    PrettyPrattGroupingSyntax { nodes }
}

/// Projects one frozen parse element into grouping-proof syntax.
fn emitted_pretty_grouping_element(
    element: &ElementParsePlan,
    behavior: &PrettyElementBehavior,
) -> PrettyPrattGroupingElement {
    match (&element.kind, behavior) {
        (
            ElementParseKind::Fixed { .. },
            PrettyElementBehavior::Fixed {
                spelling,
                spacing,
                rendering,
                ..
            },
        ) => PrettyPrattGroupingElement::Fixed(PrettyAttachedFixedBehavior {
            spelling: spelling.clone(),
            spacing: *spacing,
            rendering: *rendering,
        }),
        (
            ElementParseKind::OptionalFixedTokenGroup { .. },
            PrettyElementBehavior::OptionalFixedTokenGroup { .. },
        ) => PrettyPrattGroupingElement::Optional,
        (
            ElementParseKind::Semantic { projection, .. },
            PrettyElementBehavior::Semantic { projection: pretty },
        ) => match (projection.as_ref(), pretty) {
            (
                SemanticParseProjection::Direct {
                    terminal_node,
                    wrappers,
                    ..
                },
                PrettySemanticProjection::Direct { attachment, .. },
            ) => PrettyPrattGroupingElement::Semantic {
                terminal_node: *terminal_node,
                wrappers: wrappers.clone(),
                before: attachment.as_ref().map_or(Some(Vec::new()), |attachment| {
                    required_pretty_syntax(&attachment.before)
                }),
                after: attachment.as_ref().map_or(Some(Vec::new()), |attachment| {
                    required_pretty_syntax(&attachment.after)
                }),
                attachment_can_be_empty: attachment.as_ref().is_none_or(|attachment| {
                    attachment
                        .before
                        .iter()
                        .chain(&attachment.after)
                        .all(|syntax| matches!(syntax, PrettyAttachedSyntaxBehavior::Optional(_)))
                }),
            },
            (SemanticParseProjection::FixedPresence { .. }, _)
            | (_, PrettySemanticProjection::FixedPresence { .. }) => {
                PrettyPrattGroupingElement::Opaque
            }
        },
        _ => PrettyPrattGroupingElement::Opaque,
    }
}

/// Builds rendering behavior for one Node and all materialized applications.
pub(super) fn emitted_pretty_node_behavior(
    node: &NodeParsePlan,
    grouping_syntax: &PrettyPrattGroupingSyntax,
) -> PrettyNodeBehavior {
    let variant_pretty = match &node.body {
        NodeBodyPlan::Struct { .. } => vec![PrettyHints::default()],
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
        NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
    };
    let mut variants = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => vec![emitted_pretty_elements(elements)],
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .map(|variant| emitted_pretty_elements(&variant.elements))
            .collect(),
        NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .iter()
            .map(|variant| emitted_pretty_elements(&variant.elements))
            .collect(),
    };
    let variant_layouts = variants
        .iter()
        .map(|elements| pretty_sequence_layout(elements))
        .collect();
    let applications = node
        .parse_applications
        .iter()
        .map(|application| application.application)
        .collect::<Vec<_>>();
    let pratt = match &node.body {
        NodeBodyPlan::Pratt(pratt) => Some(emitted_pretty_pratt_behavior(
            node,
            pratt,
            &applications,
            &mut variants,
            grouping_syntax,
        )),
        NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
    };
    PrettyNodeBehavior {
        node: node.node,
        applications,
        pretty: node.pretty.clone(),
        variant_pretty,
        variant_layouts,
        variants,
        pratt,
    }
}

/// Derives Pratt grouping, binding-power, and recursive-operand behavior.
pub(super) fn emitted_pretty_pratt_behavior(
    node: &NodeParsePlan,
    pratt: &PrattParsePlan,
    applications: &[MaterializedApplicationId],
    variants: &mut [Vec<PrettyElementBehavior>],
    grouping_syntax: &PrettyPrattGroupingSyntax,
) -> PrettyPrattBehavior {
    let declaration_variants = trait_variants(&node.body);
    let mut positions = vec![None; declaration_variants.len()];
    for branch in &pratt.nud {
        positions[branch.source_index] = Some(branch.position);
    }
    for branch in &pratt.led {
        positions[branch.source_index] = Some(branch.position);
    }
    let variant_behaviors = declaration_variants
        .iter()
        .zip(variants.iter_mut())
        .zip(positions.iter().copied())
        .map(|((variant, elements), position)| {
            let position = position.expect("every Pratt source variant has a typed position");
            apply_pretty_pratt_fixed_layout(position, &variant.elements, elements);
            let mut semantic_index = 0;
            let operands = variant
                .elements
                .iter()
                .filter_map(|element| {
                    let ElementParseKind::Semantic { projection, .. } = &element.kind else {
                        return None;
                    };
                    let SemanticParseProjection::Direct { terminal_node, .. } = projection.as_ref()
                    else {
                        return None;
                    };
                    let current = semantic_index;
                    semantic_index += 1;
                    Some(if *terminal_node == Some(node.node) {
                        pretty_recursive_operand(node.node, applications, position, current)
                    } else {
                        PrettyPrattOperandBehavior::Ordinary
                    })
                })
                .collect();
            PrettyPrattVariantBehavior {
                parenthesization: pretty_pratt_parenthesization(position),
                operands,
            }
        })
        .collect();
    PrettyPrattBehavior {
        grouping: pretty_pratt_grouping(
            node.node,
            &declaration_variants,
            variants,
            &positions,
            grouping_syntax,
        ),
        variants: variant_behaviors,
    }
}

/// Converts a Pratt branch's entry and exit powers into a parenthesization contract.
pub(super) fn pretty_pratt_parenthesization(
    position: PrattPosition,
) -> PrettyPrattParenthesization {
    let (entry_lbp, exit_rbp) = match position {
        PrattPosition::Atom => (None, 256),
        PrattPosition::Prefix { bp } => (None, u16::from(bp)),
        PrattPosition::Infix { lbp, rbp } => (Some(u16::from(lbp)), u16::from(rbp)),
        PrattPosition::Postfix { bp } => (Some(u16::from(bp)), 256),
    };
    PrettyPrattParenthesization {
        entry_lbp,
        exit_rbp,
    }
}

/// Resolves a recursive Pratt operand and its binding-power context.
pub(super) fn pretty_recursive_operand(
    owner: NodeId,
    applications: &[MaterializedApplicationId],
    position: PrattPosition,
    semantic_index: usize,
) -> PrettyPrattOperandBehavior {
    let (minimum, next) = match position {
        PrattPosition::Atom => (PrettyPrattMinimum::Zero, PrettyPrattNext::None),
        PrattPosition::Prefix { bp } => (
            PrettyPrattMinimum::Binding(u16::from(bp)),
            PrettyPrattNext::Inner,
        ),
        PrattPosition::Postfix { bp } => (
            PrettyPrattMinimum::Inner,
            PrettyPrattNext::Binding(u16::from(bp)),
        ),
        PrattPosition::Infix { lbp, .. } if semantic_index == 0 => (
            PrettyPrattMinimum::Inner,
            PrettyPrattNext::Binding(u16::from(lbp)),
        ),
        PrattPosition::Infix { rbp, .. } => (
            PrettyPrattMinimum::Binding(u16::from(rbp)),
            PrettyPrattNext::Inner,
        ),
    };
    PrettyPrattOperandBehavior::Recursive {
        owner,
        applications: applications.to_vec(),
        minimum,
        next,
    }
}

/// Applies fixed-token spacing and break hints to a Pratt variant element.
pub(super) fn apply_pretty_pratt_fixed_layout(
    position: PrattPosition,
    elements: &[ElementParsePlan],
    behaviors: &mut [PrettyElementBehavior],
) {
    for (index, behavior) in behaviors.iter_mut().enumerate() {
        let PrettyElementBehavior::Fixed {
            spacing,
            rendering,
            soft_break_before,
            soft_break_after,
            ..
        } = behavior
        else {
            continue;
        };
        let run_start =
            index == 0 || matches!(elements[index - 1].kind, ElementParseKind::Semantic { .. });
        let run_end = index + 1 == elements.len()
            || matches!(elements[index + 1].kind, ElementParseKind::Semantic { .. });
        if matches!(position, PrattPosition::Prefix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto
            && *rendering == PrettyFixedRendering::Punctuation
        {
            *rendering = PrettyFixedRendering::Opening;
        }
        *soft_break_before = matches!(position, PrattPosition::Infix { .. })
            && run_start
            && *spacing == PrettySpacing::Auto;
        *soft_break_after = matches!(position, PrattPosition::Infix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto;
    }
}

/// Extracts the explicit grouping variant's before/after delimiter behavior.
pub(super) fn pretty_pratt_grouping(
    owner: NodeId,
    declaration_variants: &[VariantParsePlan],
    behaviors: &[Vec<PrettyElementBehavior>],
    positions: &[Option<PrattPosition>],
    grouping_syntax: &PrettyPrattGroupingSyntax,
) -> Option<PrettyPrattGrouping> {
    let mut analyzer = PrettyPrattGroupingAnalyzer::new(grouping_syntax);
    let candidates = declaration_variants
        .iter()
        .zip(behaviors)
        .zip(positions)
        .filter(|(_, position)| matches!(position, Some(PrattPosition::Atom)))
        .map(|((variant, behavior), _)| {
            variant
                .elements
                .iter()
                .zip(behavior)
                .map(|(element, behavior)| emitted_pretty_grouping_element(element, behavior))
                .collect::<Vec<_>>()
        })
        .filter_map(|elements| analyzer.grouping(owner, &elements))
        .collect::<Vec<_>>();
    if let Some(direct) = candidates.iter().find(|candidate| !candidate.delegated) {
        return Some(direct.grouping.clone());
    }
    let [delegated] = candidates.as_slice() else {
        return None;
    };
    Some(delegated.grouping.clone())
}

/// Projects source-order parse elements into fixed and semantic rendering behavior.
pub(super) fn emitted_pretty_elements(elements: &[ElementParsePlan]) -> Vec<PrettyElementBehavior> {
    elements
        .iter()
        .map(|element| match &element.kind {
            ElementParseKind::Fixed {
                spelling, spacing, ..
            } => PrettyElementBehavior::Fixed {
                spelling: spelling.clone(),
                spacing: *spacing,
                rendering: pretty_fixed_rendering(spelling, *spacing),
                soft_break_before: false,
                soft_break_after: false,
            },
            ElementParseKind::OptionalFixedTokenGroup { tokens, .. } => {
                PrettyElementBehavior::OptionalFixedTokenGroup {
                    tokens: pretty_presence_tokens(tokens),
                }
            }
            ElementParseKind::Semantic { projection, .. } => PrettyElementBehavior::Semantic {
                projection: match projection.as_ref() {
                    SemanticParseProjection::FixedPresence {
                        before,
                        sequence,
                        after,
                        pretty,
                    } => PrettySemanticProjection::FixedPresence {
                        before: pretty_attachment_syntax(before),
                        sequence: pretty_presence_tokens(sequence),
                        after: pretty_attachment_syntax(after),
                        pretty: pretty.clone(),
                    },
                    SemanticParseProjection::Direct {
                        container,
                        wrappers,
                        attachment,
                        separator,
                        opaque_provenance,
                        repetition_break,
                        pretty,
                        ..
                    } => PrettySemanticProjection::Direct {
                        repetition: pretty_repetition_behavior(
                            matches!(container.as_ref(), PlannedContainer::Repeated { .. }),
                            separator.clone(),
                            matches!(
                                repetition_break,
                                PlannedRepetitionBreak::TopLevelStatement(_)
                                    | PlannedRepetitionBreak::TopLevelStatementFamily(_)
                            ),
                            pretty,
                        ),
                        attachment: attachment.as_ref().map(|attachment| {
                            let before = attachment
                                .before
                                .iter()
                                .map(pretty_attachment_part)
                                .collect::<Vec<_>>();
                            let after = attachment
                                .after
                                .iter()
                                .map(pretty_attachment_part)
                                .collect::<Vec<_>>();
                            PrettyAttachmentBehavior {
                                layout: pretty_attachment_layout(&before, &after),
                                before,
                                after,
                            }
                        }),
                        provenance: if *opaque_provenance {
                            PrettyProvenanceBehavior::Opaque
                        } else {
                            PrettyProvenanceBehavior::Structured
                        },
                        pretty: pretty.clone(),
                        presence: pretty_presence(wrappers),
                    },
                },
            },
        })
        .collect()
}

/// Converts parsed fixed-token plans into their canonical Pretty behavior.
fn pretty_presence_tokens(tokens: &[AttachedFixedParsePlan]) -> Vec<PrettyAttachedFixedBehavior> {
    tokens
        .iter()
        .map(|fixed| PrettyAttachedFixedBehavior {
            spelling: fixed.spelling.clone(),
            spacing: fixed.spacing,
            rendering: pretty_fixed_rendering(&fixed.spelling, fixed.spacing),
        })
        .collect()
}

/// Converts one parsed attachment entry into its Pretty behavior.
fn pretty_attachment_part(syntax: &AttachedFixedSyntaxParsePlan) -> PrettyAttachedSyntaxBehavior {
    match syntax {
        AttachedFixedSyntaxParsePlan::Required(fixed) => {
            PrettyAttachedSyntaxBehavior::Required(pretty_presence_token(fixed))
        }
        AttachedFixedSyntaxParsePlan::Optional(tokens) => {
            PrettyAttachedSyntaxBehavior::Optional(pretty_presence_tokens(tokens))
        }
    }
}

/// Converts an attachment sequence into source-ordered Pretty behavior.
fn pretty_attachment_syntax(
    syntax: &[AttachedFixedSyntaxParsePlan],
) -> Vec<PrettyAttachedSyntaxBehavior> {
    syntax.iter().map(pretty_attachment_part).collect()
}

/// Converts one parsed fixed token into canonical Pretty behavior.
fn pretty_presence_token(fixed: &AttachedFixedParsePlan) -> PrettyAttachedFixedBehavior {
    PrettyAttachedFixedBehavior {
        spelling: fixed.spelling.clone(),
        spacing: fixed.spacing,
        rendering: pretty_fixed_rendering(&fixed.spelling, fixed.spacing),
    }
}

/// Derives separator placement and break policy for one repetition.
pub(super) fn pretty_repetition_behavior(
    repeated: bool,
    separator: Option<String>,
    hard_break: bool,
    pretty: &PrettyHints,
) -> Option<PrettyRepetitionBehavior> {
    repeated.then(|| PrettyRepetitionBehavior {
        space_before_separator: separator.as_deref().is_some_and(is_word_shaped),
        separator,
        trailing_separator: matches!(
            pretty.trailing_separator,
            Some(PrettyTrailingSeparator::Always)
        ),
        hard_break,
        leading_separator: matches!(pretty.separator, Some(PrettySeparatorPosition::Leading)),
    })
}

/// Selects fixed-token placement from its sequence position and local hints.
pub(super) fn pretty_fixed_rendering(
    spelling: &str,
    spacing: PrettySpacing,
) -> PrettyFixedRendering {
    match spacing {
        PrettySpacing::Tight => PrettyFixedRendering::TightSequence,
        PrettySpacing::Before => PrettyFixedRendering::Before,
        PrettySpacing::After => PrettyFixedRendering::After,
        PrettySpacing::Around => PrettyFixedRendering::Around,
        PrettySpacing::Auto if matches!(spelling, "(" | "[" | "{") => PrettyFixedRendering::Opening,
        PrettySpacing::Auto if is_word_shaped(spelling) => PrettyFixedRendering::Word,
        PrettySpacing::Auto => PrettyFixedRendering::Punctuation,
    }
}

/// Chooses linear or delimited grouping for one Node or variant shape.
pub(super) fn pretty_sequence_layout(elements: &[PrettyElementBehavior]) -> PrettySequenceLayout {
    let delimited = elements.first().and_then(|first| {
        let PrettyElementBehavior::Fixed { spelling, .. } = first else {
            return None;
        };
        delimiter_close(spelling).filter(|close| {
            elements.last().is_some_and(|last| {
                matches!(last, PrettyElementBehavior::Fixed { spelling, .. } if spelling == close)
            })
        })
    });
    if delimited.is_some() {
        PrettySequenceLayout::Delimited
    } else {
        PrettySequenceLayout::Linear
    }
}

/// Chooses grouping for fixed syntax attached around a semantic value.
pub(super) fn pretty_attachment_layout(
    before: &[PrettyAttachedSyntaxBehavior],
    after: &[PrettyAttachedSyntaxBehavior],
) -> PrettySequenceLayout {
    let delimited = before
        .first()
        .and_then(|open| match open {
            PrettyAttachedSyntaxBehavior::Required(open) => delimiter_close(&open.spelling),
            PrettyAttachedSyntaxBehavior::Optional(_) => None,
        })
        .is_some_and(|close| {
            after.last().is_some_and(|fixed| {
                matches!(fixed, PrettyAttachedSyntaxBehavior::Required(fixed) if fixed.spelling == close)
            })
        });
    if delimited {
        PrettySequenceLayout::Delimited
    } else {
        PrettySequenceLayout::Linear
    }
}

/// Maps a semantic container shape to its Pretty emission condition.
pub(super) fn pretty_presence(wrappers: &[FieldWrapper]) -> PrettyPresence {
    if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    {
        PrettyPresence::Optional
    } else if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore))
    {
        PrettyPresence::NonEmpty
    } else {
        PrettyPresence::Always
    }
}

/// Projects authoritative source correlations into the compact grouping syntax graph.
fn authoritative_pretty_grouping_syntax(
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<PrettyPrattGroupingSyntax, String> {
    let source = lowered.source_modeled.source_model();
    let resolved = lowered.source_modeled.resolved();
    let mut nodes = BTreeMap::new();
    for source_node in &source.nodes {
        let resolved_node = &resolved.nodes[source_node.resolved_node.index()];
        let variants = match &source_node.body {
            SourceNodeBody::Struct(elements) | SourceNodeBody::Presence(elements) => {
                vec![elements.as_slice()]
            }
            SourceNodeBody::Enum(variants) => variants
                .iter()
                .map(|variant| variant.elements.as_slice())
                .collect(),
            SourceNodeBody::Pratt(_) => continue,
        };
        let variants = variants
            .into_iter()
            .map(|elements| {
                let behaviors = authoritative_pretty_elements(lowered, resolved_node, elements)?;
                elements
                    .iter()
                    .zip(&behaviors)
                    .map(|(element, behavior)| {
                        authoritative_pretty_grouping_element(
                            resolved_node,
                            element,
                            behavior,
                            resolved,
                        )
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .collect::<Result<Vec<_>, String>>()?;
        nodes.insert(source_node.resolved_node, variants);
    }
    Ok(PrettyPrattGroupingSyntax { nodes })
}

/// Projects one authoritative source element into grouping-proof syntax.
fn authoritative_pretty_grouping_element(
    resolved_node: &ResolvedNode,
    element: &GrammarElement,
    behavior: &PrettyElementBehavior,
    resolved: &ResolvedSourceGrammar,
) -> Result<PrettyPrattGroupingElement, String> {
    match (element, behavior) {
        (
            GrammarElement::FixedToken { .. },
            PrettyElementBehavior::Fixed {
                spelling,
                spacing,
                rendering,
                ..
            },
        ) => Ok(PrettyPrattGroupingElement::Fixed(
            PrettyAttachedFixedBehavior {
                spelling: spelling.clone(),
                spacing: *spacing,
                rendering: *rendering,
            },
        )),
        (
            GrammarElement::OptionalFixedTokenGroup { .. },
            PrettyElementBehavior::OptionalFixedTokenGroup { .. },
        ) => Ok(PrettyPrattGroupingElement::Optional),
        (
            GrammarElement::SemanticField {
                resolved_field,
                projection,
                ..
            },
            PrettyElementBehavior::Semantic { projection: pretty },
        ) => match (projection, pretty) {
            (
                crate::source_model::SemanticFieldProjection::Direct,
                PrettySemanticProjection::Direct { attachment, .. },
            ) => {
                let field = resolved_node
                    .fields
                    .iter()
                    .find(|field| field.id == *resolved_field)
                    .ok_or_else(|| "a Pretty grouping field lacks resolved syntax".to_owned())?;
                let projection = authored_projection(&field.ty, resolved)?;
                Ok(PrettyPrattGroupingElement::Semantic {
                    terminal_node: projection.terminal_node,
                    wrappers: projection.wrappers,
                    before: attachment.as_ref().map_or(Some(Vec::new()), |attachment| {
                        required_pretty_syntax(&attachment.before)
                    }),
                    after: attachment.as_ref().map_or(Some(Vec::new()), |attachment| {
                        required_pretty_syntax(&attachment.after)
                    }),
                    attachment_can_be_empty: attachment.as_ref().is_none_or(|attachment| {
                        attachment
                            .before
                            .iter()
                            .chain(&attachment.after)
                            .all(|syntax| {
                                matches!(syntax, PrettyAttachedSyntaxBehavior::Optional(_))
                            })
                    }),
                })
            }
            (crate::source_model::SemanticFieldProjection::FixedPresence { .. }, _)
            | (_, PrettySemanticProjection::FixedPresence { .. }) => {
                Ok(PrettyPrattGroupingElement::Opaque)
            }
        },
        _ => Ok(PrettyPrattGroupingElement::Opaque),
    }
}

/// Reconstructs the Pretty plan from authored source for independent validation.
pub(super) fn authoritative_pretty_plan(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
) -> Result<Option<PrettyEmissionPlan>, String> {
    let source = lowered.source_modeled.source_model();
    let grouping_syntax = authoritative_pretty_grouping_syntax(lowered)?;
    let tokens = resolved
        .content_tokens
        .iter()
        .map(|content| {
            source
                .token_definitions
                .iter()
                .find_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(token) if token.semantic == content.id => {
                        Some(token)
                    }
                    TokenDefinitionKind::Content(_)
                    | TokenDefinitionKind::Fixed(_)
                    | TokenDefinitionKind::LexicalBase(_) => None,
                })
                .is_some_and(|token| {
                    token
                        .configured_traits
                        .effective
                        .iter()
                        .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty)
                })
                .then_some(PrettyTokenBehavior::MatchedOrStoredText)
        })
        .collect::<Vec<_>>();
    let nodes = resolved
        .nodes
        .iter()
        .map(|resolved_node| {
            let source_node = source
                .nodes
                .iter()
                .find(|node| node.resolved_node == resolved_node.id)
                .ok_or_else(|| {
                    format!(
                        "Node `{}` lacks an authoritative Pretty plan",
                        resolved_node.ident
                    )
                })?;
            if !source_node
                .configured_traits
                .effective
                .iter()
                .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty)
            {
                return Ok(None);
            }
            authoritative_pretty_node_behavior(
                lowered,
                resolved_node,
                source_node,
                &grouping_syntax,
            )
            .map(Some)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let plan = PrettyEmissionPlan { tokens, nodes };
    Ok(pretty_family_identity(&plan).map(|_| plan))
}

/// Reconstructs one Node's rendering behavior without consulting emitted Pretty data.
pub(super) fn authoritative_pretty_node_behavior(
    lowered: &crate::materialization::LoweredGrammar,
    resolved_node: &ResolvedNode,
    source_node: &SourceNode,
    grouping_syntax: &PrettyPrattGroupingSyntax,
) -> Result<PrettyNodeBehavior, String> {
    let variant_pretty = match &source_node.body {
        SourceNodeBody::Struct(_) | SourceNodeBody::Presence(_) => vec![PrettyHints::default()],
        SourceNodeBody::Enum(variants) => variants
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
        SourceNodeBody::Pratt(variants) => variants
            .iter()
            .map(|variant| variant.common.pretty.clone())
            .collect(),
    };
    let mut variants = match &source_node.body {
        SourceNodeBody::Struct(elements) | SourceNodeBody::Presence(elements) => {
            vec![authoritative_pretty_elements(
                lowered,
                resolved_node,
                elements,
            )?]
        }
        SourceNodeBody::Enum(variants) => variants
            .iter()
            .map(|variant| authoritative_pretty_elements(lowered, resolved_node, &variant.elements))
            .collect::<Result<Vec<_>, String>>()?,
        SourceNodeBody::Pratt(variants) => variants
            .iter()
            .map(|variant| {
                authoritative_pretty_elements(lowered, resolved_node, &variant.common.elements)
            })
            .collect::<Result<Vec<_>, String>>()?,
    };
    let variant_layouts = variants
        .iter()
        .map(|elements| pretty_sequence_layout(elements))
        .collect();
    let applications = lowered
        .applications
        .iter()
        .filter(|application| application.node == resolved_node.id)
        .map(|application| application.id)
        .collect::<Vec<_>>();
    let pratt = match &source_node.body {
        SourceNodeBody::Pratt(source_variants) => Some(authoritative_pretty_pratt_behavior(
            lowered,
            resolved_node,
            source_variants,
            &applications,
            &mut variants,
            grouping_syntax,
        )?),
        SourceNodeBody::Struct(_) | SourceNodeBody::Presence(_) | SourceNodeBody::Enum(_) => None,
    };
    Ok(PrettyNodeBehavior {
        node: resolved_node.id,
        applications,
        pretty: source_node.pretty.clone(),
        variant_pretty,
        variant_layouts,
        variants,
        pratt,
    })
}

/// Reconstructs Pratt rendering semantics from authoritative source correlations.
pub(super) fn authoritative_pretty_pratt_behavior(
    lowered: &crate::materialization::LoweredGrammar,
    resolved_node: &ResolvedNode,
    source_variants: &[crate::source_model::SourcePrattVariant],
    applications: &[MaterializedApplicationId],
    variants: &mut [Vec<PrettyElementBehavior>],
    grouping_syntax: &PrettyPrattGroupingSyntax,
) -> Result<PrettyPrattBehavior, String> {
    let resolved = lowered.source_modeled.resolved();
    let positions = source_variants
        .iter()
        .map(|variant| match variant.position {
            SourcePrattPosition::Atom => PrattPosition::Atom,
            SourcePrattPosition::Prefix { bp } => PrattPosition::Prefix { bp },
            SourcePrattPosition::Infix { lbp, rbp } => PrattPosition::Infix { lbp, rbp },
            SourcePrattPosition::Postfix { bp } => PrattPosition::Postfix { bp },
        })
        .collect::<Vec<_>>();
    let mut planned = Vec::with_capacity(source_variants.len());
    for ((variant, behavior), position) in source_variants
        .iter()
        .zip(variants.iter_mut())
        .zip(positions.iter().copied())
    {
        apply_authoritative_pretty_pratt_fixed_layout(position, &variant.common.elements, behavior);
        let mut semantic_index = 0;
        let mut operands = Vec::new();
        for element in &variant.common.elements {
            let GrammarElement::SemanticField { resolved_field, .. } = element else {
                continue;
            };
            let field = resolved_node
                .fields
                .iter()
                .find(|field| field.id == *resolved_field)
                .ok_or_else(|| "a Pratt Pretty operand lacks resolved syntax".to_owned())?;
            let projection = authored_projection(&field.ty, resolved)?;
            operands.push(if projection.terminal_node == Some(resolved_node.id) {
                pretty_recursive_operand(resolved_node.id, applications, position, semantic_index)
            } else {
                PrettyPrattOperandBehavior::Ordinary
            });
            semantic_index += 1;
        }
        planned.push(PrettyPrattVariantBehavior {
            parenthesization: pretty_pratt_parenthesization(position),
            operands,
        });
    }
    Ok(PrettyPrattBehavior {
        grouping: authoritative_pretty_pratt_grouping(
            resolved_node,
            source_variants,
            variants,
            &positions,
            resolved,
            grouping_syntax,
        )?,
        variants: planned,
    })
}

/// Applies authored fixed-token hints to an authoritative Pratt element plan.
pub(super) fn apply_authoritative_pretty_pratt_fixed_layout(
    position: PrattPosition,
    elements: &[GrammarElement],
    behaviors: &mut [PrettyElementBehavior],
) {
    for (index, behavior) in behaviors.iter_mut().enumerate() {
        let PrettyElementBehavior::Fixed {
            spacing,
            rendering,
            soft_break_before,
            soft_break_after,
            ..
        } = behavior
        else {
            continue;
        };
        let semantic =
            |element: &GrammarElement| matches!(element, GrammarElement::SemanticField { .. });
        let run_start = index == 0 || semantic(&elements[index - 1]);
        let run_end = index + 1 == elements.len() || semantic(&elements[index + 1]);
        if matches!(position, PrattPosition::Prefix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto
            && *rendering == PrettyFixedRendering::Punctuation
        {
            *rendering = PrettyFixedRendering::Opening;
        }
        *soft_break_before = matches!(position, PrattPosition::Infix { .. })
            && run_start
            && *spacing == PrettySpacing::Auto;
        *soft_break_after = matches!(position, PrattPosition::Infix { .. })
            && run_end
            && *spacing == PrettySpacing::Auto;
    }
}

/// Reconstructs explicit Pratt grouping delimiters from authored syntax.
pub(super) fn authoritative_pretty_pratt_grouping(
    resolved_node: &ResolvedNode,
    source_variants: &[crate::source_model::SourcePrattVariant],
    behaviors: &[Vec<PrettyElementBehavior>],
    positions: &[PrattPosition],
    resolved: &ResolvedSourceGrammar,
    grouping_syntax: &PrettyPrattGroupingSyntax,
) -> Result<Option<PrettyPrattGrouping>, String> {
    let mut analyzer = PrettyPrattGroupingAnalyzer::new(grouping_syntax);
    let mut candidates = Vec::new();
    for ((variant, behavior), position) in source_variants.iter().zip(behaviors).zip(positions) {
        if !matches!(position, PrattPosition::Atom) {
            continue;
        }
        let elements = variant
            .common
            .elements
            .iter()
            .zip(behavior)
            .map(|(element, behavior)| {
                authoritative_pretty_grouping_element(resolved_node, element, behavior, resolved)
            })
            .collect::<Result<Vec<_>, String>>()?;
        if let Some(grouping) = analyzer.grouping(resolved_node.id, &elements) {
            candidates.push(grouping);
        }
    }
    if let Some(direct) = candidates.iter().find(|candidate| !candidate.delegated) {
        return Ok(Some(direct.grouping.clone()));
    }
    let [delegated] = candidates.as_slice() else {
        return Ok(None);
    };
    Ok(Some(delegated.grouping.clone()))
}

/// Reconstructs ordered element rendering behavior from authored Node shapes.
pub(super) fn authoritative_pretty_elements(
    lowered: &crate::materialization::LoweredGrammar,
    resolved_node: &ResolvedNode,
    elements: &[GrammarElement],
) -> Result<Vec<PrettyElementBehavior>, String> {
    let source = lowered.source_modeled.source_model();
    let resolved = lowered.source_modeled.resolved();
    elements
        .iter()
        .map(|element| match element {
            GrammarElement::FixedToken { token_use, .. } => {
                let usage = &source.token_uses[token_use.index()];
                let definition = &source.token_definitions[usage.definition.index()];
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return Err("an authored Pretty fixed element is not fixed syntax".to_owned());
                };
                let spelling =
                    fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence);
                Ok(PrettyElementBehavior::Fixed {
                    rendering: pretty_fixed_rendering(&spelling, fixed.pretty_spacing),
                    spelling,
                    spacing: fixed.pretty_spacing,
                    soft_break_before: false,
                    soft_break_after: false,
                })
            }
            GrammarElement::OptionalFixedTokenGroup { tokens, .. } => {
                let tokens = tokens
                    .iter()
                    .map(|token| {
                        let usage = &source.token_uses[token.token_use.index()];
                        let definition = &source.token_definitions[usage.definition.index()];
                        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                            return Err("an optional group token is not fixed syntax".to_owned());
                        };
                        let spelling = fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        );
                        Ok(PrettyAttachedFixedBehavior {
                            rendering: pretty_fixed_rendering(&spelling, fixed.pretty_spacing),
                            spelling,
                            spacing: fixed.pretty_spacing,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(PrettyElementBehavior::OptionalFixedTokenGroup { tokens })
            }
            GrammarElement::SemanticField {
                projection: crate::source_model::SemanticFieldProjection::FixedPresence { tokens },
                attachment,
                pretty,
                ..
            } => {
                let fixed = |token: &crate::source_model::AttachedFixedToken| {
                    let usage = &source.token_uses[token.token_use.index()];
                    let definition = &source.token_definitions[usage.definition.index()];
                    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                        return Err("an authored presence token is not fixed syntax".to_owned());
                    };
                    let spelling =
                        fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence);
                    Ok(PrettyAttachedFixedBehavior {
                        rendering: pretty_fixed_rendering(&spelling, fixed.pretty_spacing),
                        spelling,
                        spacing: fixed.pretty_spacing,
                    })
                };
                let syntax = |syntax: &crate::source_model::AttachedFixedSyntax| match syntax {
                    crate::source_model::AttachedFixedSyntax::Required(token) => {
                        fixed(token).map(PrettyAttachedSyntaxBehavior::Required)
                    }
                    crate::source_model::AttachedFixedSyntax::Optional { tokens, .. } => tokens
                        .iter()
                        .map(fixed)
                        .collect::<Result<Vec<_>, _>>()
                        .map(PrettyAttachedSyntaxBehavior::Optional),
                };
                let before = attachment.as_ref().map_or_else(
                    || Ok(Vec::new()),
                    |attachment| attachment.before.iter().map(syntax).collect(),
                )?;
                let after = attachment.as_ref().map_or_else(
                    || Ok(Vec::new()),
                    |attachment| attachment.after.iter().map(syntax).collect(),
                )?;
                Ok(PrettyElementBehavior::Semantic {
                    projection: PrettySemanticProjection::FixedPresence {
                        before,
                        sequence: tokens.iter().map(fixed).collect::<Result<_, _>>()?,
                        after,
                        pretty: pretty.clone(),
                    },
                })
            }
            GrammarElement::SemanticField {
                resolved_field,
                projection: crate::source_model::SemanticFieldProjection::Direct,
                attachment,
                separator,
                pretty,
                ..
            } => {
                let field = resolved_node
                    .fields
                    .iter()
                    .find(|field| field.id == *resolved_field)
                    .ok_or_else(|| "an authored Pretty field lacks resolved syntax".to_owned())?;
                let projection = authored_projection(&field.ty, resolved)?;
                let repeated = matches!(
                    declaration_container_operation(&projection.wrappers),
                    ContainerOperation::ZeroOrMore
                        | ContainerOperation::OneOrMore
                        | ContainerOperation::OptionalOneOrMore
                );
                let separator_spelling =
                    authoritative_pretty_separator(lowered, separator.as_ref())?;
                let top_level_break = separator.as_ref().is_some_and(|separator| {
                    let usage = &source.token_uses[separator.token_use.index()];
                    let definition = &source.token_definitions[usage.definition.index()];
                    matches!(
                        &definition.kind,
                        TokenDefinitionKind::Fixed(fixed) if fixed.spelling == ";"
                    )
                }) && repeated
                    && projection.terminal_node.is_some_and(|node_id| {
                        matches!(
                            resolved.nodes[node_id.index()].body,
                            ResolvedNodeBody::Enum { .. }
                        )
                    });
                Ok(PrettyElementBehavior::Semantic {
                    projection: PrettySemanticProjection::Direct {
                        repetition: pretty_repetition_behavior(
                            repeated,
                            separator_spelling,
                            top_level_break,
                            pretty,
                        ),
                        attachment: attachment
                            .as_ref()
                            .map(|attachment| authoritative_pretty_attachment(lowered, attachment))
                            .transpose()?,
                        provenance: if authored_type_depends_on_node_generics(
                            &field.ty,
                            resolved_node,
                            resolved,
                        ) {
                            PrettyProvenanceBehavior::Opaque
                        } else {
                            PrettyProvenanceBehavior::Structured
                        },
                        pretty: pretty.clone(),
                        presence: pretty_presence(&projection.wrappers),
                    },
                })
            }
        })
        .collect()
}

/// Resolves an authored repetition separator to its canonical spelling and spacing.
pub(super) fn authoritative_pretty_separator(
    lowered: &crate::materialization::LoweredGrammar,
    separator: Option<&crate::source_model::Separator>,
) -> Result<Option<String>, String> {
    let Some(separator) = separator else {
        return Ok(None);
    };
    let source = lowered.source_modeled.source_model();
    let usage = &source.token_uses[separator.token_use.index()];
    let definition = &source.token_definitions[usage.definition.index()];
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("a Pretty separator is not fixed syntax".to_owned());
    };
    Ok(Some(fixed_pretty_spelling(
        &fixed.spelling,
        lowered.grammar.text_equivalence,
    )))
}

/// Reconstructs fixed syntax attached around an authored semantic field.
pub(super) fn authoritative_pretty_attachment(
    lowered: &crate::materialization::LoweredGrammar,
    attachment: &crate::source_model::FixedTokenAttachment,
) -> Result<PrettyAttachmentBehavior, String> {
    let source = lowered.source_modeled.source_model();
    let fixed = |token_use: crate::lowering::TokenUseId| {
        let usage = &source.token_uses[token_use.index()];
        let definition = &source.token_definitions[usage.definition.index()];
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            return Err("an attached Pretty element is not fixed syntax".to_owned());
        };
        let spelling = fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence);
        Ok(PrettyAttachedFixedBehavior {
            rendering: pretty_fixed_rendering(&spelling, fixed.pretty_spacing),
            spelling,
            spacing: fixed.pretty_spacing,
        })
    };
    let syntax = |syntax: &crate::source_model::AttachedFixedSyntax| match syntax {
        crate::source_model::AttachedFixedSyntax::Required(token) => {
            fixed(token.token_use).map(PrettyAttachedSyntaxBehavior::Required)
        }
        crate::source_model::AttachedFixedSyntax::Optional { tokens, .. } => tokens
            .iter()
            .map(|token| fixed(token.token_use))
            .collect::<Result<Vec<_>, _>>()
            .map(PrettyAttachedSyntaxBehavior::Optional),
    };
    let before = attachment
        .before
        .iter()
        .map(syntax)
        .collect::<Result<Vec<_>, _>>()?;
    let after = attachment
        .after
        .iter()
        .map(syntax)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PrettyAttachmentBehavior {
        layout: pretty_attachment_layout(&before, &after),
        before,
        after,
    })
}
/// Hashes the complete Pretty plan for cross-phase drift validation.
pub(super) fn pretty_family_identity(plan: &PrettyEmissionPlan) -> Option<u64> {
    (plan.tokens.iter().any(Option::is_some) || plan.nodes.iter().any(Option::is_some))
        .then(|| plan_family_identity(plan))
}

/// Renders precedence-aware `Pretty` behavior for one Pratt Node.
///
/// # Long-function contract
///
/// Rationale: keeps Pratt parenthesization beside typed Pretty behavior.
/// Invariants: rendered precedence and associativity mirror the parse plan without consulting runtime grammar metadata.
/// Phases: selects active variant, renders operands and attachments, then applies grouping.
pub(super) fn render_pratt_node_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    _pratt: &PrattParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Pratt behavior");
    let pratt_behavior = behavior
        .pratt
        .as_ref()
        .expect("validated typed Pretty Pratt behavior");
    let (impl_generics, type_generics, where_clause) = pretty_impl_parts(facade, node);
    let variants = trait_variants(&node.body);
    let variant_count = variants.len();
    let rendered_variants = variants
        .iter()
        .zip(&behavior.variant_layouts)
        .zip(&behavior.variants)
        .zip(&behavior.variant_pretty)
        .zip(&pratt_behavior.variants)
        .enumerate()
        .map(
            |(variant_index, ((((variant, layout), behavior), variant_pretty), pratt_variant))| {
            let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "pretty_pratt");
            let mut binding_index = 0_usize;
            let mut semantic_index = 0_usize;
            let rendered = variant
            .elements
            .iter()
            .zip(behavior)
            .enumerate()
            .map(|(index, (element, behavior))| match (&element.kind, behavior) {
                (
                    ElementParseKind::Fixed { .. },
                    PrettyElementBehavior::Fixed {
                        spelling,
                        rendering,
                        soft_break_before,
                        soft_break_after,
                        ..
                    },
                ) => {
                    let fixed = render_pretty_fixed(plan, index, spelling, *rendering);
                    let before = soft_break_before
                        .then(|| quote!(context.soft_break(" ")?;));
                    let after = soft_break_after.then(|| quote!(context.soft_break(" ")?;));
                    quote!(#before #fixed #after)
                }
                (
                    ElementParseKind::OptionalFixedTokenGroup { .. },
                    PrettyElementBehavior::OptionalFixedTokenGroup { tokens },
                ) => render_pretty_optional_fixed_group(index, tokens),
                (
                    ElementParseKind::Semantic {
                        projection: parse_projection,
                        ..
                    },
                    PrettyElementBehavior::Semantic {
                        projection: pretty_projection,
                    },
                ) => match (parse_projection.as_ref(), pretty_projection) {
                    (
                        SemanticParseProjection::FixedPresence { .. },
                        PrettySemanticProjection::FixedPresence {
                            before,
                            sequence,
                            after,
                            pretty,
                        },
                    ) => {
                        let binding = &bindings[binding_index];
                        binding_index += 1;
                        render_pretty_fixed_token_presence(
                            index,
                            quote!(*#binding),
                            before,
                            sequence,
                            after,
                            pretty,
                        )
                    }
                    (
                        SemanticParseProjection::Direct {
                            wrappers, container, ..
                        },
                        PrettySemanticProjection::Direct {
                            attachment,
                            pretty,
                            presence,
                            provenance,
                            ..
                        },
                    ) => {
                        let binding = &bindings[binding_index];
                        binding_index += 1;
                        let operand = &pratt_variant.operands[semantic_index];
                        semantic_index += 1;
                        match operand {
                            PrettyPrattOperandBehavior::Ordinary => {
                                render_non_pratt_pretty_element(
                                    plan,
                                    index,
                                    element,
                                    behavior,
                                    Some((quote!(#binding), true)),
                                )
                            }
                            PrettyPrattOperandBehavior::Recursive { minimum, next, .. } => {
                                assert!(
                                    matches!(container.as_ref(), PlannedContainer::Direct { .. })
                                        && wrappers.iter().all(|wrapper| {
                                            matches!(wrapper, FieldWrapper::Boxed)
                                        }),
                                    "validated recursive Pratt operands are direct and may only use Box storage wrappers",
                                );
                                let minimum = render_pretty_pratt_minimum(*minimum);
                                let next = render_pretty_pratt_next(*next);
                                let semantic = quote!(
                                    (#binding).__recursa_pretty_pratt(context, #minimum, #next)
                                );
                                let rendered = if let Some(attachment) = attachment {
                                    let unit = render_pretty_attachment_unit(
                                        attachment,
                                        *provenance,
                                        semantic,
                                    );
                                    quote! {
                                        context.with_child(#index, |context| {
                                            context.with_child(0, |context| { #unit Ok(()) })
                                        })?;
                                    }
                                } else {
                                    let child = pretty_provenance_child_method(*provenance);
                                    quote! {
                                        context.with_child(#index, |context| {
                                            context.#child(0, |context| { #semantic })
                                        })?;
                                    }
                                };
                                render_pretty_presence(
                                    pretty,
                                    *presence,
                                    wrappers,
                                    quote!(#binding),
                                    rendered,
                                )
                            }
                        }
                    }
                    _ => unreachable!("validated Pretty Pratt semantic projection"),
                },
                (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
                | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. })
                | (ElementParseKind::OptionalFixedTokenGroup { .. }, _)
                | (_, PrettyElementBehavior::OptionalFixedTokenGroup { .. }) => {
                    unreachable!("validated Pretty Pratt element behavior")
                }
            })
            .collect::<Vec<_>>();
            let rendered = render_pretty_sequence(*layout, &rendered);
            let rendered = render_pretty_hints(variant_pretty, rendered);
            let entry_check = pratt_variant
                .parenthesization
                .entry_lbp
                .map_or_else(|| quote!(false), |entry| quote!(#entry < min_bp));
            let exit = pratt_variant.parenthesization.exit_rbp;
            let group_open = pratt_behavior.grouping.as_ref().map(|grouping| {
                grouping
                    .before
                    .iter()
                    .map(|fixed| {
                        render_pretty_synthetic_fixed(plan, &fixed.spelling, fixed.rendering)
                    })
                    .collect::<proc_macro2::TokenStream>()
            });
            let group_close = pratt_behavior.grouping.as_ref().map(|grouping| {
                grouping
                    .after
                    .iter()
                    .map(|fixed| {
                        render_pretty_synthetic_fixed(plan, &fixed.spelling, fixed.rendering)
                    })
                    .collect::<proc_macro2::TokenStream>()
            });
            let helper_body = quote! {
                    let __recursa_parenthesize = #entry_check
                        || next_lbp.is_some_and(|next| next >= #exit);
                    if __recursa_parenthesize {
                        #group_open
                    }
                    let __recursa_inner_min = if __recursa_parenthesize { 0 } else { min_bp };
                    let __recursa_inner_next = if __recursa_parenthesize { None } else { next_lbp };
                    #rendered
                    if __recursa_parenthesize {
                        #group_close
                    }
                    Ok(())
            };
            let helper = format_ident!(
                "__recursa_pretty_pratt_variant_{node_index}_{variant_index}"
            );
            let helper_body = if variant_count == 1 {
                quote! {
                    let #pattern = self;
                    #helper_body
                }
            } else {
                quote! {
                    match self {
                        #pattern => { #helper_body }
                        _ => unreachable!("Pratt Pretty dispatched to the wrong variant helper"),
                    }
                }
            };
            let variant_pattern = variant_pattern(node_ident, &variant.ident, variant.style);
            (
                quote! {
                    #variant_pattern => self.#helper(context, min_bp, next_lbp)
                },
                quote! {
                    fn #helper(
                        &self,
                        context: &mut #facade::PrettyContext,
                        min_bp: u16,
                        next_lbp: Option<u16>,
                    ) -> ::core::fmt::Result {
                        #helper_body
                    }
                },
            )
        },
        )
        .collect::<Vec<_>>();
    let arms = rendered_variants.iter().map(|(arm, _)| arm);
    let helper_methods = rendered_variants.iter().map(|(_, helper)| helper);
    let rendered_node = render_pretty_hints(
        &behavior.pretty,
        quote!(self.__recursa_pretty_pratt(context, 0, None)?;),
    );
    quote! {
        impl #impl_generics #node_ident #type_generics #where_clause {
            fn __recursa_pretty_pratt(
                &self,
                context: &mut #facade::PrettyContext,
                min_bp: u16,
                next_lbp: Option<u16>,
            ) -> ::core::fmt::Result {
                match self { #(#arms),* }
            }

            #(#helper_methods)*
        }

        impl #impl_generics #facade::Pretty for #node_ident #type_generics #where_clause {
            fn pretty(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.configure_defaults(#max_width, #indent);
                context.baseline_group(|context| { #rendered_node Ok(()) })
            }
        }
    }
}

/// Renders the minimum binding-power expression for a recursive operand.
pub(super) fn render_pretty_pratt_minimum(minimum: PrettyPrattMinimum) -> proc_macro2::TokenStream {
    match minimum {
        PrettyPrattMinimum::Zero => quote!(0u16),
        PrettyPrattMinimum::Inner => quote!(__recursa_inner_min),
        PrettyPrattMinimum::Binding(binding) => quote!(#binding),
    }
}

/// Renders the following binding-power context for parenthesization.
pub(super) fn render_pretty_pratt_next(next: PrettyPrattNext) -> proc_macro2::TokenStream {
    match next {
        PrettyPrattNext::None => quote!(None),
        PrettyPrattNext::Inner => quote!(__recursa_inner_next),
        PrettyPrattNext::Binding(binding) => quote!(Some(#binding)),
    }
}

/// Extracts the terminal Node identifier used in generated trait calls.
pub(super) fn terminal_path_ident(terminal: &Type) -> Option<&Ident> {
    let Type::Path(path) = terminal else {
        return None;
    };
    path.path.segments.last().map(|segment| &segment.ident)
}

/// Renders one non-Pratt element from its frozen value and formatting projections.
fn render_non_pratt_pretty_element(
    plan: &EmissionPlan,
    index: usize,
    element: &ElementParsePlan,
    behavior: &PrettyElementBehavior,
    semantic_value: Option<(proc_macro2::TokenStream, bool)>,
) -> proc_macro2::TokenStream {
    match (&element.kind, behavior) {
        (
            ElementParseKind::Fixed { .. },
            PrettyElementBehavior::Fixed {
                spelling,
                rendering,
                ..
            },
        ) => render_pretty_fixed(plan, index, spelling, *rendering),
        (
            ElementParseKind::OptionalFixedTokenGroup { .. },
            PrettyElementBehavior::OptionalFixedTokenGroup { tokens },
        ) => render_pretty_optional_fixed_group(index, tokens),
        (
            ElementParseKind::Semantic {
                projection: parse_projection,
                ..
            },
            PrettyElementBehavior::Semantic {
                projection: pretty_projection,
            },
        ) => {
            let (value, borrowed) = semantic_value.expect("semantic Pretty element has a value");
            let content_token = matches!(
                parse_projection.as_ref(),
                SemanticParseProjection::Direct {
                    explorer_terminal: ExplorerTerminalPlan::Content(_),
                    ..
                }
            );
            match (parse_projection.as_ref(), pretty_projection) {
                (
                    SemanticParseProjection::FixedPresence { .. },
                    PrettySemanticProjection::FixedPresence {
                        before,
                        sequence,
                        after,
                        pretty,
                    },
                ) => {
                    let condition = if borrowed { quote!(*#value) } else { value };
                    render_pretty_fixed_token_presence(
                        index, condition, before, sequence, after, pretty,
                    )
                }
                (
                    SemanticParseProjection::Direct { wrappers, .. },
                    PrettySemanticProjection::Direct {
                        repetition,
                        attachment,
                        pretty,
                        presence,
                        provenance,
                    },
                ) => {
                    let rendered = if let Some(attachment) = attachment {
                        render_pretty_attached_semantic(
                            plan,
                            index,
                            value.clone(),
                            borrowed,
                            wrappers,
                            attachment,
                            repetition.as_ref(),
                            *presence,
                            *provenance,
                            content_token,
                        )
                    } else {
                        let reference = if borrowed {
                            value.clone()
                        } else {
                            quote!(&#value)
                        };
                        render_pretty_semantic(
                            plan,
                            index,
                            reference,
                            repetition.as_ref(),
                            *provenance,
                            content_token,
                        )
                    };
                    let reference = if borrowed { value } else { quote!(&#value) };
                    render_pretty_presence(pretty, *presence, wrappers, reference, rendered)
                }
                _ => unreachable!("validated non-Pratt Pretty semantic projection"),
            }
        }
        (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
        | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. })
        | (ElementParseKind::OptionalFixedTokenGroup { .. }, _)
        | (_, PrettyElementBehavior::OptionalFixedTokenGroup { .. }) => {
            unreachable!("validated non-Pratt Pretty element behavior")
        }
    }
}

/// Renders structured Pretty and source-map behavior for a non-Pratt Node.
///
/// # Long-function contract
///
/// Rationale: renders one Node from its frozen Pretty behavior and source-order shape.
/// Invariants: every spacing, break, separator, and provenance decision participates in family identity.
/// Phases: binds the active shape, renders its element sequence, then writes through PrettyContext.
pub(super) fn render_node_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Node behavior");
    let (impl_generics, type_generics, where_clause) = pretty_impl_parts(facade, node);
    let elements = |elements: &[ElementParsePlan],
                    behavior: &[PrettyElementBehavior],
                    layout: PrettySequenceLayout,
                    base: proc_macro2::TokenStream| {
        let rendered = elements
            .iter()
            .zip(behavior)
            .enumerate()
            .map(|(index, (element, behavior))| {
                let value = match &element.kind {
                    ElementParseKind::Semantic { field, .. } => Some((
                        field_access(base.clone(), field.ident.as_ref(), field.index),
                        false,
                    )),
                    ElementParseKind::Fixed { .. }
                    | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
                };
                render_non_pratt_pretty_element(plan, index, element, behavior, value)
            })
            .collect::<Vec<_>>();
        render_pretty_sequence(layout, &rendered)
    };
    let body = match &node.body {
        NodeBodyPlan::Struct {
            elements: fields, ..
        } => {
            let rendered = elements(
                fields,
                &behavior.variants[0],
                behavior.variant_layouts[0],
                quote!(self),
            );
            quote!({ #rendered Ok(()) })
        }
        NodeBodyPlan::Pratt(pratt) => {
            return render_pratt_node_pretty(plan, node, node_index, pratt);
        }
        NodeBodyPlan::Enum { .. } => {
            let variants = trait_variants(&node.body);
            let arms = variants
                .iter()
                .zip(&behavior.variant_layouts)
                .zip(&behavior.variants)
                .zip(&behavior.variant_pretty)
                .map(|(((variant, layout), behavior), variant_pretty)| {
                    let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "pretty");
                    let mut binding_index = 0;
                    let rendered = variant
                        .elements
                        .iter()
                        .zip(behavior)
                        .enumerate()
                        .map(|(index, (element, behavior))| {
                            let value = if matches!(element.kind, ElementParseKind::Semantic { .. })
                            {
                                let binding = &bindings[binding_index];
                                binding_index += 1;
                                Some((quote!(#binding), true))
                            } else {
                                None
                            };
                            render_non_pratt_pretty_element(plan, index, element, behavior, value)
                        })
                        .collect::<Vec<_>>();
                    let rendered = render_pretty_sequence(*layout, &rendered);
                    let rendered = render_pretty_hints(variant_pretty, rendered);
                    quote!(#pattern => { #rendered Ok(()) })
                });
            quote!(match self { #(#arms),* })
        }
    };
    let body = render_pretty_hints(&behavior.pretty, quote!(#body?;));
    quote! {
        impl #impl_generics #facade::Pretty for #node_ident #type_generics #where_clause {
            fn pretty(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.configure_defaults(#max_width, #indent);
                context.baseline_group(|context| { #body Ok(()) })
            }
        }
    }
}

/// Renders one semantic field with its presence, repetition, attachment, and provenance rules.
pub(super) fn render_pretty_semantic(
    plan: &EmissionPlan,
    index: usize,
    value: proc_macro2::TokenStream,
    repetition: Option<&PrettyRepetitionBehavior>,
    provenance: PrettyProvenanceBehavior,
    content_token: bool,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let pretty = if content_token {
        quote! {
            <_ as #module_anchor::__RecursaPrettyWith>
                ::__recursa_pretty_with(#value, context)
        }
    } else {
        quote!(#facade::Pretty::pretty(#value, context))
    };
    let render_value = if let Some(repetition) = repetition {
        let separator = repetition
            .separator
            .as_deref()
            .map_or_else(|| quote!(None), |separator| quote!(Some(#separator)));
        let trailing = repetition.trailing_separator;
        let hard_break = repetition.hard_break;
        let leading = repetition.leading_separator;
        let space_before_separator = repetition.space_before_separator;
        quote! {
            context.with_repetition_style(#separator, #trailing, #hard_break, #leading, #space_before_separator, |context| {
                #pretty
            })
        }
    } else {
        pretty
    };
    if repetition.is_some() {
        quote! {
            context.with_child(#index, |context| #render_value)?;
        }
    } else {
        let child = pretty_provenance_child_method(provenance);
        quote! {
            context.with_child(#index, |context| {
                context.#child(0, |context| #render_value)
            })?;
        }
    }
}

/// Wraps a rendered fragment in the configured grouping and break hints.
pub(super) fn render_pretty_hints(
    hints: &PrettyHints,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let before = match hints.break_before {
        Some(PrettyBreak::Soft) => quote!(context.soft_break(" ")?;),
        Some(PrettyBreak::Hard) => quote!(context.hard_break()?;),
        None => quote!(),
    };
    let after = match hints.break_after {
        Some(PrettyBreak::Soft) => quote!(context.soft_break(" ")?;),
        Some(PrettyBreak::Hard) => quote!(context.hard_break()?;),
        None => quote!(),
    };
    let body = if hints.indent {
        quote!(context.indented(|context| { #body Ok(()) })?;)
    } else {
        body
    };
    let body = match hints.group {
        Some(PrettyGroup::Consistent) => quote!(context.group(|context| { #body Ok(()) })?;),
        Some(PrettyGroup::Inconsistent) => {
            quote!(context.inconsistent_group(|context| { #body Ok(()) })?;)
        }
        None => body,
    };
    quote!(#before #body #after)
}

/// Applies optional or non-empty guards around a semantic fragment.
pub(super) fn render_pretty_presence(
    hints: &PrettyHints,
    presence: PrettyPresence,
    wrappers: &[FieldWrapper],
    value: proc_macro2::TokenStream,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let body = render_pretty_hints(hints, body);
    if hints.break_before.is_none()
        && hints.break_after.is_none()
        && !hints.indent
        && hints.group.is_none()
    {
        return body;
    }
    if presence == PrettyPresence::Always {
        return body;
    }
    let mut reference = value;
    for wrapper in wrappers {
        match wrapper {
            FieldWrapper::Boxed => reference = quote!((#reference).as_ref()),
            FieldWrapper::Optional if presence == PrettyPresence::Optional => {
                return quote!(if (#reference).is_some() { #body });
            }
            FieldWrapper::ZeroOrMore if presence == PrettyPresence::NonEmpty => {
                return quote!(if !(#reference).is_empty() { #body });
            }
            FieldWrapper::Optional | FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {}
        }
    }
    unreachable!("frozen Pretty presence has no matching storage wrapper")
}

/// Selects structured-child or opaque-span source mapping for a semantic field.
pub(super) fn pretty_provenance_child_method(provenance: PrettyProvenanceBehavior) -> Ident {
    match provenance {
        PrettyProvenanceBehavior::Structured => format_ident!("with_child"),
        PrettyProvenanceBehavior::Opaque => format_ident!("with_opaque_child"),
    }
}

/// Renders fixed syntax from matched source when available, otherwise canonically.
pub(super) fn render_pretty_fixed(
    plan: &EmissionPlan,
    index: usize,
    spelling: &str,
    rendering: PrettyFixedRendering,
) -> proc_macro2::TokenStream {
    render_pretty_fixed_with_method(plan, index, spelling, rendering)
}

/// Applies the planned placement method to one fixed-token document.
pub(super) fn render_pretty_fixed_with_method(
    plan: &EmissionPlan,
    index: usize,
    spelling: &str,
    rendering: PrettyFixedRendering,
) -> proc_macro2::TokenStream {
    let method = pretty_fixed_method(rendering);
    let fallback = pretty_fixed_fallback(plan, spelling);
    quote! {
        context.with_child(#index, |context| {
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#fallback))
        })?;
    }
}

/// Renders a boolean-controlled fixed sequence inside its required attachment envelope.
fn render_pretty_fixed_token_presence(
    index: usize,
    value: proc_macro2::TokenStream,
    before: &[PrettyAttachedSyntaxBehavior],
    sequence: &[PrettyAttachedFixedBehavior],
    after: &[PrettyAttachedSyntaxBehavior],
    pretty: &PrettyHints,
) -> proc_macro2::TokenStream {
    let before_tokens = before
        .iter()
        .enumerate()
        .map(|(index, syntax)| render_pretty_attached_syntax(index, syntax));
    let presence_index = before.len();
    let sequence_tokens = sequence
        .iter()
        .enumerate()
        .map(|(index, fixed)| render_pretty_attached_fixed(index, fixed));
    let after_tokens = after
        .iter()
        .enumerate()
        .map(|(offset, fixed)| render_pretty_attached_syntax(presence_index + 1 + offset, fixed));
    let rendered = quote! {
        context.with_child(#index, |context| {
            #(#before_tokens)*
            if #value {
                context.with_child(#presence_index, |context| {
                    #(#sequence_tokens)*
                    Ok(())
                })?;
            }
            #(#after_tokens)*
            Ok(())
        })?;
    };
    render_pretty_hints(pretty, rendered)
}

/// Renders one erased optional group only when parsed provenance records it.
fn render_pretty_optional_fixed_group(
    index: usize,
    tokens: &[PrettyAttachedFixedBehavior],
) -> proc_macro2::TokenStream {
    let tokens = tokens
        .iter()
        .enumerate()
        .map(|(index, fixed)| render_pretty_attached_fixed(index, fixed));
    quote! {
        context.with_child(#index, |context| {
            if context.has_source_span() {
                #(#tokens)*
            }
            Ok(())
        })?;
    }
}

/// Renders canonical fixed text for a synthetic document without source provenance.
pub(super) fn render_pretty_synthetic_fixed(
    plan: &EmissionPlan,
    spelling: &str,
    rendering: PrettyFixedRendering,
) -> proc_macro2::TokenStream {
    let method = pretty_fixed_method(rendering);
    let fallback = pretty_fixed_fallback(plan, spelling);
    quote!(context.without_provenance(|context| context.#method(#fallback))?;)
}

/// Selects the document-builder method for fixed-token placement.
pub(super) fn pretty_fixed_method(rendering: PrettyFixedRendering) -> syn::Ident {
    match rendering {
        PrettyFixedRendering::TightSequence => format_ident!("tight_sequence_token"),
        PrettyFixedRendering::Before => format_ident!("before_token"),
        PrettyFixedRendering::After => format_ident!("after_token"),
        PrettyFixedRendering::Around => format_ident!("around_token"),
        PrettyFixedRendering::Opening => format_ident!("opening_token"),
        PrettyFixedRendering::Word => format_ident!("token"),
        PrettyFixedRendering::Punctuation => format_ident!("punctuation_token"),
    }
}

/// Produces canonical fixed text when matched source text is unavailable.
pub(super) fn pretty_fixed_fallback(_plan: &EmissionPlan, spelling: &str) -> String {
    spelling.to_owned()
}

/// Renders an ordered element sequence under its linear or delimited layout.
pub(super) fn render_pretty_sequence(
    layout: PrettySequenceLayout,
    rendered: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    if layout == PrettySequenceLayout::Delimited && rendered.len() >= 2 {
        let open = &rendered[0];
        let middle = &rendered[1..rendered.len() - 1];
        let close = &rendered[rendered.len() - 1];
        quote! {
            #open
            context.indented(|context| {
                context.soft_break("")?;
                #(#middle)*
                Ok(())
            })?;
            context.soft_break("")?;
            #close
            context.close_delimiter();
        }
    } else {
        quote!(#(#rendered)*)
    }
}

/// Renders the fixed-token portion of an attachment in source order.
pub(super) fn render_pretty_attached_fixed(
    index: usize,
    fixed: &PrettyAttachedFixedBehavior,
) -> proc_macro2::TokenStream {
    let method = pretty_fixed_method(fixed.rendering);
    let spelling = &fixed.spelling;
    quote! {
        context.with_child(#index, |context| {
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#spelling))
        })?;
    }
}

/// Renders one required or erased-optional attachment entry.
fn render_pretty_attached_syntax(
    index: usize,
    syntax: &PrettyAttachedSyntaxBehavior,
) -> proc_macro2::TokenStream {
    match syntax {
        PrettyAttachedSyntaxBehavior::Required(fixed) => render_pretty_attached_fixed(index, fixed),
        PrettyAttachedSyntaxBehavior::Optional(tokens) => {
            render_pretty_optional_fixed_group(index, tokens)
        }
    }
}

/// Groups one semantic value with its attached fixed syntax.
pub(super) fn render_pretty_attachment_unit(
    attachment: &PrettyAttachmentBehavior,
    provenance: PrettyProvenanceBehavior,
    semantic_body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let before = attachment
        .before
        .iter()
        .enumerate()
        .map(|(index, syntax)| render_pretty_attached_syntax(index, syntax))
        .collect::<Vec<_>>();
    let semantic_index = before.len();
    let child = pretty_provenance_child_method(provenance);
    let semantic = quote! {
        context.#child(#semantic_index, |context| { #semantic_body })?;
    };
    let after = attachment
        .after
        .iter()
        .enumerate()
        .map(|(index, syntax)| render_pretty_attached_syntax(semantic_index + 1 + index, syntax))
        .collect::<Vec<_>>();
    if attachment.layout == PrettySequenceLayout::Delimited {
        let open = &before[0];
        let leading = &before[1..];
        let close = after
            .last()
            .expect("delimited attachment has a close token");
        let trailing = &after[..after.len() - 1];
        quote! {
            #open
            context.indented(|context| {
                context.soft_break("")?;
                #(#leading)*
                #semantic
                #(#trailing)*
                Ok(())
            })?;
            context.soft_break("")?;
            #close
            context.close_delimiter();
        }
    } else {
        quote!(#(#before)* #semantic #(#after)*)
    }
}

#[allow(clippy::too_many_arguments)]
/// Renders an attached semantic value while retaining its source-map child.
pub(super) fn render_pretty_attached_semantic(
    plan: &EmissionPlan,
    element_index: usize,
    value: proc_macro2::TokenStream,
    value_is_reference: bool,
    wrappers: &[FieldWrapper],
    attachment: &PrettyAttachmentBehavior,
    repetition: Option<&PrettyRepetitionBehavior>,
    presence: PrettyPresence,
    provenance: PrettyProvenanceBehavior,
    content_token: bool,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let render_unit = |terminal: proc_macro2::TokenStream| {
        let semantic = if content_token {
            quote! {
                <_ as #module_anchor::__RecursaPrettyWith>
                    ::__recursa_pretty_with(#terminal, context)
            }
        } else {
            quote!(#facade::Pretty::pretty(#terminal, context))
        };
        render_pretty_attachment_unit(attachment, provenance, semantic)
    };

    if let Some(repetition) = repetition {
        let repeat_index = wrappers
            .iter()
            .position(|wrapper| {
                matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
            })
            .expect("repeated attached field has a repetition wrapper");
        let outer = &wrappers[..repeat_index];
        let inner = &wrappers[repeat_index + 1..];
        let item = render_terminal_reference(quote!(item), inner, true);
        let unit = render_unit(item);
        let separator = repetition
            .separator
            .as_deref()
            .map_or_else(|| quote!(None), |separator| quote!(Some(#separator)));
        let trailing = repetition.trailing_separator;
        let hard_break = repetition.hard_break;
        let leading = repetition.leading_separator;
        let space_before_separator = repetition.space_before_separator;
        let render_collection = |collection: proc_macro2::TokenStream| {
            quote! {
                context.with_repetition_style(
                    #separator,
                    #trailing,
                    #hard_break,
                    #leading,
                    #space_before_separator,
                    |context| {
                        context.render_repeated_by(
                            (#collection).iter(),
                            |context, item| { #unit Ok(()) },
                        )
                    },
                )?;
            }
        };
        let outer_optional = presence == PrettyPresence::Optional;
        let body = if outer_optional {
            let collection = render_optional_view_reference(value, outer, value_is_reference);
            let rendered = render_collection(quote!(collection));
            quote! {
                if let Some(collection) = #collection {
                    #rendered
                }
            }
        } else {
            let collection = render_terminal_reference(value, outer, value_is_reference);
            render_collection(collection)
        };
        quote! {
            context.with_child(#element_index, |context| {
                #body
                Ok(())
            })?;
        }
    } else if presence == PrettyPresence::Optional {
        let terminal = render_optional_view_reference(value, wrappers, value_is_reference);
        let unit = render_unit(quote!(terminal));
        quote! {
            if let Some(terminal) = #terminal {
                context.with_child(#element_index, |context| {
                    context.with_child(0, |context| { #unit Ok(()) })
                })?;
            }
        }
    } else {
        let terminal = render_terminal_reference(value, wrappers, value_is_reference);
        let unit = render_unit(terminal);
        quote! {
            context.with_child(#element_index, |context| {
                context.with_child(0, |context| { #unit Ok(()) })
            })?;
        }
    }
}

/// Returns the closing delimiter paired with an opening fixed spelling.
pub(super) fn delimiter_close(open: &str) -> Option<&'static str> {
    match open {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        _ => None,
    }
}
