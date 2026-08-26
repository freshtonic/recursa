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

use crate::lowering::{
    ElementId as GrammarElementId, ModuleId, NodeId, TokenDefinitionId, TokenUseId,
};
use crate::resolution::{
    CatalogType, ContentTokenId, ResolvedFieldId, ResolvedFieldStyle, ResolvedGenericArgument,
    ResolvedNodeBody, ResolvedNodeField, ResolvedSourceGrammar, ResolvedType, ResolvedTypeKind,
    ResolvedVariantId, SemanticTarget,
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

    #[cfg(test)]
    pub(crate) fn source_model_mut(&mut self) -> &mut SourceModel {
        &mut self.source_model
    }

    pub(crate) fn source_order_key(&self, site: &SourceSite) -> SourceOrderKey {
        canonical_source_order_key(site)
    }

    pub(crate) fn grammar_configuration_site(&self, range: Range<u32>) -> SourceSite {
        let root = self
            .resolved
            .tree
            .inclusions
            .first()
            .expect("a resolved grammar has a root module inclusion");
        source_site(
            &self.resolved,
            SourceOrigin {
                module: root.id,
                source: self.resolved.configuration.source,
            },
            range,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceModel {
    pub(crate) grammar_trait_defaults: Vec<ConfiguredTraitSelection>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) token_definitions: Vec<TokenDefinition>,
    pub(crate) lexer_records: Vec<NonTokenLexerRecord>,
    pub(crate) token_uses: Vec<TokenUse>,
    pub(crate) post_lex: Option<syn::Path>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NonTokenLexerRecord {
    pub(crate) pattern: String,
    pub(crate) outcome: NonTokenLexerOutcome,
    pub(crate) site: SourceSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NonTokenLexerOutcome {
    NonEmitting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Node {
    pub(crate) resolved_node: NodeId,
    pub(crate) name: String,
    pub(crate) body: NodeBody,
    pub(crate) configured_traits: ConfiguredTraitPlan,
    pub(crate) pretty: PrettyHints,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NodeBody {
    Struct(Vec<GrammarElement>),
    Presence(Vec<GrammarElement>),
    Enum(Vec<Variant>),
    Pratt(Vec<SourcePrattVariant>),
}

impl Node {
    pub(crate) const fn shape(&self) -> NodeShape {
        match self.body {
            NodeBody::Struct(_) => NodeShape::Struct,
            NodeBody::Presence(_) => NodeShape::Presence,
            NodeBody::Enum(_) => NodeShape::Enum,
            NodeBody::Pratt(_) => NodeShape::Pratt,
        }
    }

    pub(crate) fn elements(&self) -> &[GrammarElement] {
        match &self.body {
            NodeBody::Struct(elements) | NodeBody::Presence(elements) => elements,
            NodeBody::Enum(_) | NodeBody::Pratt(_) => &[],
        }
    }

    pub(crate) fn variants(&self) -> &[Variant] {
        match &self.body {
            NodeBody::Enum(variants) => variants,
            NodeBody::Struct(_) | NodeBody::Presence(_) | NodeBody::Pratt(_) => &[],
        }
    }

    pub(crate) fn pratt_variants(&self) -> &[SourcePrattVariant] {
        match &self.body {
            NodeBody::Pratt(variants) => variants,
            NodeBody::Struct(_) | NodeBody::Presence(_) | NodeBody::Enum(_) => &[],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Variant {
    pub(crate) resolved_variant: ResolvedVariantId,
    pub(crate) name: String,
    pub(crate) elements: Vec<GrammarElement>,
    pub(crate) site: SourceSite,
    pub(crate) pretty: PrettyHints,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PrettyHints {
    pub(crate) group: Option<PrettyGroup>,
    pub(crate) indent: bool,
    pub(crate) break_before: Option<PrettyBreak>,
    pub(crate) break_after: Option<PrettyBreak>,
    pub(crate) separator: Option<PrettySeparatorPosition>,
    pub(crate) trailing_separator: Option<PrettyTrailingSeparator>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrettyGroup {
    Consistent,
    Inconsistent,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrettyBreak {
    Soft,
    Hard,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrettySeparatorPosition {
    Leading,
    Trailing,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrettyTrailingSeparator {
    Always,
    Never,
}

impl PrettyHints {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourcePrattVariant {
    pub(crate) common: Variant,
    pub(crate) position: SourcePrattPosition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourcePrattPosition {
    Atom,
    Prefix { bp: u8 },
    Infix { lbp: u8, rbp: u8 },
    Postfix { bp: u8 },
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
    Pratt,
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
        attachment: Option<FixedTokenAttachment>,
        separator: Option<Separator>,
        pretty: PrettyHints,
        attachment_before: usize,
        attachment_after: usize,
        site: SourceSite,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FixedTokenAttachment {
    pub(crate) before: Vec<AttachedFixedToken>,
    pub(crate) after: Vec<AttachedFixedToken>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttachedFixedToken {
    pub(crate) token_use: TokenUseId,
    pub(crate) site: SourceSite,
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

    pub(crate) fn attachment(&self) -> Option<&FixedTokenAttachment> {
        match self {
            Self::SemanticField { attachment, .. } => attachment.as_ref(),
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
    LexicalBase(LexicalBaseTokenDefinition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FixedTokenDefinition {
    pub(crate) spelling: String,
    pub(crate) authored_classifications: Vec<AuthoredFixedClassification>,
    pub(crate) pretty_spacing: PrettySpacing,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PrettySpacing {
    #[default]
    Auto,
    Tight,
    Before,
    After,
    Around,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthoredFixedTokenClass {
    Keyword,
    Punctuation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthoredFixedClassification {
    pub(crate) class: AuthoredFixedTokenClass,
    pub(crate) site: SourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContentTokenDefinition {
    pub(crate) semantic: ContentTokenId,
    pub(crate) pattern: String,
    pub(crate) callback: Option<syn::Path>,
    pub(crate) priority: Option<u32>,
    pub(crate) admission_set: Vec<TokenDefinitionId>,
    pub(crate) authored_admissions: Vec<AuthoredAdmissions>,
    pub(crate) text: ContentTextStorage,
    pub(crate) configured_traits: ConfiguredTraitPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalBaseTokenDefinition {
    pub(crate) pattern: String,
    pub(crate) callback: Option<syn::Path>,
    pub(crate) priority: Option<u32>,
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
    pub(crate) module: ModuleId,
    pub(crate) logical_module: Vec<String>,
    pub(crate) source: crate::SourceId,
    pub(crate) path: PathBuf,
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SourceOrderKey {
    module: usize,
    start: u32,
    end: u32,
}

impl SourceOrderKey {
    pub(crate) const LAST: Self = Self {
        module: usize::MAX,
        start: u32::MAX,
        end: u32::MAX,
    };
}

#[derive(Clone, Copy)]
struct SourceOrigin {
    module: ModuleId,
    source: crate::SourceId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SourceModelDiagnosticKind {
    InvalidConfiguration,
    InvalidPrettyHint,
    InapplicablePrettyHint,
    UnsafePrettyHint,
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
    InvalidSeparator,
    UnsupportedFixedSyntax,
    InvalidTokenPlacement,
    InvalidPrattTokenPlacement,
    ConfiguredOrdinaryDeriveConflict,
    MultiplePostLexHooks,
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
    tokens.collect_fixed_definitions(&input.resolved, input.cfg, &mut diagnostics);
    if tokens.post_lex_sites.len() > 1 {
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::MultiplePostLexHooks,
            sites: tokens.post_lex_sites.clone(),
            message: "a grammar accepts exactly one post_lex hook declaration".to_owned(),
        });
    }
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
    builder.lower_central_content_definitions();
    builder.lower_nodes();
    builder.validate_pretty_spacing();
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
        position: SourcePrattPosition,
        second_field_attachment: Option<usize>,
    },
}

#[derive(Default)]
struct LoweredFields {
    elements: Vec<GrammarElement>,
    attachment_starts: Vec<usize>,
}

#[derive(Clone)]
enum PrettyBoundaryKind {
    Fixed(TokenDefinitionId),
    Dynamic(String),
}

#[derive(Clone)]
struct PrettyBoundary {
    kind: PrettyBoundaryKind,
    site: SourceSite,
}

#[derive(Clone, Copy)]
enum PrettyBoundarySide {
    First,
    Last,
}

fn pattern_may_emit_any(pattern: &str, spelling: &str) -> bool {
    let Ok(hir) = regex_syntax::Parser::new().parse(pattern) else {
        return true;
    };
    spelling
        .chars()
        .any(|character| hir_may_emit(&hir, &|candidate| candidate == character))
}

fn pattern_may_emit_xid(pattern: &str) -> bool {
    let Ok(hir) = regex_syntax::Parser::new().parse(pattern) else {
        return true;
    };
    hir_may_emit(&hir, &unicode_ident::is_xid_continue)
}

fn hir_may_emit(hir: &regex_syntax::hir::Hir, predicate: &impl Fn(char) -> bool) -> bool {
    use regex_syntax::hir::{Class, HirKind};

    match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => false,
        HirKind::Literal(literal) => std::str::from_utf8(&literal.0)
            .map(|text| text.chars().any(predicate))
            .unwrap_or(true),
        HirKind::Class(Class::Unicode(class)) => class.ranges().iter().any(|range| {
            (u32::from(range.start())..=u32::from(range.end()))
                .filter_map(char::from_u32)
                .any(predicate)
        }),
        HirKind::Class(Class::Bytes(class)) => class
            .ranges()
            .iter()
            .any(|range| (range.start()..=range.end()).any(|byte| predicate(char::from(byte)))),
        HirKind::Repetition(repetition) => hir_may_emit(&repetition.sub, predicate),
        HirKind::Capture(capture) => hir_may_emit(&capture.sub, predicate),
        HirKind::Concat(parts) | HirKind::Alternation(parts) => {
            parts.iter().any(|part| hir_may_emit(part, predicate))
        }
    }
}

fn hir_matches_single_char(hir: &regex_syntax::hir::Hir, required: char) -> (bool, bool) {
    use regex_syntax::hir::{Class, HirKind};

    match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => (true, false),
        HirKind::Literal(literal) => {
            std::str::from_utf8(&literal.0).map_or((false, false), |text| {
                let mut chars = text.chars();
                let first = chars.next();
                (
                    first.is_none(),
                    first == Some(required) && chars.next().is_none(),
                )
            })
        }
        HirKind::Class(Class::Unicode(class)) => (
            false,
            class
                .ranges()
                .iter()
                .any(|range| range.start() <= required && required <= range.end()),
        ),
        HirKind::Class(Class::Bytes(class)) => (
            false,
            u8::try_from(required).is_ok_and(|required| {
                class
                    .ranges()
                    .iter()
                    .any(|range| range.start() <= required && required <= range.end())
            }),
        ),
        HirKind::Repetition(repetition) => {
            let (sub_empty, sub_char) = hir_matches_single_char(&repetition.sub, required);
            let can_repeat = repetition.max.is_none_or(|max| max >= 1);
            (
                repetition.min == 0 || sub_empty,
                can_repeat && sub_char && (repetition.min <= 1 || sub_empty),
            )
        }
        HirKind::Capture(capture) => hir_matches_single_char(&capture.sub, required),
        HirKind::Concat(parts) => {
            let properties = parts
                .iter()
                .map(|part| hir_matches_single_char(part, required))
                .collect::<Vec<_>>();
            let empty = properties.iter().all(|(empty, _)| *empty);
            let one = properties.iter().enumerate().any(|(candidate, (_, one))| {
                *one && properties
                    .iter()
                    .enumerate()
                    .all(|(index, (empty, _))| index == candidate || *empty)
            });
            (empty, one)
        }
        HirKind::Alternation(parts) => parts.iter().fold((false, false), |result, part| {
            let part = hir_matches_single_char(part, required);
            (result.0 || part.0, result.1 || part.1)
        }),
    }
}

fn parse_pretty_hints(metas: &[Meta]) -> Result<PrettyHints, String> {
    let named_pretty = metas
        .iter()
        .filter(|meta| {
            meta.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "pretty")
        })
        .collect::<Vec<_>>();
    if named_pretty
        .iter()
        .any(|meta| !meta.path().is_ident("pretty"))
    {
        return Err("#[pretty(...)] must use the exact unqualified `pretty` path".to_owned());
    }
    let pretty = named_pretty;
    if pretty.len() > 1 {
        return Err("duplicate #[pretty(...)] attribute".to_owned());
    }
    let Some(Meta::List(list)) = pretty.first().copied() else {
        return if pretty.is_empty() {
            Ok(PrettyHints::default())
        } else {
            Err("#[pretty] requires a non-empty hint list".to_owned())
        };
    };
    let entries = syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated
        .parse2(list.tokens.clone())
        .map_err(|error| format!("invalid #[pretty(...)] syntax: {error}"))?;
    if entries.is_empty() {
        return Err("#[pretty(...)] requires at least one hint".to_owned());
    }
    let mut hints = PrettyHints::default();
    for entry in entries {
        let key = entry
            .path()
            .get_ident()
            .map(ToString::to_string)
            .ok_or_else(|| "Pretty hint keys must be unqualified identifiers".to_owned())?;
        if matches!(entry, Meta::Path(_)) {
            if key != "indent" {
                return Err(format!("unknown bare Pretty hint `{key}`"));
            }
            if hints.indent {
                return Err("duplicate Pretty hint `indent`".to_owned());
            }
            hints.indent = true;
            continue;
        }
        let Meta::NameValue(value) = entry else {
            return Err(format!("Pretty hint `{key}` must use `key = value`"));
        };
        let syn::Expr::Path(value) = value.value else {
            return Err(format!("Pretty hint `{key}` requires an identifier value"));
        };
        let value = value
            .path
            .get_ident()
            .map(ToString::to_string)
            .ok_or_else(|| {
                format!("Pretty hint `{key}` requires an unqualified identifier value")
            })?;
        match key.as_str() {
            "group" => {
                if hints.group.is_some() {
                    return Err("duplicate Pretty hint `group`".to_owned());
                }
                hints.group = Some(match value.as_str() {
                    "consistent" => PrettyGroup::Consistent,
                    "inconsistent" => PrettyGroup::Inconsistent,
                    _ => {
                        return Err(
                            "Pretty `group` must be `consistent` or `inconsistent`".to_owned()
                        );
                    }
                });
            }
            "break_before" | "break_after" => {
                let parsed = match value.as_str() {
                    "soft" => PrettyBreak::Soft,
                    "hard" => PrettyBreak::Hard,
                    _ => return Err(format!("Pretty `{key}` must be `soft` or `hard`")),
                };
                let slot = if key == "break_before" {
                    &mut hints.break_before
                } else {
                    &mut hints.break_after
                };
                if slot.replace(parsed).is_some() {
                    return Err(format!("duplicate Pretty hint `{key}`"));
                }
            }
            "separator" => {
                if hints.separator.is_some() {
                    return Err("duplicate Pretty hint `separator`".to_owned());
                }
                hints.separator = Some(match value.as_str() {
                    "leading" => PrettySeparatorPosition::Leading,
                    "trailing" => PrettySeparatorPosition::Trailing,
                    _ => {
                        return Err("Pretty `separator` must be `leading` or `trailing`".to_owned());
                    }
                });
            }
            "trailing_separator" => {
                if hints.trailing_separator.is_some() {
                    return Err("duplicate Pretty hint `trailing_separator`".to_owned());
                }
                hints.trailing_separator = Some(match value.as_str() {
                    "always" => PrettyTrailingSeparator::Always,
                    "never" => PrettyTrailingSeparator::Never,
                    _ => {
                        return Err(
                            "Pretty `trailing_separator` must be `always` or `never`".to_owned()
                        );
                    }
                });
            }
            _ => return Err(format!("unknown Pretty hint `{key}`")),
        }
    }
    Ok(hints)
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
    fn validate_pretty_spacing(&mut self) {
        let mut diagnostics = Vec::new();
        for node in &self.nodes {
            let pretty_enabled = node
                .configured_traits
                .effective
                .iter()
                .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty);
            if !pretty_enabled {
                self.report_disabled_pretty_hints(node, &mut diagnostics);
                continue;
            }
            if node.pretty.separator.is_some() || node.pretty.trailing_separator.is_some() {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites: Vec::new(),
                    message: "separator Pretty hints apply only to repeated fields".to_owned(),
                });
            }
            match &node.body {
                NodeBody::Struct(elements) | NodeBody::Presence(elements) => {
                    self.validate_pretty_scope(&node.pretty, elements, "Node", &mut diagnostics);
                    self.validate_pretty_element_boundaries(elements, &mut diagnostics);
                }
                NodeBody::Enum(variants) => {
                    self.validate_pretty_multi_scope(
                        &node.pretty,
                        variants.iter().map(|variant| variant.elements.as_slice()),
                        "enum Node",
                        &mut diagnostics,
                    );
                    for variant in variants {
                        self.validate_pretty_scope(
                            &variant.pretty,
                            &variant.elements,
                            "variant",
                            &mut diagnostics,
                        );
                        self.validate_pretty_element_boundaries(
                            &variant.elements,
                            &mut diagnostics,
                        );
                    }
                }
                NodeBody::Pratt(variants) => {
                    self.validate_pretty_multi_scope(
                        &node.pretty,
                        variants
                            .iter()
                            .map(|variant| variant.common.elements.as_slice()),
                        "Pratt Node",
                        &mut diagnostics,
                    );
                    let requires_grouping = variants
                        .iter()
                        .any(|variant| !matches!(variant.position, SourcePrattPosition::Atom));
                    let has_grouping = variants.iter().any(|variant| {
                        matches!(variant.position, SourcePrattPosition::Atom)
                            && self.is_pretty_pratt_grouping(node.resolved_node, &variant.common)
                    });
                    if requires_grouping && !has_grouping {
                        diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: variants
                                .first()
                                .map(|variant| variant.common.site.clone())
                                .into_iter()
                                .collect(),
                            message: format!(
                                "Pretty cannot repair precedence for Pratt Node `{}` because its grammar has no atom that encloses Self between fixed tokens",
                                node.name,
                            ),
                        });
                    }
                    for variant in variants {
                        self.validate_pretty_scope(
                            &variant.common.pretty,
                            &variant.common.elements,
                            "Pratt variant",
                            &mut diagnostics,
                        );
                        self.validate_pretty_element_boundaries(
                            &variant.common.elements,
                            &mut diagnostics,
                        );
                    }
                }
            }
        }
        self.diagnostics.extend(diagnostics);
    }

    fn report_disabled_pretty_hints(
        &self,
        node: &Node,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let mut report = |pretty: &PrettyHints, sites: Vec<SourceSite>| {
            if !pretty.is_empty() {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites,
                    message: "#[pretty(...)] requires the effective Pretty configured trait"
                        .to_owned(),
                });
            }
        };
        report(&node.pretty, Vec::new());
        let variants = match &node.body {
            NodeBody::Struct(elements) | NodeBody::Presence(elements) => {
                vec![(None, elements.as_slice())]
            }
            NodeBody::Enum(variants) => variants
                .iter()
                .map(|variant| {
                    (
                        Some((&variant.pretty, &variant.site)),
                        variant.elements.as_slice(),
                    )
                })
                .collect(),
            NodeBody::Pratt(variants) => variants
                .iter()
                .map(|variant| {
                    (
                        Some((&variant.common.pretty, &variant.common.site)),
                        variant.common.elements.as_slice(),
                    )
                })
                .collect(),
        };
        for (variant, elements) in variants {
            if let Some((pretty, site)) = variant {
                report(pretty, vec![site.clone()]);
            }
            for element in elements {
                if let GrammarElement::SemanticField { pretty, site, .. } = element {
                    report(pretty, vec![site.clone()]);
                }
            }
        }
    }

    fn validate_pretty_scope(
        &self,
        pretty: &PrettyHints,
        elements: &[GrammarElement],
        scope: &str,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        self.validate_pretty_scope_facts(
            pretty,
            self.pretty_scope_has_soft_opportunity(elements, &mut Vec::new()),
            scope,
            diagnostics,
        );
    }

    fn validate_pretty_scope_facts(
        &self,
        pretty: &PrettyHints,
        has_soft_opportunity: bool,
        scope: &str,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        if pretty.separator.is_some() || pretty.trailing_separator.is_some() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                sites: Vec::new(),
                message: format!("separator Pretty hints do not apply to a {scope}"),
            });
        }
        if pretty.group.is_some() && !has_soft_opportunity {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                sites: Vec::new(),
                message: format!("Pretty `group` on {scope} has no soft-break opportunity"),
            });
        }
        if pretty.indent && !has_soft_opportunity {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                sites: Vec::new(),
                message: format!("Pretty `indent` on {scope} has no possible break"),
            });
        }
        if (pretty.break_before.is_some() || pretty.break_after.is_some())
            && !self.pretty_trivia_supports_breaks()
        {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::UnsafePrettyHint,
                sites: Vec::new(),
                message: format!(
                    "Pretty breaks on {scope} require ignored lexer trivia accepting both spaces and newlines"
                ),
            });
        }
    }

    fn validate_pretty_multi_scope<'a>(
        &self,
        pretty: &PrettyHints,
        mut variants: impl Iterator<Item = &'a [GrammarElement]>,
        scope: &str,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let has_soft_opportunity = variants
            .any(|elements| self.pretty_scope_has_soft_opportunity(elements, &mut Vec::new()));
        self.validate_pretty_scope_facts(pretty, has_soft_opportunity, scope, diagnostics);
    }

    fn pretty_scope_has_soft_opportunity(
        &self,
        elements: &[GrammarElement],
        visiting: &mut Vec<NodeId>,
    ) -> bool {
        if matches!(
            (elements.first(), elements.last()),
            (
                Some(GrammarElement::FixedToken { token_use: open, .. }),
                Some(GrammarElement::FixedToken { token_use: close, .. })
            ) if matches!(
                (
                    self.fixed_definition_for_use(*open).spelling.as_str(),
                    self.fixed_definition_for_use(*close).spelling.as_str(),
                ),
                ("(", ")") | ("[", "]") | ("{", "}")
            )
        ) {
            return true;
        }
        if elements.iter().any(|element| {
            let GrammarElement::SemanticField {
                resolved_field,
                separator,
                ..
            } = element
            else {
                return false;
            };
            separator.is_some()
                || self.field_has_repetition(*resolved_field).unwrap_or(true)
                || self.pretty_type_has_soft_opportunity(
                    &self.resolved_field(*resolved_field).ty,
                    visiting,
                )
        }) {
            return true;
        }
        elements.windows(2).any(|adjacent| {
            let [left, right] = adjacent else {
                unreachable!()
            };
            let left =
                self.pretty_element_boundaries(left, PrettyBoundarySide::Last, &mut Vec::new());
            let right =
                self.pretty_element_boundaries(right, PrettyBoundarySide::First, &mut Vec::new());
            left.iter().any(|left| {
                right
                    .iter()
                    .any(|right| self.pretty_boundary_emits_soft_break(left, right))
            })
        })
    }

    fn pretty_type_has_soft_opportunity(
        &self,
        ty: &ResolvedType,
        visiting: &mut Vec<NodeId>,
    ) -> bool {
        match &ty.kind {
            ResolvedTypeKind::Semantic {
                target, arguments, ..
            } => match target {
                SemanticTarget::Node(node) => {
                    if visiting.contains(node) {
                        return false;
                    }
                    visiting.push(*node);
                    let result = self
                        .nodes
                        .iter()
                        .find(|candidate| candidate.resolved_node == *node)
                        .is_some_and(|node| match &node.body {
                            NodeBody::Struct(elements) | NodeBody::Presence(elements) => {
                                self.pretty_scope_has_soft_opportunity(elements, visiting)
                            }
                            NodeBody::Enum(variants) => variants.iter().any(|variant| {
                                self.pretty_scope_has_soft_opportunity(&variant.elements, visiting)
                            }),
                            NodeBody::Pratt(variants) => variants.iter().any(|variant| {
                                self.pretty_scope_has_soft_opportunity(
                                    &variant.common.elements,
                                    visiting,
                                )
                            }),
                        });
                    visiting.pop();
                    result
                }
                SemanticTarget::Catalog(CatalogType::Vec | CatalogType::Vec1) => true,
                SemanticTarget::Catalog(CatalogType::Box | CatalogType::Option) => arguments
                    .iter()
                    .find_map(|argument| match argument {
                        ResolvedGenericArgument::Type(inner) => {
                            Some(self.pretty_type_has_soft_opportunity(inner, visiting))
                        }
                        ResolvedGenericArgument::Lifetime(_)
                        | ResolvedGenericArgument::Unsupported(_) => None,
                    })
                    .unwrap_or(false),
                SemanticTarget::TypeAlias(alias) => self
                    .resolved
                    .type_aliases
                    .iter()
                    .find(|candidate| candidate.id == *alias)
                    .and_then(|alias| alias.target.as_ref())
                    .is_some_and(|target| self.pretty_type_has_soft_opportunity(target, visiting)),
                SemanticTarget::ContentToken(_) | SemanticTarget::GenericParameter(_) => false,
            },
            ResolvedTypeKind::Group(inner)
            | ResolvedTypeKind::Parenthesized(inner)
            | ResolvedTypeKind::Reference(inner) => {
                self.pretty_type_has_soft_opportunity(inner, visiting)
            }
            ResolvedTypeKind::Tuple(_) | ResolvedTypeKind::Unsupported => false,
        }
    }

    fn is_pretty_pratt_grouping(&self, node: NodeId, variant: &Variant) -> bool {
        if let [
            GrammarElement::SemanticField {
                resolved_field,
                attachment: Some(attachment),
                ..
            },
        ] = variant.elements.as_slice()
        {
            return !attachment.before.is_empty()
                && !attachment.after.is_empty()
                && self
                    .is_direct_recursive_operand(&self.resolved_field(*resolved_field).ty, node);
        }
        let [
            GrammarElement::FixedToken { .. },
            GrammarElement::SemanticField { resolved_field, .. },
            GrammarElement::FixedToken { .. },
        ] = variant.elements.as_slice()
        else {
            return false;
        };
        self.is_direct_recursive_operand(&self.resolved_field(*resolved_field).ty, node)
    }

    fn is_direct_recursive_operand(&self, ty: &ResolvedType, node: NodeId) -> bool {
        match &ty.kind {
            ResolvedTypeKind::Semantic {
                target, arguments, ..
            } => match target {
                SemanticTarget::Node(candidate) => *candidate == node,
                SemanticTarget::Catalog(CatalogType::Box) => {
                    let mut types = arguments.iter().filter_map(|argument| match argument {
                        ResolvedGenericArgument::Type(ty) => Some(ty),
                        ResolvedGenericArgument::Lifetime(_)
                        | ResolvedGenericArgument::Unsupported(_) => None,
                    });
                    let Some(inner) = types.next() else {
                        return false;
                    };
                    types.next().is_none() && self.is_direct_recursive_operand(inner, node)
                }
                SemanticTarget::TypeAlias(alias) => self
                    .resolved
                    .type_aliases
                    .iter()
                    .find(|candidate| candidate.id == *alias)
                    .and_then(|alias| alias.target.as_ref())
                    .is_some_and(|ty| self.is_direct_recursive_operand(ty, node)),
                SemanticTarget::Catalog(
                    CatalogType::Option | CatalogType::Vec | CatalogType::Vec1,
                )
                | SemanticTarget::ContentToken(_)
                | SemanticTarget::GenericParameter(_) => false,
            },
            ResolvedTypeKind::Group(inner)
            | ResolvedTypeKind::Parenthesized(inner)
            | ResolvedTypeKind::Reference(inner) => self.is_direct_recursive_operand(inner, node),
            ResolvedTypeKind::Tuple(_) | ResolvedTypeKind::Unsupported => false,
        }
    }

    fn validate_pretty_element_boundaries(
        &self,
        elements: &[GrammarElement],
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        for (index, element) in elements.iter().enumerate() {
            let GrammarElement::SemanticField {
                resolved_field,
                attachment,
                separator,
                pretty,
                site,
                ..
            } = element
            else {
                continue;
            };
            if pretty.separator.is_some() && separator.is_none() {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty `separator` requires a repeated field with #[sep]".to_owned(),
                });
            }
            if pretty.trailing_separator.is_some()
                && !separator
                    .as_ref()
                    .is_some_and(|separator| separator.optional_trailing)
            {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty `trailing_separator` requires #[sep(..., trailing)]"
                        .to_owned(),
                });
            }
            let attachment_start = index;
            let attachment_end = index;
            let attachment_has_soft_opportunity = attachment.as_ref().is_some_and(|attachment| {
                self.pretty_attachment_has_soft_opportunity(*resolved_field, attachment)
            });
            let has_internal_break = separator.is_some()
                || self.field_has_repetition(*resolved_field).unwrap_or(true)
                || attachment_has_soft_opportunity
                || self
                    .pretty_scope_has_soft_opportunity(&elements[index..=index], &mut Vec::new());
            if pretty.group.is_some() && !has_internal_break {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty `group` on field has no soft-break opportunity".to_owned(),
                });
            }
            if pretty.indent && !has_internal_break {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty `indent` on field has no possible break".to_owned(),
                });
            }
            if pretty.break_before.is_some() && attachment_start == 0 {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty `break_before` requires a preceding grammar element"
                        .to_owned(),
                });
            }
            if pretty.break_after.is_some() && attachment_end + 1 == elements.len() {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InapplicablePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty `break_after` requires a following grammar element".to_owned(),
                });
            }
            let boundary_is_tight = |left: &GrammarElement, right: &GrammarElement| {
                let left =
                    self.pretty_element_boundaries(left, PrettyBoundarySide::Last, &mut Vec::new());
                let right = self.pretty_element_boundaries(
                    right,
                    PrettyBoundarySide::First,
                    &mut Vec::new(),
                );
                left.iter()
                    .any(|left| self.pretty_boundary_forces_tight_after(left))
                    || right
                        .iter()
                        .any(|right| self.pretty_boundary_forces_tight_before(right))
            };
            let tight_before = attachment_start.checked_sub(1).is_some_and(|before| {
                boundary_is_tight(&elements[before], &elements[attachment_start])
            });
            let tight_after = elements
                .get(attachment_end + 1)
                .is_some_and(|after| boundary_is_tight(&elements[attachment_end], after));
            if (pretty.break_before.is_some() && tight_before)
                || (pretty.break_after.is_some() && tight_after)
            {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::UnsafePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty attachment break crosses a mandatory-tight boundary"
                        .to_owned(),
                });
            }
            if (pretty.break_before.is_some() || pretty.break_after.is_some())
                && !self.pretty_trivia_supports_breaks()
            {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::UnsafePrettyHint,
                    sites: vec![site.clone()],
                    message: "Pretty attachment breaks require ignored lexer trivia accepting both spaces and newlines"
                        .to_owned(),
                });
            }
        }
        for adjacent in elements.windows(2) {
            let [left, right] = adjacent else {
                unreachable!()
            };
            let left_boundaries =
                self.pretty_element_boundaries(left, PrettyBoundarySide::Last, &mut Vec::new());
            let right_boundaries =
                self.pretty_element_boundaries(right, PrettyBoundarySide::First, &mut Vec::new());
            for left in &left_boundaries {
                for right in &right_boundaries {
                    self.validate_pretty_boundary(left, right, diagnostics);
                }
            }
        }
    }

    fn pretty_attachment_has_soft_opportunity(
        &self,
        field: ResolvedFieldId,
        attachment: &FixedTokenAttachment,
    ) -> bool {
        let fixed = |token: &AttachedFixedToken| PrettyBoundary {
            kind: PrettyBoundaryKind::Fixed(self.tokens.uses[token.token_use.index()].definition),
            site: token.site.clone(),
        };
        let before = attachment.before.iter().map(fixed).collect::<Vec<_>>();
        let after = attachment.after.iter().map(fixed).collect::<Vec<_>>();
        let site = attachment
            .before
            .first()
            .or_else(|| attachment.after.first())
            .map(|token| token.site.clone())
            .expect("an attachment has fixed syntax");
        let first = self.pretty_type_boundaries(
            &self.resolved_field(field).ty,
            PrettyBoundarySide::First,
            &site,
            &mut Vec::new(),
        );
        let last = self.pretty_type_boundaries(
            &self.resolved_field(field).ty,
            PrettyBoundarySide::Last,
            &site,
            &mut Vec::new(),
        );
        before
            .windows(2)
            .any(|pair| self.pretty_boundary_emits_soft_break(&pair[0], &pair[1]))
            || after
                .windows(2)
                .any(|pair| self.pretty_boundary_emits_soft_break(&pair[0], &pair[1]))
            || before.last().is_some_and(|left| {
                first
                    .iter()
                    .any(|right| self.pretty_boundary_emits_soft_break(left, right))
            })
            || after.first().is_some_and(|right| {
                last.iter()
                    .any(|left| self.pretty_boundary_emits_soft_break(left, right))
            })
    }

    fn pretty_trivia_supports_breaks(&self) -> bool {
        [' ', '\n'].into_iter().all(|required| {
            self.tokens.lexer_records.iter().any(|record| {
                regex_syntax::Parser::new()
                    .parse(&record.pattern)
                    .is_ok_and(|hir| hir_matches_single_char(&hir, required).1)
            })
        })
    }

    fn field_has_repetition(&self, field: ResolvedFieldId) -> Option<bool> {
        fn repeated(builder: &SourceModelBuilder<'_>, ty: &ResolvedType) -> Option<bool> {
            match &ty.kind {
                ResolvedTypeKind::Semantic {
                    target, arguments, ..
                } => match target {
                    SemanticTarget::Catalog(CatalogType::Vec | CatalogType::Vec1) => Some(true),
                    SemanticTarget::Catalog(CatalogType::Box | CatalogType::Option) => arguments
                        .iter()
                        .filter_map(|argument| match argument {
                            ResolvedGenericArgument::Type(ty) => Some(ty),
                            ResolvedGenericArgument::Lifetime(_)
                            | ResolvedGenericArgument::Unsupported(_) => None,
                        })
                        .next()
                        .and_then(|ty| repeated(builder, ty)),
                    SemanticTarget::TypeAlias(alias) => builder
                        .resolved
                        .type_aliases
                        .iter()
                        .find(|candidate| candidate.id == *alias)
                        .and_then(|alias| alias.target.as_ref())
                        .and_then(|ty| repeated(builder, ty)),
                    SemanticTarget::Node(_) | SemanticTarget::ContentToken(_) => Some(false),
                    SemanticTarget::GenericParameter(_) => None,
                },
                ResolvedTypeKind::Group(inner)
                | ResolvedTypeKind::Parenthesized(inner)
                | ResolvedTypeKind::Reference(inner) => repeated(builder, inner),
                ResolvedTypeKind::Tuple(_) | ResolvedTypeKind::Unsupported => Some(false),
            }
        }
        repeated(self, &self.resolved_field(field).ty)
    }

    fn validate_pretty_boundary(
        &self,
        left: &PrettyBoundary,
        right: &PrettyBoundary,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let left_fixed = match left.kind {
            PrettyBoundaryKind::Fixed(id) => Some(self.fixed_definition_by_id(id)),
            PrettyBoundaryKind::Dynamic(_) => None,
        };
        let right_fixed = match right.kind {
            PrettyBoundaryKind::Fixed(id) => Some(self.fixed_definition_by_id(id)),
            PrettyBoundaryKind::Dynamic(_) => None,
        };
        let left_requests_space = matches!(
            left_fixed.map(|fixed| fixed.pretty_spacing),
            Some(PrettySpacing::After | PrettySpacing::Around)
        );
        let right_requests_space = matches!(
            right_fixed.map(|fixed| fixed.pretty_spacing),
            Some(PrettySpacing::Before | PrettySpacing::Around)
        );
        if left_requests_space || right_requests_space {
            return;
        }
        let forced_tight = matches!(
            left_fixed.map(|fixed| fixed.pretty_spacing),
            Some(PrettySpacing::Tight | PrettySpacing::Before)
        ) || matches!(
            right_fixed.map(|fixed| fixed.pretty_spacing),
            Some(PrettySpacing::Tight | PrettySpacing::After)
        );
        if !forced_tight {
            return;
        }

        match (&left.kind, &right.kind) {
            (PrettyBoundaryKind::Fixed(_), PrettyBoundaryKind::Fixed(_)) => {
                self.validate_pretty_fixed_boundary(
                    left_fixed.expect("fixed boundary"),
                    &left.site,
                    right_fixed.expect("fixed boundary"),
                    &right.site,
                    diagnostics,
                );
            }
            (PrettyBoundaryKind::Dynamic(pattern), PrettyBoundaryKind::Fixed(_)) => {
                let right_fixed = right_fixed.expect("fixed boundary");
                if pattern_may_emit_xid(pattern)
                    && right_fixed
                        .spelling
                        .chars()
                        .next()
                        .is_some_and(unicode_ident::is_xid_continue)
                {
                    diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![left.site.clone(), right.site.clone()],
                            message: "Pretty spacing would suppress a Unicode identifier boundary involving regex-backed content".to_owned(),
                        });
                } else if pattern_may_emit_any(pattern, &right_fixed.spelling) {
                    diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![left.site.clone(), right.site.clone()],
                            message: "Pretty spacing would create an unproved lexer boundary involving regex-backed content".to_owned(),
                        });
                }
            }
            (PrettyBoundaryKind::Fixed(_), PrettyBoundaryKind::Dynamic(pattern)) => {
                let left_fixed = left_fixed.expect("fixed boundary");
                if left_fixed
                    .spelling
                    .chars()
                    .next_back()
                    .is_some_and(unicode_ident::is_xid_continue)
                    && pattern_may_emit_xid(pattern)
                {
                    diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![left.site.clone(), right.site.clone()],
                            message: "Pretty spacing would suppress a Unicode identifier boundary involving regex-backed content".to_owned(),
                        });
                } else if pattern_may_emit_any(pattern, &left_fixed.spelling) {
                    diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![left.site.clone(), right.site.clone()],
                            message: "Pretty spacing would create an unproved lexer boundary involving regex-backed content".to_owned(),
                        });
                }
            }
            (PrettyBoundaryKind::Dynamic(_), PrettyBoundaryKind::Dynamic(_)) => {}
        }
    }

    fn validate_pretty_fixed_boundary(
        &self,
        left_fixed: &FixedTokenDefinition,
        left_site: &SourceSite,
        right_fixed: &FixedTokenDefinition,
        right_site: &SourceSite,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let left_requests_space = matches!(
            left_fixed.pretty_spacing,
            PrettySpacing::After | PrettySpacing::Around
        );
        let right_requests_space = matches!(
            right_fixed.pretty_spacing,
            PrettySpacing::Before | PrettySpacing::Around
        );
        if left_requests_space || right_requests_space {
            return;
        }
        let forced_tight = matches!(
            left_fixed.pretty_spacing,
            PrettySpacing::Tight | PrettySpacing::Before
        ) || matches!(
            right_fixed.pretty_spacing,
            PrettySpacing::Tight | PrettySpacing::After
        );
        if !forced_tight {
            return;
        }

        let left_tail = left_fixed.spelling.chars().next_back();
        let right_head = right_fixed.spelling.chars().next();
        if left_tail.is_some_and(unicode_ident::is_xid_continue)
            && right_head.is_some_and(unicode_ident::is_xid_continue)
        {
            diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidConfiguration,
                    sites: vec![left_site.clone(), right_site.clone()],
                    message: format!(
                        "Pretty spacing would suppress a Unicode identifier boundary between `{}` and `{}`",
                        left_fixed.spelling, right_fixed.spelling,
                    ),
                });
            return;
        }

        let combined = format!("{}{}", left_fixed.spelling, right_fixed.spelling);
        if let Some(longer) = self.tokens.definitions.iter().find_map(|definition| {
            let TokenDefinitionKind::Fixed(candidate) = &definition.kind else {
                return None;
            };
            (candidate.spelling.len() > left_fixed.spelling.len()
                && combined.starts_with(&candidate.spelling))
            .then_some(candidate)
        }) {
            diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidConfiguration,
                    sites: vec![left_site.clone(), right_site.clone()],
                    message: format!(
                        "Pretty spacing would suppress a lexer-required boundary between `{}` and `{}`; longest match `{}` would change tokenization",
                        left_fixed.spelling, right_fixed.spelling, longer.spelling,
                    ),
                });
        }
    }

    fn fixed_definition_by_id(&self, id: TokenDefinitionId) -> &FixedTokenDefinition {
        let TokenDefinitionKind::Fixed(fixed) = &self.tokens.definitions[id.index()].kind else {
            unreachable!("fixed boundary points to fixed syntax")
        };
        fixed
    }

    fn fixed_definition_for_use(&self, id: TokenUseId) -> &FixedTokenDefinition {
        self.fixed_definition_by_id(self.tokens.uses[id.index()].definition)
    }

    fn pretty_boundary_emits_soft_break(
        &self,
        left: &PrettyBoundary,
        right: &PrettyBoundary,
    ) -> bool {
        let fixed = |boundary: &PrettyBoundary| match boundary.kind {
            PrettyBoundaryKind::Fixed(id) => Some(self.fixed_definition_by_id(id)),
            PrettyBoundaryKind::Dynamic(_) => None,
        };
        let left_fixed = fixed(left);
        let right_fixed = fixed(right);
        if right_fixed.is_some_and(|right| {
            matches!(
                right.pretty_spacing,
                PrettySpacing::Before | PrettySpacing::Around
            )
        }) {
            return true;
        }
        if left_fixed.is_some_and(|left| {
            matches!(
                left.pretty_spacing,
                PrettySpacing::After | PrettySpacing::Around
            )
        }) {
            return true;
        }
        if left_fixed.is_some_and(|left| {
            matches!(
                left.pretty_spacing,
                PrettySpacing::Tight | PrettySpacing::Before
            ) || (left.pretty_spacing == PrettySpacing::Auto
                && matches!(left.spelling.as_str(), "(" | "[" | "{"))
        }) {
            return false;
        }
        match right_fixed {
            None => true,
            Some(right) if right.pretty_spacing != PrettySpacing::Auto => false,
            Some(right) => {
                let right_word = right
                    .spelling
                    .chars()
                    .next()
                    .is_some_and(unicode_ident::is_xid_start)
                    && right.spelling.chars().all(unicode_ident::is_xid_continue);
                if left_fixed.is_none() {
                    right_word
                } else {
                    true
                }
            }
        }
    }

    fn pretty_boundary_spacing(&self, boundary: &PrettyBoundary) -> Option<PrettySpacing> {
        match boundary.kind {
            PrettyBoundaryKind::Fixed(id) => Some(self.fixed_definition_by_id(id).pretty_spacing),
            PrettyBoundaryKind::Dynamic(_) => None,
        }
    }

    fn pretty_boundary_forces_tight_after(&self, boundary: &PrettyBoundary) -> bool {
        matches!(
            self.pretty_boundary_spacing(boundary),
            Some(PrettySpacing::Tight | PrettySpacing::Before)
        )
    }

    fn pretty_boundary_forces_tight_before(&self, boundary: &PrettyBoundary) -> bool {
        matches!(
            self.pretty_boundary_spacing(boundary),
            Some(PrettySpacing::Tight | PrettySpacing::After)
        )
    }

    fn pretty_element_boundaries(
        &self,
        element: &GrammarElement,
        side: PrettyBoundarySide,
        visiting: &mut Vec<NodeId>,
    ) -> Vec<PrettyBoundary> {
        match element {
            GrammarElement::FixedToken {
                token_use, site, ..
            } => vec![PrettyBoundary {
                kind: PrettyBoundaryKind::Fixed(self.tokens.uses[token_use.index()].definition),
                site: site.clone(),
            }],
            GrammarElement::SemanticField {
                resolved_field,
                site,
                ..
            } => self.pretty_type_boundaries(
                &self.resolved_field(*resolved_field).ty,
                side,
                site,
                visiting,
            ),
        }
    }

    fn pretty_type_boundaries(
        &self,
        ty: &ResolvedType,
        side: PrettyBoundarySide,
        site: &SourceSite,
        visiting: &mut Vec<NodeId>,
    ) -> Vec<PrettyBoundary> {
        match &ty.kind {
            ResolvedTypeKind::Semantic {
                target, arguments, ..
            } => match target {
                SemanticTarget::Node(node) => {
                    self.pretty_node_boundaries(*node, side, site, visiting)
                }
                SemanticTarget::ContentToken(content) => {
                    let pattern = self
                        .tokens
                        .definitions
                        .iter()
                        .find_map(|definition| match &definition.kind {
                            TokenDefinitionKind::Content(candidate)
                                if candidate.semantic == *content =>
                            {
                                Some(candidate.pattern.clone())
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    vec![PrettyBoundary {
                        kind: PrettyBoundaryKind::Dynamic(pattern),
                        site: site.clone(),
                    }]
                }
                SemanticTarget::Catalog(_) => arguments
                    .iter()
                    .find_map(|argument| match argument {
                        ResolvedGenericArgument::Type(inner) => {
                            Some(self.pretty_type_boundaries(inner, side, site, visiting))
                        }
                        ResolvedGenericArgument::Lifetime(_)
                        | ResolvedGenericArgument::Unsupported(_) => None,
                    })
                    .unwrap_or_default(),
                SemanticTarget::TypeAlias(alias) => self
                    .resolved
                    .type_aliases
                    .iter()
                    .find(|candidate| candidate.id == *alias)
                    .and_then(|alias| alias.target.as_ref())
                    .map(|target| self.pretty_type_boundaries(target, side, site, visiting))
                    .unwrap_or_default(),
                SemanticTarget::GenericParameter(_) => vec![PrettyBoundary {
                    kind: PrettyBoundaryKind::Dynamic(String::new()),
                    site: site.clone(),
                }],
            },
            ResolvedTypeKind::Group(inner)
            | ResolvedTypeKind::Parenthesized(inner)
            | ResolvedTypeKind::Reference(inner) => {
                self.pretty_type_boundaries(inner, side, site, visiting)
            }
            ResolvedTypeKind::Tuple(_) | ResolvedTypeKind::Unsupported => vec![PrettyBoundary {
                kind: PrettyBoundaryKind::Dynamic(String::new()),
                site: site.clone(),
            }],
        }
    }

    fn pretty_node_boundaries(
        &self,
        node: NodeId,
        side: PrettyBoundarySide,
        site: &SourceSite,
        visiting: &mut Vec<NodeId>,
    ) -> Vec<PrettyBoundary> {
        if visiting.contains(&node) {
            return vec![PrettyBoundary {
                kind: PrettyBoundaryKind::Dynamic(String::new()),
                site: site.clone(),
            }];
        }
        visiting.push(node);
        let Some(node) = self
            .nodes
            .iter()
            .find(|candidate| candidate.resolved_node == node)
        else {
            visiting.pop();
            return Vec::new();
        };
        let sequences = match &node.body {
            NodeBody::Struct(elements) | NodeBody::Presence(elements) => vec![elements.as_slice()],
            NodeBody::Enum(variants) => variants
                .iter()
                .map(|variant| variant.elements.as_slice())
                .collect(),
            NodeBody::Pratt(variants) => variants
                .iter()
                .map(|variant| variant.common.elements.as_slice())
                .collect(),
        };
        let mut boundaries = Vec::new();
        for elements in sequences {
            let element = match side {
                PrettyBoundarySide::First => elements.first(),
                PrettyBoundarySide::Last => elements.last(),
            };
            if let Some(element) = element {
                boundaries.extend(self.pretty_element_boundaries(element, side, visiting));
            }
        }
        visiting.pop();
        boundaries
    }

    fn pretty_fixed_element<'model, 'element>(
        &'model self,
        element: &'element GrammarElement,
    ) -> Option<(&'model FixedTokenDefinition, &'element SourceSite)> {
        let GrammarElement::FixedToken {
            token_use, site, ..
        } = element
        else {
            return None;
        };
        let definition = self.tokens.uses[token_use.index()].definition;
        let TokenDefinitionKind::Fixed(fixed) = &self.tokens.definitions[definition.index()].kind
        else {
            unreachable!("fixed element points to a fixed definition")
        };
        Some((fixed, site))
    }

    fn lower_central_content_definitions(&mut self) {
        for definition in std::mem::take(&mut self.tokens.central_content) {
            if let Some(semantic) = definition.semantic {
                self.tokens.define_content(
                    semantic,
                    definition.name,
                    definition.pattern,
                    definition.callback,
                    None,
                    Vec::new(),
                    AuthoredAdmissions {
                        site: definition.site.clone(),
                        definitions: Vec::new(),
                    },
                    definition.site,
                    &self.trait_planner.grammar_defaults,
                );
            } else {
                self.tokens.define_lexical_base(definition);
            }
        }
    }

    fn finish(mut self) -> SourceModelReport {
        normalize_source_order(&mut self.nodes, &mut self.tokens);
        let source_modeled = self.diagnostics.is_empty().then(|| SourceModeledGrammar {
            resolved: self.resolved,
            source_model: SourceModel {
                grammar_trait_defaults: self.trait_planner.grammar_defaults,
                nodes: self.nodes,
                token_definitions: self.tokens.definitions,
                lexer_records: self.tokens.lexer_records,
                token_uses: self.tokens.uses,
                post_lex: self.tokens.post_lex,
            },
        });
        SourceModelReport {
            diagnostics: self.diagnostics,
            source_modeled,
        }
    }

    fn reject_misplaced_content_declarations(&mut self) {
        for inclusion in self.resolved.tree.inclusions.clone() {
            let origin = SourceOrigin {
                module: inclusion.id,
                source: inclusion.source,
            };
            for item in &inclusion.items {
                let configured = match configured_metas(item_attrs(item), self.cfg) {
                    Ok(Some(configured)) => configured,
                    Ok(None) => continue,
                    Err(error) => {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![self.site(origin, error.range)],
                            message: error.message,
                        });
                        continue;
                    }
                };
                reject_misplaced_content_declarations(
                    &self.resolved,
                    origin,
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
                                    sites: vec![self.site(origin, error.range)],
                                    message: error.message,
                                });
                                continue;
                            }
                        };
                        reject_misplaced_content_declarations(
                            &self.resolved,
                            origin,
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
            let origin = SourceOrigin {
                module: node.module,
                source: node.source,
            };
            let configured = &node.metas;
            let body = match &node.body {
                ResolvedNodeBody::Struct { style } => {
                    reject_pratt_marker(
                        &self.resolved,
                        origin,
                        configured,
                        false,
                        &mut self.diagnostics,
                    );
                    reject_pratt_roles_off_pratt(
                        &self.resolved,
                        origin,
                        configured,
                        &mut self.diagnostics,
                    );
                    let field_ids = node.fields.iter().map(|field| field.id).collect::<Vec<_>>();
                    let fields = self.lower_fields(origin, &field_ids).into_elements();
                    let elements = PositionalTokenPlanner::lower(
                        &self.resolved,
                        &mut self.tokens,
                        &mut self.diagnostics,
                        origin,
                        configured,
                        fields,
                        TokenAttachment::StructBody,
                    );
                    if *style == ResolvedFieldStyle::Unit
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
                        origin,
                        configured,
                        &mut self.diagnostics,
                    );
                    if let Some(token_meta) =
                        configured.iter().find(|meta| meta.path().is_ident("tok"))
                    {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidTokenPlacement,
                            sites: vec![self.site(origin, span_range(token_meta.span()))],
                            message: "#[tok] belongs on each enum variant, not on the enum body"
                                .to_owned(),
                        });
                    }
                    reject_pratt_roles_off_pratt(
                        &self.resolved,
                        origin,
                        configured,
                        &mut self.diagnostics,
                    );
                    let is_pratt = reject_pratt_marker(
                        &self.resolved,
                        origin,
                        configured,
                        true,
                        &mut self.diagnostics,
                    );
                    if is_pratt {
                        NodeBody::Pratt(
                            resolved_variants
                                .iter()
                                .map(|variant| {
                                    reject_pratt_marker(
                                        &self.resolved,
                                        origin,
                                        &variant.metas,
                                        false,
                                        &mut self.diagnostics,
                                    );
                                    let position = SourcePrattPosition::from_attributes(
                                        &self.resolved,
                                        origin,
                                        &variant.metas,
                                        &mut self.diagnostics,
                                    );
                                    let body = self.lower_fields(origin, &variant.fields);
                                    let second_field_attachment = body.second_attachment_start();
                                    let elements = PositionalTokenPlanner::lower(
                                        &self.resolved,
                                        &mut self.tokens,
                                        &mut self.diagnostics,
                                        origin,
                                        &variant.metas,
                                        body.into_elements(),
                                        TokenAttachment::PrattVariant {
                                            position,
                                            second_field_attachment,
                                        },
                                    );
                                    SourcePrattVariant {
                                        common: Variant {
                                            resolved_variant: variant.id,
                                            name: variant.ident.clone(),
                                            elements: reassign_element_ids(elements),
                                            site: self.site(origin, variant.range.clone()),
                                            pretty: self.lower_pretty_hints(
                                                origin,
                                                &variant.metas,
                                                variant.range.clone(),
                                            ),
                                        },
                                        position,
                                    }
                                })
                                .collect(),
                        )
                    } else {
                        NodeBody::Enum(
                            resolved_variants
                                .iter()
                                .map(|variant| {
                                    reject_pratt_marker(
                                        &self.resolved,
                                        origin,
                                        &variant.metas,
                                        false,
                                        &mut self.diagnostics,
                                    );
                                    reject_pratt_roles_off_pratt(
                                        &self.resolved,
                                        origin,
                                        &variant.metas,
                                        &mut self.diagnostics,
                                    );
                                    let body = self.lower_fields(origin, &variant.fields);
                                    let elements = PositionalTokenPlanner::lower(
                                        &self.resolved,
                                        &mut self.tokens,
                                        &mut self.diagnostics,
                                        origin,
                                        &variant.metas,
                                        body.into_elements(),
                                        TokenAttachment::Variant,
                                    );
                                    Variant {
                                        resolved_variant: variant.id,
                                        name: variant.ident.clone(),
                                        elements: reassign_element_ids(elements),
                                        site: self.site(origin, variant.range.clone()),
                                        pretty: self.lower_pretty_hints(
                                            origin,
                                            &variant.metas,
                                            variant.range.clone(),
                                        ),
                                    }
                                })
                                .collect(),
                        )
                    }
                }
            };
            let configured_traits = self.trait_planner.node_plan(
                &self.resolved,
                origin,
                configured,
                &mut self.diagnostics,
            );
            self.trait_planner.validate_ordinary_derive_ownership(
                &self.resolved,
                origin,
                configured,
                &configured_traits,
                &mut self.diagnostics,
            );
            let pretty = self.lower_pretty_hints(origin, configured, node.range);
            self.nodes.push(Node {
                resolved_node: node.id,
                name: node.ident,
                body,
                configured_traits,
                pretty,
            });
        }
    }

    fn lower_fields(&mut self, origin: SourceOrigin, fields: &[ResolvedFieldId]) -> LoweredFields {
        let mut body = LoweredFields::default();
        for field in fields {
            let resolved_field = self.resolved_field(*field).clone();
            let field = &resolved_field.syntax;
            let field_configured = &resolved_field.metas;
            let site = self.site(origin, span_range(field.span()));
            reject_pratt_marker(
                &self.resolved,
                origin,
                field_configured,
                false,
                &mut self.diagnostics,
            );
            reject_pratt_roles_off_pratt(
                &self.resolved,
                origin,
                field_configured,
                &mut self.diagnostics,
            );
            self.tokens.lower_content_definition(
                &self.resolved,
                &self.semantic_facts,
                &self.trait_planner.grammar_defaults,
                origin,
                field_configured,
                &resolved_field,
                &mut self.diagnostics,
            );
            let semantic = GrammarElement::SemanticField {
                id: GrammarElementId::next(0),
                resolved_field: resolved_field.id,
                name: field.ident.as_ref().map(|ident| ident.unraw().to_string()),
                attachment: None,
                separator: SeparatorPlanner::lower(
                    &self.resolved,
                    &self.semantic_facts,
                    &mut self.tokens,
                    &mut self.diagnostics,
                    origin,
                    field_configured,
                    &resolved_field,
                ),
                pretty: self.lower_pretty_hints(
                    origin,
                    field_configured,
                    resolved_field.range.clone(),
                ),
                attachment_before: 0,
                attachment_after: 0,
                site,
            };
            let mut attachment = PositionalTokenPlanner::lower(
                &self.resolved,
                &mut self.tokens,
                &mut self.diagnostics,
                origin,
                field_configured,
                vec![semantic],
                TokenAttachment::Field,
            );
            let semantic = attachment
                .iter()
                .position(|element| matches!(element, GrammarElement::SemanticField { .. }))
                .expect("a field attachment retains its semantic element");
            let after = attachment.len() - semantic - 1;
            let GrammarElement::SemanticField {
                attachment_before,
                attachment_after,
                ..
            } = &mut attachment[semantic]
            else {
                unreachable!()
            };
            *attachment_before = semantic;
            *attachment_after = after;
            body.push_attachment(attachment);
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

    fn lower_pretty_hints(
        &mut self,
        origin: SourceOrigin,
        metas: &[Meta],
        range: Range<u32>,
    ) -> PrettyHints {
        match parse_pretty_hints(metas) {
            Ok(hints) => hints,
            Err(message) => {
                self.diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidPrettyHint,
                    sites: vec![self.site(origin, range)],
                    message,
                });
                PrettyHints::default()
            }
        }
    }

    fn site(&self, origin: SourceOrigin, range: Range<u32>) -> SourceSite {
        source_site(&self.resolved, origin, range)
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
    content_by_name: BTreeMap<String, Vec<TokenDefinitionId>>,
    central_content: Vec<CentralContentDefinition>,
    lexer_records: Vec<NonTokenLexerRecord>,
    uses: Vec<TokenUse>,
    post_lex: Option<syn::Path>,
    post_lex_sites: Vec<SourceSite>,
}

struct CentralContentDefinition {
    semantic: Option<ContentTokenId>,
    name: String,
    pattern: String,
    callback: Option<syn::Path>,
    site: SourceSite,
}

impl TokenCatalog {
    fn collect_fixed_definitions(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        cfg: &CargoCfgSnapshot,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        for inclusion in &resolved.tree.inclusions {
            let origin = SourceOrigin {
                module: inclusion.id,
                source: inclusion.source,
            };
            for item in &inclusion.items {
                let Ok(Some(metas)) = configured_metas(item_attrs(item), cfg) else {
                    continue;
                };
                if let Item::Macro(item_macro) = item
                    && item_macro
                        .mac
                        .path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "tokens")
                {
                    match syn::parse2::<CentralTokensSyntax>(item_macro.mac.tokens.clone()) {
                        Ok(syntax) => self.collect_central_definitions(resolved, origin, syntax),
                        Err(error) => diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![source_site(resolved, origin, span_range(error.span()))],
                            message: error.to_string(),
                        }),
                    }
                }
                self.collect_fixed_definitions_from_metas(resolved, origin, &metas);
                match item {
                    Item::Struct(item) => {
                        for field in &item.fields {
                            if let Ok(Some(metas)) = configured_metas(&field.attrs, cfg) {
                                self.collect_fixed_definitions_from_metas(resolved, origin, &metas);
                            }
                        }
                    }
                    Item::Enum(item) => {
                        for variant in &item.variants {
                            let Ok(Some(metas)) = configured_metas(&variant.attrs, cfg) else {
                                continue;
                            };
                            self.collect_fixed_definitions_from_metas(resolved, origin, &metas);
                            for field in &variant.fields {
                                if let Ok(Some(metas)) = configured_metas(&field.attrs, cfg) {
                                    self.collect_fixed_definitions_from_metas(
                                        resolved, origin, &metas,
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
        origin: SourceOrigin,
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
                self.define_fixed(
                    resolved,
                    origin,
                    spelling.value(),
                    spelling,
                    None,
                    PrettySpacing::Auto,
                );
            }
        }
    }

    fn collect_central_definitions(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
        syntax: CentralTokensSyntax,
    ) {
        for post_lex in &syntax.post_lex {
            self.post_lex_sites
                .push(source_site(resolved, origin, span_range(post_lex.span())));
            if self.post_lex.is_none() {
                self.post_lex = Some(post_lex.clone());
            }
        }
        for fixed in syntax.fixed {
            self.define_fixed(
                resolved,
                origin,
                fixed.name.unraw().to_string(),
                fixed.spelling,
                Some(fixed.authored_class),
                fixed.pretty_spacing,
            );
        }
        for content in syntax.content {
            let name = content.name.unraw().to_string();
            let semantic = resolved
                .content_tokens
                .iter()
                .find_map(|token| (token.ident == name).then_some(token.id));
            self.central_content.push(CentralContentDefinition {
                semantic,
                name,
                pattern: content.pattern.value(),
                callback: content.callback,
                site: source_site(resolved, origin, span_range(content.name.span())),
            });
        }
        self.lexer_records
            .extend(
                syntax
                    .non_token_lexer_records
                    .into_iter()
                    .map(|record| NonTokenLexerRecord {
                        pattern: record.pattern.value(),
                        outcome: record.outcome,
                        site: source_site(resolved, origin, span_range(record.pattern.span())),
                    }),
            );
    }

    fn define_fixed(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
        name: String,
        spelling: LitStr,
        authored_class: Option<AuthoredFixedTokenClass>,
        pretty_spacing: PrettySpacing,
    ) {
        let site = source_site(resolved, origin, span_range(spelling.span()));
        let spelling = spelling.value();
        if let Some(id) = self.fixed_by_name.get(&name).copied() {
            let definition = &mut self.definitions[id.index()];
            let TokenDefinitionKind::Fixed(existing) = &mut definition.kind else {
                unreachable!("fixed namespace points only to fixed definitions")
            };
            if existing.spelling == spelling {
                definition.definition_sites.push(site.clone());
                if let Some(class) = authored_class {
                    existing
                        .authored_classifications
                        .push(AuthoredFixedClassification { class, site });
                }
                return;
            }
        }
        let id = TokenDefinitionId::next(self.definitions.len());
        self.fixed_by_name.entry(name.clone()).or_insert(id);
        self.definitions.push(TokenDefinition {
            id,
            name,
            kind: TokenDefinitionKind::Fixed(FixedTokenDefinition {
                spelling,
                pretty_spacing,
                authored_classifications: authored_class
                    .map(|class| AuthoredFixedClassification {
                        class,
                        site: site.clone(),
                    })
                    .into_iter()
                    .collect(),
            }),
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
        origin: SourceOrigin,
        token: TokenSequenceEntry,
    ) -> Option<GrammarElement> {
        let (definition, site) = match token {
            TokenSequenceEntry::Definition(spelling) => {
                let site = source_site(resolved, origin, span_range(spelling.span()));
                let name = spelling.value();
                let definition = self
                    .fixed_definition(&name)
                    .expect("fixed definitions are collected before uses");
                (definition, site)
            }
            TokenSequenceEntry::Reference(ident) => {
                let site = source_site(resolved, origin, span_range(ident.span()));
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
        let origin = SourceOrigin {
            module: resolved
                .tree
                .inclusions
                .first()
                .expect("a resolved grammar has a root module inclusion")
                .id,
            source: resolved.configuration.source,
        };
        let mut grammar_defaults = Vec::new();
        for path in resolved.configuration.syntax.derives.clone() {
            let site = source_site(resolved, origin, span_range(path.span()));
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
        origin: SourceOrigin,
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
                    .map(|meta| source_site(resolved, origin, span_range(meta.span())))
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
                    sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                    message: "#[node] must use parentheses".to_owned(),
                });
                return self.grammar_default_plan();
            };
            let syntax = match syn::parse2::<NodeTraitDeltaSyntax>(list.tokens.clone()) {
                Ok(syntax) => syntax,
                Err(error) => {
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                        sites: vec![source_site(resolved, origin, span_range(error.span()))],
                        message: error.to_string(),
                    });
                    return self.grammar_default_plan();
                }
            };
            additions = self.normalize_paths(
                resolved,
                origin,
                syntax.additions,
                ConfiguredTraitOwnership::LocalAddition,
                diagnostics,
            );
            omissions = self
                .normalize_paths(
                    resolved,
                    origin,
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
                        .map(|meta| source_site(resolved, origin, span_range(meta.span())))
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
        origin: SourceOrigin,
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
                    sites: vec![source_site(resolved, origin, span_range(path.span()))],
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
        origin: SourceOrigin,
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
                        sites: vec![source_site(resolved, origin, span_range(path.span()))],
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
        origin: SourceOrigin,
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
                    .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                    .collect(),
                message: "a repeated field accepts at most one #[sep(...)]".to_owned(),
            });
            return None;
        }
        let meta = separator_metas[0];
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidSeparator,
                sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                message: "#[sep] must use parentheses".to_owned(),
            });
            return None;
        };
        let syntax = match syn::parse2::<SeparatorSyntax>(list.tokens.clone()) {
            Ok(syntax) => syntax,
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidSeparator,
                    sites: vec![source_site(resolved, origin, span_range(error.span()))],
                    message: error.to_string(),
                });
                return None;
            }
        };
        let declared_repetition = semantic_facts.declared_repetition(field);
        let (name, site) = match syntax.token {
            FixedTokenSyntax::Definition(spelling) => (
                spelling.value(),
                source_site(resolved, origin, span_range(spelling.span())),
            ),
            FixedTokenSyntax::Reference(ident) => (
                ident.unraw().to_string(),
                source_site(resolved, origin, span_range(ident.span())),
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
        origin: SourceOrigin,
        metas: &[Meta],
        mut body: Vec<GrammarElement>,
        attachment: TokenAttachment,
    ) -> Vec<GrammarElement> {
        Self::reject_legacy_syntax(resolved, origin, metas, diagnostics);
        let token_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("tok"))
            .collect::<Vec<_>>();
        if token_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::MultipleTokenSequences,
                sites: token_metas
                    .iter()
                    .map(|meta| source_site(resolved, origin, span_range(meta.span())))
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
                sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                message: "#[tok] must contain one non-empty sequence".to_owned(),
            });
            return body;
        };
        let sequence = match TokenSequence::parse.parse2(list.tokens.clone()) {
            Ok(sequence) => sequence,
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::EmptyTokenSequence,
                    sites: vec![source_site(resolved, origin, span_range(error.span()))],
                    message: error.to_string(),
                });
                return body;
            }
        };
        if sequence.0.is_empty() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::EmptyTokenSequence,
                sites: vec![source_site(resolved, origin, span_range(meta.span()))],
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
                    .map(|span| source_site(resolved, origin, span_range(*span)))
                    .collect(),
                message: "a #[tok(...)] sequence may contain at most one `this`".to_owned(),
            });
            return body;
        }
        if matches!(attachment, TokenAttachment::PrattVariant { .. }) && !this_sites.is_empty() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                sites: vec![source_site(resolved, origin, span_range(this_sites[0]))],
                message: "`this` is prohibited on Pratt variants; attach interleaved syntax to operand fields"
                    .to_owned(),
            });
            return body;
        }
        if !this_sites.is_empty() && body.is_empty() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::EmptyThisAnchor,
                sites: vec![source_site(resolved, origin, span_range(this_sites[0]))],
                message: "`this` cannot anchor an empty semantic body".to_owned(),
            });
            return body;
        }

        if !this_sites.is_empty() {
            if matches!(attachment, TokenAttachment::Field) {
                let mut before = Vec::new();
                let mut after = Vec::new();
                let mut seen_this = false;
                for entry in sequence.0 {
                    match entry {
                        TokenSequenceEntry::This(_) => seen_this = true,
                        token => {
                            if let Some(element) =
                                tokens.fixed_element(resolved, diagnostics, origin, token)
                            {
                                let GrammarElement::FixedToken {
                                    token_use, site, ..
                                } = element
                                else {
                                    unreachable!("fixed-element lowering returns fixed syntax")
                                };
                                let attached = AttachedFixedToken { token_use, site };
                                if seen_this {
                                    after.push(attached);
                                } else {
                                    before.push(attached);
                                }
                            }
                        }
                    }
                }
                let [
                    GrammarElement::SemanticField {
                        attachment: field_attachment,
                        ..
                    },
                ] = body.as_mut_slice()
                else {
                    unreachable!("a field attachment owns exactly one semantic element")
                };
                *field_attachment = Some(FixedTokenAttachment { before, after });
                return body;
            }
            let mut output = Vec::new();
            for entry in sequence.0 {
                match entry {
                    TokenSequenceEntry::This(_) => output.extend(body.clone()),
                    token => {
                        if let Some(element) =
                            tokens.fixed_element(resolved, diagnostics, origin, token)
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
            .filter_map(|token| tokens.fixed_element(resolved, diagnostics, origin, token))
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
                position: SourcePrattPosition::Postfix { .. },
                ..
            } => {
                body.extend(fixed);
                body
            }
            TokenAttachment::PrattVariant {
                position: SourcePrattPosition::Infix { .. },
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
                position: SourcePrattPosition::Atom | SourcePrattPosition::Prefix { .. },
                ..
            } => {
                fixed.extend(body);
                fixed
            }
        }
    }

    fn reject_legacy_syntax(
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
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
                sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                message: format!(
                    "#[{}] is unsupported; #[tok(...)] is the sole fixed-token syntax",
                    path_text(meta.path()),
                ),
            });
        }
    }
}

impl SourcePrattPosition {
    fn from_attributes(
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
        metas: &[Meta],
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> Self {
        let parse_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("parse"))
            .collect::<Vec<_>>();
        if parse_metas.is_empty() {
            return Self::Atom;
        }
        if parse_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                sites: parse_metas
                    .iter()
                    .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                    .collect(),
                message: "a Pratt variant accepts at most one #[parse(...)] operator role"
                    .to_owned(),
            });
            return Self::Atom;
        }
        let meta = parse_metas[0];
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                message: "a Pratt #[parse] role must use parentheses".to_owned(),
            });
            return Self::Atom;
        };
        match syn::parse2::<PrattSyntax>(list.tokens.clone()) {
            Ok(syntax) => syntax.position,
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                    sites: vec![source_site(resolved, origin, span_range(error.span()))],
                    message: error.to_string(),
                });
                Self::Atom
            }
        }
    }
}

fn reject_pratt_marker(
    resolved: &ResolvedSourceGrammar,
    origin: SourceOrigin,
    metas: &[Meta],
    enum_body: bool,
    diagnostics: &mut Vec<SourceModelDiagnostic>,
) -> bool {
    let pratt_metas = metas
        .iter()
        .filter(|meta| meta.path().is_ident("pratt"))
        .collect::<Vec<_>>();
    if pratt_metas.is_empty() {
        return false;
    }
    if !enum_body {
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
            sites: pratt_metas
                .iter()
                .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                .collect(),
            message: "#[pratt] is valid only on an enum Node".to_owned(),
        });
        return false;
    }
    if pratt_metas.len() > 1 || !matches!(pratt_metas[0], Meta::Path(_)) {
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
            sites: pratt_metas
                .iter()
                .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                .collect(),
            message: "a Pratt enum requires exactly one marker-form #[pratt]".to_owned(),
        });
    }
    true
}

fn reject_pratt_roles_off_pratt(
    resolved: &ResolvedSourceGrammar,
    origin: SourceOrigin,
    metas: &[Meta],
    diagnostics: &mut Vec<SourceModelDiagnostic>,
) {
    let parse_metas = metas
        .iter()
        .filter(|meta| is_pratt_role_meta(meta))
        .collect::<Vec<_>>();
    if !parse_metas.is_empty() {
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
            sites: parse_metas
                .iter()
                .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                .collect(),
            message: "Pratt #[parse(...)] roles are valid only on variants of a #[pratt] enum"
                .to_owned(),
        });
    }
}

fn is_pratt_role_meta(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("parse") {
        return false;
    }
    matches!(
        list.tokens.clone().into_iter().next(),
        Some(proc_macro2::TokenTree::Ident(role))
            if role == "prefix" || role == "infix" || role == "postfix"
    )
}

fn reject_misplaced_content_declarations(
    resolved: &ResolvedSourceGrammar,
    origin: SourceOrigin,
    metas: &[Meta],
    diagnostics: &mut Vec<SourceModelDiagnostic>,
) {
    for meta in metas.iter().filter(|meta| meta.path().is_ident("lex")) {
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::InvalidContentToken,
            sites: vec![source_site(resolved, origin, span_range(meta.span()))],
            message: "#[lex(...)] declares a content-token field and is valid only on a field"
                .to_owned(),
        });
    }
}

fn source_site(
    resolved: &ResolvedSourceGrammar,
    origin: SourceOrigin,
    range: Range<u32>,
) -> SourceSite {
    SourceSite {
        module: origin.module,
        logical_module: resolved.tree.inclusions[origin.module.index()]
            .logical_path
            .clone(),
        source: origin.source,
        path: resolved.tree.files[origin.source.index()].path.clone(),
        range,
    }
}

fn canonical_source_order_key(site: &SourceSite) -> SourceOrderKey {
    SourceOrderKey {
        module: site.module.index(),
        start: site.range.start,
        end: site.range.end,
    }
}

impl TokenCatalog {
    fn define_lexical_base(&mut self, definition: CentralContentDefinition) {
        if let Some(existing) = self.definitions.iter_mut().find(|existing| {
            existing.name == definition.name
                && matches!(
                    &existing.kind,
                    TokenDefinitionKind::LexicalBase(base)
                        if base.pattern == definition.pattern
                            && base.callback == definition.callback
                            && base.priority.is_none()
                )
        }) {
            existing.definition_sites.push(definition.site);
            return;
        }
        let id = TokenDefinitionId::next(self.definitions.len());
        self.definitions.push(TokenDefinition {
            id,
            name: definition.name,
            kind: TokenDefinitionKind::LexicalBase(LexicalBaseTokenDefinition {
                pattern: definition.pattern,
                callback: definition.callback,
                priority: None,
            }),
            definition_sites: vec![definition.site],
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_content_definition(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        semantic_facts: &SemanticFacts,
        grammar_defaults: &[ConfiguredTraitSelection],
        origin: SourceOrigin,
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
                    .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                    .collect(),
                message: "a field accepts at most one #[lex(...)] declaration".to_owned(),
            });
            return;
        }
        let meta = lex_metas[0];
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidContentToken,
                sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                message: "#[lex] must use parentheses".to_owned(),
            });
            return;
        };
        let syntax = match syn::parse2::<LexSyntax>(list.tokens.clone()) {
            Ok(syntax) => syntax,
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidContentToken,
                    sites: vec![source_site(resolved, origin, span_range(error.span()))],
                    message: error.to_string(),
                });
                return;
            }
        };
        if let Some(display) = syntax.display {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::ContentDisplayOverride,
                sites: vec![source_site(resolved, origin, span_range(display.span()))],
                message: "content tokens always display their exact matched text; `display` is unsupported"
                    .to_owned(),
            });
            return;
        }
        let Some((semantic, name)) = semantic_facts.content_token(field) else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidContentToken,
                sites: vec![source_site(resolved, origin, field.range.clone())],
                message: "#[lex] requires a resolved content-token field target".to_owned(),
            });
            return;
        };

        let mut authored_definitions = Vec::new();
        for reference in syntax.admissions {
            let site = source_site(resolved, origin, span_range(reference.span()));
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

        let definition_site = source_site(resolved, origin, span_range(meta.span()));
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
            syntax.callback,
            syntax.priority,
            admission_set,
            authored,
            definition_site,
            grammar_defaults,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn define_content(
        &mut self,
        semantic: ContentTokenId,
        name: String,
        pattern: String,
        callback: Option<syn::Path>,
        priority: Option<u32>,
        admission_set: Vec<TokenDefinitionId>,
        authored: AuthoredAdmissions,
        site: SourceSite,
        grammar_defaults: &[ConfiguredTraitSelection],
    ) {
        let compatible = self.content_by_name.get(&name).and_then(|definitions| {
            definitions.iter().copied().find(|id| {
                matches!(
                    &self.definitions[id.index()].kind,
                    TokenDefinitionKind::Content(content)
                        if content.semantic == semantic
                            && content.pattern == pattern
                            && content.callback == callback
                            && content.priority == priority
                            && content.admission_set == admission_set
                )
            })
        });
        if let Some(id) = compatible {
            let definition = &mut self.definitions[id.index()];
            definition.definition_sites.push(site);
            let TokenDefinitionKind::Content(content) = &mut definition.kind else {
                unreachable!("content namespace points only to content definitions");
            };
            content.authored_admissions.push(authored);
            return;
        }

        let id = TokenDefinitionId::next(self.definitions.len());
        self.content_by_name
            .entry(name.clone())
            .or_default()
            .push(id);
        self.definitions.push(TokenDefinition {
            id,
            name,
            kind: TokenDefinitionKind::Content(ContentTokenDefinition {
                semantic,
                pattern,
                callback,
                priority,
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

struct CentralTokensSyntax {
    fixed: Vec<CentralFixedSyntax>,
    content: Vec<CentralContentSyntax>,
    non_token_lexer_records: Vec<CentralNonTokenLexerRecordSyntax>,
    post_lex: Vec<syn::Path>,
}

struct CentralNonTokenLexerRecordSyntax {
    pattern: LitStr,
    outcome: NonTokenLexerOutcome,
}

struct CentralFixedSyntax {
    name: syn::Ident,
    spelling: LitStr,
    authored_class: AuthoredFixedTokenClass,
    pretty_spacing: PrettySpacing,
}

struct CentralContentSyntax {
    name: syn::Ident,
    pattern: LitStr,
    callback: Option<syn::Path>,
}

impl Parse for CentralTokensSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut fixed = Vec::new();
        let mut content = Vec::new();
        let mut non_token_lexer_records = Vec::new();
        let mut post_lex = Vec::new();
        while !input.is_empty() {
            let section = syn::Ident::parse_any(input)?;
            if input.peek(Token![=]) {
                input.parse::<Token![=]>()?;
                if section == "ignore" {
                    non_token_lexer_records.push(CentralNonTokenLexerRecordSyntax {
                        pattern: input.parse()?,
                        outcome: NonTokenLexerOutcome::NonEmitting,
                    });
                } else if section == "post_lex" {
                    post_lex.push(input.parse()?);
                } else {
                    input.parse::<syn::Expr>()?;
                }
            } else {
                let body;
                syn::braced!(body in input);
                if section == "keywords" || section == "punctuation" {
                    let authored_class = if section == "keywords" {
                        AuthoredFixedTokenClass::Keyword
                    } else {
                        AuthoredFixedTokenClass::Punctuation
                    };
                    while !body.is_empty() {
                        let name = syn::Ident::parse_any(&body)?;
                        body.parse::<Token![=>]>()?;
                        let spelling = body.parse()?;
                        let pretty_spacing = parse_fixed_pretty_spacing(&body, &spelling)?;
                        fixed.push(CentralFixedSyntax {
                            name,
                            spelling,
                            authored_class,
                            pretty_spacing,
                        });
                    }
                } else if section == "literals" || section == "lexer_tokens" {
                    while !body.is_empty() {
                        let name = syn::Ident::parse_any(&body)?;
                        body.parse::<Token![=>]>()?;
                        let pattern = body.parse()?;
                        let callback = if body.peek(syn::Ident) {
                            let marker = syn::Ident::parse_any(&body)?;
                            if marker != "with" {
                                return Err(syn::Error::new(
                                    marker.span(),
                                    "central token entries accept only `with path` after the pattern",
                                ));
                            }
                            Some(body.parse()?)
                        } else {
                            None
                        };
                        content.push(CentralContentSyntax {
                            name,
                            pattern,
                            callback,
                        });
                        if body.peek(Token![,]) {
                            body.parse::<Token![,]>()?;
                        } else if !body.is_empty() {
                            return Err(body.error("expected `,` after central token entry"));
                        }
                    }
                } else {
                    body.parse::<proc_macro2::TokenStream>()?;
                }
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            fixed,
            content,
            non_token_lexer_records,
            post_lex,
        })
    }
}

fn parse_fixed_pretty_spacing(
    input: ParseStream<'_>,
    spelling: &LitStr,
) -> syn::Result<PrettySpacing> {
    if !input.peek(Token![,]) {
        return Ok(PrettySpacing::Auto);
    }
    let fork = input.fork();
    fork.parse::<Token![,]>()?;
    if fork.is_empty() {
        input.parse::<Token![,]>()?;
        return Ok(PrettySpacing::Auto);
    }
    let marker = syn::Ident::parse_any(&fork)?;
    if fork.peek(Token![=>]) || !fork.peek(Token![=]) {
        input.parse::<Token![,]>()?;
        return Ok(PrettySpacing::Auto);
    }
    if marker != "spacing" {
        return Err(syn::Error::new(
            marker.span(),
            format!("unsupported fixed-token setting `{marker}`"),
        ));
    }
    input.parse::<Token![,]>()?;
    syn::Ident::parse_any(input)?;
    input.parse::<Token![=]>()?;
    let mode = syn::Ident::parse_any(input)?;
    let spacing = match mode.to_string().as_str() {
        "auto" => PrettySpacing::Auto,
        "tight" => PrettySpacing::Tight,
        "before" => PrettySpacing::Before,
        "after" => PrettySpacing::After,
        "around" => PrettySpacing::Around,
        _ => {
            return Err(syn::Error::new(
                mode.span(),
                "spacing must be auto, tight, before, after, or around",
            ));
        }
    };
    if matches!(
        spacing,
        PrettySpacing::Tight | PrettySpacing::Before | PrettySpacing::After
    ) && spelling
        .value()
        .chars()
        .all(|character| character == '_' || character.is_alphanumeric())
    {
        return Err(syn::Error::new(
            mode.span(),
            "this spacing mode is unsafe for a word-shaped fixed token",
        ));
    }
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
    }
    Ok(spacing)
}

fn skip_central_entry_tail(input: ParseStream<'_>) -> syn::Result<()> {
    while !input.is_empty() && !input.peek(Token![,]) {
        input.parse::<proc_macro2::TokenTree>()?;
    }
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
    }
    Ok(())
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
    position: SourcePrattPosition,
}

impl Parse for PrattSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let role = syn::Ident::parse_any(input)?;
        let role_name = role.to_string();
        if !matches!(role_name.as_str(), "prefix" | "infix" | "postfix") {
            return Err(syn::Error::new(
                role.span(),
                "Pratt role must be prefix, infix, or postfix",
            ));
        }
        let mut bp = None;
        let mut lbp = None;
        let mut rbp = None;
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key = syn::Ident::parse_any(input)?;
            input.parse::<Token![=]>()?;
            let value: syn::LitInt = input.parse()?;
            let value = value.base10_parse::<u8>().map_err(|_| {
                syn::Error::new(
                    value.span(),
                    "Pratt binding power must be a u8 from 0 to 255",
                )
            })?;
            let slot = if key == "bp" {
                &mut bp
            } else if key == "lbp" {
                &mut lbp
            } else if key == "rbp" {
                &mut rbp
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    format!("unsupported Pratt binding-power setting `{key}`"),
                ));
            };
            if slot.replace(value).is_some() {
                return Err(syn::Error::new(
                    key.span(),
                    format!("duplicate Pratt binding-power setting `{key}`"),
                ));
            }
        }
        let position = match role_name.as_str() {
            "prefix" if lbp.is_none() && rbp.is_none() => SourcePrattPosition::Prefix {
                bp: bp.ok_or_else(|| input.error("prefix Pratt variant requires bp"))?,
            },
            "infix" if bp.is_none() => SourcePrattPosition::Infix {
                lbp: lbp.ok_or_else(|| input.error("infix Pratt variant requires lbp"))?,
                rbp: rbp.ok_or_else(|| input.error("infix Pratt variant requires rbp"))?,
            },
            "postfix" if lbp.is_none() && rbp.is_none() => SourcePrattPosition::Postfix {
                bp: bp.ok_or_else(|| input.error("postfix Pratt variant requires bp"))?,
            },
            "prefix" | "postfix" => {
                return Err(input.error("prefix and postfix Pratt variants accept only bp"));
            }
            "infix" => return Err(input.error("infix Pratt variants accept only lbp and rbp")),
            _ => unreachable!("role spelling validated above"),
        };
        Ok(Self { position })
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
    callback: Option<syn::Path>,
    priority: Option<u32>,
    admissions: Vec<syn::Ident>,
    display: Option<LitStr>,
}

impl Parse for LexSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut pattern = None;
        let mut callback = None;
        let mut priority = None;
        let mut admissions = None;
        let mut display = None;
        while !input.is_empty() {
            let key = syn::Ident::parse_any(input)?;
            if key == "pattern" {
                input.parse::<Token![=]>()?;
                if pattern.replace(input.parse()?).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate lex pattern"));
                }
            } else if key == "callback" {
                input.parse::<Token![=]>()?;
                if callback.replace(input.parse()?).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate lex callback"));
                }
            } else if key == "priority" {
                input.parse::<Token![=]>()?;
                let value: syn::LitInt = input.parse()?;
                if priority.replace(value.base10_parse()?).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate lex priority"));
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
            callback,
            priority,
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

pub(crate) fn configured_trait_recipe(
    path: &syn::Path,
    facade: &str,
) -> Option<ConfiguredTraitRecipe> {
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
    tokens
        .lexer_records
        .sort_by_key(|record| canonical_source_order_key(&record.site));
    for definition in &mut tokens.definitions {
        definition
            .definition_sites
            .sort_by_key(canonical_source_order_key);
        match &mut definition.kind {
            TokenDefinitionKind::Fixed(fixed) => fixed
                .authored_classifications
                .sort_by_key(|authored| canonical_source_order_key(&authored.site)),
            TokenDefinitionKind::Content(content) => content
                .authored_admissions
                .sort_by_key(|authored| canonical_source_order_key(&authored.site)),
            TokenDefinitionKind::LexicalBase(_) => {}
        }
    }
    tokens.definitions.sort_by_key(|definition| {
        definition
            .definition_sites
            .first()
            .map(canonical_source_order_key)
            .unwrap_or(SourceOrderKey::LAST)
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
        .sort_by_key(|token_use| canonical_source_order_key(&token_use.site));
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
            NodeBody::Pratt(variants) => {
                for variant in variants {
                    assign_ordered_elements(
                        &mut variant.common.elements,
                        &mut next_element,
                        &use_remap,
                    );
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
            GrammarElement::SemanticField {
                attachment,
                separator,
                ..
            } => {
                if let Some(attachment) = attachment {
                    for fixed in attachment.before.iter_mut().chain(&mut attachment.after) {
                        fixed.token_use = use_remap[fixed.token_use.index()];
                    }
                }
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
        CentralTokensSyntax, ConfiguredTraitOwnership, ConfiguredTraitRecipe, ContentTextStorage,
        DeclaredRepetition, GrammarElement, GrammarElementKind, NodeBody, NodeShape, PrettySpacing,
        SourceModelDiagnosticKind, SourceModelInput, SourcePrattPosition, TokenDefinitionKind,
        TokenUseRole, lower_source_model,
    };
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::RustEdition;
    use crate::resolution::{
        ResolutionInput, ResolvedFieldStyle, ResolvedNodeBody, resolve_local_semantics,
    };
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

    #[test]
    fn multiple_post_lex_hooks_are_rejected_instead_of_using_source_order() {
        let report = model(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! { post_lex = crate::first }
recursa::tokens! { post_lex = crate::second }

#[derive(recursa::Node)]
struct Root<'input> {
    #[lex(pattern = r"[a-z]+")]
    value: Word<'input>,
}
"#,
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::MultiplePostLexHooks,
            "RCA3117",
        );
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == SourceModelDiagnosticKind::MultiplePostLexHooks)
            .expect("a singular grammar hook cannot be selected by source order");
        assert_eq!(diagnostic.sites.len(), 2);
        assert!(report.source_modeled.is_none());
    }

    #[test]
    fn duplicate_post_lex_hooks_in_one_tokens_block_preserve_both_sites() {
        let report = model(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! {
    post_lex = crate::first,
    post_lex = crate::second,
}

#[derive(recursa::Node)]
struct Root;
"#,
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::MultiplePostLexHooks,
            "RCA3117",
        );
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == SourceModelDiagnosticKind::MultiplePostLexHooks)
            .expect("both hook declarations produce one grammar-level diagnostic");
        assert_eq!(diagnostic.sites.len(), 2);
        assert!(report.source_modeled.is_none());
    }

    fn completed(report: super::SourceModelReport) -> super::SourceModel {
        report.source_modeled.unwrap().source_model
    }

    fn assert_code(report: &super::SourceModelReport, kind: SourceModelDiagnosticKind, code: &str) {
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == kind)
            .unwrap_or_else(|| panic!("missing {kind:?} in {:?}", report.diagnostics));
        assert_eq!(crate::m3::source_model_code(diagnostic.kind), code);
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
    fn preserves_field_positional_syntax_as_one_semantic_attachment() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[tok("?", this, "!")]
    value: Option<Present>,
}

#[derive(recursa::Node)]
#[tok("PRESENT")]
struct Present;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(
            model.nodes[0]
                .elements()
                .iter()
                .map(|element| element.kind())
                .collect::<Vec<_>>(),
            [GrammarElementKind::SemanticField],
        );
        let attachment = model.nodes[0].elements()[0]
            .attachment()
            .expect("field positional syntax is retained as an attachment");
        assert_eq!(attachment.before.len(), 1);
        assert_eq!(attachment.after.len(), 1);
        assert_eq!(
            model.token_definitions[model.token_uses[attachment.before[0].token_use.index()]
                .definition
                .index()]
            .name,
            "?",
        );
        assert_eq!(
            model.token_definitions[model.token_uses[attachment.after[0].token_use.index()]
                .definition
                .index()]
            .name,
            "!",
        );
    }

    #[test]
    fn source_modeled_grammar_retains_visibility_and_field_styles_for_emission() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
pub struct Named { value: Leaf }

#[derive(recursa::Node)]
pub(crate) struct Tuple(Leaf);

#[derive(recursa::Node)]
struct Unit;

#[derive(recursa::Node)]
pub enum Choice {
    Named { value: Leaf },
    Tuple(Leaf),
    Unit,
}

#[derive(recursa::Node)]
struct Leaf;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let source_modeled = report.source_modeled.unwrap();
        let resolved = source_modeled.resolved();
        let named = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Named")
            .unwrap();
        let tuple = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Tuple")
            .unwrap();
        let unit = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Unit")
            .unwrap();
        let choice = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Choice")
            .unwrap();

        assert_eq!(named.visibility, syn::parse_quote!(pub));
        assert_eq!(tuple.visibility, syn::parse_quote!(pub(crate)));
        assert_eq!(unit.visibility, syn::Visibility::Inherited);
        assert_eq!(choice.visibility, syn::parse_quote!(pub));
        assert!(matches!(
            named.body,
            ResolvedNodeBody::Struct {
                style: ResolvedFieldStyle::Named
            }
        ));
        assert!(matches!(
            tuple.body,
            ResolvedNodeBody::Struct {
                style: ResolvedFieldStyle::Unnamed
            }
        ));
        assert!(matches!(
            unit.body,
            ResolvedNodeBody::Struct {
                style: ResolvedFieldStyle::Unit
            }
        ));
        let ResolvedNodeBody::Enum { variants } = &choice.body else {
            panic!("Choice remains an enum")
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| (variant.ident.as_str(), variant.style))
                .collect::<Vec<_>>(),
            [
                ("Named", ResolvedFieldStyle::Named),
                ("Tuple", ResolvedFieldStyle::Unnamed),
                ("Unit", ResolvedFieldStyle::Unit),
            ]
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
                TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => None,
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
        assert_code(
            &report,
            SourceModelDiagnosticKind::MultipleThisAnchors,
            "RCA3104",
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::UnsupportedFixedSyntax,
            "RCA3113",
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
            "RCA3115",
        );
    }

    #[test]
    fn rejects_unsupported_trait_recipes_delta_overlap_and_ordinary_derive_ownership() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Debug, Default) }

#[derive(recursa::Node, Debug)]
#[node(derive(Clone), omit(Clone))]
struct Root;

#[derive(recursa::Node)]
#[node(derive(Clone))]
#[node(derive(Eq))]
struct InvalidDelta;
"#,
        );

        assert!(report.source_modeled.is_none());
        let kinds = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&SourceModelDiagnosticKind::UnsupportedConfiguredTrait));
        assert!(kinds.contains(&SourceModelDiagnosticKind::InvalidTraitDelta));
        assert!(kinds.contains(&SourceModelDiagnosticKind::ConflictingTraitDelta));
        assert!(kinds.contains(&SourceModelDiagnosticKind::ConfiguredOrdinaryDeriveConflict));
        assert_code(
            &report,
            SourceModelDiagnosticKind::UnsupportedConfiguredTrait,
            "RCA3107",
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::InvalidTraitDelta,
            "RCA3108",
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::ConflictingTraitDelta,
            "RCA3109",
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::ConfiguredOrdinaryDeriveConflict,
            "RCA3116",
        );
    }

    #[test]
    fn rejects_content_display_and_misplacement_before_lexical_planning() {
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
        assert_code(
            &report,
            SourceModelDiagnosticKind::InvalidContentToken,
            "RCA3110",
        );
        assert_code(
            &report,
            SourceModelDiagnosticKind::ContentDisplayOverride,
            "RCA3111",
        );
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
                node.elements()
                    .iter()
                    .chain(
                        node.variants()
                            .iter()
                            .flat_map(|variant| variant.elements.iter()),
                    )
                    .chain(
                        node.pratt_variants()
                            .iter()
                            .flat_map(|variant| variant.common.elements.iter()),
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
                TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => None,
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
                "RCA3102",
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
                "RCA3103",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok(this)]
struct Root;
"#,
                SourceModelDiagnosticKind::EmptyThisAnchor,
                "RCA3105",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok("BAD")]
enum Root { Variant }
"#,
                SourceModelDiagnosticKind::InvalidTokenPlacement,
                "RCA3114",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[tok(MISSING)]
struct Root;
"#,
                SourceModelDiagnosticKind::UnresolvedFixedToken,
                "RCA3106",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[pratt]
enum Root { #[tok(this, this)] Group(Box<Self>) }
"#,
                SourceModelDiagnosticKind::MultipleThisAnchors,
                "RCA3104",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[kwd("OLD")]
struct Root;
"#,
                SourceModelDiagnosticKind::UnsupportedFixedSyntax,
                "RCA3113",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[pratt]
enum Root { #[parse(prefix)] Prefix(Box<Self>) }
"#,
                SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                "RCA3115",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)] struct Leaf;
#[derive(recursa::Node)]
struct Root { #[sep] values: Vec<Leaf> }
"#,
                SourceModelDiagnosticKind::InvalidSeparator,
                "RCA3112",
            ),
        ];

        let mut emitted_codes = Vec::new();
        for (source, expected, code) in cases {
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
            assert_code(&report, expected, code);
            emitted_codes.push(crate::m3::source_model_code(expected));
        }

        assert_eq!(
            emitted_codes,
            [
                "RCA3102", "RCA3103", "RCA3105", "RCA3114", "RCA3106", "RCA3104", "RCA3113",
                "RCA3115", "RCA3112",
            ],
        );

        let package = TempDir::new().unwrap();
        let root = package.path().join("grammar.rs");
        fs::write(
            &root,
            r#"
recursa::grammar! { module = crate }
#[cfg_attr(feature = "strict", cfg(unknown_source_cfg))]
#[derive(recursa::Node)]
struct Root;
"#,
        )
        .unwrap();
        let relaxed = CargoCfgSnapshot::default();
        let discovery = discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new("grammar.rs"),
            edition: RustEdition::Rust2024,
            cfg: &relaxed,
        });
        assert!(discovery.diagnostics.is_empty());
        let resolution = resolve_local_semantics(ResolutionInput {
            discovery_inputs: discovery.discovery_inputs,
            tree: discovery.tree.unwrap(),
            cfg: &relaxed,
        });
        assert!(resolution.diagnostics.is_empty());
        let strict = CargoCfgSnapshot::default().with_value("feature", "strict");
        let report = lower_source_model(SourceModelInput {
            resolved: resolution.resolved.unwrap(),
            cfg: &strict,
        });

        assert!(report.source_modeled.is_none());
        assert_code(
            &report,
            SourceModelDiagnosticKind::InvalidConfiguration,
            "RCA3101",
        );
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
        let model = completed(report);
        let elements = &model.nodes[0].pratt_variants()[0].common.elements;
        assert_eq!(
            elements
                .iter()
                .map(GrammarElement::kind)
                .collect::<Vec<_>>(),
            [
                GrammarElementKind::FixedToken,
                GrammarElementKind::SemanticField,
            ],
        );
        let attachment = elements[1]
            .attachment()
            .expect("Pratt operand keeps its positional attachment");
        assert_eq!((attachment.before.len(), attachment.after.len()), (1, 1));
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
        let variants = model.nodes[0].pratt_variants();
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.position)
                .collect::<Vec<_>>(),
            [
                SourcePrattPosition::Prefix { bp: 30 },
                SourcePrattPosition::Infix { lbp: 20, rbp: 21 },
                SourcePrattPosition::Postfix { bp: 40 },
            ],
        );
        let roles = variants
            .iter()
            .map(|variant| {
                variant
                    .common
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
                TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => None,
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
        let NodeBody::Pratt(variants) = &model.nodes[0].body else {
            panic!("#[pratt] must remain a distinct source-model shape")
        };
        assert_eq!(variants[0].position, SourcePrattPosition::Atom);
    }

    #[test]
    fn pratt_positions_retain_exact_u8_binding_powers() {
        let report = model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[pratt]
enum Expr {
    #[tok("ATOM")]
    Atom,
    #[parse(prefix, bp = 0)]
    Prefix(Box<Self>),
    #[parse(infix, lbp = 254, rbp = 255)]
    Infix(Box<Self>, Box<Self>),
    #[parse(postfix, bp = 37)]
    Postfix(Box<Self>),
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let NodeBody::Pratt(variants) = &model.nodes[0].body else {
            panic!("#[pratt] must remain a distinct source-model shape")
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.position)
                .collect::<Vec<_>>(),
            [
                SourcePrattPosition::Atom,
                SourcePrattPosition::Prefix { bp: 0 },
                SourcePrattPosition::Infix { lbp: 254, rbp: 255 },
                SourcePrattPosition::Postfix { bp: 37 },
            ],
        );
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
        let rendered = model.nodes[0].pratt_variants()[0]
            .common
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

    #[test]
    fn rejects_misplaced_and_non_exact_pratt_attributes() {
        let invalid_roles = [
            "prefix",
            "prefix, bp = 1, bp = 2",
            "prefix, lbp = 1",
            "infix, lbp = 1",
            "infix, lbp = 1, rbp = 2, bp = 3",
            "postfix, bp = 256",
            "postfix, bp = 1, unknown = 2",
        ];
        for role in invalid_roles {
            let source = format!(
                r#"
recursa::grammar! {{ module = crate }}
#[derive(recursa::Node)]
#[pratt]
enum Expr {{
    #[parse({role})]
    Invalid(Box<Self>),
}}
"#,
            );
            let report = model(&source);
            assert!(report.source_modeled.is_none(), "accepted #[parse({role})]");
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == SourceModelDiagnosticKind::InvalidPrattTokenPlacement
            }));
        }

        for source in [
            r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[pratt]
struct NotAnEnum;
"#,
            r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
enum NotPratt {
    #[parse(prefix, bp = 1)]
    Variant,
}
"#,
            r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[pratt(unexpected)]
enum InvalidMarker { Atom }
"#,
        ] {
            let report = model(source);
            assert!(report.source_modeled.is_none());
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == SourceModelDiagnosticKind::InvalidPrattTokenPlacement
            }));
        }
    }

    #[test]
    fn rejects_pratt_markers_and_roles_on_variants_and_fields_only() {
        let report = model(
            r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[parse(skip)]
struct FutureNodeParseForm {
    #[parse(prefix, bp = 1)]
    misplaced_role: Leaf,
    #[pratt]
    misplaced_marker: Leaf,
}
#[derive(recursa::Node)]
enum Choice {
    #[pratt]
    MisplacedMarker,
}
#[derive(recursa::Node)]
struct Leaf;
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
            3,
            "node-level future #[parse] forms must not be classified as Pratt roles",
        );
    }

    #[test]
    fn central_fixed_tokens_accept_only_the_five_safe_spacing_modes() {
        let syntax: CentralTokensSyntax = syn::parse_str(
            r#"punctuation {
                AUTO => "+",
                TIGHT => "::", spacing = tight,
                BEFORE => ":", spacing = before,
                AFTER => ":", spacing = after,
                AROUND => ":", spacing = around,
            }"#,
        )
        .expect("safe fixed-token spacing modes parse");
        assert_eq!(
            syntax
                .fixed
                .iter()
                .map(|fixed| fixed.pretty_spacing)
                .collect::<Vec<_>>(),
            [
                PrettySpacing::Auto,
                PrettySpacing::Tight,
                PrettySpacing::Before,
                PrettySpacing::After,
                PrettySpacing::Around,
            ]
        );

        let unknown = syn::parse_str::<CentralTokensSyntax>(
            r#"punctuation { COLON => ":", spacing = sideways }"#,
        )
        .err()
        .expect("unknown spacing is rejected");
        assert_eq!(
            unknown.to_string(),
            "spacing must be auto, tight, before, after, or around"
        );

        let unsafe_keyword = syn::parse_str::<CentralTokensSyntax>(
            r#"keywords { SELECT => "SELECT", spacing = tight }"#,
        )
        .err()
        .expect("unsafe keyword spacing is rejected");
        assert_eq!(
            unsafe_keyword.to_string(),
            "this spacing mode is unsafe for a word-shaped fixed token"
        );
    }

    #[test]
    fn pretty_spacing_rejects_a_tight_boundary_that_changes_longest_match() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation {
        COLON => ":", spacing = tight,
        DOUBLE_COLON => "::",
    }
}

#[derive(recursa::Node)]
#[tok(COLON, COLON)]
struct Root;
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("longest match `::`")
        }));
    }

    #[test]
    fn pretty_spacing_rejects_a_tight_unicode_identifier_boundary() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation {
        BASE => "a",
        COMBINING_ACUTE => "\u{301}", spacing = after,
    }
}

#[derive(recursa::Node)]
#[tok(BASE, COMBINING_ACUTE)]
struct Root;
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("Unicode identifier")
        }));
    }

    #[test]
    fn pretty_spacing_accepts_safe_requested_boundaries() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation {
        COLON => ":", spacing = after,
        DOUBLE_COLON => "::",
    }
}

#[derive(recursa::Node)]
#[tok(COLON, COLON)]
struct Root;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_spacing_rejects_a_longest_match_across_a_semantic_child_boundary() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation {
        COLON => ":", spacing = tight,
        DOUBLE_COLON => "::",
    }
}

#[derive(recursa::Node)]
#[tok(COLON)]
struct Child;

#[derive(recursa::Node)]
#[tok(this, COLON)]
struct Root(Child);
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("longest match `::`")
        }));
    }

    #[test]
    fn pretty_spacing_rejects_an_unproved_regex_boundary() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = tight }
}

#[derive(recursa::Node)]
#[tok(this, COLON)]
struct Root<'input> {
    #[lex(pattern = r"[A-Za-z:]+")]
    text: RootText<'input>,
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("regex-backed")
        }));
    }

    #[test]
    fn pretty_spacing_rejects_a_unicode_boundary_from_dynamic_content() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COMBINING_ACUTE => "\u{301}", spacing = after }
}

#[derive(recursa::Node)]
#[tok(this, COMBINING_ACUTE)]
struct Root<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    text: RootText<'input>,
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("Unicode identifier")
        }));
    }

    #[test]
    fn invalid_pretty_spacing_is_reported_during_m3() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = sideways }
}

#[derive(recursa::Node)]
#[tok(COLON)]
struct Root;
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic
                    .message
                    .contains("spacing must be auto, tight, before, after, or around")
        }));
    }

    #[test]
    fn pretty_pratt_requires_authored_grouping_syntax() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { PLUS => "+" } }

#[derive(recursa::Node)]
struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    value: IntegerText<'input>,
}

#[derive(recursa::Node)]
#[pratt]
enum Expr<'input> {
    Integer(Integer<'input>),
    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("cannot repair precedence")
        }));
    }

    #[test]
    fn pretty_pratt_accepts_grammar_owned_grouping_syntax() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    keywords { BEGIN => "begin", END => "end" }
    punctuation { PLUS => "+" }
}

#[derive(recursa::Node)]
struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    value: IntegerText<'input>,
}

#[derive(recursa::Node)]
#[pratt]
enum Expr<'input> {
    Integer(Integer<'input>),
    Group(#[tok(BEGIN, this, END)] Box<Self>),
    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_pratt_rejects_optional_grouping_like_atoms() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { OPEN => "[", CLOSE => "]", PLUS => "+" }
}

#[derive(recursa::Node)]
struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    value: IntegerText<'input>,
}

#[derive(recursa::Node)]
#[pratt]
enum Expr<'input> {
    Integer(Integer<'input>),
    NotAGroup(#[tok(OPEN, this, CLOSE)] Option<Box<Self>>),
    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("cannot repair precedence")
        }));
    }

    #[test]
    fn pretty_hints_accept_the_closed_local_catalog() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"\s+" }
recursa::tokens! { punctuation { COMMA => ",", TAIL => "." } }
#[derive(recursa::Node)]
struct Item<'input> { #[lex(pattern = r"[a-z]+") ] text: ItemText<'input> }
#[derive(recursa::Node)] #[tok(TAIL)] struct Tail;
#[derive(recursa::Node)]
#[pretty(group = inconsistent, indent, break_before = soft, break_after = hard)]
struct Root<'input> {
    head: Tail,
    #[sep(COMMA, trailing)]
    #[pretty(group = consistent, indent, break_before = soft, break_after = hard, separator = leading, trailing_separator = always)]
    values: Vec<Item<'input>>,
    tail: Tail,
}
"#,
        );
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_hints_reject_unknown_duplicate_and_inapplicable_settings() {
        let unknown = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
#[derive(recursa::Node)]
#[pretty(text = normalized)]
struct Root;
"#,
        );
        assert!(unknown.source_modeled.is_none());
        assert_code(
            &unknown,
            SourceModelDiagnosticKind::InvalidPrettyHint,
            "RCA3118",
        );
        assert!(unknown.diagnostics.iter().any(|diagnostic| diagnostic.kind
            == SourceModelDiagnosticKind::InvalidPrettyHint
            && diagnostic.message.contains("unknown Pretty hint")));

        let qualified = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
#[derive(recursa::Node)]
#[other::pretty(group = consistent)]
struct Root { value: Leaf }
#[derive(recursa::Node)] struct Leaf;
"#,
        );
        assert!(qualified.source_modeled.is_none());
        assert!(qualified.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidPrettyHint
                && diagnostic.message.contains("exact unqualified")
        }));

        let inapplicable = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
#[derive(recursa::Node)]
struct Item;
#[derive(recursa::Node)]
struct Root { #[pretty(separator = leading, trailing_separator = never)] value: Item }
"#,
        );
        assert!(inapplicable.source_modeled.is_none());
        assert!(
            inapplicable
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind
                    == SourceModelDiagnosticKind::InapplicablePrettyHint)
                .count()
                >= 2
        );
        assert_code(
            &inapplicable,
            SourceModelDiagnosticKind::InapplicablePrettyHint,
            "RCA3210",
        );

        let duplicate = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
#[derive(recursa::Node)]
#[pretty(group = consistent)]
#[pretty(group = inconsistent)]
struct Root { first: Leaf, second: Leaf }
#[derive(recursa::Node)] struct Leaf;
"#,
        );
        assert!(duplicate.source_modeled.is_none());
        assert!(duplicate.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidPrettyHint
                && diagnostic.message.contains("duplicate")
        }));

        let cfg_selected = model_with_cfg(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { keywords { A => "a", B => "b" } }
#[derive(recursa::Node)]
#[cfg_attr(feature = "pretty-layout", pretty(group = inconsistent))]
#[tok(A, B)]
struct Root;
"#,
            &CargoCfgSnapshot::default().with_value("feature", "pretty-layout"),
        );
        assert!(
            cfg_selected.diagnostics.is_empty(),
            "{:?}",
            cfg_selected.diagnostics
        );
        assert!(cfg_selected.source_modeled.is_some());

        let empty_scopes = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { keywords { ONLY => "only" } }
#[derive(recursa::Node)]
#[pretty(group = consistent)]
enum Single { #[tok(ONLY)] Only }
#[derive(recursa::Node)]
#[pratt]
#[pretty(indent)]
enum Atom<'input> {
    Value(#[lex(pattern = r"[a-z]+") ] Word<'input>),
}
"#,
        );
        assert!(empty_scopes.source_modeled.is_none());
        assert!(empty_scopes.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InapplicablePrettyHint
                && diagnostic.message.contains("enum Node")
        }));
        assert!(empty_scopes.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InapplicablePrettyHint
                && diagnostic.message.contains("Pratt Node")
        }));
    }

    #[test]
    fn pretty_hints_reject_missing_pretty_trait_empty_groups_and_tight_breaks() {
        let report = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight } }
#[derive(recursa::Node)]
struct Empty;
#[derive(recursa::Node)]
#[tok(COLON, this)]
struct Root {
    #[pretty(break_before = hard)]
    value: Empty,
}
"#,
        );
        assert!(report.source_modeled.is_none());
        assert_code(
            &report,
            SourceModelDiagnosticKind::UnsafePrettyHint,
            "RCA3211",
        );
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::UnsafePrettyHint
                && diagnostic.message.contains("mandatory-tight")
        }));

        let disabled = model(
            r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)] struct Leaf;
#[derive(recursa::Node)]
#[pretty(group = consistent)]
enum Empty {
    #[pretty(group = inconsistent)]
    Value(#[pretty(group = consistent)] Leaf),
}
"#,
        );
        assert!(disabled.source_modeled.is_none());
        assert_eq!(
            disabled
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.kind == SourceModelDiagnosticKind::InapplicablePrettyHint
                        && diagnostic.message.contains("effective Pretty")
                })
                .count(),
            3,
        );
        assert!(!disabled.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("Pretty cannot repair precedence")
        }));

        let document_edges = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"\s+" }
#[derive(recursa::Node)] struct Leaf;
#[derive(recursa::Node)]
#[pretty(break_before = soft)]
struct Root {
    #[pretty(break_before = soft, break_after = hard)]
    value: Leaf,
}
"#,
        );
        assert!(document_edges.source_modeled.is_none());
        assert!(document_edges.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InapplicablePrettyHint
                && diagnostic.message.contains("preceding grammar element")
        }));
        assert!(document_edges.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InapplicablePrettyHint
                && diagnostic.message.contains("following grammar element")
        }));

        let no_ignored_trivia = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { DOT => "." } }
#[derive(recursa::Node)] #[tok(DOT)] struct Marker;
#[derive(recursa::Node)] struct Leaf;
#[derive(recursa::Node)]
#[pretty(break_before = soft)]
struct Root {
    before: Marker,
    #[pretty(break_before = soft, break_after = hard)]
    value: Leaf,
    after: Marker,
}
#[derive(recursa::Node)]
enum Choice {
    #[pretty(break_after = hard)]
    Value(Leaf),
}
"#,
        );
        assert!(no_ignored_trivia.source_modeled.is_none());
        assert!(no_ignored_trivia.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::UnsafePrettyHint
                && diagnostic.message.contains("ignored lexer trivia")
        }));
        assert!(
            no_ignored_trivia
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == SourceModelDiagnosticKind::UnsafePrettyHint)
                .count()
                >= 3
        );
    }

    #[test]
    fn pretty_grouping_requires_real_soft_breaks_and_break_safety_follows_nested_sides() {
        let no_soft = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"\s+"
    punctuation { LEFT => "<", spacing = tight, RIGHT => ">", spacing = tight }
}
#[derive(recursa::Node)]
#[pretty(group = consistent)]
#[tok(LEFT, RIGHT)]
struct AllTight;
#[derive(recursa::Node)]
#[pretty(group = inconsistent, break_before = hard)]
struct HardOnly;
"#,
        );
        assert!(no_soft.source_modeled.is_none());
        assert!(
            no_soft
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind
                    == SourceModelDiagnosticKind::InapplicablePrettyHint
                    && diagnostic.message.contains("no soft-break opportunity"))
                .count()
                >= 2
        );

        let nested = model(
            r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \n]+"
    punctuation {
        BEFORE => "@", spacing = before,
        AFTER => "%", spacing = after,
    }
}
#[derive(recursa::Node)] #[tok(BEFORE)] struct EndsBefore;
#[derive(recursa::Node)] #[tok(AFTER)] struct StartsAfter;
#[derive(recursa::Node)] struct Empty;
#[derive(recursa::Node)]
struct BreakBefore {
    left: EndsBefore,
    #[pretty(break_before = hard)] right: Empty,
}
#[derive(recursa::Node)]
struct BreakAfter {
    #[pretty(break_after = hard)] left: Empty,
    right: StartsAfter,
}
"#,
        );
        assert!(nested.source_modeled.is_none());
        assert!(
            nested
                .diagnostics
                .iter()
                .filter(
                    |diagnostic| diagnostic.kind == SourceModelDiagnosticKind::UnsafePrettyHint
                        && diagnostic.message.contains("mandatory-tight")
                )
                .count()
                >= 2,
            "{:?}",
            nested.diagnostics
        );
    }
}
