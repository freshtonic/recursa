#![allow(
    dead_code,
    reason = "private source analysis materialization seam is consumed by final source lowering"
)]
#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Materialization of closed grammar type applications and container shapes.
//!
//! Resolved Node declarations become concrete applications only after generic
//! arguments and the closed container catalog are known. Memoized application
//! identities permit legal recursive Nodes while explicit alias tracking
//! rejects open, cyclic, or unsupported authored types.

#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;
use syn::ext::IdentExt as _;

use syn::GenericParam;

use crate::lexical::{
    LexicalDiagnostic, LexicalPlan, TokenDefinitionKindCorrelation, kind_label, plan_lexical,
};
use crate::lowering::{ElementId as GrammarElementId, NodeId, TokenUseId};
use crate::resolution::{
    CatalogType, ContentTokenId, GenericParameterId, ResolutionTraceId, ResolvedFieldId,
    ResolvedGenericArgument, ResolvedNode, ResolvedSourceGrammar, ResolvedType, ResolvedTypeKind,
    ResolvedVariantId, SemanticOwner, SemanticTarget, SemanticUseId, TypeAliasId,
};
#[cfg(test)]
use crate::source_model::PrettyBoundaryProofCounts;
use crate::source_model::{
    AttachedFixedSyntax, ConfiguredTraitRecipe, DeclaredRepetition, GrammarElement,
    MaterializedPrettyBoundary, NodeBody, PrattExtensionFilter, PrettyBoundaryProver,
    PrettyPatternCatalogId, SemanticFieldProjection, Separator, SourceModel, SourceModeledGrammar,
    SourcePrattPosition, SourcePrattVariant, TokenDefinitionKind,
    materialized_pretty_boundary_changes_lexing, materialized_pretty_lexical_patterns,
};
use crate::{
    AtomDescription, AtomExpression, ContainerShape, DeclId, Declaration, GrammarDescription,
    KindDescription, Location, ParseSite, PrattPosition, RepeatSeparator, SiteId, SourceId,
    TypeDescription, TypeId, TypeShape,
};

/// Defines a dense materialization identity with checked allocation and indexing.
macro_rules! materialization_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(u32);

        impl $name {
            fn next(length: usize) -> Self {
                Self(length.try_into().expect("materialized identity overflow"))
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

materialization_id!(MaterializedApplicationId);

#[derive(Clone, Debug, Eq, PartialEq)]
/// Outcome of expanding every reachable closed grammar application.
pub(crate) struct MaterializationReport {
    /// Independent lexical-policy failures retained alongside materialization failures.
    pub(crate) lexical_diagnostics: Vec<LexicalDiagnostic>,
    /// Application, generic-argument, and container-shape failures in stable order.
    pub(crate) diagnostics: Vec<MaterializationDiagnostic>,
    /// Complete lowered grammar, withheld when either diagnostic collection blocks it.
    pub(crate) lowered: Option<LoweredGrammar>,
    /// Counts Pretty-boundary proof work for focused cache invariant tests.
    #[cfg(test)]
    pub(crate) pretty_boundary_proof_counts: PrettyBoundaryProofCounts,
    /// Counts closed Pretty-summary fixed-point work for focused cache invariant tests.
    #[cfg(test)]
    pub(crate) closed_pretty_summary_counts: ClosedPrettySummaryCounts,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Counts deterministic closed Pretty-summary work within one materialization run.
pub(crate) struct ClosedPrettySummaryCounts {
    /// Counts whole-application fixed-point rounds, including the stable round.
    pub(crate) fixed_point_rounds: usize,
    /// Counts application bodies evaluated while finding the fixed point.
    pub(crate) body_evaluations: usize,
    /// Counts Node-summary queries served from the run-local cache.
    pub(crate) cache_queries: usize,
    /// Counts fixed-point summaries that differ from the former cycle-cut oracle in tests.
    pub(crate) legacy_mismatches: usize,
    /// Records the largest distinct boundary set retained by one summary side.
    pub(crate) max_retained_boundaries: usize,
    /// Records the largest number of insertions attempted while building one summary side.
    pub(crate) max_attempted_boundary_insertions: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Closed grammar aggregate consumed by semantic analysis and generation.
pub(crate) struct LoweredGrammar {
    /// Resolved authored structure from which applications were expanded.
    pub(crate) source_modeled: SourceModeledGrammar,
    /// Validated token kinds, matchers, and parser-target admissions.
    pub(crate) lexical_plan: LexicalPlan,
    /// Reachable Node applications in deterministic identity order.
    pub(crate) applications: Vec<MaterializedNodeApplication>,
    /// Source-independent normalized grammar for the closed applications.
    pub(crate) grammar: GrammarDescription,
    /// Correlations from resolved source identities to normalized identities.
    pub(crate) lowering_map: LoweringMap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Explicit correlations created while closing and normalizing grammar applications.
pub(crate) struct LoweringMap {
    /// Physical source identities correlated with normalized location sources.
    pub(crate) sources: Vec<SourceCorrelation>,
    /// Source token definitions correlated with normalized token kinds.
    pub(crate) token_definitions: Vec<TokenDefinitionKindCorrelation>,
    /// Content-token targets correlated with normalized grammar types.
    pub(crate) content_targets: Vec<ContentTargetTypeCorrelation>,
    /// Materialized Node applications correlated with normalized grammar types.
    pub(crate) applications: Vec<ApplicationTypeCorrelation>,
    /// Typed document-framing identities correlated with one exact application.
    pub(crate) framing: Option<FramingApplicationCorrelation>,
    /// Authored variants correlated with normalized alternatives.
    pub(crate) variants: Vec<VariantAlternativeCorrelation>,
    /// Authored fields correlated with parse sites or implicit Pratt operands.
    pub(crate) fields: Vec<MaterializedFieldCorrelation>,
    /// Repetition attachment units correlated with their normalized sites.
    pub(crate) attachments: Vec<AttachmentUnitCorrelation>,
    /// Boolean fixed-token presence units correlated with their normalized sites.
    pub(crate) presences: Vec<FixedPresenceCorrelation>,
    /// Erased optional fixed-token groups correlated with their normalized sequences.
    pub(crate) optional_fixed_groups: Vec<OptionalFixedGroupCorrelation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Correlates typed framing declarations with materialized parser identities.
pub(crate) struct FramingApplicationCorrelation {
    /// Exact resolved Node declaration selected by source resolution.
    pub(crate) node: NodeId,
    /// Exact closed Node application used for every typed island.
    pub(crate) application: MaterializedApplicationId,
    /// Optional exact resolved variant that commits payload mode.
    pub(crate) payload_variant: Option<ResolvedVariantId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the content target type correlation while expanding resolved grammar type applications.
pub(crate) struct ContentTargetTypeCorrelation {
    /// Supplies the semantic needed to materialize a concrete grammar application.
    pub(crate) semantic: ContentTokenId,
    /// Supplies the lowered needed to materialize a concrete grammar application.
    pub(crate) lowered: TypeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the source correlation while expanding resolved grammar type applications.
pub(crate) struct SourceCorrelation {
    /// Supplies the resolved needed to materialize a concrete grammar application.
    pub(crate) resolved: SourceId,
    /// Supplies the lowered needed to materialize a concrete grammar application.
    pub(crate) lowered: SourceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the application type correlation while expanding resolved grammar type applications.
pub(crate) struct ApplicationTypeCorrelation {
    /// Supplies the application needed to materialize a concrete grammar application.
    pub(crate) application: MaterializedApplicationId,
    /// Supplies the node needed to materialize a concrete grammar application.
    pub(crate) node: NodeId,
    /// Supplies the lowered needed to materialize a concrete grammar application.
    pub(crate) lowered: TypeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the materialized field correlation while expanding resolved grammar type applications.
pub(crate) struct MaterializedFieldCorrelation {
    /// Supplies the application needed to materialize a concrete grammar application.
    pub(crate) application: MaterializedApplicationId,
    /// Supplies the field needed to materialize a concrete grammar application.
    pub(crate) field: ResolvedFieldId,
    /// Supplies the lowered needed to materialize a concrete grammar application.
    pub(crate) lowered: MaterializedFieldLowering,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the materialized field lowering alternatives supported during materialization.
pub(crate) enum MaterializedFieldLowering {
    /// Correlates an ordinary semantic field with its normalized parse site.
    ParseSite(SiteId),
    /// Selects the pratt lhs materialization alternative.
    PrattLhs {
        #[doc = "Normalized Pratt type that owns the implicit left-hand side."]
        owner: TypeId,
        #[doc = "Normalized led variant containing the implicit left-hand side."]
        variant: DeclId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the attachment unit correlation while expanding resolved grammar type applications.
pub(crate) struct AttachmentUnitCorrelation {
    /// Supplies the application needed to materialize a concrete grammar application.
    pub(crate) application: MaterializedApplicationId,
    /// Supplies the field needed to materialize a concrete grammar application.
    pub(crate) field: ResolvedFieldId,
    /// Supplies the outer needed to materialize a concrete grammar application.
    pub(crate) outer: SiteId,
    /// Supplies the unit needed to materialize a concrete grammar application.
    pub(crate) unit: TypeId,
    /// Supplies the sites needed to materialize a concrete grammar application.
    pub(crate) sites: Vec<SiteId>,
    /// Supplies the semantic index needed to materialize a concrete grammar application.
    pub(crate) semantic_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Correlates one authored boolean presence field with its internal normalized sequences.
pub(crate) struct FixedPresenceCorrelation {
    /// Supplies the application containing the authored field.
    pub(crate) application: MaterializedApplicationId,
    /// Supplies the authored boolean field.
    pub(crate) field: ResolvedFieldId,
    /// Supplies the outer site in the containing Node sequence.
    pub(crate) outer: SiteId,
    /// Whether the outer site itself is the optional presence selector.
    pub(crate) outer_is_optional: bool,
    /// Supplies the normalized envelope sequence type.
    pub(crate) envelope: TypeId,
    /// Supplies required-before, optional-presence, and required-after sites.
    pub(crate) envelope_sites: Vec<SiteId>,
    /// Supplies the index of the optional presence site in the envelope.
    pub(crate) presence_index: usize,
    /// Supplies the normalized fixed-only sequence type selected atomically.
    pub(crate) sequence: TypeId,
    /// Supplies the fixed-token sites in the atomic sequence.
    pub(crate) sequence_sites: Vec<SiteId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Correlates one erased optional fixed-token group with its normalized sequence.
pub(crate) struct OptionalFixedGroupCorrelation {
    /// Supplies the application containing the authored group.
    pub(crate) application: MaterializedApplicationId,
    /// Supplies the source element identity.
    pub(crate) element: GrammarElementId,
    /// Supplies the optional outer site in the containing sequence.
    pub(crate) outer: SiteId,
    /// Supplies the normalized fixed-only sequence type.
    pub(crate) sequence: TypeId,
    /// Supplies the fixed-token sites in the atomic sequence.
    pub(crate) sequence_sites: Vec<SiteId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the variant alternative correlation while expanding resolved grammar type applications.
pub(crate) struct VariantAlternativeCorrelation {
    /// Supplies the application needed to materialize a concrete grammar application.
    pub(crate) application: MaterializedApplicationId,
    /// Supplies the variant needed to materialize a concrete grammar application.
    pub(crate) variant: ResolvedVariantId,
    /// Supplies the lowered needed to materialize a concrete grammar application.
    pub(crate) lowered: DeclId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized node application while expanding resolved grammar type applications.
pub(crate) struct MaterializedNodeApplication {
    /// Supplies the id needed to materialize a concrete grammar application.
    pub(crate) id: MaterializedApplicationId,
    /// Supplies the node needed to materialize a concrete grammar application.
    pub(crate) node: NodeId,
    /// Supplies the display name needed to materialize a concrete grammar application.
    pub(crate) display_name: String,
    /// Supplies the type arguments needed to materialize a concrete grammar application.
    pub(crate) type_arguments: Vec<ClosedType>,
    /// Supplies the pretty needed to materialize a concrete grammar application.
    pub(crate) pretty: crate::source_model::PrettyHints,
    /// Supplies the body needed to materialize a concrete grammar application.
    pub(crate) body: MaterializedNodeBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the materialized node body alternatives supported during materialization.
pub(crate) enum MaterializedNodeBody {
    /// Selects the struct materialization alternative.
    Struct(Vec<MaterializedElement>),
    /// Selects the presence materialization alternative.
    Presence(Vec<MaterializedElement>),
    /// Selects the enum materialization alternative.
    Enum(Vec<MaterializedVariant>),
    /// Selects the pratt materialization alternative.
    Pratt(Vec<MaterializedPrattVariant>),
}

/// Borrows one source-ordered materialized element group with its Pratt operand threshold.
struct MaterializedElementGroup<'a> {
    /// Elements belonging to one struct, enum variant, or Pratt branch.
    elements: &'a [MaterializedElement],
    /// Minimum binding power applied to recursive operands in this group.
    recursive_min_bp: u8,
}

impl MaterializedNodeBody {
    /// Returns every source-ordered element group through one shared body traversal.
    fn element_groups(&self) -> Vec<MaterializedElementGroup<'_>> {
        match self {
            Self::Struct(elements) | Self::Presence(elements) => vec![MaterializedElementGroup {
                elements,
                recursive_min_bp: 0,
            }],
            Self::Enum(variants) => variants
                .iter()
                .map(|variant| MaterializedElementGroup {
                    elements: &variant.elements,
                    recursive_min_bp: 0,
                })
                .collect(),
            Self::Pratt(variants) => variants
                .iter()
                .map(|variant| match variant {
                    MaterializedPrattVariant::Nud(nud) => MaterializedElementGroup {
                        elements: &nud.body,
                        recursive_min_bp: match nud.position {
                            MaterializedPrattNudPosition::Atom => 0,
                            MaterializedPrattNudPosition::Prefix { bp } => bp,
                        },
                    },
                    MaterializedPrattVariant::Led(led) => MaterializedElementGroup {
                        elements: &led.tail,
                        recursive_min_bp: match led.position {
                            MaterializedPrattLedPosition::Infix { rbp, .. } => rbp,
                            MaterializedPrattLedPosition::Postfix { bp } => bp,
                        },
                    },
                })
                .collect(),
        }
    }

    /// Derives the parsed elements while materializing grammar applications and container shapes.
    fn parsed_elements(&self) -> impl Iterator<Item = &MaterializedElement> {
        self.element_groups()
            .into_iter()
            .flat_map(|group| group.elements.iter())
    }

    /// Derives the fields while materializing grammar applications and container shapes.
    fn fields(&self) -> impl Iterator<Item = MaterializedFieldView<'_>> {
        let mut fields = Vec::new();
        match self {
            Self::Struct(elements) | Self::Presence(elements) => {
                fields.extend(elements.iter().filter_map(MaterializedFieldView::parsed));
            }
            Self::Enum(variants) => {
                fields.extend(variants.iter().flat_map(|variant| {
                    variant
                        .elements
                        .iter()
                        .filter_map(MaterializedFieldView::parsed)
                }));
            }
            Self::Pratt(variants) => {
                for variant in variants {
                    match variant {
                        MaterializedPrattVariant::Nud(nud) => {
                            fields
                                .extend(nud.body.iter().filter_map(MaterializedFieldView::parsed));
                        }
                        MaterializedPrattVariant::Led(led) => {
                            fields.push(MaterializedFieldView::PrattLhs(&led.lhs));
                            fields
                                .extend(led.tail.iter().filter_map(MaterializedFieldView::parsed));
                        }
                    }
                }
            }
        }
        fields.into_iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized variant while expanding resolved grammar type applications.
pub(crate) struct MaterializedVariant {
    /// Supplies the resolved variant needed to materialize a concrete grammar application.
    pub(crate) resolved_variant: ResolvedVariantId,
    /// Supplies the name needed to materialize a concrete grammar application.
    pub(crate) name: String,
    /// Supplies the pretty needed to materialize a concrete grammar application.
    pub(crate) pretty: crate::source_model::PrettyHints,
    /// Supplies the elements needed to materialize a concrete grammar application.
    pub(crate) elements: Vec<MaterializedElement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the materialized pratt variant alternatives supported during materialization.
pub(crate) enum MaterializedPrattVariant {
    /// Selects the nud materialization alternative.
    Nud(MaterializedPrattNud),
    /// Selects the led materialization alternative.
    Led(MaterializedPrattLed),
}

impl MaterializedPrattVariant {
    /// Borrows the declaration metadata shared by all Pratt variant roles.
    fn common(&self) -> &MaterializedPrattCommon {
        match self {
            Self::Nud(variant) => &variant.common,
            Self::Led(variant) => &variant.common,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized pratt common while expanding resolved grammar type applications.
pub(crate) struct MaterializedPrattCommon {
    /// Supplies the resolved variant needed to materialize a concrete grammar application.
    pub(crate) resolved_variant: ResolvedVariantId,
    /// Supplies the name needed to materialize a concrete grammar application.
    pub(crate) name: String,
    /// Supplies the pretty needed to materialize a concrete grammar application.
    pub(crate) pretty: crate::source_model::PrettyHints,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized pratt nud while expanding resolved grammar type applications.
pub(crate) struct MaterializedPrattNud {
    /// Declaration metadata shared by all Pratt null-denotation roles.
    pub(crate) common: MaterializedPrattCommon,
    /// Supplies the position needed to materialize a concrete grammar application.
    pub(crate) position: MaterializedPrattNudPosition,
    /// Supplies the body needed to materialize a concrete grammar application.
    pub(crate) body: Vec<MaterializedElement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the materialized pratt nud position alternatives supported during materialization.
pub(crate) enum MaterializedPrattNudPosition {
    /// Selects the atom materialization alternative.
    Atom,
    /// Selects the prefix materialization alternative.
    Prefix {
        #[doc = "Right binding power of the recursive prefix operand."]
        bp: u8,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized pratt led while expanding resolved grammar type applications.
pub(crate) struct MaterializedPrattLed {
    /// Declaration metadata shared by all Pratt left-denotation roles.
    pub(crate) common: MaterializedPrattCommon,
    /// Supplies the position needed to materialize a concrete grammar application.
    pub(crate) position: MaterializedPrattLedPosition,
    /// Supplies the lhs needed to materialize a concrete grammar application.
    pub(crate) lhs: MaterializedPrattLhs,
    /// Supplies the tail needed to materialize a concrete grammar application.
    pub(crate) tail: Vec<MaterializedElement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the materialized pratt led position alternatives supported during materialization.
pub(crate) enum MaterializedPrattLedPosition {
    /// Selects the infix materialization alternative.
    Infix {
        #[doc = "Left binding power required to select the infix operator."]
        lbp: u8,
        #[doc = "Right binding power used for the infix operand."]
        rbp: u8,
    },
    /// Selects the postfix materialization alternative.
    Postfix {
        #[doc = "Left binding power required to select the postfix operator."]
        bp: u8,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized pratt lhs while expanding resolved grammar type applications.
pub(crate) struct MaterializedPrattLhs {
    /// Supplies the field needed to materialize a concrete grammar application.
    pub(crate) field: ResolvedFieldId,
    /// Supplies the owner needed to materialize a concrete grammar application.
    pub(crate) owner: MaterializedNodeUse,
    /// Supplies the trace needed to materialize a concrete grammar application.
    pub(crate) trace: MaterializationTrace,
    /// Supplies the pretty needed to materialize a concrete grammar application.
    pub(crate) pretty: crate::source_model::PrettyHints,
    /// Supplies the attachment before needed to materialize a concrete grammar application.
    pub(crate) attachment_before: usize,
    /// Supplies the attachment after needed to materialize a concrete grammar application.
    pub(crate) attachment_after: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the materialized element alternatives supported during materialization.
pub(crate) enum MaterializedElement {
    /// Selects the fixed token materialization alternative.
    FixedToken {
        /// Supplies the token use needed to materialize a concrete grammar application.
        token_use: TokenUseId,
    },
    /// Selects one erased atomic optional fixed-token sequence.
    OptionalFixedTokenGroup {
        /// Supplies the source element identity retained for correlation.
        element: GrammarElementId,
        /// Supplies the fixed sequence selected atomically.
        tokens: Vec<TokenUseId>,
        /// Locates the authored optional group.
        site: crate::source_model::SourceSite,
    },
    /// Selects the semantic field materialization alternative.
    SemanticField {
        /// Supplies the field needed to materialize a concrete grammar application.
        field: ResolvedFieldId,
        /// Selects direct semantic parsing or a boolean fixed-presence projection.
        projection: MaterializedSemanticFieldProjection,
        /// Supplies the attachment needed to materialize a concrete grammar application.
        attachment: Option<MaterializedFixedAttachment>,
        /// Supplies the pretty needed to materialize a concrete grammar application.
        pretty: crate::source_model::PrettyHints,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies how one materialized semantic field obtains its public value.
pub(crate) enum MaterializedSemanticFieldProjection {
    /// Parses the field's closed semantic type directly.
    Direct {
        /// Supplies the closed field type.
        ty: ClosedType,
        /// Supplies the type-resolution trace.
        trace: MaterializationTrace,
        /// Supplies repetition-separator metadata.
        separator: Option<Separator>,
        /// Restricts the Pratt extensions admitted while parsing this field.
        pratt_extensions: Option<PrattExtensionFilter>,
        /// Counts standalone fixed syntax before the semantic element.
        attachment_before: usize,
        /// Counts standalone fixed syntax after the semantic element.
        attachment_after: usize,
    },
    /// Projects one atomic optional fixed-token sequence to an authored boolean.
    FixedPresence {
        /// Supplies the fixed sequence selected atomically.
        tokens: Vec<TokenUseId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized fixed attachment while expanding resolved grammar type applications.
pub(crate) struct MaterializedFixedAttachment {
    /// Supplies the before needed to materialize a concrete grammar application.
    pub(crate) before: Vec<MaterializedAttachedFixedSyntax>,
    /// Supplies the after needed to materialize a concrete grammar application.
    pub(crate) after: Vec<MaterializedAttachedFixedSyntax>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies required and erased-optional syntax in a materialized attachment.
pub(crate) enum MaterializedAttachedFixedSyntax {
    /// Selects one required fixed token.
    Required(TokenUseId),
    /// Selects one erased atomic optional fixed sequence.
    Optional {
        /// Supplies the source-ordered fixed sequence.
        tokens: Vec<TokenUseId>,
        /// Locates the authored optional group.
        site: crate::source_model::SourceSite,
    },
}

impl MaterializedAttachedFixedSyntax {
    /// Returns the first token use in this source-ordered syntax entry.
    fn first(&self) -> TokenUseId {
        match self {
            Self::Required(token) => *token,
            Self::Optional { tokens, .. } => tokens[0],
        }
    }

    /// Returns the last token use in this source-ordered syntax entry.
    fn last(&self) -> TokenUseId {
        match self {
            Self::Required(token) => *token,
            Self::Optional { tokens, .. } => *tokens.last().expect("optional groups are non-empty"),
        }
    }
}

/// Projects one source attachment entry into its application-independent materialized form.
fn materialize_attached_fixed(syntax: &AttachedFixedSyntax) -> MaterializedAttachedFixedSyntax {
    match syntax {
        AttachedFixedSyntax::Required(token) => {
            MaterializedAttachedFixedSyntax::Required(token.token_use)
        }
        AttachedFixedSyntax::Optional { tokens, site } => {
            MaterializedAttachedFixedSyntax::Optional {
                tokens: tokens.iter().map(|token| token.token_use).collect(),
                site: site.clone(),
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// Classifies the materialized field view alternatives supported during materialization.
pub(crate) enum MaterializedFieldView<'a> {
    /// Selects the parsed materialization alternative.
    Parsed {
        /// Supplies the field needed to materialize a concrete grammar application.
        field: ResolvedFieldId,
        /// Supplies the ty needed to materialize a concrete grammar application.
        ty: &'a ClosedType,
        /// Supplies the trace needed to materialize a concrete grammar application.
        trace: &'a MaterializationTrace,
        /// Supplies the separator needed to materialize a concrete grammar application.
        separator: Option<&'a Separator>,
    },
    /// Selects the pratt lhs materialization alternative.
    PrattLhs(&'a MaterializedPrattLhs),
}

impl<'a> MaterializedFieldView<'a> {
    /// Derives the parsed while materializing grammar applications and container shapes.
    fn parsed(element: &'a MaterializedElement) -> Option<Self> {
        match element {
            MaterializedElement::SemanticField {
                field,
                projection:
                    MaterializedSemanticFieldProjection::Direct {
                        ty,
                        trace,
                        separator,
                        ..
                    },
                ..
            } => Some(Self::Parsed {
                field: *field,
                ty,
                trace,
                separator: separator.as_ref(),
            }),
            MaterializedElement::FixedToken { .. }
            | MaterializedElement::OptionalFixedTokenGroup { .. }
            | MaterializedElement::SemanticField {
                projection: MaterializedSemanticFieldProjection::FixedPresence { .. },
                ..
            } => None,
        }
    }
}

impl MaterializedNodeApplication {
    /// Derives the fields while materializing grammar applications and container shapes.
    pub(crate) fn fields(&self) -> impl Iterator<Item = MaterializedFieldView<'_>> {
        self.body.fields()
    }

    /// Derives the parsed fields while materializing grammar applications and container shapes.
    pub(crate) fn parsed_fields(&self) -> impl Iterator<Item = &MaterializedElement> {
        self.body.parsed_elements().filter(|element| {
            matches!(
                element,
                MaterializedElement::SemanticField {
                    projection: MaterializedSemanticFieldProjection::Direct { .. },
                    ..
                }
            )
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the closed type alternatives supported during materialization.
pub(crate) enum ClosedType {
    /// Selects the node materialization alternative.
    Node(MaterializedNodeUse),
    /// Selects the content token materialization alternative.
    ContentToken(ContentTokenId),
    /// Selects the boxed materialization alternative.
    Boxed(Box<Self>),
    /// Selects the optional materialization alternative.
    Optional(Box<Self>),
    /// Selects the zero or more materialization alternative.
    ZeroOrMore(Box<Self>),
    /// Selects the one or more materialization alternative.
    OneOrMore(Box<Self>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialized node use while expanding resolved grammar type applications.
pub(crate) struct MaterializedNodeUse {
    /// Supplies the application needed to materialize a concrete grammar application.
    pub(crate) application: MaterializedApplicationId,
    /// Supplies the lifetimes needed to materialize a concrete grammar application.
    pub(crate) lifetimes: Vec<syn::Lifetime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialization trace while expanding resolved grammar type applications.
pub(crate) struct MaterializationTrace {
    /// Supplies the steps needed to materialize a concrete grammar application.
    pub(crate) steps: Vec<MaterializationTraceStep>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the materialization trace step alternatives supported during materialization.
pub(crate) enum MaterializationTraceStep {
    /// Selects the use materialization alternative.
    Use(SemanticUseId),
    /// Selects the resolution materialization alternative.
    Resolution(ResolutionTraceId),
    /// Selects the alias materialization alternative.
    Alias(TypeAliasId),
    /// Selects the parameter materialization alternative.
    Parameter(GenericParameterId),
    /// Selects the self application materialization alternative.
    SelfApplication(MaterializedApplicationId),
    /// Selects the catalog materialization alternative.
    Catalog(CatalogType),
    /// Selects the application materialization alternative.
    Application(MaterializedApplicationId),
    /// Selects the content token materialization alternative.
    ContentToken(ContentTokenId),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the materialization diagnostic kind alternatives supported during materialization.
pub(crate) enum MaterializationDiagnosticKind {
    /// Selects the open application materialization alternative.
    OpenApplication,
    /// Selects the const generic materialization alternative.
    ConstGeneric,
    /// Selects the unsupported type materialization alternative.
    UnsupportedType,
    /// Selects the invalid arguments materialization alternative.
    InvalidArguments,
    /// Selects the invalid container materialization alternative.
    InvalidContainer,
    /// Selects the invalid pratt shape materialization alternative.
    InvalidPrattShape,
    /// Selects the inapplicable pretty hint materialization alternative.
    InapplicablePrettyHint,
    /// Selects the unsafe pretty boundary materialization alternative.
    UnsafePrettyBoundary,
    /// Selects the alias cycle materialization alternative.
    AliasCycle,
    /// Selects the non regular recursion materialization alternative.
    NonRegularRecursion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the materialization diagnostic while expanding resolved grammar type applications.
pub(crate) struct MaterializationDiagnostic {
    /// Supplies the kind needed to materialize a concrete grammar application.
    pub(crate) kind: MaterializationDiagnosticKind,
    /// Supplies the module needed to materialize a concrete grammar application.
    pub(crate) module: Option<crate::lowering::ModuleId>,
    /// Supplies the logical module needed to materialize a concrete grammar application.
    pub(crate) logical_module: Option<Vec<String>>,
    /// Supplies the path needed to materialize a concrete grammar application.
    pub(crate) path: PathBuf,
    /// Supplies the range needed to materialize a concrete grammar application.
    pub(crate) range: Option<Range<u32>>,
    /// Supplies the message needed to materialize a concrete grammar application.
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Carries the application key while expanding resolved grammar type applications.
struct ApplicationKey {
    /// Supplies the node needed to materialize a concrete grammar application.
    node: NodeId,
    /// Supplies the arguments needed to materialize a concrete grammar application.
    arguments: Vec<Vec<TypeIdentityAtom>>,
}

#[derive(Clone)]
/// Carries the attachment unit projection while expanding resolved grammar type applications.
struct AttachmentUnitProjection {
    /// Supplies the id needed to materialize a concrete grammar application.
    id: TypeId,
    /// Supplies the application needed to materialize a concrete grammar application.
    application: MaterializedApplicationId,
    /// Supplies the field needed to materialize a concrete grammar application.
    field: ResolvedFieldId,
    /// Supplies the ty needed to materialize a concrete grammar application.
    ty: ClosedType,
    /// Supplies the attachment needed to materialize a concrete grammar application.
    attachment: MaterializedFixedAttachment,
    /// Supplies the recursive min bp needed to materialize a concrete grammar application.
    recursive_min_bp: u8,
}

#[derive(Clone)]
/// Carries one boolean presence field into its two internal normalized sequences.
struct FixedPresenceProjection {
    /// Supplies the fixed-only sequence type identity.
    sequence: TypeId,
    /// Supplies the required envelope type identity.
    envelope: TypeId,
    /// Supplies the containing materialized application.
    application: MaterializedApplicationId,
    /// Supplies the authored boolean field.
    field: ResolvedFieldId,
    /// Supplies the fixed sequence selected atomically.
    tokens: Vec<TokenUseId>,
    /// Supplies ordinary required syntax surrounding the optional sequence.
    attachment: Option<MaterializedFixedAttachment>,
}

#[derive(Clone)]
/// Carries one erased optional fixed-token group into an internal normalized sequence.
struct OptionalFixedGroupProjection {
    /// Supplies the fixed-only sequence type identity.
    sequence: TypeId,
    /// Supplies the containing materialized application.
    application: MaterializedApplicationId,
    /// Supplies the standalone source element identity, when this group is not attached.
    element: Option<GrammarElementId>,
    /// Supplies the unique first token use used to identify attached groups.
    key: TokenUseId,
    /// Supplies the fixed sequence selected atomically.
    tokens: Vec<TokenUseId>,
    /// Locates the authored optional group.
    site: crate::source_model::SourceSite,
}

/// Dense lexical-terminal products that seed normalized grammar projection.
struct NormalizedTerminalProjection {
    /// Grammar containing every lexical terminal in stable kind order.
    grammar: GrammarDescription,
    /// Fixed token type by runtime kind.
    fixed_types: BTreeMap<crate::KindId, TypeId>,
    /// Content-target type by semantic identity.
    content_types: BTreeMap<ContentTokenId, TypeId>,
    /// Boundary after lexical terminal types.
    terminal_type_count: usize,
}

/// Reserved normalized identities for applications and private projection sequences.
struct NormalizedProjectionReservations {
    /// Boolean presence projection units in stable application order.
    presences: Vec<FixedPresenceProjection>,
    /// Erased optional fixed-token units in stable application order.
    optional_fixed: Vec<OptionalFixedGroupProjection>,
    /// Attached field units in stable application order.
    attachments: Vec<AttachmentUnitProjection>,
    /// Presence field target types and whether the outer site is optional.
    presence_types: BTreeMap<(MaterializedApplicationId, ResolvedFieldId), (TypeId, bool)>,
    /// Erased optional-group target types keyed by application and first token.
    optional_fixed_types: BTreeMap<(MaterializedApplicationId, TokenUseId), TypeId>,
    /// Attachment target types keyed by application and field.
    attachment_types: BTreeMap<(MaterializedApplicationId, ResolvedFieldId), TypeId>,
    /// Public application-to-type correlations.
    applications: Vec<ApplicationTypeCorrelation>,
}

/// Correlations produced while projecting authored application bodies.
struct AuthoredBodyProjection {
    /// Materialized field-to-site correlations.
    fields: Vec<MaterializedFieldCorrelation>,
    /// Resolved variant-to-alternative correlations.
    variants: Vec<VariantAlternativeCorrelation>,
}

/// Correlations produced while publishing private normalized projection types.
struct PrivateProjectionCorrelations {
    /// Attachment projection correlations.
    attachments: Vec<AttachmentUnitCorrelation>,
    /// Boolean fixed-presence correlations.
    presences: Vec<FixedPresenceCorrelation>,
    /// Erased optional-group correlations.
    optional_fixed: Vec<OptionalFixedGroupCorrelation>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the type identity atom alternatives supported during materialization.
enum TypeIdentityAtom {
    /// Selects the node materialization alternative.
    Node(MaterializedApplicationId),
    /// Selects the content token materialization alternative.
    ContentToken(ContentTokenId),
    /// Selects the boxed materialization alternative.
    Boxed,
    /// Selects the optional materialization alternative.
    Optional,
    /// Selects the zero or more materialization alternative.
    ZeroOrMore,
    /// Selects the one or more materialization alternative.
    OneOrMore,
}

impl ClosedType {
    /// Derives the identity while materializing grammar applications and container shapes.
    fn identity(&self) -> Vec<TypeIdentityAtom> {
        let mut identity = Vec::new();
        self.extend_identity(&mut identity);
        identity
    }

    /// Derives the extend identity while materializing grammar applications and container shapes.
    fn extend_identity(&self, identity: &mut Vec<TypeIdentityAtom>) {
        match self {
            Self::Node(node) => identity.push(TypeIdentityAtom::Node(node.application)),
            Self::ContentToken(token) => identity.push(TypeIdentityAtom::ContentToken(*token)),
            Self::Boxed(inner) => {
                identity.push(TypeIdentityAtom::Boxed);
                inner.extend_identity(identity);
            }
            Self::Optional(inner) => {
                identity.push(TypeIdentityAtom::Optional);
                inner.extend_identity(identity);
            }
            Self::ZeroOrMore(inner) => {
                identity.push(TypeIdentityAtom::ZeroOrMore);
                inner.extend_identity(identity);
            }
            Self::OneOrMore(inner) => {
                identity.push(TypeIdentityAtom::OneOrMore);
                inner.extend_identity(identity);
            }
        }
    }
}

#[derive(Clone)]
/// Carries the evaluated type while expanding resolved grammar type applications.
struct EvaluatedType {
    /// Supplies the closed needed to materialize a concrete grammar application.
    closed: ClosedType,
    /// Supplies the trace needed to materialize a concrete grammar application.
    trace: Vec<MaterializationTraceStep>,
}

#[derive(Clone, Default)]
/// Carries the evaluation environment while expanding resolved grammar type applications.
struct EvaluationEnvironment {
    /// Supplies the types needed to materialize a concrete grammar application.
    types: BTreeMap<GenericParameterId, EvaluatedType>,
    /// Supplies the lifetimes needed to materialize a concrete grammar application.
    lifetimes: BTreeMap<String, syn::Lifetime>,
}

/// Carries the materializer while expanding resolved grammar type applications.
struct Materializer {
    /// Supplies the source modeled needed to materialize a concrete grammar application.
    source_modeled: SourceModeledGrammar,
    /// Supplies the lexical plan needed to materialize a concrete grammar application.
    lexical_plan: Option<LexicalPlan>,
    /// Supplies the application by key needed to materialize a concrete grammar application.
    application_by_key: BTreeMap<ApplicationKey, MaterializedApplicationId>,
    /// Supplies the applications needed to materialize a concrete grammar application.
    applications: Vec<MaterializedNodeApplication>,
    /// Exact closed framing identities, withheld until typed references materialize.
    framing: Option<FramingApplicationCorrelation>,
    /// Supplies the diagnostics needed to materialize a concrete grammar application.
    diagnostics: Vec<MaterializationDiagnostic>,
    /// Supplies the active aliases needed to materialize a concrete grammar application.
    active_aliases: Vec<TypeAliasId>,
    /// Supplies the active applications needed to materialize a concrete grammar application.
    active_applications: Vec<ApplicationKey>,
    /// Reuses compiled regular-language products across Pretty-boundary checks in this run.
    pretty_boundary_prover: RefCell<PrettyBoundaryProver>,
    /// Identifies the immutable source-ordered lexical pattern catalog for Pretty checks.
    pretty_lexical_catalog: PrettyPatternCatalogId,
    /// Caches the least fixed point of every closed application Pretty boundary summary.
    closed_pretty_boundary_summaries: RefCell<Vec<PrettyBoundarySummary>>,
    /// Counts fixed-point and cache work for deterministic focused tests.
    #[cfg(test)]
    closed_pretty_summary_counts: Cell<ClosedPrettySummaryCounts>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Carries the pretty boundary summary while expanding resolved grammar type applications.
struct PrettyBoundarySummary {
    /// Supplies the first needed to materialize a concrete grammar application.
    first: OrderedPrettyBoundaries,
    /// Supplies the last needed to materialize a concrete grammar application.
    last: OrderedPrettyBoundaries,
    /// Supplies the nullable needed to materialize a concrete grammar application.
    nullable: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Gives one Pretty boundary a stable identity for fixed-point set normalization.
enum PrettyBoundaryIdentity {
    /// Identifies one fixed token definition.
    Fixed(crate::lowering::TokenDefinitionId),
    /// Identifies one regex-backed boundary language.
    Regex(String),
}

impl From<&MaterializedPrettyBoundary> for PrettyBoundaryIdentity {
    fn from(boundary: &MaterializedPrettyBoundary) -> Self {
        match boundary {
            MaterializedPrettyBoundary::Fixed(id) => Self::Fixed(*id),
            MaterializedPrettyBoundary::Regex(pattern) => Self::Regex(pattern.clone()),
        }
    }
}

#[derive(Clone, Debug, Default)]
/// Retains distinct Pretty boundaries in deterministic first-discovery order.
struct OrderedPrettyBoundaries {
    /// Stores boundary values in deterministic semantic iteration order.
    values: Vec<MaterializedPrettyBoundary>,
    /// Rejects duplicate values before they can expand fixed-point intermediates.
    identities: BTreeSet<PrettyBoundaryIdentity>,
    /// Counts attempted insertions for deterministic bounded-work tests.
    #[cfg(test)]
    attempted_insertions: usize,
}

impl PartialEq for OrderedPrettyBoundaries {
    fn eq(&self, other: &Self) -> bool {
        self.identities == other.identities
    }
}

impl Eq for OrderedPrettyBoundaries {}

impl OrderedPrettyBoundaries {
    /// Creates one ordered set containing exactly one boundary.
    fn singleton(boundary: MaterializedPrettyBoundary) -> Self {
        let mut boundaries = Self::default();
        boundaries.insert(boundary);
        boundaries
    }

    /// Inserts one boundary only when its semantic identity is not already retained.
    fn insert(&mut self, boundary: MaterializedPrettyBoundary) {
        #[cfg(test)]
        {
            self.attempted_insertions += 1;
        }
        if self
            .identities
            .insert(PrettyBoundaryIdentity::from(&boundary))
        {
            self.values.push(boundary);
        }
        debug_assert_eq!(self.identities.len(), self.values.len());
    }

    /// Extends this set from another ordered set without constructing duplicate intermediates.
    fn extend_from(&mut self, boundaries: &Self) {
        for boundary in &boundaries.values {
            self.insert(boundary.clone());
        }
    }

    /// Borrows retained values in deterministic first-discovery order.
    fn as_slice(&self) -> &[MaterializedPrettyBoundary] {
        &self.values
    }

    /// Reports whether every retained identity is also present in `other`.
    fn is_subset(&self, other: &Self) -> bool {
        self.identities.is_subset(&other.identities)
    }

    /// Returns the number of distinct retained boundaries.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns the number of attempted insertions that built this set.
    #[cfg(test)]
    fn attempted_insertions(&self) -> usize {
        self.attempted_insertions
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the pretty boundary site alternatives supported during materialization.
enum PrettyBoundarySite {
    /// Selects the fixed materialization alternative.
    Fixed(TokenUseId),
    /// Selects the field materialization alternative.
    Field(ResolvedFieldId),
}

/// Derives the materialize while materializing grammar applications and container shapes.
pub(crate) fn materialize(source_modeled: SourceModeledGrammar) -> MaterializationReport {
    let equivalence = source_modeled
        .resolved()
        .configuration
        .syntax
        .text_equivalence;
    let lexical_patterns =
        materialized_pretty_lexical_patterns(source_modeled.source_model(), equivalence);
    let mut pretty_boundary_prover = PrettyBoundaryProver::default();
    let pretty_lexical_catalog = pretty_boundary_prover.intern_catalog(&lexical_patterns);
    let lexical = plan_lexical(&source_modeled);
    let materializer = Materializer {
        source_modeled,
        lexical_plan: lexical.plan,
        application_by_key: BTreeMap::new(),
        applications: Vec::new(),
        framing: None,
        diagnostics: Vec::new(),
        active_aliases: Vec::new(),
        active_applications: Vec::new(),
        pretty_boundary_prover: RefCell::new(pretty_boundary_prover),
        pretty_lexical_catalog,
        closed_pretty_boundary_summaries: RefCell::default(),
        #[cfg(test)]
        closed_pretty_summary_counts: Cell::default(),
    };
    materializer.run(lexical.diagnostics)
}

impl Materializer {
    /// Derives the lexical plan while materializing grammar applications and container shapes.
    fn lexical_plan(&self) -> &LexicalPlan {
        self.lexical_plan
            .as_ref()
            .expect("semantic analysis projection runs only after successful lexical planning")
    }

    /// Derives the resolved while materializing grammar applications and container shapes.
    fn resolved(&self) -> &ResolvedSourceGrammar {
        self.source_modeled.resolved()
    }

    /// Derives the source model while materializing grammar applications and container shapes.
    fn source_model(&self) -> &SourceModel {
        self.source_modeled.source_model()
    }

    /// Derives the kind for definition while materializing grammar applications and container shapes.
    /// Describes the authored item a site came from and attaches the `#[greedy(...)]`
    /// acceptance that covers it, if any.
    ///
    /// A field attribute wins over its variant's, and a variant's over its Node's. The
    /// acceptance identity is the attribute's index in the source model, so every site
    /// materialized from the same authored attribute (across generic applications, across the
    /// fields of one variant, or across every variant of a Node) shares one identity and is
    /// matched as one group.
    fn with_greedy(
        &self,
        application: &MaterializedNodeApplication,
        variant: Option<ResolvedVariantId>,
        field: Option<ResolvedFieldId>,
        role: crate::AuthoredRole,
        declaration: Declaration,
    ) -> Declaration {
        let node = &self.resolved().nodes[application.node.index()];
        let mut variant_ident = None;
        let mut field_name = None;
        let mut index = 0;
        let mut variant_id = variant;
        if let Some(field) = field {
            match &node.body {
                crate::resolution::ResolvedNodeBody::Struct { .. } => {
                    index = node
                        .fields
                        .iter()
                        .position(|candidate| candidate.id == field)
                        .unwrap_or(0);
                }
                crate::resolution::ResolvedNodeBody::Enum { variants } => {
                    for candidate in variants {
                        if let Some(position) = candidate.fields.iter().position(|id| *id == field)
                        {
                            variant_id = Some(candidate.id);
                            index = position;
                        }
                    }
                }
            }
            field_name = self
                .resolved_field(field)
                .syntax
                .ident
                .as_ref()
                .map(|ident| ident.unraw().to_string());
        }
        if let (Some(variant), crate::resolution::ResolvedNodeBody::Enum { variants }) =
            (variant_id, &node.body)
        {
            variant_ident = variants
                .iter()
                .find(|candidate| candidate.id == variant)
                .map(|candidate| candidate.ident.clone());
        }
        let declaration = declaration.with_authored(crate::AuthoredSite {
            node: node.ident.clone(),
            variant: variant_ident,
            field: field_name,
            index,
            role,
        });
        let Some((position, greedy)) = self
            .source_model()
            .greedy
            .iter()
            .enumerate()
            .filter(|(_, greedy)| greedy.owner.covers(application.node, variant_id, field))
            .min_by_key(|(_, greedy)| greedy.owner.specificity())
        else {
            return declaration;
        };
        let id = u32::try_from(position).expect("greedy acceptance count fits u32");
        match &greedy.kinds {
            crate::source_model::GreedyKindNames::All => {
                declaration.with_greedy(crate::GreedyAcceptance::all(id))
            }
            crate::source_model::GreedyKindNames::Exact(definitions) => {
                let mut kinds = definitions
                    .iter()
                    .map(|definition| self.kind_for_definition(*definition))
                    .collect::<Vec<_>>();
                kinds.sort_unstable();
                kinds.dedup();
                declaration.with_greedy(crate::GreedyAcceptance::new(id, kinds))
            }
        }
    }

    fn kind_for_definition(&self, definition: crate::lowering::TokenDefinitionId) -> crate::KindId {
        self.lexical_plan()
            .lowering_map
            .token_definitions
            .iter()
            .find_map(|correlation| {
                (correlation.source == definition).then_some(correlation.lowered)
            })
            .expect("every source token definition has a lexical KindId correlation")
    }

    /// Expands seed Nodes transitively and publishes only a complete materialization plan.
    fn run(mut self, lexical_diagnostics: Vec<LexicalDiagnostic>) -> MaterializationReport {
        let seeds = self
            .resolved()
            .nodes
            .iter()
            .filter(|node| {
                node.generics
                    .params
                    .iter()
                    .all(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
            })
            .map(|node| node.id)
            .collect::<Vec<_>>();
        for node in seeds {
            self.ensure_application(node, Vec::new());
        }
        self.materialize_framing();
        if self.diagnostics.is_empty() && lexical_diagnostics.is_empty() {
            self.validate_closed_pretty_boundaries();
        }
        #[cfg(test)]
        let pretty_boundary_proof_counts = self.pretty_boundary_prover.borrow().proof_counts();
        #[cfg(test)]
        let closed_pretty_summary_counts = self.closed_pretty_summary_counts.get();
        if !self.diagnostics.is_empty() {
            return MaterializationReport {
                lexical_diagnostics,
                diagnostics: std::mem::take(&mut self.diagnostics),
                lowered: None,
                #[cfg(test)]
                pretty_boundary_proof_counts,
                #[cfg(test)]
                closed_pretty_summary_counts,
            };
        }
        if !lexical_diagnostics.is_empty() {
            return MaterializationReport {
                lexical_diagnostics,
                diagnostics: Vec::new(),
                lowered: None,
                #[cfg(test)]
                pretty_boundary_proof_counts,
                #[cfg(test)]
                closed_pretty_summary_counts,
            };
        }
        let (grammar, lowering_map) = self.project_normalized_grammar();
        MaterializationReport {
            lexical_diagnostics: Vec::new(),
            diagnostics: Vec::new(),
            lowered: Some(LoweredGrammar {
                source_modeled: self.source_modeled,
                lexical_plan: self
                    .lexical_plan
                    .expect("successful lexical planning precedes semantic analysis projection"),
                applications: self.applications,
                grammar,
                lowering_map,
            }),
            #[cfg(test)]
            pretty_boundary_proof_counts,
            #[cfg(test)]
            closed_pretty_summary_counts,
        }
    }

    /// Closes typed framing references and freezes their exact application correlation.
    ///
    /// # Long-function contract
    ///
    /// Rationale: resolved Node identity alone is insufficient once generic applications exist; the
    /// generated framing plan must select the same closed application used by strict parsing.
    /// Invariants: the island evaluates to one Node application correlated to its resolved Node, the
    /// payload owner evaluates to that exact application, and only the previously resolved variant ID
    /// crosses into the lowering map. Major phases: evaluate and validate the island type, evaluate
    /// and compare the optional payload owner, retain its exact variant correlation, then publish the
    /// single immutable framing application correlation consumed by emission planning.
    fn materialize_framing(&mut self) {
        let Some(framing) = self.resolved().configuration.framing.clone() else {
            return;
        };
        let environment = EvaluationEnvironment::default();
        let Some(island) = self.evaluate_type(&framing.island, None, &environment) else {
            return;
        };
        let ClosedType::Node(island) = island.closed else {
            self.push_diagnostic(
                framing.island.source,
                Some(framing.island.range),
                MaterializationDiagnosticKind::UnsupportedType,
                "framing island must close to one Node application".to_owned(),
            );
            return;
        };
        if self.applications[island.application.index()].node != framing.island_node {
            self.push_diagnostic(
                framing.island.source,
                Some(framing.island.range),
                MaterializationDiagnosticKind::UnsupportedType,
                "framing island application diverges from its resolved Node correlation".to_owned(),
            );
            return;
        }
        let payload_variant = if let Some(payload) = framing.payload {
            let Some(after) = self.evaluate_type(&payload.after, None, &environment) else {
                return;
            };
            let ClosedType::Node(after) = after.closed else {
                self.push_diagnostic(
                    payload.after.source,
                    Some(payload.after.range),
                    MaterializationDiagnosticKind::UnsupportedType,
                    "payload `after` owner must close to the framing island application".to_owned(),
                );
                return;
            };
            if after.application != island.application {
                self.push_diagnostic(
                    framing.island.source,
                    Some(framing.island.range),
                    MaterializationDiagnosticKind::UnsupportedType,
                    "payload `after` must resolve to the exact framing island application"
                        .to_owned(),
                );
                return;
            }
            Some(payload.variant)
        } else {
            None
        };
        self.framing = Some(FramingApplicationCorrelation {
            node: framing.island_node,
            application: island.application,
            payload_variant,
        });
    }

    /// Validates closed pretty boundaries.
    fn validate_closed_pretty_boundaries(&mut self) {
        self.initialize_closed_pretty_boundary_summaries();
        let invalid = self
            .applications
            .iter()
            .filter(|application| {
                self.source_model()
                    .nodes
                    .iter()
                    .find(|node| node.resolved_node == application.node)
                    .is_some_and(|node| {
                        node.configured_traits
                            .effective
                            .iter()
                            .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty)
                    })
            })
            .filter_map(|application| {
                self.body_pretty_lexing_violation(&application.body)
                    .map(|site| (application.node, site))
            })
            .collect::<BTreeMap<_, _>>();
        for (node_id, site) in invalid {
            let node = self.resolved().nodes[node_id.index()].clone();
            let message = format!(
                "a closed Pretty boundary in Node `{}` would change tokenization",
                node.key.segments.join("::")
            );
            match site {
                PrettyBoundarySite::Fixed(token_use) => {
                    let site = self.source_model().token_uses[token_use.index()]
                        .site
                        .clone();
                    self.push_diagnostic_in_module(
                        Some(site.module),
                        site.source,
                        Some(site.range),
                        MaterializationDiagnosticKind::UnsafePrettyBoundary,
                        message,
                    );
                }
                PrettyBoundarySite::Field(field) => {
                    let field = self.resolved_field(field).clone();
                    self.push_diagnostic_in_module(
                        Some(node.module),
                        field.ty.source,
                        Some(field.range),
                        MaterializationDiagnosticKind::UnsafePrettyBoundary,
                        message,
                    );
                }
            }
        }
    }

    /// Computes every application boundary summary as one finite monotone least fixed point.
    fn initialize_closed_pretty_boundary_summaries(&self) {
        let mut current = vec![PrettyBoundarySummary::default(); self.applications.len()];
        let lattice_height = self
            .applications
            .len()
            .saturating_mul(
                self.source_model()
                    .token_definitions
                    .len()
                    .saturating_mul(2)
                    .saturating_add(1),
            )
            .saturating_add(1);
        let mut rounds = 0_usize;
        loop {
            rounds += 1;
            assert!(
                rounds <= lattice_height,
                "closed Pretty-summary fixed point exceeded its finite lattice height"
            );
            *self.closed_pretty_boundary_summaries.borrow_mut() = current.clone();
            #[cfg(test)]
            self.update_closed_pretty_summary_counts(|counts| {
                counts.fixed_point_rounds += 1;
            });

            let next = self
                .applications
                .iter()
                .map(|application| {
                    #[cfg(test)]
                    self.update_closed_pretty_summary_counts(|counts| {
                        counts.body_evaluations += 1;
                    });
                    self.body_pretty_boundaries(&application.body, &mut BTreeSet::new())
                })
                .collect::<Vec<_>>();
            debug_assert!(current.iter().zip(&next).all(|(before, after)| {
                before.first.is_subset(&after.first)
                    && before.last.is_subset(&after.last)
                    && (!before.nullable || after.nullable)
            }));
            if next == current {
                break;
            }
            current = next;
        }

        #[cfg(test)]
        {
            let summaries = self.closed_pretty_boundary_summaries.borrow();
            let max_retained_boundaries = summaries
                .iter()
                .flat_map(|summary| [&summary.first, &summary.last])
                .map(OrderedPrettyBoundaries::len)
                .max()
                .unwrap_or_default();
            let max_attempted_boundary_insertions = summaries
                .iter()
                .flat_map(|summary| [&summary.first, &summary.last])
                .map(OrderedPrettyBoundaries::attempted_insertions)
                .max()
                .unwrap_or_default();
            self.update_closed_pretty_summary_counts(|counts| {
                counts.max_retained_boundaries = max_retained_boundaries;
                counts.max_attempted_boundary_insertions = max_attempted_boundary_insertions;
            });
        }

        #[cfg(test)]
        self.compare_closed_pretty_summaries_with_cycle_cut_oracle();
    }

    /// Updates deterministic test-only counters without introducing shared global state.
    #[cfg(test)]
    fn update_closed_pretty_summary_counts(
        &self,
        update: impl FnOnce(&mut ClosedPrettySummaryCounts),
    ) {
        let mut counts = self.closed_pretty_summary_counts.get();
        update(&mut counts);
        self.closed_pretty_summary_counts.set(counts);
    }

    /// Compares the fixed point with the former active-cycle-cut traversal on small fixtures.
    #[cfg(test)]
    fn compare_closed_pretty_summaries_with_cycle_cut_oracle(&self) {
        let fixed_point = std::mem::take(&mut *self.closed_pretty_boundary_summaries.borrow_mut());
        let mismatches = self
            .applications
            .iter()
            .enumerate()
            .filter(|(index, application)| {
                let oracle = self.body_pretty_boundaries(&application.body, &mut BTreeSet::new());
                oracle != fixed_point[*index]
            })
            .count();
        *self.closed_pretty_boundary_summaries.borrow_mut() = fixed_point;
        self.update_closed_pretty_summary_counts(|counts| {
            counts.legacy_mismatches = mismatches;
        });
    }

    /// Derives the body pretty lexing violation while materializing grammar applications and container shapes.
    fn body_pretty_lexing_violation(
        &self,
        body: &MaterializedNodeBody,
    ) -> Option<PrettyBoundarySite> {
        match body {
            MaterializedNodeBody::Struct(elements) | MaterializedNodeBody::Presence(elements) => {
                self.sequence_pretty_lexing_violation(elements)
            }
            MaterializedNodeBody::Enum(variants) => variants
                .iter()
                .find_map(|variant| self.sequence_pretty_lexing_violation(&variant.elements)),
            MaterializedNodeBody::Pratt(variants) => {
                variants.iter().find_map(|variant| match variant {
                    MaterializedPrattVariant::Nud(nud) => {
                        self.sequence_pretty_lexing_violation(&nud.body)
                    }
                    MaterializedPrattVariant::Led(led) => {
                        self.sequence_pretty_lexing_violation(&led.tail)
                    }
                })
            }
        }
    }

    /// Derives the sequence pretty lexing violation while materializing grammar applications and container shapes.
    fn sequence_pretty_lexing_violation(
        &self,
        elements: &[MaterializedElement],
    ) -> Option<PrettyBoundarySite> {
        let mut trailing = OrderedPrettyBoundaries::default();
        for element in elements {
            if self.attachment_changes_pretty_lexing(element) {
                return Some(Self::element_pretty_boundary_site(element));
            }
            let summary = self.element_pretty_boundaries(element, &mut BTreeSet::new());
            if !Self::element_has_pretty_break_before(element)
                && self.pretty_boundary_pairs_change_lexing(
                    trailing.as_slice(),
                    summary.first.as_slice(),
                )
            {
                return Some(Self::element_pretty_boundary_site(element));
            }
            let last = if Self::element_has_pretty_break_after(element) {
                OrderedPrettyBoundaries::default()
            } else {
                summary.last
            };
            if summary.nullable {
                trailing.extend_from(&last);
            } else {
                trailing = last;
            }
        }
        None
    }

    /// Reports whether this materialized element emits a separating Pretty break before itself.
    fn element_has_pretty_break_before(element: &MaterializedElement) -> bool {
        matches!(
            element,
            MaterializedElement::SemanticField { pretty, .. }
                if pretty.break_before.is_some()
        )
    }

    /// Reports whether this materialized element emits a separating Pretty break after itself.
    fn element_has_pretty_break_after(element: &MaterializedElement) -> bool {
        matches!(
            element,
            MaterializedElement::SemanticField { pretty, .. }
                if pretty.break_after.is_some()
        )
    }

    /// Derives the element pretty boundary site while materializing grammar applications and container shapes.
    fn element_pretty_boundary_site(element: &MaterializedElement) -> PrettyBoundarySite {
        match element {
            MaterializedElement::FixedToken { token_use } => PrettyBoundarySite::Fixed(*token_use),
            MaterializedElement::OptionalFixedTokenGroup { tokens, .. } => {
                PrettyBoundarySite::Fixed(tokens[0])
            }
            MaterializedElement::SemanticField { field, .. } => PrettyBoundarySite::Field(*field),
        }
    }

    /// Derives the attachment changes pretty lexing while materializing grammar applications and container shapes.
    fn attachment_changes_pretty_lexing(&self, element: &MaterializedElement) -> bool {
        let (mut summaries, optional_sequences) = match element {
            MaterializedElement::OptionalFixedTokenGroup { tokens, .. } => {
                return self.fixed_sequence_changes_pretty_lexing(tokens);
            }
            MaterializedElement::SemanticField {
                projection,
                attachment,
                ..
            } => {
                let mut summaries = attachment
                    .as_ref()
                    .map_or(&[][..], |attachment| attachment.before.as_slice())
                    .iter()
                    .map(|syntax| self.attached_pretty_boundaries(syntax))
                    .collect::<Vec<_>>();
                summaries.push(
                    self.semantic_projection_pretty_boundaries(projection, &mut BTreeSet::new()),
                );
                summaries.extend(
                    attachment
                        .as_ref()
                        .map_or(&[][..], |attachment| attachment.after.as_slice())
                        .iter()
                        .map(|syntax| self.attached_pretty_boundaries(syntax)),
                );
                let optional_sequences = attachment
                    .iter()
                    .flat_map(|attachment| attachment.before.iter().chain(&attachment.after))
                    .filter_map(|syntax| match syntax {
                        MaterializedAttachedFixedSyntax::Required(_) => None,
                        MaterializedAttachedFixedSyntax::Optional { tokens, .. } => Some(tokens),
                    })
                    .chain(match projection {
                        MaterializedSemanticFieldProjection::Direct { .. } => None,
                        MaterializedSemanticFieldProjection::FixedPresence { tokens } => {
                            Some(tokens)
                        }
                    })
                    .collect::<Vec<_>>();
                (summaries, optional_sequences)
            }
            MaterializedElement::FixedToken { .. } => return false,
        };
        if optional_sequences
            .into_iter()
            .any(|tokens| self.fixed_sequence_changes_pretty_lexing(tokens))
        {
            return true;
        }
        let mut trailing = OrderedPrettyBoundaries::default();
        for summary in summaries.drain(..) {
            if self
                .pretty_boundary_pairs_change_lexing(trailing.as_slice(), summary.first.as_slice())
            {
                return true;
            }
            if summary.nullable {
                trailing.extend_from(&summary.last);
            } else {
                trailing = summary.last;
            }
        }
        false
    }

    /// Derives the pretty boundary pairs change lexing while materializing grammar applications and container shapes.
    fn pretty_boundary_pairs_change_lexing(
        &self,
        left: &[MaterializedPrettyBoundary],
        right: &[MaterializedPrettyBoundary],
    ) -> bool {
        let equivalence = self.resolved().configuration.syntax.text_equivalence;
        let mut prover = self.pretty_boundary_prover.borrow_mut();
        left.iter().any(|left| {
            right.iter().any(|right| {
                materialized_pretty_boundary_changes_lexing(
                    self.source_model(),
                    equivalence,
                    left,
                    right,
                    self.pretty_lexical_catalog,
                    &mut prover,
                )
            })
        })
    }

    /// Derives the element pretty boundaries while materializing grammar applications and container shapes.
    fn element_pretty_boundaries(
        &self,
        element: &MaterializedElement,
        active: &mut BTreeSet<MaterializedApplicationId>,
    ) -> PrettyBoundarySummary {
        match element {
            MaterializedElement::FixedToken { token_use } => {
                self.fixed_pretty_boundaries(*token_use)
            }
            MaterializedElement::OptionalFixedTokenGroup { tokens, .. } => {
                self.optional_fixed_pretty_boundaries(tokens)
            }
            MaterializedElement::SemanticField {
                projection,
                attachment,
                ..
            } => {
                let semantic = self.semantic_projection_pretty_boundaries(projection, active);
                let Some(attachment) = attachment else {
                    return semantic;
                };
                let summaries = attachment
                    .before
                    .iter()
                    .map(|syntax| self.attached_pretty_boundaries(syntax))
                    .chain(std::iter::once(semantic))
                    .chain(
                        attachment
                            .after
                            .iter()
                            .map(|syntax| self.attached_pretty_boundaries(syntax)),
                    )
                    .collect::<Vec<_>>();
                Self::pretty_sequence_summary(&summaries)
            }
        }
    }

    /// Summarizes the concrete syntax that produces one semantic field value.
    fn semantic_projection_pretty_boundaries(
        &self,
        projection: &MaterializedSemanticFieldProjection,
        active: &mut BTreeSet<MaterializedApplicationId>,
    ) -> PrettyBoundarySummary {
        match projection {
            MaterializedSemanticFieldProjection::Direct { ty, .. } => {
                self.closed_pretty_boundaries(attachment_terminal(ty), active)
            }
            MaterializedSemanticFieldProjection::FixedPresence { tokens } => {
                self.optional_fixed_pretty_boundaries(tokens)
            }
        }
    }

    /// Summarizes one required or erased-optional attachment entry.
    fn attached_pretty_boundaries(
        &self,
        syntax: &MaterializedAttachedFixedSyntax,
    ) -> PrettyBoundarySummary {
        match syntax {
            MaterializedAttachedFixedSyntax::Required(token) => {
                self.fixed_pretty_boundaries(*token)
            }
            MaterializedAttachedFixedSyntax::Optional { tokens, .. } => {
                self.optional_fixed_pretty_boundaries(tokens)
            }
        }
    }

    /// Summarizes one non-empty atomic optional fixed sequence.
    fn optional_fixed_pretty_boundaries(&self, tokens: &[TokenUseId]) -> PrettyBoundarySummary {
        PrettyBoundarySummary {
            first: self.fixed_pretty_boundaries(tokens[0]).first,
            last: self
                .fixed_pretty_boundaries(*tokens.last().expect("optional groups are non-empty"))
                .last,
            nullable: true,
        }
    }

    /// Summarizes a source-ordered sequence while preserving nullable boundary alternatives.
    fn pretty_sequence_summary(summaries: &[PrettyBoundarySummary]) -> PrettyBoundarySummary {
        let mut first = OrderedPrettyBoundaries::default();
        for summary in summaries {
            first.extend_from(&summary.first);
            if !summary.nullable {
                break;
            }
        }
        let mut last = OrderedPrettyBoundaries::default();
        for summary in summaries.iter().rev() {
            last.extend_from(&summary.last);
            if !summary.nullable {
                break;
            }
        }
        PrettyBoundarySummary {
            first,
            last,
            nullable: summaries.iter().all(|summary| summary.nullable),
        }
    }

    /// Reports whether canonical Pretty spacing would retokenize one fixed sequence.
    fn fixed_sequence_changes_pretty_lexing(&self, tokens: &[TokenUseId]) -> bool {
        tokens.windows(2).any(|pair| {
            self.pretty_boundary_pairs_change_lexing(
                self.fixed_pretty_boundaries(pair[0]).last.as_slice(),
                self.fixed_pretty_boundaries(pair[1]).first.as_slice(),
            )
        })
    }

    /// Derives the fixed pretty boundaries while materializing grammar applications and container shapes.
    fn fixed_pretty_boundaries(&self, token_use: TokenUseId) -> PrettyBoundarySummary {
        let definition = self.source_model().token_uses[token_use.index()].definition;
        let boundary = MaterializedPrettyBoundary::Fixed(definition);
        PrettyBoundarySummary {
            first: OrderedPrettyBoundaries::singleton(boundary.clone()),
            last: OrderedPrettyBoundaries::singleton(boundary),
            nullable: false,
        }
    }

    /// Derives the closed pretty boundaries while materializing grammar applications and container shapes.
    fn closed_pretty_boundaries(
        &self,
        ty: &ClosedType,
        active: &mut BTreeSet<MaterializedApplicationId>,
    ) -> PrettyBoundarySummary {
        match ty {
            ClosedType::ContentToken(token) => {
                let pattern = self
                    .source_model()
                    .token_definitions
                    .iter()
                    .find_map(|definition| match &definition.kind {
                        crate::source_model::TokenDefinitionKind::Content(content)
                            if content.semantic == *token =>
                        {
                            Some(content.pattern.clone())
                        }
                        crate::source_model::TokenDefinitionKind::Content(_)
                        | crate::source_model::TokenDefinitionKind::Fixed(_)
                        | crate::source_model::TokenDefinitionKind::LexicalBase(_) => None,
                    })
                    .expect("every closed content token has a source lexical pattern");
                let boundary = MaterializedPrettyBoundary::Regex(pattern);
                PrettyBoundarySummary {
                    first: OrderedPrettyBoundaries::singleton(boundary.clone()),
                    last: OrderedPrettyBoundaries::singleton(boundary),
                    nullable: false,
                }
            }
            ClosedType::Node(node) => {
                if let Some(summary) = self
                    .closed_pretty_boundary_summaries
                    .borrow()
                    .get(node.application.index())
                    .cloned()
                {
                    #[cfg(test)]
                    self.update_closed_pretty_summary_counts(|counts| {
                        counts.cache_queries += 1;
                    });
                    return summary;
                }
                if !active.insert(node.application) {
                    return PrettyBoundarySummary::default();
                }
                let summary = self.body_pretty_boundaries(
                    &self.applications[node.application.index()].body,
                    active,
                );
                active.remove(&node.application);
                summary
            }
            ClosedType::Boxed(inner) | ClosedType::OneOrMore(inner) => {
                self.closed_pretty_boundaries(inner, active)
            }
            ClosedType::Optional(inner) | ClosedType::ZeroOrMore(inner) => {
                let mut summary = self.closed_pretty_boundaries(inner, active);
                summary.nullable = true;
                summary
            }
        }
    }

    /// Derives the body pretty boundaries while materializing grammar applications and container shapes.
    fn body_pretty_boundaries(
        &self,
        body: &MaterializedNodeBody,
        active: &mut BTreeSet<MaterializedApplicationId>,
    ) -> PrettyBoundarySummary {
        match body {
            MaterializedNodeBody::Struct(elements) | MaterializedNodeBody::Presence(elements) => {
                self.elements_pretty_boundaries(elements, active)
            }
            MaterializedNodeBody::Enum(variants) => self.alternative_pretty_boundaries(
                variants
                    .iter()
                    .map(|variant| self.elements_pretty_boundaries(&variant.elements, active)),
            ),
            MaterializedNodeBody::Pratt(variants) => {
                self.alternative_pretty_boundaries(variants.iter().map(|variant| match variant {
                    MaterializedPrattVariant::Nud(nud) => {
                        self.elements_pretty_boundaries(&nud.body, active)
                    }
                    MaterializedPrattVariant::Led(led) => {
                        self.elements_pretty_boundaries(&led.tail, active)
                    }
                }))
            }
        }
    }

    /// Derives the elements pretty boundaries while materializing grammar applications and container shapes.
    fn elements_pretty_boundaries(
        &self,
        elements: &[MaterializedElement],
        active: &mut BTreeSet<MaterializedApplicationId>,
    ) -> PrettyBoundarySummary {
        let summaries = elements
            .iter()
            .map(|element| self.element_pretty_boundaries(element, active))
            .collect::<Vec<_>>();
        let mut first = OrderedPrettyBoundaries::default();
        for summary in &summaries {
            first.extend_from(&summary.first);
            if !summary.nullable {
                break;
            }
        }
        let mut last = OrderedPrettyBoundaries::default();
        for summary in summaries.iter().rev() {
            last.extend_from(&summary.last);
            if !summary.nullable {
                break;
            }
        }
        PrettyBoundarySummary {
            first,
            last,
            nullable: summaries.iter().all(|summary| summary.nullable),
        }
    }

    /// Derives the alternative pretty boundaries while materializing grammar applications and container shapes.
    fn alternative_pretty_boundaries(
        &self,
        summaries: impl Iterator<Item = PrettyBoundarySummary>,
    ) -> PrettyBoundarySummary {
        let mut combined = PrettyBoundarySummary::default();
        for summary in summaries {
            combined.first.extend_from(&summary.first);
            combined.last.extend_from(&summary.last);
            combined.nullable |= summary.nullable;
        }
        combined
    }

    /// Interns one closed Node application before recursively materializing its body.
    fn ensure_application(
        &mut self,
        node_id: NodeId,
        type_arguments: Vec<EvaluatedType>,
    ) -> Option<MaterializedApplicationId> {
        // Memoize before descending through recursive applications so legal recursion terminates with stable identities.
        let key = ApplicationKey {
            node: node_id,
            arguments: type_arguments
                .iter()
                .map(|argument| argument.closed.identity())
                .collect(),
        };
        if let Some(id) = self.application_by_key.get(&key) {
            return Some(*id);
        }
        let node = self.resolved().nodes[node_id.index()].clone();
        if self
            .active_applications
            .iter()
            .any(|active| active.node == node_id)
        {
            self.push_node_diagnostic(
                &node,
                MaterializationDiagnosticKind::NonRegularRecursion,
                format!(
                    "recursive Node `{}` changes its closed type arguments",
                    node.key.segments.join("::")
                ),
            );
            return None;
        }
        let type_parameters = node
            .generics
            .params
            .iter()
            .filter_map(|parameter| match parameter {
                GenericParam::Type(parameter) => Some(parameter.ident.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if type_parameters.len() != type_arguments.len() {
            self.push_node_diagnostic(
                &node,
                MaterializationDiagnosticKind::OpenApplication,
                format!(
                    "Node `{}` needs {} closed type argument(s), found {}",
                    node.key.segments.join("::"),
                    type_parameters.len(),
                    type_arguments.len(),
                ),
            );
            return None;
        }
        if node
            .generics
            .params
            .iter()
            .any(|parameter| matches!(parameter, GenericParam::Const(_)))
        {
            self.push_node_diagnostic(
                &node,
                MaterializationDiagnosticKind::ConstGeneric,
                "const-generic Nodes are outside the closed grammar type language".to_owned(),
            );
            return None;
        }

        let id = MaterializedApplicationId::next(self.applications.len());
        self.application_by_key.insert(key.clone(), id);
        self.active_applications.push(key);
        let display_name = self.display_application(&node, &type_arguments);
        let closed_type_arguments = type_arguments
            .iter()
            .map(|argument| argument.closed.clone())
            .collect();
        self.applications.push(MaterializedNodeApplication {
            id,
            node: node_id,
            display_name,
            type_arguments: closed_type_arguments,
            pretty: crate::source_model::PrettyHints::default(),
            body: MaterializedNodeBody::Struct(Vec::new()),
        });
        let parameter_ids = self
            .resolved()
            .generic_parameters
            .iter()
            .filter(|parameter| parameter.owner == SemanticOwner::Node(node_id))
            .map(|parameter| parameter.id)
            .collect::<Vec<_>>();
        let types = parameter_ids
            .into_iter()
            .zip(type_arguments)
            .collect::<BTreeMap<_, _>>();
        let lifetimes = node
            .generics
            .params
            .iter()
            .filter_map(|parameter| match parameter {
                GenericParam::Lifetime(parameter) => Some((
                    parameter.lifetime.ident.to_string(),
                    parameter.lifetime.clone(),
                )),
                _ => None,
            })
            .collect();
        let environment = EvaluationEnvironment { types, lifetimes };
        let current_use = MaterializedNodeUse {
            application: id,
            lifetimes: node
                .generics
                .params
                .iter()
                .filter_map(|parameter| match parameter {
                    GenericParam::Lifetime(parameter) => Some(parameter.lifetime.clone()),
                    _ => None,
                })
                .collect(),
        };
        let source_node = self
            .source_model()
            .nodes
            .iter()
            .find(|source_node| source_node.resolved_node == node_id)
            .cloned()
            .expect("source model covers every resolved Node");
        let body =
            match source_node.body {
                NodeBody::Struct(elements) => MaterializedNodeBody::Struct(
                    self.materialize_elements(&elements, &current_use, &environment),
                ),
                NodeBody::Presence(elements) => MaterializedNodeBody::Presence(
                    self.materialize_elements(&elements, &current_use, &environment),
                ),
                NodeBody::Enum(variants) => MaterializedNodeBody::Enum(
                    variants
                        .into_iter()
                        .map(|variant| MaterializedVariant {
                            resolved_variant: variant.resolved_variant,
                            name: variant.name,
                            pretty: variant.pretty,
                            elements: self.materialize_elements(
                                &variant.elements,
                                &current_use,
                                &environment,
                            ),
                        })
                        .collect(),
                ),
                NodeBody::Pratt(variants) => MaterializedNodeBody::Pratt(
                    variants
                        .into_iter()
                        .filter_map(|variant| {
                            self.materialize_pratt_variant(variant, &current_use, &environment)
                        })
                        .collect(),
                ),
            };
        self.applications[id.index()].pretty = source_node.pretty;
        self.applications[id.index()].body = body;
        let popped = self.active_applications.pop();
        debug_assert!(popped.is_some_and(|application| application.node == node_id));
        Some(id)
    }

    /// Derives the materialize pratt variant while materializing grammar applications and container shapes.
    fn materialize_pratt_variant(
        &mut self,
        variant: SourcePrattVariant,
        current_application: &MaterializedNodeUse,
        environment: &EvaluationEnvironment,
    ) -> Option<MaterializedPrattVariant> {
        let common = MaterializedPrattCommon {
            resolved_variant: variant.common.resolved_variant,
            name: variant.common.name,
            pretty: variant.common.pretty,
        };
        let mut elements =
            self.materialize_elements(&variant.common.elements, current_application, environment);
        match variant.position {
            SourcePrattPosition::Atom => {
                Some(MaterializedPrattVariant::Nud(MaterializedPrattNud {
                    common,
                    position: MaterializedPrattNudPosition::Atom,
                    body: elements,
                }))
            }
            SourcePrattPosition::Prefix { bp } => {
                if !self.validate_single_pratt_operand(
                    common.resolved_variant,
                    current_application,
                    &elements,
                    "prefix",
                ) {
                    return None;
                }
                Some(MaterializedPrattVariant::Nud(MaterializedPrattNud {
                    common,
                    position: MaterializedPrattNudPosition::Prefix { bp },
                    body: elements,
                }))
            }
            SourcePrattPosition::Infix { lbp, rbp } => {
                let lhs = self.take_pratt_lhs(
                    common.resolved_variant,
                    current_application,
                    &mut elements,
                )?;
                if !self.validate_single_pratt_rhs(
                    common.resolved_variant,
                    current_application,
                    &elements,
                ) {
                    return None;
                }
                Some(MaterializedPrattVariant::Led(MaterializedPrattLed {
                    common,
                    position: MaterializedPrattLedPosition::Infix { lbp, rbp },
                    lhs,
                    tail: elements,
                }))
            }
            SourcePrattPosition::Postfix { bp } => {
                let lhs = self.take_pratt_lhs(
                    common.resolved_variant,
                    current_application,
                    &mut elements,
                )?;
                Some(MaterializedPrattVariant::Led(MaterializedPrattLed {
                    common,
                    position: MaterializedPrattLedPosition::Postfix { bp },
                    lhs,
                    tail: elements,
                }))
            }
        }
    }

    /// Validates one exact Pratt operand without admitting other semantic fields.
    fn validate_single_pratt_operand(
        &mut self,
        variant: ResolvedVariantId,
        current_application: &MaterializedNodeUse,
        elements: &[MaterializedElement],
        role: &str,
    ) -> bool {
        let semantic = elements
            .iter()
            .filter(|element| matches!(element, MaterializedElement::SemanticField { .. }))
            .collect::<Vec<_>>();
        if semantic.len() != 1 {
            let (source, range) = self.resolved_variant_site(variant);
            self.push_diagnostic(
                source,
                Some(range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                format!(
                    "a Pratt {role} requires exactly one Box<Self> operand; found {} semantic fields",
                    semantic.len(),
                ),
            );
            return false;
        }
        if exact_pratt_self_operand(semantic[0], current_application) {
            return true;
        }
        let MaterializedElement::SemanticField { field, .. } = semantic[0] else {
            unreachable!("Pratt operand selection contains only semantic fields")
        };
        let resolved_field = self.resolved_field(*field).clone();
        self.push_diagnostic(
            resolved_field.ty.source,
            Some(resolved_field.range),
            MaterializationDiagnosticKind::InvalidPrattShape,
            format!("a Pratt {role} operand must be an authored Box<Self> for this application"),
        );
        false
    }

    /// Validates exactly one recursive infix rhs while admitting ordinary semantic tail fields.
    fn validate_single_pratt_rhs(
        &mut self,
        variant: ResolvedVariantId,
        current_application: &MaterializedNodeUse,
        elements: &[MaterializedElement],
    ) -> bool {
        let recursive = elements
            .iter()
            .filter(|element| {
                let MaterializedElement::SemanticField {
                    projection: MaterializedSemanticFieldProjection::Direct { ty, .. },
                    ..
                } = element
                else {
                    return false;
                };
                is_recursive_pratt_operand(ty, current_application.application)
            })
            .collect::<Vec<_>>();
        if recursive.len() != 1 {
            let (source, range) = self.resolved_variant_site(variant);
            self.push_diagnostic(
                source,
                Some(range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                format!(
                    "a Pratt infix rhs requires exactly one recursive Box<Self> operand; found {} recursive semantic fields",
                    recursive.len(),
                ),
            );
            return false;
        }
        if exact_pratt_self_operand(recursive[0], current_application) {
            return true;
        }
        let MaterializedElement::SemanticField { field, .. } = recursive[0] else {
            unreachable!("recursive Pratt rhs selection contains only semantic fields")
        };
        let resolved_field = self.resolved_field(*field).clone();
        self.push_diagnostic(
            resolved_field.ty.source,
            Some(resolved_field.range),
            MaterializationDiagnosticKind::InvalidPrattShape,
            "a Pratt infix rhs operand must be an authored Box<Self> for this application"
                .to_owned(),
        );
        false
    }

    /// Derives the resolved variant site while materializing grammar applications and container shapes.
    fn resolved_variant_site(&self, variant: ResolvedVariantId) -> (SourceId, Range<u32>) {
        self.resolved()
            .nodes
            .iter()
            .find_map(|node| match &node.body {
                crate::resolution::ResolvedNodeBody::Enum { variants } => variants
                    .iter()
                    .find(|candidate| candidate.id == variant)
                    .map(|variant| (node.source, variant.range.clone())),
                crate::resolution::ResolvedNodeBody::Struct { .. } => None,
            })
            .expect("source Pratt variant has a resolved identity")
    }

    /// Removes and validates the single implicit left operand from a Pratt led body.
    fn take_pratt_lhs(
        &mut self,
        variant: ResolvedVariantId,
        current_application: &MaterializedNodeUse,
        elements: &mut Vec<MaterializedElement>,
    ) -> Option<MaterializedPrattLhs> {
        // Extract exactly one typed Pratt left-hand side before materializing led fields and binding-power thresholds.
        let (variant_source, resolved_variant) = self
            .resolved()
            .nodes
            .iter()
            .find_map(|node| match &node.body {
                crate::resolution::ResolvedNodeBody::Enum { variants } => variants
                    .iter()
                    .find(|candidate| candidate.id == variant)
                    .map(|variant| (node.source, variant)),
                crate::resolution::ResolvedNodeBody::Struct { .. } => None,
            })
            .map(|(source, variant)| (source, variant.clone()))
            .expect("source Pratt variant has a resolved identity");
        let Some(lhs_field) = resolved_variant.fields.first().copied() else {
            self.push_diagnostic(
                variant_source,
                Some(resolved_variant.range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                "a Pratt led variant requires a first Box<Self> field for its implicit lhs"
                    .to_owned(),
            );
            return None;
        };
        let Some(index) = elements.iter().position(|element| {
            matches!(
                element,
                MaterializedElement::SemanticField {
                    field,
                    projection: MaterializedSemanticFieldProjection::Direct { .. },
                    ..
                } if *field == lhs_field
            )
        }) else {
            self.push_diagnostic(
                variant_source,
                Some(resolved_variant.range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                "a Pratt led variant requires a materializable first Box<Self> field".to_owned(),
            );
            return None;
        };
        if elements[..index]
            .iter()
            .any(|element| matches!(element, MaterializedElement::FixedToken { .. }))
        {
            let resolved_field = self.resolved_field(lhs_field).clone();
            self.push_diagnostic(
                resolved_field.ty.source,
                Some(resolved_field.range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                "fixed syntax before the implicit Pratt lhs cannot be consumed by a led variant"
                    .to_owned(),
            );
            return None;
        }
        let MaterializedElement::SemanticField {
            field,
            projection:
                MaterializedSemanticFieldProjection::Direct {
                    ty,
                    trace,
                    separator,
                    attachment_before,
                    attachment_after,
                    ..
                },
            attachment,
            pretty,
        } = elements.remove(index)
        else {
            unreachable!("the selected Pratt lhs element is semantic")
        };
        if attachment
            .as_ref()
            .is_some_and(|attachment| !attachment.before.is_empty())
        {
            let resolved_field = self.resolved_field(lhs_field).clone();
            self.push_diagnostic(
                resolved_field.ty.source,
                Some(resolved_field.range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                "fixed syntax before the implicit Pratt lhs cannot be consumed by a led variant"
                    .to_owned(),
            );
            return None;
        }
        if attachment.as_ref().is_some_and(|attachment| {
            attachment
                .after
                .iter()
                .any(|syntax| matches!(syntax, MaterializedAttachedFixedSyntax::Optional { .. }))
        }) {
            let resolved_field = self.resolved_field(lhs_field).clone();
            self.push_diagnostic(
                resolved_field.ty.source,
                Some(resolved_field.range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                "optional fixed syntax cannot be attached to the implicit Pratt lhs".to_owned(),
            );
            return None;
        }
        if let Some(attachment) = attachment {
            elements.splice(
                index..index,
                attachment.after.into_iter().map(|syntax| {
                    let MaterializedAttachedFixedSyntax::Required(token_use) = syntax else {
                        unreachable!("optional Pratt lhs syntax was rejected")
                    };
                    MaterializedElement::FixedToken { token_use }
                }),
            );
        }
        if !exact_pratt_self_parts(&ty, &trace, separator.as_ref(), current_application) {
            let resolved_field = self.resolved_field(field).clone();
            self.push_diagnostic(
                resolved_field.ty.source,
                Some(resolved_field.range),
                MaterializationDiagnosticKind::InvalidPrattShape,
                "the first field of a Pratt led variant must be an authored Box<Self> for this application"
                    .to_owned(),
            );
            return None;
        }
        let ClosedType::Boxed(inner) = ty else {
            unreachable!("validated Pratt lhs is boxed")
        };
        let ClosedType::Node(owner) = *inner else {
            unreachable!("validated Pratt lhs targets the owning Node")
        };
        Some(MaterializedPrattLhs {
            field,
            owner,
            trace,
            pretty,
            attachment_before,
            attachment_after,
        })
    }

    /// Derives the materialize elements while materializing grammar applications and container shapes.
    fn materialize_elements(
        &mut self,
        elements: &[GrammarElement],
        current_application: &MaterializedNodeUse,
        environment: &EvaluationEnvironment,
    ) -> Vec<MaterializedElement> {
        // Preserve authored element order while separating syntax-only attachments from semantic fields.
        let mut materialized = Vec::new();
        for element in elements {
            match element {
                GrammarElement::FixedToken { token_use, .. } => {
                    materialized.push(MaterializedElement::FixedToken {
                        token_use: *token_use,
                    });
                }
                GrammarElement::OptionalFixedTokenGroup { id, tokens, site } => {
                    materialized.push(MaterializedElement::OptionalFixedTokenGroup {
                        element: *id,
                        tokens: tokens.iter().map(|token| token.token_use).collect(),
                        site: site.clone(),
                    });
                }
                GrammarElement::SemanticField {
                    resolved_field,
                    projection,
                    attachment,
                    separator,
                    pratt_extensions,
                    pretty,
                    attachment_before,
                    attachment_after,
                    ..
                } => {
                    let field = self.resolved_field(*resolved_field).clone();
                    let attachment =
                        attachment
                            .as_ref()
                            .map(|attachment| MaterializedFixedAttachment {
                                before: attachment
                                    .before
                                    .iter()
                                    .map(materialize_attached_fixed)
                                    .collect(),
                                after: attachment
                                    .after
                                    .iter()
                                    .map(materialize_attached_fixed)
                                    .collect(),
                            });
                    let SemanticFieldProjection::Direct = projection else {
                        let SemanticFieldProjection::FixedPresence { tokens } = projection else {
                            unreachable!()
                        };
                        materialized.push(MaterializedElement::SemanticField {
                            field: field.id,
                            projection: MaterializedSemanticFieldProjection::FixedPresence {
                                tokens: tokens.iter().map(|token| token.token_use).collect(),
                            },
                            attachment,
                            pretty: pretty.clone(),
                        });
                        continue;
                    };
                    let Some(evaluated) = self.evaluate_type(
                        &field.ty,
                        Some(current_application.clone()),
                        environment,
                    ) else {
                        continue;
                    };
                    let (_, container) = terminal_and_container(&evaluated.closed, None);
                    let semantics = container.semantics();
                    let pretty_break_opportunity = attachment.is_some()
                        || semantics.is_some_and(|semantics| semantics.cardinality.is_repeated());
                    if (pretty.group.is_some() || pretty.indent) && !pretty_break_opportunity {
                        self.push_diagnostic(
                            field.ty.source,
                            Some(field.range.clone()),
                            MaterializationDiagnosticKind::InapplicablePrettyHint,
                            "Pretty field grouping or indentation has no materialized break opportunity"
                                .to_owned(),
                        );
                    }
                    if semantics.is_none() {
                        self.push_diagnostic(
                            field.ty.source,
                            Some(field.range.clone()),
                            MaterializationDiagnosticKind::InvalidContainer,
                            format!(
                                "unsupported container shape `{container}`; expected T, Option<T>, Vec<T>, Vec1<T>, or Option<Vec1<T>> after ignoring Box"
                            ),
                        );
                    } else if separator.as_ref().is_some_and(|_| {
                        semantics.is_some_and(|semantics| !semantics.cardinality.is_repeated())
                    }) {
                        self.push_diagnostic(
                            field.ty.source,
                            Some(field.range.clone()),
                            MaterializationDiagnosticKind::InvalidContainer,
                            "#[sep] requires a normalized Vec<T>, Vec1<T>, or Option<Vec1<T>> field"
                                .to_owned(),
                        );
                    } else if let (Some(separator), Some(semantics)) = (separator, semantics)
                        && separator.declared_repetition.is_some_and(|declared| {
                            !matches!(
                                (declared, semantics.cardinality),
                                (
                                    DeclaredRepetition::ZeroOrMore,
                                    crate::ir::ContainerCardinality::ZeroOrMore,
                                ) | (
                                    DeclaredRepetition::OneOrMore,
                                    crate::ir::ContainerCardinality::OneOrMore
                                        | crate::ir::ContainerCardinality::OptionalOneOrMore,
                                )
                            )
                        })
                    {
                        self.push_diagnostic(
                            field.ty.source,
                            Some(field.range.clone()),
                            MaterializationDiagnosticKind::InvalidContainer,
                            "separator repetition metadata disagrees with the normalized container"
                                .to_owned(),
                        );
                    }
                    materialized.push(MaterializedElement::SemanticField {
                        field: field.id,
                        projection: MaterializedSemanticFieldProjection::Direct {
                            ty: evaluated.closed,
                            trace: MaterializationTrace {
                                steps: evaluated.trace,
                            },
                            separator: separator.clone(),
                            pratt_extensions: pratt_extensions.clone(),
                            attachment_before: *attachment_before,
                            attachment_after: *attachment_after,
                        },
                        attachment,
                        pretty: pretty.clone(),
                    });
                }
            }
        }
        materialized
    }

    /// Derives the resolved field while materializing grammar applications and container shapes.
    fn resolved_field(&self, id: ResolvedFieldId) -> &crate::resolution::ResolvedNodeField {
        self.resolved()
            .nodes
            .iter()
            .flat_map(|node| &node.fields)
            .find(|field| field.id == id)
            .expect("source-model field identity belongs to resolved grammar")
    }

    /// Closes a resolved semantic type under the current generic substitution.
    fn evaluate_type(
        &mut self,
        ty: &ResolvedType,
        current_application: Option<MaterializedNodeUse>,
        environment: &EvaluationEnvironment,
    ) -> Option<EvaluatedType> {
        // Classify containers before resolving their terminal target so unsupported shapes fail at the authored boundary.
        match &ty.kind {
            ResolvedTypeKind::Group(inner) | ResolvedTypeKind::Parenthesized(inner) => {
                self.evaluate_type(inner, current_application, environment)
            }
            ResolvedTypeKind::Semantic {
                use_site,
                target,
                trace,
                arguments,
            } => {
                let authored = self.resolved().uses[use_site.index()].authored.clone();
                let prefix = vec![
                    MaterializationTraceStep::Use(*use_site),
                    MaterializationTraceStep::Resolution(*trace),
                ];
                match *target {
                    SemanticTarget::GenericParameter(parameter) => environment
                        .types
                        .get(&parameter)
                        .cloned()
                        .map(|mut value| {
                            let mut steps = prefix;
                            steps.push(MaterializationTraceStep::Parameter(parameter));
                            steps.extend(value.trace);
                            value.trace = steps;
                            value
                        })
                        .or_else(|| {
                            self.push_diagnostic(
                                ty.source,
                                Some(ty.range.clone()),
                                MaterializationDiagnosticKind::OpenApplication,
                                format!("generic parameter `{authored}` remains open"),
                            );
                            None
                        }),
                    SemanticTarget::ContentToken(token) => {
                        if !matches!(
                            arguments.as_slice(),
                            [] | [ResolvedGenericArgument::Lifetime(_)]
                        ) {
                            self.push_invalid_arguments(ty, &authored);
                            return None;
                        }
                        let mut trace = prefix;
                        trace.push(MaterializationTraceStep::ContentToken(token));
                        Some(EvaluatedType {
                            closed: ClosedType::ContentToken(token),
                            trace,
                        })
                    }
                    SemanticTarget::Catalog(catalog) => {
                        let [ResolvedGenericArgument::Type(argument)] = arguments.as_slice() else {
                            self.push_invalid_arguments(ty, &authored);
                            return None;
                        };
                        let argument =
                            self.evaluate_type(argument, current_application, environment)?;
                        Some(wrap_catalog(catalog, prefix, argument))
                    }
                    SemanticTarget::Node(node) => {
                        if authored == "Self" {
                            if !arguments.is_empty() {
                                self.push_invalid_arguments(ty, "Self");
                                return None;
                            }
                            let Some(current_application) = current_application else {
                                self.push_diagnostic(
                                    ty.source,
                                    Some(ty.range.clone()),
                                    MaterializationDiagnosticKind::UnsupportedType,
                                    "`Self` has no current Node application in this context"
                                        .to_owned(),
                                );
                                return None;
                            };
                            let mut trace = prefix;
                            trace.push(MaterializationTraceStep::SelfApplication(
                                current_application.application,
                            ));
                            trace.push(MaterializationTraceStep::Application(
                                current_application.application,
                            ));
                            return Some(EvaluatedType {
                                closed: ClosedType::Node(current_application),
                                trace,
                            });
                        }
                        let (arguments, lifetimes) = self.bind_node_arguments(
                            node,
                            arguments,
                            ty,
                            current_application,
                            environment,
                        )?;
                        let argument_trace = arguments
                            .iter()
                            .flat_map(|argument| argument.trace.iter().copied())
                            .collect::<Vec<_>>();
                        let application = self.ensure_application(node, arguments)?;
                        let mut trace = prefix;
                        trace.extend(argument_trace);
                        trace.push(MaterializationTraceStep::Application(application));
                        Some(EvaluatedType {
                            closed: ClosedType::Node(MaterializedNodeUse {
                                application,
                                lifetimes,
                            }),
                            trace,
                        })
                    }
                    SemanticTarget::TypeAlias(alias) => self.evaluate_alias(
                        alias,
                        arguments,
                        ty,
                        current_application,
                        environment,
                        prefix,
                    ),
                }
            }
            ResolvedTypeKind::FixedTokenPresence
            | ResolvedTypeKind::Reference(_)
            | ResolvedTypeKind::Tuple(_)
            | ResolvedTypeKind::Unsupported => {
                self.push_diagnostic(
                    ty.source,
                    Some(ty.range.clone()),
                    MaterializationDiagnosticKind::UnsupportedType,
                    "only resolved paths and supported containers are grammar-semantic types"
                        .to_owned(),
                );
                None
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Derives the bind node arguments while materializing grammar applications and container shapes.
    fn bind_node_arguments(
        &mut self,
        node: NodeId,
        supplied: &[ResolvedGenericArgument],
        use_type: &ResolvedType,
        current_application: Option<MaterializedNodeUse>,
        environment: &EvaluationEnvironment,
    ) -> Option<(Vec<EvaluatedType>, Vec<syn::Lifetime>)> {
        let declaration = self.resolved().nodes[node.index()].clone();
        let mut supplied = supplied.iter().peekable();
        let mut types = Vec::new();
        let mut lifetimes = Vec::new();
        for parameter in &declaration.generics.params {
            match parameter {
                GenericParam::Lifetime(_) => {
                    if let Some(ResolvedGenericArgument::Lifetime(lifetime)) = supplied.peek() {
                        lifetimes.push(
                            environment
                                .lifetimes
                                .get(&lifetime.ident.to_string())
                                .cloned()
                                .unwrap_or_else(|| lifetime.clone()),
                        );
                        supplied.next();
                    }
                }
                GenericParam::Type(parameter) => {
                    let argument = match supplied.next() {
                        Some(ResolvedGenericArgument::Type(argument)) => argument,
                        Some(_) => {
                            self.push_invalid_arguments(use_type, &declaration.ident);
                            return None;
                        }
                        None => {
                            self.push_diagnostic(
                                use_type.source,
                                Some(use_type.range.clone()),
                                MaterializationDiagnosticKind::OpenApplication,
                                format!(
                                    "Node `{}` leaves type parameter `{}` open",
                                    declaration.ident, parameter.ident
                                ),
                            );
                            return None;
                        }
                    };
                    types.push(self.evaluate_type(
                        argument,
                        current_application.clone(),
                        environment,
                    )?);
                }
                GenericParam::Const(_) => {
                    self.push_node_diagnostic(
                        &declaration,
                        MaterializationDiagnosticKind::ConstGeneric,
                        "const-generic Nodes are outside the closed grammar type language"
                            .to_owned(),
                    );
                    return None;
                }
            }
        }
        if supplied.next().is_some() {
            self.push_invalid_arguments(use_type, &declaration.ident);
            return None;
        }
        Some((types, lifetimes))
    }

    #[allow(clippy::too_many_arguments)]
    /// Expands a type alias while detecting recursion and unsupported open arguments.
    fn evaluate_alias(
        &mut self,
        alias: TypeAliasId,
        supplied: &[ResolvedGenericArgument],
        use_type: &ResolvedType,
        current_application: Option<MaterializedNodeUse>,
        caller_environment: &EvaluationEnvironment,
        prefix: Vec<MaterializationTraceStep>,
    ) -> Option<EvaluatedType> {
        // Track the alias stack explicitly to distinguish a legal recursive Node application from an alias cycle.
        if self.active_aliases.contains(&alias) {
            let declaration = &self.resolved().type_aliases[alias.index()];
            self.push_diagnostic(
                use_type.source,
                Some(use_type.range.clone()),
                MaterializationDiagnosticKind::AliasCycle,
                format!("type alias cycle reaches `{}`", declaration.ident),
            );
            return None;
        }
        let declaration = self.resolved().type_aliases[alias.index()].clone();
        if declaration
            .generics
            .params
            .iter()
            .any(|parameter| matches!(parameter, GenericParam::Const(_)))
        {
            self.push_diagnostic(
                declaration.source,
                Some(declaration.range.clone()),
                MaterializationDiagnosticKind::ConstGeneric,
                format!(
                    "const-generic type alias `{}` is outside the closed grammar type language",
                    declaration.ident
                ),
            );
            return None;
        }
        let parameter_ids = self
            .resolved()
            .generic_parameters
            .iter()
            .filter(|parameter| parameter.owner == SemanticOwner::TypeAlias(alias))
            .map(|parameter| (parameter.ident.clone(), parameter.id))
            .collect::<BTreeMap<_, _>>();
        let mut supplied = supplied.iter().peekable();
        let mut alias_environment = EvaluationEnvironment::default();
        let mut alias_is_active = false;
        for parameter in &declaration.generics.params {
            match parameter {
                GenericParam::Lifetime(parameter) => {
                    let value = if let Some(ResolvedGenericArgument::Lifetime(lifetime)) =
                        supplied.peek()
                    {
                        let value = caller_environment
                            .lifetimes
                            .get(&lifetime.ident.to_string())
                            .cloned()
                            .unwrap_or_else(|| lifetime.clone());
                        supplied.next();
                        value
                    } else {
                        parameter.lifetime.clone()
                    };
                    alias_environment
                        .lifetimes
                        .insert(parameter.lifetime.ident.to_string(), value);
                }
                GenericParam::Type(parameter) => {
                    let value = match supplied.peek() {
                        Some(ResolvedGenericArgument::Type(_)) => {
                            let Some(ResolvedGenericArgument::Type(argument)) = supplied.next()
                            else {
                                unreachable!("peeked type argument")
                            };
                            self.evaluate_type(
                                argument,
                                current_application.clone(),
                                caller_environment,
                            )?
                        }
                        Some(_) => {
                            if alias_is_active {
                                self.active_aliases.pop();
                            }
                            self.push_invalid_arguments(use_type, &declaration.ident);
                            return None;
                        }
                        None => {
                            let parameter_id = parameter_ids[&parameter.ident.to_string()];
                            let Some(default) = declaration
                                .defaults
                                .iter()
                                .find(|default| default.parameter == parameter_id)
                            else {
                                if alias_is_active {
                                    self.active_aliases.pop();
                                }
                                self.push_diagnostic(
                                    use_type.source,
                                    Some(use_type.range.clone()),
                                    MaterializationDiagnosticKind::OpenApplication,
                                    format!(
                                        "type alias `{}` leaves parameter `{}` open",
                                        declaration.ident, parameter.ident
                                    ),
                                );
                                return None;
                            };
                            if !alias_is_active {
                                self.active_aliases.push(alias);
                                alias_is_active = true;
                            }
                            let evaluated = self.evaluate_type(
                                &default.ty,
                                current_application.clone(),
                                &alias_environment,
                            );
                            let Some(evaluated) = evaluated else {
                                self.active_aliases.pop();
                                return None;
                            };
                            evaluated
                        }
                    };
                    alias_environment
                        .types
                        .insert(parameter_ids[&parameter.ident.to_string()], value);
                }
                GenericParam::Const(_) => unreachable!("rejected above"),
            }
        }
        if supplied.next().is_some() {
            if alias_is_active {
                self.active_aliases.pop();
            }
            self.push_invalid_arguments(use_type, &declaration.ident);
            return None;
        }
        if !alias_is_active {
            self.active_aliases.push(alias);
        }
        let target = declaration
            .target
            .as_ref()
            .expect("reachable alias has a resolved target");
        let evaluated = self.evaluate_type(target, current_application, &alias_environment);
        let popped = self.active_aliases.pop();
        debug_assert_eq!(popped, Some(alias));
        evaluated.map(|mut evaluated| {
            let mut steps = prefix;
            steps.push(MaterializationTraceStep::Alias(alias));
            steps.extend(evaluated.trace);
            evaluated.trace = steps;
            evaluated
        })
    }

    /// Projects lexical kinds and parser targets before any generated application types.
    fn project_normalized_terminals(&self) -> NormalizedTerminalProjection {
        let mut grammar = GrammarDescription::new(
            self.resolved()
                .build_inputs
                .module_anchor
                .segments
                .join("::"),
            self.lexical_plan().token_kinds.len(),
            self.resolved().build_inputs.max_lookahead,
            self.resolved().build_inputs.text_equivalence,
        );
        grammar.source_count = self
            .resolved()
            .tree
            .files
            .len()
            .try_into()
            .expect("source count overflow");
        let mut kind_atoms = Vec::with_capacity(self.lexical_plan().token_kinds.len());
        let mut fixed_types = BTreeMap::new();
        let mut fixed_kinds_by_spelling = BTreeMap::new();
        for kind_plan in &self.lexical_plan().token_kinds {
            let source_definition = self
                .lexical_plan()
                .lowering_map
                .token_definitions
                .iter()
                .find(|correlation| correlation.lowered == kind_plan.id)
                .map(|correlation| {
                    &self.source_model().token_definitions[correlation.source.index()]
                })
                .expect("every lexical kind has source evidence");
            let site = &source_definition.definition_sites[0];
            let location = grammar.push_location(Location::new(site.source, site.range.clone()));
            let name = kind_label(&kind_plan.identity);
            let declaration = grammar.push_declaration(Declaration::new(&name, location));
            let kind = grammar.push_kind(
                KindDescription::new(&name, declaration)
                    .with_authored(source_definition.name.clone()),
            );
            assert_eq!(
                kind, kind_plan.id,
                "semantic analysis KindId must equal the lexical KindId"
            );
            let atom = grammar.push_atom(AtomDescription::new(
                &name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            kind_atoms.push(atom);
            if matches!(
                kind_plan.identity,
                crate::lexical::TokenKindIdentity::Fixed(_)
            ) {
                let TokenDefinitionKind::Fixed(fixed) = &source_definition.kind else {
                    unreachable!("a fixed lexical kind has a fixed source definition")
                };
                fixed_kinds_by_spelling.insert(fixed.spelling.as_str(), kind);
                let ty = grammar.push_type(TypeDescription::new(
                    &name,
                    declaration,
                    TypeShape::Token {
                        atom,
                        text_predicate: None,
                    },
                ));
                fixed_types.insert(kind, ty);
            }
        }
        for (open, close) in [("(", ")"), ("[", "]"), ("{", "}")] {
            if let (Some(open), Some(close)) = (
                fixed_kinds_by_spelling.get(open),
                fixed_kinds_by_spelling.get(close),
            ) {
                grammar.push_balanced_delimiter(*open, *close);
            }
        }
        let mut content_types = BTreeMap::new();
        for target in &self.lexical_plan().parser_targets {
            let source_definition =
                &self.source_model().token_definitions[target.definition.index()];
            let site = &source_definition.definition_sites[0];
            let location = grammar.push_location(Location::new(site.source, site.range.clone()));
            let declaration = grammar.push_declaration(Declaration::new(&target.name, location));
            let atom = grammar.push_atom(AtomDescription::new(
                &target.name,
                declaration,
                AtomExpression::Target {
                    base: kind_atoms[target.base_kind.index()],
                    admits: target
                        .admitted_kinds
                        .iter()
                        .map(|kind| kind_atoms[kind.index()])
                        .collect(),
                },
            ));
            let ty = grammar.push_type(TypeDescription::new(
                &target.name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ));
            content_types.insert(target.semantic, ty);
        }
        let terminal_type_count = grammar.types.len();
        NormalizedTerminalProjection {
            grammar,
            fixed_types,
            content_types,
            terminal_type_count,
        }
    }

    /// Reserves dense identities for public applications and private projection sequences.
    fn reserve_normalized_projection_types(
        &self,
        terminal_type_count: usize,
    ) -> NormalizedProjectionReservations {
        let presences = self.fixed_presence_projections(terminal_type_count);
        let presence_types = presences
            .iter()
            .map(|unit| {
                let has_required_envelope = unit.attachment.as_ref().is_some_and(|attachment| {
                    !attachment.before.is_empty() || !attachment.after.is_empty()
                });
                (
                    (unit.application, unit.field),
                    if has_required_envelope {
                        (unit.envelope, false)
                    } else {
                        (unit.sequence, true)
                    },
                )
            })
            .collect();
        let optional_fixed = self.optional_fixed_group_projections(
            terminal_type_count + self.applications.len() + presences.len() * 2,
        );
        let optional_fixed_types = optional_fixed
            .iter()
            .map(|unit| ((unit.application, unit.key), unit.sequence))
            .collect();
        let attachments = self.attachment_unit_projections(
            terminal_type_count
                + self.applications.len()
                + presences.len() * 2
                + optional_fixed.len(),
        );
        let attachment_types = attachments
            .iter()
            .map(|unit| ((unit.application, unit.field), unit.id))
            .collect();
        let applications = self
            .applications
            .iter()
            .map(|application| ApplicationTypeCorrelation {
                application: application.id,
                node: application.node,
                lowered: TypeId::new(
                    (terminal_type_count + application.id.index())
                        .try_into()
                        .expect("normalized type ID overflow"),
                ),
            })
            .collect();
        NormalizedProjectionReservations {
            presences,
            optional_fixed,
            attachments,
            presence_types,
            optional_fixed_types,
            attachment_types,
            applications,
        }
    }

    /// Projects authored application bodies after every target identity has been reserved.
    ///
    /// # Long-function contract
    ///
    /// Rationale: struct, enum, and Pratt shapes share one dense application namespace and one
    /// field-correlation stream. Invariants: each application is published at its reserved type
    /// identity, and every variant/field correlation is recorded while its parse site is created.
    /// Phases: reserves application declarations, projects each structural/enum/Pratt body, then
    /// publishes the completed application type at its preallocated identity.
    fn project_normalized_application_bodies(
        &self,
        grammar: &mut GrammarDescription,
        terminal_type_count: usize,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        content_types: &BTreeMap<ContentTokenId, TypeId>,
        reservations: &NormalizedProjectionReservations,
    ) -> AuthoredBodyProjection {
        let mut fields = Vec::new();
        let mut variants = Vec::new();
        let mut declarations = Vec::new();
        for application in &self.applications {
            let node = &self.resolved().nodes[application.node.index()];
            let location = grammar.push_location(Location::new(node.source, node.range.clone()));
            declarations.push(
                grammar
                    .push_declaration(Declaration::new(application.display_name.clone(), location)),
            );
        }
        for application in &self.applications {
            let owner = TypeId::new(
                (terminal_type_count + application.id.index())
                    .try_into()
                    .expect("normalized type ID overflow"),
            );
            let shape = match &application.body {
                MaterializedNodeBody::Struct(elements)
                | MaterializedNodeBody::Presence(elements) => TypeShape::Sequence {
                    sites: self.project_elements(
                        grammar,
                        application,
                        owner,
                        elements,
                        None,
                        terminal_type_count,
                        fixed_types,
                        content_types,
                        &reservations.presence_types,
                        &reservations.optional_fixed_types,
                        &reservations.attachment_types,
                        &mut fields,
                        0,
                    ),
                },
                MaterializedNodeBody::Enum(materialized_variants) => TypeShape::Enum {
                    alternatives: materialized_variants
                        .iter()
                        .map(|variant| {
                            let source_variant = self
                                .source_model()
                                .nodes
                                .iter()
                                .find(|node| node.resolved_node == application.node)
                                .expect("materialized Node belongs to source model")
                                .variants()
                                .iter()
                                .find(|source_variant| {
                                    source_variant.resolved_variant == variant.resolved_variant
                                })
                                .expect("typed variant identity belongs to materialized Node");
                            let location = grammar.push_location(Location::new(
                                source_variant.site.source,
                                source_variant.site.range.clone(),
                            ));
                            let declaration = grammar.push_declaration(Declaration::new(
                                format!("{}::{}", application.display_name, variant.name),
                                location,
                            ));
                            variants.push(VariantAlternativeCorrelation {
                                application: application.id,
                                variant: variant.resolved_variant,
                                lowered: declaration,
                            });
                            crate::Alternative {
                                declaration,
                                sites: self.project_elements(
                                    grammar,
                                    application,
                                    owner,
                                    &variant.elements,
                                    Some(variant.resolved_variant),
                                    terminal_type_count,
                                    fixed_types,
                                    content_types,
                                    &reservations.presence_types,
                                    &reservations.optional_fixed_types,
                                    &reservations.attachment_types,
                                    &mut fields,
                                    0,
                                ),
                            }
                        })
                        .collect(),
                },
                MaterializedNodeBody::Pratt(materialized_variants) => TypeShape::Pratt {
                    variants: materialized_variants
                        .iter()
                        .map(|variant| {
                            let common = variant.common();
                            let source_variant = self
                                .source_model()
                                .nodes
                                .iter()
                                .find(|node| node.resolved_node == application.node)
                                .expect("materialized Node belongs to source model")
                                .pratt_variants()
                                .iter()
                                .find(|source_variant| {
                                    source_variant.common.resolved_variant
                                        == common.resolved_variant
                                })
                                .expect("typed Pratt variant belongs to materialized Node");
                            let location = grammar.push_location(Location::new(
                                source_variant.common.site.source,
                                source_variant.common.site.range.clone(),
                            ));
                            let declaration = grammar.push_declaration(Declaration::new(
                                format!("{}::{}", application.display_name, common.name),
                                location,
                            ));
                            variants.push(VariantAlternativeCorrelation {
                                application: application.id,
                                variant: common.resolved_variant,
                                lowered: declaration,
                            });
                            match variant {
                                MaterializedPrattVariant::Nud(nud) => {
                                    let (position, min_bp) = match nud.position {
                                        MaterializedPrattNudPosition::Atom => {
                                            (PrattPosition::Atom, 0)
                                        }
                                        MaterializedPrattNudPosition::Prefix { bp } => {
                                            (PrattPosition::Prefix { bp }, bp)
                                        }
                                    };
                                    crate::PrattVariant {
                                        declaration,
                                        position,
                                        sites: self.project_elements(
                                            grammar,
                                            application,
                                            owner,
                                            &nud.body,
                                            Some(common.resolved_variant),
                                            terminal_type_count,
                                            fixed_types,
                                            content_types,
                                            &reservations.presence_types,
                                            &reservations.optional_fixed_types,
                                            &reservations.attachment_types,
                                            &mut fields,
                                            min_bp,
                                        ),
                                    }
                                }
                                MaterializedPrattVariant::Led(led) => {
                                    fields.push(MaterializedFieldCorrelation {
                                        application: application.id,
                                        field: led.lhs.field,
                                        lowered: MaterializedFieldLowering::PrattLhs {
                                            owner,
                                            variant: declaration,
                                        },
                                    });
                                    let (position, min_bp) = match led.position {
                                        MaterializedPrattLedPosition::Infix { lbp, rbp } => {
                                            (PrattPosition::Infix { lbp, rbp }, rbp)
                                        }
                                        MaterializedPrattLedPosition::Postfix { bp } => {
                                            (PrattPosition::Postfix { bp }, bp)
                                        }
                                    };
                                    crate::PrattVariant {
                                        declaration,
                                        position,
                                        sites: self.project_elements(
                                            grammar,
                                            application,
                                            owner,
                                            &led.tail,
                                            Some(common.resolved_variant),
                                            terminal_type_count,
                                            fixed_types,
                                            content_types,
                                            &reservations.presence_types,
                                            &reservations.optional_fixed_types,
                                            &reservations.attachment_types,
                                            &mut fields,
                                            min_bp,
                                        ),
                                    }
                                }
                            }
                        })
                        .collect(),
                },
            };
            assert_eq!(
                grammar.push_type(TypeDescription::new(
                    application.display_name.clone(),
                    declarations[application.id.index()],
                    shape,
                )),
                owner,
            );
            grammar.entry_types.push(owner);
        }
        AuthoredBodyProjection { fields, variants }
    }

    /// Publishes private presence, erased-optional, and attachment sequence types.
    ///
    /// # Long-function contract
    ///
    /// Rationale: private sequences are allocated after public applications but their outer sites
    /// were created with those applications. Invariants: every reserved identity is published in
    /// dense order and correlated back to exactly one already-projected authored field or element.
    /// Phases: publishes presence sequence/envelope pairs, publishes erased optional sequences,
    /// publishes attachment units, then derives their authored-site correlations.
    fn project_normalized_private_sequences(
        &self,
        grammar: &mut GrammarDescription,
        terminal_type_count: usize,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        content_types: &BTreeMap<ContentTokenId, TypeId>,
        reservations: &NormalizedProjectionReservations,
        fields: &[MaterializedFieldCorrelation],
    ) -> PrivateProjectionCorrelations {
        let presences = reservations
            .presences
            .iter()
            .map(|unit| {
                assert_eq!(grammar.types.len(), unit.sequence.index());
                let (sequence_sites, envelope_sites, presence_index) = self.project_fixed_presence(
                    grammar,
                    unit,
                    fixed_types,
                    &reservations.optional_fixed_types,
                );
                let application = &self.applications[unit.application.index()];
                let field = self.resolved_field(unit.field);
                let location =
                    grammar.push_location(Location::new(field.ty.source, field.range.clone()));
                let sequence_declaration = grammar.push_declaration(Declaration::new(
                    format!("{}::__recursa_presence_sequence", application.display_name),
                    location,
                ));
                assert_eq!(
                    grammar.push_type(TypeDescription::new(
                        format!("{}::__recursa_presence_sequence", application.display_name),
                        sequence_declaration,
                        TypeShape::Sequence {
                            sites: sequence_sites.clone(),
                        },
                    )),
                    unit.sequence,
                );
                let envelope_declaration = grammar.push_declaration(Declaration::new(
                    format!("{}::__recursa_presence_envelope", application.display_name),
                    location,
                ));
                assert_eq!(grammar.types.len(), unit.envelope.index());
                assert_eq!(
                    grammar.push_type(TypeDescription::new(
                        format!("{}::__recursa_presence_envelope", application.display_name),
                        envelope_declaration,
                        TypeShape::Sequence {
                            sites: envelope_sites.clone(),
                        },
                    )),
                    unit.envelope,
                );
                let outer = fields
                    .iter()
                    .find_map(|correlation| {
                        (correlation.application == unit.application
                            && correlation.field == unit.field)
                            .then_some(correlation.lowered)
                    })
                    .and_then(|lowering| match lowering {
                        MaterializedFieldLowering::ParseSite(site) => Some(site),
                        MaterializedFieldLowering::PrattLhs { .. } => None,
                    })
                    .expect("a fixed presence has one correlated authored field site");
                FixedPresenceCorrelation {
                    application: unit.application,
                    field: unit.field,
                    outer,
                    outer_is_optional: !unit.attachment.as_ref().is_some_and(|attachment| {
                        !attachment.before.is_empty() || !attachment.after.is_empty()
                    }),
                    envelope: unit.envelope,
                    envelope_sites,
                    presence_index,
                    sequence: unit.sequence,
                    sequence_sites,
                }
            })
            .collect();
        let optional_fixed = reservations
            .optional_fixed
            .iter()
            .filter_map(|unit| {
                assert_eq!(grammar.types.len(), unit.sequence.index());
                let application = &self.applications[unit.application.index()];
                let sequence_sites = self.project_optional_fixed_group(grammar, unit, fixed_types);
                let location =
                    grammar.push_location(Location::new(unit.site.source, unit.site.range.clone()));
                let declaration = grammar.push_declaration(Declaration::new(
                    format!("{}::__recursa_optional_fixed", application.display_name),
                    location,
                ));
                assert_eq!(
                    grammar.push_type(TypeDescription::new(
                        format!("{}::__recursa_optional_fixed", application.display_name),
                        declaration,
                        TypeShape::Sequence {
                            sites: sequence_sites.clone(),
                        },
                    )),
                    unit.sequence,
                );
                let element = unit.element?;
                let outer = grammar
                    .sites
                    .iter()
                    .enumerate()
                    .find_map(|(index, site)| {
                        (site.target == unit.sequence
                            && matches!(site.container, ContainerShape::Optional(_)))
                        .then(|| SiteId::new(index as u32))
                    })
                    .expect("an optional fixed group has one normalized outer site");
                Some(OptionalFixedGroupCorrelation {
                    application: unit.application,
                    element,
                    outer,
                    sequence: unit.sequence,
                    sequence_sites,
                })
            })
            .collect();
        for unit in &reservations.attachments {
            assert_eq!(grammar.types.len(), unit.id.index());
            let sites = self.project_attachment_unit(
                grammar,
                unit,
                terminal_type_count,
                fixed_types,
                content_types,
                &reservations.optional_fixed_types,
            );
            let application = &self.applications[unit.application.index()];
            let field = self.resolved_field(unit.field);
            let location =
                grammar.push_location(Location::new(field.ty.source, field.range.clone()));
            let declaration = grammar.push_declaration(Declaration::new(
                format!("{}::__recursa_attachment", application.display_name),
                location,
            ));
            assert_eq!(
                grammar.push_type(TypeDescription::new(
                    format!("{}::__recursa_attachment", application.display_name),
                    declaration,
                    TypeShape::Sequence { sites },
                )),
                unit.id,
            );
        }
        let attachments = reservations
            .attachments
            .iter()
            .map(|unit| {
                let outer = fields
                    .iter()
                    .find_map(|correlation| {
                        (correlation.application == unit.application
                            && correlation.field == unit.field)
                            .then_some(correlation.lowered)
                    })
                    .and_then(|lowering| match lowering {
                        MaterializedFieldLowering::ParseSite(site) => Some(site),
                        MaterializedFieldLowering::PrattLhs { .. } => None,
                    })
                    .expect("an attachment unit has one correlated authored field site");
                let TypeShape::Sequence { sites } = &grammar.types[unit.id.index()].shape else {
                    unreachable!("attachment units are synthetic sequences")
                };
                AttachmentUnitCorrelation {
                    application: unit.application,
                    field: unit.field,
                    outer,
                    unit: unit.id,
                    sites: sites.clone(),
                    semantic_index: unit.attachment.before.len(),
                }
            })
            .collect();
        PrivateProjectionCorrelations {
            attachments,
            presences,
            optional_fixed,
        }
    }

    /// Builds normalized types, sites, declarations, and correlations from closed applications.
    ///
    /// # Long-function contract
    ///
    /// Rationale: dense normalized identities and their lowering-map correlations must be allocated
    /// and published together so no phase observes a partially backpatched grammar.
    /// Invariants: lexical terminals precede applications and private projection types; every
    /// source identity has one validated normalized correlation before the aggregate is returned.
    /// Phases: projects lexical terminals, reserves application and private projection identities,
    /// projects authored bodies, materializes private presence/optional/attachment types, validates
    /// dense ordering, then publishes the complete lowering map.
    fn project_normalized_grammar(&self) -> (GrammarDescription, LoweringMap) {
        let NormalizedTerminalProjection {
            mut grammar,
            fixed_types,
            content_types,
            terminal_type_count,
        } = self.project_normalized_terminals();
        let reservations = self.reserve_normalized_projection_types(terminal_type_count);
        let AuthoredBodyProjection {
            fields: projected_fields,
            variants: projected_variants,
        } = self.project_normalized_application_bodies(
            &mut grammar,
            terminal_type_count,
            &fixed_types,
            &content_types,
            &reservations,
        );
        let variant_correlations = projected_variants;
        let PrivateProjectionCorrelations {
            attachments: attachment_correlations,
            presences: presence_correlations,
            optional_fixed: optional_fixed_correlations,
        } = self.project_normalized_private_sequences(
            &mut grammar,
            terminal_type_count,
            &fixed_types,
            &content_types,
            &reservations,
            &projected_fields,
        );
        (
            grammar,
            LoweringMap {
                sources: self
                    .resolved()
                    .tree
                    .files
                    .iter()
                    .map(|source| SourceCorrelation {
                        resolved: source.id,
                        lowered: source.id,
                    })
                    .collect(),
                token_definitions: self.lexical_plan().lowering_map.token_definitions.clone(),
                content_targets: content_types
                    .iter()
                    .map(|(semantic, lowered)| ContentTargetTypeCorrelation {
                        semantic: *semantic,
                        lowered: *lowered,
                    })
                    .collect(),
                applications: reservations.applications,
                framing: self.framing,
                variants: variant_correlations,
                fields: projected_fields,
                attachments: attachment_correlations,
                presences: presence_correlations,
                optional_fixed_groups: optional_fixed_correlations,
            },
        )
    }

    /// Derives boolean fixed-token presence projections in authored application order.
    fn fixed_presence_projections(
        &self,
        terminal_type_count: usize,
    ) -> Vec<FixedPresenceProjection> {
        let mut units = Vec::new();
        let mut collect = |application: &MaterializedNodeApplication,
                           elements: &[MaterializedElement]| {
            for element in elements {
                let MaterializedElement::SemanticField {
                    field,
                    attachment,
                    projection: MaterializedSemanticFieldProjection::FixedPresence { tokens },
                    ..
                } = element
                else {
                    continue;
                };
                let base = terminal_type_count + self.applications.len() + units.len() * 2;
                units.push(FixedPresenceProjection {
                    sequence: TypeId::new(
                        base.try_into()
                            .expect("fixed presence sequence type ID overflow"),
                    ),
                    envelope: TypeId::new(
                        (base + 1)
                            .try_into()
                            .expect("fixed presence envelope type ID overflow"),
                    ),
                    application: application.id,
                    field: *field,
                    tokens: tokens.clone(),
                    attachment: attachment.clone(),
                });
            }
        };
        for application in &self.applications {
            for group in application.body.element_groups() {
                collect(application, group.elements);
            }
        }
        units
    }

    /// Derives erased optional fixed-group projections in authored application order.
    fn optional_fixed_group_projections(
        &self,
        synthetic_type_start: usize,
    ) -> Vec<OptionalFixedGroupProjection> {
        let mut units = Vec::new();
        let mut collect = |application: &MaterializedNodeApplication,
                           elements: &[MaterializedElement]| {
            for element in elements {
                let mut push_group = |element, tokens: &Vec<TokenUseId>, site| {
                    units.push(OptionalFixedGroupProjection {
                        sequence: TypeId::new(
                            (synthetic_type_start + units.len())
                                .try_into()
                                .expect("optional fixed-group type ID overflow"),
                        ),
                        application: application.id,
                        element,
                        key: tokens[0],
                        tokens: tokens.clone(),
                        site,
                    });
                };
                match element {
                    MaterializedElement::OptionalFixedTokenGroup {
                        element,
                        tokens,
                        site,
                    } => push_group(Some(*element), tokens, site.clone()),
                    MaterializedElement::SemanticField { attachment, .. } => {
                        if let Some(attachment) = attachment {
                            for syntax in attachment.before.iter().chain(&attachment.after) {
                                if let MaterializedAttachedFixedSyntax::Optional { tokens, site } =
                                    syntax
                                {
                                    push_group(None, tokens, site.clone());
                                }
                            }
                        }
                    }
                    MaterializedElement::FixedToken { .. } => {}
                }
            }
        };
        for application in &self.applications {
            for group in application.body.element_groups() {
                collect(application, group.elements);
            }
        }
        units
    }

    /// Derives the attachment unit projections while materializing grammar applications and container shapes.
    fn attachment_unit_projections(
        &self,
        synthetic_type_start: usize,
    ) -> Vec<AttachmentUnitProjection> {
        let mut units = Vec::new();
        let mut collect = |application: &MaterializedNodeApplication,
                           elements: &[MaterializedElement],
                           recursive_min_bp: u8| {
            for element in elements {
                let MaterializedElement::SemanticField {
                    field,
                    projection: MaterializedSemanticFieldProjection::Direct { ty, .. },
                    attachment: Some(attachment),
                    ..
                } = element
                else {
                    continue;
                };
                let id = TypeId::new(
                    (synthetic_type_start + units.len())
                        .try_into()
                        .expect("semantic analysis attachment type ID overflow"),
                );
                units.push(AttachmentUnitProjection {
                    id,
                    application: application.id,
                    field: *field,
                    ty: ty.clone(),
                    attachment: attachment.clone(),
                    recursive_min_bp,
                });
            }
        };
        for application in &self.applications {
            for group in application.body.element_groups() {
                collect(application, group.elements, group.recursive_min_bp);
            }
        }
        units
    }

    /// Correlates one repeated attachment unit with its normalized semantic sites.
    fn project_attachment_unit(
        &self,
        grammar: &mut GrammarDescription,
        unit: &AttachmentUnitProjection,
        terminal_type_count: usize,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        content_types: &BTreeMap<ContentTokenId, TypeId>,
        optional_fixed_types: &BTreeMap<(MaterializedApplicationId, TokenUseId), TypeId>,
    ) -> Vec<SiteId> {
        let application = &self.applications[unit.application.index()];
        let mut sites = Vec::new();
        for syntax in &unit.attachment.before {
            let index = sites.len();
            sites.push(self.project_attached_fixed_syntax(
                grammar,
                unit.id,
                application,
                syntax,
                Some(unit.field),
                fixed_types,
                optional_fixed_types,
                "attachment",
                index,
            ));
        }
        let field = self.resolved_field(unit.field);
        let location = grammar.push_location(Location::new(field.ty.source, field.range.clone()));
        let declaration = grammar.push_declaration(self.with_greedy(
            application,
            None,
            Some(unit.field),
            crate::AuthoredRole::Field,
            Declaration::new(
                format!(
                    "{}::__recursa_attachment_element{}",
                    application.display_name,
                    sites.len()
                ),
                location,
            ),
        ));
        let (terminal, _) = terminal_and_container(&unit.ty, None);
        let target = match terminal {
            Terminal::Node(application) => TypeId::new(
                (terminal_type_count + application.index())
                    .try_into()
                    .expect("normalized type ID overflow"),
            ),
            Terminal::ContentToken(token) => content_types[&token],
        };
        let mut semantic = ParseSite::new(unit.id, target, declaration, ContainerShape::Direct);
        if is_recursive_pratt_operand(&unit.ty, unit.application) {
            semantic = semantic.with_min_bp(unit.recursive_min_bp);
        }
        sites.push(grammar.push_site(semantic));
        for syntax in &unit.attachment.after {
            let index = sites.len();
            sites.push(self.project_attached_fixed_syntax(
                grammar,
                unit.id,
                application,
                syntax,
                Some(unit.field),
                fixed_types,
                optional_fixed_types,
                "attachment",
                index,
            ));
        }
        sites
    }

    /// Projects one required or erased-optional attachment entry into one normalized site.
    #[allow(clippy::too_many_arguments)]
    fn project_attached_fixed_syntax(
        &self,
        grammar: &mut GrammarDescription,
        owner: TypeId,
        application: &MaterializedNodeApplication,
        syntax: &MaterializedAttachedFixedSyntax,
        field: Option<ResolvedFieldId>,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        optional_fixed_types: &BTreeMap<(MaterializedApplicationId, TokenUseId), TypeId>,
        role: &str,
        index: usize,
    ) -> SiteId {
        let (target, container, site) = match syntax {
            MaterializedAttachedFixedSyntax::Required(token_use) => {
                let token_use = &self.source_model().token_uses[token_use.index()];
                (
                    fixed_types[&self.kind_for_definition(token_use.definition)],
                    ContainerShape::Direct,
                    &token_use.site,
                )
            }
            MaterializedAttachedFixedSyntax::Optional { tokens, site } => (
                optional_fixed_types[&(application.id, tokens[0])],
                ContainerShape::optional(ContainerShape::Direct),
                site,
            ),
        };
        let location = grammar.push_location(Location::new(site.source, site.range.clone()));
        let declaration = grammar.push_declaration(self.with_greedy(
            application,
            None,
            field,
            crate::AuthoredRole::Attachment,
            Declaration::new(
                format!(
                    "{}::__recursa_{role}_element{index}",
                    application.display_name
                ),
                location,
            ),
        ));
        grammar.push_site(ParseSite::new(owner, target, declaration, container))
    }

    /// Projects one authored boolean presence into a fixed-only optional sequence inside a required envelope.
    fn project_fixed_presence(
        &self,
        grammar: &mut GrammarDescription,
        unit: &FixedPresenceProjection,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        optional_fixed_types: &BTreeMap<(MaterializedApplicationId, TokenUseId), TypeId>,
    ) -> (Vec<SiteId>, Vec<SiteId>, usize) {
        let application = &self.applications[unit.application.index()];
        let push_fixed = |grammar: &mut GrammarDescription,
                          owner: TypeId,
                          sites: &mut Vec<SiteId>,
                          token_use: TokenUseId,
                          role: &str| {
            let token_use = &self.source_model().token_uses[token_use.index()];
            let location = grammar.push_location(Location::new(
                token_use.site.source,
                token_use.site.range.clone(),
            ));
            let declaration = grammar.push_declaration(Declaration::new(
                format!(
                    "{}::__recursa_presence_{role}_element{}",
                    application.display_name,
                    sites.len()
                ),
                location,
            ));
            sites.push(grammar.push_site(ParseSite::new(
                owner,
                fixed_types[&self.kind_for_definition(token_use.definition)],
                declaration,
                ContainerShape::Direct,
            )));
        };

        let mut sequence_sites = Vec::new();
        for token in &unit.tokens {
            push_fixed(
                grammar,
                unit.sequence,
                &mut sequence_sites,
                *token,
                "sequence",
            );
        }

        let mut envelope_sites = Vec::new();
        if let Some(attachment) = &unit.attachment {
            for syntax in &attachment.before {
                let index = envelope_sites.len();
                envelope_sites.push(self.project_attached_fixed_syntax(
                    grammar,
                    unit.envelope,
                    application,
                    syntax,
                    Some(unit.field),
                    fixed_types,
                    optional_fixed_types,
                    "presence_envelope",
                    index,
                ));
            }
        }
        let presence_index = envelope_sites.len();
        let field = self.resolved_field(unit.field);
        let location = grammar.push_location(Location::new(field.ty.source, field.range.clone()));
        let declaration = grammar.push_declaration(self.with_greedy(
            application,
            None,
            Some(unit.field),
            crate::AuthoredRole::Presence,
            Declaration::new(
                format!("{}::__recursa_presence_optional", application.display_name),
                location,
            ),
        ));
        envelope_sites.push(grammar.push_site(ParseSite::new(
            unit.envelope,
            unit.sequence,
            declaration,
            ContainerShape::optional(ContainerShape::Direct),
        )));
        if let Some(attachment) = &unit.attachment {
            for syntax in &attachment.after {
                let index = envelope_sites.len();
                envelope_sites.push(self.project_attached_fixed_syntax(
                    grammar,
                    unit.envelope,
                    application,
                    syntax,
                    Some(unit.field),
                    fixed_types,
                    optional_fixed_types,
                    "presence_envelope",
                    index,
                ));
            }
        }
        (sequence_sites, envelope_sites, presence_index)
    }

    /// Projects one erased optional fixed group into its fixed-only sequence.
    fn project_optional_fixed_group(
        &self,
        grammar: &mut GrammarDescription,
        unit: &OptionalFixedGroupProjection,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
    ) -> Vec<SiteId> {
        let application = &self.applications[unit.application.index()];
        unit.tokens
            .iter()
            .enumerate()
            .map(|(index, token_use)| {
                let token_use = &self.source_model().token_uses[token_use.index()];
                let location = grammar.push_location(Location::new(
                    token_use.site.source,
                    token_use.site.range.clone(),
                ));
                let declaration = grammar.push_declaration(Declaration::new(
                    format!(
                        "{}::__recursa_optional_fixed_element{index}",
                        application.display_name,
                    ),
                    location,
                ));
                grammar.push_site(ParseSite::new(
                    unit.sequence,
                    fixed_types[&self.kind_for_definition(token_use.definition)],
                    declaration,
                    ContainerShape::Direct,
                ))
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    /// Lowers materialized elements to normalized parse sites in source order.
    fn project_elements(
        &self,
        grammar: &mut GrammarDescription,
        application: &MaterializedNodeApplication,
        owner: TypeId,
        elements: &[MaterializedElement],
        variant: Option<ResolvedVariantId>,
        terminal_type_count: usize,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        content_types: &BTreeMap<ContentTokenId, TypeId>,
        presence_types: &BTreeMap<(MaterializedApplicationId, ResolvedFieldId), (TypeId, bool)>,
        optional_fixed_types: &BTreeMap<(MaterializedApplicationId, TokenUseId), TypeId>,
        attachment_types: &BTreeMap<(MaterializedApplicationId, ResolvedFieldId), TypeId>,
        field_correlations: &mut Vec<MaterializedFieldCorrelation>,
        recursive_min_bp: u8,
    ) -> Vec<SiteId> {
        elements
            .iter()
            .enumerate()
            .map(|(index, element)| match element {
                MaterializedElement::FixedToken { token_use } => {
                    let token_use = &self.source_model().token_uses[token_use.index()];
                    let location = grammar.push_location(Location::new(
                        token_use.site.source,
                        token_use.site.range.clone(),
                    ));
                    let declaration = grammar.push_declaration(Declaration::new(
                        format!("{}::element{index}", application.display_name),
                        location,
                    ));
                    grammar.push_site(ParseSite::new(
                        owner,
                        fixed_types[&self.kind_for_definition(token_use.definition)],
                        declaration,
                        ContainerShape::Direct,
                    ))
                }
                MaterializedElement::OptionalFixedTokenGroup { tokens, site, .. } => {
                    let location =
                        grammar.push_location(Location::new(site.source, site.range.clone()));
                    let declaration = grammar.push_declaration(self.with_greedy(
                        application,
                        variant,
                        None,
                        crate::AuthoredRole::Attachment,
                        Declaration::new(
                            format!("{}::element{index}", application.display_name),
                            location,
                        ),
                    ));
                    grammar.push_site(ParseSite::new(
                        owner,
                        optional_fixed_types[&(application.id, tokens[0])],
                        declaration,
                        ContainerShape::optional(ContainerShape::Direct),
                    ))
                }
                MaterializedElement::SemanticField {
                    field,
                    projection: MaterializedSemanticFieldProjection::FixedPresence { .. },
                    ..
                } => {
                    let resolved_field = self.resolved_field(*field);
                    let location = grammar.push_location(Location::new(
                        resolved_field.ty.source,
                        resolved_field.range.clone(),
                    ));
                    let declaration = grammar.push_declaration(self.with_greedy(
                        application,
                        variant,
                        Some(*field),
                        crate::AuthoredRole::Presence,
                        Declaration::new(
                            format!("{}::element{index}", application.display_name),
                            location,
                        ),
                    ));
                    let (target, optional) = presence_types[&(application.id, *field)];
                    let site = grammar.push_site(ParseSite::new(
                        owner,
                        target,
                        declaration,
                        if optional {
                            ContainerShape::optional(ContainerShape::Direct)
                        } else {
                            ContainerShape::Direct
                        },
                    ));
                    field_correlations.push(MaterializedFieldCorrelation {
                        application: application.id,
                        field: *field,
                        lowered: MaterializedFieldLowering::ParseSite(site),
                    });
                    site
                }
                MaterializedElement::SemanticField {
                    field,
                    projection: MaterializedSemanticFieldProjection::Direct { ty, separator, .. },
                    attachment,
                    ..
                } => {
                    let resolved_field = self.resolved_field(*field);
                    let location = grammar.push_location(Location::new(
                        resolved_field.ty.source,
                        resolved_field.range.clone(),
                    ));
                    let declaration = grammar.push_declaration(self.with_greedy(
                        application,
                        variant,
                        Some(*field),
                        crate::AuthoredRole::Field,
                        Declaration::new(
                            format!("{}::element{index}", application.display_name),
                            location,
                        ),
                    ));
                    let separator_type = separator.as_ref().map(|separator| {
                        let token_use =
                            &self.source_model().token_uses[separator.token_use.index()];
                        RepeatSeparator::new(
                            fixed_types[&self.kind_for_definition(token_use.definition)],
                            separator.optional_trailing,
                        )
                    });
                    let (terminal, container) = terminal_and_container(ty, separator_type);
                    let semantic_target = match terminal {
                        Terminal::Node(application) => TypeId::new(
                            (terminal_type_count + application.index())
                                .try_into()
                                .expect("normalized type ID overflow"),
                        ),
                        Terminal::ContentToken(token) => content_types[&token],
                    };
                    let target = attachment.as_ref().map_or(semantic_target, |_| {
                        attachment_types[&(application.id, *field)]
                    });
                    let mut parse_site = ParseSite::new(owner, target, declaration, container);
                    if is_recursive_pratt_operand(ty, application.id) {
                        parse_site = parse_site.with_min_bp(recursive_min_bp);
                    }
                    let site = grammar.push_site(parse_site);
                    field_correlations.push(MaterializedFieldCorrelation {
                        application: application.id,
                        field: *field,
                        lowered: MaterializedFieldLowering::ParseSite(site),
                    });
                    site
                }
            })
            .collect()
    }

    /// Derives the display application while materializing grammar applications and container shapes.
    fn display_application(&self, node: &ResolvedNode, arguments: &[EvaluatedType]) -> String {
        let name = node.key.segments.join("::");
        if arguments.is_empty() {
            return name;
        }
        format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(|argument| self.display_type(&argument.closed))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Derives the display type while materializing grammar applications and container shapes.
    fn display_type(&self, ty: &ClosedType) -> String {
        match ty {
            ClosedType::Node(node) => self.applications[node.application.index()]
                .display_name
                .clone(),
            ClosedType::ContentToken(token) => {
                self.resolved().content_tokens[token.index()].ident.clone()
            }
            ClosedType::Boxed(inner) => format!("Box<{}>", self.display_type(inner)),
            ClosedType::Optional(inner) => format!("Option<{}>", self.display_type(inner)),
            ClosedType::ZeroOrMore(inner) => format!("Vec<{}>", self.display_type(inner)),
            ClosedType::OneOrMore(inner) => format!("Vec1<{}>", self.display_type(inner)),
        }
    }

    /// Attaches a materialization diagnostic to an authored Node declaration.
    fn push_node_diagnostic(
        &mut self,
        node: &ResolvedNode,
        kind: MaterializationDiagnosticKind,
        message: String,
    ) {
        self.push_diagnostic_in_module(
            Some(node.module),
            node.source,
            Some(node.range.clone()),
            kind,
            message,
        );
    }

    /// Diagnoses semantic type arguments that violate the target's accepted shape.
    fn push_invalid_arguments(&mut self, ty: &ResolvedType, name: &str) {
        self.push_diagnostic(
            ty.source,
            Some(ty.range.clone()),
            MaterializationDiagnosticKind::InvalidArguments,
            format!("invalid semantic arguments for `{name}`"),
        );
    }

    /// Adds a diagnostic at the active application and authored source range.
    fn push_diagnostic(
        &mut self,
        source: crate::SourceId,
        range: Option<Range<u32>>,
        kind: MaterializationDiagnosticKind,
        message: String,
    ) {
        let module = self
            .active_applications
            .last()
            .map(|application| self.resolved().nodes[application.node.index()].module);
        self.push_diagnostic_in_module(module, source, range, kind, message);
    }

    /// Adds a diagnostic with both physical and logical module provenance.
    fn push_diagnostic_in_module(
        &mut self,
        module: Option<crate::lowering::ModuleId>,
        source: crate::SourceId,
        range: Option<Range<u32>>,
        kind: MaterializationDiagnosticKind,
        message: String,
    ) {
        self.diagnostics.push(MaterializationDiagnostic {
            kind,
            module,
            logical_module: module.map(|module| {
                self.resolved().tree.inclusions[module.index()]
                    .logical_path
                    .clone()
            }),
            path: self.resolved().tree.files[source.index()].path.clone(),
            range,
            message,
        });
    }
}

/// Applies a supported container constructor around a closed semantic type.
fn wrap_catalog(
    catalog: CatalogType,
    mut prefix: Vec<MaterializationTraceStep>,
    argument: EvaluatedType,
) -> EvaluatedType {
    prefix.push(MaterializationTraceStep::Catalog(catalog));
    prefix.extend(argument.trace);
    match catalog {
        CatalogType::Box => EvaluatedType {
            closed: ClosedType::Boxed(Box::new(argument.closed)),
            trace: prefix,
        },
        CatalogType::Option => EvaluatedType {
            closed: ClosedType::Optional(Box::new(argument.closed)),
            trace: prefix,
        },
        CatalogType::Vec => EvaluatedType {
            closed: ClosedType::ZeroOrMore(Box::new(argument.closed)),
            trace: prefix,
        },
        CatalogType::Vec1 => EvaluatedType {
            closed: ClosedType::OneOrMore(Box::new(argument.closed)),
            trace: prefix,
        },
    }
}

#[derive(Clone, Copy)]
/// Classifies the terminal alternatives supported during materialization.
enum Terminal {
    /// Selects the node materialization alternative.
    Node(MaterializedApplicationId),
    /// Selects the content token materialization alternative.
    ContentToken(ContentTokenId),
}

/// Derives the exact pratt self operand while materializing grammar applications and container shapes.
fn exact_pratt_self_operand(
    element: &MaterializedElement,
    current_application: &MaterializedNodeUse,
) -> bool {
    let MaterializedElement::SemanticField {
        projection:
            MaterializedSemanticFieldProjection::Direct {
                ty,
                trace,
                separator,
                ..
            },
        ..
    } = element
    else {
        return false;
    };
    exact_pratt_self_parts(ty, trace, separator.as_ref(), current_application)
}

/// Derives the exact pratt self parts while materializing grammar applications and container shapes.
fn exact_pratt_self_parts(
    ty: &ClosedType,
    trace: &MaterializationTrace,
    separator: Option<&Separator>,
    current_application: &MaterializedNodeUse,
) -> bool {
    let ClosedType::Boxed(inner) = ty else {
        return false;
    };
    let ClosedType::Node(owner) = inner.as_ref() else {
        return false;
    };
    separator.is_none()
        && owner == current_application
        && trace
            .steps
            .contains(&MaterializationTraceStep::SelfApplication(
                current_application.application,
            ))
}

/// Reports whether this value is recursive pratt operand.
fn is_recursive_pratt_operand(ty: &ClosedType, owner: MaterializedApplicationId) -> bool {
    match ty {
        ClosedType::Node(node) => node.application == owner,
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => is_recursive_pratt_operand(inner, owner),
        ClosedType::ContentToken(_) => false,
    }
}

/// Derives the attachment terminal while materializing grammar applications and container shapes.
fn attachment_terminal(ty: &ClosedType) -> &ClosedType {
    match ty {
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => attachment_terminal(inner),
        ClosedType::Node(_) | ClosedType::ContentToken(_) => ty,
    }
}

/// Derives the terminal and container while materializing grammar applications and container shapes.
fn terminal_and_container(
    ty: &ClosedType,
    separator: Option<RepeatSeparator>,
) -> (Terminal, ContainerShape) {
    match ty {
        ClosedType::Node(node) => (Terminal::Node(node.application), ContainerShape::Direct),
        ClosedType::ContentToken(token) => (Terminal::ContentToken(*token), ContainerShape::Direct),
        ClosedType::Boxed(inner) => {
            let (terminal, shape) = terminal_and_container(inner, separator);
            (terminal, ContainerShape::boxed(shape))
        }
        ClosedType::Optional(inner) => {
            let (terminal, shape) = terminal_and_container(inner, separator);
            (terminal, ContainerShape::optional(shape))
        }
        ClosedType::ZeroOrMore(inner) => {
            let (terminal, shape) = terminal_and_container(inner, None);
            (
                terminal,
                ContainerShape::repeated_with_separator(
                    shape,
                    crate::RepetitionCardinality::ZeroOrMore,
                    separator,
                ),
            )
        }
        ClosedType::OneOrMore(inner) => {
            let (terminal, shape) = terminal_and_container(inner, None);
            (
                terminal,
                ContainerShape::repeated_with_separator(
                    shape,
                    crate::RepetitionCardinality::OneOrMore,
                    separator,
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        ClosedType, MaterializationDiagnosticKind, MaterializationTraceStep, MaterializedElement,
        MaterializedFieldLowering, MaterializedFieldView, MaterializedNodeApplication,
        MaterializedNodeBody, MaterializedPrattLed, MaterializedPrattLedPosition,
        MaterializedPrattNud, MaterializedPrattNudPosition, MaterializedPrattVariant,
        MaterializedSemanticFieldProjection, PrettyBoundarySummary, materialize,
    };
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::{RustEdition, TokenDefinitionId};
    use crate::resolution::{ResolutionInput, resolve_local_semantics};
    use crate::source_model::{MaterializedPrettyBoundary, SourceModelInput, lower_source_model};
    use crate::source_syntax::CargoCfgSnapshot;
    use crate::{AtomExpression, ContainerShape, KindSet, TypeShape};

    fn resolve_report(source: &str) -> crate::resolution::ResolutionReport {
        let package = TempDir::new().unwrap();
        let path = package.path().join("grammar/root.rs");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        let cfg = CargoCfgSnapshot::default();
        let discovery = discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new("grammar/root.rs"),
            edition: RustEdition::Rust2024,
            cfg: &cfg,
        });
        assert!(discovery.diagnostics.is_empty());
        resolve_local_semantics(ResolutionInput {
            discovery_inputs: discovery.discovery_inputs,
            tree: discovery.tree.unwrap(),
            cfg: &cfg,
        })
    }

    fn resolve(source: &str) -> crate::source_model::SourceModeledGrammar {
        let resolution = resolve_report(source);
        assert!(
            resolution.diagnostics.is_empty(),
            "{:?}",
            resolution.diagnostics
        );
        let resolved = resolution.resolved.unwrap();
        let source_model = lower_source_model(SourceModelInput {
            resolved: resolved.clone(),
            cfg: &CargoCfgSnapshot::default(),
        });
        assert!(
            source_model.diagnostics.is_empty(),
            "{:?}",
            source_model.diagnostics
        );
        source_model.source_modeled.unwrap()
    }

    fn target_kind_set(grammar: &crate::GrammarDescription, target: crate::TypeId) -> KindSet {
        let TypeShape::Token { atom, .. } = grammar.types[target.index()].shape else {
            panic!("content target must lower to a token type")
        };
        let AtomExpression::Target { base, ref admits } = grammar.atoms[atom.index()].expression
        else {
            panic!("content target must lower to a target atom")
        };
        let members = std::iter::once(base).chain(admits.iter().copied());
        KindSet::from_kinds(
            grammar.kind_count,
            members.map(|member| match grammar.atoms[member.index()].expression {
                AtomExpression::Kind(kind) => kind,
                _ => panic!("target members must be direct kind atoms"),
            }),
        )
        .unwrap()
    }

    #[test]
    fn closed_generic_pretty_boundaries_recheck_concrete_lexical_languages() {
        let unsafe_source = r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { LBRACE => "{" } }

#[derive(recursa::Node)]
struct Name<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    text: NameText<'input>,
}

#[derive(recursa::Node)]
struct Merged<'input> {
    #[lex(pattern = r"\{[A-Za-z]+")]
    text: MergedText<'input>,
}

#[derive(recursa::Node)]
struct Attached<T> {
    #[tok(LBRACE, this)]
    value: T,
}

#[derive(recursa::Node)]
struct Root<'input> {
    value: Attached<Name<'input>>,
}
"#;
        let unsafe_report = materialize(resolve(unsafe_source));
        assert!(unsafe_report.lowered.is_none());
        let diagnostic = unsafe_report
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.kind == MaterializationDiagnosticKind::UnsafePrettyBoundary
                    && diagnostic.message.contains("closed Pretty boundary")
            })
            .expect("the unsafe concrete attachment boundary is diagnosed");
        assert_eq!(
            crate::source_analysis::materialization_code(diagnostic.kind),
            "RCA3211"
        );
        assert!(!unsafe_report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == MaterializationDiagnosticKind::InapplicablePrettyHint
                && diagnostic.message.contains("closed Pretty boundary")
        }));
        let range = diagnostic
            .range
            .clone()
            .expect("boundary has a source range");
        assert!(
            unsafe_source[range.start as usize..range.end as usize].contains('T'),
            "diagnostic must point at the authored boundary field: {diagnostic:?}"
        );

        let safe_report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { LBRACE => "{" } }

#[derive(recursa::Node)]
struct Name<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    text: NameText<'input>,
}

#[derive(recursa::Node)]
struct Attached<T> {
    #[tok(LBRACE, this)]
    value: T,
}

#[derive(recursa::Node)]
struct Root<'input> {
    value: Attached<Name<'input>>,
}
"#,
        ));
        assert!(
            safe_report.diagnostics.is_empty(),
            "{:?}",
            safe_report.diagnostics
        );
        assert!(safe_report.lowered.is_some());

        let fixed_report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight, DOUBLE_COLON => "::" } }

#[derive(recursa::Node)] #[tok(COLON)] struct Colon;
#[derive(recursa::Node)]
struct Attached<T> {
    #[tok(COLON, this)]
    value: T,
}
#[derive(recursa::Node)] struct Root { value: Attached<Colon> }
"#,
        ));
        assert!(fixed_report.lowered.is_none());
        assert!(fixed_report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == MaterializationDiagnosticKind::UnsafePrettyBoundary
                && diagnostic.message.contains("closed Pretty boundary")
        }));

        let mixed_case_report = materialize(resolve(
            r#"
recursa::grammar! {
    module = crate,
    derives(Pretty),
    keyword_matching = ascii_insensitive,
}
recursa::tokens! {
    keywords { WORD => "word" }
    punctuation { COLON => ":" }
    lexer_tokens { MIXED_MERGED => r"WoRd:" }
}
#[derive(recursa::Node)] #[tok(COLON)] struct Colon;
#[derive(recursa::Node)]
struct Attached<T> { #[tok(WORD, this)] value: T }
#[derive(recursa::Node)] struct Root { value: Attached<Colon> }
"#,
        ));
        assert!(mixed_case_report.lowered.is_none());
        assert!(mixed_case_report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == MaterializationDiagnosticKind::UnsafePrettyBoundary
                && diagnostic.message.contains("closed Pretty boundary")
        }));

        let trivia_report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
#[derive(recursa::Node)]
struct Number<'input> { #[lex(pattern = r"[0-9]+")] text: NumberText<'input> }
#[derive(recursa::Node)]
struct Name<'input> { #[lex(pattern = r"[A-Za-z]+")] text: NameText<'input> }
#[derive(recursa::Node)] struct Pair<A, B> { left: A, right: B }
#[derive(recursa::Node)]
struct Root<'input> { value: Pair<Number<'input>, Name<'input>> }
"#,
        ));
        assert!(trivia_report.lowered.is_none());
        assert!(trivia_report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == MaterializationDiagnosticKind::UnsafePrettyBoundary
                && diagnostic.message.contains("closed Pretty boundary")
        }));

        let trivia_narrow_report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"[ ]+" }
#[derive(recursa::Node)]
struct Number<'input> { #[lex(pattern = r"[0-9]+")] text: NumberText<'input> }
#[derive(recursa::Node)]
struct Name<'input> { #[lex(pattern = r"[A-Za-z]+")] text: NameText<'input> }
#[derive(recursa::Node)] struct Pair<A, B> { left: A, right: B }
#[derive(recursa::Node)]
struct Root<'input> { value: Pair<Number<'input>, Name<'input>> }
"#,
        ));
        assert!(trivia_narrow_report.lowered.is_none());
        assert!(trivia_narrow_report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == MaterializationDiagnosticKind::UnsafePrettyBoundary
                && diagnostic.message.contains("closed Pretty boundary")
        }));

        let trivia_safe_report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"[ \t\r\n]+" }
#[derive(recursa::Node)]
struct Number<'input> { #[lex(pattern = r"[0-9]+")] text: NumberText<'input> }
#[derive(recursa::Node)]
struct Name<'input> { #[lex(pattern = r"[A-Za-z]+")] text: NameText<'input> }
#[derive(recursa::Node)] struct Pair<A, B> { left: A, right: B }
#[derive(recursa::Node)]
struct Root<'input> { value: Pair<Number<'input>, Name<'input>> }
"#,
        ));
        assert!(
            trivia_safe_report.diagnostics.is_empty(),
            "{:?}",
            trivia_safe_report.diagnostics
        );
        assert!(trivia_safe_report.lowered.is_some());

        let break_safe_report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":" }
    lexer_tokens { MERGED => r"[A-Za-z]+:" }
    ignore = r"[ \t\r\n]+"
}
#[derive(recursa::Node)]
struct Name<'input> { #[lex(pattern = r"[A-Za-z]+") ] text: NameText<'input> }
#[derive(recursa::Node)] #[tok(COLON)] struct Colon;
#[derive(recursa::Node)]
struct Pair<T> {
    value: T,
    #[pretty(break_before = soft)]
    suffix: Colon,
}
#[derive(recursa::Node)]
struct PairAfter<T> {
    #[pretty(break_after = soft)]
    value: T,
    suffix: Colon,
}
#[derive(recursa::Node)]
struct Root<'input> {
    before: Pair<Name<'input>>,
    after: PairAfter<Name<'input>>,
}
"#,
        ));
        assert!(
            break_safe_report.diagnostics.is_empty(),
            "{:?}",
            break_safe_report.diagnostics
        );
        assert!(break_safe_report.lowered.is_some());
    }

    #[test]
    fn materialization_reuses_one_prebuilt_pretty_catalog_for_repeated_boundaries() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight } }

#[derive(recursa::Node)] #[tok(COLON)] struct Colon;
#[derive(recursa::Node)]
struct Repeated<T> {
    first: T,
    second: T,
    third: T,
    fourth: T,
    fifth: T,
    sixth: T,
    seventh: T,
    eighth: T,
}
#[derive(recursa::Node)] struct Root { value: Repeated<Colon> }
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.lowered.is_some());
        assert_eq!(report.pretty_boundary_proof_counts.catalogs, 1);
        assert_eq!(report.pretty_boundary_proof_counts.catalog_queries, 7);
        assert_eq!(report.pretty_boundary_proof_counts.catalog_products, 1);
    }

    #[test]
    fn closed_pretty_summaries_converge_once_for_a_recursive_shared_dag() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight } }

#[derive(recursa::Node)] #[tok(COLON)] struct Colon;
#[derive(recursa::Node)]
struct Recursive<T> {
    value: T,
    next: Option<Box<Self>>,
}
#[derive(recursa::Node)]
enum Fanout<T> {
    First(Recursive<T>),
    Second(Recursive<T>),
    Third(Recursive<T>),
    Fourth(Recursive<T>),
    Fifth(Recursive<T>),
    Sixth(Recursive<T>),
    Seventh(Recursive<T>),
    Eighth(Recursive<T>),
}
#[derive(recursa::Node)] struct Root { value: Fanout<Colon> }
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.lowered.is_some());
        assert_eq!(report.closed_pretty_summary_counts.fixed_point_rounds, 5);
        assert_eq!(report.closed_pretty_summary_counts.body_evaluations, 20);
        assert_eq!(
            report.closed_pretty_summary_counts.max_retained_boundaries,
            1
        );
        assert_eq!(
            report
                .closed_pretty_summary_counts
                .max_attempted_boundary_insertions,
            8
        );
        assert!(
            report.closed_pretty_summary_counts.cache_queries
                > report.closed_pretty_summary_counts.body_evaluations
        );
    }

    #[test]
    fn closed_pretty_fixed_point_matches_cycle_cut_oracle_for_mutual_recursion() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight } }

#[derive(recursa::Node)] #[tok(COLON)] struct Colon;
#[derive(recursa::Node)] enum Left { Base(Colon), Next(Box<Right>) }
#[derive(recursa::Node)] enum Right { Base(Colon), Next(Box<Left>) }
#[derive(recursa::Node)]
struct Shared {
    first: Left,
    second: Left,
    third: Left,
    fourth: Left,
}
#[derive(recursa::Node)] struct Root { value: Shared }
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.lowered.is_some());
        assert_eq!(report.closed_pretty_summary_counts.legacy_mismatches, 0);
        assert!(
            report.closed_pretty_summary_counts.cache_queries
                > report.closed_pretty_summary_counts.body_evaluations
        );
    }

    #[test]
    fn closed_pretty_fixed_point_ignores_mutual_recursion_discovery_order() {
        let x = MaterializedPrettyBoundary::Regex("X".to_owned());
        let y = MaterializedPrettyBoundary::Regex("Y".to_owned());
        let mut current = [
            PrettyBoundarySummary::default(),
            PrettyBoundarySummary::default(),
        ];
        let mut converged = false;

        for _ in 0..8 {
            let mut next = [
                PrettyBoundarySummary::default(),
                PrettyBoundarySummary::default(),
            ];
            next[0].first.extend_from(&current[1].first);
            next[0].first.insert(x.clone());
            next[1].first.extend_from(&current[0].first);
            next[1].first.insert(y.clone());
            if next == current {
                current = next;
                converged = true;
                break;
            }
            current = next;
        }

        assert!(
            converged,
            "membership must converge despite alternating order"
        );
        for summary in current {
            assert!(summary.first.as_slice().contains(&x));
            assert!(summary.first.as_slice().contains(&y));
            assert_eq!(summary.first.as_slice().len(), 2);
        }

        let mut forward = super::OrderedPrettyBoundaries::default();
        forward.insert(x.clone());
        forward.insert(y.clone());
        let mut reversed = super::OrderedPrettyBoundaries::default();
        reversed.insert(y.clone());
        reversed.insert(x.clone());
        reversed.insert(x.clone());
        assert_eq!(forward, reversed);
        assert_ne!(forward.as_slice(), reversed.as_slice());
        assert_eq!(reversed.as_slice().len(), 2);

        let mut distinct_kinds = super::OrderedPrettyBoundaries::default();
        distinct_kinds.insert(MaterializedPrettyBoundary::Fixed(TokenDefinitionId::next(
            0,
        )));
        distinct_kinds.insert(x);
        assert_eq!(distinct_kinds.as_slice().len(), 2);
    }

    fn assert_code(
        diagnostic: &super::MaterializationDiagnostic,
        kind: MaterializationDiagnosticKind,
        code: &str,
    ) {
        assert_eq!(diagnostic.kind, kind);
        assert_eq!(
            crate::source_analysis::materialization_code(diagnostic.kind),
            code
        );
    }

    #[test]
    fn contextual_keyword_targets_are_concrete_kind_sets_before_semantic_analysis() {
        let report = materialize(resolve(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("SELECT", "FROM", "AS", ",")]
struct Fixed;

#[derive(recursa::Node)]
struct Projection {
    #[lex(pattern = r"[A-Za-z_]+")]
    word: Word,
    #[lex(pattern = r"[A-Za-z_]+", admits(AS))]
    column: ColumnName,
    #[lex(pattern = r"[A-Za-z_]+", admits(AS))]
    table: TableName,
    #[lex(pattern = r"[A-Za-z_]+")]
    alias: Alias,
}
"#,
        ));

        assert!(report.lexical_diagnostics.is_empty());
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.unwrap();
        assert_eq!(lowered.lexical_plan.token_kinds.len(), 5);
        assert_eq!(lowered.grammar.kind_count, 5);
        assert_eq!(lowered.lexical_plan.content_bases.len(), 1);

        for target in &lowered.lexical_plan.parser_targets {
            let correlation = lowered
                .lowering_map
                .content_targets
                .iter()
                .find(|correlation| correlation.semantic == target.semantic)
                .unwrap();
            assert_eq!(
                target_kind_set(&lowered.grammar, correlation.lowered),
                target.accepted_kinds(),
                "{} must reach semantic analysis as its exact base-plus-admissions set",
                target.name,
            );
            assert_eq!(
                target.admitted_kinds.iter().count(),
                usize::from(matches!(target.name.as_str(), "ColumnName" | "TableName")),
            );
        }

        let content_kinds = lowered
            .lowering_map
            .token_definitions
            .iter()
            .filter_map(|correlation| {
                matches!(
                    lowered.source_modeled.source_model().token_definitions
                        [correlation.source.index()]
                    .kind,
                    crate::source_model::TokenDefinitionKind::Content(_)
                )
                .then_some(correlation.lowered)
            })
            .collect::<Vec<_>>();
        assert_eq!(content_kinds.len(), 4);
        assert!(content_kinds.iter().all(|kind| *kind == content_kinds[0]));
    }

    #[test]
    fn content_target_type_ids_preserve_source_definition_order() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"z")]
    zeta: Zeta,
    #[lex(pattern = r"a")]
    alpha: Alpha,
}
"#,
        ));

        assert!(report.lexical_diagnostics.is_empty());
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.expect("valid grammar materializes");
        assert_eq!(
            lowered
                .lexical_plan
                .parser_targets
                .iter()
                .map(|target| (target.name.as_str(), target.definition.index()))
                .collect::<Vec<_>>(),
            [("Zeta", 0), ("Alpha", 1)],
        );
        let target_type_ids = lowered
            .lexical_plan
            .parser_targets
            .iter()
            .map(|target| {
                lowered
                    .lowering_map
                    .content_targets
                    .iter()
                    .find(|correlation| correlation.semantic == target.semantic)
                    .expect("every parser target reaches semantic analysis")
                    .lowered
                    .index()
            })
            .collect::<Vec<_>>();
        assert!(target_type_ids.windows(2).all(|ids| ids[0] < ids[1]));
        assert_eq!(
            target_type_ids
                .iter()
                .map(|id| lowered.grammar.types[*id].name.as_str())
                .collect::<Vec<_>>(),
            ["Zeta", "Alpha"],
        );
    }

    fn fields(
        application: &MaterializedNodeApplication,
    ) -> Vec<(&ClosedType, &super::MaterializationTrace)> {
        application
            .parsed_fields()
            .map(|element| match element {
                MaterializedElement::SemanticField {
                    projection: MaterializedSemanticFieldProjection::Direct { ty, trace, .. },
                    ..
                } => (ty, trace),
                MaterializedElement::FixedToken { .. }
                | MaterializedElement::OptionalFixedTokenGroup { .. }
                | MaterializedElement::SemanticField {
                    projection: MaterializedSemanticFieldProjection::FixedPresence { .. },
                    ..
                } => {
                    unreachable!("filtered semantic field")
                }
            })
            .collect()
    }

    #[test]
    fn materializes_closed_generic_applications_and_self_without_unrolling_cycles() {
        let resolved = resolve(
            r#"
recursa::grammar! { module = crate::arith }

#[derive(recursa::Node)]
struct Number<'input> {
    #[lex(pattern = r"[0-9]+")]
    text: Digits,
}

#[derive(recursa::Node)]
struct Recursive<'input, T> {
    value: T,
    next: Option<Box<Self>>,
}

#[derive(recursa::Node)]
struct Root<'input> {
    expression: Recursive<'input, Number<'input>>,
    static_expression: Recursive<'static, Number<'static>>,
}
"#,
        );

        let report = materialize(resolved);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let grammar = report.lowered.unwrap();
        assert_eq!(
            grammar
                .applications
                .iter()
                .map(|application| application.display_name.as_str())
                .collect::<Vec<_>>(),
            [
                "arith::Number",
                "arith::Root",
                "arith::Recursive<arith::Number>"
            ],
        );
        let recursive = grammar
            .applications
            .iter()
            .find(|application| application.display_name.starts_with("arith::Recursive"))
            .unwrap();
        assert_eq!(
            grammar
                .applications
                .iter()
                .filter(|application| application.display_name.starts_with("arith::Recursive"))
                .count(),
            1,
        );
        let root = grammar
            .applications
            .iter()
            .find(|application| application.display_name == "arith::Root")
            .unwrap();
        assert!(matches!(
            fields(root)[0].0,
            ClosedType::Node(node) if node.lifetimes[0].to_string() == "'input"
        ));
        assert!(matches!(
            fields(root)[1].0,
            ClosedType::Node(node) if node.lifetimes[0].to_string() == "'static"
        ));
        assert!(matches!(
            fields(recursive)[1].0,
            ClosedType::Optional(inner)
                if matches!(inner.as_ref(), ClosedType::Boxed(boxed)
                    if matches!(boxed.as_ref(), ClosedType::Node(node)
                        if node.application == recursive.id
                            && node.lifetimes[0].to_string() == "'input"))
        ));
        let recursive_trace = &fields(recursive)[1].1.steps;
        assert!(recursive_trace.contains(&MaterializationTraceStep::SelfApplication(recursive.id)));
        assert_eq!(
            recursive_trace.last(),
            Some(&MaterializationTraceStep::Application(recursive.id)),
        );
        assert_eq!(grammar.grammar.types.len(), grammar.applications.len() + 1);
    }

    #[test]
    fn expands_selected_tree_aliases_with_defaults_and_retains_alias_traces() {
        let resolved = resolve(
            r#"
recursa::grammar! { module = crate::arith }

#[derive(recursa::Node)]
struct Number<'input> {
    #[lex(pattern = r"[0-9]+")]
    text: Digits,
}

type Heap<T> = Box<T>;
type Maybe<'input, T = Number<'input>> = Option<Heap<T>>;
type OpaqueToGrammar = dependency::OrdinaryRustType;

#[derive(recursa::Node)]
struct Root<'input> {
    explicit: Maybe<'input, Number<'input>>,
    defaulted: Maybe<'input>,
    static_defaulted: Maybe<'static>,
}
"#,
        );

        let report = materialize(resolved);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let grammar = report.lowered.unwrap();
        let root = grammar
            .applications
            .iter()
            .find(|application| application.display_name == "arith::Root")
            .unwrap();
        assert_eq!(fields(root)[0].0, fields(root)[1].0);
        assert_eq!(
            fields(root)[0]
                .1
                .steps
                .iter()
                .filter(|step| matches!(step, super::MaterializationTraceStep::Alias(_)))
                .count(),
            2,
        );
        let explicit_trace = &fields(root)[0].1.steps;
        let alias_positions = explicit_trace
            .iter()
            .enumerate()
            .filter_map(|(position, step)| {
                matches!(step, MaterializationTraceStep::Alias(_)).then_some(position)
            })
            .collect::<Vec<_>>();
        let parameter_position = explicit_trace
            .iter()
            .position(|step| matches!(step, MaterializationTraceStep::Parameter(_)))
            .unwrap();
        assert!(alias_positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            alias_positions
                .iter()
                .all(|position| *position < parameter_position)
        );
        assert!(
            explicit_trace
                .iter()
                .any(|step| matches!(step, MaterializationTraceStep::Parameter(_)))
        );
        assert!(matches!(
            explicit_trace.last(),
            Some(MaterializationTraceStep::Application(_))
        ));
        assert!(matches!(
            fields(root)[2].0,
            ClosedType::Optional(optional)
                if matches!(optional.as_ref(), ClosedType::Boxed(boxed)
                    if matches!(boxed.as_ref(), ClosedType::Node(node)
                        if node.lifetimes[0].to_string() == "'static"))
        ));
        assert!(matches!(
            fields(root)[0].0,
            ClosedType::Optional(optional)
                if matches!(optional.as_ref(), ClosedType::Boxed(boxed)
                    if matches!(boxed.as_ref(), ClosedType::Node(_)))
        ));
    }

    #[test]
    fn preserves_box_placement_and_exact_supported_cardinalities_in_normalized_grammar() {
        let resolved = resolve(
            r#"
recursa::grammar! { module = crate::shapes }
#[derive(recursa::Node)]
struct Leaf { #[lex(pattern = r"x")] text: Text }
#[derive(recursa::Node)]
struct Root {
    one: Box<Leaf>,
    optional: Option<Box<Leaf>>,
    zero_or_more: Vec<Box<Leaf>>,
    one_or_more: recursa::Vec1<Box<Leaf>>,
    optional_one_or_more: Option<Box<recursa::Vec1<Box<Leaf>>>>,
}
"#,
        );

        let report = materialize(resolved);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let grammar = report.lowered.unwrap();
        let root = grammar
            .applications
            .iter()
            .find(|application| application.display_name == "shapes::Root")
            .unwrap();
        let root_type =
            &grammar.grammar.types[grammar.grammar.entry_types[root.id.index()].index()];
        let TypeShape::Sequence { sites } = &root_type.shape else {
            panic!("root is a sequence")
        };
        assert_eq!(
            sites
                .iter()
                .map(|site| grammar.grammar.sites[site.index()].container.to_string())
                .collect::<Vec<_>>(),
            [
                "Box<T>",
                "Option<Box<T>>",
                "Vec<Box<T>>",
                "Vec1<Box<T>>",
                "Option<Box<Vec1<Box<T>>>>",
            ],
        );
    }

    #[test]
    fn rejects_open_unsupported_cyclic_and_invalid_closed_types_before_semantic_analysis() {
        let invalid_containers = materialize(resolve(
            r#"
recursa::grammar! { module = crate::invalid }
#[derive(recursa::Node)] struct Leaf { this: Self }
#[derive(recursa::Node)] struct Root {
    a: Option<Option<Leaf>>,
    b: Option<Vec<Leaf>>,
    c: Vec<Option<Leaf>>,
    d: Vec<Vec<Leaf>>,
    e: recursa::Vec1<Option<Leaf>>,
}
"#,
        ));
        assert!(invalid_containers.lowered.is_none());
        assert_eq!(
            invalid_containers
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [MaterializationDiagnosticKind::InvalidContainer; 5],
        );
        assert!(invalid_containers.diagnostics.iter().all(|diagnostic| {
            crate::source_analysis::materialization_code(diagnostic.kind) == "RCA3206"
        }));

        let unsupported = materialize(resolve(
            r#"
recursa::grammar! { module = crate::invalid }
#[derive(recursa::Node)] struct Generic<T> { value: T }
#[derive(recursa::Node)] struct ConstGeneric<const N: usize> { this: Self }
#[derive(recursa::Node)] struct Root {
    open: Generic,
    inferred: _,
    opaque: impl Sized,
    constant: ConstGeneric<1>,
}
"#,
        ));
        assert!(unsupported.lowered.is_none());
        assert_eq!(
            unsupported
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                MaterializationDiagnosticKind::OpenApplication,
                MaterializationDiagnosticKind::UnsupportedType,
                MaterializationDiagnosticKind::UnsupportedType,
                MaterializationDiagnosticKind::ConstGeneric,
            ],
        );
        assert_eq!(
            unsupported
                .diagnostics
                .iter()
                .map(|diagnostic| crate::source_analysis::materialization_code(diagnostic.kind))
                .collect::<Vec<_>>(),
            ["RCA3202", "RCA3204", "RCA3204", "RCA3203"],
        );

        let aliases = materialize(resolve(
            r#"
recursa::grammar! { module = crate::invalid }
type A = B;
type B = A;
#[derive(recursa::Node)] struct Root { value: A }
"#,
        ));
        assert_code(
            &aliases.diagnostics[0],
            MaterializationDiagnosticKind::AliasCycle,
            "RCA3207",
        );

        let default_cycle = materialize(resolve(
            r#"
recursa::grammar! { module = crate::invalid }
type Loop<T = Loop> = T;
#[derive(recursa::Node)] struct Root { value: Loop }
"#,
        ));
        assert_eq!(
            default_cycle.diagnostics[0].kind,
            MaterializationDiagnosticKind::AliasCycle,
        );

        let changing = materialize(resolve(
            r#"
recursa::grammar! { module = crate::invalid }
#[derive(recursa::Node)] struct Leaf { this: Self }
#[derive(recursa::Node)] struct Growing<T> { next: Box<Growing<Option<T>>> }
#[derive(recursa::Node)] struct Root { value: Growing<Leaf> }
"#,
        ));
        assert_code(
            &changing.diagnostics[0],
            MaterializationDiagnosticKind::NonRegularRecursion,
            "RCA3208",
        );

        let invalid_arguments = materialize(resolve(
            r#"
recursa::grammar! { module = crate::invalid }
#[derive(recursa::Node)] struct Leaf;
#[derive(recursa::Node)] struct Root { value: Option<Leaf, Leaf> }
"#,
        ));
        assert_code(
            &invalid_arguments.diagnostics[0],
            MaterializationDiagnosticKind::InvalidArguments,
            "RCA3205",
        );

        for source in [
            r#"
recursa::grammar! { module = crate::invalid }
type External = dependency::Thing;
#[derive(recursa::Node)] struct Root { value: External }
"#,
            r#"
recursa::grammar! { module = crate::invalid }
#[derive(recursa::Node)] struct Generic<T> { value: T::Associated }
#[derive(recursa::Node)] struct Root { value: Generic<Root> }
"#,
            r#"
recursa::grammar! { module = crate::invalid }
#[derive(recursa::Node)] struct Generic<T> { value: <T as Trait>::Associated }
#[derive(recursa::Node)] struct Root { value: Generic<Root> }
"#,
        ] {
            let resolution = resolve_report(source);
            assert!(resolution.resolved.is_none());
            assert!(!resolution.diagnostics.is_empty());
        }
    }

    #[test]
    fn arithmetic_generic_application_reaches_successful_semantic_analysis() {
        let resolved = resolve(
            r#"
recursa::grammar! {
    module = crate::arith,
    max_lookahead = 3,
    keyword_matching = ascii_insensitive,
}
#[derive(recursa::Node)]
struct Number<'input> { #[lex(pattern = r"[0-9]+")] text: Digits }
#[derive(recursa::Node)]
struct Word<'input> { #[lex(pattern = r"[A-Za-z]+")] text: Letters }
#[derive(recursa::Node)]
struct Binary<'input, T> { left: T, right: T }
#[derive(recursa::Node)]
struct Expression<'input> {
    sum: Binary<'input, Number<'input>>,
    words: Binary<'input, Word<'input>>,
}
"#,
        );

        let report = materialize(resolved);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let materialized = report.lowered.unwrap();
        let build_inputs = &materialized.source_modeled.resolved().build_inputs;
        assert_eq!(build_inputs.max_lookahead, 3);
        assert_eq!(
            build_inputs.text_equivalence,
            crate::TextEquivalence::AsciiInsensitive,
        );
        assert_eq!(materialized.grammar.max_lookahead, 3);
        assert_eq!(
            materialized.grammar.text_equivalence,
            crate::TextEquivalence::AsciiInsensitive,
        );
        let binary = materialized
            .applications
            .iter()
            .find(|application| application.display_name == "arith::Binary<arith::Number>")
            .unwrap();
        assert_eq!(
            materialized
                .applications
                .iter()
                .filter(|application| application.node == binary.node)
                .count(),
            2,
        );
        let correlation = materialized
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == binary.id)
            .unwrap();
        assert_eq!(correlation.node, binary.node);
        assert_eq!(
            materialized.grammar.types[correlation.lowered.index()].name,
            "arith::Binary<arith::Number>",
        );
        let analysis = crate::analyze(&materialized.grammar);
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        assert!(analysis.output.is_some());
        assert!(materialized.lowering_map.fields.iter().all(|correlation| {
            materialized
                .source_modeled
                .resolved()
                .nodes
                .iter()
                .flat_map(|node| &node.fields)
                .any(|field| field.id == correlation.field)
        }));
        for field in &materialized.source_modeled.resolved().nodes[binary.node.index()].fields {
            assert_eq!(
                materialized
                    .lowering_map
                    .fields
                    .iter()
                    .filter(|correlation| correlation.field == field.id)
                    .count(),
                2,
            );
        }
    }

    #[test]
    fn typed_source_field_and_variant_identities_drive_every_materialized_join() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::choices }
#[derive(recursa::Node)]
struct Leaf { #[lex(pattern = "x")] text: Text }
#[derive(recursa::Node)]
enum Choice {
    #[tok("A", this)]
    First(Leaf),
    #[tok("B", this)]
    Second(Leaf),
}
"#,
        ));
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.unwrap();
        let resolved = lowered.source_modeled.resolved();
        let source_model = lowered.source_modeled.source_model();
        assert_eq!(
            lowered.lowering_map.sources.len(),
            resolved.tree.files.len()
        );
        assert!(
            lowered
                .lowering_map
                .sources
                .iter()
                .all(|correlation| correlation.resolved == correlation.lowered)
        );

        let resolved_choice = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Choice")
            .unwrap();
        let crate::resolution::ResolvedNodeBody::Enum { variants } = &resolved_choice.body else {
            panic!("Choice is an enum")
        };
        let source_choice = source_model
            .nodes
            .iter()
            .find(|node| node.resolved_node == resolved_choice.id)
            .unwrap();
        assert_eq!(source_choice.variants().len(), variants.len());
        let application = lowered
            .applications
            .iter()
            .find(|application| application.node == resolved_choice.id)
            .unwrap();

        for variant in variants {
            let source_variant = source_choice
                .variants()
                .iter()
                .find(|candidate| candidate.resolved_variant == variant.id)
                .unwrap();
            assert_eq!(source_variant.site.source, resolved_choice.source);
            let correlation = lowered
                .lowering_map
                .variants
                .iter()
                .find(|correlation| {
                    correlation.application == application.id && correlation.variant == variant.id
                })
                .unwrap();
            let declaration = &lowered.grammar.declarations[correlation.lowered.index()];
            assert_eq!(
                lowered.grammar.locations[declaration.location.index()].source,
                resolved_choice.source,
            );
            for field in &variant.fields {
                assert!(lowered.lowering_map.fields.iter().any(|correlation| {
                    correlation.application == application.id && correlation.field == *field
                }));
            }
        }
        assert!(
            source_model
                .token_uses
                .iter()
                .all(|token_use| token_use.site.source == resolved_choice.source)
        );
    }

    #[test]
    fn all_atom_pratt_remains_pratt_in_normalized_grammar() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::atoms }
#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[tok("INT")]
    Int,
    #[tok("NAME")]
    Name,
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.unwrap();
        let expr = lowered
            .applications
            .iter()
            .find(|application| application.display_name == "atoms::Expr")
            .unwrap();
        let correlation = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == expr.id)
            .unwrap();
        let TypeShape::Pratt { variants } =
            &lowered.grammar.types[correlation.lowered.index()].shape
        else {
            panic!("#[pratt] must not collapse to an ordinary enum")
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.position)
                .collect::<Vec<_>>(),
            [crate::PrattPosition::Atom, crate::PrattPosition::Atom],
        );
    }

    #[test]
    fn materializes_pratt_nud_led_thresholds_and_typed_lhs_correlations() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::arith }
#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[tok("INT")]
    Int,
    #[parse(prefix, bp = 30)]
    #[tok("-")]
    Neg(Box<Self>),
    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok("+")]
    Add(Box<Self>, Box<Self>),
    #[parse(postfix, bp = 40)]
    #[tok("!")]
    Factorial(Box<Self>),
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.unwrap();
        let expr = lowered
            .applications
            .iter()
            .find(|application| application.display_name == "arith::Expr")
            .unwrap();
        let MaterializedNodeBody::Pratt(materialized_variants) = &expr.body else {
            panic!("materialized Expr is Pratt")
        };
        assert!(matches!(
            materialized_variants[0],
            MaterializedPrattVariant::Nud(MaterializedPrattNud {
                position: MaterializedPrattNudPosition::Atom,
                ..
            })
        ));
        assert!(matches!(
            materialized_variants[1],
            MaterializedPrattVariant::Nud(MaterializedPrattNud {
                position: MaterializedPrattNudPosition::Prefix { bp: 30 },
                ..
            })
        ));
        let MaterializedPrattVariant::Led(infix) = &materialized_variants[2] else {
            panic!("Add is a typed led variant")
        };
        assert_eq!(
            infix.position,
            MaterializedPrattLedPosition::Infix { lbp: 10, rbp: 11 },
        );
        assert_eq!(infix.lhs.owner.application, expr.id);
        assert_eq!(
            infix
                .tail
                .iter()
                .filter(|element| matches!(element, MaterializedElement::SemanticField { .. }))
                .count(),
            1,
        );
        assert!(matches!(
            materialized_variants[3],
            MaterializedPrattVariant::Led(MaterializedPrattLed {
                position: MaterializedPrattLedPosition::Postfix { bp: 40 },
                ..
            })
        ));
        assert_eq!(
            expr.fields().count(),
            4,
            "the all-field seam includes both implicit led lhs fields",
        );
        assert_eq!(
            expr.fields()
                .filter(|field| matches!(field, MaterializedFieldView::PrattLhs(_)))
                .count(),
            2,
        );
        let expr_type = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == expr.id)
            .unwrap()
            .lowered;
        let TypeShape::Pratt { variants } = &lowered.grammar.types[expr_type.index()].shape else {
            panic!("Expr is Pratt")
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.position)
                .collect::<Vec<_>>(),
            [
                crate::PrattPosition::Atom,
                crate::PrattPosition::Prefix { bp: 30 },
                crate::PrattPosition::Infix { lbp: 10, rbp: 11 },
                crate::PrattPosition::Postfix { bp: 40 },
            ],
        );
        let prefix_recursive = variants[1]
            .sites
            .iter()
            .map(|site| &lowered.grammar.sites[site.index()])
            .find(|site| site.target == expr_type)
            .unwrap();
        assert_eq!(prefix_recursive.min_bp, 30);
        let infix_recursive = variants[2]
            .sites
            .iter()
            .map(|site| &lowered.grammar.sites[site.index()])
            .find(|site| site.target == expr_type)
            .unwrap();
        assert_eq!(infix_recursive.min_bp, 11);
        assert_eq!(
            lowered
                .lowering_map
                .fields
                .iter()
                .filter(|correlation| matches!(
                    correlation.lowered,
                    MaterializedFieldLowering::PrattLhs { owner, .. } if owner == expr_type
                ))
                .count(),
            2,
        );
        let lhs_variants = lowered
            .lowering_map
            .fields
            .iter()
            .filter_map(|correlation| match correlation.lowered {
                MaterializedFieldLowering::PrattLhs { variant, .. } => Some(variant),
                MaterializedFieldLowering::ParseSite(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            lhs_variants,
            [variants[2].declaration, variants[3].declaration],
        );
        assert_eq!(
            lowered
                .lowering_map
                .fields
                .iter()
                .filter(|correlation| matches!(
                    correlation.lowered,
                    MaterializedFieldLowering::ParseSite(_)
                ))
                .count(),
            2,
        );
        assert_eq!(
            variants[2].sites.len(),
            2,
            "lhs is not a normalized parse site"
        );
        assert_eq!(
            variants[3].sites.len(),
            1,
            "lhs is not a normalized parse site"
        );
    }

    #[test]
    fn rejects_missing_unboxed_and_foreign_pratt_lhs_fields() {
        for invalid_variant in [
            r#"#[parse(infix, lbp = 1, rbp = 2)] Missing"#,
            r#"#[parse(infix, lbp = 1, rbp = 2)] Unboxed(Self, Box<Self>)"#,
            r#"#[parse(postfix, bp = 3)] Foreign(Box<Leaf>)"#,
        ] {
            let source = format!(
                r#"
recursa::grammar! {{ module = crate::invalid }}
#[derive(recursa::Node)]
struct Leaf;
#[derive(recursa::Node)]
#[pratt]
enum Expr {{ {invalid_variant} }}
"#,
            );
            let report = materialize(resolve(&source));
            assert!(report.lowered.is_none(), "accepted `{invalid_variant}`");
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == MaterializationDiagnosticKind::InvalidPrattShape
            }));
            let diagnostic = report
                .diagnostics
                .iter()
                .find(|diagnostic| {
                    diagnostic.kind == MaterializationDiagnosticKind::InvalidPrattShape
                })
                .unwrap();
            assert_eq!(
                crate::source_analysis::materialization_code(diagnostic.kind),
                "RCA3209"
            );
        }
    }

    #[test]
    fn rejects_prefix_and_infix_without_exact_recursive_operand_shapes() {
        let invalid_variants = [
            r#"#[parse(prefix, bp = 1)] Unit"#,
            r#"#[parse(prefix, bp = 1)] Unboxed(Self)"#,
            r#"#[parse(prefix, bp = 1)] Foreign(Box<Leaf>)"#,
            r#"#[parse(prefix, bp = 1)] Multiple(Box<Self>, Box<Self>)"#,
            r#"#[parse(infix, lbp = 1, rbp = 2)] OnlyLhs(Box<Self>)"#,
            r#"#[parse(infix, lbp = 1, rbp = 2)] ForeignRhs(Box<Self>, Box<Leaf>)"#,
            r#"#[parse(infix, lbp = 1, rbp = 2)] MultipleRhs(Box<Self>, Box<Self>, Box<Self>)"#,
        ];
        for invalid_variant in invalid_variants {
            let source = format!(
                r#"
recursa::grammar! {{ module = crate::invalid }}
#[derive(recursa::Node)]
struct Leaf;
#[derive(recursa::Node)]
#[pratt]
enum Expr {{ {invalid_variant} }}
"#,
            );
            let report = materialize(resolve(&source));
            assert!(report.lowered.is_none(), "accepted `{invalid_variant}`");
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == MaterializationDiagnosticKind::InvalidPrattShape
            }));
        }
    }

    #[test]
    fn infix_tail_allows_content_fields_around_one_recursive_rhs() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::operators }
#[derive(recursa::Node)]
#[pratt]
enum Expr<'input> {
    #[tok("ATOM")]
    Atom,
    #[parse(infix, lbp = 8, rbp = 9)]
    Custom(
        Box<Self>,
        #[lex(pattern = r"[~@]+")]
        OperatorText<'input>,
        Box<Self>,
    ),
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report
            .lowered
            .expect("one recursive rhs with an ordinary operator field materializes");
        let expr = lowered
            .applications
            .iter()
            .find(|application| application.display_name == "operators::Expr")
            .expect("Expr application materializes");
        let MaterializedNodeBody::Pratt(variants) = &expr.body else {
            panic!("Expr is Pratt")
        };
        let MaterializedPrattVariant::Led(custom) = &variants[1] else {
            panic!("Custom is led")
        };
        assert_eq!(custom.tail.len(), 2);
        assert!(matches!(
            &custom.tail[0],
            MaterializedElement::SemanticField {
                projection: MaterializedSemanticFieldProjection::Direct {
                    ty: ClosedType::ContentToken(_),
                    ..
                },
                ..
            }
        ));
        let MaterializedElement::SemanticField {
            projection:
                MaterializedSemanticFieldProjection::Direct {
                    ty: ClosedType::Boxed(rhs),
                    trace,
                    separator: None,
                    ..
                },
            ..
        } = &custom.tail[1]
        else {
            panic!("the final infix field is the direct boxed rhs")
        };
        let ClosedType::Node(rhs) = rhs.as_ref() else {
            panic!("the infix rhs targets Expr")
        };
        assert_eq!(rhs.application, expr.id);
        assert!(
            trace
                .steps
                .contains(&MaterializationTraceStep::SelfApplication(expr.id))
        );
        let application = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == expr.id)
            .expect("Expr retains its lowered type correlation");
        let TypeShape::Pratt { variants } =
            &lowered.grammar.types[application.lowered.index()].shape
        else {
            panic!("lowered Expr is Pratt")
        };
        assert_eq!(
            variants[1]
                .sites
                .iter()
                .map(|site| lowered.grammar.sites[site.index()].min_bp)
                .collect::<Vec<_>>(),
            [0, 9],
            "only the recursive infix rhs receives rbp",
        );
    }

    #[test]
    fn rejects_zero_multiple_and_non_exact_recursive_infix_rhs_fields() {
        for (invalid_variant, expected) in [
            (
                r#"#[parse(infix, lbp = 1, rbp = 2)] Zero(Box<Self>, Leaf)"#,
                "found 0 recursive semantic fields",
            ),
            (
                r#"#[parse(infix, lbp = 1, rbp = 2)] Multiple(Box<Self>, Box<Self>, Leaf, Box<Self>)"#,
                "found 2 recursive semantic fields",
            ),
            (
                r#"#[parse(infix, lbp = 1, rbp = 2)] Optional(Box<Self>, Leaf, Option<Box<Self>>)"#,
                "must be an authored Box<Self>",
            ),
        ] {
            let source = format!(
                r#"
recursa::grammar! {{ module = crate::invalid }}
#[derive(recursa::Node)]
#[tok("LEAF")]
struct Leaf;
#[derive(recursa::Node)]
#[pratt]
enum Expr {{
    #[tok("ATOM")]
    Atom,
    {invalid_variant},
}}
"#,
            );
            let report = materialize(resolve(&source));
            assert!(report.lowered.is_none(), "accepted `{invalid_variant}`");
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == MaterializationDiagnosticKind::InvalidPrattShape
                    && diagnostic.message.contains(expected)
            }));
        }
    }

    #[test]
    fn rejects_named_and_lifetime_mismatched_pratt_self_operands() {
        let invalid_variants = [
            r#"#[parse(prefix, bp = 1)] Named(Box<Expr<'input>>)"#,
            r#"#[parse(prefix, bp = 1)] Static(Box<Expr<'static>>)"#,
            r#"#[parse(infix, lbp = 1, rbp = 2)] NamedLhs(Box<Expr<'input>>, Box<Self>)"#,
            r#"#[parse(infix, lbp = 1, rbp = 2)] NamedRhs(Box<Self>, Box<Expr<'input>>)"#,
            r#"#[parse(infix, lbp = 1, rbp = 2)] StaticRhs(Box<Self>, Box<Expr<'static>>)"#,
        ];
        for invalid_variant in invalid_variants {
            let source = format!(
                r#"
recursa::grammar! {{ module = crate::invalid }}
#[derive(recursa::Node)]
#[pratt]
enum Expr<'input> {{ {invalid_variant} }}
"#,
            );
            let report = materialize(resolve(&source));
            assert!(report.lowered.is_none(), "accepted `{invalid_variant}`");
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == MaterializationDiagnosticKind::InvalidPrattShape
            }));
        }
    }

    #[test]
    fn rejects_fixed_syntax_attached_to_the_implicit_pratt_lhs() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::invalid }
#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[parse(infix, lbp = 1, rbp = 2)]
    #[tok("+")]
    Invalid(#[tok("(", this, ")")] Box<Self>, Box<Self>),
}
"#,
        ));

        assert!(report.lowered.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == MaterializationDiagnosticKind::InvalidPrattShape
                && diagnostic.message.contains("implicit Pratt lhs")
        }));
    }

    #[test]
    fn preserves_fixed_syntax_attached_to_prefix_and_infix_rhs_operands() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::valid }
#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[tok("ATOM")]
    Atom,
    #[parse(prefix, bp = 3)]
    #[tok("!")]
    Prefix(#[tok("(", this, ")")] Box<Self>),
    #[parse(infix, lbp = 1, rbp = 2)]
    #[tok("+")]
    Infix(
        #[tok(this, "~")] Box<Self>,
        #[tok("[", this, "]")] Box<Self>,
    ),
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report
            .lowered
            .expect("valid interleaved syntax materializes");
        let expr = lowered
            .applications
            .iter()
            .find(|application| application.display_name == "valid::Expr")
            .unwrap();
        let MaterializedNodeBody::Pratt(variants) = &expr.body else {
            panic!("Expr is Pratt")
        };
        let MaterializedPrattVariant::Nud(prefix) = &variants[1] else {
            panic!("Prefix is nud")
        };
        assert_eq!(
            prefix
                .body
                .iter()
                .filter(|element| matches!(element, MaterializedElement::FixedToken { .. }))
                .count(),
            1,
        );
        let MaterializedElement::SemanticField {
            attachment: Some(prefix_attachment),
            ..
        } = &prefix.body[1]
        else {
            panic!("prefix operand retains its fixed attachment")
        };
        assert_eq!(
            (
                prefix_attachment.before.len(),
                prefix_attachment.after.len()
            ),
            (1, 1),
        );
        let MaterializedPrattVariant::Led(infix) = &variants[2] else {
            panic!("Infix is led")
        };
        assert_eq!(
            infix
                .tail
                .iter()
                .filter(|element| matches!(element, MaterializedElement::FixedToken { .. }))
                .count(),
            2,
        );
        let MaterializedElement::SemanticField {
            attachment: Some(infix_attachment),
            ..
        } = infix.tail.last().unwrap()
        else {
            panic!("infix rhs retains its fixed attachment")
        };
        assert_eq!(
            (infix_attachment.before.len(), infix_attachment.after.len()),
            (1, 1),
        );
    }

    #[test]
    fn projects_field_attachments_as_private_normalized_sequence_units() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::attached }
#[derive(recursa::Node)]
#[tok("LEAF")]
struct Leaf;
#[derive(recursa::Node)]
struct Root {
    #[tok("[", this, "]")]
    value: Option<Leaf>,
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.expect("attachment grammar materializes");
        let root = lowered
            .applications
            .iter()
            .find(|application| application.display_name == "attached::Root")
            .unwrap();
        let root_type = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == root.id)
            .unwrap()
            .lowered;
        let TypeShape::Sequence { sites: root_sites } =
            &lowered.grammar.types[root_type.index()].shape
        else {
            panic!("Root is a sequence")
        };
        assert_eq!(
            root_sites.len(),
            1,
            "the attachment is one authored field site"
        );
        let outer = &lowered.grammar.sites[root_sites[0].index()];
        assert!(matches!(outer.container, ContainerShape::Optional(_)));
        let attachment_type = outer.target;
        let [attachment_correlation] = lowered.lowering_map.attachments.as_slice() else {
            panic!("one typed private attachment correlation")
        };
        assert_eq!(attachment_correlation.application, root.id);
        assert_eq!(attachment_correlation.outer, root_sites[0]);
        assert_eq!(attachment_correlation.unit, attachment_type);
        assert_eq!(attachment_correlation.semantic_index, 1);
        assert!(
            !lowered.grammar.entry_types.contains(&attachment_type),
            "the attachment unit is not an authored entry type",
        );
        let TypeShape::Sequence { sites: unit_sites } =
            &lowered.grammar.types[attachment_type.index()].shape
        else {
            panic!("attachment target is a synthetic sequence")
        };
        assert_eq!(unit_sites.len(), 3, "fixed-before, field, fixed-after");
        assert!(unit_sites.iter().all(|site| matches!(
            lowered.grammar.sites[site.index()].container,
            ContainerShape::Direct
        )));
        let analysis = crate::analyze(&lowered.grammar);
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let projection = crate::semantic_projection::canonical_semantic_projection(
            &crate::source_analysis::AnalyzedGrammar {
                lowered: lowered.clone(),
                analysis: analysis.clone(),
            },
        );
        assert!(
            !projection.contains("__recursa_attachment"),
            "private attachment units do not become semantic/debug identities",
        );
        assert!(
            projection.contains("target=attachment("),
            "non-direct attachment decisions remain visible anonymously",
        );
        let output = analysis.output.unwrap();
        assert_eq!(
            output.first(attachment_type).unwrap().kinds(),
            output
                .first(lowered.grammar.sites[unit_sites[0].index()].target)
                .unwrap()
                .kinds(),
            "FIRST for the field container begins at its attached fixed prefix",
        );
        assert!(!output.first(attachment_type).unwrap().nullable);
        let field = root.parsed_fields().next().unwrap();
        let MaterializedElement::SemanticField {
            attachment: Some(attachment),
            ..
        } = field
        else {
            panic!("materialization retains the attachment")
        };
        assert_eq!((attachment.before.len(), attachment.after.len()), (1, 1));
    }

    #[test]
    fn authored_attachment_suffix_names_remain_visible_in_semantic_projection() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::named }
#[allow(non_camel_case_types)]
#[derive(recursa::Node)]
#[tok("LEAF")]
struct __recursa_attachment;
#[derive(recursa::Node)]
struct Root {
    value: __recursa_attachment,
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.expect("named grammar materializes");
        let analysis = crate::analyze(&lowered.grammar);
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let projection = crate::semantic_projection::canonical_semantic_projection(
            &crate::source_analysis::AnalyzedGrammar { lowered, analysis },
        );
        assert!(projection.contains("type named::__recursa_attachment = sequence("));
        assert!(projection.contains("target=named::__recursa_attachment"));
        assert!(projection.contains("facts named::__recursa_attachment "));
        assert!(!projection.contains("target=attachment(named::"));
    }

    #[test]
    fn projected_attachment_ordinals_account_for_every_preceding_expansion() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::ordinals }
#[derive(recursa::Node)]
#[tok("LEAF")]
struct Leaf;
#[derive(recursa::Node)]
struct Root {
    #[tok("[", this, "!", "]")]
    first: Leaf,
    #[tok("(", "+", this, ")")]
    second: Leaf,
    third: Leaf,
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.expect("ordinal grammar materializes");
        let analysis = crate::analyze(&lowered.grammar);
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let projection = crate::semantic_projection::canonical_semantic_projection(
            &crate::source_analysis::AnalyzedGrammar { lowered, analysis },
        );
        for ordinal in 0..9 {
            assert!(
                projection.contains(&format!("ordinals::Root::element{ordinal}")),
                "projection lacks expanded element ordinal {ordinal}:\n{projection}",
            );
        }
        assert!(!projection.contains("ordinals::Root::element9"));
        assert!(
            projection
                .contains("field ordinals::Root::first -> parse-site(ordinals::Root::element1)")
        );
        assert!(
            projection
                .contains("field ordinals::Root::second -> parse-site(ordinals::Root::element6)")
        );
        assert!(
            projection
                .contains("field ordinals::Root::third -> parse-site(ordinals::Root::element8)")
        );
    }

    #[test]
    fn separator_metadata_reaches_the_normalized_repeat_and_is_repeat_only() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::lists }
#[derive(recursa::Node)] struct Leaf { #[lex(pattern = "x")] text: Text }
type NonEmpty<T> = Option<Box<recursa::Vec1<Box<T>>>>;
#[derive(recursa::Node)]
struct Root { #[sep(",", trailing)] values: NonEmpty<Leaf> }
"#,
        ));
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.unwrap();
        let root = lowered
            .applications
            .iter()
            .find(|application| application.display_name == "lists::Root")
            .unwrap();
        let correlation = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == root.id)
            .unwrap();
        let TypeShape::Sequence { sites } =
            &lowered.grammar.types[correlation.lowered.index()].shape
        else {
            panic!("root is a sequence")
        };
        let site = &lowered.grammar.sites[sites[0].index()];
        let ContainerShape::Optional(optional) = &site.container else {
            panic!("NonEmpty alias keeps optional cardinality")
        };
        let ContainerShape::Boxed(boxed) = optional.as_ref() else {
            panic!("NonEmpty alias keeps Box placement")
        };
        let ContainerShape::Repeat {
            cardinality: crate::RepetitionCardinality::OneOrMore,
            separator: Some(separator),
            ..
        } = boxed.as_ref()
        else {
            panic!("NonEmpty alias keeps one-or-more cardinality")
        };
        assert!(separator.optional_trailing);
        let mut separators = Vec::new();
        site.container
            .visit_separators(&mut |separator| separators.push(separator));
        assert_eq!(separators.len(), 1);
        let MaterializedElement::SemanticField {
            projection:
                MaterializedSemanticFieldProjection::Direct {
                    separator: Some(separator),
                    ..
                },
            ..
        } = root.parsed_fields().next().unwrap()
        else {
            panic!("root field retains separator metadata")
        };
        assert!(separator.optional_trailing);

        let invalid = materialize(resolve(
            r#"
recursa::grammar! { module = crate::lists }
#[derive(recursa::Node)] struct Vec<T> { value: T }
#[derive(recursa::Node)] struct Leaf { this: Self }
#[derive(recursa::Node)] struct Root { #[sep(",")] values: Vec<Leaf> }
"#,
        ));
        assert_eq!(
            invalid.diagnostics[0].kind,
            MaterializationDiagnosticKind::InvalidContainer,
        );
    }

    #[test]
    fn normalized_repeats_keep_cardinality_and_optional_trailing_separately() {
        let report = materialize(resolve(
            r#"
recursa::grammar! { module = crate::lists }
#[derive(recursa::Node)] struct Leaf { #[lex(pattern = "x")] text: Text }
#[derive(recursa::Node)]
struct Root {
    #[sep(",")]
    zero_or_more: Vec<Leaf>,
    #[sep(";", trailing)]
    one_or_more: recursa::Vec1<Leaf>,
}
"#,
        ));

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let lowered = report.lowered.unwrap();
        let root = lowered
            .applications
            .iter()
            .find(|application| application.display_name == "lists::Root")
            .unwrap();
        let root_type = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == root.id)
            .unwrap()
            .lowered;
        let TypeShape::Sequence { sites } = &lowered.grammar.types[root_type.index()].shape else {
            panic!("Root is a sequence")
        };
        let repetitions = sites
            .iter()
            .map(|site| &lowered.grammar.sites[site.index()].container)
            .map(|container| match container {
                ContainerShape::Repeat {
                    cardinality,
                    separator: Some(separator),
                    ..
                } => (*cardinality, separator.optional_trailing),
                _ => panic!("separated field is a normalized repeat"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            repetitions,
            [
                (crate::RepetitionCardinality::ZeroOrMore, false),
                (crate::RepetitionCardinality::OneOrMore, true),
            ],
        );
    }

    #[test]
    fn generic_alias_pretty_applicability_is_checked_per_closed_application() {
        let valid = materialize(resolve(
            r#"
recursa::grammar! { module = crate::pretty, derives(Pretty) }
#[derive(recursa::Node)] struct Leaf { #[lex(pattern = "x")] text: Text }
type Alias<T> = T;
#[derive(recursa::Node)]
struct Generic<T> { #[pretty(group = inconsistent)] values: Alias<T> }
#[derive(recursa::Node)]
struct Root { value: Generic<Vec<Leaf>> }
"#,
        ));
        assert!(valid.diagnostics.is_empty(), "{:?}", valid.diagnostics);
        assert!(valid.lowered.is_some());

        let invalid = materialize(resolve(
            r#"
recursa::grammar! { module = crate::pretty, derives(Pretty) }
#[derive(recursa::Node)] struct Leaf { #[lex(pattern = "x")] text: Text }
type Alias<T> = T;
#[derive(recursa::Node)]
struct Generic<T> { #[pretty(group = consistent)] value: Alias<T> }
#[derive(recursa::Node)]
struct Root { value: Generic<Leaf> }
"#,
        ));
        assert!(invalid.lowered.is_none());
        assert!(invalid.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == MaterializationDiagnosticKind::InapplicablePrettyHint
                && diagnostic
                    .message
                    .contains("materialized break opportunity")
        }));
    }
}
