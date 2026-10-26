//! Parse-plan construction, validation, dispatch, container, and FOLLOW rendering.
//!
//! This module keeps source-aware construction beside executable parser emission. Frozen
//! semantic decisions are consumed here without recomputing analysis during rendering.

use super::*;

#[derive(Clone, Debug, Hash)]
/// Complete generated parsing plan for one node.
pub(super) struct NodeParsePlan {
    /// Stable node identity used to join semantic and generation plans.
    pub(super) node: NodeId,
    /// Canonical schema key published for the node.
    pub(super) semantic_key: String,
    /// Module that owns the emitted parser implementation.
    pub(super) module: ModuleId,
    /// Authored Rust identifier retained for emitted declarations.
    pub(super) ident: Ident,
    /// Concrete AST declaration emitted at the authored macro site.
    pub(super) concrete_item: Option<Item>,
    /// Whether loader compatibility requires a public materialization.
    pub(super) publicly_materialized: bool,
    /// Stable loader symbol used to discover this node.
    pub(super) loader_anchor: String,
    /// Authored generic parameters preserved by parser emission.
    pub(super) generics: Generics,
    /// Lifetime used for source-borrowing fields in generated types.
    pub(super) input_lifetime: syn::Lifetime,
    /// Frozen structural parsing strategy.
    pub(super) body: NodeBodyPlan,
    /// Materialized applications emitted in canonical application order.
    pub(super) parse_applications: Vec<NodeApplicationParsePlan>,
    /// Source-capture policy for the completed node.
    pub(super) capture: PlannedCapturePolicy,
    /// Trait recipes attached to the generated node declaration.
    pub(super) configured_traits: Vec<ConfiguredTraitRecipe>,
    /// Whether Explorer visitation was explicitly configured for this node.
    pub(super) explorer_visit_configured: bool,
    /// Formatting hints shared with Pretty emission.
    pub(super) pretty: PrettyHints,
    /// Flat AST storage selection retained from the declaration's `#[flat(...)]` attribute.
    pub(super) flat: crate::source_model::FlatHints,
}

#[derive(Clone, Debug, Hash)]
/// Parser specialization for one materialized node application.
pub(super) struct NodeApplicationParsePlan {
    /// Stable identity of the materialized application.
    pub(super) application: MaterializedApplicationId,
    /// Node family specialized by this application.
    pub(super) node: NodeId,
    /// Canonical parser index assigned during planning.
    pub(super) parse_index: usize,
    /// Concrete generated result type.
    pub(super) ty: Type,
    /// Source-independent form used by static metadata.
    pub(super) static_ty: Type,
    /// Frozen parse body specialized for the application.
    pub(super) body: NodeBodyPlan,
}

#[derive(Clone, Debug, Hash)]
/// Frozen structural shape consumed by LR lowering.
pub(super) enum NodeBodyPlan {
    /// Parses a single field sequence into a struct.
    Struct {
        /// Construction style for named, tuple, or unit fields.
        style: ResolvedFieldStyle,
        /// Elements parsed in authored field order.
        elements: Vec<ElementParsePlan>,
    },
    /// Selects and parses one enum variant.
    Enum {
        /// Variant bodies in authored declaration order.
        variants: Vec<VariantParsePlan>,
    },
}

#[derive(Clone, Debug, Hash)]
/// Construction and element contract for one resolved enum variant.
pub(super) struct VariantParsePlan {
    /// Stable resolved identity used across semantic products.
    pub(super) resolved_variant: ResolvedVariantId,
    /// Rust identifier used for construction.
    pub(super) ident: Ident,
    /// Named, tuple, or unit construction style.
    pub(super) style: ResolvedFieldStyle,
    /// Parse elements in authored field order.
    pub(super) elements: Vec<ElementParsePlan>,
    /// Optional source-capture policy for the variant.
    pub(super) capture: Option<PlannedCapturePolicy>,
    /// Formatting hints retained for shared projections.
    pub(super) pretty: PrettyHints,
    /// Declared `precedence { ... }` level and associativity for the variant's rule; `None`
    /// when the rule has no declared terminal and no `#[parse(prec = NAME)]` override.
    pub(super) declared_precedence: Option<DeclaredVariantLevel>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// Declared `precedence { ... }` level and associativity assigned to one plain enum variant's
/// rule under the bison-style model that [`crate::lr::declared_precedence`] applies to the LR
/// tables: a `#[parse(prec = NAME)]` override on the variant, else on its Node, else the last
/// fixed token in the variant's authored element sequence, tokens attached to fields included,
/// that names a declared level.
pub(super) struct DeclaredVariantLevel {
    /// Zero-based ordinal into the authored `precedence` block, lowest to highest; a higher
    /// ordinal binds tighter.
    pub(super) ordinal: u32,
    /// The level's associativity.
    pub(super) associativity: PrecedenceAssociativity,
}

/// Computes the declared level a plain enum variant's rule takes, mirroring
/// `lr::declared_precedence::apply`'s rule-precedence assignment so Pretty's parenthesization
/// and the LR resolver never disagree about a rule's level.
///
/// # Long-function contract
///
/// Rationale: keeps override precedence and terminal-scanning precedence beside one another so
/// the bison "variant, then Node, then last declared terminal" order stays visibly total.
/// Invariants: a variant override wins over its Node's, which wins over the last authored
/// terminal with a declared level; a later authored terminal shadows an earlier one exactly as
/// bison's own last-terminal rule does.
/// Phases: checks the variant override, then the Node override, then scans authored elements
/// left to right, including tokens attached to fields, for the last terminal with a level.
pub(super) fn declared_variant_level(
    source: &SourceModel,
    owner_node: NodeId,
    resolved_variant: ResolvedVariantId,
    elements: &[GrammarElement],
) -> Option<DeclaredVariantLevel> {
    if source.precedence.is_empty() {
        return None;
    }
    let level_of = |level: PrecedenceLevelId| DeclaredVariantLevel {
        ordinal: level.get(),
        associativity: source.precedence[level.index()].associativity,
    };
    let override_level = source
        .precedence_overrides
        .iter()
        .find(|over| {
            matches!(
                over.owner,
                PrecedenceOwner::Variant(candidate) if candidate == resolved_variant
            )
        })
        .or_else(|| {
            source.precedence_overrides.iter().find(|over| {
                matches!(over.owner, PrecedenceOwner::Type(candidate) if candidate == owner_node)
            })
        });
    if let Some(over) = override_level {
        return Some(level_of(over.level));
    }
    let level_of_token = |definition: TokenDefinitionId| token_declared_level(source, definition);
    let mut last = None;
    let mut visit = |token_use: TokenUseId| {
        let definition = source.token_uses[token_use.index()].definition;
        if let Some(level) = level_of_token(definition) {
            last = Some(level);
        }
    };
    for element in elements {
        match element {
            GrammarElement::FixedToken { token_use, .. } => visit(*token_use),
            GrammarElement::OptionalFixedTokenGroup { tokens, .. } => {
                for token in tokens {
                    visit(token.token_use);
                }
            }
            GrammarElement::SemanticField { attachment, .. } => {
                let Some(attachment) = attachment else {
                    continue;
                };
                for syntax in attachment.before.iter().chain(attachment.after.iter()) {
                    match syntax {
                        AttachedFixedSyntax::Required(token) => visit(token.token_use),
                        AttachedFixedSyntax::Optional { tokens, .. } => {
                            for token in tokens {
                                visit(token.token_use);
                            }
                        }
                    }
                }
            }
        }
    }
    last
}

/// Returns the declared `precedence { ... }` level of one token definition, or `None` when no
/// level names it.
pub(super) fn token_declared_level(
    source: &SourceModel,
    definition: TokenDefinitionId,
) -> Option<DeclaredVariantLevel> {
    source
        .precedence
        .iter()
        .enumerate()
        .find(|(_, level)| {
            level.kinds.iter().any(
                |kind| matches!(kind, PrecedenceKindReference::Token(candidate) if *candidate == definition),
            )
        })
        .map(|(index, level)| DeclaredVariantLevel {
            ordinal: index as u32,
            associativity: level.associativity,
        })
}

#[derive(Clone, Debug, Hash)]
/// Schema provenance and parser recipe for one authored element.
pub(super) struct ElementParsePlan {
    /// Schema provenance exposed for Explorer occurrences.
    pub(super) explorer_source: ExplorerElementSource,
    /// Stable element name used by generated metadata.
    pub(super) metadata_name: String,
    /// Fixed or semantic parsing recipe.
    pub(super) kind: ElementParseKind,
}

#[derive(Clone, Debug, Hash)]
/// Fixed-token envelope around a semantic container.
pub(super) struct FixedAttachmentParsePlan {
    /// Fixed syntax consumed before the semantic child.
    pub(super) before: Vec<AttachedFixedSyntaxParsePlan>,
    /// Container operation that parses the semantic child.
    pub(super) semantic: Box<PlannedContainer>,
    /// Fixed syntax consumed after the semantic child.
    pub(super) after: Vec<AttachedFixedSyntaxParsePlan>,
}

#[derive(Clone, Debug, Hash)]
/// Classifies required and erased-optional syntax in a generated attachment.
pub(super) enum AttachedFixedSyntaxParsePlan {
    /// Consumes one required fixed token.
    Required(AttachedFixedParsePlan),
    /// Consumes a fixed sequence atomically when its full spelling is present.
    Optional(Vec<AttachedFixedParsePlan>),
}

#[derive(Clone, Debug, Hash)]
/// Matching, capture, and formatting contract for an attached token.
pub(super) struct AttachedFixedParsePlan {
    /// Source-capture policy for the fixed token.
    pub(super) capture: PlannedCapturePolicy,
    /// Canonical fixed spelling.
    pub(super) spelling: String,
    /// Pretty spacing adjacent to the spelling.
    pub(super) spacing: PrettySpacing,
    /// The token's declared `precedence { ... }` level, which Pretty compares when the token is
    /// the parser's lookahead after an operand; `None` for a token with no level.
    pub(super) precedence: Option<DeclaredVariantLevel>,
}

#[derive(Clone, Debug, Hash)]
/// Parser recipe for a fixed token or semantic field.
pub(super) enum ElementParseKind {
    /// Matches an authored fixed token and records its accessor.
    Fixed {
        #[cfg_attr(not(feature = "explorer"), allow(dead_code))]
        /// Runtime token kind exposed to Explorer.
        kind: u16,
        /// Source-capture policy for the token.
        capture: PlannedCapturePolicy,
        /// Canonical authored spelling.
        spelling: String,
        /// Pretty spacing adjacent to the token.
        spacing: PrettySpacing,
        /// The token's declared `precedence { ... }` level; `None` for a token with no level.
        precedence: Option<DeclaredVariantLevel>,
        /// Generated accessor identifying this occurrence.
        accessor: Ident,
    },
    /// Parses one erased fixed-token group atomically when its full sequence is present.
    OptionalFixedTokenGroup {
        /// Fixed tokens selected and consumed as one atomic sequence.
        tokens: Vec<AttachedFixedParsePlan>,
        /// Generated span accessor identifying the optional occurrence.
        accessor: Ident,
    },
    /// Parses an authored semantic field through its selected value projection.
    Semantic {
        /// Construction slot receiving the parsed value.
        field: FieldPlan,
        /// Selects direct semantic parsing or boolean fixed-presence projection.
        projection: Box<SemanticParseProjection>,
    },
}

#[derive(Clone, Debug, Hash)]
/// Classifies how generated parsing constructs one authored semantic field value.
pub(super) enum SemanticParseProjection {
    /// Parses the authored field type through its container policy.
    Direct {
        /// Authored terminal type before materialization.
        terminal: Box<Type>,
        /// Resolved node family, when the terminal is generated.
        terminal_node: Option<NodeId>,
        /// Materialized generated-node application, when available.
        terminal_application: Option<MaterializedApplicationId>,
        /// Grammar-erased terminal projection for Explorer schema.
        #[cfg_attr(not(feature = "explorer"), allow(dead_code))]
        explorer_terminal: ExplorerTerminalPlan,
        /// Authored type wrappers, outermost first.
        wrappers: Vec<FieldWrapper>,
        /// Optionality or repetition control-flow recipe.
        container: Box<PlannedContainer>,
        /// Source-capture policy for the stored field value.
        capture: PlannedCapturePolicy,
        /// Source-capture policy applied to each parsed child.
        child_capture: PlannedCapturePolicy,
        /// Canonical repetition separator spelling, if any.
        separator: Option<String>,
        /// Pretty spacing declared by the repetition separator, if any.
        separator_spacing: Option<PrettySpacing>,
        /// Fixed tokens attached around the semantic child.
        attachment: Option<Box<FixedAttachmentParsePlan>>,
        /// Whether the terminal hides generated-node provenance.
        opaque_provenance: bool,
        /// Pretty line-breaking policy for repeated children.
        repetition_break: PlannedRepetitionBreak,
        /// Formatting hints retained for the field.
        pretty: PrettyHints,
        /// The restricted expression the field parses its base through, as its dense index in
        /// the base's [`RestrictionPlan`] list; `None` at every ordinary field.
        restriction: Option<u16>,
    },
    /// Parses required fixed syntax around one atomic optional sequence stored as `bool`.
    FixedPresence {
        /// Fixed syntax before the optional sequence.
        before: Vec<AttachedFixedSyntaxParsePlan>,
        /// Fixed tokens selected and consumed as one atomic sequence.
        sequence: Vec<AttachedFixedParsePlan>,
        /// Fixed syntax after the optional sequence.
        after: Vec<AttachedFixedSyntaxParsePlan>,
        /// Formatting hints retained for the field.
        pretty: PrettyHints,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Pretty break behavior retained for a repeated parse field.
pub(super) enum PlannedRepetitionBreak {
    /// Allows the formatter to choose a repetition break.
    Soft,
    /// Breaks between occurrences of one materialized statement type.
    TopLevelStatement(MaterializedApplicationId),
    /// Breaks between occurrences from one statement node family.
    TopLevelStatementFamily(NodeId),
}

#[derive(Clone, Debug, Hash)]
/// Construction position and optional named binding for a parsed field.
pub(super) struct FieldPlan {
    /// Named-field binding, absent for tuple and unit positions.
    pub(super) ident: Option<Ident>,
    /// Zero-based construction position in authored field order.
    pub(super) index: usize,
    /// Generated accessor when the field is addressable by name.
    pub(super) accessor: Option<Ident>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Storage constructor applied around an authored terminal type.
pub(super) enum FieldWrapper {
    /// Allocates the child behind `Box`.
    Boxed,
    /// Stores absence or one child in `Option`.
    Optional,
    /// Collects any number of children.
    ZeroOrMore,
    /// Collects children while enforcing a non-empty result.
    OneOrMore,
}

#[derive(Clone, Debug, Hash)]
/// Authored cardinality retained for shared Pretty and diagram projections.
pub(super) enum PlannedContainer {
    /// Requires exactly one child.
    Direct,
    /// Stores at most one child.
    Optional,
    /// Parses a bounded or unbounded sequence with optional separators.
    Repeated {
        /// Collection cardinality and construction operation.
        operation: ContainerOperation,
        /// Whether the repetition has a configured separator.
        has_separator: bool,
        /// Whether a separator may terminate the sequence.
        optional_trailing: bool,
    },
}

#[derive(Clone, Debug)]
/// Grammar-scoped direct lookup from token kinds to their canonical atom identities.
pub(super) struct ExpectedAtomIndex {
    /// First direct atom declared for each grammar-local token kind.
    by_kind: Vec<Option<u32>>,
}

impl ExpectedAtomIndex {
    /// Indexes direct kind atoms once while preserving the former first-match rule.
    pub(super) fn from_grammar(grammar: &crate::GrammarDescription) -> Result<Self, String> {
        let mut by_kind = vec![None; grammar.kind_count];
        for (atom, description) in grammar.atoms.iter().enumerate() {
            let AtomExpression::Kind(kind) = description.expression else {
                continue;
            };
            let slot = by_kind
                .get_mut(kind.index())
                .ok_or_else(|| "generated expectation kind exceeds grammar width".to_owned())?;
            if slot.is_none() {
                *slot = Some(
                    atom.try_into()
                        .map_err(|_| "generated expectation atom exceeds runtime width")?,
                );
            }
        }
        Ok(Self { by_kind })
    }

    /// Returns the canonical direct atom for one validated token kind.
    pub(super) fn get(&self, kind: crate::KindId) -> Option<u32> {
        self.by_kind.get(kind.index()).copied().flatten()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Source-capture policy after inheritance has been resolved or retained.
pub(super) enum PlannedCapturePolicy {
    /// Defers capture choice to the caller context.
    Inherited,
    /// Records no source capture.
    Off,
    /// Captures source when the runtime context enables it.
    Optional,
    /// Requires source capture for the parsed value.
    Mandatory,
}

impl PlannedCapturePolicy {
    /// Returns the stable capture-policy spelling used in plan identities.
    pub(super) const fn canonical_name(self) -> &'static str {
        match self {
            Self::Inherited => "inherited",
            Self::Off => "off",
            Self::Optional => "optional",
            Self::Mandatory => "mandatory",
        }
    }

    /// Emits the corresponding runtime capture-policy expression.
    pub(super) fn render(self, facade: &Ident) -> proc_macro2::TokenStream {
        match self {
            Self::Inherited => quote!(context.capture_policy()),
            Self::Off => quote!(#facade::__private::CapturePolicy::Off),
            Self::Optional => quote!(#facade::__private::CapturePolicy::Optional),
            Self::Mandatory => quote!(#facade::__private::CapturePolicy::Mandatory),
        }
    }
}

impl From<CaptureMode> for PlannedCapturePolicy {
    fn from(value: CaptureMode) -> Self {
        match value {
            CaptureMode::Off => Self::Off,
            CaptureMode::Optional => Self::Optional,
            CaptureMode::Mandatory => Self::Mandatory,
        }
    }
}

/// Converts one analyzed FIRST position into a runtime dispatch condition.
pub(super) fn structural_body(
    node: &ResolvedNode,
    source_node: &SourceNode,
    _materialized_body: &MaterializedNodeBody,
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
) -> Result<NodeBodyPlan, String> {
    let body = match (&source_node.body, &node.body) {
        (
            SourceNodeBody::Struct(elements) | SourceNodeBody::Presence(elements),
            ResolvedNodeBody::Struct { style },
        ) => NodeBodyPlan::Struct {
            style: *style,
            elements: declaration_elements(node, None, elements, lowered, kind_count)?,
        },
        (
            SourceNodeBody::Enum(variants),
            ResolvedNodeBody::Enum {
                variants: resolved_variants,
            },
        ) => {
            let variants = variants
                .iter()
                .map(|variant| {
                    let resolved_variant = resolved_variants
                        .iter()
                        .find(|resolved| resolved.id == variant.resolved_variant)
                        .ok_or_else(|| "authored enum variant lacks resolved shape".to_owned())?;
                    let source = lowered.source_modeled.source_model();
                    Ok(VariantParsePlan {
                        resolved_variant: resolved_variant.id,
                        ident: emitted_ident(&variant.name)?,
                        style: resolved_variant.style,
                        elements: declaration_elements(
                            node,
                            Some((variant, resolved_variant)),
                            &variant.elements,
                            lowered,
                            kind_count,
                        )?,
                        capture: resolved_variant.capture.map(PlannedCapturePolicy::from),
                        pretty: variant.pretty.clone(),
                        declared_precedence: declared_variant_level(
                            source,
                            node.id,
                            resolved_variant.id,
                            &variant.elements,
                        ),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            NodeBodyPlan::Enum { variants }
        }
        _ => {
            return Err(format!(
                "Node `{}` authored and resolved declaration shapes disagree",
                node.ident
            ));
        }
    };
    let mut body = body;
    disambiguate_fixed_accessors(&mut body)?;
    validate_accessor_names(node, &body)?;
    Ok(body)
}

/// Builds one LR node plan without predictive decisions or FOLLOW recipes.
pub(super) fn plan_lr_node(
    node: &ResolvedNode,
    source_node: &SourceNode,
    application: &MaterializedNodeApplication,
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
) -> Result<(NodeParsePlan, NodeBodyPlan), String> {
    let source = &lowered.source_modeled.resolved().tree.files[node.source.index()];
    let physical_source = normalized_source_identity(&source.path)?;
    let (line, column) = source_line_column(&source.text, node.range.start)?;
    let loader_anchor = declaration_loader_anchor(&physical_source, line, column, &node.ident);
    let input_lifetime = node
        .generics
        .params
        .iter()
        .find_map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => Some(parameter.lifetime.clone()),
            GenericParam::Type(_) | GenericParam::Const(_) => None,
        })
        .unwrap_or_else(|| syn::parse_quote!('__recursa_input));
    let materialization_lifetime: syn::Lifetime =
        if lowered.source_modeled.resolved().build_inputs.arena_ast {
            syn::parse_quote!('__recursa_arena)
        } else {
            syn::parse_quote!('__recursa_input)
        };
    let body = structural_body(node, source_node, &application.body, lowered, kind_count)?;
    let mut application_body = body.clone();
    materialize_lr_body(
        node,
        &mut application_body,
        &application.body,
        lowered,
        &materialization_lifetime,
    )
    .map_err(|detail| {
        format!(
            "LR plan for Node `{}` application `{}`: {detail}",
            node.ident, application.display_name
        )
    })?;
    Ok((
        NodeParsePlan {
            node: node.id,
            semantic_key: node.key.segments.join("::"),
            module: node.module,
            ident: emitted_ident(&node.ident)?,
            concrete_item: match &node.origin {
                ResolvedNodeOrigin::Declaration(origin) => Some(origin.concrete_item.clone()),
                ResolvedNodeOrigin::DirectRust => None,
            },
            publicly_materialized: publicly_reachable(
                &lowered.source_modeled.resolved().tree,
                node.module,
                &node.visibility,
            ),
            loader_anchor,
            generics: node.generics.clone(),
            input_lifetime,
            body,
            parse_applications: Vec::new(),
            capture: node
                .capture
                .unwrap_or(lowered.source_modeled.resolved().build_inputs.spans)
                .into(),
            configured_traits: Vec::new(),
            explorer_visit_configured: false,
            pretty: application.pretty.clone(),
            // `#[flat(...)]` is declaration-level, so every application of one declaration shares
            // the source Node's hints; there is nothing per-application to materialize.
            flat: source_node.flat,
        },
        application_body,
    ))
}

/// Substitutes a closed materialized application's terminal types into a declaration plan.
fn materialize_lr_body(
    node: &ResolvedNode,
    planned: &mut NodeBodyPlan,
    materialized: &MaterializedNodeBody,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<(), String> {
    match (planned, materialized) {
        (NodeBodyPlan::Struct { elements, .. }, MaterializedNodeBody::Struct(values))
        | (NodeBodyPlan::Struct { elements, .. }, MaterializedNodeBody::Presence(values)) => {
            materialize_lr_elements(node, elements, values, lowered, input_lifetime)
        }
        (NodeBodyPlan::Enum { variants, .. }, MaterializedNodeBody::Enum(values)) => {
            for value in values {
                let variant = variants
                    .iter_mut()
                    .find(|variant| variant.resolved_variant == value.resolved_variant)
                    .ok_or_else(|| {
                        "materialized enum variant lacks a declaration plan".to_owned()
                    })?;
                materialize_lr_elements(
                    node,
                    &mut variant.elements,
                    &value.elements,
                    lowered,
                    input_lifetime,
                )?;
            }
            Ok(())
        }
        _ => Err("materialized and declared LR node shapes disagree".to_owned()),
    }
}

/// Joins materialized LR elements by source identity and closes semantic field types.
///
/// A fixed element with no declaration-tail entry is reconstructed here.
fn materialize_lr_elements(
    node: &ResolvedNode,
    planned: &mut Vec<ElementParsePlan>,
    materialized: &[MaterializedElement],
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<(), String> {
    let mut declarations = std::mem::take(planned);
    let mut joined = Vec::with_capacity(materialized.len());
    for materialized in materialized {
        let matched = declarations.iter().position(|planned| {
            matches!(
                (&planned.explorer_source, materialized),
                (
                    ExplorerElementSource::Fixed(left),
                    MaterializedElement::FixedToken { token_use: right }
                ) if left == right
            ) || matches!(
                (&planned.explorer_source, materialized),
                (
                    ExplorerElementSource::OptionalFixed(left),
                    MaterializedElement::OptionalFixedTokenGroup { element: right, .. }
                ) if left == right
            ) || matches!(
                (&planned.explorer_source, materialized),
                (
                    ExplorerElementSource::Semantic(left),
                    MaterializedElement::SemanticField { field: right, .. }
                ) if left == right
            )
        });
        let mut element = if let Some(index) = matched {
            declarations.remove(index)
        } else if let MaterializedElement::FixedToken { token_use } = materialized {
            declaration_fixed_element(node, *token_use, lowered)?
        } else {
            return Err("materialized LR element lacks its declaration plan".to_owned());
        };
        close_lr_semantic_element(&mut element, materialized, lowered, input_lifetime)?;
        joined.push(element);
    }
    if !declarations.is_empty() {
        return Err(format!(
            "declared LR elements are absent from materialization: {:?}",
            declarations
                .iter()
                .map(|element| element.metadata_name.as_str())
                .collect::<Vec<_>>()
        ));
    }
    *planned = joined;
    Ok(())
}

/// Builds a standalone fixed element that has no declaration-tail entry of its own.
fn declaration_fixed_element(
    node: &ResolvedNode,
    token_use: crate::lowering::TokenUseId,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<ElementParsePlan, String> {
    let source = lowered.source_modeled.source_model();
    let usage = &source.token_uses[token_use.index()];
    let definition = &source.token_definitions[usage.definition.index()];
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("a materialized fixed LR element references non-fixed syntax".to_owned());
    };
    let name = fixed_element_name(node, usage, definition, fixed);
    Ok(ElementParsePlan {
        explorer_source: ExplorerElementSource::Fixed(token_use),
        metadata_name: name.clone(),
        kind: ElementParseKind::Fixed {
            kind: explorer_kind_index(lowered, definition.id)?,
            capture: PlannedCapturePolicy::Inherited,
            spelling: fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence),
            spacing: fixed.pretty_spacing,
            precedence: token_declared_level(source, definition.id),
            accessor: fixed_accessor_ident(&name)?,
        },
    })
}

/// Substitutes a closed materialized type into one semantic declaration element.
fn close_lr_semantic_element(
    planned: &mut ElementParsePlan,
    materialized: &MaterializedElement,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<(), String> {
    let (
        ElementParseKind::Semantic {
            projection: planned,
            ..
        },
        MaterializedElement::SemanticField {
            projection: MaterializedSemanticFieldProjection::Direct { ty, .. },
            ..
        },
    ) = (&mut planned.kind, materialized)
    else {
        return Ok(());
    };
    let SemanticParseProjection::Direct {
        terminal,
        terminal_node,
        terminal_application,
        explorer_terminal: explorer,
        wrappers,
        opaque_provenance,
        repetition_break,
        separator,
        ..
    } = planned.as_mut()
    else {
        return Err("materialized direct LR field disagrees with its declaration".to_owned());
    };
    **terminal = terminal_type(ty, lowered, input_lifetime)?;
    *wrappers = field_wrappers(ty)?;
    *terminal_application = closed_terminal_application(ty);
    *terminal_node = terminal_application.and_then(|application| {
        lowered
            .applications
            .iter()
            .find(|candidate| candidate.id == application)
            .map(|candidate| candidate.node)
    });
    *explorer = explorer_terminal(ty)?;
    *opaque_provenance = false;
    if let Some(application) = *terminal_application
        && separator.as_deref() == Some(";")
        && matches!(
            declaration_container_operation(wrappers),
            ContainerOperation::ZeroOrMore
                | ContainerOperation::OneOrMore
                | ContainerOperation::OptionalOneOrMore
        )
        && lowered.applications.iter().any(|candidate| {
            candidate.id == application && matches!(candidate.body, MaterializedNodeBody::Enum(_))
        })
    {
        *repetition_break = PlannedRepetitionBreak::TopLevelStatement(application);
    }
    Ok(())
}

/// Plans declaration elements in authored field order.
///
/// # Long-function contract
///
/// Rationale: freezes declaration-local loader and visibility facts before render.
/// Invariants: selected-root anchors and semantic keys remain source-relative and deterministic.
/// Phases: resolves inclusion, derives loader anchors, then records generic and visibility facts.
pub(super) fn declaration_elements(
    node: &ResolvedNode,
    variant: Option<(
        &crate::source_model::Variant,
        &crate::resolution::ResolvedNodeVariant,
    )>,
    elements: &[GrammarElement],
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
) -> Result<Vec<ElementParsePlan>, String> {
    let source = lowered.source_modeled.source_model();
    let resolved = lowered.source_modeled.resolved();
    elements
        .iter()
        .map(|element| match element {
            GrammarElement::FixedToken { token_use, .. } => {
                let usage = &source.token_uses[token_use.index()];
                let definition = &source.token_definitions[usage.definition.index()];
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return Err("an authored fixed element references a non-fixed token".to_owned());
                };
                let name = fixed_element_name(node, usage, definition, fixed);
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::Fixed(*token_use),
                    metadata_name: name.clone(),
                    kind: ElementParseKind::Fixed {
                        kind: explorer_kind_index(lowered, definition.id)?,
                        capture: PlannedCapturePolicy::Inherited,
                        spelling: fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        ),
                        spacing: fixed.pretty_spacing,
                        precedence: token_declared_level(source, definition.id),
                        accessor: fixed_accessor_ident(&name)?,
                    },
                })
            }
            GrammarElement::OptionalFixedTokenGroup { id, tokens, .. } => {
                let metadata_name = format!(
                    "optional_{}",
                    tokens
                        .iter()
                        .map(|token| {
                            let usage = &source.token_uses[token.token_use.index()];
                            snake_case(&source.token_definitions[usage.definition.index()].name)
                        })
                        .collect::<Vec<_>>()
                        .join("_")
                );
                let plan_fixed = |token: &crate::source_model::AttachedFixedToken| {
                    let usage = &source.token_uses[token.token_use.index()];
                    let definition = &source.token_definitions[usage.definition.index()];
                    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                        return Err("an optional group references non-fixed syntax".to_owned());
                    };
                    Ok(AttachedFixedParsePlan {
                        capture: PlannedCapturePolicy::Mandatory,
                        spelling: fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        ),
                        spacing: fixed.pretty_spacing,
                        precedence: token_declared_level(source, definition.id),
                    })
                };
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::OptionalFixed(*id),
                    metadata_name: metadata_name.clone(),
                    kind: ElementParseKind::OptionalFixedTokenGroup {
                        tokens: tokens.iter().map(plan_fixed).collect::<Result<_, _>>()?,
                        accessor: fixed_accessor_ident(&metadata_name)?,
                    },
                })
            }
            GrammarElement::SemanticField {
                resolved_field,
                projection,
                attachment,
                separator,
                pretty,
                ..
            } => {
                let field = node
                    .fields
                    .iter()
                    .find(|field| field.id == *resolved_field)
                    .ok_or_else(|| "an authored semantic element lacks its field".to_owned())?;
                let field_index = variant
                    .map_or_else(
                        || {
                            node.fields
                                .iter()
                                .position(|candidate| candidate.id == field.id)
                        },
                        |(_, resolved)| {
                            resolved
                                .fields
                                .iter()
                                .position(|candidate| *candidate == field.id)
                        },
                    )
                    .ok_or_else(|| {
                        "an authored semantic element lacks its field index".to_owned()
                    })?;
                let base = field
                    .syntax
                    .ident
                    .clone()
                    .unwrap_or_else(|| format_ident!("field_{field_index}"));
                let (metadata_name, accessor) = variant.map_or_else(
                    || (base.unraw().to_string(), base.clone()),
                    |(source_variant, _)| {
                        let prefix = snake_case(&source_variant.name);
                        let accessor = if base.unraw() == prefix {
                            base.clone()
                        } else {
                            format_ident!("{}_{}", prefix, base)
                        };
                        (format!("{}_{}", prefix, base.unraw()), accessor)
                    },
                );
                let projection = match projection {
                    crate::source_model::SemanticFieldProjection::FixedPresence { tokens } => {
                        let plan_fixed = |token_use: crate::lowering::TokenUseId| {
                            declaration_fixed_token(
                                token_use,
                                lowered,
                                PlannedCapturePolicy::Inherited,
                            )
                        };
                        let attachment = attachment
                            .as_ref()
                            .map(|attachment| declaration_fixed_attachment(attachment, lowered))
                            .transpose()?;
                        SemanticParseProjection::FixedPresence {
                            before: attachment
                                .as_ref()
                                .map_or_else(Vec::new, |attachment| attachment.before.clone()),
                            sequence: tokens
                                .iter()
                                .map(|token| plan_fixed(token.token_use))
                                .collect::<Result<_, _>>()?,
                            after: attachment.map_or_else(Vec::new, |attachment| attachment.after),
                            pretty: pretty.clone(),
                        }
                    }
                    crate::source_model::SemanticFieldProjection::Direct => {
                        let authored = authored_projection(&field.ty, resolved)?;
                        let operation = declaration_container_operation(&authored.wrappers);
                        let separator_behavior = separator
                            .as_ref()
                            .map(|separator| {
                                let usage = &source.token_uses[separator.token_use.index()];
                                let definition =
                                    &source.token_definitions[usage.definition.index()];
                                match &definition.kind {
                                    TokenDefinitionKind::Fixed(fixed) => Ok((
                                        fixed_pretty_spelling(
                                            &fixed.spelling,
                                            lowered.grammar.text_equivalence,
                                        ),
                                        fixed.pretty_spacing,
                                    )),
                                    _ => Err("an authored separator references a non-fixed token"
                                        .to_owned()),
                                }
                            })
                            .transpose()?;
                        let repetition_break = authored
                            .terminal_node
                            .filter(|terminal_node| {
                                separator_behavior
                                    .as_ref()
                                    .is_some_and(|(spelling, _)| spelling == ";")
                                    && matches!(
                                        operation,
                                        ContainerOperation::ZeroOrMore
                                            | ContainerOperation::OneOrMore
                                            | ContainerOperation::OptionalOneOrMore
                                    )
                                    && matches!(
                                        resolved.nodes[terminal_node.index()].body,
                                        ResolvedNodeBody::Enum { .. }
                                    )
                            })
                            .map(PlannedRepetitionBreak::TopLevelStatementFamily)
                            .unwrap_or(PlannedRepetitionBreak::Soft);
                        SemanticParseProjection::Direct {
                            terminal: Box::new(authored.terminal),
                            terminal_node: authored.terminal_node,
                            terminal_application: None,
                            explorer_terminal: declaration_explorer_terminal(
                                lowered,
                                node.id,
                                *resolved_field,
                            )?,
                            wrappers: authored.wrappers,
                            container: Box::new(declaration_container(
                                operation,
                                separator.is_some(),
                                separator
                                    .as_ref()
                                    .is_some_and(|separator| separator.optional_trailing),
                                kind_count,
                            )),
                            capture: PlannedCapturePolicy::Inherited,
                            child_capture: PlannedCapturePolicy::Inherited,
                            separator: separator_behavior
                                .as_ref()
                                .map(|(spelling, _)| spelling.clone()),
                            separator_spacing: separator_behavior.map(|(_, spacing)| spacing),
                            attachment: attachment
                                .as_ref()
                                .map(|attachment| {
                                    declaration_fixed_attachment(attachment, lowered).map(Box::new)
                                })
                                .transpose()?,
                            opaque_provenance: authored_type_depends_on_node_generics(
                                &field.ty, node, resolved,
                            ),
                            repetition_break,
                            pretty: pretty.clone(),
                            restriction: field_restriction(&field.ty, resolved, source),
                        }
                    }
                };
                Ok(ElementParsePlan {
                    explorer_source: ExplorerElementSource::Semantic(*resolved_field),
                    metadata_name,
                    kind: ElementParseKind::Semantic {
                        field: FieldPlan {
                            ident: field.syntax.ident.clone(),
                            index: field_index,
                            accessor: Some(accessor),
                        },
                        projection: Box::new(projection),
                    },
                })
            }
        })
        .collect()
}

/// Freezes fixed tokens attached before and after a semantic field.
pub(super) fn declaration_fixed_attachment(
    attachment: &crate::source_model::FixedTokenAttachment,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<FixedAttachmentParsePlan, String> {
    let plan_fixed = |token_use: crate::lowering::TokenUseId| {
        declaration_fixed_token(token_use, lowered, PlannedCapturePolicy::Inherited)
    };
    let plan_syntax = |syntax: &crate::source_model::AttachedFixedSyntax| match syntax {
        crate::source_model::AttachedFixedSyntax::Required(token) => {
            plan_fixed(token.token_use).map(AttachedFixedSyntaxParsePlan::Required)
        }
        crate::source_model::AttachedFixedSyntax::Optional { tokens, .. } => tokens
            .iter()
            .map(|token| plan_fixed(token.token_use))
            .collect::<Result<Vec<_>, _>>()
            .map(AttachedFixedSyntaxParsePlan::Optional),
    };
    Ok(FixedAttachmentParsePlan {
        before: attachment
            .before
            .iter()
            .map(plan_syntax)
            .collect::<Result<_, _>>()?,
        semantic: Box::new(PlannedContainer::Direct),
        after: attachment
            .after
            .iter()
            .map(plan_syntax)
            .collect::<Result<_, _>>()?,
    })
}

/// Freezes one authored fixed-token use for generated attachment or presence parsing.
fn declaration_fixed_token(
    token_use: crate::lowering::TokenUseId,
    lowered: &crate::materialization::LoweredGrammar,
    capture: PlannedCapturePolicy,
) -> Result<AttachedFixedParsePlan, String> {
    let source = lowered.source_modeled.source_model();
    let usage = &source.token_uses[token_use.index()];
    let definition = &source.token_definitions[usage.definition.index()];
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("an authored attachment references non-fixed syntax".to_owned());
    };
    Ok(AttachedFixedParsePlan {
        capture,
        spelling: fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence),
        spacing: fixed.pretty_spacing,
        precedence: token_declared_level(source, definition.id),
    })
}

/// Projects a resolved terminal into grammar-erased Explorer metadata.
pub(super) fn declaration_explorer_terminal(
    lowered: &crate::materialization::LoweredGrammar,
    node: NodeId,
    field: ResolvedFieldId,
) -> Result<ExplorerTerminalPlan, String> {
    let application = lowered
        .applications
        .iter()
        .find(|application| application.node == node)
        .ok_or_else(|| "an authored explorer field lacks a closed Node application".to_owned())?;
    let explorer = authoritative_explorer_node(lowered, application)?;
    let variants: Vec<&ExplorerVariantPlan> = match &explorer.body {
        ExplorerNodeBodyPlan::Struct(variant) => vec![variant],
        ExplorerNodeBodyPlan::Enum(variants) => variants.iter().collect(),
    };
    variants
        .into_iter()
        .flat_map(|variant| &variant.elements)
        .find_map(|element| {
            matches!(
                element.source,
                ExplorerElementSource::Semantic(candidate) if candidate == field
            )
            .then_some(&element.occurrence)
        })
        .and_then(|occurrence| match occurrence {
            ExplorerOccurrencePlan::Children { terminal, .. } => Some(*terminal),
            ExplorerOccurrencePlan::Leaf => None,
        })
        .ok_or_else(|| "an authored explorer field lacks a materialized terminal".to_owned())
}

/// Derives the runtime collection operation from authored wrappers.
pub(super) fn declaration_container_operation(wrappers: &[FieldWrapper]) -> ContainerOperation {
    let repetition = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore));
    let optional_before_repeat = repetition.is_some_and(|index| {
        wrappers[..index]
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    });
    match repetition.map(|index| wrappers[index]) {
        Some(FieldWrapper::ZeroOrMore) => ContainerOperation::ZeroOrMore,
        Some(FieldWrapper::OneOrMore) if optional_before_repeat => {
            ContainerOperation::OptionalOneOrMore
        }
        Some(FieldWrapper::OneOrMore) => ContainerOperation::OneOrMore,
        Some(FieldWrapper::Boxed | FieldWrapper::Optional) => unreachable!(),
        None if wrappers
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional)) =>
        {
            ContainerOperation::Optional
        }
        None => ContainerOperation::Direct,
    }
}

/// Plans optional or repeated control flow and its failure evidence.
pub(super) fn declaration_container(
    operation: ContainerOperation,
    has_separator: bool,
    optional_trailing: bool,
    _kind_count: usize,
) -> PlannedContainer {
    match operation {
        ContainerOperation::Direct => PlannedContainer::Direct,
        ContainerOperation::Optional => PlannedContainer::Optional,
        operation @ (ContainerOperation::ZeroOrMore
        | ContainerOperation::OneOrMore
        | ContainerOperation::OptionalOneOrMore) => PlannedContainer::Repeated {
            operation,
            has_separator,
            optional_trailing,
        },
    }
}

#[derive(Clone)]
/// Authored terminal type projected through its storage wrappers.
pub(super) struct AuthoredProjection {
    /// Terminal type after applying resolved generic bindings.
    pub(super) terminal: Type,
    /// Generated node family referenced by the terminal, when any.
    pub(super) terminal_node: Option<NodeId>,
    /// Authored storage wrappers, outermost first.
    pub(super) wrappers: Vec<FieldWrapper>,
}

#[derive(Clone, Default)]
/// Generic substitutions available while projecting authored types.
pub(super) struct AuthoredBindings {
    /// Type arguments keyed by their resolved parameter identities.
    pub(super) types: BTreeMap<GenericParameterId, ResolvedType>,
    /// Lifetime arguments keyed by authored parameter names.
    pub(super) lifetimes: BTreeMap<String, syn::Lifetime>,
}

/// Reports whether a type references generics owned by the enclosing node.
pub(super) fn authored_type_depends_on_node_generics(
    ty: &ResolvedType,
    node: &ResolvedNode,
    resolved: &ResolvedSourceGrammar,
) -> bool {
    match &ty.kind {
        ResolvedTypeKind::Semantic {
            use_site,
            target,
            arguments,
            ..
        } => {
            let parameter_is_owned = |parameter| {
                resolved.generic_parameters.iter().any(|candidate| {
                    candidate.id == parameter && candidate.owner == SemanticOwner::Node(node.id)
                })
            };
            matches!(target, SemanticTarget::GenericParameter(parameter) if parameter_is_owned(*parameter))
                || (matches!(target, SemanticTarget::Node(owner) if *owner == node.id)
                    && !node.generics.params.is_empty()
                    && resolved.uses[use_site.index()].authored == "Self")
                || arguments.iter().any(|argument| match argument {
                    ResolvedGenericArgument::Type(argument) => {
                        authored_type_depends_on_node_generics(argument, node, resolved)
                    }
                    ResolvedGenericArgument::Lifetime(_)
                    | ResolvedGenericArgument::Unsupported(_) => false,
                })
        }
        ResolvedTypeKind::Group(inner)
        | ResolvedTypeKind::Parenthesized(inner)
        | ResolvedTypeKind::Reference(inner) => {
            authored_type_depends_on_node_generics(inner, node, resolved)
        }
        ResolvedTypeKind::Tuple(elements) => elements
            .iter()
            .any(|element| authored_type_depends_on_node_generics(element, node, resolved)),
        ResolvedTypeKind::FixedTokenPresence | ResolvedTypeKind::Unsupported => false,
    }
}

/// Resolves an authored terminal and wrappers under the available bindings.
pub(super) fn authored_projection(
    ty: &ResolvedType,
    resolved: &ResolvedSourceGrammar,
) -> Result<AuthoredProjection, String> {
    authored_projection_with(
        ty,
        resolved,
        &AuthoredBindings::default(),
        &mut BTreeSet::new(),
    )
}

/// Resolves an authored projection after extending the current bindings.
///
/// # Long-function contract
///
/// Rationale: projects one materialized body back into authored grammar-element order.
/// Invariants: fixed tokens and semantic fields retain exact source correlations.
/// Phases: indexes lowering correlations, walks authored elements, then validates complete coverage.
pub(super) fn authored_projection_with(
    ty: &ResolvedType,
    resolved: &ResolvedSourceGrammar,
    bindings: &AuthoredBindings,
    active_aliases: &mut BTreeSet<usize>,
) -> Result<AuthoredProjection, String> {
    match &ty.kind {
        ResolvedTypeKind::Group(inner) | ResolvedTypeKind::Parenthesized(inner) => {
            authored_projection_with(inner, resolved, bindings, active_aliases)
        }
        ResolvedTypeKind::Semantic {
            use_site,
            target,
            arguments,
            ..
        } => match target {
            SemanticTarget::GenericParameter(parameter) => {
                bindings.types.get(parameter).map_or_else(
                    || {
                        Ok(AuthoredProjection {
                            terminal: ty.syntax.clone(),
                            terminal_node: None,
                            wrappers: Vec::new(),
                        })
                    },
                    |bound| authored_projection_with(bound, resolved, bindings, active_aliases),
                )
            }
            SemanticTarget::Catalog(catalog) => {
                let argument = arguments.iter().find_map(|argument| match argument {
                    ResolvedGenericArgument::Type(ty) => Some(ty),
                    ResolvedGenericArgument::Lifetime(_)
                    | ResolvedGenericArgument::Unsupported(_) => None,
                });
                let argument = argument.ok_or_else(|| {
                    format!("authored catalog type `{catalog:?}` has no type argument")
                })?;
                let mut projected =
                    authored_projection_with(argument, resolved, bindings, active_aliases)?;
                let wrapper = match catalog {
                    CatalogType::Option => FieldWrapper::Optional,
                    CatalogType::Box => FieldWrapper::Boxed,
                    CatalogType::Vec => FieldWrapper::ZeroOrMore,
                    CatalogType::Vec1 => FieldWrapper::OneOrMore,
                };
                projected.wrappers.insert(0, wrapper);
                Ok(projected)
            }
            SemanticTarget::TypeAlias(alias_id) => {
                if !active_aliases.insert(alias_id.index()) {
                    return Err("an authored type-alias projection contains a cycle".to_owned());
                }
                let alias = &resolved.type_aliases[alias_id.index()];
                let parameter_ids = resolved
                    .generic_parameters
                    .iter()
                    .filter(|parameter| parameter.owner == SemanticOwner::TypeAlias(*alias_id))
                    .map(|parameter| (parameter.ident.as_str(), parameter.id))
                    .collect::<BTreeMap<_, _>>();
                let mut supplied = arguments.iter().peekable();
                let mut alias_bindings = bindings.clone();
                for parameter in &alias.generics.params {
                    match parameter {
                        GenericParam::Lifetime(parameter) => {
                            let argument = match supplied.peek() {
                                Some(ResolvedGenericArgument::Lifetime(_)) => {
                                    let Some(ResolvedGenericArgument::Lifetime(argument)) =
                                        supplied.next()
                                    else {
                                        unreachable!("peeked a lifetime argument")
                                    };
                                    bindings
                                        .lifetimes
                                        .get(&argument.ident.to_string())
                                        .cloned()
                                        .unwrap_or_else(|| argument.clone())
                                }
                                Some(_) | None => syn::parse_quote!('input),
                            };
                            alias_bindings
                                .lifetimes
                                .insert(parameter.lifetime.ident.to_string(), argument);
                        }
                        GenericParam::Type(parameter) => {
                            let id = parameter_ids
                                .get(parameter.ident.to_string().as_str())
                                .ok_or_else(|| {
                                    format!(
                                        "type alias `{}` lost generic parameter identity",
                                        alias.ident
                                    )
                                })?;
                            let argument = match supplied.peek() {
                                Some(ResolvedGenericArgument::Type(_)) => {
                                    let Some(ResolvedGenericArgument::Type(argument)) =
                                        supplied.next()
                                    else {
                                        unreachable!("peeked a type argument")
                                    };
                                    argument.clone()
                                }
                                Some(_) => {
                                    return Err(format!(
                                        "type alias `{}` has mismatched generic arguments",
                                        alias.ident
                                    ));
                                }
                                None => alias
                                    .defaults
                                    .iter()
                                    .find(|default| default.parameter == *id)
                                    .map(|default| default.ty.clone())
                                    .ok_or_else(|| {
                                        format!(
                                            "type alias `{}` leaves `{}` open",
                                            alias.ident, parameter.ident
                                        )
                                    })?,
                            };
                            alias_bindings.types.insert(*id, argument);
                        }
                        GenericParam::Const(_) => {
                            return Err(format!(
                                "type alias `{}` uses an unsupported const generic",
                                alias.ident
                            ));
                        }
                    }
                }
                if supplied.next().is_some() {
                    return Err(format!(
                        "type alias `{}` has excess generic arguments",
                        alias.ident
                    ));
                }
                let target = alias.target.as_ref().ok_or_else(|| {
                    format!("type alias `{}` has no resolved target", alias.ident)
                })?;
                let projected =
                    authored_projection_with(target, resolved, &alias_bindings, active_aliases);
                active_aliases.remove(&alias_id.index());
                projected
            }
            SemanticTarget::Node(node_id) => Ok(AuthoredProjection {
                terminal: authored_node_type(
                    *node_id,
                    *use_site,
                    arguments,
                    resolved,
                    bindings,
                    active_aliases,
                )?,
                terminal_node: Some(*node_id),
                wrappers: Vec::new(),
            }),
            SemanticTarget::ContentToken(_) => Ok(AuthoredProjection {
                terminal: type_with_generated_input_lifetime(&ty.syntax)?,
                terminal_node: None,
                wrappers: Vec::new(),
            }),
        },
        ResolvedTypeKind::FixedTokenPresence
        | ResolvedTypeKind::Reference(_)
        | ResolvedTypeKind::Tuple(_)
        | ResolvedTypeKind::Unsupported => {
            Err("an authored semantic field has no supported declaration projection".to_owned())
        }
    }
}

/// Renders a resolved type while leaving node-owned parameters open.
pub(super) fn authored_open_type(
    ty: &ResolvedType,
    resolved: &ResolvedSourceGrammar,
    bindings: &AuthoredBindings,
    active_aliases: &mut BTreeSet<usize>,
) -> Result<Type, String> {
    let projection = authored_projection_with(ty, resolved, bindings, active_aliases)?;
    let facade = emitted_ident(&resolved.build_inputs.facade.ident)?;
    Ok(render_wrapped_type(
        &facade,
        &projection.terminal,
        &projection.wrappers,
    ))
}

/// Renders the generated node type under the supplied application bindings.
pub(super) fn authored_node_type(
    node_id: NodeId,
    use_site: crate::resolution::SemanticUseId,
    arguments: &[ResolvedGenericArgument],
    resolved: &ResolvedSourceGrammar,
    bindings: &AuthoredBindings,
    active_aliases: &mut BTreeSet<usize>,
) -> Result<Type, String> {
    if resolved.uses[use_site.index()].authored == "Self" {
        return Ok(syn::parse_quote!(Self));
    }
    let node = &resolved.nodes[node_id.index()];
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in &node.key.segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    let mut rendered = syn::punctuated::Punctuated::new();
    for argument in arguments {
        match argument {
            ResolvedGenericArgument::Lifetime(lifetime) => {
                let lifetime = bindings
                    .lifetimes
                    .get(&lifetime.ident.to_string())
                    .cloned()
                    .unwrap_or_else(|| syn::parse_quote!('input));
                rendered.push(GenericArgument::Lifetime(lifetime));
            }
            ResolvedGenericArgument::Type(ty) => {
                rendered.push(GenericArgument::Type(authored_open_type(
                    ty,
                    resolved,
                    bindings,
                    active_aliases,
                )?));
            }
            ResolvedGenericArgument::Unsupported(_) => {
                return Err(format!(
                    "Node `{}` has an unsupported authored generic argument",
                    node.ident
                ));
            }
        }
    }
    if node.generics.lifetimes().next().is_some()
        && !rendered
            .iter()
            .any(|argument| matches!(argument, GenericArgument::Lifetime(_)))
    {
        rendered.insert(0, GenericArgument::Lifetime(syn::parse_quote!('input)));
    }
    if !rendered.is_empty() {
        let segment = path
            .segments
            .last_mut()
            .ok_or_else(|| "authored Node path is empty".to_owned())?;
        segment.arguments = PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: rendered,
            gt_token: Default::default(),
        });
    }
    Ok(Type::Path(syn::TypePath { qself: None, path }))
}

/// Supplies the declaration convention's inferred input lifetime to a generated token type.
fn type_with_generated_input_lifetime(ty: &Type) -> Result<Type, String> {
    let Type::Path(type_path) = ty else {
        return Err("a generated content-token reference is not a type path".to_owned());
    };
    let mut type_path = type_path.clone();
    let segment = type_path
        .path
        .segments
        .last_mut()
        .ok_or_else(|| "a generated content-token path is empty".to_owned())?;
    if matches!(segment.arguments, PathArguments::None) {
        let arguments: syn::AngleBracketedGenericArguments = syn::parse_quote!(<'input>);
        segment.arguments = PathArguments::AngleBracketed(arguments);
    }
    Ok(Type::Path(type_path))
}

/// Visits declaration elements in authored order with their construction indices.
pub(super) fn for_each_element(body: &NodeBodyPlan, visit: &mut impl FnMut(&ElementParsePlan)) {
    match body {
        NodeBodyPlan::Struct { elements, .. } => elements.iter().for_each(visit),
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .flat_map(|variant| &variant.elements)
            .for_each(visit),
    }
}

/// Assigns collision-free accessors to repeated fixed-token spellings.
pub(super) fn disambiguate_fixed_accessors(body: &mut NodeBodyPlan) -> Result<(), String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for_each_element(body, &mut |element| {
        let name = match &element.kind {
            ElementParseKind::Fixed { accessor, .. } => Some(accessor.unraw().to_string()),
            ElementParseKind::OptionalFixedTokenGroup { accessor, .. } => {
                Some(accessor.unraw().to_string())
            }
            ElementParseKind::Semantic { field, .. } => field
                .accessor
                .as_ref()
                .map(|accessor| accessor.unraw().to_string()),
        };
        if let Some(name) = name {
            *counts.entry(name).or_default() += 1;
        }
    });
    let mut occupied = counts
        .iter()
        .filter_map(|(name, count)| (*count == 1).then_some(name.clone()))
        .collect::<BTreeSet<_>>();

    fn qualify(
        elements: &mut [ElementParsePlan],
        variant: Option<&Ident>,
        counts: &BTreeMap<String, usize>,
        occupied: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        for (index, element) in elements.iter_mut().enumerate() {
            let accessor = match &mut element.kind {
                ElementParseKind::Fixed { accessor, .. }
                | ElementParseKind::OptionalFixedTokenGroup { accessor, .. } => accessor,
                ElementParseKind::Semantic { .. } => {
                    continue;
                }
            };
            let original = accessor.unraw().to_string();
            if counts.get(&original).copied().unwrap_or(0) <= 1 {
                continue;
            }
            let prefix = variant.map_or_else(
                || "fixed".to_owned(),
                |variant| snake_case(&variant.unraw().to_string()),
            );
            let mut candidate = format!("{prefix}_{original}");
            if occupied.contains(&candidate) {
                candidate = format!("{prefix}_element_{index}_{original}");
            }
            let mut suffix = 2;
            while occupied.contains(&candidate) {
                candidate = format!("{prefix}_element_{index}_{suffix}_{original}");
                suffix += 1;
            }
            *accessor = emitted_ident(&candidate)?;
            occupied.insert(candidate);
        }
        Ok(())
    }

    match body {
        NodeBodyPlan::Struct { elements, .. } => qualify(elements, None, &counts, &mut occupied)?,
        NodeBodyPlan::Enum { variants, .. } => {
            for variant in variants {
                qualify(
                    &mut variant.elements,
                    Some(&variant.ident),
                    &counts,
                    &mut occupied,
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(feature = "diagrams")]
/// Returns the display name of one materialized diagram terminal.
pub(super) fn diagram_terminal_name(
    ty: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    match ty {
        ClosedType::Node(node) => lowered
            .applications
            .get(node.application.index())
            .map(|application| application.display_name.clone())
            .ok_or_else(|| "diagram terminal references an unknown Node application".to_owned()),
        ClosedType::ContentToken(token) => lowered
            .source_modeled
            .resolved()
            .content_tokens
            .iter()
            .find(|content| content.id == *token)
            .map(|content| content.ident.clone())
            .ok_or_else(|| "diagram terminal references an unknown content token".to_owned()),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => diagram_terminal_name(inner, lowered),
    }
}

#[cfg(feature = "diagrams")]
/// Resolves a repetition separator to its canonical fixed spelling.
pub(super) fn diagram_separator_spelling(
    separator: Option<&crate::source_model::Separator>,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<Option<String>, String> {
    let Some(separator) = separator else {
        return Ok(None);
    };
    let source_model = lowered.source_modeled.source_model();
    let token_use = source_model
        .token_uses
        .get(separator.token_use.index())
        .ok_or_else(|| "diagram separator references an unknown token use".to_owned())?;
    let definition = source_model
        .token_definitions
        .get(token_use.definition.index())
        .ok_or_else(|| "diagram separator references an unknown token definition".to_owned())?;
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("diagram separator does not reference fixed-token syntax".to_owned());
    };
    Ok(Some(fixed.spelling.clone()))
}

/// Derives the stable metadata name for a fixed element occurrence.
pub(super) fn fixed_element_name(
    node: &ResolvedNode,
    token_use: &crate::source_model::TokenUse,
    definition: &crate::source_model::TokenDefinition,
    fixed: &crate::source_model::FixedTokenDefinition,
) -> String {
    let field = fixed_element_field(node, token_use);
    let base = if definition.name.starts_with('"') {
        snake_case(&fixed.spelling)
    } else {
        snake_case(&definition.name)
    };
    field
        .and_then(|field| field.syntax.ident.as_ref())
        .map_or(base.clone(), |field| {
            format!("{}_{}", snake_case(&field.unraw().to_string()), base)
        })
}

/// Determines the construction slot occupied by a fixed element.
pub(super) fn fixed_element_field<'node>(
    node: &'node ResolvedNode,
    token_use: &crate::source_model::TokenUse,
) -> Option<&'node crate::resolution::ResolvedNodeField> {
    node.fields.iter().find(|field| {
        field.metas.iter().any(|meta| {
            let range = span_range(meta.span());
            range.start <= token_use.site.range.start && range.end >= token_use.site.range.end
        })
    })
}

/// Converts authored type constructors into parser storage wrappers.
pub(super) fn field_wrappers(ty: &ClosedType) -> Result<Vec<FieldWrapper>, String> {
    fn walk(ty: &ClosedType, output: &mut Vec<FieldWrapper>) {
        match ty {
            ClosedType::Node(_) | ClosedType::ContentToken(_) => {}
            ClosedType::Boxed(inner) => {
                output.push(FieldWrapper::Boxed);
                walk(inner, output)
            }
            ClosedType::Optional(inner) => {
                output.push(FieldWrapper::Optional);
                walk(inner, output)
            }
            ClosedType::ZeroOrMore(inner) => {
                output.push(FieldWrapper::ZeroOrMore);
                walk(inner, output)
            }
            ClosedType::OneOrMore(inner) => {
                output.push(FieldWrapper::OneOrMore);
                walk(inner, output)
            }
        }
    }
    let mut wrappers = Vec::new();
    walk(ty, &mut wrappers);
    Ok(wrappers)
}

/// Projects a resolved terminal into Explorer's grammar-erased schema.
pub(super) fn explorer_terminal(ty: &ClosedType) -> Result<ExplorerTerminalPlan, String> {
    match ty {
        ClosedType::Node(node) => Ok(ExplorerTerminalPlan::Node(node.application)),
        ClosedType::ContentToken(token) => Ok(ExplorerTerminalPlan::Content(*token)),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => explorer_terminal(inner),
    }
}

/// Rejects wrapper combinations unsupported by parser container semantics.
pub(super) fn terminal_type(
    closed: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    match closed {
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => terminal_type(inner, lowered, input_lifetime),
        ClosedType::Node(node_use) => materialized_application_type(
            &lowered.applications[node_use.application.index()],
            &node_use.lifetimes,
            lowered,
            input_lifetime,
        ),
        ClosedType::ContentToken(token) => {
            let mut path: syn::Path = syn::parse_quote!(crate);
            for segment in &lowered
                .source_modeled
                .resolved()
                .build_inputs
                .module_anchor
                .segments
            {
                path.segments
                    .push(syn::PathSegment::from(emitted_ident(segment)?));
            }
            path.segments.push(syn::PathSegment {
                ident: format_ident!("__recursa_content_tokens"),
                arguments: PathArguments::None,
            });
            path.segments.push(syn::PathSegment {
                ident: format_ident!("__RecursaContentToken{}", token.index()),
                arguments: PathArguments::AngleBracketed(syn::parse_quote!(<#input_lifetime>)),
            });
            Ok(Type::Path(syn::TypePath { qself: None, path }))
        }
    }
}

/// Finds the fully bound generated application for a terminal, when one exists.
pub(super) fn closed_terminal_application(
    closed: &ClosedType,
) -> Option<MaterializedApplicationId> {
    match closed {
        ClosedType::Node(node) => Some(node.application),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => closed_terminal_application(inner),
        ClosedType::ContentToken(_) => None,
    }
}

/// Renders the Rust type of one closed Node application.
pub(super) fn materialized_application_type(
    application: &MaterializedNodeApplication,
    lifetimes: &[syn::Lifetime],
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    let resolved = lowered.source_modeled.resolved();
    let node = &resolved.nodes[application.node.index()];
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in &node.key.segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    let segment = path
        .segments
        .last_mut()
        .ok_or_else(|| "materialized application path is empty".to_owned())?;
    let lifetime_parameter_count = node.generics.lifetimes().count();
    if lifetimes.len() != lifetime_parameter_count {
        return Err(format!(
            "materialized application `{}` has {} lifetime arguments for {lifetime_parameter_count} parameters",
            application.display_name,
            lifetimes.len(),
        ));
    }
    let mut type_arguments = application.type_arguments.iter();
    let mut arguments = syn::punctuated::Punctuated::new();
    for parameter in &node.generics.params {
        match parameter {
            GenericParam::Lifetime(_) => {
                arguments.push(GenericArgument::Lifetime(input_lifetime.clone()));
            }
            GenericParam::Type(_) => {
                let argument = type_arguments.next().ok_or_else(|| {
                    format!(
                        "materialized application `{}` lacks a closed type argument",
                        application.display_name
                    )
                })?;
                arguments.push(GenericArgument::Type(closed_type(
                    argument,
                    lowered,
                    input_lifetime,
                )?));
            }
            GenericParam::Const(_) => {
                return Err("const-generic materialized applications cannot be emitted".to_owned());
            }
        }
    }
    if type_arguments.next().is_some() {
        return Err(format!(
            "materialized application `{}` has excess generic arguments",
            application.display_name
        ));
    }
    if !arguments.is_empty() {
        segment.arguments = PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: arguments,
            gt_token: Default::default(),
        });
    }
    Ok(Type::Path(syn::TypePath { qself: None, path }))
}

/// Renders a resolved type after requiring every generic to be bound.
pub(super) fn closed_type(
    closed: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    let facade = emitted_ident(&lowered.source_modeled.resolved().build_inputs.facade.ident)?;
    Ok(match closed {
        ClosedType::Node(_) | ClosedType::ContentToken(_) => {
            terminal_type(closed, lowered, input_lifetime)?
        }
        ClosedType::Boxed(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            if lowered.source_modeled.resolved().build_inputs.arena_ast {
                let facade =
                    emitted_ident(&lowered.source_modeled.resolved().build_inputs.facade.ident)?;
                syn::parse_quote!(#facade::ArenaBox<#input_lifetime, #inner>)
            } else {
                syn::parse_quote!(Box<#inner>)
            }
        }
        ClosedType::Optional(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(Option<#inner>)
        }
        ClosedType::ZeroOrMore(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            if lowered.source_modeled.resolved().build_inputs.arena_ast {
                let facade =
                    emitted_ident(&lowered.source_modeled.resolved().build_inputs.facade.ident)?;
                syn::parse_quote!(#facade::ArenaVec<#input_lifetime, #inner>)
            } else {
                syn::parse_quote!(Vec<#inner>)
            }
        }
        ClosedType::OneOrMore(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            if lowered.source_modeled.resolved().build_inputs.arena_ast {
                syn::parse_quote!(#facade::ArenaVec1<#input_lifetime, #inner>)
            } else {
                syn::parse_quote!(#facade::Vec1<#inner>)
            }
        }
    })
}

/// Renders the construction expression of one variant or struct body.
pub(super) fn render_construction(
    variant: Option<&Ident>,
    style: ResolvedFieldStyle,
    values: &[(FieldPlan, Ident)],
) -> proc_macro2::TokenStream {
    let target = variant.map_or_else(|| quote!(Self), |variant| quote!(Self::#variant));
    match style {
        ResolvedFieldStyle::Named => {
            let fields = values.iter().map(|(field, value)| {
                let ident = field
                    .ident
                    .as_ref()
                    .expect("named source shape has named fields");
                quote!(#ident: #value)
            });
            quote!(#target { #(#fields),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let mut ordered = values.iter().collect::<Vec<_>>();
            ordered.sort_by_key(|(field, _)| field.index);
            let fields = ordered.into_iter().map(|(_, value)| value);
            quote!(#target(#(#fields),*))
        }
        ResolvedFieldStyle::Unit => quote!(#target),
    }
}

/// Applies authored storage wrappers to a parsed value expression.
pub(super) fn render_wrapped_type(
    facade: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
) -> Type {
    wrappers
        .iter()
        .rev()
        .fold(terminal.clone(), |ty, wrapper| match wrapper {
            FieldWrapper::Boxed => syn::parse_quote!(Box<#ty>),
            FieldWrapper::Optional => syn::parse_quote!(Option<#ty>),
            FieldWrapper::ZeroOrMore => syn::parse_quote!(Vec<#ty>),
            FieldWrapper::OneOrMore => syn::parse_quote!(#facade::Vec1<#ty>),
        })
}

/// Applies arena-backed authored storage wrappers to a parsed value type.
pub(super) fn render_arena_wrapped_type(
    facade: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
) -> Type {
    wrappers
        .iter()
        .rev()
        .fold(terminal.clone(), |ty, wrapper| match wrapper {
            FieldWrapper::Boxed => syn::parse_quote!(#facade::ArenaBox<'_, #ty>),
            FieldWrapper::Optional => syn::parse_quote!(Option<#ty>),
            FieldWrapper::ZeroOrMore => syn::parse_quote!(#facade::ArenaVec<'_, #ty>),
            FieldWrapper::OneOrMore => syn::parse_quote!(#facade::ArenaVec1<'_, #ty>),
        })
}

/// Emits a reference to a terminal value through its wrapper shape.
pub(super) fn render_terminal_reference(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
    value_is_reference: bool,
) -> proc_macro2::TokenStream {
    let initial = if value_is_reference {
        value
    } else {
        quote!(&#value)
    };
    wrappers
        .iter()
        .fold(initial, |reference, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!((#reference).as_ref()),
            FieldWrapper::Optional => {
                unreachable!("non-optional accessors contain no optional wrapper")
            }
            FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {
                unreachable!("container access stops at the repetition wrapper")
            }
        })
}

/// Emits an optional borrowed view through wrappers that may be absent.
pub(super) fn render_optional_view_reference(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
    value_is_reference: bool,
) -> proc_macro2::TokenStream {
    let mut reference = if value_is_reference {
        value
    } else {
        quote!(&#value)
    };
    let mut optional = false;
    for wrapper in wrappers {
        match (optional, wrapper) {
            (false, FieldWrapper::Boxed) => reference = quote!((#reference).as_ref()),
            (false, FieldWrapper::Optional) => {
                reference = quote!((#reference).as_ref());
                optional = true;
            }
            (true, FieldWrapper::Boxed) => {
                reference = quote!((#reference).map(|value| value.as_ref()));
            }
            (true, FieldWrapper::Optional) => {
                reference = quote!((#reference).and_then(|value| value.as_ref()));
            }
            (_, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore) => {
                unreachable!("container access stops at the repetition wrapper")
            }
        }
    }
    debug_assert!(optional);
    reference
}

/// Emits field access for a named or positional construction slot.
pub(super) fn field_access(
    base: proc_macro2::TokenStream,
    ident: Option<&Ident>,
    index: usize,
) -> proc_macro2::TokenStream {
    ident.map_or_else(
        || {
            let index = syn::Index::from(index);
            quote!(#base.#index)
        },
        |ident| quote!(#base.#ident),
    )
}

/// Emits the binding pattern for a named or positional field.
pub(super) fn field_binding_pattern(
    node: &Ident,
    variant: &Ident,
    style: ResolvedFieldStyle,
    field: Option<&Ident>,
    index: usize,
    binding: &Ident,
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => {
            let field = field.expect("named variant field has an identifier");
            quote!(#node::#variant { #field: #binding, .. })
        }
        ResolvedFieldStyle::Unnamed => {
            let before = (0..index).map(|_| quote!(_));
            quote!(#node::#variant(#(#before,)* #binding, ..))
        }
        ResolvedFieldStyle::Unit => quote!(#node::#variant),
    }
}

/// Emits the destructuring pattern for one enum variant.
pub(super) fn variant_pattern(
    node: &Ident,
    variant: &Ident,
    style: ResolvedFieldStyle,
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => quote!(#node::#variant { .. }),
        ResolvedFieldStyle::Unnamed => quote!(#node::#variant(..)),
        ResolvedFieldStyle::Unit => quote!(#node::#variant),
    }
}

/// Derives the collision-free identifier for generated metadata storage.
pub(super) fn metadata_static(node: usize, variant: Option<usize>) -> Ident {
    variant.map_or_else(
        || format_ident!("__RECURSA_ELEMENTS_{node}"),
        |variant| format_ident!("__RECURSA_ELEMENTS_{node}_{variant}"),
    )
}
