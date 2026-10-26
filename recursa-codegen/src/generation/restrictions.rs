//! Restricted expressions in the emission plan (`docs/declared-precedence.md`, "Restricted
//! expressions" and "Consumers").
//!
//! A restricted expression is a grammar nonterminal, never an AST type, so a field that names
//! one holds the base's Rust type. The LR lowering is not the only consumer that must know the
//! restriction: `Arbitrary` must sample only the language of the site's own nonterminal, and
//! `Pretty` must write the parentheses that make a base value parse at the site. This module
//! freezes what both need: for each field, the restriction it parses its base through, and for
//! each base, the variants each restriction admits.

use super::*;

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
/// Every used restriction, grouped by the root base enum Node whose rules it lowers.
///
/// The index of a restriction in its base's list is the dense restriction index a field's
/// [`SemanticParseProjection::Direct`] `restriction` names. The order is the authored order of
/// [`SourceModel::restrictions`] filtered to the base, so [`field_restriction`] and
/// [`plan_restrictions`] number every restriction the same way.
pub(super) struct RestrictionPlan {
    /// Restrictions keyed by the root base Node.
    bases: BTreeMap<NodeId, Vec<PlannedRestriction>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// One restricted expression, projected onto its base's frozen parse plan.
pub(super) struct PlannedRestriction {
    /// The authored name of the restricted expression, for generated documentation.
    pub(super) name: String,
    /// Whether the restriction admits each base variant, in parse-plan variant order.
    pub(super) admitted: Vec<bool>,
    /// `#[restricts(Base, leading)]`: only an operand that leads a rule is retyped.
    pub(super) leading_only: bool,
}

impl RestrictionPlan {
    /// Returns the restrictions of one base Node, in dense index order.
    pub(super) fn of(&self, base: NodeId) -> &[PlannedRestriction] {
        self.bases.get(&base).map_or(&[], Vec::as_slice)
    }
}

/// The operands of one variant that a restriction retypes when it lowers the variant.
///
/// Each entry is an element index of the variant. The lowering retypes the first right-hand-side
/// symbol when it is the base, and the last one when it is the base and the restriction is not
/// `leading`; the base symbol of a variant is a semantic field that names the base Node itself,
/// with no attached token on the side of the rule boundary. A field that names a restriction is
/// another nonterminal, so it is never retyped.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct RestrictedEdges {
    /// The operand that is the first symbol of the rule.
    pub(super) first: Option<usize>,
    /// The operand that is the last symbol of the rule.
    pub(super) last: Option<usize>,
}

/// Reports whether one element is a self operand of `base` that the lowering can retype.
fn is_base_operand(element: &ElementParsePlan, base: NodeId) -> bool {
    let ElementParseKind::Semantic { projection, .. } = &element.kind else {
        return false;
    };
    matches!(
        projection.as_ref(),
        SemanticParseProjection::Direct {
            terminal_node: Some(terminal),
            restriction: None,
            container,
            ..
        } if *terminal == base
            && matches!(container.as_ref(), PlannedContainer::Direct | PlannedContainer::Optional)
    )
}

/// Returns the attachment of one semantic element, when it has one.
fn element_attachment(element: &ElementParsePlan) -> Option<&FixedAttachmentParsePlan> {
    let ElementParseKind::Semantic { projection, .. } = &element.kind else {
        return None;
    };
    match projection.as_ref() {
        SemanticParseProjection::Direct { attachment, .. } => attachment.as_deref(),
        SemanticParseProjection::FixedPresence { .. } => None,
    }
}

/// Derives the operands a restriction retypes in one variant of its base.
///
/// This is the same test as [`DeclaredOperandPosition::classify`]: element 0 with no attached
/// token before it is the first symbol, and the last element with no attached token after it is
/// the last symbol. [`plan_restrictions`] refuses a variant where an element that can be absent
/// stands between an operand and the rule boundary, because there the lowering retypes the
/// operand in some rules and not in others.
pub(super) fn restricted_edges(variant: &VariantParsePlan, base: NodeId) -> RestrictedEdges {
    let len = variant.elements.len();
    let mut edges = RestrictedEdges::default();
    if let Some(first) = variant.elements.first()
        && is_base_operand(first, base)
        && element_attachment(first).is_none_or(|attachment| attachment.before.is_empty())
    {
        edges.first = Some(0);
    }
    if len > 0
        && let Some(last) = variant.elements.last()
        && is_base_operand(last, base)
        && element_attachment(last).is_none_or(|attachment| attachment.after.is_empty())
    {
        edges.last = Some(len - 1);
    }
    edges
}

/// Reports whether one element can be absent from a rule the lowering emits.
pub(super) fn element_can_be_absent(element: &ElementParsePlan) -> bool {
    match &element.kind {
        ElementParseKind::Fixed { .. } => false,
        ElementParseKind::OptionalFixedTokenGroup { .. } => true,
        ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
            SemanticParseProjection::FixedPresence { before, after, .. } => {
                before.is_empty() && after.is_empty()
            }
            SemanticParseProjection::Direct { wrappers, .. } => wrappers.iter().any(|wrapper| {
                matches!(wrapper, FieldWrapper::Optional | FieldWrapper::ZeroOrMore)
            }),
        },
    }
}

/// Reports an admitted variant whose operand is a rule boundary only when an element before
/// or after it is absent.
fn ambiguous_edge(variant: &VariantParsePlan, base: NodeId) -> bool {
    let elements = &variant.elements;
    let leading = elements
        .iter()
        .position(|element| !element_can_be_absent(element));
    if let Some(index) = leading
        && index > 0
        && is_base_operand(&elements[index], base)
        && element_attachment(&elements[index])
            .is_none_or(|attachment| attachment.before.is_empty())
    {
        return true;
    }
    let trailing = elements
        .iter()
        .rposition(|element| !element_can_be_absent(element));
    trailing.is_some_and(|index| {
        index + 1 < elements.len()
            && is_base_operand(&elements[index], base)
            && element_attachment(&elements[index])
                .is_none_or(|attachment| attachment.after.is_empty())
    })
}

/// Finds the restriction a field parses its base through, as its dense index in the base.
///
/// The field's type is walked through its groups and its catalog wrappers to the first type
/// alias. A restricted expression expanded to a type alias of its base, so that alias names the
/// restriction; an ordinary alias is followed to its target.
pub(super) fn field_restriction(
    ty: &ResolvedType,
    resolved: &ResolvedSourceGrammar,
    source: &SourceModel,
) -> Option<u16> {
    match &ty.kind {
        ResolvedTypeKind::Group(inner) | ResolvedTypeKind::Parenthesized(inner) => {
            field_restriction(inner, resolved, source)
        }
        ResolvedTypeKind::Semantic {
            target: SemanticTarget::Catalog(_),
            arguments,
            ..
        } => arguments.iter().find_map(|argument| match argument {
            ResolvedGenericArgument::Type(inner) => field_restriction(inner, resolved, source),
            ResolvedGenericArgument::Lifetime(_) | ResolvedGenericArgument::Unsupported(_) => None,
        }),
        ResolvedTypeKind::Semantic {
            target: SemanticTarget::TypeAlias(alias),
            ..
        } => {
            if let Some(position) = source
                .restrictions
                .iter()
                .position(|restriction| restriction.alias == *alias)
            {
                let base = source.restrictions[position].base;
                let index = source.restrictions[..position]
                    .iter()
                    .filter(|restriction| restriction.base == base)
                    .count();
                return u16::try_from(index).ok();
            }
            resolved.type_aliases[alias.index()]
                .target
                .as_ref()
                .and_then(|target| field_restriction(target, resolved, source))
        }
        _ => None,
    }
}

/// Projects every restricted expression onto its base's frozen parse plan.
///
/// Fails when an admitted variant has an operand that is a rule boundary only when an element
/// beside it is absent: the lowering then retypes the operand in some of the variant's rules and
/// not in others, and neither `Arbitrary` nor `Pretty` can follow that per rule.
pub(super) fn plan_restrictions(
    source: &SourceModel,
    nodes: &[NodeParsePlan],
) -> Result<RestrictionPlan, String> {
    let mut plan = RestrictionPlan::default();
    for restriction in &source.restrictions {
        let entry = plan.bases.entry(restriction.base).or_default();
        let Some(NodeBodyPlan::Enum { variants }) = nodes
            .iter()
            .find(|node| node.node == restriction.base)
            .map(|node| &node.body)
        else {
            entry.push(PlannedRestriction {
                name: restriction.name.clone(),
                admitted: Vec::new(),
                leading_only: restriction.leading_only,
            });
            continue;
        };
        let admitted = variants
            .iter()
            .map(|variant| restriction.variants.contains(&variant.resolved_variant))
            .collect::<Vec<_>>();
        for (variant, admitted) in variants.iter().zip(&admitted) {
            if *admitted && ambiguous_edge(variant, restriction.base) {
                return Err(format!(
                    "restricted expression `{}` admits variant `{}`, whose operand begins or \
                     ends the rule only when an optional element beside it is absent; Pretty and \
                     Arbitrary cannot follow a restriction that retypes the operand in some rules \
                     of the variant only",
                    restriction.name, variant.ident,
                ));
            }
        }
        entry.push(PlannedRestriction {
            name: restriction.name.clone(),
            admitted,
            leading_only: restriction.leading_only,
        });
    }
    Ok(plan)
}
