#![allow(
    dead_code,
    reason = "private M3 materialization seam is consumed by final source lowering"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;

use syn::GenericParam;

use crate::lexical::{
    LexicalDiagnostic, LexicalPlan, TokenDefinitionKindCorrelation, kind_label, plan_lexical,
};
use crate::lowering::{NodeId, TokenUseId};
use crate::resolution::{
    CatalogType, ContentTokenId, GenericParameterId, ResolutionTraceId, ResolvedFieldId,
    ResolvedGenericArgument, ResolvedNode, ResolvedSourceGrammar, ResolvedType, ResolvedTypeKind,
    ResolvedVariantId, SemanticOwner, SemanticTarget, SemanticUseId, TypeAliasId,
};
use crate::source_model::{
    ConfiguredTraitRecipe, DeclaredRepetition, GrammarElement, MaterializedPrettyBoundary,
    NodeBody, Separator, SourceModel, SourceModeledGrammar, SourcePrattPosition,
    SourcePrattVariant, materialized_pretty_boundary_changes_lexing,
};
use crate::{
    AtomDescription, AtomExpression, ContainerShape, DeclId, Declaration, GrammarDescription,
    KindDescription, Location, ParseSite, PrattPosition, RepeatSeparator, SiteId, SourceId,
    TypeDescription, TypeId, TypeShape,
};

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
pub(crate) struct MaterializationReport {
    pub(crate) lexical_diagnostics: Vec<LexicalDiagnostic>,
    pub(crate) diagnostics: Vec<MaterializationDiagnostic>,
    pub(crate) lowered: Option<LoweredGrammar>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoweredGrammar {
    pub(crate) source_modeled: SourceModeledGrammar,
    pub(crate) lexical_plan: LexicalPlan,
    pub(crate) applications: Vec<MaterializedNodeApplication>,
    pub(crate) grammar: GrammarDescription,
    pub(crate) lowering_map: LoweringMap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoweringMap {
    pub(crate) sources: Vec<SourceCorrelation>,
    pub(crate) token_definitions: Vec<TokenDefinitionKindCorrelation>,
    pub(crate) content_targets: Vec<ContentTargetTypeCorrelation>,
    pub(crate) applications: Vec<ApplicationTypeCorrelation>,
    pub(crate) variants: Vec<VariantAlternativeCorrelation>,
    pub(crate) fields: Vec<MaterializedFieldCorrelation>,
    pub(crate) attachments: Vec<AttachmentUnitCorrelation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ContentTargetTypeCorrelation {
    pub(crate) semantic: ContentTokenId,
    pub(crate) lowered: TypeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceCorrelation {
    pub(crate) resolved: SourceId,
    pub(crate) lowered: SourceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ApplicationTypeCorrelation {
    pub(crate) application: MaterializedApplicationId,
    pub(crate) node: NodeId,
    pub(crate) lowered: TypeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedFieldCorrelation {
    pub(crate) application: MaterializedApplicationId,
    pub(crate) field: ResolvedFieldId,
    pub(crate) lowered: MaterializedFieldLowering,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaterializedFieldLowering {
    ParseSite(SiteId),
    PrattLhs { owner: TypeId, variant: DeclId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttachmentUnitCorrelation {
    pub(crate) application: MaterializedApplicationId,
    pub(crate) field: ResolvedFieldId,
    pub(crate) outer: SiteId,
    pub(crate) unit: TypeId,
    pub(crate) sites: Vec<SiteId>,
    pub(crate) semantic_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VariantAlternativeCorrelation {
    pub(crate) application: MaterializedApplicationId,
    pub(crate) variant: ResolvedVariantId,
    pub(crate) lowered: DeclId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedNodeApplication {
    pub(crate) id: MaterializedApplicationId,
    pub(crate) node: NodeId,
    pub(crate) display_name: String,
    pub(crate) type_arguments: Vec<ClosedType>,
    pub(crate) pretty: crate::source_model::PrettyHints,
    pub(crate) body: MaterializedNodeBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MaterializedNodeBody {
    Struct(Vec<MaterializedElement>),
    Presence(Vec<MaterializedElement>),
    Enum(Vec<MaterializedVariant>),
    Pratt(Vec<MaterializedPrattVariant>),
}

impl MaterializedNodeBody {
    fn parsed_elements(&self) -> impl Iterator<Item = &MaterializedElement> {
        let mut elements = Vec::new();
        match self {
            Self::Struct(direct) | Self::Presence(direct) => elements.extend(direct),
            Self::Enum(variants) => {
                elements.extend(variants.iter().flat_map(|variant| &variant.elements));
            }
            Self::Pratt(variants) => {
                elements.extend(
                    variants
                        .iter()
                        .flat_map(MaterializedPrattVariant::parsed_elements),
                );
            }
        }
        elements.into_iter()
    }

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
pub(crate) struct MaterializedVariant {
    pub(crate) resolved_variant: ResolvedVariantId,
    pub(crate) name: String,
    pub(crate) pretty: crate::source_model::PrettyHints,
    pub(crate) elements: Vec<MaterializedElement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MaterializedPrattVariant {
    Nud(MaterializedPrattNud),
    Led(MaterializedPrattLed),
}

impl MaterializedPrattVariant {
    fn common(&self) -> &MaterializedPrattCommon {
        match self {
            Self::Nud(variant) => &variant.common,
            Self::Led(variant) => &variant.common,
        }
    }

    fn parsed_elements(&self) -> std::slice::Iter<'_, MaterializedElement> {
        match self {
            Self::Nud(variant) => variant.body.iter(),
            Self::Led(variant) => variant.tail.iter(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedPrattCommon {
    pub(crate) resolved_variant: ResolvedVariantId,
    pub(crate) name: String,
    pub(crate) pretty: crate::source_model::PrettyHints,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedPrattNud {
    pub(crate) common: MaterializedPrattCommon,
    pub(crate) position: MaterializedPrattNudPosition,
    pub(crate) body: Vec<MaterializedElement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaterializedPrattNudPosition {
    Atom,
    Prefix { bp: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedPrattLed {
    pub(crate) common: MaterializedPrattCommon,
    pub(crate) position: MaterializedPrattLedPosition,
    pub(crate) lhs: MaterializedPrattLhs,
    pub(crate) tail: Vec<MaterializedElement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaterializedPrattLedPosition {
    Infix { lbp: u8, rbp: u8 },
    Postfix { bp: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedPrattLhs {
    pub(crate) field: ResolvedFieldId,
    pub(crate) owner: MaterializedNodeUse,
    pub(crate) trace: MaterializationTrace,
    pub(crate) pretty: crate::source_model::PrettyHints,
    pub(crate) attachment_before: usize,
    pub(crate) attachment_after: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MaterializedElement {
    FixedToken {
        token_use: TokenUseId,
    },
    SemanticField {
        field: ResolvedFieldId,
        ty: ClosedType,
        trace: MaterializationTrace,
        attachment: Option<MaterializedFixedAttachment>,
        separator: Option<Separator>,
        pretty: crate::source_model::PrettyHints,
        attachment_before: usize,
        attachment_after: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedFixedAttachment {
    pub(crate) before: Vec<TokenUseId>,
    pub(crate) after: Vec<TokenUseId>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum MaterializedFieldView<'a> {
    Parsed {
        field: ResolvedFieldId,
        ty: &'a ClosedType,
        trace: &'a MaterializationTrace,
        separator: Option<&'a Separator>,
    },
    PrattLhs(&'a MaterializedPrattLhs),
}

impl<'a> MaterializedFieldView<'a> {
    fn parsed(element: &'a MaterializedElement) -> Option<Self> {
        match element {
            MaterializedElement::SemanticField {
                field,
                ty,
                trace,
                separator,
                ..
            } => Some(Self::Parsed {
                field: *field,
                ty,
                trace,
                separator: separator.as_ref(),
            }),
            MaterializedElement::FixedToken { .. } => None,
        }
    }
}

impl MaterializedNodeApplication {
    pub(crate) fn fields(&self) -> impl Iterator<Item = MaterializedFieldView<'_>> {
        self.body.fields()
    }

    pub(crate) fn parsed_fields(&self) -> impl Iterator<Item = &MaterializedElement> {
        self.body
            .parsed_elements()
            .filter(|element| matches!(element, MaterializedElement::SemanticField { .. }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClosedType {
    Node(MaterializedNodeUse),
    ContentToken(ContentTokenId),
    Boxed(Box<Self>),
    Optional(Box<Self>),
    ZeroOrMore(Box<Self>),
    OneOrMore(Box<Self>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedNodeUse {
    pub(crate) application: MaterializedApplicationId,
    pub(crate) lifetimes: Vec<syn::Lifetime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializationTrace {
    pub(crate) steps: Vec<MaterializationTraceStep>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaterializationTraceStep {
    Use(SemanticUseId),
    Resolution(ResolutionTraceId),
    Alias(TypeAliasId),
    Parameter(GenericParameterId),
    SelfApplication(MaterializedApplicationId),
    Catalog(CatalogType),
    Application(MaterializedApplicationId),
    ContentToken(ContentTokenId),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum MaterializationDiagnosticKind {
    OpenApplication,
    ConstGeneric,
    UnsupportedType,
    InvalidArguments,
    InvalidContainer,
    InvalidPrattShape,
    InapplicablePrettyHint,
    UnsafePrettyBoundary,
    AliasCycle,
    NonRegularRecursion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializationDiagnostic {
    pub(crate) kind: MaterializationDiagnosticKind,
    pub(crate) module: Option<crate::lowering::ModuleId>,
    pub(crate) logical_module: Option<Vec<String>>,
    pub(crate) path: PathBuf,
    pub(crate) range: Option<Range<u32>>,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ApplicationKey {
    node: NodeId,
    arguments: Vec<Vec<TypeIdentityAtom>>,
}

#[derive(Clone)]
struct AttachmentUnitProjection {
    id: TypeId,
    application: MaterializedApplicationId,
    field: ResolvedFieldId,
    ty: ClosedType,
    attachment: MaterializedFixedAttachment,
    recursive_min_bp: u8,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TypeIdentityAtom {
    Node(MaterializedApplicationId),
    ContentToken(ContentTokenId),
    Boxed,
    Optional,
    ZeroOrMore,
    OneOrMore,
}

impl ClosedType {
    fn identity(&self) -> Vec<TypeIdentityAtom> {
        let mut identity = Vec::new();
        self.extend_identity(&mut identity);
        identity
    }

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
struct EvaluatedType {
    closed: ClosedType,
    trace: Vec<MaterializationTraceStep>,
}

#[derive(Clone, Default)]
struct EvaluationEnvironment {
    types: BTreeMap<GenericParameterId, EvaluatedType>,
    lifetimes: BTreeMap<String, syn::Lifetime>,
}

struct Materializer {
    source_modeled: SourceModeledGrammar,
    lexical_plan: Option<LexicalPlan>,
    application_by_key: BTreeMap<ApplicationKey, MaterializedApplicationId>,
    applications: Vec<MaterializedNodeApplication>,
    diagnostics: Vec<MaterializationDiagnostic>,
    active_aliases: Vec<TypeAliasId>,
    active_applications: Vec<ApplicationKey>,
}

#[derive(Clone, Debug, Default)]
struct PrettyBoundarySummary {
    first: Vec<MaterializedPrettyBoundary>,
    last: Vec<MaterializedPrettyBoundary>,
    nullable: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PrettyBoundarySite {
    Fixed(TokenUseId),
    Field(ResolvedFieldId),
}

pub(crate) fn materialize(source_modeled: SourceModeledGrammar) -> MaterializationReport {
    let lexical = plan_lexical(&source_modeled);
    let materializer = Materializer {
        source_modeled,
        lexical_plan: lexical.plan,
        application_by_key: BTreeMap::new(),
        applications: Vec::new(),
        diagnostics: Vec::new(),
        active_aliases: Vec::new(),
        active_applications: Vec::new(),
    };
    materializer.run(lexical.diagnostics)
}

impl Materializer {
    fn lexical_plan(&self) -> &LexicalPlan {
        self.lexical_plan
            .as_ref()
            .expect("M2 projection runs only after successful lexical planning")
    }

    fn resolved(&self) -> &ResolvedSourceGrammar {
        self.source_modeled.resolved()
    }

    fn source_model(&self) -> &SourceModel {
        self.source_modeled.source_model()
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
        if self.diagnostics.is_empty() && lexical_diagnostics.is_empty() {
            self.validate_closed_pretty_boundaries();
        }
        if !self.diagnostics.is_empty() {
            return MaterializationReport {
                lexical_diagnostics,
                diagnostics: std::mem::take(&mut self.diagnostics),
                lowered: None,
            };
        }
        if !lexical_diagnostics.is_empty() {
            return MaterializationReport {
                lexical_diagnostics,
                diagnostics: Vec::new(),
                lowered: None,
            };
        }
        let (grammar, lowering_map) = self.project_m2();
        MaterializationReport {
            lexical_diagnostics: Vec::new(),
            diagnostics: Vec::new(),
            lowered: Some(LoweredGrammar {
                source_modeled: self.source_modeled,
                lexical_plan: self
                    .lexical_plan
                    .expect("successful lexical planning precedes M2 projection"),
                applications: self.applications,
                grammar,
                lowering_map,
            }),
        }
    }

    fn validate_closed_pretty_boundaries(&mut self) {
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

    fn sequence_pretty_lexing_violation(
        &self,
        elements: &[MaterializedElement],
    ) -> Option<PrettyBoundarySite> {
        let mut trailing = Vec::new();
        for element in elements {
            if self.attachment_changes_pretty_lexing(element) {
                return Some(Self::element_pretty_boundary_site(element));
            }
            let summary = self.element_pretty_boundaries(element, &mut BTreeSet::new());
            if self.pretty_boundary_pairs_change_lexing(&trailing, &summary.first) {
                return Some(Self::element_pretty_boundary_site(element));
            }
            if summary.nullable {
                trailing.extend(summary.last);
            } else {
                trailing = summary.last;
            }
        }
        None
    }

    fn element_pretty_boundary_site(element: &MaterializedElement) -> PrettyBoundarySite {
        match element {
            MaterializedElement::FixedToken { token_use } => PrettyBoundarySite::Fixed(*token_use),
            MaterializedElement::SemanticField { field, .. } => PrettyBoundarySite::Field(*field),
        }
    }

    fn attachment_changes_pretty_lexing(&self, element: &MaterializedElement) -> bool {
        let MaterializedElement::SemanticField {
            ty,
            attachment: Some(attachment),
            ..
        } = element
        else {
            return false;
        };
        let mut summaries = attachment
            .before
            .iter()
            .map(|token_use| self.fixed_pretty_boundaries(*token_use))
            .collect::<Vec<_>>();
        summaries
            .push(self.closed_pretty_boundaries(attachment_terminal(ty), &mut BTreeSet::new()));
        summaries.extend(
            attachment
                .after
                .iter()
                .map(|token_use| self.fixed_pretty_boundaries(*token_use)),
        );
        let mut trailing = Vec::new();
        for summary in summaries {
            if self.pretty_boundary_pairs_change_lexing(&trailing, &summary.first) {
                return true;
            }
            if summary.nullable {
                trailing.extend(summary.last);
            } else {
                trailing = summary.last;
            }
        }
        false
    }

    fn pretty_boundary_pairs_change_lexing(
        &self,
        left: &[MaterializedPrettyBoundary],
        right: &[MaterializedPrettyBoundary],
    ) -> bool {
        let equivalence = self.resolved().configuration.syntax.text_equivalence;
        left.iter().any(|left| {
            right.iter().any(|right| {
                materialized_pretty_boundary_changes_lexing(
                    self.source_model(),
                    equivalence,
                    left,
                    right,
                )
            })
        })
    }

    fn element_pretty_boundaries(
        &self,
        element: &MaterializedElement,
        active: &mut BTreeSet<MaterializedApplicationId>,
    ) -> PrettyBoundarySummary {
        match element {
            MaterializedElement::FixedToken { token_use } => {
                self.fixed_pretty_boundaries(*token_use)
            }
            MaterializedElement::SemanticField { ty, attachment, .. } => {
                let mut summary = self.closed_pretty_boundaries(ty, active);
                if let Some(attachment) = attachment {
                    if let Some(first) = attachment.before.first() {
                        summary.first = self.fixed_pretty_boundaries(*first).first;
                    }
                    if let Some(last) = attachment.after.last() {
                        summary.last = self.fixed_pretty_boundaries(*last).last;
                    }
                }
                summary
            }
        }
    }

    fn fixed_pretty_boundaries(&self, token_use: TokenUseId) -> PrettyBoundarySummary {
        let definition = self.source_model().token_uses[token_use.index()].definition;
        let boundary = MaterializedPrettyBoundary::Fixed(definition);
        PrettyBoundarySummary {
            first: vec![boundary.clone()],
            last: vec![boundary],
            nullable: false,
        }
    }

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
                    first: vec![boundary.clone()],
                    last: vec![boundary],
                    nullable: false,
                }
            }
            ClosedType::Node(node) => {
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

    fn elements_pretty_boundaries(
        &self,
        elements: &[MaterializedElement],
        active: &mut BTreeSet<MaterializedApplicationId>,
    ) -> PrettyBoundarySummary {
        let summaries = elements
            .iter()
            .map(|element| self.element_pretty_boundaries(element, active))
            .collect::<Vec<_>>();
        let mut first = Vec::new();
        for summary in &summaries {
            first.extend(summary.first.clone());
            if !summary.nullable {
                break;
            }
        }
        let mut last = Vec::new();
        for summary in summaries.iter().rev() {
            last.extend(summary.last.clone());
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

    fn alternative_pretty_boundaries(
        &self,
        summaries: impl Iterator<Item = PrettyBoundarySummary>,
    ) -> PrettyBoundarySummary {
        let mut combined = PrettyBoundarySummary::default();
        for summary in summaries {
            combined.first.extend(summary.first);
            combined.last.extend(summary.last);
            combined.nullable |= summary.nullable;
        }
        combined
    }

    fn ensure_application(
        &mut self,
        node_id: NodeId,
        type_arguments: Vec<EvaluatedType>,
    ) -> Option<MaterializedApplicationId> {
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
                if !self.validate_single_pratt_operand(
                    common.resolved_variant,
                    current_application,
                    &elements,
                    "infix rhs",
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

    fn take_pratt_lhs(
        &mut self,
        variant: ResolvedVariantId,
        current_application: &MaterializedNodeUse,
        elements: &mut Vec<MaterializedElement>,
    ) -> Option<MaterializedPrattLhs> {
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
            matches!(element, MaterializedElement::SemanticField { field, .. } if *field == lhs_field)
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
            ty,
            trace,
            attachment,
            separator,
            pretty,
            attachment_before,
            attachment_after,
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
        if let Some(attachment) = attachment {
            elements.splice(
                index..index,
                attachment
                    .after
                    .into_iter()
                    .map(|token_use| MaterializedElement::FixedToken { token_use }),
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

    fn materialize_elements(
        &mut self,
        elements: &[GrammarElement],
        current_application: &MaterializedNodeUse,
        environment: &EvaluationEnvironment,
    ) -> Vec<MaterializedElement> {
        let mut materialized = Vec::new();
        for element in elements {
            match element {
                GrammarElement::FixedToken { token_use, .. } => {
                    materialized.push(MaterializedElement::FixedToken {
                        token_use: *token_use,
                    });
                }
                GrammarElement::SemanticField {
                    resolved_field,
                    attachment,
                    separator,
                    pretty,
                    attachment_before,
                    attachment_after,
                    ..
                } => {
                    let field = self.resolved_field(*resolved_field).clone();
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
                        ty: evaluated.closed,
                        trace: MaterializationTrace {
                            steps: evaluated.trace,
                        },
                        attachment: attachment.as_ref().map(|attachment| {
                            MaterializedFixedAttachment {
                                before: attachment
                                    .before
                                    .iter()
                                    .map(|fixed| fixed.token_use)
                                    .collect(),
                                after: attachment
                                    .after
                                    .iter()
                                    .map(|fixed| fixed.token_use)
                                    .collect(),
                            }
                        }),
                        separator: separator.clone(),
                        pretty: pretty.clone(),
                        attachment_before: *attachment_before,
                        attachment_after: *attachment_after,
                    });
                }
            }
        }
        materialized
    }

    fn resolved_field(&self, id: ResolvedFieldId) -> &crate::resolution::ResolvedNodeField {
        self.resolved()
            .nodes
            .iter()
            .flat_map(|node| &node.fields)
            .find(|field| field.id == id)
            .expect("source-model field identity belongs to resolved grammar")
    }

    fn evaluate_type(
        &mut self,
        ty: &ResolvedType,
        current_application: Option<MaterializedNodeUse>,
        environment: &EvaluationEnvironment,
    ) -> Option<EvaluatedType> {
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
            ResolvedTypeKind::Reference(_)
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
    fn evaluate_alias(
        &mut self,
        alias: TypeAliasId,
        supplied: &[ResolvedGenericArgument],
        use_type: &ResolvedType,
        current_application: Option<MaterializedNodeUse>,
        caller_environment: &EvaluationEnvironment,
        prefix: Vec<MaterializationTraceStep>,
    ) -> Option<EvaluatedType> {
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

    fn project_m2(&self) -> (GrammarDescription, LoweringMap) {
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
            let kind = grammar.push_kind(KindDescription::new(&name, declaration));
            assert_eq!(
                kind, kind_plan.id,
                "M2 KindId must equal the lexical KindId"
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
        let attachment_units = self.attachment_unit_projections(terminal_type_count);
        let attachment_types = attachment_units
            .iter()
            .map(|unit| ((unit.application, unit.field), unit.id))
            .collect::<BTreeMap<_, _>>();
        let applications = self
            .applications
            .iter()
            .map(|application| ApplicationTypeCorrelation {
                application: application.id,
                node: application.node,
                lowered: TypeId::new(
                    (terminal_type_count + application.id.index())
                        .try_into()
                        .expect("M2 type ID overflow"),
                ),
            })
            .collect::<Vec<_>>();
        let mut field_correlations = Vec::new();
        let mut variant_correlations = Vec::new();
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
                    .expect("M2 type ID overflow"),
            );
            let shape = match &application.body {
                MaterializedNodeBody::Struct(elements)
                | MaterializedNodeBody::Presence(elements) => TypeShape::Sequence {
                    sites: self.project_elements(
                        &mut grammar,
                        application,
                        owner,
                        elements,
                        terminal_type_count,
                        &fixed_types,
                        &content_types,
                        &attachment_types,
                        &mut field_correlations,
                        0,
                    ),
                },
                MaterializedNodeBody::Enum(variants) => TypeShape::Enum {
                    alternatives: variants
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
                            variant_correlations.push(VariantAlternativeCorrelation {
                                application: application.id,
                                variant: variant.resolved_variant,
                                lowered: declaration,
                            });
                            crate::Alternative {
                                declaration,
                                sites: self.project_elements(
                                    &mut grammar,
                                    application,
                                    owner,
                                    &variant.elements,
                                    terminal_type_count,
                                    &fixed_types,
                                    &content_types,
                                    &attachment_types,
                                    &mut field_correlations,
                                    0,
                                ),
                            }
                        })
                        .collect(),
                },
                MaterializedNodeBody::Pratt(variants) => TypeShape::Pratt {
                    variants: variants
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
                            variant_correlations.push(VariantAlternativeCorrelation {
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
                                            &mut grammar,
                                            application,
                                            owner,
                                            &nud.body,
                                            terminal_type_count,
                                            &fixed_types,
                                            &content_types,
                                            &attachment_types,
                                            &mut field_correlations,
                                            min_bp,
                                        ),
                                    }
                                }
                                MaterializedPrattVariant::Led(led) => {
                                    field_correlations.push(MaterializedFieldCorrelation {
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
                                            (PrattPosition::Postfix { bp }, 0)
                                        }
                                    };
                                    crate::PrattVariant {
                                        declaration,
                                        position,
                                        sites: self.project_elements(
                                            &mut grammar,
                                            application,
                                            owner,
                                            &led.tail,
                                            terminal_type_count,
                                            &fixed_types,
                                            &content_types,
                                            &attachment_types,
                                            &mut field_correlations,
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
        for unit in &attachment_units {
            assert_eq!(grammar.types.len(), unit.id.index());
            let sites = self.project_attachment_unit(
                &mut grammar,
                unit,
                terminal_type_count,
                &fixed_types,
                &content_types,
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
        let attachment_correlations = attachment_units
            .iter()
            .map(|unit| {
                let outer = field_correlations
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
                applications,
                variants: variant_correlations,
                fields: field_correlations,
                attachments: attachment_correlations,
            },
        )
    }

    fn attachment_unit_projections(
        &self,
        terminal_type_count: usize,
    ) -> Vec<AttachmentUnitProjection> {
        let mut units = Vec::new();
        let mut collect = |application: &MaterializedNodeApplication,
                           elements: &[MaterializedElement],
                           recursive_min_bp: u8| {
            for element in elements {
                let MaterializedElement::SemanticField {
                    field,
                    ty,
                    attachment: Some(attachment),
                    ..
                } = element
                else {
                    continue;
                };
                let id = TypeId::new(
                    (terminal_type_count + self.applications.len() + units.len())
                        .try_into()
                        .expect("M2 attachment type ID overflow"),
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
            match &application.body {
                MaterializedNodeBody::Struct(elements)
                | MaterializedNodeBody::Presence(elements) => collect(application, elements, 0),
                MaterializedNodeBody::Enum(variants) => {
                    for variant in variants {
                        collect(application, &variant.elements, 0);
                    }
                }
                MaterializedNodeBody::Pratt(variants) => {
                    for variant in variants {
                        match variant {
                            MaterializedPrattVariant::Nud(nud) => {
                                let min_bp = match nud.position {
                                    MaterializedPrattNudPosition::Atom => 0,
                                    MaterializedPrattNudPosition::Prefix { bp } => bp,
                                };
                                collect(application, &nud.body, min_bp);
                            }
                            MaterializedPrattVariant::Led(led) => {
                                let min_bp = match led.position {
                                    MaterializedPrattLedPosition::Infix { rbp, .. } => rbp,
                                    MaterializedPrattLedPosition::Postfix { .. } => 0,
                                };
                                collect(application, &led.tail, min_bp);
                            }
                        }
                    }
                }
            }
        }
        units
    }

    fn project_attachment_unit(
        &self,
        grammar: &mut GrammarDescription,
        unit: &AttachmentUnitProjection,
        terminal_type_count: usize,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        content_types: &BTreeMap<ContentTokenId, TypeId>,
    ) -> Vec<SiteId> {
        let application = &self.applications[unit.application.index()];
        let mut sites = Vec::new();
        let push_fixed =
            |grammar: &mut GrammarDescription, sites: &mut Vec<SiteId>, token_use: TokenUseId| {
                let token_use = &self.source_model().token_uses[token_use.index()];
                let location = grammar.push_location(Location::new(
                    token_use.site.source,
                    token_use.site.range.clone(),
                ));
                let declaration = grammar.push_declaration(Declaration::new(
                    format!(
                        "{}::__recursa_attachment_element{}",
                        application.display_name,
                        sites.len()
                    ),
                    location,
                ));
                sites.push(grammar.push_site(ParseSite::new(
                    unit.id,
                    fixed_types[&self.kind_for_definition(token_use.definition)],
                    declaration,
                    ContainerShape::Direct,
                )));
            };
        for token_use in &unit.attachment.before {
            push_fixed(grammar, &mut sites, *token_use);
        }
        let field = self.resolved_field(unit.field);
        let location = grammar.push_location(Location::new(field.ty.source, field.range.clone()));
        let declaration = grammar.push_declaration(Declaration::new(
            format!(
                "{}::__recursa_attachment_element{}",
                application.display_name,
                sites.len()
            ),
            location,
        ));
        let (terminal, _) = terminal_and_container(&unit.ty, None);
        let target = match terminal {
            Terminal::Node(application) => TypeId::new(
                (terminal_type_count + application.index())
                    .try_into()
                    .expect("M2 type ID overflow"),
            ),
            Terminal::ContentToken(token) => content_types[&token],
        };
        let mut semantic = ParseSite::new(unit.id, target, declaration, ContainerShape::Direct);
        if is_recursive_pratt_operand(&unit.ty, unit.application) {
            semantic = semantic.with_min_bp(unit.recursive_min_bp);
        }
        sites.push(grammar.push_site(semantic));
        for token_use in &unit.attachment.after {
            push_fixed(grammar, &mut sites, *token_use);
        }
        sites
    }

    #[allow(clippy::too_many_arguments)]
    fn project_elements(
        &self,
        grammar: &mut GrammarDescription,
        application: &MaterializedNodeApplication,
        owner: TypeId,
        elements: &[MaterializedElement],
        terminal_type_count: usize,
        fixed_types: &BTreeMap<crate::KindId, TypeId>,
        content_types: &BTreeMap<ContentTokenId, TypeId>,
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
                MaterializedElement::SemanticField {
                    field,
                    ty,
                    attachment,
                    separator,
                    ..
                } => {
                    let resolved_field = self.resolved_field(*field);
                    let location = grammar.push_location(Location::new(
                        resolved_field.ty.source,
                        resolved_field.range.clone(),
                    ));
                    let declaration = grammar.push_declaration(Declaration::new(
                        format!("{}::element{index}", application.display_name),
                        location,
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
                                .expect("M2 type ID overflow"),
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

    fn push_invalid_arguments(&mut self, ty: &ResolvedType, name: &str) {
        self.push_diagnostic(
            ty.source,
            Some(ty.range.clone()),
            MaterializationDiagnosticKind::InvalidArguments,
            format!("invalid semantic arguments for `{name}`"),
        );
    }

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
enum Terminal {
    Node(MaterializedApplicationId),
    ContentToken(ContentTokenId),
}

fn exact_pratt_self_operand(
    element: &MaterializedElement,
    current_application: &MaterializedNodeUse,
) -> bool {
    let MaterializedElement::SemanticField {
        ty,
        trace,
        separator,
        ..
    } = element
    else {
        return false;
    };
    exact_pratt_self_parts(ty, trace, separator.as_ref(), current_application)
}

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

fn attachment_terminal(ty: &ClosedType) -> &ClosedType {
    match ty {
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => attachment_terminal(inner),
        ClosedType::Node(_) | ClosedType::ContentToken(_) => ty,
    }
}

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
        MaterializedPrattNud, MaterializedPrattNudPosition, MaterializedPrattVariant, materialize,
    };
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::RustEdition;
    use crate::resolution::{ResolutionInput, resolve_local_semantics};
    use crate::source_model::{SourceModelInput, lower_source_model};
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
        assert_eq!(crate::m3::materialization_code(diagnostic.kind), "RCA3211");
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

        let trivia_safe_report = materialize(resolve(
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
        assert!(
            trivia_safe_report.diagnostics.is_empty(),
            "{:?}",
            trivia_safe_report.diagnostics
        );
        assert!(trivia_safe_report.lowered.is_some());
    }

    fn assert_code(
        diagnostic: &super::MaterializationDiagnostic,
        kind: MaterializationDiagnosticKind,
        code: &str,
    ) {
        assert_eq!(diagnostic.kind, kind);
        assert_eq!(crate::m3::materialization_code(diagnostic.kind), code);
    }

    #[test]
    fn contextual_keyword_targets_are_concrete_m2_kind_sets_before_analysis() {
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
                "{} must reach M2 as its exact base-plus-admissions set",
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
                    .expect("every parser target reaches M2")
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
                MaterializedElement::SemanticField { ty, trace, .. } => (ty, trace),
                MaterializedElement::FixedToken { .. } => unreachable!("filtered semantic field"),
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
    fn preserves_box_placement_and_exact_supported_cardinalities_in_m2() {
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
    fn rejects_open_unsupported_cyclic_and_invalid_closed_types_before_m2() {
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
        assert!(
            invalid_containers.diagnostics.iter().all(|diagnostic| {
                crate::m3::materialization_code(diagnostic.kind) == "RCA3206"
            })
        );

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
                .map(|diagnostic| crate::m3::materialization_code(diagnostic.kind))
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
    fn arithmetic_generic_application_reaches_a_successful_m2_analysis() {
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
    fn all_atom_pratt_remains_pratt_in_m2() {
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
        assert_eq!(variants[2].sites.len(), 2, "lhs is not an M2 parse site");
        assert_eq!(variants[3].sites.len(), 1, "lhs is not an M2 parse site");
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
            assert_eq!(crate::m3::materialization_code(diagnostic.kind), "RCA3209");
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
    fn projects_field_attachments_as_private_m2_sequence_units() {
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
        let projection =
            crate::m3_projection::canonical_semantic_projection(&crate::m3::AnalyzedGrammar {
                lowered: lowered.clone(),
                analysis: analysis.clone(),
            });
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
        let projection =
            crate::m3_projection::canonical_semantic_projection(&crate::m3::AnalyzedGrammar {
                lowered,
                analysis,
            });
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
        let projection =
            crate::m3_projection::canonical_semantic_projection(&crate::m3::AnalyzedGrammar {
                lowered,
                analysis,
            });
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
            separator: Some(separator),
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
