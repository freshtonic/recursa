#![allow(
    dead_code,
    reason = "private M3 source-model seam is consumed by later pipeline tickets"
)]

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

use proc_macro2::Span;
use syn::ext::IdentExt as _;
use syn::parse::{Parse, ParseStream, Parser as _};
use syn::spanned::Spanned as _;
use syn::{Item, LitStr, Meta, Token};

use crate::lowering::{ElementId as GrammarElementId, NodeId, TokenDefinitionId, TokenUseId};
use crate::resolution::{
    CatalogType, ContentTokenId, ResolvedFieldId, ResolvedNodeBody, ResolvedNodeField,
    ResolvedSourceGrammar, ResolvedTypeKind, ResolvedVariantId, SemanticTarget,
};
use crate::source_syntax::{
    CargoCfgSnapshot, configured_attribute_metas as configured_metas,
    item_attributes as item_attrs, path_text, span_range,
};

#[derive(Clone, Debug)]
pub(crate) struct SourceModelInput<'cfg> {
    pub(crate) resolved: ResolvedSourceGrammar,
    pub(crate) cfg: &'cfg CargoCfgSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceModelReport {
    pub(crate) diagnostics: Vec<SourceModelDiagnostic>,
    pub(crate) source_modeled: Option<SourceModeledGrammar>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceModeledGrammar {
    resolved: ResolvedSourceGrammar,
    source_model: SourceModel,
}

impl SourceModeledGrammar {
    pub(crate) const fn resolved(&self) -> &ResolvedSourceGrammar {
        &self.resolved
    }

    pub(crate) const fn source_model(&self) -> &SourceModel {
        &self.source_model
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceModel {
    pub(crate) grammar_trait_defaults: Vec<ConfiguredTraitSelection>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) token_definitions: Vec<TokenDefinition>,
    pub(crate) token_uses: Vec<TokenUse>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Node {
    pub(crate) resolved_node: NodeId,
    pub(crate) name: String,
    pub(crate) body: NodeBody,
    pub(crate) configured_traits: ConfiguredTraitPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NodeBody {
    Struct(Vec<GrammarElement>),
    Presence(Vec<GrammarElement>),
    Enum(Vec<Variant>),
}

impl Node {
    pub(crate) const fn shape(&self) -> NodeShape {
        match self.body {
            NodeBody::Struct(_) => NodeShape::Struct,
            NodeBody::Presence(_) => NodeShape::Presence,
            NodeBody::Enum(_) => NodeShape::Enum,
        }
    }

    pub(crate) fn elements(&self) -> &[GrammarElement] {
        match &self.body {
            NodeBody::Struct(elements) | NodeBody::Presence(elements) => elements,
            NodeBody::Enum(_) => &[],
        }
    }

    pub(crate) fn variants(&self) -> &[Variant] {
        match &self.body {
            NodeBody::Enum(variants) => variants,
            NodeBody::Struct(_) | NodeBody::Presence(_) => &[],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Variant {
    pub(crate) resolved_variant: ResolvedVariantId,
    pub(crate) name: String,
    pub(crate) elements: Vec<GrammarElement>,
    pub(crate) pratt_role: Option<PrattRole>,
    pub(crate) site: SourceSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrattRole {
    Prefix,
    Infix,
    Postfix,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConfiguredTraitPlan {
    pub(crate) effective: Vec<ConfiguredTraitSelection>,
    pub(crate) additions: Vec<ConfiguredTraitSelection>,
    pub(crate) omissions: Vec<ConfiguredTraitOmission>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConfiguredTraitSelection {
    pub(crate) recipe: ConfiguredTraitRecipe,
    pub(crate) ownership: ConfiguredTraitOwnership,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConfiguredTraitOmission {
    pub(crate) recipe: ConfiguredTraitRecipe,
    pub(crate) ownership: ConfiguredTraitOwnership,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ConfiguredTraitRecipe {
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Pretty,
    Visit,
    VisitMut,
    Arbitrary,
    Serialize,
    Deserialize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfiguredTraitOwnership {
    GrammarDefault,
    LocalAddition,
    LocalOmission,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NodeShape {
    Struct,
    Enum,
    Presence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GrammarElement {
    FixedToken {
        id: GrammarElementId,
        token_use: TokenUseId,
        site: SourceSite,
    },
    SemanticField {
        id: GrammarElementId,
        resolved_field: ResolvedFieldId,
        name: Option<String>,
        separator: Option<Separator>,
        site: SourceSite,
    },
}

impl GrammarElement {
    pub(crate) const fn id(&self) -> GrammarElementId {
        match self {
            Self::FixedToken { id, .. } | Self::SemanticField { id, .. } => *id,
        }
    }

    fn set_id(&mut self, value: GrammarElementId) {
        match self {
            Self::FixedToken { id, .. } | Self::SemanticField { id, .. } => *id = value,
        }
    }

    pub(crate) const fn kind(&self) -> GrammarElementKind {
        match self {
            Self::FixedToken { .. } => GrammarElementKind::FixedToken,
            Self::SemanticField { .. } => GrammarElementKind::SemanticField,
        }
    }

    pub(crate) fn separator(&self) -> Option<&Separator> {
        match self {
            Self::SemanticField { separator, .. } => separator.as_ref(),
            Self::FixedToken { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Separator {
    pub(crate) token_use: TokenUseId,
    pub(crate) declared_repetition: Option<DeclaredRepetition>,
    pub(crate) optional_trailing: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeclaredRepetition {
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GrammarElementKind {
    FixedToken,
    SemanticField,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TokenDefinition {
    pub(crate) id: TokenDefinitionId,
    pub(crate) name: String,
    pub(crate) kind: TokenDefinitionKind,
    pub(crate) definition_sites: Vec<SourceSite>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TokenDefinitionKind {
    Fixed(FixedTokenDefinition),
    Content(ContentTokenDefinition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FixedTokenDefinition {
    pub(crate) spelling: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContentTokenDefinition {
    pub(crate) semantic: ContentTokenId,
    pub(crate) pattern: String,
    pub(crate) admission_set: Vec<TokenDefinitionId>,
    pub(crate) authored_admissions: Vec<AuthoredAdmissions>,
    pub(crate) text: ContentTextStorage,
    pub(crate) configured_traits: ConfiguredTraitPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthoredAdmissions {
    pub(crate) site: SourceSite,
    pub(crate) definitions: Vec<TokenDefinitionId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContentTextStorage {
    BorrowedOrOwnedMatchedText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TokenUse {
    pub(crate) id: TokenUseId,
    pub(crate) definition: TokenDefinitionId,
    pub(crate) role: TokenUseRole,
    pub(crate) site: SourceSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenUseRole {
    FixedElement,
    Separator,
    ContentAdmission,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceSite {
    pub(crate) source: crate::SourceId,
    pub(crate) path: PathBuf,
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SourceModelDiagnosticKind {
    InvalidConfiguration,
    MultipleTokenSequences,
    EmptyTokenSequence,
    MultipleThisAnchors,
    EmptyThisAnchor,
    UnresolvedFixedToken,
    UnsupportedConfiguredTrait,
    InvalidTraitDelta,
    ConflictingTraitDelta,
    InvalidContentToken,
    ContentDisplayOverride,
    ConflictingTokenDefinition,
    InvalidSeparator,
    UnsupportedFixedSyntax,
    InvalidTokenPlacement,
    InvalidPrattTokenPlacement,
    ConfiguredOrdinaryDeriveConflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceModelDiagnostic {
    pub(crate) kind: SourceModelDiagnosticKind,
    pub(crate) sites: Vec<SourceSite>,
    pub(crate) message: String,
}

pub(crate) fn lower_source_model(input: SourceModelInput<'_>) -> SourceModelReport {
    let semantic_facts = SemanticFacts::from_resolved(&input.resolved);
    let mut diagnostics = Vec::new();
    let trait_planner = TraitPlanner::from_grammar(&input.resolved, &mut diagnostics);
    let mut tokens = TokenCatalog::default();
    tokens.collect_fixed_definitions(&input.resolved, input.cfg);
    let mut builder = SourceModelBuilder {
        resolved: input.resolved,
        cfg: input.cfg,
        semantic_facts,
        trait_planner,
        nodes: Vec::new(),
        tokens,
        diagnostics,
    };
    builder.reject_misplaced_content_declarations();
    builder.lower_nodes();
    builder.finish()
}

struct SourceModelBuilder<'cfg> {
    resolved: ResolvedSourceGrammar,
    cfg: &'cfg CargoCfgSnapshot,
    semantic_facts: SemanticFacts,
    trait_planner: TraitPlanner,
    nodes: Vec<Node>,
    tokens: TokenCatalog,
    diagnostics: Vec<SourceModelDiagnostic>,
}

#[derive(Clone, Copy)]
enum TokenAttachment {
    StructBody,
    Field,
    Variant,
    PrattVariant {
        role: Option<PrattRole>,
        second_field_attachment: Option<usize>,
    },
}

#[derive(Default)]
struct LoweredFields {
    elements: Vec<GrammarElement>,
    attachment_starts: Vec<usize>,
}

impl LoweredFields {
    fn push_attachment(&mut self, elements: Vec<GrammarElement>) {
        self.attachment_starts.push(self.elements.len());
        self.elements.extend(elements);
    }

    fn second_attachment_start(&self) -> Option<usize> {
        self.attachment_starts.get(1).copied()
    }

    fn into_elements(self) -> Vec<GrammarElement> {
        self.elements
    }
}

impl SourceModelBuilder<'_> {
    fn finish(mut self) -> SourceModelReport {
        normalize_source_order(&mut self.nodes, &mut self.tokens);
        let source_modeled = self.diagnostics.is_empty().then(|| SourceModeledGrammar {
            resolved: self.resolved,
            source_model: SourceModel {
                grammar_trait_defaults: self.trait_planner.grammar_defaults,
                nodes: self.nodes,
                token_definitions: self.tokens.definitions,
                token_uses: self.tokens.uses,
            },
        });
        SourceModelReport {
            diagnostics: self.diagnostics,
            source_modeled,
        }
    }

    fn reject_misplaced_content_declarations(&mut self) {
        for inclusion in self.resolved.tree.inclusions.clone() {
            for item in &inclusion.items {
                let configured = match configured_metas(item_attrs(item), self.cfg) {
                    Ok(Some(configured)) => configured,
                    Ok(None) => continue,
                    Err(error) => {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![self.site(inclusion.source, error.range)],
                            message: error.message,
                        });
                        continue;
                    }
                };
                reject_misplaced_content_declarations(
                    &self.resolved,
                    inclusion.source,
                    &configured,
                    &mut self.diagnostics,
                );
                if let Item::Enum(item) = item {
                    for variant in &item.variants {
                        let configured = match configured_metas(&variant.attrs, self.cfg) {
                            Ok(Some(configured)) => configured,
                            Ok(None) => continue,
                            Err(error) => {
                                self.diagnostics.push(SourceModelDiagnostic {
                                    kind: SourceModelDiagnosticKind::InvalidConfiguration,
                                    sites: vec![self.site(inclusion.source, error.range)],
                                    message: error.message,
                                });
                                continue;
                            }
                        };
                        reject_misplaced_content_declarations(
                            &self.resolved,
                            inclusion.source,
                            &configured,
                            &mut self.diagnostics,
                        );
                    }
                }
            }
        }
    }

    fn lower_nodes(&mut self) {
        for node in self.resolved.nodes.clone() {
            let source = node.source;
            let configured = &node.metas;
            let body = match &node.body {
                ResolvedNodeBody::Struct { unit } => {
                    let field_ids = node.fields.iter().map(|field| field.id).collect::<Vec<_>>();
                    let fields = self.lower_fields(source, &field_ids).into_elements();
                    let elements = PositionalTokenPlanner::lower(
                        &self.resolved,
                        &mut self.tokens,
                        &mut self.diagnostics,
                        source,
                        configured,
                        fields,
                        TokenAttachment::StructBody,
                    );
                    if *unit
                        && elements
                            .iter()
                            .all(|element| element.kind() == GrammarElementKind::FixedToken)
                        && !elements.is_empty()
                    {
                        NodeBody::Presence(reassign_element_ids(elements))
                    } else {
                        NodeBody::Struct(reassign_element_ids(elements))
                    }
                }
                ResolvedNodeBody::Enum {
                    variants: resolved_variants,
                } => {
                    PositionalTokenPlanner::reject_legacy_syntax(
                        &self.resolved,
                        source,
                        configured,
                        &mut self.diagnostics,
                    );
                    if let Some(token_meta) =
                        configured.iter().find(|meta| meta.path().is_ident("tok"))
                    {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidTokenPlacement,
                            sites: vec![self.site(source, span_range(token_meta.span()))],
                            message: "#[tok] belongs on each enum variant, not on the enum body"
                                .to_owned(),
                        });
                    }
                    let is_pratt = configured.iter().any(|meta| meta.path().is_ident("pratt"));
                    NodeBody::Enum(
                        resolved_variants
                            .iter()
                            .map(|variant| {
                                let pratt_role = PrattRole::from_attributes(
                                    &self.resolved,
                                    source,
                                    &variant.metas,
                                    is_pratt,
                                    &mut self.diagnostics,
                                );
                                let body = self.lower_fields(source, &variant.fields);
                                let second_field_attachment = body.second_attachment_start();
                                let elements = PositionalTokenPlanner::lower(
                                    &self.resolved,
                                    &mut self.tokens,
                                    &mut self.diagnostics,
                                    source,
                                    &variant.metas,
                                    body.into_elements(),
                                    if is_pratt {
                                        TokenAttachment::PrattVariant {
                                            role: pratt_role,
                                            second_field_attachment,
                                        }
                                    } else {
                                        TokenAttachment::Variant
                                    },
                                );
                                Variant {
                                    resolved_variant: variant.id,
                                    name: variant.ident.clone(),
                                    elements: reassign_element_ids(elements),
                                    pratt_role,
                                    site: self.site(source, variant.range.clone()),
                                }
                            })
                            .collect(),
                    )
                }
            };
            let configured_traits = self.trait_planner.node_plan(
                &self.resolved,
                source,
                configured,
                &mut self.diagnostics,
            );
            self.trait_planner.validate_ordinary_derive_ownership(
                &self.resolved,
                source,
                configured,
                &configured_traits,
                &mut self.diagnostics,
            );
            self.nodes.push(Node {
                resolved_node: node.id,
                name: node.ident,
                body,
                configured_traits,
            });
        }
    }

    fn lower_fields(
        &mut self,
        source: crate::SourceId,
        fields: &[ResolvedFieldId],
    ) -> LoweredFields {
        let mut body = LoweredFields::default();
        for field in fields {
            let resolved_field = self.resolved_field(*field).clone();
            let field = &resolved_field.syntax;
            let field_configured = &resolved_field.metas;
            let site = self.site(source, span_range(field.span()));
            self.tokens.lower_content_definition(
                &self.resolved,
                &self.semantic_facts,
                &self.trait_planner.grammar_defaults,
                source,
                field_configured,
                &resolved_field,
                &mut self.diagnostics,
            );
            let semantic = GrammarElement::SemanticField {
                id: GrammarElementId::next(0),
                resolved_field: resolved_field.id,
                name: field.ident.as_ref().map(|ident| ident.unraw().to_string()),
                separator: SeparatorPlanner::lower(
                    &self.resolved,
                    &self.semantic_facts,
                    &mut self.tokens,
                    &mut self.diagnostics,
                    source,
                    field_configured,
                    &resolved_field,
                ),
                site,
            };
            body.push_attachment(PositionalTokenPlanner::lower(
                &self.resolved,
                &mut self.tokens,
                &mut self.diagnostics,
                source,
                field_configured,
                vec![semantic],
                TokenAttachment::Field,
            ));
        }
        body
    }

    fn resolved_field(&self, id: ResolvedFieldId) -> &ResolvedNodeField {
        self.resolved
            .nodes
            .iter()
            .flat_map(|node| &node.fields)
            .find(|field| field.id == id)
            .expect("resolved Node body owns every field identity")
    }

    fn site(&self, source: crate::SourceId, range: Range<u32>) -> SourceSite {
        source_site(&self.resolved, source, range)
    }
}

struct SemanticFacts {
    content_token_names: BTreeMap<ContentTokenId, String>,
}

impl SemanticFacts {
    fn from_resolved(resolved: &ResolvedSourceGrammar) -> Self {
        let content_token_names = resolved
            .content_tokens
            .iter()
            .map(|token| (token.id, token.ident.clone()))
            .collect();
        Self {
            content_token_names,
        }
    }

    fn target_for_field(field: &ResolvedNodeField) -> Option<SemanticTarget> {
        let ResolvedTypeKind::Semantic { target, .. } = field.ty.kind else {
            return None;
        };
        Some(target)
    }

    fn content_token(&self, field: &ResolvedNodeField) -> Option<(ContentTokenId, String)> {
        let SemanticTarget::ContentToken(token) = Self::target_for_field(field)? else {
            return None;
        };
        Some((token, self.content_token_names.get(&token)?.clone()))
    }

    fn declared_repetition(&self, field: &ResolvedNodeField) -> Option<DeclaredRepetition> {
        match Self::target_for_field(field)? {
            SemanticTarget::Catalog(CatalogType::Vec) => Some(DeclaredRepetition::ZeroOrMore),
            SemanticTarget::Catalog(CatalogType::Vec1) => Some(DeclaredRepetition::OneOrMore),
            SemanticTarget::Node(_)
            | SemanticTarget::GenericParameter(_)
            | SemanticTarget::ContentToken(_)
            | SemanticTarget::TypeAlias(_)
            | SemanticTarget::Catalog(CatalogType::Option | CatalogType::Box) => None,
        }
    }
}

#[derive(Default)]
struct TokenCatalog {
    definitions: Vec<TokenDefinition>,
    fixed_by_name: BTreeMap<String, TokenDefinitionId>,
    content_by_name: BTreeMap<String, TokenDefinitionId>,
    content_conflict_by_name: BTreeMap<String, usize>,
    uses: Vec<TokenUse>,
}

impl TokenCatalog {
    fn collect_fixed_definitions(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        cfg: &CargoCfgSnapshot,
    ) {
        for inclusion in &resolved.tree.inclusions {
            for item in &inclusion.items {
                let Ok(Some(metas)) = configured_metas(item_attrs(item), cfg) else {
                    continue;
                };
                self.collect_fixed_definitions_from_metas(resolved, inclusion.source, &metas);
                match item {
                    Item::Struct(item) => {
                        for field in &item.fields {
                            if let Ok(Some(metas)) = configured_metas(&field.attrs, cfg) {
                                self.collect_fixed_definitions_from_metas(
                                    resolved,
                                    inclusion.source,
                                    &metas,
                                );
                            }
                        }
                    }
                    Item::Enum(item) => {
                        for variant in &item.variants {
                            let Ok(Some(metas)) = configured_metas(&variant.attrs, cfg) else {
                                continue;
                            };
                            self.collect_fixed_definitions_from_metas(
                                resolved,
                                inclusion.source,
                                &metas,
                            );
                            for field in &variant.fields {
                                if let Ok(Some(metas)) = configured_metas(&field.attrs, cfg) {
                                    self.collect_fixed_definitions_from_metas(
                                        resolved,
                                        inclusion.source,
                                        &metas,
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn collect_fixed_definitions_from_metas(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        source: crate::SourceId,
        metas: &[Meta],
    ) {
        for meta in metas {
            if !meta.path().is_ident("tok") && !meta.path().is_ident("sep") {
                continue;
            }
            let Meta::List(list) = meta else { continue };
            let Ok(sequence) = TokenSequence::parse.parse2(list.tokens.clone()) else {
                continue;
            };
            for entry in sequence.0 {
                let TokenSequenceEntry::Definition(spelling) = entry else {
                    continue;
                };
                self.define_fixed(resolved, source, spelling);
            }
        }
    }

    fn define_fixed(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        source: crate::SourceId,
        spelling: LitStr,
    ) {
        let site = source_site(resolved, source, span_range(spelling.span()));
        let spelling = spelling.value();
        let name = spelling.clone();
        if let Some(id) = self.fixed_by_name.get(&name).copied() {
            self.definitions[id.index()].definition_sites.push(site);
            return;
        }
        let id = TokenDefinitionId::next(self.definitions.len());
        self.fixed_by_name.insert(name.clone(), id);
        self.definitions.push(TokenDefinition {
            id,
            name,
            kind: TokenDefinitionKind::Fixed(FixedTokenDefinition { spelling }),
            definition_sites: vec![site],
        });
    }

    fn fixed_definition(&self, name: &str) -> Option<TokenDefinitionId> {
        self.fixed_by_name.get(name).copied()
    }

    fn record_use(
        &mut self,
        definition: TokenDefinitionId,
        role: TokenUseRole,
        site: SourceSite,
    ) -> TokenUseId {
        let id = TokenUseId::next(self.uses.len());
        self.uses.push(TokenUse {
            id,
            definition,
            role,
            site,
        });
        id
    }

    fn fixed_element(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
        source: crate::SourceId,
        token: TokenSequenceEntry,
    ) -> Option<GrammarElement> {
        let (definition, site) = match token {
            TokenSequenceEntry::Definition(spelling) => {
                let site = source_site(resolved, source, span_range(spelling.span()));
                let name = spelling.value();
                let definition = self
                    .fixed_definition(&name)
                    .expect("fixed definitions are collected before uses");
                (definition, site)
            }
            TokenSequenceEntry::Reference(ident) => {
                let site = source_site(resolved, source, span_range(ident.span()));
                let name = ident.unraw().to_string();
                let Some(definition) = self.fixed_definition(&name) else {
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::UnresolvedFixedToken,
                        sites: vec![site],
                        message: format!("fixed token `{name}` has no string definition"),
                    });
                    return None;
                };
                (definition, site)
            }
            TokenSequenceEntry::This(_) => return None,
        };
        let token_use = self.record_use(definition, TokenUseRole::FixedElement, site.clone());
        Some(GrammarElement::FixedToken {
            id: GrammarElementId::next(0),
            token_use,
            site,
        })
    }
}

struct TraitPlanner {
    grammar_defaults: Vec<ConfiguredTraitSelection>,
}

impl TraitPlanner {
    fn from_grammar(
        resolved: &ResolvedSourceGrammar,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> Self {
        let source = resolved.configuration.source;
        let mut grammar_defaults = Vec::new();
        for path in resolved.configuration.syntax.derives.clone() {
            let site = source_site(resolved, source, span_range(path.span()));
            let Some(recipe) = configured_trait_recipe(&path, &resolved.build_inputs.facade.ident)
            else {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::UnsupportedConfiguredTrait,
                    sites: vec![site],
                    message: format!(
                        "`{}` is not a supported configured trait recipe",
                        path_text(&path),
                    ),
                });
                continue;
            };
            if grammar_defaults
                .iter()
                .all(|selection: &ConfiguredTraitSelection| selection.recipe != recipe)
            {
                grammar_defaults.push(ConfiguredTraitSelection {
                    recipe,
                    ownership: ConfiguredTraitOwnership::GrammarDefault,
                });
            }
        }
        Self { grammar_defaults }
    }

    fn node_plan(
        &self,
        resolved: &ResolvedSourceGrammar,
        source: crate::SourceId,
        metas: &[Meta],
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> ConfiguredTraitPlan {
        let node_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("node"))
            .collect::<Vec<_>>();
        if node_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                sites: node_metas
                    .iter()
                    .map(|meta| source_site(resolved, source, span_range(meta.span())))
                    .collect(),
                message: "a Node accepts at most one #[node(...)] trait delta".to_owned(),
            });
            return self.grammar_default_plan();
        }

        let mut additions = Vec::new();
        let mut omissions = Vec::new();
        if let Some(meta) = node_metas.first() {
            let Meta::List(list) = meta else {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                    sites: vec![source_site(resolved, source, span_range(meta.span()))],
                    message: "#[node] must use parentheses".to_owned(),
                });
                return self.grammar_default_plan();
            };
            let syntax = match syn::parse2::<NodeTraitDeltaSyntax>(list.tokens.clone()) {
                Ok(syntax) => syntax,
                Err(error) => {
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                        sites: vec![source_site(resolved, source, span_range(error.span()))],
                        message: error.to_string(),
                    });
                    return self.grammar_default_plan();
                }
            };
            additions = self.normalize_paths(
                resolved,
                source,
                syntax.additions,
                ConfiguredTraitOwnership::LocalAddition,
                diagnostics,
            );
            omissions = self
                .normalize_paths(
                    resolved,
                    source,
                    syntax.omissions,
                    ConfiguredTraitOwnership::LocalOmission,
                    diagnostics,
                )
                .into_iter()
                .map(|selection| ConfiguredTraitOmission {
                    recipe: selection.recipe,
                    ownership: selection.ownership,
                })
                .collect();
        }

        for addition in &additions {
            if omissions
                .iter()
                .any(|omission| omission.recipe == addition.recipe)
            {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::ConflictingTraitDelta,
                    sites: node_metas
                        .iter()
                        .map(|meta| source_site(resolved, source, span_range(meta.span())))
                        .collect(),
                    message: format!(
                        "`{:?}` cannot be both added and omitted on one Node",
                        addition.recipe,
                    ),
                });
            }
        }

        let mut effective = self
            .grammar_defaults
            .iter()
            .copied()
            .filter(|selection| {
                omissions
                    .iter()
                    .all(|omission| omission.recipe != selection.recipe)
            })
            .collect::<Vec<_>>();
        for addition in additions.iter().copied() {
            if omissions
                .iter()
                .all(|omission| omission.recipe != addition.recipe)
                && effective
                    .iter()
                    .all(|selection| selection.recipe != addition.recipe)
            {
                effective.push(addition);
            }
        }
        ConfiguredTraitPlan {
            effective,
            additions,
            omissions,
        }
    }

    fn grammar_default_plan(&self) -> ConfiguredTraitPlan {
        ConfiguredTraitPlan {
            effective: self.grammar_defaults.clone(),
            additions: Vec::new(),
            omissions: Vec::new(),
        }
    }

    fn normalize_paths(
        &self,
        resolved: &ResolvedSourceGrammar,
        source: crate::SourceId,
        paths: Vec<syn::Path>,
        ownership: ConfiguredTraitOwnership,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> Vec<ConfiguredTraitSelection> {
        let mut selections = Vec::new();
        for path in paths {
            let Some(recipe) = configured_trait_recipe(&path, &resolved.build_inputs.facade.ident)
            else {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::UnsupportedConfiguredTrait,
                    sites: vec![source_site(resolved, source, span_range(path.span()))],
                    message: format!(
                        "`{}` is not a supported configured trait recipe",
                        path_text(&path),
                    ),
                });
                continue;
            };
            if selections
                .iter()
                .all(|selection: &ConfiguredTraitSelection| selection.recipe != recipe)
            {
                selections.push(ConfiguredTraitSelection { recipe, ownership });
            }
        }
        selections
    }

    fn validate_ordinary_derive_ownership(
        &self,
        resolved: &ResolvedSourceGrammar,
        source: crate::SourceId,
        metas: &[Meta],
        configured: &ConfiguredTraitPlan,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        for list in metas.iter().filter_map(|meta| {
            let Meta::List(list) = meta else { return None };
            list.path.is_ident("derive").then_some(list)
        }) {
            let Ok(paths) = syn::punctuated::Punctuated::<syn::Path, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
            else {
                continue;
            };
            for path in paths {
                let Some(recipe) =
                    configured_trait_recipe(&path, &resolved.build_inputs.facade.ident)
                else {
                    continue;
                };
                if configured
                    .effective
                    .iter()
                    .any(|selection| selection.recipe == recipe)
                {
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::ConfiguredOrdinaryDeriveConflict,
                        sites: vec![source_site(resolved, source, span_range(path.span()))],
                        message: format!(
                            "`{}` is owned by both configured generation and ordinary #[derive]",
                            path_text(&path),
                        ),
                    });
                }
            }
        }
    }
}

struct SeparatorPlanner;

impl SeparatorPlanner {
    #[allow(clippy::too_many_arguments)]
    fn lower(
        resolved: &ResolvedSourceGrammar,
        semantic_facts: &SemanticFacts,
        tokens: &mut TokenCatalog,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
        source: crate::SourceId,
        metas: &[Meta],
        field: &ResolvedNodeField,
    ) -> Option<Separator> {
        let separator_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("sep"))
            .collect::<Vec<_>>();
        if separator_metas.is_empty() {
            return None;
        }
        if separator_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidSeparator,
                sites: separator_metas
                    .iter()
                    .map(|meta| source_site(resolved, source, span_range(meta.span())))
                    .collect(),
                message: "a repeated field accepts at most one #[sep(...)]".to_owned(),
            });
            return None;
        }
        let meta = separator_metas[0];
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidSeparator,
                sites: vec![source_site(resolved, source, span_range(meta.span()))],
                message: "#[sep] must use parentheses".to_owned(),
            });
            return None;
        };
        let syntax = match syn::parse2::<SeparatorSyntax>(list.tokens.clone()) {
            Ok(syntax) => syntax,
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidSeparator,
                    sites: vec![source_site(resolved, source, span_range(error.span()))],
                    message: error.to_string(),
                });
                return None;
            }
        };
        let declared_repetition = semantic_facts.declared_repetition(field);
        let (name, site) = match syntax.token {
            FixedTokenSyntax::Definition(spelling) => (
                spelling.value(),
                source_site(resolved, source, span_range(spelling.span())),
            ),
            FixedTokenSyntax::Reference(ident) => (
                ident.unraw().to_string(),
                source_site(resolved, source, span_range(ident.span())),
            ),
        };
        let Some(definition) = tokens.fixed_definition(&name) else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::UnresolvedFixedToken,
                sites: vec![site],
                message: format!("fixed token `{name}` has no string definition"),
            });
            return None;
        };
        let token_use = tokens.record_use(definition, TokenUseRole::Separator, site);
        Some(Separator {
            token_use,
            declared_repetition,
            optional_trailing: syntax.optional_trailing,
        })
    }
}

struct PositionalTokenPlanner;

impl PositionalTokenPlanner {
    fn lower(
        resolved: &ResolvedSourceGrammar,
        tokens: &mut TokenCatalog,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
        source: crate::SourceId,
        metas: &[Meta],
        body: Vec<GrammarElement>,
        attachment: TokenAttachment,
    ) -> Vec<GrammarElement> {
        Self::reject_legacy_syntax(resolved, source, metas, diagnostics);
        let token_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("tok"))
            .collect::<Vec<_>>();
        if token_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::MultipleTokenSequences,
                sites: token_metas
                    .iter()
                    .map(|meta| source_site(resolved, source, span_range(meta.span())))
                    .collect(),
                message: "an attachment accepts at most one #[tok(...)] sequence".to_owned(),
            });
            return body;
        }
        let Some(meta) = token_metas.first() else {
            return body;
        };
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::EmptyTokenSequence,
                sites: vec![source_site(resolved, source, span_range(meta.span()))],
                message: "#[tok] must contain one non-empty sequence".to_owned(),
            });
            return body;
        };
        let sequence = match TokenSequence::parse.parse2(list.tokens.clone()) {
            Ok(sequence) => sequence,
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::EmptyTokenSequence,
                    sites: vec![source_site(resolved, source, span_range(error.span()))],
                    message: error.to_string(),
                });
                return body;
            }
        };
        if sequence.0.is_empty() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::EmptyTokenSequence,
                sites: vec![source_site(resolved, source, span_range(meta.span()))],
                message: "#[tok] must contain one non-empty sequence".to_owned(),
            });
            return body;
        }
        let this_sites = sequence
            .0
            .iter()
            .filter_map(|entry| match entry {
                TokenSequenceEntry::This(span) => Some(*span),
                TokenSequenceEntry::Definition(_) | TokenSequenceEntry::Reference(_) => None,
            })
            .collect::<Vec<_>>();
        if this_sites.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::MultipleThisAnchors,
                sites: this_sites
                    .iter()
                    .map(|span| source_site(resolved, source, span_range(*span)))
                    .collect(),
                message: "a #[tok(...)] sequence may contain at most one `this`".to_owned(),
            });
            return body;
        }
        if matches!(attachment, TokenAttachment::PrattVariant { .. }) && !this_sites.is_empty() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                sites: vec![source_site(resolved, source, span_range(this_sites[0]))],
                message: "`this` is prohibited on Pratt variants; attach interleaved syntax to operand fields"
                    .to_owned(),
            });
            return body;
        }
        if !this_sites.is_empty() && body.is_empty() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::EmptyThisAnchor,
                sites: vec![source_site(resolved, source, span_range(this_sites[0]))],
                message: "`this` cannot anchor an empty semantic body".to_owned(),
            });
            return body;
        }

        if !this_sites.is_empty() {
            let mut output = Vec::new();
            for entry in sequence.0 {
                match entry {
                    TokenSequenceEntry::This(_) => output.extend(body.clone()),
                    token => {
                        if let Some(element) =
                            tokens.fixed_element(resolved, diagnostics, source, token)
                        {
                            output.push(element);
                        }
                    }
                }
            }
            return output;
        }

        let fixed = sequence
            .0
            .into_iter()
            .filter_map(|token| tokens.fixed_element(resolved, diagnostics, source, token))
            .collect::<Vec<_>>();
        Self::place_anchorless(fixed, body, attachment)
    }

    fn place_anchorless(
        mut fixed: Vec<GrammarElement>,
        mut body: Vec<GrammarElement>,
        attachment: TokenAttachment,
    ) -> Vec<GrammarElement> {
        match attachment {
            TokenAttachment::PrattVariant {
                role: Some(PrattRole::Postfix),
                ..
            } => {
                body.extend(fixed);
                body
            }
            TokenAttachment::PrattVariant {
                role: Some(PrattRole::Infix),
                second_field_attachment,
            } => {
                let insertion = second_field_attachment.unwrap_or(body.len());
                body.splice(insertion..insertion, fixed);
                body
            }
            TokenAttachment::StructBody
            | TokenAttachment::Field
            | TokenAttachment::Variant
            | TokenAttachment::PrattVariant {
                role: None | Some(PrattRole::Prefix),
                ..
            } => {
                fixed.extend(body);
                fixed
            }
        }
    }

    fn reject_legacy_syntax(
        resolved: &ResolvedSourceGrammar,
        source: crate::SourceId,
        metas: &[Meta],
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        for meta in metas.iter().filter(|meta| {
            matches!(
                path_text(meta.path()).as_str(),
                "kwd" | "surrounded" | "trailing"
            )
        }) {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::UnsupportedFixedSyntax,
                sites: vec![source_site(resolved, source, span_range(meta.span()))],
                message: format!(
                    "#[{}] is unsupported; #[tok(...)] is the sole fixed-token syntax",
                    path_text(meta.path()),
                ),
            });
        }
    }
}

impl PrattRole {
    fn from_attributes(
        resolved: &ResolvedSourceGrammar,
        source: crate::SourceId,
        metas: &[Meta],
        is_pratt: bool,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> Option<Self> {
        if !is_pratt {
            return None;
        }
        let parse_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("parse"))
            .collect::<Vec<_>>();
        if parse_metas.is_empty() {
            return None;
        }
        if parse_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                sites: parse_metas
                    .iter()
                    .map(|meta| source_site(resolved, source, span_range(meta.span())))
                    .collect(),
                message: "a Pratt variant accepts at most one #[parse(...)] operator role"
                    .to_owned(),
            });
            return None;
        }
        let meta = parse_metas[0];
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                sites: vec![source_site(resolved, source, span_range(meta.span()))],
                message: "a Pratt #[parse] role must use parentheses".to_owned(),
            });
            return None;
        };
        match syn::parse2::<PrattSyntax>(list.tokens.clone()) {
            Ok(syntax) => Some(syntax.role),
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                    sites: vec![source_site(resolved, source, span_range(error.span()))],
                    message: error.to_string(),
                });
                None
            }
        }
    }
}

fn reject_misplaced_content_declarations(
    resolved: &ResolvedSourceGrammar,
    source: crate::SourceId,
    metas: &[Meta],
    diagnostics: &mut Vec<SourceModelDiagnostic>,
) {
    for meta in metas.iter().filter(|meta| meta.path().is_ident("lex")) {
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::InvalidContentToken,
            sites: vec![source_site(resolved, source, span_range(meta.span()))],
            message: "#[lex(...)] declares a content-token field and is valid only on a field"
                .to_owned(),
        });
    }
}

fn source_site(
    resolved: &ResolvedSourceGrammar,
    source: crate::SourceId,
    range: Range<u32>,
) -> SourceSite {
    SourceSite {
        source,
        path: resolved.tree.files[source.index()].path.clone(),
        range,
    }
}

impl TokenCatalog {
    #[allow(clippy::too_many_arguments)]
    fn lower_content_definition(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        semantic_facts: &SemanticFacts,
        grammar_defaults: &[ConfiguredTraitSelection],
        source: crate::SourceId,
        metas: &[Meta],
        field: &ResolvedNodeField,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let lex_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("lex"))
            .collect::<Vec<_>>();
        if lex_metas.is_empty() {
            return;
        }
        if lex_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidContentToken,
                sites: lex_metas
                    .iter()
                    .map(|meta| source_site(resolved, source, span_range(meta.span())))
                    .collect(),
                message: "a field accepts at most one #[lex(...)] declaration".to_owned(),
            });
            return;
        }
        let meta = lex_metas[0];
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidContentToken,
                sites: vec![source_site(resolved, source, span_range(meta.span()))],
                message: "#[lex] must use parentheses".to_owned(),
            });
            return;
        };
        let syntax = match syn::parse2::<LexSyntax>(list.tokens.clone()) {
            Ok(syntax) => syntax,
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidContentToken,
                    sites: vec![source_site(resolved, source, span_range(error.span()))],
                    message: error.to_string(),
                });
                return;
            }
        };
        if let Some(display) = syntax.display {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::ContentDisplayOverride,
                sites: vec![source_site(resolved, source, span_range(display.span()))],
                message: "content tokens always display their exact matched text; `display` is unsupported"
                    .to_owned(),
            });
            return;
        }
        let Some((semantic, name)) = semantic_facts.content_token(field) else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidContentToken,
                sites: vec![source_site(resolved, source, field.range.clone())],
                message: "#[lex] requires a resolved content-token field target".to_owned(),
            });
            return;
        };

        let mut authored_definitions = Vec::new();
        for reference in syntax.admissions {
            let site = source_site(resolved, source, span_range(reference.span()));
            let fixed_name = reference.unraw().to_string();
            let Some(definition) = self.fixed_definition(&fixed_name) else {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::UnresolvedFixedToken,
                    sites: vec![site],
                    message: format!("fixed token `{fixed_name}` has no string definition"),
                });
                continue;
            };
            authored_definitions.push(definition);
            self.record_use(definition, TokenUseRole::ContentAdmission, site);
        }

        let definition_site = source_site(resolved, source, span_range(meta.span()));
        let mut admission_set = authored_definitions.clone();
        admission_set.sort_unstable();
        admission_set.dedup();
        let authored = AuthoredAdmissions {
            site: definition_site.clone(),
            definitions: authored_definitions,
        };
        self.define_content(
            semantic,
            name,
            syntax.pattern.value(),
            admission_set,
            authored,
            definition_site,
            grammar_defaults,
            diagnostics,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn define_content(
        &mut self,
        semantic: ContentTokenId,
        name: String,
        pattern: String,
        admission_set: Vec<TokenDefinitionId>,
        authored: AuthoredAdmissions,
        site: SourceSite,
        grammar_defaults: &[ConfiguredTraitSelection],
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        if let Some(id) = self.content_by_name.get(&name).copied() {
            let definition = &mut self.definitions[id.index()];
            let compatible = matches!(
                &definition.kind,
                TokenDefinitionKind::Content(content)
                    if content.semantic == semantic
                        && content.pattern == pattern
                        && content.admission_set == admission_set
            );
            definition.definition_sites.push(site);
            let TokenDefinitionKind::Content(content) = &mut definition.kind else {
                unreachable!("content namespace points only to content definitions");
            };
            content.authored_admissions.push(authored);
            if !compatible {
                let diagnostic = SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::ConflictingTokenDefinition,
                    sites: definition.definition_sites.clone(),
                    message: format!(
                        "content token `{name}` has incompatible declarations at every listed site"
                    ),
                };
                if let Some(index) = self.content_conflict_by_name.get(&name).copied() {
                    diagnostics[index] = diagnostic;
                } else {
                    let index = diagnostics.len();
                    diagnostics.push(diagnostic);
                    self.content_conflict_by_name.insert(name, index);
                }
            } else if let Some(index) = self.content_conflict_by_name.get(&name).copied() {
                diagnostics[index].sites = definition.definition_sites.clone();
            }
            return;
        }

        let id = TokenDefinitionId::next(self.definitions.len());
        self.content_by_name.insert(name.clone(), id);
        self.definitions.push(TokenDefinition {
            id,
            name,
            kind: TokenDefinitionKind::Content(ContentTokenDefinition {
                semantic,
                pattern,
                admission_set,
                authored_admissions: vec![authored],
                text: ContentTextStorage::BorrowedOrOwnedMatchedText,
                configured_traits: ConfiguredTraitPlan {
                    effective: grammar_defaults.to_vec(),
                    additions: Vec::new(),
                    omissions: Vec::new(),
                },
            }),
            definition_sites: vec![site],
        });
    }
}

#[derive(Clone)]
struct TokenSequence(Vec<TokenSequenceEntry>);

#[derive(Clone)]
enum TokenSequenceEntry {
    Definition(LitStr),
    Reference(syn::Ident),
    This(Span),
}

enum FixedTokenSyntax {
    Definition(LitStr),
    Reference(syn::Ident),
}

struct PrattSyntax {
    role: PrattRole,
}

impl Parse for PrattSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let role = syn::Ident::parse_any(input)?;
        let role = if role == "prefix" {
            PrattRole::Prefix
        } else if role == "infix" {
            PrattRole::Infix
        } else if role == "postfix" {
            PrattRole::Postfix
        } else {
            return Err(syn::Error::new(
                role.span(),
                "Pratt role must be prefix, infix, or postfix",
            ));
        };
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            input.parse::<Meta>()?;
        }
        Ok(Self { role })
    }
}

struct SeparatorSyntax {
    token: FixedTokenSyntax,
    optional_trailing: bool,
}

impl Parse for SeparatorSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let token = if input.peek(LitStr) {
            FixedTokenSyntax::Definition(input.parse()?)
        } else {
            FixedTokenSyntax::Reference(syn::Ident::parse_any(input)?)
        };
        let optional_trailing = if input.is_empty() {
            false
        } else {
            input.parse::<Token![,]>()?;
            let trailing = syn::Ident::parse_any(input)?;
            if trailing != "trailing" {
                return Err(syn::Error::new(
                    trailing.span(),
                    "#[sep] accepts only the `trailing` option",
                ));
            }
            true
        };
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after #[sep] declaration"));
        }
        Ok(Self {
            token,
            optional_trailing,
        })
    }
}

struct LexSyntax {
    pattern: LitStr,
    admissions: Vec<syn::Ident>,
    display: Option<LitStr>,
}

impl Parse for LexSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut pattern = None;
        let mut admissions = None;
        let mut display = None;
        while !input.is_empty() {
            let key = syn::Ident::parse_any(input)?;
            if key == "pattern" {
                input.parse::<Token![=]>()?;
                if pattern.replace(input.parse()?).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate lex pattern"));
                }
            } else if key == "admits" {
                let content;
                syn::parenthesized!(content in input);
                let values =
                    syn::punctuated::Punctuated::<syn::Ident, Token![,]>::parse_terminated(
                        &content,
                    )?
                    .into_iter()
                    .collect();
                if admissions.replace(values).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate admits list"));
                }
            } else if key == "display" {
                input.parse::<Token![=]>()?;
                if display.replace(input.parse()?).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate display option"));
                }
            } else {
                return Err(syn::Error::new(key.span(), "unsupported #[lex] option"));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            pattern: pattern.ok_or_else(|| input.error("#[lex] requires pattern = r\"...\""))?,
            admissions: admissions.unwrap_or_default(),
            display,
        })
    }
}

struct NodeTraitDeltaSyntax {
    additions: Vec<syn::Path>,
    omissions: Vec<syn::Path>,
}

impl Parse for NodeTraitDeltaSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut additions = None;
        let mut omissions = None;
        while !input.is_empty() {
            let key = syn::Ident::parse_any(input)?;
            let content;
            syn::parenthesized!(content in input);
            let paths =
                syn::punctuated::Punctuated::<syn::Path, Token![,]>::parse_terminated(&content)?
                    .into_iter()
                    .collect();
            if key == "derive" {
                if additions.replace(paths).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate local derive list"));
                }
            } else if key == "omit" {
                if omissions.replace(paths).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate local omit list"));
                }
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "#[node] supports only derive(...) and omit(...)",
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            additions: additions.unwrap_or_default(),
            omissions: omissions.unwrap_or_default(),
        })
    }
}

impl Parse for TokenSequence {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut entries = Vec::new();
        while !input.is_empty() {
            if input.peek(LitStr) {
                entries.push(TokenSequenceEntry::Definition(input.parse()?));
            } else {
                let ident = syn::Ident::parse_any(input)?;
                if ident == "this" {
                    entries.push(TokenSequenceEntry::This(ident.span()));
                } else {
                    entries.push(TokenSequenceEntry::Reference(ident));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self(entries))
    }
}

fn configured_trait_recipe(path: &syn::Path, facade: &str) -> Option<ConfiguredTraitRecipe> {
    if path.leading_colon.is_some()
        || path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
    {
        return None;
    }
    let text = path_text(path);
    let bare = path.segments.len() == 1;
    match text.as_str() {
        "Debug" | "core::fmt::Debug" | "std::fmt::Debug" => Some(ConfiguredTraitRecipe::Debug),
        "Clone" | "core::clone::Clone" | "std::clone::Clone" => Some(ConfiguredTraitRecipe::Clone),
        "Copy" | "core::marker::Copy" | "std::marker::Copy" => Some(ConfiguredTraitRecipe::Copy),
        "PartialEq" | "core::cmp::PartialEq" | "std::cmp::PartialEq" => {
            Some(ConfiguredTraitRecipe::PartialEq)
        }
        "Eq" | "core::cmp::Eq" | "std::cmp::Eq" => Some(ConfiguredTraitRecipe::Eq),
        "PartialOrd" | "core::cmp::PartialOrd" | "std::cmp::PartialOrd" => {
            Some(ConfiguredTraitRecipe::PartialOrd)
        }
        "Ord" | "core::cmp::Ord" | "std::cmp::Ord" => Some(ConfiguredTraitRecipe::Ord),
        "Hash" | "core::hash::Hash" | "std::hash::Hash" => Some(ConfiguredTraitRecipe::Hash),
        "Pretty" if bare => Some(ConfiguredTraitRecipe::Pretty),
        "Visit" if bare => Some(ConfiguredTraitRecipe::Visit),
        "VisitMut" if bare => Some(ConfiguredTraitRecipe::VisitMut),
        "Arbitrary" if bare => Some(ConfiguredTraitRecipe::Arbitrary),
        "Serialize" | "serde::Serialize" => Some(ConfiguredTraitRecipe::Serialize),
        "Deserialize" | "serde::Deserialize" => Some(ConfiguredTraitRecipe::Deserialize),
        _ if text == format!("{facade}::Pretty") => Some(ConfiguredTraitRecipe::Pretty),
        _ if text == format!("{facade}::Visit") => Some(ConfiguredTraitRecipe::Visit),
        _ if text == format!("{facade}::VisitMut") => Some(ConfiguredTraitRecipe::VisitMut),
        _ if text == format!("{facade}::Arbitrary") => Some(ConfiguredTraitRecipe::Arbitrary),
        _ => None,
    }
}

fn normalize_source_order(nodes: &mut [Node], tokens: &mut TokenCatalog) {
    let site_key = |site: &SourceSite| (site.source.index(), site.range.start, site.range.end);

    for definition in &mut tokens.definitions {
        definition.definition_sites.sort_by_key(&site_key);
        if let TokenDefinitionKind::Content(content) = &mut definition.kind {
            content
                .authored_admissions
                .sort_by_key(|authored| site_key(&authored.site));
        }
    }
    tokens.definitions.sort_by_key(|definition| {
        definition
            .definition_sites
            .first()
            .map(&site_key)
            .unwrap_or((usize::MAX, u32::MAX, u32::MAX))
    });
    let mut definition_remap = vec![TokenDefinitionId::next(0); tokens.definitions.len()];
    for (new_index, definition) in tokens.definitions.iter_mut().enumerate() {
        let new_id = TokenDefinitionId::next(new_index);
        definition_remap[definition.id.index()] = new_id;
        definition.id = new_id;
    }
    for definition in &mut tokens.definitions {
        if let TokenDefinitionKind::Content(content) = &mut definition.kind {
            for admission in &mut content.admission_set {
                *admission = definition_remap[admission.index()];
            }
            content.admission_set.sort_unstable();
            content.admission_set.dedup();
            for authored in &mut content.authored_admissions {
                for admission in &mut authored.definitions {
                    *admission = definition_remap[admission.index()];
                }
            }
        }
    }
    for token_use in &mut tokens.uses {
        token_use.definition = definition_remap[token_use.definition.index()];
    }

    tokens
        .uses
        .sort_by_key(|token_use| site_key(&token_use.site));
    let mut use_remap = vec![TokenUseId::next(0); tokens.uses.len()];
    for (new_index, token_use) in tokens.uses.iter_mut().enumerate() {
        let new_id = TokenUseId::next(new_index);
        use_remap[token_use.id.index()] = new_id;
        token_use.id = new_id;
    }

    let mut next_element = 0;
    for node in nodes {
        match &mut node.body {
            NodeBody::Struct(elements) | NodeBody::Presence(elements) => {
                assign_ordered_elements(elements, &mut next_element, &use_remap);
            }
            NodeBody::Enum(variants) => {
                for variant in variants {
                    assign_ordered_elements(&mut variant.elements, &mut next_element, &use_remap);
                }
            }
        }
    }
}

fn reassign_element_ids(mut elements: Vec<GrammarElement>) -> Vec<GrammarElement> {
    for (index, element) in elements.iter_mut().enumerate() {
        element.set_id(GrammarElementId::next(index));
    }
    elements
}

fn assign_ordered_elements(
    elements: &mut [GrammarElement],
    next_element: &mut usize,
    use_remap: &[TokenUseId],
) {
    for element in elements {
        element.set_id(GrammarElementId::next(*next_element));
        *next_element += 1;
        match element {
            GrammarElement::FixedToken { token_use, .. } => {
                *token_use = use_remap[token_use.index()];
            }
            GrammarElement::SemanticField { separator, .. } => {
                if let Some(separator) = separator {
                    separator.token_use = use_remap[separator.token_use.index()];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        ConfiguredTraitOwnership, ConfiguredTraitRecipe, ContentTextStorage, DeclaredRepetition,
        GrammarElement, GrammarElementKind, NodeShape, SourceModelDiagnosticKind, SourceModelInput,
        TokenDefinitionKind, TokenUseRole, lower_source_model,
    };
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::RustEdition;
    use crate::resolution::{ResolutionInput, resolve_local_semantics};
    use crate::source_syntax::CargoCfgSnapshot;

    fn model_with_cfg(source: &str, cfg: &CargoCfgSnapshot) -> super::SourceModelReport {
        let package = TempDir::new().unwrap();
        let root = package.path().join("grammar.rs");
        fs::write(&root, source).unwrap();
        let discovery = discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new("grammar.rs"),
            edition: RustEdition::Rust2024,
            cfg,
        });
        assert!(
            discovery.diagnostics.is_empty(),
            "{:?}",
            discovery.diagnostics
        );
        let resolution = resolve_local_semantics(ResolutionInput {
            discovery_inputs: discovery.discovery_inputs,
            tree: discovery.tree.unwrap(),
            cfg,
        });
        assert!(
            resolution.diagnostics.is_empty(),
            "{:?}",
            resolution.diagnostics
        );
        lower_source_model(SourceModelInput {
            resolved: resolution.resolved.unwrap(),
            cfg,
        })
    }

    fn model(source: &str) -> super::SourceModelReport {
        model_with_cfg(source, &CargoCfgSnapshot::default())
    }

    fn completed(report: super::SourceModelReport) -> super::SourceModel {
        report.source_modeled.unwrap().source_model
    }

    #[test]
    fn lowers_positional_tokens_and_presence_nodes_in_source_order() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("(", this, ")")]
struct Root { value: Present }

#[derive(recursa::Node)]
#[tok("PRESENT")]
struct Present;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(
            model
                .nodes
                .iter()
                .map(|node| (node.name.as_str(), node.shape()))
                .collect::<Vec<_>>(),
            [
                ("Root", NodeShape::Struct),
                ("Present", NodeShape::Presence)
            ],
        );
        assert_eq!(
            model.nodes[0]
                .elements()
                .iter()
                .map(|element| element.kind())
                .collect::<Vec<_>>(),
            [
                GrammarElementKind::FixedToken,
                GrammarElementKind::SemanticField,
                GrammarElementKind::FixedToken,
            ],
        );
        assert_eq!(
            model.nodes[1]
                .elements()
                .iter()
                .map(|element| element.kind())
                .collect::<Vec<_>>(),
            [GrammarElementKind::FixedToken],
        );
        assert_eq!(
            model
                .token_definitions
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["(", ")", "PRESENT"],
        );
    }

    #[test]
    fn applies_grammar_trait_defaults_and_node_owned_additions_and_omissions() {
        let report = model(
            r#"
recursa::grammar! {
    module = crate,
    derives(Debug, Clone, Visit),
}

#[derive(recursa::Node)]
#[node(derive(Pretty), omit(Clone))]
struct Root { child: Leaf }

#[derive(recursa::Node)]
struct Leaf;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.grammar_trait_defaults.len(), 3);
        assert_eq!(
            model.nodes[0].configured_traits.additions[0].ownership,
            ConfiguredTraitOwnership::LocalAddition,
        );
        assert_eq!(
            model.nodes[0]
                .configured_traits
                .effective
                .iter()
                .map(|recipe| (recipe.recipe, recipe.ownership))
                .collect::<Vec<_>>(),
            [
                (
                    ConfiguredTraitRecipe::Debug,
                    ConfiguredTraitOwnership::GrammarDefault,
                ),
                (
                    ConfiguredTraitRecipe::Visit,
                    ConfiguredTraitOwnership::GrammarDefault,
                ),
                (
                    ConfiguredTraitRecipe::Pretty,
                    ConfiguredTraitOwnership::LocalAddition,
                ),
            ],
        );
        assert_eq!(
            model.nodes[0].configured_traits.omissions[0].recipe,
            ConfiguredTraitRecipe::Clone,
        );
        assert_eq!(model.nodes[1].configured_traits.effective.len(), 3);
    }

    #[test]
    fn content_declarations_model_exact_borrowed_or_owned_match_text() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Clone) }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    word: Word,
}

"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let content = model
            .token_definitions
            .iter()
            .find_map(|definition| match &definition.kind {
                TokenDefinitionKind::Content(content) => Some(content),
                TokenDefinitionKind::Fixed(_) => None,
            })
            .expect("content token definition");
        assert_eq!(content.pattern, "[A-Za-z_][A-Za-z0-9_]*");
        assert_eq!(content.text, ContentTextStorage::BorrowedOrOwnedMatchedText);
        assert_eq!(content.configured_traits.effective.len(), 1);
        assert_eq!(
            content.configured_traits.effective[0].recipe,
            ConfiguredTraitRecipe::Clone,
        );
    }

    #[test]
    fn separators_are_repetition_metadata_and_model_optional_trailing_use() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[sep(",")]
    zero_or_more: Vec<Leaf>,
    #[sep(",", trailing)]
    one_or_more: recursa::Vec1<Leaf>,
}

#[derive(recursa::Node)]
struct Leaf;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.nodes[0].elements().len(), 2);
        let first = model.nodes[0].elements()[0]
            .separator()
            .expect("Vec separator");
        assert_eq!(
            first.declared_repetition,
            Some(DeclaredRepetition::ZeroOrMore)
        );
        assert!(!first.optional_trailing);
        let second = model.nodes[0].elements()[1]
            .separator()
            .expect("Vec1 separator");
        assert_eq!(
            second.declared_repetition,
            Some(DeclaredRepetition::OneOrMore)
        );
        assert!(second.optional_trailing);
        assert_eq!(
            model
                .token_uses
                .iter()
                .filter(|token_use| token_use.role == TokenUseRole::Separator)
                .count(),
            2,
        );
    }

    #[test]
    fn non_pratt_variants_keep_independent_positional_token_sequences() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
enum Choice {
    #[tok("(", this, ")")]
    Wrapped(Leaf),
    #[tok("NULL")]
    Null,
}

#[derive(recursa::Node)]
struct Leaf;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.nodes[0].shape(), NodeShape::Enum);
        assert_eq!(
            model.nodes[0]
                .variants()
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            ["Wrapped", "Null"],
        );
        assert_eq!(
            model.nodes[0].variants()[0]
                .elements
                .iter()
                .map(|element| element.kind())
                .collect::<Vec<_>>(),
            [
                GrammarElementKind::FixedToken,
                GrammarElementKind::SemanticField,
                GrammarElementKind::FixedToken,
            ],
        );
        assert_eq!(
            model.nodes[0].variants()[1].elements[0].kind(),
            GrammarElementKind::FixedToken,
        );
    }

    #[test]
    fn rejects_duplicate_this_pratt_variant_anchors_and_legacy_fixed_syntax() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[parse(prefix, bp = 10)]
    #[tok("!", this)]
    Not(Box<Expr>),

    #[tok("(", this, this, ")")]
    Group(Box<Expr>),
}

#[derive(recursa::Node)]
#[kwd("OLD")]
struct Legacy;
"#,
        );

        assert!(report.source_modeled.is_none());
        let kinds = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&SourceModelDiagnosticKind::InvalidPrattTokenPlacement));
        assert!(kinds.contains(&SourceModelDiagnosticKind::MultipleThisAnchors));
        assert!(kinds.contains(&SourceModelDiagnosticKind::UnsupportedFixedSyntax));
    }

    #[test]
    fn rejects_unsupported_trait_recipes_delta_overlap_and_ordinary_derive_ownership() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Debug, Default) }

#[derive(recursa::Node, Debug)]
#[node(derive(Clone), omit(Clone))]
struct Root;
"#,
        );

        assert!(report.source_modeled.is_none());
        let kinds = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&SourceModelDiagnosticKind::UnsupportedConfiguredTrait));
        assert!(kinds.contains(&SourceModelDiagnosticKind::ConflictingTraitDelta));
        assert!(kinds.contains(&SourceModelDiagnosticKind::ConfiguredOrdinaryDeriveConflict));
    }

    #[test]
    fn rejects_content_display_misplacement_and_reports_every_conflicting_site() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = "a", display = "A")]
    displayed: Word,
    #[lex(pattern = "a")]
    first: Duplicate,
    #[lex(pattern = "b")]
    second: Duplicate,
    #[lex(pattern = "a")]
    third: Duplicate,
}

#[derive(recursa::Node)]
#[lex(pattern = "misplaced")]
struct Misplaced;
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::ContentDisplayOverride
        }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidContentToken
                && diagnostic.message.contains("field")
        }));
        let conflict = report
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.kind == SourceModelDiagnosticKind::ConflictingTokenDefinition
            })
            .expect("incompatible content definitions conflict");
        assert_eq!(conflict.sites.len(), 3);
    }

    #[test]
    fn fixed_references_are_order_independent_and_exact_duplicates_unify() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok(IF, this)]
struct Root { value: First }

#[derive(recursa::Node)]
#[tok("IF")]
struct First;

#[derive(recursa::Node)]
#[tok("IF")]
struct Second;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.token_definitions.len(), 1);
        assert_eq!(model.token_definitions[0].name, "IF");
        assert_eq!(model.token_definitions[0].definition_sites.len(), 2);
        assert_eq!(model.token_uses.len(), 3);
        assert!(
            model
                .token_uses
                .iter()
                .all(|token_use| token_use.definition == model.token_definitions[0].id)
        );
    }

    #[test]
    fn configured_local_trait_deltas_are_applied_after_grammar_defaults() {
        let cfg = CargoCfgSnapshot::default().with_flag("unix");
        let report = model_with_cfg(
            r#"
recursa::grammar! { module = crate, derives(Debug, Visit) }

#[derive(recursa::Node)]
#[cfg_attr(unix, node(derive(Clone), omit(Debug)))]
struct Root;
"#,
            &cfg,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let traits = &completed(report).nodes[0].configured_traits;
        assert_eq!(
            traits
                .effective
                .iter()
                .map(|selection| (selection.recipe, selection.ownership))
                .collect::<Vec<_>>(),
            [
                (
                    ConfiguredTraitRecipe::Visit,
                    ConfiguredTraitOwnership::GrammarDefault,
                ),
                (
                    ConfiguredTraitRecipe::Clone,
                    ConfiguredTraitOwnership::LocalAddition,
                ),
            ],
        );
        assert_eq!(traits.omissions[0].recipe, ConfiguredTraitRecipe::Debug);
    }

    #[test]
    fn accepts_only_the_closed_configured_trait_recipe_catalog() {
        let report = model(
            r#"
my_recursa::grammar! {
    module = crate,
    facade = my_recursa,
    derives(
        core::fmt::Debug,
        core::clone::Clone,
        core::marker::Copy,
        core::cmp::PartialEq,
        core::cmp::Eq,
        core::cmp::PartialOrd,
        core::cmp::Ord,
        core::hash::Hash,
        my_recursa::Pretty,
        my_recursa::Visit,
        my_recursa::VisitMut,
        my_recursa::Arbitrary,
        serde::Serialize,
        serde::Deserialize,
    ),
}

#[derive(my_recursa::Node)]
struct Root;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert_eq!(
            completed(report).nodes[0].configured_traits.effective.len(),
            14,
        );
    }

    #[test]
    fn token_definitions_and_elements_have_stable_global_source_order() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = "word")]
    first: Word,
    later: Later,
}

#[derive(recursa::Node)]
#[tok("LATE")]
struct Later;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(
            model
                .token_definitions
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["Word", "LATE"],
        );
        let element_ids = model
            .nodes
            .iter()
            .flat_map(|node| {
                node.elements().iter().chain(
                    node.variants()
                        .iter()
                        .flat_map(|variant| variant.elements.iter()),
                )
            })
            .map(|element| element.id().index())
            .collect::<Vec<_>>();
        assert_eq!(element_ids, (0..element_ids.len()).collect::<Vec<_>>());
    }

    #[test]
    fn content_admissions_resolve_through_the_fixed_token_namespace() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = "word", admits(IF))]
    word: Word,
}

#[derive(recursa::Node)]
#[tok("IF")]
struct IfPresence;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let (content_id, admission) = model
            .token_definitions
            .iter()
            .find_map(|definition| match &definition.kind {
                TokenDefinitionKind::Content(content) => {
                    Some((definition.id, content.admission_set[0]))
                }
                TokenDefinitionKind::Fixed(_) => None,
            })
            .unwrap();
        assert_ne!(content_id, admission);
        assert!(model.token_uses.iter().any(|token_use| {
            token_use.role == TokenUseRole::ContentAdmission && token_use.definition == admission
        }));
    }

    #[test]
    fn a_token_sequence_without_this_precedes_the_semantic_body() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("BEGIN")]
struct Root { value: Leaf }

#[derive(recursa::Node)]
struct Leaf;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert_eq!(
            completed(report).nodes[0]
                .elements()
                .iter()
                .map(|element| element.kind())
                .collect::<Vec<_>>(),
            [
                GrammarElementKind::FixedToken,
                GrammarElementKind::SemanticField,
            ],
        );
    }

    #[test]
    fn rejects_every_invalid_positional_attachment_before_m2() {
        let cases = [
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok("A")]
#[tok("B")]
struct Root { value: Leaf }
#[derive(recursa::Node)] struct Leaf;
"#,
                SourceModelDiagnosticKind::MultipleTokenSequences,
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok()]
struct Root { value: Leaf }
#[derive(recursa::Node)] struct Leaf;
"#,
                SourceModelDiagnosticKind::EmptyTokenSequence,
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok(this)]
struct Root;
"#,
                SourceModelDiagnosticKind::EmptyThisAnchor,
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok("BAD")]
enum Root { Variant }
"#,
                SourceModelDiagnosticKind::InvalidTokenPlacement,
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok(MISSING)]
struct Root;
"#,
                SourceModelDiagnosticKind::UnresolvedFixedToken,
            ),
        ];

        for (source, expected) in cases {
            let report = model(source);
            assert!(report.source_modeled.is_none());
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.kind == expected),
                "missing {expected:?} in {:?}",
                report.diagnostics,
            );
        }
    }

    #[test]
    fn pratt_operand_fields_may_anchor_interleaved_fixed_syntax() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[parse(prefix, bp = 10)]
    #[tok("!")]
    Not(#[tok("(", this, ")")] Box<Expr>),
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert_eq!(
            completed(report).nodes[0].variants()[0]
                .elements
                .iter()
                .map(|element| element.kind())
                .collect::<Vec<_>>(),
            [
                GrammarElementKind::FixedToken,
                GrammarElementKind::FixedToken,
                GrammarElementKind::SemanticField,
                GrammarElementKind::FixedToken,
            ],
        );
    }

    #[test]
    fn unresolved_repetition_is_retained_for_post_resolution_materialization() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Vec<T> { value: T }

#[derive(recursa::Node)]
struct Root {
    #[sep(",")]
    values: Vec<Leaf>,
}

#[derive(recursa::Node)]
struct Leaf;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let source_model = completed(report);
        let GrammarElement::SemanticField {
            separator: Some(separator),
            ..
        } = &source_model.nodes[1].elements()[0]
        else {
            panic!("separator is retained until aliases and applications are materialized")
        };
        assert_eq!(separator.declared_repetition, None);
    }

    #[test]
    fn content_declarations_require_a_resolved_token_type_target() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root<Word> {
    #[lex(pattern = "word")]
    word: Word,
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidContentToken
        }));
    }

    #[test]
    fn pratt_roles_are_retained_and_place_anchorless_tokens() {
        use super::PrattRole;

        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[parse(prefix, bp = 30)]
    #[tok("!")]
    Prefix(Box<Expr>),

    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok("+")]
    Infix(Box<Expr>, Box<Expr>),

    #[parse(postfix, bp = 40)]
    #[tok("?")]
    Postfix(Box<Expr>),
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let variants = model.nodes[0].variants();
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.pratt_role)
                .collect::<Vec<_>>(),
            [
                Some(PrattRole::Prefix),
                Some(PrattRole::Infix),
                Some(PrattRole::Postfix),
            ],
        );
        let roles = variants
            .iter()
            .map(|variant| {
                variant
                    .elements
                    .iter()
                    .map(|element| element.kind())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            roles,
            [
                vec![
                    GrammarElementKind::FixedToken,
                    GrammarElementKind::SemanticField,
                ],
                vec![
                    GrammarElementKind::SemanticField,
                    GrammarElementKind::FixedToken,
                    GrammarElementKind::SemanticField,
                ],
                vec![
                    GrammarElementKind::SemanticField,
                    GrammarElementKind::FixedToken,
                ],
            ],
        );
    }

    #[test]
    fn compatible_content_admissions_use_a_semantic_set() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = "word", admits(IF, ELSE))]
    first: Word,
    #[lex(pattern = "word", admits(ELSE, IF))]
    second: Word,
}

#[derive(recursa::Node)]
#[tok("IF")]
struct IfToken;

#[derive(recursa::Node)]
#[tok("ELSE")]
struct ElseToken;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let content = model
            .token_definitions
            .iter()
            .find_map(|definition| match &definition.kind {
                TokenDefinitionKind::Content(content) => Some(content),
                TokenDefinitionKind::Fixed(_) => None,
            })
            .unwrap();
        assert_eq!(content.admission_set.len(), 2);
        assert_eq!(
            content
                .authored_admissions
                .iter()
                .map(|authored| authored
                    .definitions
                    .iter()
                    .map(|definition| model.token_definitions[definition.index()].name.as_str())
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [vec!["IF", "ELSE"], vec!["ELSE", "IF"]],
        );
        assert_eq!(
            model
                .token_uses
                .iter()
                .filter(|token_use| token_use.role == TokenUseRole::ContentAdmission)
                .map(
                    |token_use| model.token_definitions[token_use.definition.index()]
                        .name
                        .as_str()
                )
                .collect::<Vec<_>>(),
            ["IF", "ELSE", "ELSE", "IF"],
        );
    }

    #[test]
    fn unannotated_pratt_variants_are_atoms_without_an_operator_role() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[tok("ATOM")]
    Atom,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.nodes[0].variants()[0].pratt_role, None);
    }

    #[test]
    fn infix_tokens_precede_the_entire_second_field_attachment() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[parse(infix, lbp = 20, rbp = 21)]
    #[tok("+")]
    Infix(Box<Expr>, #[tok("AND")] Box<Expr>),
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let rendered = model.nodes[0].variants()[0]
            .elements
            .iter()
            .map(|element| match element {
                super::GrammarElement::SemanticField { .. } => "field",
                super::GrammarElement::FixedToken { token_use, .. } => {
                    let definition = model.token_uses[token_use.index()].definition;
                    model.token_definitions[definition.index()].name.as_str()
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered, ["field", "+", "AND", "field"]);
    }

    #[test]
    fn multiple_or_malformed_pratt_roles_remain_diagnostics() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[parse(prefix)]
    #[parse(postfix)]
    Multiple(Box<Expr>),

    #[parse(sideways)]
    Malformed(Box<Expr>),
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.kind == SourceModelDiagnosticKind::InvalidPrattTokenPlacement
                })
                .count(),
            2,
        );
    }
}
