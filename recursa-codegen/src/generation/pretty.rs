//! Pretty planning, authoritative validation, and structured rendering.
//!
//! Pretty plan construction and emission are intentionally co-located: spacing, provenance,
//! Parenthesization, repetition, and attachment layout form one behavioral contract.

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
    focused_case!(pretty_recursion_uses_typed_node_identity_not_the_final_path_segment);
    focused_case!(pretty_grouping_follows_a_unique_authored_helper_path);
    focused_case!(pretty_grouping_accepts_a_merged_kind_twin_as_fixed_syntax);
    focused_case!(pretty_grouping_accepts_agreeing_delegated_enclosure_atoms);
    focused_case!(pretty_grouping_refuses_disagreeing_delegated_enclosure_atoms);
    focused_case!(pretty_grouping_refuses_a_close_whose_arms_render_different_syntax);
    focused_case!(pretty_grouping_survives_a_witnessless_cyclic_sibling_variant);
    focused_case!(pretty_grouping_ignores_a_witnessed_cycle_behind_a_half_open_context);
    focused_case!(pretty_variants_are_typechecked_in_isolated_helpers);
    focused_case!(generated_pretty_scopes_do_not_emit_callback_boundaries);
    focused_case!(every_typed_pretty_declared_decision_is_part_of_family_authority);
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

        impl<T> __RecursaPrettyWith for #facade::ArenaBox<'_, T>
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
                context.render_repeated_by(self.iter(), |context, value| {
                    value.__recursa_pretty_with(context)
                })
            }
        }

        impl<T> __RecursaPrettyWith for #facade::ArenaVec<'_, T>
        where
            T: __RecursaPrettyWith,
        {
            fn __recursa_pretty_with(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.render_repeated_by(self.iter(), |context, value| {
                    value.__recursa_pretty_with(context)
                })
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
                context.render_repeated_by(self.iter(), |context, value| {
                    value.__recursa_pretty_with(context)
                })
            }
        }


        impl<T> __RecursaPrettyWith for #facade::ArenaVec1<'_, T>
        where
            T: __RecursaPrettyWith,
        {
            fn __recursa_pretty_with(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.render_repeated_by(self.iter(), |context, value| {
                    value.__recursa_pretty_with(context)
                })
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Layout, element, and precedence behavior shared by a node family.
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
    /// Declared-precedence grouping and parenthesization decisions for a plain enum Node with
    /// at least one self-recursive operand; `None` for every other Node, including a plain
    /// enum with no self-recursion at all.
    pub(super) declared: Option<PrettyDeclaredBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Fixed syntax surrounding an explicit recursive grouping.
pub(super) struct PrettyGrouping {
    /// Fixed syntax rendered before the recursively grouped expression.
    pub(super) before: Vec<PrettyAttachedFixedBehavior>,
    /// Fixed syntax rendered after the recursively grouped expression.
    pub(super) after: Vec<PrettyAttachedFixedBehavior>,
}

#[derive(Clone, Debug, Default)]
/// Compact syntax graph used to prove an indirect authored grouping path.
pub(super) struct PrettyGroupingSyntax {
    /// Structural helper bodies keyed by generated Node identity.
    nodes: BTreeMap<NodeId, Vec<Vec<PrettyGroupingElement>>>,
}

#[derive(Clone, Debug)]
/// One fixed, optional, or semantic step in a grouping-proof sequence.
enum PrettyGroupingElement {
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
struct PrettyGroupingProof {
    /// Whether the fragment can contribute no required syntax.
    empty: bool,
    /// Authored single-owner paths, capped at two to distinguish ambiguity.
    witnesses: Vec<PrettyGroupingWitness>,
    /// Helper Nodes whose re-entry left this witness list possibly incomplete.
    cyclic: BTreeSet<NodeId>,
}

#[derive(Clone, Debug)]
/// One grouping derivation and whether it traverses an authored helper Node.
struct PrettyGroupingWitness {
    /// Fixed syntax synthesized around the precedence-sensitive expression.
    grouping: PrettyGrouping,
    /// Whether the recursive owner is reached indirectly through a helper.
    delegated: bool,
    /// The semantic element on the path at each step, outermost first: the element of the atom
    /// variant, then the variant and element of each helper Node, down to the owner.
    steps: Vec<PrettyGroupingStep>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One step of a grouping witness: the variant of a Node and its element on the path.
struct PrettyGroupingStep {
    /// The variant index of the Node the step is in; 0 for a struct and for the atom variant,
    /// whose index the caller knows.
    variant: usize,
    /// The element index of the semantic element on the path.
    element: usize,
}

impl PrettyGroupingProof {
    /// Retains enough paths to distinguish a unique witness from ambiguity.
    fn push_witness(&mut self, witness: PrettyGroupingWitness) {
        if self.witnesses.len() < 2 {
            self.witnesses.push(witness);
        }
    }
}

/// Run-local, cycle-safe analyzer for one owning expression family.
struct PrettyGroupingAnalyzer<'a> {
    /// Structural syntax graph independently projected from source or frozen parse plans.
    syntax: &'a PrettyGroupingSyntax,
    /// Completed helper summaries keyed by owner and helper Node identities.
    memo: BTreeMap<(NodeId, NodeId), PrettyGroupingProof>,
    /// Active helper Nodes used to reject context-dependent recursive proofs.
    active: BTreeSet<NodeId>,
    /// Exact fixed-only helper expansions, including failed proofs.
    fixed_memo: BTreeMap<NodeId, Option<Vec<PrettyAttachedFixedBehavior>>>,
    /// Fixed-only helper Nodes active in the current depth-first proof.
    fixed_active: BTreeSet<NodeId>,
}

impl<'a> PrettyGroupingAnalyzer<'a> {
    /// Creates one analyzer whose memo is shared by every atom in an expression family.
    fn new(syntax: &'a PrettyGroupingSyntax) -> Self {
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
        elements: &[PrettyGroupingElement],
    ) -> Option<PrettyGroupingWitness> {
        let proof = self.elements(owner, elements);
        matches!(proof.witnesses.as_slice(), [witness]
        if proof.cyclic.is_empty()
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
        elements: &[PrettyGroupingElement],
    ) -> PrettyGroupingProof {
        let empty = elements
            .iter()
            .all(|element| self.element_is_empty(owner, element));
        let mut proof = PrettyGroupingProof {
            empty,
            ..PrettyGroupingProof::default()
        };
        for (index, element) in elements.iter().enumerate() {
            let PrettyGroupingElement::Semantic {
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
            let mut nested = terminal_node.map_or_else(PrettyGroupingProof::default, |terminal| {
                if terminal == owner {
                    PrettyGroupingProof {
                        witnesses: vec![PrettyGroupingWitness {
                            grouping: PrettyGrouping {
                                before: Vec::new(),
                                after: Vec::new(),
                            },
                            delegated: false,
                            steps: Vec::new(),
                        }],
                        ..PrettyGroupingProof::default()
                    }
                } else {
                    self.node(owner, terminal)
                }
            });
            let mut context_before = fixed_before.clone();
            context_before.extend(attached_before.iter().cloned());
            let mut context_after = attached_after.clone();
            context_after.extend(fixed_after.iter().cloned());
            let context_is_transparent = context_before.is_empty() && context_after.is_empty();
            let context_is_enclosure = !context_before.is_empty() && !context_after.is_empty();
            // A half-open fixed context (required syntax on exactly one side)
            // can never contribute a witness, so an under-enumerated cyclic
            // subtree behind it cannot hide one either; only witness-capable
            // contexts propagate cyclic incompleteness into the proof. The
            // source model applies the same rule, and the two must agree.
            if context_is_transparent || context_is_enclosure {
                proof.cyclic.extend(nested.cyclic.iter().copied());
            }
            for witness in nested.witnesses.drain(..) {
                let mut context_before = context_before.clone();
                let context_after = context_after.clone();
                let witness_is_transparent =
                    witness.grouping.before.is_empty() && witness.grouping.after.is_empty();
                if context_is_transparent || (context_is_enclosure && witness_is_transparent) {
                    context_before.extend(witness.grouping.before);
                    let mut after = witness.grouping.after;
                    after.extend(context_after);
                    let mut steps = vec![PrettyGroupingStep {
                        variant: 0,
                        element: index,
                    }];
                    steps.extend(witness.steps);
                    proof.push_witness(PrettyGroupingWitness {
                        grouping: PrettyGrouping {
                            before: context_before,
                            after,
                        },
                        delegated: witness.delegated,
                        steps,
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
        elements: &[PrettyGroupingElement],
    ) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        let mut fixed = Vec::new();
        for element in elements {
            match element {
                PrettyGroupingElement::Fixed(token) => fixed.push(token.clone()),
                PrettyGroupingElement::Optional => {}
                PrettyGroupingElement::Semantic { .. } if self.element_is_empty(owner, element) => {
                }
                PrettyGroupingElement::Semantic { .. } => {
                    let nested = self.fixed_only_element(element)?;
                    if nested.is_empty() {
                        return None;
                    }
                    fixed.extend(nested);
                }
                PrettyGroupingElement::Opaque => return None,
            }
        }
        Some(fixed)
    }

    /// Expands one required semantic marker into exact fixed Pretty syntax.
    fn fixed_only_element(
        &mut self,
        element: &PrettyGroupingElement,
    ) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        let PrettyGroupingElement::Semantic {
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

    /// Expands a non-recursive helper Node that renders exactly one fixed sequence.
    ///
    /// Every variant must expand to the same *rendered* fixed syntax: the same number of fixed
    /// entries, each with the same spelling, the same mechanical spacing class and the same
    /// placement. Equal rendering, not one variant and not equal token kinds, is the test,
    /// because the enclosure proof needs only the text the enclosure renders. A Node that names
    /// a `lookahead { ... }` merged kind beside the token it merges — pg-sql's
    /// `ParenthesizedClose: RPAREN | RPAREN_SELECT_LA`, gram.y's `any_with: WITH | WITH_LA`
    /// (#139) — is therefore still fixed syntax: such a position resolves to the definition of
    /// the merged token, so both arms carry that token's spelling and spacing, and the merged
    /// kind is the same record read differently (#142).
    fn fixed_only_node(&mut self, node: NodeId) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        if let Some(fixed) = self.fixed_memo.get(&node) {
            return fixed.clone();
        }
        if !self.fixed_active.insert(node) {
            return None;
        }
        let fixed = self.fixed_only_variants(node);
        self.fixed_active.remove(&node);
        self.fixed_memo.insert(node, fixed.clone());
        fixed
    }

    /// Expands every variant of one helper Node to the one fixed sequence they all render.
    ///
    /// A Node with no variant at all, a variant that is not fixed-only, or two variants whose
    /// rendered fixed syntax differs leaves the Node without fixed syntax.
    fn fixed_only_variants(&mut self, node: NodeId) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        let variants = self.syntax.nodes.get(&node)?;
        let mut agreed: Option<Vec<PrettyAttachedFixedBehavior>> = None;
        for elements in variants {
            let expansion = self.fixed_only_elements(elements)?;
            match &agreed {
                Some(previous) if *previous != expansion => return None,
                Some(_) => {}
                None => agreed = Some(expansion),
            }
        }
        agreed
    }

    /// Expands one helper sequence containing only required fixed syntax.
    fn fixed_only_elements(
        &mut self,
        elements: &[PrettyGroupingElement],
    ) -> Option<Vec<PrettyAttachedFixedBehavior>> {
        let mut fixed = Vec::new();
        for element in elements {
            match element {
                PrettyGroupingElement::Fixed(token) => fixed.push(token.clone()),
                PrettyGroupingElement::Semantic { .. } => {
                    fixed.extend(self.fixed_only_element(element)?);
                }
                PrettyGroupingElement::Optional | PrettyGroupingElement::Opaque => {
                    return None;
                }
            }
        }
        Some(fixed)
    }

    /// Reports whether one non-path element can disappear without required syntax.
    fn element_is_empty(&mut self, owner: NodeId, element: &PrettyGroupingElement) -> bool {
        match element {
            PrettyGroupingElement::Optional => true,
            PrettyGroupingElement::Semantic {
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
                        proof.cyclic.is_empty() && proof.empty
                    }))
            }
            PrettyGroupingElement::Fixed(_) | PrettyGroupingElement::Opaque => false,
        }
    }

    /// Derives and memoizes one structural helper Node summary.
    fn node(&mut self, owner: NodeId, node: NodeId) -> PrettyGroupingProof {
        if let Some(proof) = self.memo.get(&(owner, node)) {
            return proof.clone();
        }
        if !self.active.insert(node) {
            return PrettyGroupingProof {
                cyclic: BTreeSet::from([node]),
                ..PrettyGroupingProof::default()
            };
        }
        let mut proof = PrettyGroupingProof::default();
        if let Some(variants) = self.syntax.nodes.get(&node) {
            for (variant_index, elements) in variants.iter().enumerate() {
                let variant = self.elements(owner, elements);
                proof.empty |= variant.empty;
                proof.cyclic.extend(variant.cyclic.iter().copied());
                for mut witness in variant.witnesses {
                    if let Some(step) = witness.steps.first_mut() {
                        step.variant = variant_index;
                    }
                    proof.push_witness(witness);
                }
            }
        }
        self.active.remove(&node);
        // Mirrors the source model: a helper that closes its own frame without a
        // witness has the empty witness set as a fixed point of its own witness
        // equation, so no unrolling of the cycle it closes can add one.
        if proof.witnesses.is_empty() {
            proof.cyclic.remove(&node);
        }
        for witness in &mut proof.witnesses {
            witness.delegated = true;
        }
        if proof.cyclic.is_empty() {
            self.memo.insert((owner, node), proof.clone());
        }
        proof
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grouping and variant-specific rendering rules for a plain enum Node whose grammar declares
/// a `precedence { ... }` block.
pub(super) struct PrettyDeclaredBehavior {
    /// Authored delimiter behavior for the explicit grouping variant: the atom that escapes
    /// precedence with parentheses.
    pub(super) grouping: Option<PrettyGrouping>,
    /// Parenthesization and operand plans in declaration variant order.
    pub(super) variants: Vec<PrettyDeclaredVariantBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Parenthesization and operands for one plain enum variant under declared precedence.
pub(super) struct PrettyDeclaredVariantBehavior {
    /// This variant's own declared level: `None` for a variant with no declared terminal and
    /// no `#[parse(prec = NAME)]` override, which is never parenthesized as anyone's operand.
    pub(super) level: Option<u32>,
    /// Ordinary or self-recursive operand behavior in semantic-field order.
    pub(super) operands: Vec<PrettyDeclaredOperandBehavior>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Rendering rule for an ordinary or self-recursive declared-precedence operand.
pub(super) enum PrettyDeclaredOperandBehavior {
    /// Renders through the field's normal Pretty implementation.
    Ordinary,
    /// Renders through the owning Node's declared-precedence entry point.
    Recursive {
        /// Node recursively targeted by the operand; always the owning Node itself.
        owner: NodeId,
        /// Concrete recursive applications accepted at this site.
        applications: Vec<MaterializedApplicationId>,
        /// Left-edge precedence bound passed into recursive rendering.
        minimum: PrettyDeclaredMinimum,
        /// Right-edge precedence bound exposed after the rendered operand.
        next: PrettyDeclaredNext,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Left-edge precedence bound passed to a recursive declared-precedence operand.
pub(super) enum PrettyDeclaredMinimum {
    /// Imposes no left-edge bound: an interior operand, or the sole symbol of a level-less
    /// rule, is never parenthesized by precedence.
    None,
    /// Reuses the enclosing call's own left-edge bound unchanged.
    Inner,
    /// Establishes a concrete left-edge bound from the enclosing variant's own level.
    Binding(PrettyDeclaredBound),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Right-edge precedence bound exposed after a recursive declared-precedence operand.
pub(super) enum PrettyDeclaredNext {
    /// Imposes no right-edge bound.
    None,
    /// Reuses the enclosing call's own right-edge bound unchanged.
    Inner,
    /// Establishes a concrete right-edge bound from the enclosing variant's own level.
    Binding(PrettyDeclaredBound),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One declared-precedence threshold: a level and whether an operand at exactly that level
/// must still be parenthesized, bison's associativity tie-break restated for one edge.
pub(super) struct PrettyDeclaredBound {
    /// The threshold level: an operand whose own level is lower is always parenthesized.
    pub(super) level: u32,
    /// Whether an operand whose own level equals this threshold is also parenthesized.
    pub(super) tie_parenthesizes: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Where one semantic element sits in its owning variant's authored symbol sequence, which
/// drives whether a self-recursive operand there needs a fresh precedence bound.
pub(super) enum DeclaredOperandPosition {
    /// The rule's first symbol: nothing, not even a fixed token, precedes it.
    First,
    /// The rule's last symbol: nothing follows it.
    Last,
    /// Both the first and the last symbol: the rule has no other syntax at all.
    Sole,
    /// Neither: delimited by fixed tokens or other operands on at least one side, so never
    /// parenthesized by precedence.
    Interior,
}

impl DeclaredOperandPosition {
    /// Classifies from whether the operand is a rule boundary on each side.
    pub(super) const fn classify(is_first_symbol: bool, is_last_symbol: bool) -> Self {
        match (is_first_symbol, is_last_symbol) {
            (true, true) => Self::Sole,
            (true, false) => Self::First,
            (false, true) => Self::Last,
            (false, false) => Self::Interior,
        }
    }
}

/// Derives the precedence bound passed into one recursive declared-precedence operand from its
/// enclosing variant's own level and the operand's rule-boundary position.
///
/// The bison tie-break is folded in here: a first operand is parenthesized on a tie when the
/// enclosing level is `right` or `nonassoc`, and a last operand is parenthesized on a tie when
/// the enclosing level is `left` or `nonassoc`. A level-less enclosing variant establishes no
/// new bound and passes its own received bound through unchanged, the same as a sole operand.
pub(super) fn pretty_declared_operand_bound(
    own_level: Option<DeclaredVariantLevel>,
    position: DeclaredOperandPosition,
) -> (PrettyDeclaredMinimum, PrettyDeclaredNext) {
    let bound = |tie_parenthesizes_when_not: PrecedenceAssociativity| {
        own_level.map(|level| PrettyDeclaredBound {
            level: level.ordinal,
            tie_parenthesizes: level.associativity != tie_parenthesizes_when_not,
        })
    };
    match position {
        DeclaredOperandPosition::Interior => {
            (PrettyDeclaredMinimum::None, PrettyDeclaredNext::None)
        }
        DeclaredOperandPosition::Sole => (PrettyDeclaredMinimum::Inner, PrettyDeclaredNext::Inner),
        DeclaredOperandPosition::First => {
            let next = bound(PrecedenceAssociativity::Left)
                .map_or(PrettyDeclaredNext::Inner, PrettyDeclaredNext::Binding);
            (PrettyDeclaredMinimum::Inner, next)
        }
        DeclaredOperandPosition::Last => {
            let minimum = bound(PrecedenceAssociativity::Right)
                .map_or(PrettyDeclaredMinimum::Inner, PrettyDeclaredMinimum::Binding);
            (minimum, PrettyDeclaredNext::Inner)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Grouping strategy for an emitted element sequence.
pub(super) enum PrettySequenceLayout {
    /// Emits elements in the current document group.
    Linear,
    /// Groups a delimiter-bounded sequence for coordinated breaks.
    Delimited,
    /// Groups the delimited part of an attachment whose fixed prefix comes before the opening
    /// delimiter, as in `KEYWORD ( value )`: the last fixed token before the value opens the
    /// delimiter that the first fixed token after it closes.
    DelimitedAfterPrefix,
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
    /// Whether separators require whitespace before the following item.
    pub(super) space_after_separator: bool,
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
    /// Closes a delimited group tightly on both sides.
    Closing,
    /// Behaves as word-like syntax for adjacency.
    Word,
    /// Behaves as punctuation for adjacency.
    Punctuation,
}

/// Projects configured rendering behavior from the frozen parse and token plans.
///
/// Fails when a plain enum Node needs precedence-based parenthesization, because it declares a
/// `precedence { ... }` level somewhere and has a self-recursive operand at a rule boundary,
/// but its grammar has no atom that encloses `Self` between fixed tokens to escape it with.
pub(super) fn emitted_pretty_plan(
    nodes: &[NodeParsePlan],
    tokens: &[TokenParsePlan],
) -> Result<PrettyEmissionPlan, String> {
    let grouping_syntax = emitted_pretty_grouping_syntax(nodes);
    Ok(PrettyEmissionPlan {
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
                    .transpose()
            })
            .collect::<Result<Vec<_>, String>>()?,
    })
}

/// Projects frozen parse plans into the compact syntax graph used for grouping proofs.
fn emitted_pretty_grouping_syntax(nodes: &[NodeParsePlan]) -> PrettyGroupingSyntax {
    let nodes = nodes
        .iter()
        .map(|node| {
            let variants = match &node.body {
                NodeBodyPlan::Struct { elements, .. } => vec![elements.as_slice()],
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter()
                    .map(|variant| variant.elements.as_slice())
                    .collect(),
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
            (node.node, variants)
        })
        .collect();
    PrettyGroupingSyntax { nodes }
}

/// Projects one frozen parse element into grouping-proof syntax.
fn emitted_pretty_grouping_element(
    element: &ElementParsePlan,
    behavior: &PrettyElementBehavior,
) -> PrettyGroupingElement {
    match (&element.kind, behavior) {
        (
            ElementParseKind::Fixed { .. },
            PrettyElementBehavior::Fixed {
                spelling,
                spacing,
                rendering,
                ..
            },
        ) => PrettyGroupingElement::Fixed(PrettyAttachedFixedBehavior {
            spelling: spelling.clone(),
            spacing: *spacing,
            rendering: *rendering,
        }),
        (
            ElementParseKind::OptionalFixedTokenGroup { .. },
            PrettyElementBehavior::OptionalFixedTokenGroup { .. },
        ) => PrettyGroupingElement::Optional,
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
            ) => PrettyGroupingElement::Semantic {
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
            | (_, PrettySemanticProjection::FixedPresence { .. }) => PrettyGroupingElement::Opaque,
        },
        _ => PrettyGroupingElement::Opaque,
    }
}

/// Builds rendering behavior for one Node and all materialized applications.
pub(super) fn emitted_pretty_node_behavior(
    node: &NodeParsePlan,
    grouping_syntax: &PrettyGroupingSyntax,
) -> Result<PrettyNodeBehavior, String> {
    let variant_pretty = match &node.body {
        NodeBodyPlan::Struct { .. } => vec![PrettyHints::default()],
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
    };
    let variants = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => vec![emitted_pretty_elements(elements)],
        NodeBodyPlan::Enum { variants, .. } => variants
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
    let declared = match &node.body {
        NodeBodyPlan::Enum {
            variants: declaration_variants,
        } => emitted_pretty_declared_behavior(
            node,
            declaration_variants,
            &variants,
            grouping_syntax,
        )?,
        NodeBodyPlan::Struct { .. } => None,
    };
    Ok(PrettyNodeBehavior {
        node: node.node,
        applications,
        pretty: node.pretty.clone(),
        variant_pretty,
        variant_layouts,
        variants,
        declared,
    })
}

/// Derives declared-precedence grouping and operand behavior for a plain enum Node, or `None`
/// when no variant has a self-recursive operand and nothing needs parenthesizing.
///
/// # Long-function contract
///
/// Rationale: keeps operand-position classification beside the grouping-atom requirement they
/// share.
/// Invariants: a variant's own level and every operand's rule-boundary position are derived
/// purely from its authored element sequence.
/// Phases: classifies each variant's operands, then proves a grouping atom exists whenever some
/// declared level could ever require one.
fn emitted_pretty_declared_behavior(
    node: &NodeParsePlan,
    declaration_variants: &[VariantParsePlan],
    variants: &[Vec<PrettyElementBehavior>],
    grouping_syntax: &PrettyGroupingSyntax,
) -> Result<Option<PrettyDeclaredBehavior>, String> {
    let applications = node
        .parse_applications
        .iter()
        .map(|application| application.application)
        .collect::<Vec<_>>();
    let mut has_recursive_operand = false;
    let mut is_atom = Vec::with_capacity(declaration_variants.len());
    let mut variant_behaviors = Vec::with_capacity(declaration_variants.len());
    for variant in declaration_variants {
        let len = variant.elements.len();
        let mut operands = Vec::new();
        let mut has_edge_operand = false;
        for (index, element) in variant.elements.iter().enumerate() {
            let ElementParseKind::Semantic { projection, .. } = &element.kind else {
                continue;
            };
            let SemanticParseProjection::Direct {
                terminal_node,
                attachment,
                ..
            } = projection.as_ref()
            else {
                continue;
            };
            if *terminal_node != Some(node.node) {
                operands.push(PrettyDeclaredOperandBehavior::Ordinary);
                continue;
            }
            has_recursive_operand = true;
            let before_empty = attachment
                .as_ref()
                .is_none_or(|attachment| attachment.before.is_empty());
            let after_empty = attachment
                .as_ref()
                .is_none_or(|attachment| attachment.after.is_empty());
            let position = DeclaredOperandPosition::classify(
                index == 0 && before_empty,
                index + 1 == len && after_empty,
            );
            has_edge_operand |= !matches!(position, DeclaredOperandPosition::Interior);
            let (minimum, next) =
                pretty_declared_operand_bound(variant.declared_precedence, position);
            operands.push(PrettyDeclaredOperandBehavior::Recursive {
                owner: node.node,
                applications: applications.clone(),
                minimum,
                next,
            });
        }
        is_atom.push(!has_edge_operand);
        variant_behaviors.push(PrettyDeclaredVariantBehavior {
            level: variant.declared_precedence.map(|level| level.ordinal),
            operands,
        });
    }
    if !has_recursive_operand {
        return Ok(None);
    }
    let requires_grouping = is_atom.contains(&false)
        && declaration_variants
            .iter()
            .any(|variant| variant.declared_precedence.is_some());
    let grouping = pretty_grouping(
        node.node,
        declaration_variants,
        variants,
        &is_atom,
        grouping_syntax,
    );
    if requires_grouping && grouping.is_none() {
        return Err(format!(
            "Pretty cannot repair precedence for Node `{}` because its grammar has no atom that encloses Self between fixed tokens",
            node.ident,
        ));
    }
    Ok(Some(PrettyDeclaredBehavior {
        grouping,
        variants: variant_behaviors,
    }))
}

/// Extracts the explicit grouping variant's before/after delimiter behavior.
///
/// `is_atom` selects, in declaration variant order, every variant with no self-recursive operand playing an operator
/// role or a declared-precedence first/last-symbol role, since only such a variant can be the
/// grouping atom that lets Pretty repair precedence with parentheses. [`agreed_grouping`] decides
/// between the proven candidates.
pub(super) fn pretty_grouping(
    owner: NodeId,
    declaration_variants: &[VariantParsePlan],
    behaviors: &[Vec<PrettyElementBehavior>],
    is_atom: &[bool],
    grouping_syntax: &PrettyGroupingSyntax,
) -> Option<PrettyGrouping> {
    let mut analyzer = PrettyGroupingAnalyzer::new(grouping_syntax);
    let candidates = declaration_variants
        .iter()
        .zip(behaviors)
        .zip(is_atom)
        .filter(|(_, is_atom)| **is_atom)
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
    agreed_grouping(&candidates)
}

/// Selects the one enclosure every proven candidate atom agrees on.
///
/// A candidate that reaches the recursive owner directly outranks one that reaches it through a
/// helper Node, because a direct witness is the enum's own authored grouping variant. Among the
/// outranking candidates the enclosure must be unanimous: equal `before` and `after` sequences,
/// compared by what they render — spelling, mechanical spacing class and placement — rather than
/// by the atom or the token kind they come from. Two atoms that delegate to witnesses rendering
/// the same enclosure are therefore one answer, which is what lets an atom name a `lookahead`
/// merged kind in a sibling arm, while atoms that disagree leave the Node without grouping
/// syntax and `pretty_declared_behavior` reports the enclosure-proof error. Pretty never picks
/// one of several disagreeing enclosures silently (#142).
fn agreed_grouping(candidates: &[PrettyGroupingWitness]) -> Option<PrettyGrouping> {
    let has_direct = candidates.iter().any(|candidate| !candidate.delegated);
    let mut agreed: Option<&PrettyGrouping> = None;
    for candidate in candidates {
        // With no direct candidate every candidate is delegated, so this discards nothing.
        if has_direct && candidate.delegated {
            continue;
        }
        match agreed {
            Some(previous) if *previous != candidate.grouping => return None,
            Some(_) => {}
            None => agreed = Some(&candidate.grouping),
        }
    }
    agreed.cloned()
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
                        separator_spacing,
                        opaque_provenance,
                        repetition_break,
                        pretty,
                        ..
                    } => PrettySemanticProjection::Direct {
                        repetition: pretty_repetition_behavior(
                            matches!(container.as_ref(), PlannedContainer::Repeated { .. }),
                            separator.clone().map(|spelling| {
                                (spelling, separator_spacing.unwrap_or(PrettySpacing::Auto))
                            }),
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
    separator: Option<(String, PrettySpacing)>,
    hard_break: bool,
    pretty: &PrettyHints,
) -> Option<PrettyRepetitionBehavior> {
    repeated.then(|| {
        let (space_before_separator, space_after_separator) =
            separator
                .as_ref()
                .map_or(
                    (false, true),
                    |(spelling, spacing)| match pretty_fixed_rendering(spelling, *spacing) {
                        PrettyFixedRendering::TightSequence | PrettyFixedRendering::Opening => {
                            (false, false)
                        }
                        PrettyFixedRendering::Before => (true, false),
                        PrettyFixedRendering::After
                        | PrettyFixedRendering::Closing
                        | PrettyFixedRendering::Punctuation => (false, true),
                        PrettyFixedRendering::Around | PrettyFixedRendering::Word => (true, true),
                    },
                );
        PrettyRepetitionBehavior {
            separator: separator.map(|(spelling, _)| spelling),
            space_before_separator,
            space_after_separator,
            trailing_separator: matches!(
                pretty.trailing_separator,
                Some(PrettyTrailingSeparator::Always)
            ),
            hard_break,
            leading_separator: matches!(pretty.separator, Some(PrettySeparatorPosition::Leading)),
        }
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
        PrettySpacing::Auto if matches!(spelling, ")" | "]" | "}") => PrettyFixedRendering::Closing,
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
        return PrettySequenceLayout::Delimited;
    }
    let delimited_after_prefix = before
        .last()
        .and_then(|open| match open {
            PrettyAttachedSyntaxBehavior::Required(open) => delimiter_close(&open.spelling),
            PrettyAttachedSyntaxBehavior::Optional(_) => None,
        })
        .is_some_and(|close| {
            after.first().is_some_and(|fixed| {
                matches!(fixed, PrettyAttachedSyntaxBehavior::Required(fixed) if fixed.spelling == close)
            })
        });
    if delimited_after_prefix {
        PrettySequenceLayout::DelimitedAfterPrefix
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
) -> Result<PrettyGroupingSyntax, String> {
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
    Ok(PrettyGroupingSyntax { nodes })
}

/// Projects one authoritative source element into grouping-proof syntax.
fn authoritative_pretty_grouping_element(
    resolved_node: &ResolvedNode,
    element: &GrammarElement,
    behavior: &PrettyElementBehavior,
    resolved: &ResolvedSourceGrammar,
) -> Result<PrettyGroupingElement, String> {
    match (element, behavior) {
        (
            GrammarElement::FixedToken { .. },
            PrettyElementBehavior::Fixed {
                spelling,
                spacing,
                rendering,
                ..
            },
        ) => Ok(PrettyGroupingElement::Fixed(PrettyAttachedFixedBehavior {
            spelling: spelling.clone(),
            spacing: *spacing,
            rendering: *rendering,
        })),
        (
            GrammarElement::OptionalFixedTokenGroup { .. },
            PrettyElementBehavior::OptionalFixedTokenGroup { .. },
        ) => Ok(PrettyGroupingElement::Optional),
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
                Ok(PrettyGroupingElement::Semantic {
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
                Ok(PrettyGroupingElement::Opaque)
            }
        },
        _ => Ok(PrettyGroupingElement::Opaque),
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
    grouping_syntax: &PrettyGroupingSyntax,
) -> Result<PrettyNodeBehavior, String> {
    let variant_pretty = match &source_node.body {
        SourceNodeBody::Struct(_) | SourceNodeBody::Presence(_) => vec![PrettyHints::default()],
        SourceNodeBody::Enum(variants) => variants
            .iter()
            .map(|variant| variant.pretty.clone())
            .collect(),
    };
    let variants = match &source_node.body {
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
    let declared = match &source_node.body {
        SourceNodeBody::Enum(declaration_variants) => authoritative_pretty_declared_behavior(
            lowered,
            resolved_node,
            declaration_variants,
            &applications,
            &variants,
            grouping_syntax,
        )?,
        SourceNodeBody::Struct(_) | SourceNodeBody::Presence(_) => None,
    };
    Ok(PrettyNodeBehavior {
        node: resolved_node.id,
        applications,
        pretty: source_node.pretty.clone(),
        variant_pretty,
        variant_layouts,
        variants,
        declared,
    })
}

/// Reconstructs declared-precedence grouping and operand behavior from authored source
/// correlations, independent of the frozen emitted parse plan, or `None` when no variant has a
/// self-recursive operand.
///
/// # Long-function contract
///
/// Rationale: mirrors [`emitted_pretty_declared_behavior`] from raw source structure so the two
/// can never silently drift; both call the shared [`declared_variant_level`] and
/// [`pretty_declared_operand_bound`] decision functions.
/// Invariants: a variant's own level and every operand's rule-boundary position are derived
/// purely from its authored element sequence.
/// Phases: classifies each variant's operands, then proves a grouping atom exists whenever some
/// declared level could ever require one.
fn authoritative_pretty_declared_behavior(
    lowered: &crate::materialization::LoweredGrammar,
    resolved_node: &ResolvedNode,
    variants: &[crate::source_model::Variant],
    applications: &[MaterializedApplicationId],
    behaviors: &[Vec<PrettyElementBehavior>],
    grouping_syntax: &PrettyGroupingSyntax,
) -> Result<Option<PrettyDeclaredBehavior>, String> {
    let source = lowered.source_modeled.source_model();
    let resolved = lowered.source_modeled.resolved();
    let mut has_recursive_operand = false;
    let mut is_atom = Vec::with_capacity(variants.len());
    let mut levels = Vec::with_capacity(variants.len());
    let mut variant_behaviors = Vec::with_capacity(variants.len());
    for variant in variants {
        let level = declared_variant_level(
            source,
            resolved_node.id,
            variant.resolved_variant,
            &variant.elements,
        );
        let len = variant.elements.len();
        let mut operands = Vec::new();
        let mut has_edge_operand = false;
        for (index, element) in variant.elements.iter().enumerate() {
            let GrammarElement::SemanticField {
                resolved_field,
                attachment,
                projection: SemanticFieldProjection::Direct,
                ..
            } = element
            else {
                // A standalone `Fixed` or `OptionalFixedTokenGroup` element, or a boolean
                // `FixedPresence` field, is never a recursive operand; leave `operands` alone,
                // matching how `render_declared_enum_pretty` never advances its own operand
                // index for one either.
                continue;
            };
            let field = resolved_node
                .fields
                .iter()
                .find(|field| field.id == *resolved_field)
                .ok_or_else(|| {
                    "a declared-precedence Pretty operand lacks resolved syntax".to_owned()
                })?;
            let projection = authored_projection(&field.ty, resolved)?;
            if projection.terminal_node != Some(resolved_node.id) {
                operands.push(PrettyDeclaredOperandBehavior::Ordinary);
                continue;
            }
            has_recursive_operand = true;
            let before_empty = attachment
                .as_ref()
                .is_none_or(|attachment| attachment.before.is_empty());
            let after_empty = attachment
                .as_ref()
                .is_none_or(|attachment| attachment.after.is_empty());
            let position = DeclaredOperandPosition::classify(
                index == 0 && before_empty,
                index + 1 == len && after_empty,
            );
            has_edge_operand |= !matches!(position, DeclaredOperandPosition::Interior);
            let (minimum, next) = pretty_declared_operand_bound(level, position);
            operands.push(PrettyDeclaredOperandBehavior::Recursive {
                owner: resolved_node.id,
                applications: applications.to_vec(),
                minimum,
                next,
            });
        }
        is_atom.push(!has_edge_operand);
        levels.push(level);
        variant_behaviors.push(PrettyDeclaredVariantBehavior {
            level: level.map(|level| level.ordinal),
            operands,
        });
    }
    if !has_recursive_operand {
        return Ok(None);
    }
    let requires_grouping = is_atom.contains(&false) && levels.iter().any(Option::is_some);
    let variant_elements = variants
        .iter()
        .map(|variant| variant.elements.as_slice())
        .collect::<Vec<_>>();
    let grouping = authoritative_pretty_grouping(
        resolved_node,
        &variant_elements,
        behaviors,
        &is_atom,
        resolved,
        grouping_syntax,
    )?;
    if requires_grouping && grouping.is_none() {
        return Err(format!(
            "Pretty cannot repair precedence for Node `{}` because its grammar has no atom that encloses Self between fixed tokens",
            resolved_node.ident,
        ));
    }
    Ok(Some(PrettyDeclaredBehavior {
        grouping,
        variants: variant_behaviors,
    }))
}

/// Reconstructs explicit grouping delimiters from authored syntax.
///
/// `is_atom` selects, in declaration variant order, every variant with no self-recursive operand playing an operator
/// role or a declared-precedence first/last-symbol role. [`agreed_grouping`] decides between the
/// proven candidates, exactly as the emitted model does.
pub(super) fn authoritative_pretty_grouping(
    resolved_node: &ResolvedNode,
    variant_elements: &[&[GrammarElement]],
    behaviors: &[Vec<PrettyElementBehavior>],
    is_atom: &[bool],
    resolved: &ResolvedSourceGrammar,
    grouping_syntax: &PrettyGroupingSyntax,
) -> Result<Option<PrettyGrouping>, String> {
    let mut analyzer = PrettyGroupingAnalyzer::new(grouping_syntax);
    let mut candidates = Vec::new();
    for ((elements, behavior), is_atom) in variant_elements.iter().zip(behaviors).zip(is_atom) {
        if !is_atom {
            continue;
        }
        let elements = elements
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
    Ok(agreed_grouping(&candidates))
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
                let separator_behavior =
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
                            separator_behavior,
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
) -> Result<Option<(String, PrettySpacing)>, String> {
    let Some(separator) = separator else {
        return Ok(None);
    };
    let source = lowered.source_modeled.source_model();
    let usage = &source.token_uses[separator.token_use.index()];
    let definition = &source.token_definitions[usage.definition.index()];
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("a Pretty separator is not fixed syntax".to_owned());
    };
    Ok(Some((
        fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence),
        fixed.pretty_spacing,
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

/// Renders precedence-aware `Pretty` behavior for one plain enum Node whose grammar declares a
/// `precedence { ... }` block and that has a self-recursive operand.
///
/// # Long-function contract
///
/// Rationale: keeps declared-precedence parenthesization beside typed Pretty behavior, in one
/// self-checking, context-threading shape.
/// Invariants: rendered precedence and associativity mirror [`PrettyDeclaredBehavior`] without
/// consulting runtime grammar metadata.
/// Phases: selects the active variant, renders operands and attachments, then applies grouping.
pub(super) fn render_declared_enum_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    access: PrettyAccess,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &access.target_ident(node);
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Node behavior");
    let declared_behavior = behavior
        .declared
        .as_ref()
        .expect("validated declared-precedence Pretty behavior");
    let frame = access.frame(plan, node);
    let PrettyFrame {
        inherent_generics,
        type_generics,
        inherent_where,
        method_generics,
        method_where,
        method_parameters,
        method_arguments,
    } = &frame;
    let variants = trait_variants(&node.body);
    let variant_count = variants.len();
    let capable = pretty_leading_level_nodes(plan);
    let frame = render_grouping_frame(plan, node, node_index, &quote!(__recursa_body(context)?;));
    let restrictions = plan.restrictions.of(node.node);
    let repairable = repairable_restrictions(plan, node, node_index);
    // The `leading` restrictions, which keep the base on an operand that ends a rule.
    let leading_only = restrictions
        .iter()
        .enumerate()
        .filter(|(_, restriction)| restriction.leading_only)
        .map(|(index, _)| u16::try_from(index).expect("a restriction index fits u16"))
        .collect::<Vec<_>>();
    let rendered_variants = variants
        .iter()
        .zip(&behavior.variant_layouts)
        .zip(&behavior.variants)
        .zip(&behavior.variant_pretty)
        .zip(&declared_behavior.variants)
        .enumerate()
        .map(
            |(variant_index, ((((variant, layout), behavior), variant_pretty), declared_variant))| {
            let (pattern, bindings) = variant_bound_pattern(node_ident, variant, "pretty_declared");
            // The token after the first operand, which the parser compares with the operand's
            // rule when it decides to reduce it (`docs/declared-precedence.md`, "Consumers").
            let lookahead =
                render_first_operand_lookahead(variant, node.node, &bindings, access, &capable);
            // The operands a restriction retypes in this variant receive the restriction the
            // value was rendered under (`docs/declared-precedence.md`, "Restricted expressions").
            let edges = restricted_edges(variant, node.node);
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
                        let operand = &declared_variant.operands[semantic_index];
                        semantic_index += 1;
                        match operand {
                            PrettyDeclaredOperandBehavior::Ordinary => {
                                let followed = render_field_follow(
                                    plan,
                                    &variant.elements,
                                    index,
                                    |holder| element_binding(&variant.elements, &bindings, holder),
                                    access,
                                    &capable,
                                );
                                render_pretty_element(
                                    plan,
                                    index,
                                    element,
                                    behavior,
                                    Some((quote!(#binding), true)),
                                    access,
                                    followed,
                                )
                            }
                            PrettyDeclaredOperandBehavior::Recursive { minimum, next, .. } => {
                                let minimum_behavior = minimum;
                                // A recursive operand is `Box<Self>`, or the attached optional
                                // `#[tok(X, this)] Option<Box<Self>>`, whose absence renders
                                // nothing, attachment included.
                                let optional = matches!(
                                    container.as_ref(),
                                    PlannedContainer::Optional
                                );
                                assert!(
                                    (optional
                                        || matches!(
                                            container.as_ref(),
                                            PlannedContainer::Direct
                                        ))
                                        && wrappers.iter().all(|wrapper| {
                                            matches!(wrapper, FieldWrapper::Boxed)
                                                || (optional
                                                    && matches!(wrapper, FieldWrapper::Optional))
                                        }),
                                    "validated recursive declared-precedence operands are direct or attached optionals and may only use Box storage wrappers",
                                );
                                let restriction = match parse_projection.as_ref() {
                                    SemanticParseProjection::Direct {
                                        restriction: Some(restriction),
                                        ..
                                    } => quote!(::core::option::Option::Some(#restriction)),
                                    _ if edges.first == Some(index) => {
                                        quote!(__recursa_inner_restriction)
                                    }
                                    _ if edges.last == Some(index) && leading_only.is_empty() => {
                                        quote!(__recursa_inner_restriction)
                                    }
                                    _ if edges.last == Some(index) => quote! {
                                        __recursa_inner_restriction
                                            .filter(|restriction| !matches!(restriction, #(#leading_only)|*))
                                    },
                                    _ => quote!(::core::option::Option::None),
                                };
                                let wrappers = &access.value_wrappers(wrappers);
                                let minimum = render_pretty_declared_minimum(*minimum);
                                // An interior operand that ends a rule of this Node in some
                                // derivation is also a last operand there.
                                let minimum = match (
                                    *minimum_behavior,
                                    shadow_minimum(facade, &variants, variant_index, index),
                                ) {
                                    (PrettyDeclaredMinimum::None, Some(shadow)) => shadow,
                                    _ => minimum,
                                };
                                let next = render_pretty_declared_next(*next);
                                let next = match (&lookahead, index) {
                                    (Some(lookahead), 0) => quote! {
                                        #facade::__private::pretty_stricter_bound(#next, #lookahead)
                                    },
                                    _ => next,
                                };
                                let operand = if optional {
                                    quote!(operand)
                                } else {
                                    quote!(#binding)
                                };
                                let semantic = access.declared_recursion(
                                    operand,
                                    &minimum,
                                    &next,
                                    &restriction,
                                );
                                let rendered = if let Some(attachment) = attachment {
                                    // An attached operator token has no fixed neighbor in the
                                    // enclosing document group the way a standalone
                                    // `#[tok(TOKEN)]` element does, so it carries no automatic
                                    // separating space. A token with a rule sibling on its
                                    // attached side (an infix or the far side of a postfix
                                    // token with more syntax following) needs one synthesized;
                                    // a token with no sibling there (a prefix token, or the
                                    // grouping atom's own delimiters) stays tight.
                                    let element_is_first = index == 0;
                                    let element_is_last = index + 1 == variant.elements.len();
                                    let before_has_sibling =
                                        !attachment.before.is_empty() && !element_is_first;
                                    let after_has_sibling =
                                        !attachment.after.is_empty() && !element_is_last;
                                    let unit = if attachment.layout
                                        == PrettySequenceLayout::Delimited
                                    {
                                        render_pretty_attachment_unit(
                                            attachment,
                                            *provenance,
                                            semantic,
                                        )
                                    } else {
                                        let before_len = attachment.before.len();
                                        let before_tokens = attachment
                                            .before
                                            .iter()
                                            .enumerate()
                                            .map(|(index, syntax)| {
                                                if element_is_first && index + 1 == before_len {
                                                    render_pretty_attached_syntax_as_opening(
                                                        index, syntax,
                                                    )
                                                } else {
                                                    render_pretty_attached_syntax(index, syntax)
                                                }
                                            })
                                            .collect::<Vec<_>>();
                                        let semantic_index = before_tokens.len();
                                        let child = pretty_provenance_child_method(*provenance);
                                        let semantic_rendered = render_pretty_context_scope(
                                            &child,
                                            quote!(#semantic_index),
                                            semantic,
                                        );
                                        let semantic_rendered = quote!(#semantic_rendered?;);
                                        let after_tokens = attachment
                                            .after
                                            .iter()
                                            .enumerate()
                                            .map(|(index, syntax)| {
                                                render_pretty_attached_syntax(
                                                    semantic_index + 1 + index,
                                                    syntax,
                                                )
                                            })
                                            .collect::<Vec<_>>();
                                        let space_after_before = before_has_sibling
                                            .then(|| quote!(context.separating_space()?;));
                                        let space_before_after = after_has_sibling
                                            .then(|| quote!(context.separating_space()?;));
                                        quote! {
                                            #(#before_tokens)*
                                            #space_after_before
                                            #semantic_rendered
                                            #space_before_after
                                            #(#after_tokens)*
                                        }
                                    };
                                    let leading_space = before_has_sibling
                                        .then(|| quote!(context.separating_space()?;));
                                    let inner = render_pretty_context_scope(
                                        &format_ident!("child_scope"),
                                        quote!(0),
                                        quote!(#unit Ok(())),
                                    );
                                    let outer = render_pretty_context_scope(
                                        &format_ident!("child_scope"),
                                        quote!(#index),
                                        inner,
                                    );
                                    quote!(#leading_space #outer?;)
                                } else {
                                    let child = pretty_provenance_child_method(*provenance);
                                    let inner = render_pretty_context_scope(
                                        &child,
                                        quote!(0),
                                        semantic,
                                    );
                                    let outer = render_pretty_context_scope(
                                        &format_ident!("child_scope"),
                                        quote!(#index),
                                        inner,
                                    );
                                    quote!(#outer?;)
                                };
                                let rendered = if optional {
                                    let view = render_optional_view_reference(
                                        quote!(#binding),
                                        wrappers,
                                        true,
                                    );
                                    quote! {
                                        if let Some(operand) = #view {
                                            #rendered
                                        }
                                    }
                                } else {
                                    rendered
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
                    _ => unreachable!("validated declared-precedence Pretty semantic projection"),
                },
                (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
                | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. })
                | (ElementParseKind::OptionalFixedTokenGroup { .. }, _)
                | (_, PrettyElementBehavior::OptionalFixedTokenGroup { .. }) => {
                    unreachable!("validated declared-precedence Pretty element behavior")
                }
            })
            .collect::<Vec<_>>();
            let rendered = render_pretty_sequence(*layout, &rendered);
            let rendered = render_pretty_hints(variant_pretty, rendered);
            // A variant with no declared level is never parenthesized, so the whole test folds
            // to `false` here rather than to a `false || false` the downstream crate's
            // deny-by-default `clippy::eq_op` would reject.
            // The terms of the parenthesization test. A variant with no term folds the test to
            // `false`, never to a comparison of a literal with itself, which the downstream
            // crate's deny-by-default `clippy::eq_op` would reject.
            let mut parenthesize_terms = Vec::new();
            if let Some(level) = declared_variant.level {
                let bound = |bound: proc_macro2::TokenStream| {
                    quote! {
                        #bound.is_some_and(|(candidate, tie)| {
                            #level < candidate || (#level == candidate && tie)
                        })
                    }
                };
                parenthesize_terms.push(bound(quote!(min)));
                parenthesize_terms.push(bound(quote!(next)));
            }
            // As the last operand of its parent, a value whose first symbol is an operand is
            // also parenthesized when the parser would reduce the parent before the token after
            // that operand: the token's level is below the parent's bound.
            if let Some(lookahead) = &lookahead {
                parenthesize_terms.push(quote! {
                    (#lookahead).is_some_and(|(lookahead, _)| {
                        min.is_some_and(|(candidate, tie)| {
                            lookahead < candidate || (lookahead == candidate && tie)
                        })
                    })
                });
            }
            let group_open = declared_behavior.grouping.as_ref().map(|grouping| {
                grouping
                    .before
                    .iter()
                    .map(|fixed| {
                        render_pretty_synthetic_fixed(plan, &fixed.spelling, fixed.rendering)
                    })
                    .collect::<proc_macro2::TokenStream>()
            });
            let group_close = declared_behavior.grouping.as_ref().map(|grouping| {
                grouping
                    .after
                    .iter()
                    .map(|fixed| {
                        render_pretty_synthetic_fixed(plan, &fixed.spelling, fixed.rendering)
                    })
                    .collect::<proc_macro2::TokenStream>()
            });
            // A value whose variant the restriction refuses is parenthesized, when the restriction
            // admits the grouping atom: the atom's interior is the base again.
            let inadmissible = restrictions
                .iter()
                .enumerate()
                .filter(|(index, restriction)| {
                    repairable.contains(index)
                        && !restriction.admitted.get(variant_index).copied().unwrap_or(true)
                })
                .map(|(index, _)| u16::try_from(index).expect("a restriction index fits u16"))
                .collect::<Vec<_>>();
            if !inadmissible.is_empty() {
                parenthesize_terms.push(quote! {
                    matches!(restriction, ::core::option::Option::Some(#(#inadmissible)|*))
                });
            }
            let parenthesize_check = if parenthesize_terms.is_empty() {
                quote!(false)
            } else {
                quote!(#(#parenthesize_terms)||*)
            };
            let helper_body = match &frame {
                // The synthesized parentheses render through the frame of the grouping atom, so
                // they lay out as the atom of a parsed value does.
                Some(frame) => quote! {
                    let __recursa_parenthesize = #parenthesize_check;
                    let __recursa_declared_inner_min = if __recursa_parenthesize { None } else { min };
                    let __recursa_declared_inner_next = if __recursa_parenthesize { None } else { next };
                    let __recursa_inner_restriction = if __recursa_parenthesize { None } else { restriction };
                    let __recursa_body = |context: &mut #facade::PrettyContext| -> ::core::fmt::Result {
                        #rendered
                        Ok(())
                    };
                    if __recursa_parenthesize {
                        #frame
                    } else {
                        __recursa_body(context)?;
                    }
                    Ok(())
                },
                None => quote! {
                    let __recursa_parenthesize = #parenthesize_check;
                    if __recursa_parenthesize {
                        #group_open
                    }
                    let __recursa_declared_inner_min = if __recursa_parenthesize { None } else { min };
                    let __recursa_declared_inner_next = if __recursa_parenthesize { None } else { next };
                    let __recursa_inner_restriction = if __recursa_parenthesize { None } else { restriction };
                    #rendered
                    if __recursa_parenthesize {
                        #group_close
                    }
                    Ok(())
                },
            };
            let helper = format_ident!(
                "__recursa_pretty_declared_variant_{node_index}_{variant_index}"
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
                        _ => unreachable!("declared-precedence Pretty dispatched to the wrong variant helper"),
                    }
                }
            };
            let variant_pattern = variant_pattern(node_ident, &variant.ident, variant.style);
            (
                quote! {
                    #variant_pattern => self.#helper(#method_arguments context, min, next, restriction)
                },
                quote! {
                    fn #helper #method_generics(
                        &self,
                        #method_parameters
                        context: &mut #facade::PrettyContext,
                        min: ::core::option::Option<(u32, bool)>,
                        next: ::core::option::Option<(u32, bool)>,
                        restriction: ::core::option::Option<u16>,
                    ) -> ::core::fmt::Result #method_where {
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
        quote!(self.__recursa_pretty_declared(
            #method_arguments
            context,
            ::core::option::Option::None,
            ::core::option::Option::None,
            ::core::option::Option::None,
        )?;),
    );
    let restricted_node = render_pretty_hints(
        &behavior.pretty,
        quote!(self.__recursa_pretty_declared(
            #method_arguments
            context,
            ::core::option::Option::None,
            ::core::option::Option::None,
            ::core::option::Option::Some(restriction),
        )?;),
    );
    let restricted_entry = (!restrictions.is_empty())
        .then(|| render_pretty_restricted_entry(plan, node, access, restricted_node));
    let followed_node = render_pretty_hints(
        &behavior.pretty,
        quote!(self.__recursa_pretty_declared(
            #method_arguments
            context,
            ::core::option::Option::None,
            next,
            ::core::option::Option::None,
        )?;),
    );
    let followed_entry = render_pretty_followed_entry(plan, node, access, followed_node);
    let trait_impl = render_pretty_trait_impl(plan, node, access, &behavior.pretty, rendered_node);
    let leading = render_pretty_leading_level(plan, node, access, &capable);
    quote! {
        #leading

        impl #inherent_generics #node_ident #type_generics #inherent_where {
            fn __recursa_pretty_declared #method_generics(
                &self,
                #method_parameters
                context: &mut #facade::PrettyContext,
                min: ::core::option::Option<(u32, bool)>,
                next: ::core::option::Option<(u32, bool)>,
                restriction: ::core::option::Option<u16>,
            ) -> ::core::fmt::Result #method_where {
                match self { #(#arms),* }
            }

            #(#helper_methods)*

            #restricted_entry

            #followed_entry
        }

        #trait_impl
    }
}

/// Renders the left-edge precedence bound expression for a recursive declared-precedence
/// operand.
pub(super) fn render_pretty_declared_minimum(
    minimum: PrettyDeclaredMinimum,
) -> proc_macro2::TokenStream {
    match minimum {
        PrettyDeclaredMinimum::None => quote!(::core::option::Option::None),
        PrettyDeclaredMinimum::Inner => quote!(__recursa_declared_inner_min),
        PrettyDeclaredMinimum::Binding(bound) => {
            let level = bound.level;
            let tie = bound.tie_parenthesizes;
            quote!(::core::option::Option::Some((#level, #tie)))
        }
    }
}

/// Renders the right-edge precedence bound expression exposed after a recursive
/// declared-precedence operand.
pub(super) fn render_pretty_declared_next(next: PrettyDeclaredNext) -> proc_macro2::TokenStream {
    match next {
        PrettyDeclaredNext::None => quote!(::core::option::Option::None),
        PrettyDeclaredNext::Inner => quote!(__recursa_declared_inner_next),
        PrettyDeclaredNext::Binding(bound) => {
            let level = bound.level;
            let tie = bound.tie_parenthesizes;
            quote!(::core::option::Option::Some((#level, #tie)))
        }
    }
}

/// How one semantic field calls the Pretty of its value.
pub(super) enum FieldDispatch {
    /// The value's ordinary `Pretty`.
    Plain,
    /// The restricted entry of a declared base, under the restriction the field names.
    Restricted(u16),
    /// The followed entry of a declared base, with the bound of the token after the field: an
    /// expression of type `Option<(u32, bool)>` (`docs/declared-precedence.md`, "Consumers").
    Followed(proc_macro2::TokenStream),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Which representation one generated Pretty body reads its values out of.
///
/// Every layout decision — spacing, breaks, groups, separators, presence and declared-precedence
/// parenthesization — is shared, because both representations render the same
/// [`PrettyNodeBehavior`]. Only two things differ: a nested field owns its child and renders it
/// through the `Pretty` trait, while a flat field holds a four-byte handle that only the result's
/// stores can resolve.
pub(super) enum PrettyAccess {
    /// The nested AST: a field owns its child and renders through `Pretty`.
    Nested,
    /// A Flat AST: a field holds a handle and renders through the result's stores.
    Flat,
}

impl PrettyAccess {
    /// Returns the type one rendered implementation and its patterns name.
    pub(super) fn target_ident(self, node: &NodeParsePlan) -> Ident {
        match self {
            Self::Nested => node.ident.clone(),
            Self::Flat => flat_type_ident(&node.ident),
        }
    }

    /// Returns the storage wrappers a value access of this representation actually spells.
    ///
    /// A flat field has no `Box` layer: the design's field-mapping table maps `boxed!(T)` and `T`
    /// to the same handle, so a flat access never dereferences one.
    fn value_wrappers(self, wrappers: &[FieldWrapper]) -> Vec<FieldWrapper> {
        match self {
            Self::Nested => wrappers.to_vec(),
            Self::Flat => wrappers
                .iter()
                .copied()
                .filter(|wrapper| !matches!(wrapper, FieldWrapper::Boxed))
                .collect(),
        }
    }

    /// Renders the child dispatch of one semantic value.
    fn dispatch(
        self,
        plan: &EmissionPlan,
        value: proc_macro2::TokenStream,
        content_token: bool,
    ) -> proc_macro2::TokenStream {
        let facade = &plan.facade;
        let module_anchor = &plan.module_anchor;
        match (self, content_token) {
            (Self::Nested, true) => quote! {
                <_ as #module_anchor::__RecursaPrettyWith>
                    ::__recursa_pretty_with(#value, context)
            },
            (Self::Nested, false) => quote!(#facade::Pretty::pretty(#value, context)),
            // A flat child is a handle, an optional handle, a list handle or a text handle, and
            // one generated container trait resolves every one of them against the store.
            (Self::Flat, _) => flat_pretty_dispatch(plan, value),
        }
    }

    /// Renders the item iteration of one repeated field's container.
    fn iterate(self, collection: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
        match self {
            Self::Nested => quote!((#collection).iter()),
            Self::Flat => flat_pretty_iterate(collection),
        }
    }

    /// Renders the container of one repeated field, ready to iterate.
    ///
    /// A nested container is borrowed in place. A flat one is a `Copy` list handle, which the
    /// store resolves into a run, so it is taken by value instead.
    fn collection(
        self,
        value: proc_macro2::TokenStream,
        wrappers: &[FieldWrapper],
        value_is_reference: bool,
    ) -> proc_macro2::TokenStream {
        match self {
            Self::Nested => render_terminal_reference(value, wrappers, value_is_reference),
            Self::Flat => flat_pretty_collection(value, value_is_reference),
        }
    }

    /// Renders the optional container of one repeated field, ready to test and iterate.
    fn optional_collection(
        self,
        value: proc_macro2::TokenStream,
        wrappers: &[FieldWrapper],
        value_is_reference: bool,
    ) -> proc_macro2::TokenStream {
        match self {
            Self::Nested => render_optional_view_reference(value, wrappers, value_is_reference),
            Self::Flat => flat_pretty_collection(value, value_is_reference),
        }
    }

    /// Renders the recursive call one declared-precedence operand makes on its own family.
    fn declared_recursion(
        self,
        operand: proc_macro2::TokenStream,
        minimum: &proc_macro2::TokenStream,
        next: &proc_macro2::TokenStream,
        restriction: &proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        match self {
            Self::Nested => quote! {
                (#operand).__recursa_pretty_declared(context, #minimum, #next, #restriction)
            },
            Self::Flat => flat_pretty_declared_recursion(operand, minimum, next, restriction),
        }
    }

    /// Renders the call of one field that names a restricted expression of a declared base: the
    /// base's restricted entry, which writes the parentheses the site needs.
    fn restricted_dispatch(
        self,
        value: proc_macro2::TokenStream,
        restriction: u16,
    ) -> proc_macro2::TokenStream {
        match self {
            Self::Nested => quote!((#value).__recursa_pretty_restricted(context, #restriction)),
            // The value is a reference to a handle; the binding keeps the lint
            // `clippy::deref_addrof` off a field the caller borrows in place.
            Self::Flat => quote! {
                {
                    let __recursa_handle: &_ = #value;
                    __recursa_ast[*__recursa_handle].__recursa_pretty_restricted(
                        __recursa_ast,
                        context,
                        #restriction,
                    )
                }
            },
        }
    }

    /// Renders the call of one field that holds a declared base directly and that a token with
    /// a level follows: the base's followed entry, which writes the parentheses that keep the
    /// parser from shifting that token into the value.
    fn followed_dispatch(
        self,
        value: proc_macro2::TokenStream,
        next: &proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        match self {
            Self::Nested => quote!((#value).__recursa_pretty_followed(context, #next)),
            Self::Flat => quote! {
                {
                    let __recursa_handle: &_ = #value;
                    __recursa_ast[*__recursa_handle].__recursa_pretty_followed(
                        __recursa_ast,
                        context,
                        #next,
                    )
                }
            },
        }
    }

    /// Renders the call of one semantic field value, as its [`FieldDispatch`] selects.
    fn field_dispatch(
        self,
        plan: &EmissionPlan,
        value: proc_macro2::TokenStream,
        content_token: bool,
        field: &FieldDispatch,
    ) -> proc_macro2::TokenStream {
        match field {
            FieldDispatch::Plain => self.dispatch(plan, value, content_token),
            FieldDispatch::Restricted(restriction) => self.restricted_dispatch(value, *restriction),
            FieldDispatch::Followed(next) => self.followed_dispatch(value, next),
        }
    }

    /// Returns the frame the private declared-precedence helpers are assembled into.
    fn frame(self, plan: &EmissionPlan, node: &NodeParsePlan) -> PrettyFrame {
        match self {
            Self::Nested => {
                let (inherent_generics, type_generics, inherent_where) =
                    pretty_impl_parts(&plan.facade, node);
                PrettyFrame {
                    inherent_generics,
                    type_generics,
                    inherent_where,
                    method_generics: proc_macro2::TokenStream::new(),
                    method_where: proc_macro2::TokenStream::new(),
                    method_parameters: proc_macro2::TokenStream::new(),
                    method_arguments: proc_macro2::TokenStream::new(),
                }
            }
            Self::Flat => flat_pretty_frame(plan, node),
        }
    }
}

/// The representation-specific frame one Node's private Pretty helpers are assembled into.
///
/// A nested helper needs nothing but the value, so every method-level field is empty and the
/// implementation block carries the Node's `Pretty` bounds. A flat helper threads the result's
/// stores through each generated method, because only they can resolve a handle, and an inherent
/// implementation block cannot bind the store's own parameters.
pub(super) struct PrettyFrame {
    /// Generics of the private helper implementation block.
    pub(super) inherent_generics: proc_macro2::TokenStream,
    /// Type generics of the implemented type.
    pub(super) type_generics: proc_macro2::TokenStream,
    /// Where clause of the private helper implementation block.
    pub(super) inherent_where: proc_macro2::TokenStream,
    /// Generics every private helper method declares.
    pub(super) method_generics: proc_macro2::TokenStream,
    /// Where clause every private helper method declares.
    pub(super) method_where: proc_macro2::TokenStream,
    /// Parameters every private helper method takes before its context.
    pub(super) method_parameters: proc_macro2::TokenStream,
    /// Arguments every private helper call passes before its context.
    pub(super) method_arguments: proc_macro2::TokenStream,
}

/// Wraps one rendered Pretty body in the trait implementation of its representation.
fn render_pretty_trait_impl(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    access: PrettyAccess,
    hints: &PrettyHints,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let ident = access.target_ident(node);
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let hinted_breaks = pretty_hinted_breaks(plan);
    let grouped = render_pretty_baseline_group(hints, body.clone());
    match access {
        PrettyAccess::Nested => {
            let (impl_generics, type_generics, where_clause) = pretty_impl_parts(facade, node);
            quote! {
                impl #impl_generics #facade::Pretty for #ident #type_generics #where_clause {
                    fn pretty(
                        &self,
                        context: &mut #facade::PrettyContext,
                    ) -> ::core::fmt::Result {
                        context.configure_defaults(#max_width, #indent);
                #hinted_breaks
                        context.configure_lexical_boundary(#module_anchor::__recursa_pretty_tokens_fuse);
                        #grouped
                    }
                }
            }
        }
        PrettyAccess::Flat => flat_pretty_trait_impl(plan, node, hints, body),
    }
}

/// Extracts the terminal Node identifier used in generated trait calls.
pub(super) fn terminal_path_ident(terminal: &Type) -> Option<&Ident> {
    let Type::Path(path) = terminal else {
        return None;
    };
    path.path.segments.last().map(|segment| &segment.ident)
}

/// Renders one element from its frozen value and formatting projections.
fn render_pretty_element(
    plan: &EmissionPlan,
    index: usize,
    element: &ElementParsePlan,
    behavior: &PrettyElementBehavior,
    semantic_value: Option<(proc_macro2::TokenStream, bool)>,
    access: PrettyAccess,
    followed: Option<proc_macro2::TokenStream>,
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
                    SemanticParseProjection::Direct {
                        wrappers,
                        terminal_node,
                        restriction,
                        ..
                    },
                    PrettySemanticProjection::Direct {
                        repetition,
                        attachment,
                        pretty,
                        presence,
                        provenance,
                    },
                ) => {
                    let field = match (
                        restricted_dispatch_index(plan, *terminal_node, *restriction, wrappers),
                        followed,
                    ) {
                        (Some(restriction), _) => FieldDispatch::Restricted(restriction),
                        (None, Some(next)) if repetition.is_none() => FieldDispatch::Followed(next),
                        (None, _) => FieldDispatch::Plain,
                    };
                    let wrappers = &access.value_wrappers(wrappers);
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
                            access,
                            &field,
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
                            access,
                            &field,
                        )
                    };
                    let reference = if borrowed { value } else { quote!(&#value) };
                    render_pretty_presence(pretty, *presence, wrappers, reference, rendered)
                }
                _ => unreachable!("validated Pretty semantic projection"),
            }
        }
        (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
        | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. })
        | (ElementParseKind::OptionalFixedTokenGroup { .. }, _)
        | (_, PrettyElementBehavior::OptionalFixedTokenGroup { .. }) => {
            unreachable!("validated Pretty element behavior")
        }
    }
}

/// Renders structured Pretty and source-map behavior for one nested Node.
pub(super) fn render_node_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    render_node_pretty_for(plan, node, node_index, PrettyAccess::Nested)
}

/// Renders structured Pretty behavior for one Node in the requested representation.
///
/// # Long-function contract
///
/// Rationale: renders one Node from its frozen Pretty behavior and source-order shape.
/// Invariants: every spacing, break, separator, and provenance decision participates in family
/// identity, and the two representations differ only in how a value and its children are reached.
/// Phases: binds the active shape, renders its element sequence, then writes through PrettyContext.
pub(super) fn render_node_pretty_for(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    access: PrettyAccess,
) -> proc_macro2::TokenStream {
    let target_ident = access.target_ident(node);
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Node behavior");
    let capable = pretty_leading_level_nodes(plan);
    let elements = |elements: &[ElementParsePlan],
                    behavior: &[PrettyElementBehavior],
                    layout: PrettySequenceLayout,
                    base: proc_macro2::TokenStream| {
        let field_value = |element: &ElementParsePlan| match &element.kind {
            ElementParseKind::Semantic { field, .. } => Some(field_access(
                base.clone(),
                field.ident.as_ref(),
                field.index,
            )),
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        };
        let rendered = elements
            .iter()
            .zip(behavior)
            .enumerate()
            .map(|(index, (element, behavior))| {
                let value = field_value(element).map(|value| (value, false));
                let followed = render_field_follow(
                    plan,
                    elements,
                    index,
                    |holder| {
                        let value = field_value(&elements[holder])
                            .expect("a child leading element is semantic");
                        quote!(&#value)
                    },
                    access,
                    &capable,
                );
                render_pretty_element(plan, index, element, behavior, value, access, followed)
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
        NodeBodyPlan::Enum { .. } if behavior.declared.is_some() => {
            return render_declared_enum_pretty(plan, node, node_index, access);
        }
        NodeBodyPlan::Enum { .. } => {
            let variants = trait_variants(&node.body);
            let arms = variants
                .iter()
                .zip(&behavior.variant_layouts)
                .zip(&behavior.variants)
                .zip(&behavior.variant_pretty)
                .map(|(((variant, layout), behavior), variant_pretty)| {
                    let (pattern, bindings) =
                        variant_bound_pattern(&target_ident, variant, "pretty");
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
                            let followed = render_field_follow(
                                plan,
                                &variant.elements,
                                index,
                                |holder| element_binding(&variant.elements, &bindings, holder),
                                access,
                                &capable,
                            );
                            render_pretty_element(
                                plan, index, element, behavior, value, access, followed,
                            )
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
    let leading =
        render_pretty_leading_level(plan, node, access, &pretty_leading_level_nodes(plan));
    let trait_impl = render_pretty_trait_impl(plan, node, access, &behavior.pretty, body);
    quote!(#trait_impl #leading)
}

/// Renders one semantic field with its presence, repetition, attachment, and provenance rules.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_pretty_semantic(
    plan: &EmissionPlan,
    index: usize,
    value: proc_macro2::TokenStream,
    repetition: Option<&PrettyRepetitionBehavior>,
    provenance: PrettyProvenanceBehavior,
    content_token: bool,
    access: PrettyAccess,
    field: &FieldDispatch,
) -> proc_macro2::TokenStream {
    let pretty = access.field_dispatch(plan, value, content_token, field);
    let render_value = if let Some(repetition) = repetition {
        let separator = repetition
            .separator
            .as_deref()
            .map_or_else(|| quote!(None), |separator| quote!(Some(#separator)));
        let trailing = repetition.trailing_separator;
        let hard_break = repetition.hard_break;
        let leading = repetition.leading_separator;
        let space_before_separator = repetition.space_before_separator;
        let space_after_separator = repetition.space_after_separator;
        render_pretty_context_scope(
            &format_ident!("repetition_scope_style"),
            quote!(#separator, #trailing, #hard_break, #leading, (#space_before_separator, #space_after_separator)),
            pretty,
        )
    } else {
        pretty
    };
    if repetition.is_some() {
        let scoped = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(#index),
            render_value,
        );
        quote!(#scoped?;)
    } else {
        let child = pretty_provenance_child_method(provenance);
        let inner = render_pretty_context_scope(&child, quote!(0), render_value);
        let outer =
            render_pretty_context_scope(&format_ident!("child_scope"), quote!(#index), inner);
        quote!(#outer?;)
    }
}

/// Wraps a rendered fragment in the configured grouping and break hints.
///
/// A break before the fragment is inside its group and its indentation, so that the group
/// decides it and the fragment starts one level in when it breaks. A break after the fragment is
/// inside its group but outside its indentation, so that what follows starts at the enclosing
/// level. Without a group or indentation, both breaks belong to the enclosing group.
pub(super) fn render_pretty_hints(
    hints: &PrettyHints,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let before = match hints.break_before {
        Some(PrettyBreak::Soft) => quote!(context.soft_break(" ")?;),
        Some(PrettyBreak::Hard) => quote!(context.hard_break()?;),
        Some(PrettyBreak::Space) => quote!(context.space()?;),
        None => quote!(),
    };
    let after = match hints.break_after {
        Some(PrettyBreak::Soft) => quote!(context.soft_break(" ")?;),
        Some(PrettyBreak::Hard) => quote!(context.hard_break()?;),
        Some(PrettyBreak::Space) => quote!(context.space()?;),
        None => quote!(),
    };
    let body = if hints.indent {
        quote!(context.indented(|context| { #before #body Ok(()) })?;)
    } else {
        quote!(#before #body)
    };
    match hints.group {
        Some(PrettyGroup::Consistent) => {
            quote!(context.group(|context| { #body #after Ok(()) })?;)
        }
        Some(PrettyGroup::Inconsistent) => {
            quote!(context.inconsistent_group(|context| { #body #after Ok(()) })?;)
        }
        None => quote!(#body #after),
    }
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
        PrettyProvenanceBehavior::Structured => format_ident!("child_scope"),
        PrettyProvenanceBehavior::Opaque => format_ident!("opaque_child_scope"),
    }
}

/// Enters one generated Pretty scope without creating a per-call-site callback.
fn render_pretty_context_scope(
    method: &Ident,
    arguments: proc_macro2::TokenStream,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote!({
        let mut __recursa_pretty_scope = context.#method(#arguments);
        let context = &mut *__recursa_pretty_scope;
        #body
    })
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
    let scoped = render_pretty_context_scope(
        &format_ident!("child_scope"),
        quote!(#index),
        quote!(
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#fallback))
        ),
    );
    quote!(#scoped?;)
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
    if before.is_empty() && after.is_empty() {
        let presence = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(#presence_index),
            quote!(#(#sequence_tokens)* Ok(())),
        );
        let outer = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(#index),
            quote!(#presence?; Ok(())),
        );
        let rendered = quote!(#outer?;);
        let rendered = render_pretty_hints(pretty, rendered);
        return quote! {
            if #value {
                #rendered
            }
        };
    }
    let presence = render_pretty_context_scope(
        &format_ident!("child_scope"),
        quote!(#presence_index),
        quote!(#(#sequence_tokens)* Ok(())),
    );
    let outer = render_pretty_context_scope(
        &format_ident!("child_scope"),
        quote!(#index),
        quote!(
            #(#before_tokens)*
            if #value {
                #presence?;
            }
            #(#after_tokens)*
            Ok(())
        ),
    );
    let rendered = quote!(#outer?;);
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
    let scoped = render_pretty_context_scope(
        &format_ident!("child_scope"),
        quote!(#index),
        quote!(
            if context.has_source_span() {
                #(#tokens)*
            }
            Ok(())
        ),
    );
    quote!(#scoped?;)
}

/// Renders canonical fixed text for a synthetic document without source provenance.
pub(super) fn render_pretty_synthetic_fixed(
    plan: &EmissionPlan,
    spelling: &str,
    rendering: PrettyFixedRendering,
) -> proc_macro2::TokenStream {
    let method = pretty_fixed_method(rendering);
    let fallback = pretty_fixed_fallback(plan, spelling);
    let scoped = render_pretty_context_scope(
        &format_ident!("without_provenance_scope"),
        quote!(),
        quote!(context.#method(#fallback)),
    );
    quote!(#scoped?;)
}

/// Selects the document-builder method for fixed-token placement.
pub(super) fn pretty_fixed_method(rendering: PrettyFixedRendering) -> syn::Ident {
    match rendering {
        PrettyFixedRendering::TightSequence => format_ident!("tight_sequence_token"),
        PrettyFixedRendering::Before => format_ident!("before_token"),
        PrettyFixedRendering::After => format_ident!("after_token"),
        PrettyFixedRendering::Around => format_ident!("around_token"),
        PrettyFixedRendering::Opening => format_ident!("opening_token"),
        PrettyFixedRendering::Closing => format_ident!("closing_token"),
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
    let scoped = render_pretty_context_scope(
        &format_ident!("child_scope"),
        quote!(#index),
        quote!(
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#spelling))
        ),
    );
    quote!(#scoped?;)
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

/// Renders one required attachment entry, forcing an automatic-punctuation token to open
/// tightly instead, so that a synthesized grouping delimiter immediately following a
/// declared-precedence prefix token never gets the defensive space the runtime `PrettyContext`
/// inserts between two adjacent automatic-punctuation tokens.
fn render_pretty_attached_syntax_as_opening(
    index: usize,
    syntax: &PrettyAttachedSyntaxBehavior,
) -> proc_macro2::TokenStream {
    let PrettyAttachedSyntaxBehavior::Required(fixed) = syntax else {
        return render_pretty_attached_syntax(index, syntax);
    };
    if fixed.spacing != PrettySpacing::Auto || fixed.rendering != PrettyFixedRendering::Punctuation
    {
        return render_pretty_attached_fixed(index, fixed);
    }
    let mut opened = fixed.clone();
    opened.rendering = PrettyFixedRendering::Opening;
    render_pretty_attached_fixed(index, &opened)
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
    let semantic = render_pretty_context_scope(&child, quote!(#semantic_index), semantic_body);
    let semantic = quote!(#semantic?;);
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
    } else if attachment.layout == PrettySequenceLayout::DelimitedAfterPrefix {
        let (open, prefix) = before
            .split_last()
            .expect("delimited attachment has an open token");
        let (close, suffix) = after
            .split_first()
            .expect("delimited attachment has a close token");
        quote! {
            #(#prefix)*
            #open
            context.indented(|context| {
                context.soft_break("")?;
                #semantic
                Ok(())
            })?;
            context.soft_break("")?;
            #close
            context.close_delimiter();
            #(#suffix)*
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
    access: PrettyAccess,
    field: &FieldDispatch,
) -> proc_macro2::TokenStream {
    let render_unit = |terminal: proc_macro2::TokenStream| {
        let semantic = access.field_dispatch(plan, terminal, content_token, field);
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
        let space_after_separator = repetition.space_after_separator;
        let render_collection = |collection: proc_macro2::TokenStream| {
            let items = access.iterate(collection);
            let scoped = render_pretty_context_scope(
                &format_ident!("repetition_scope_style"),
                quote!(
                    #separator,
                    #trailing,
                    #hard_break,
                    #leading,
                    (#space_before_separator, #space_after_separator)
                ),
                quote! {
                    context.render_repeated_by(
                        #items,
                        |context, item| { #unit Ok(()) },
                    )
                },
            );
            quote!(#scoped?;)
        };
        let outer_optional = presence == PrettyPresence::Optional;
        let body = if outer_optional {
            let collection = access.optional_collection(value, outer, value_is_reference);
            let rendered = render_collection(quote!(collection));
            quote! {
                if let Some(collection) = #collection {
                    #rendered
                }
            }
        } else {
            let collection = access.collection(value, outer, value_is_reference);
            render_collection(collection)
        };
        let scoped = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(#element_index),
            quote!(#body Ok(())),
        );
        quote!(#scoped?;)
    } else if presence == PrettyPresence::Optional {
        let terminal = render_optional_view_reference(value, wrappers, value_is_reference);
        let unit = render_unit(quote!(terminal));
        let inner = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(0),
            quote!(#unit Ok(())),
        );
        let outer = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(#element_index),
            inner,
        );
        quote! {
            if let Some(terminal) = #terminal {
                #outer?;
            }
        }
    } else {
        let terminal = render_terminal_reference(value, wrappers, value_is_reference);
        let unit = render_unit(terminal);
        let inner = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(0),
            quote!(#unit Ok(())),
        );
        let outer = render_pretty_context_scope(
            &format_ident!("child_scope"),
            quote!(#element_index),
            inner,
        );
        quote!(#outer?;)
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

/// What the parser sees first in one element: the token at its start.
///
/// Pretty reads the declared level of that token when the element follows an operand, because
/// bison compares the level of the lookahead token, not the level of the enclosing rule, when it
/// decides between reducing the operand and shifting
/// (`docs/declared-precedence.md`, "Consumers").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrettyLeading {
    /// A fixed token with a declared level.
    Level(DeclaredVariantLevel),
    /// A required child Node, whose own leading token decides.
    Child(NodeId),
    /// No known leading token with a level: a token with no level, a content token, or an
    /// element that can be absent.
    None,
}

/// Returns the leading token of one required attached fixed entry.
fn attached_leading(syntax: &AttachedFixedSyntaxParsePlan) -> PrettyLeading {
    match syntax {
        AttachedFixedSyntaxParsePlan::Required(fixed) => fixed
            .precedence
            .map_or(PrettyLeading::None, PrettyLeading::Level),
        AttachedFixedSyntaxParsePlan::Optional(_) => PrettyLeading::None,
    }
}

/// Returns the leading token of one element.
fn element_leading(element: &ElementParsePlan) -> PrettyLeading {
    match &element.kind {
        ElementParseKind::Fixed { precedence, .. } => {
            precedence.map_or(PrettyLeading::None, PrettyLeading::Level)
        }
        ElementParseKind::OptionalFixedTokenGroup { .. } => PrettyLeading::None,
        ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
            SemanticParseProjection::FixedPresence { before, .. } => {
                before.first().map_or(PrettyLeading::None, attached_leading)
            }
            SemanticParseProjection::Direct {
                attachment,
                wrappers,
                terminal_node,
                explorer_terminal,
                ..
            } => {
                if let Some(first) = attachment
                    .as_ref()
                    .and_then(|attachment| attachment.before.first())
                {
                    return attached_leading(first);
                }
                match (terminal_node, explorer_terminal) {
                    (Some(node), ExplorerTerminalPlan::Node(_))
                        if wrappers
                            .iter()
                            .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed)) =>
                    {
                        PrettyLeading::Child(*node)
                    }
                    _ => PrettyLeading::None,
                }
            }
        },
    }
}

/// Returns the token the parser sees right after the first operand of one variant, when the
/// variant's first symbol is a self-recursive operand of `owner`.
///
/// The second value is the binding index of the element that holds the token, for a child Node.
fn first_operand_lookahead(
    variant: &VariantParsePlan,
    owner: NodeId,
) -> Option<(PrettyLeading, usize)> {
    let first = variant.elements.first()?;
    let ElementParseKind::Semantic { projection, .. } = &first.kind else {
        return None;
    };
    let SemanticParseProjection::Direct {
        terminal_node: Some(terminal),
        attachment,
        ..
    } = projection.as_ref()
    else {
        return None;
    };
    if *terminal != owner
        || attachment
            .as_ref()
            .is_some_and(|attachment| !attachment.before.is_empty())
    {
        return None;
    }
    if let Some(after) = attachment
        .as_ref()
        .and_then(|attachment| attachment.after.first())
    {
        return Some((attached_leading(after), 0));
    }
    let next = variant.elements.get(1)?;
    Some((element_leading(next), 1))
}

/// Returns the token the parser sees right after one field of a sequence, when the field holds
/// a Node with declared-precedence Pretty directly, behind nothing but `Box`.
///
/// The values are the base, the token, and the index of the element that holds the token, for a
/// child Node.
fn field_follow(
    declared: &BTreeSet<NodeId>,
    elements: &[ElementParsePlan],
    index: usize,
) -> Option<(NodeId, PrettyLeading, usize)> {
    let ElementParseKind::Semantic { projection, .. } = &elements.get(index)?.kind else {
        return None;
    };
    let SemanticParseProjection::Direct {
        terminal_node: Some(base),
        wrappers,
        attachment,
        ..
    } = projection.as_ref()
    else {
        return None;
    };
    if !wrappers
        .iter()
        .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed))
        || !declared.contains(base)
    {
        return None;
    }
    if let Some(after) = attachment
        .as_ref()
        .and_then(|attachment| attachment.after.first())
    {
        return Some((*base, attached_leading(after), index));
    }
    let next = elements.get(index + 1)?;
    Some((*base, element_leading(next), index + 1))
}

/// Returns the binding of the semantic element at `index` of one variant's elements.
fn element_binding(
    elements: &[ElementParsePlan],
    bindings: &[Ident],
    index: usize,
) -> proc_macro2::TokenStream {
    let position = elements[..index]
        .iter()
        .filter(|element| matches!(element.kind, ElementParseKind::Semantic { .. }))
        .count();
    let binding = &bindings[position];
    quote!(#binding)
}

/// Returns the Nodes that have declared-precedence Pretty.
fn declared_pretty_nodes(plan: &EmissionPlan) -> BTreeSet<NodeId> {
    plan.nodes
        .iter()
        .zip(&plan.pretty.nodes)
        .filter(|(_, behavior)| {
            behavior
                .as_ref()
                .is_some_and(|behavior| behavior.declared.is_some())
        })
        .map(|(node, _)| node.node)
        .collect()
}

/// The analysis of the tokens after operands and fields that every Pretty body of one plan
/// reads. It is computed once for each plan, because each Node's body reads it.
struct PrettyFollowAnalysis {
    /// The Nodes with declared-precedence Pretty.
    declared: BTreeSet<NodeId>,
    /// The Nodes whose leading token a generated body reads at run time.
    capable: BTreeSet<NodeId>,
    /// The levels of the tokens that can continue a value of each declared Node.
    continuation: BTreeMap<NodeId, BTreeSet<u32>>,
}

/// The key of one plan in the follow-analysis cache: its identity, and the address and the
/// length of its Nodes.
type PrettyFollowKey = (String, usize, usize);

thread_local! {
    /// The analysis of the last plan.
    static PRETTY_FOLLOW_ANALYSIS: std::cell::RefCell<
        Option<(PrettyFollowKey, std::rc::Rc<PrettyFollowAnalysis>)>,
    > = const { std::cell::RefCell::new(None) };
}

/// Returns the follow analysis of one plan, from the cache when the plan is the last one.
fn pretty_follow_analysis(plan: &EmissionPlan) -> std::rc::Rc<PrettyFollowAnalysis> {
    let key = (
        plan.identity.clone(),
        plan.nodes.as_ptr() as usize,
        plan.nodes.len(),
    );
    PRETTY_FOLLOW_ANALYSIS.with(|cache| {
        if let Some((cached, analysis)) = cache.borrow().as_ref()
            && *cached == key
        {
            return std::rc::Rc::clone(analysis);
        }
        let declared = declared_pretty_nodes(plan);
        let capable = compute_pretty_leading_level_nodes(plan, &declared);
        let continuation = declared
            .iter()
            .map(|base| (*base, declared_continuation_levels(plan, *base)))
            .collect();
        let analysis = std::rc::Rc::new(PrettyFollowAnalysis {
            declared,
            capable,
            continuation,
        });
        *cache.borrow_mut() = Some((key, std::rc::Rc::clone(&analysis)));
        analysis
    })
}

/// Returns the levels of the tokens that can come after an operand of one declared base in its
/// own rules: the tokens that the parser can shift into a value of the base.
///
/// A token with a level that is not in this set never continues a value, so the parser reduces
/// the value before it, and a field before it needs no parentheses.
fn declared_continuation_levels(plan: &EmissionPlan, base: NodeId) -> BTreeSet<u32> {
    let mut levels = BTreeSet::new();
    let Some(node) = plan.nodes.iter().find(|candidate| candidate.node == base) else {
        return levels;
    };
    let NodeBodyPlan::Enum { variants } = &node.body else {
        return levels;
    };
    let mut visited = BTreeSet::new();
    for variant in variants {
        for (index, element) in variant.elements.iter().enumerate() {
            let ElementParseKind::Semantic { projection, .. } = &element.kind else {
                continue;
            };
            let SemanticParseProjection::Direct {
                terminal_node: Some(terminal),
                attachment,
                ..
            } = projection.as_ref()
            else {
                continue;
            };
            if *terminal != base {
                continue;
            }
            let after = attachment
                .as_ref()
                .and_then(|attachment| attachment.after.first())
                .map(attached_leading)
                .or_else(|| variant.elements.get(index + 1).map(element_leading));
            if let Some(after) = after {
                collect_leading_levels(plan, after, &mut levels, &mut visited);
            }
        }
    }
    levels
}

/// Adds the levels that one leading token can have to `levels`; a child Node adds the levels of
/// every token that one of its values can start with.
fn collect_leading_levels(
    plan: &EmissionPlan,
    leading: PrettyLeading,
    levels: &mut BTreeSet<u32>,
    visited: &mut BTreeSet<NodeId>,
) {
    match leading {
        PrettyLeading::Level(level) => {
            levels.insert(level.ordinal);
        }
        PrettyLeading::Child(child) => {
            if !visited.insert(child) {
                return;
            }
            let Some(node) = plan.nodes.iter().find(|candidate| candidate.node == child) else {
                return;
            };
            let firsts: Vec<&ElementParsePlan> = match &node.body {
                NodeBodyPlan::Struct { elements, .. } => elements.first().into_iter().collect(),
                NodeBodyPlan::Enum { variants } => variants
                    .iter()
                    .filter_map(|variant| variant.elements.first())
                    .collect(),
            };
            for first in firsts {
                collect_leading_levels(plan, element_leading(first), levels, visited);
            }
        }
        PrettyLeading::None => {}
    }
}

/// Renders the bound of the token after one field that holds a declared base, or `None` when no
/// token after it can continue a value of the base (`docs/declared-precedence.md`, "Consumers").
///
/// `holder` gives the expression that reaches the value of an element, by element index.
fn render_field_follow(
    plan: &EmissionPlan,
    elements: &[ElementParsePlan],
    index: usize,
    holder: impl FnOnce(usize) -> proc_macro2::TokenStream,
    access: PrettyAccess,
    capable: &BTreeSet<NodeId>,
) -> Option<proc_macro2::TokenStream> {
    let analysis = pretty_follow_analysis(plan);
    let (base, leading, holder_index) = field_follow(&analysis.declared, elements, index)?;
    let levels = analysis.continuation.get(&base)?;
    match leading {
        PrettyLeading::Level(level) => levels
            .contains(&level.ordinal)
            .then(|| render_leading_level_constant(level)),
        PrettyLeading::Child(child) if capable.contains(&child) && !levels.is_empty() => {
            let read = render_leading(leading, || holder(holder_index), access, capable)?;
            let levels = levels.iter();
            Some(quote! {
                (#read).filter(|(level, _)| matches!(*level, #(#levels)|*))
            })
        }
        PrettyLeading::Child(_) | PrettyLeading::None => None,
    }
}

/// Reports whether one Node can name a leading-level function: a Node with Pretty and no type or
/// const parameter.
fn leading_level_capable(plan: &EmissionPlan, node: NodeId) -> bool {
    plan.nodes
        .iter()
        .position(|candidate| candidate.node == node)
        .is_some_and(|index| {
            plan.pretty.nodes[index].is_some()
                && plan.nodes[index]
                    .generics
                    .params
                    .iter()
                    .all(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
        })
}

/// Collects the Nodes whose leading token a declared-precedence helper reads at run time.
///
/// The set starts at the child Node that follows a first operand, and closes over the child Node
/// that leads each of their variants.
pub(super) fn pretty_leading_level_nodes(plan: &EmissionPlan) -> BTreeSet<NodeId> {
    pretty_follow_analysis(plan).capable.clone()
}

/// Computes [`pretty_leading_level_nodes`].
fn compute_pretty_leading_level_nodes(
    plan: &EmissionPlan,
    declared: &BTreeSet<NodeId>,
) -> BTreeSet<NodeId> {
    let mut pending = Vec::new();
    for (node, behavior) in plan.nodes.iter().zip(&plan.pretty.nodes) {
        if !behavior
            .as_ref()
            .is_some_and(|behavior| behavior.declared.is_some())
        {
            continue;
        }
        let NodeBodyPlan::Enum { variants } = &node.body else {
            continue;
        };
        for variant in variants {
            if let Some((PrettyLeading::Child(child), _)) =
                first_operand_lookahead(variant, node.node)
            {
                pending.push(child);
            }
        }
    }
    // A child Node after a field that holds a declared base (see [`field_follow`]).
    for node in &plan.nodes {
        let sequences: Vec<&[ElementParsePlan]> = match &node.body {
            NodeBodyPlan::Struct { elements, .. } => vec![elements.as_slice()],
            NodeBodyPlan::Enum { variants } => variants
                .iter()
                .map(|variant| variant.elements.as_slice())
                .collect(),
        };
        for elements in sequences {
            for index in 0..elements.len() {
                if let Some((_, PrettyLeading::Child(child), _)) =
                    field_follow(declared, elements, index)
                {
                    pending.push(child);
                }
            }
        }
    }
    let mut nodes = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !leading_level_capable(plan, node) || !nodes.insert(node) {
            continue;
        }
        let Some(planned) = plan.nodes.iter().find(|candidate| candidate.node == node) else {
            continue;
        };
        let elements: Vec<&[ElementParsePlan]> = match &planned.body {
            NodeBodyPlan::Struct { elements, .. } => vec![elements.as_slice()],
            NodeBodyPlan::Enum { variants } => variants
                .iter()
                .map(|variant| variant.elements.as_slice())
                .collect(),
        };
        for elements in elements {
            if let Some(PrettyLeading::Child(child)) = elements.first().map(element_leading) {
                pending.push(child);
            }
        }
    }
    nodes
}

/// Renders the constant bound of one declared level as the token after an operand.
///
/// The tie flag is the one a first operand's `next` bound carries: a tie parenthesizes unless
/// the level is `left`, because bison then reduces the operand before the token.
fn render_leading_level_constant(level: DeclaredVariantLevel) -> proc_macro2::TokenStream {
    let ordinal = level.ordinal;
    let tie = level.associativity != PrecedenceAssociativity::Left;
    quote!(::core::option::Option::Some((#ordinal, #tie)))
}

/// Renders the leading bound of one element, given the expression that reaches its value.
fn render_leading(
    leading: PrettyLeading,
    value: impl FnOnce() -> proc_macro2::TokenStream,
    access: PrettyAccess,
    capable: &BTreeSet<NodeId>,
) -> Option<proc_macro2::TokenStream> {
    match leading {
        PrettyLeading::Level(level) => Some(render_leading_level_constant(level)),
        PrettyLeading::Child(child) if capable.contains(&child) => {
            let value = value();
            Some(match access {
                PrettyAccess::Nested => quote!((#value).__recursa_pretty_leading_level()),
                PrettyAccess::Flat => quote! {
                    {
                        let __recursa_handle: &_ = #value;
                        __recursa_ast[*__recursa_handle].__recursa_pretty_leading_level(__recursa_ast)
                    }
                },
            })
        }
        PrettyLeading::Child(_) | PrettyLeading::None => None,
    }
}

/// Renders the expression for the token after the first operand of one declared variant, or
/// `None` when no token with a level follows it.
pub(super) fn render_first_operand_lookahead(
    variant: &VariantParsePlan,
    owner: NodeId,
    bindings: &[Ident],
    access: PrettyAccess,
    capable: &BTreeSet<NodeId>,
) -> Option<proc_macro2::TokenStream> {
    let (leading, binding) = first_operand_lookahead(variant, owner)?;
    render_leading(
        leading,
        || {
            let binding = &bindings[binding];
            quote!(#binding)
        },
        access,
        capable,
    )
}

/// Renders the private function that returns the declared level of the token one Node value
/// starts with, for a Node that follows a first operand (see [`pretty_leading_level_nodes`]).
pub(super) fn render_pretty_leading_level(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    access: PrettyAccess,
    capable: &BTreeSet<NodeId>,
) -> proc_macro2::TokenStream {
    if !capable.contains(&node.node) {
        return proc_macro2::TokenStream::new();
    }
    let ident = access.target_ident(node);
    let none = quote!(::core::option::Option::None);
    let body = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => elements
            .first()
            .and_then(|first| {
                render_leading(
                    element_leading(first),
                    || match &first.kind {
                        ElementParseKind::Semantic { field, .. } => {
                            let value =
                                field_access(quote!(self), field.ident.as_ref(), field.index);
                            quote!(&#value)
                        }
                        ElementParseKind::Fixed { .. }
                        | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                            unreachable!("a child leading element is semantic")
                        }
                    },
                    access,
                    capable,
                )
            })
            .unwrap_or_else(|| none.clone()),
        NodeBodyPlan::Enum { variants } if variants.is_empty() => none.clone(),
        NodeBodyPlan::Enum { variants } => {
            let arms = variants.iter().map(|variant| {
                let (pattern, bindings) = variant_bound_pattern(&ident, variant, "pretty_leading");
                let value = variant
                    .elements
                    .first()
                    .and_then(|first| {
                        render_leading(
                            element_leading(first),
                            || {
                                let binding = &bindings[0];
                                quote!(#binding)
                            },
                            access,
                            capable,
                        )
                    })
                    .unwrap_or_else(|| none.clone());
                quote!(#pattern => #value)
            });
            quote!(match self { #(#arms),* })
        }
    };
    let PrettyFrame {
        inherent_generics,
        type_generics,
        inherent_where,
        method_generics,
        method_where,
        method_parameters,
        ..
    } = access.frame(plan, node);
    quote! {
        impl #inherent_generics #ident #type_generics #inherent_where {
            /// Returns the declared level of the token this value starts with, and whether a
            /// tie at that level parenthesizes the operand before it.
            #[doc(hidden)]
            #[allow(dead_code, unused_variables)]
            pub(crate) fn __recursa_pretty_leading_level #method_generics(
                &self,
                #method_parameters
            ) -> ::core::option::Option<(u32, bool)> #method_where {
                #body
            }
        }
    }
}

/// Returns the restriction one field renders its base under, when the field names a restricted
/// expression of a base with declared-precedence Pretty and holds the base directly, behind
/// nothing but `Box`.
///
/// A field of another shape renders through the base's ordinary `Pretty`, as before.
fn restricted_dispatch_index(
    plan: &EmissionPlan,
    terminal_node: Option<NodeId>,
    restriction: Option<u16>,
    wrappers: &[FieldWrapper],
) -> Option<u16> {
    let (base, restriction) = (terminal_node?, restriction?);
    if !wrappers
        .iter()
        .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed))
        || plan
            .restrictions
            .of(base)
            .get(usize::from(restriction))
            .is_none()
    {
        return None;
    }
    let index = plan.nodes.iter().position(|node| node.node == base)?;
    plan.pretty.nodes[index]
        .as_ref()
        .is_some_and(|behavior| behavior.declared.is_some())
        .then_some(restriction)
}

/// Returns the restrictions of one declared base under which Pretty can repair a value: the
/// restrictions that admit every grouping atom the enclosure proof selected.
///
/// A restriction that does not admit the atom keeps today's rendering; `Arbitrary` never samples
/// a value it refuses (`docs/declared-precedence.md`, "Consumers").
fn repairable_restrictions(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> BTreeSet<usize> {
    let restrictions = plan.restrictions.of(node.node);
    if restrictions.is_empty() {
        return BTreeSet::new();
    }
    let atoms = grouping_atom_variants(plan, node, node_index);
    if atoms.is_empty() {
        return BTreeSet::new();
    }
    restrictions
        .iter()
        .enumerate()
        .filter(|(_, restriction)| {
            atoms
                .iter()
                .all(|atom| restriction.admitted.get(*atom).copied().unwrap_or(false))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Returns the variants of one declared Node whose enclosure the grouping proof selected, by
/// the same rule as [`agreed_grouping`].
fn grouping_atom_variants(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> Vec<usize> {
    let Some(behavior) = plan.pretty.nodes[node_index].as_ref() else {
        return Vec::new();
    };
    let Some(declared) = behavior.declared.as_ref() else {
        return Vec::new();
    };
    let Some(grouping) = declared.grouping.as_ref() else {
        return Vec::new();
    };
    let NodeBodyPlan::Enum { variants } = &node.body else {
        return Vec::new();
    };
    let syntax = emitted_pretty_grouping_syntax(&plan.nodes);
    let mut analyzer = PrettyGroupingAnalyzer::new(&syntax);
    let mut candidates = Vec::new();
    for (index, ((variant, elements), declared_variant)) in variants
        .iter()
        .zip(&behavior.variants)
        .zip(&declared.variants)
        .enumerate()
    {
        let is_atom = declared_variant.operands.iter().all(|operand| {
            matches!(
                operand,
                PrettyDeclaredOperandBehavior::Ordinary
                    | PrettyDeclaredOperandBehavior::Recursive {
                        minimum: PrettyDeclaredMinimum::None,
                        next: PrettyDeclaredNext::None,
                        ..
                    }
            )
        });
        if !is_atom {
            continue;
        }
        let proof = variant
            .elements
            .iter()
            .zip(elements)
            .map(|(element, behavior)| emitted_pretty_grouping_element(element, behavior))
            .collect::<Vec<_>>();
        if let Some(witness) = analyzer.grouping(node.node, &proof) {
            candidates.push((index, witness));
        }
    }
    let has_direct = candidates.iter().any(|(_, witness)| !witness.delegated);
    candidates
        .into_iter()
        .filter(|(_, witness)| !(has_direct && witness.delegated) && witness.grouping == *grouping)
        .map(|(index, _)| index)
        .collect()
}

/// Renders the private entry a field that names a restricted expression renders through: the
/// body of the base's `Pretty` implementation, under the restriction.
fn render_pretty_restricted_entry(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    access: PrettyAccess,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let hinted_breaks = pretty_hinted_breaks(plan);
    let PrettyFrame {
        method_generics,
        method_where,
        method_parameters,
        ..
    } = access.frame(plan, node);
    quote! {
        /// Renders this value at a site that parses it through one restricted expression, with
        /// the parentheses that make every refused variant parse there.
        #[doc(hidden)]
        #[allow(dead_code)]
        pub(crate) fn __recursa_pretty_restricted #method_generics(
            &self,
            #method_parameters
            context: &mut #facade::PrettyContext,
            restriction: u16,
        ) -> ::core::fmt::Result #method_where {
            context.configure_defaults(#max_width, #indent);
                #hinted_breaks
            context.configure_lexical_boundary(#module_anchor::__recursa_pretty_tokens_fuse);
            context.baseline_group(|context| { #body Ok(()) })
        }
    }
}

/// Renders the private entry a field of another Node renders this value through when a token
/// with a level follows the field: the body of the `Pretty` implementation, with that token's
/// bound as the right edge.
fn render_pretty_followed_entry(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    access: PrettyAccess,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let hinted_breaks = pretty_hinted_breaks(plan);
    let PrettyFrame {
        method_generics,
        method_where,
        method_parameters,
        ..
    } = access.frame(plan, node);
    quote! {
        /// Renders this value before a token with a level, with the parentheses that keep the
        /// parser from shifting that token into the value.
        #[doc(hidden)]
        #[allow(dead_code)]
        pub(crate) fn __recursa_pretty_followed #method_generics(
            &self,
            #method_parameters
            context: &mut #facade::PrettyContext,
            next: ::core::option::Option<(u32, bool)>,
        ) -> ::core::fmt::Result #method_where {
            context.configure_defaults(#max_width, #indent);
                #hinted_breaks
            context.configure_lexical_boundary(#module_anchor::__recursa_pretty_tokens_fuse);
            context.baseline_group(|context| { #body Ok(()) })
        }
    }
}

/// Renders the enclosure Pretty writes when it parenthesizes a value, as the document events
/// that the grouping atom renders for a parsed value (`docs/declared-precedence.md`, "The
/// enclosure proof").
///
/// The frame walks the witness path of the selected atom: the atom variant of the owner, then the
/// variant and element of each helper Node, down to the owner. Each step renders what the atom
/// path renders: a helper Node through its `Pretty` entry (defaults, baseline group and hints),
/// its element sequence with its layout, the fixed syntax of the step, and the attachment of the
/// element on the path. Elements that the proof took as absent render nothing. Every fixed token
/// is canonical and has no provenance. `hole` renders the parenthesized value itself.
///
/// The frame is `None` when the path has a shape this renderer does not copy; the caller then
/// writes the fixed syntax of the enclosure inline, as before.
fn render_grouping_frame(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    hole: &proc_macro2::TokenStream,
) -> Option<proc_macro2::TokenStream> {
    let behavior = plan.pretty.nodes[node_index].as_ref()?;
    let declared = behavior.declared.as_ref()?;
    let grouping = declared.grouping.as_ref()?;
    let NodeBodyPlan::Enum { variants } = &node.body else {
        return None;
    };
    let syntax = emitted_pretty_grouping_syntax(&plan.nodes);
    let mut analyzer = PrettyGroupingAnalyzer::new(&syntax);
    let mut candidates = Vec::new();
    for (index, ((variant, elements), declared_variant)) in variants
        .iter()
        .zip(&behavior.variants)
        .zip(&declared.variants)
        .enumerate()
    {
        let is_atom = declared_variant.operands.iter().all(|operand| {
            matches!(
                operand,
                PrettyDeclaredOperandBehavior::Ordinary
                    | PrettyDeclaredOperandBehavior::Recursive {
                        minimum: PrettyDeclaredMinimum::None,
                        next: PrettyDeclaredNext::None,
                        ..
                    }
            )
        });
        if !is_atom {
            continue;
        }
        let proof = variant
            .elements
            .iter()
            .zip(elements)
            .map(|(element, behavior)| emitted_pretty_grouping_element(element, behavior))
            .collect::<Vec<_>>();
        if let Some(witness) = analyzer.grouping(node.node, &proof) {
            candidates.push((index, witness));
        }
    }
    let has_direct = candidates.iter().any(|(_, witness)| !witness.delegated);
    let (atom, witness) = candidates
        .into_iter()
        .find(|(_, witness)| !(has_direct && witness.delegated) && witness.grouping == *grouping)?;
    let frame = FrameRenderer {
        plan,
        owner: node.node,
        hole,
    };
    frame.sequence(
        node_index,
        atom,
        &witness.steps,
        &behavior.variant_layouts[atom],
        &behavior.variant_pretty[atom],
        true,
    )
}

/// Renders the steps of one enclosure frame (see [`render_grouping_frame`]).
struct FrameRenderer<'a> {
    /// The plan whose Nodes the path crosses.
    plan: &'a EmissionPlan,
    /// The declared Node that the enclosure parenthesizes.
    owner: NodeId,
    /// The statements that render the parenthesized value.
    hole: &'a proc_macro2::TokenStream,
}

impl FrameRenderer<'_> {
    /// Renders one Node of the path through its `Pretty` entry, as a parsed child renders.
    fn node(&self, node: NodeId, steps: &[PrettyGroupingStep]) -> Option<proc_macro2::TokenStream> {
        let index = self
            .plan
            .nodes
            .iter()
            .position(|planned| planned.node == node)?;
        let behavior = self.plan.pretty.nodes[index].as_ref()?;
        if behavior.declared.is_some() {
            return None;
        }
        let step = steps.first()?;
        let layout = behavior.variant_layouts.get(step.variant)?;
        let hints = behavior.variant_pretty.get(step.variant)?;
        let body = self.sequence(index, step.variant, steps, layout, hints, false)?;
        Some(self.entry(&behavior.pretty, body))
    }

    /// Renders the `Pretty` entry of one Node around its body: defaults, the baseline group and
    /// the Node hints.
    fn entry(
        &self,
        hints: &PrettyHints,
        body: proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        let max_width = self.plan.pretty_max_width;
        let indent = self.plan.pretty_indent;
        let hinted_breaks = pretty_hinted_breaks(self.plan);
        let body = render_pretty_hints(hints, quote!({ #body Ok(()) }?;));
        quote! {
            context.configure_defaults(#max_width, #indent);
                #hinted_breaks
            context.baseline_group(|context| { #body Ok(()) })?;
        }
    }

    /// Renders one variant of one Node of the path: its elements, its layout and its hints.
    ///
    /// `atom` is true for the atom variant of the owner, which the owner's declared helper
    /// renders without a `Pretty` entry of its own.
    fn sequence(
        &self,
        node_index: usize,
        variant: usize,
        steps: &[PrettyGroupingStep],
        layout: &PrettySequenceLayout,
        hints: &PrettyHints,
        atom: bool,
    ) -> Option<proc_macro2::TokenStream> {
        let planned = &self.plan.nodes[node_index];
        let behavior = self.plan.pretty.nodes[node_index].as_ref()?;
        let elements = match &planned.body {
            NodeBodyPlan::Struct { elements, .. } => elements,
            NodeBodyPlan::Enum { variants } => &variants.get(variant)?.elements,
        };
        let behaviors = behavior.variants.get(variant)?;
        let step = steps.first()?;
        let mut rendered = Vec::with_capacity(elements.len());
        for (index, (element, behavior)) in elements.iter().zip(behaviors).enumerate() {
            let path = index == step.element;
            rendered.push(self.element(element, behavior, path.then(|| &steps[1..]), atom)?);
        }
        let sequence = render_pretty_sequence(*layout, &rendered);
        let sequence = render_pretty_hints(hints, sequence);
        Some(match &planned.body {
            NodeBodyPlan::Struct { .. } => quote!(#sequence),
            NodeBodyPlan::Enum { .. } => sequence,
        })
    }

    /// Renders one element of a step: fixed syntax, an absent element, a fixed-only helper Node,
    /// or the element on the path (`rest` holds the steps below it).
    fn element(
        &self,
        element: &ElementParsePlan,
        behavior: &PrettyElementBehavior,
        rest: Option<&[PrettyGroupingStep]>,
        atom: bool,
    ) -> Option<proc_macro2::TokenStream> {
        match (&element.kind, behavior) {
            (
                ElementParseKind::Fixed { .. },
                PrettyElementBehavior::Fixed {
                    spelling,
                    rendering,
                    ..
                },
            ) if rest.is_none() => Some(render_pretty_synthetic_fixed(
                self.plan, spelling, *rendering,
            )),
            (ElementParseKind::OptionalFixedTokenGroup { .. }, _) if rest.is_none() => {
                Some(proc_macro2::TokenStream::new())
            }
            (
                ElementParseKind::Semantic { projection, .. },
                PrettyElementBehavior::Semantic {
                    projection:
                        PrettySemanticProjection::Direct {
                            attachment,
                            pretty,
                            presence,
                            repetition: None,
                            ..
                        },
                },
            ) => {
                let SemanticParseProjection::Direct {
                    terminal_node,
                    wrappers,
                    explorer_terminal: ExplorerTerminalPlan::Node(_),
                    ..
                } = projection.as_ref()
                else {
                    return None;
                };
                let terminal = (*terminal_node)?;
                let required = wrappers
                    .iter()
                    .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed));
                let value = match rest {
                    // The proof takes an optional element as absent.
                    None if !required => return Some(proc_macro2::TokenStream::new()),
                    None => self.fixed_only(terminal)?,
                    Some(_) if !required => return None,
                    Some(_) if terminal == self.owner && atom => {
                        // A direct atom renders its operand through the declared helper.
                        let hole = self.hole;
                        quote!(#hole)
                    }
                    Some(_) if terminal == self.owner => {
                        // A helper reaches the owner through its `Pretty` entry.
                        let index = self
                            .plan
                            .nodes
                            .iter()
                            .position(|planned| planned.node == self.owner)?;
                        let hints = &self.plan.pretty.nodes[index].as_ref()?.pretty;
                        let hole = self.hole;
                        self.entry(hints, quote!(#hole))
                    }
                    Some(rest) => self.node(terminal, rest)?,
                };
                let unit = match attachment {
                    Some(attachment) => {
                        if atom && attachment.layout != PrettySequenceLayout::Delimited {
                            return None;
                        }
                        self.attachment(attachment, value)?
                    }
                    None => value,
                };
                Some(render_pretty_presence(
                    pretty,
                    *presence,
                    wrappers,
                    quote!(()),
                    unit,
                ))
            }
            _ => None,
        }
    }

    /// Renders one fixed-only helper Node, which every variant renders as the same fixed syntax.
    fn fixed_only(&self, node: NodeId) -> Option<proc_macro2::TokenStream> {
        let index = self
            .plan
            .nodes
            .iter()
            .position(|planned| planned.node == node)?;
        let behavior = self.plan.pretty.nodes[index].as_ref()?;
        if behavior.declared.is_some() {
            return None;
        }
        let planned = &self.plan.nodes[index];
        let elements = match &planned.body {
            NodeBodyPlan::Struct { elements, .. } => elements,
            NodeBodyPlan::Enum { variants } => &variants.first()?.elements,
        };
        let mut rendered = Vec::with_capacity(elements.len());
        for (element, element_behavior) in elements.iter().zip(behavior.variants.first()?) {
            rendered.push(match (&element.kind, element_behavior) {
                (
                    ElementParseKind::Fixed { .. },
                    PrettyElementBehavior::Fixed {
                        spelling,
                        rendering,
                        ..
                    },
                ) => render_pretty_synthetic_fixed(self.plan, spelling, *rendering),
                _ => self.element(element, element_behavior, None, false)?,
            });
        }
        let sequence = render_pretty_sequence(*behavior.variant_layouts.first()?, &rendered);
        let sequence = render_pretty_hints(behavior.variant_pretty.first()?, sequence);
        Some(self.entry(&behavior.pretty, sequence))
    }

    /// Renders the attached fixed syntax of the element on the path around its value, as
    /// [`render_pretty_attachment_unit`] does, with canonical tokens and absent optional entries.
    fn attachment(
        &self,
        attachment: &PrettyAttachmentBehavior,
        value: proc_macro2::TokenStream,
    ) -> Option<proc_macro2::TokenStream> {
        let syntax = |syntax: &PrettyAttachedSyntaxBehavior| match syntax {
            PrettyAttachedSyntaxBehavior::Required(fixed) => {
                render_pretty_synthetic_fixed(self.plan, &fixed.spelling, fixed.rendering)
            }
            PrettyAttachedSyntaxBehavior::Optional(_) => proc_macro2::TokenStream::new(),
        };
        let before = attachment.before.iter().map(syntax).collect::<Vec<_>>();
        let after = attachment.after.iter().map(syntax).collect::<Vec<_>>();
        if attachment.layout == PrettySequenceLayout::Delimited {
            let open = before.first()?;
            let leading = &before[1..];
            let close = after.last()?;
            let trailing = &after[..after.len() - 1];
            Some(quote! {
                #open
                context.indented(|context| {
                    context.soft_break("")?;
                    #(#leading)*
                    #value
                    #(#trailing)*
                    Ok(())
                })?;
                context.soft_break("")?;
                #close
                context.close_delimiter();
            })
        } else {
            Some(quote!(#(#before)* #value #(#after)*))
        }
    }
}

/// Returns a comparable description of one element: its kind, its fixed spellings, and for a
/// semantic field its terminal, wrappers and restriction.
fn element_signature(element: &ElementParsePlan) -> String {
    let attached = |syntax: &[AttachedFixedSyntaxParsePlan]| {
        syntax
            .iter()
            .map(|syntax| match syntax {
                AttachedFixedSyntaxParsePlan::Required(fixed) => fixed.spelling.clone(),
                AttachedFixedSyntaxParsePlan::Optional(tokens) => format!(
                    "[{}]",
                    tokens
                        .iter()
                        .map(|fixed| fixed.spelling.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    match &element.kind {
        ElementParseKind::Fixed { spelling, .. } => format!("fixed {spelling}"),
        ElementParseKind::OptionalFixedTokenGroup { tokens, .. } => format!(
            "optional {}",
            tokens
                .iter()
                .map(|fixed| fixed.spelling.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
            SemanticParseProjection::Direct {
                terminal_node,
                wrappers,
                restriction,
                attachment,
                ..
            } => format!(
                "semantic {terminal_node:?} {wrappers:?} {restriction:?} [{}] [{}]",
                attachment
                    .as_ref()
                    .map_or_else(String::new, |attachment| attached(&attachment.before)),
                attachment
                    .as_ref()
                    .map_or_else(String::new, |attachment| attached(&attachment.after)),
            ),
            SemanticParseProjection::FixedPresence {
                before,
                sequence,
                after,
                ..
            } => format!(
                "presence [{}] [{}] [{}]",
                attached(before),
                sequence
                    .iter()
                    .map(|fixed| fixed.spelling.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                attached(after)
            ),
        },
    }
}

/// Returns the left-edge bound of an interior operand that ends a rule of its own Node in some
/// derivation, or `None` when no rule ends there.
///
/// The parser has then reduced that shorter rule when it sees the token after the operand, so
/// the operand needs the bound of a last operand of that rule. Two shapes make such a rule: the
/// elements after the operand can all be absent (`a LIKE b [ESCAPE c]`), or another variant of
/// the Node is the same sequence up to and including the operand (`LIKE` beside
/// `LIKE ... ESCAPE`). The bound of each such rule is joined with the stricter-bound rule.
fn shadow_minimum(
    facade: &Ident,
    variants: &[VariantParsePlan],
    variant_index: usize,
    element_index: usize,
) -> Option<proc_macro2::TokenStream> {
    let variant = &variants[variant_index];
    let element = variant.elements.get(element_index)?;
    let after_empty = match &element.kind {
        ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
            SemanticParseProjection::Direct { attachment, .. } => attachment
                .as_ref()
                .is_none_or(|attachment| attachment.after.is_empty()),
            SemanticParseProjection::FixedPresence { .. } => return None,
        },
        ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
            return None;
        }
    };
    if element_index == 0 || !after_empty {
        return None;
    }
    let bound = |level: DeclaredVariantLevel| {
        let ordinal = level.ordinal;
        let tie = level.associativity != PrecedenceAssociativity::Right;
        quote!(::core::option::Option::Some((#ordinal, #tie)))
    };
    let mut bounds = Vec::new();
    if variant.elements[element_index + 1..]
        .iter()
        .all(element_can_be_absent)
        && let Some(level) = variant.declared_precedence
    {
        bounds.push(bound(level));
    }
    let prefix = variant.elements[..=element_index]
        .iter()
        .map(element_signature)
        .collect::<Vec<_>>();
    for (index, sibling) in variants.iter().enumerate() {
        if index == variant_index || sibling.elements.len() != element_index + 1 {
            continue;
        }
        let Some(level) = sibling.declared_precedence else {
            continue;
        };
        if sibling
            .elements
            .iter()
            .map(element_signature)
            .eq(prefix.iter().cloned())
        {
            bounds.push(bound(level));
        }
    }
    let mut bounds = bounds.into_iter();
    let first = bounds.next()?;
    Some(bounds.fold(
        first,
        |joined, bound| quote!(#facade::__private::pretty_stricter_bound(#joined, #bound)),
    ))
}

/// Selects the grammar's `breaks = hinted` setting in a generated entry, when the grammar sets it.
pub(super) fn pretty_hinted_breaks(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    if plan.pretty_hinted_breaks {
        quote!(context.configure_hinted_breaks();)
    } else {
        proc_macro2::TokenStream::new()
    }
}

/// Wraps one Node's rendering in its default group, unless the Node opens none (`group = none`),
/// so that its breaks belong to the group around it.
pub(super) fn render_pretty_baseline_group(
    hints: &PrettyHints,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if hints.transparent {
        quote!(#body Ok(()))
    } else {
        quote!(context.baseline_group(|context| { #body Ok(()) }))
    }
}
