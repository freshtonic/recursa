#![allow(
    dead_code,
    reason = "private source analysis source-model seam is consumed by later pipeline tickets"
)]
#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Grammar-relevant authored structure retained with source locations.
//!
//! Source modeling lowers supported attributes and Rust item shapes into one
//! source-ordered vocabulary without resolving grammar-semantic paths. The
//! model retains opaque Rust fragments and precise sites so later resolution,
//! materialization, and diagnostics share the same authored evidence.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::OnceLock;

use proc_macro2::Span;
use regex_automata::dfa::{Automaton as _, StartKind, dense};
use regex_automata::{Anchored, Input, MatchKind};
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
    ResolvedVariantId, SemanticTarget, TypeAliasId,
};
use crate::source_syntax::{
    CargoCfgSnapshot, configured_attribute_metas as configured_metas,
    item_attributes as item_attrs, path_text, span_range,
};

#[derive(Clone, Debug)]
/// Inputs required to turn a resolved source grammar into an authored-source model.
pub(crate) struct SourceModelInput<'cfg> {
    /// Resolved declarations, references, and module structure to model.
    pub(crate) resolved: ResolvedSourceGrammar,
    /// Active Cargo configuration used when interpreting feature-gated attributes.
    pub(crate) cfg: &'cfg CargoCfgSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Outcome of source-model construction, including every independently detected failure.
pub(crate) struct SourceModelReport {
    /// Deterministically ordered problems found while modeling authored structure.
    pub(crate) diagnostics: Vec<SourceModelDiagnostic>,
    /// Complete model, withheld when any blocking diagnostic was produced.
    pub(crate) source_modeled: Option<SourceModeledGrammar>,
    /// Counts expanded Pretty boundary Nodes for deterministic complexity tests.
    #[cfg(test)]
    pretty_boundary_node_evaluations: usize,
    /// Counts expanded Pretty soft-opportunity Nodes for deterministic complexity tests.
    #[cfg(test)]
    pretty_soft_node_evaluations: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Keeps a resolved grammar and its authored-source model under one ownership boundary.
pub(crate) struct SourceModeledGrammar {
    /// Typed resolution facts from which the source model was built.
    resolved: ResolvedSourceGrammar,
    /// Authored structure and locations retained for lowering and generation.
    source_model: SourceModel,
}

impl SourceModeledGrammar {
    /// Borrows the typed resolution facts paired with this source model.
    pub(crate) const fn resolved(&self) -> &ResolvedSourceGrammar {
        &self.resolved
    }

    /// Borrows the authored structure paired with the resolved grammar.
    pub(crate) const fn source_model(&self) -> &SourceModel {
        &self.source_model
    }

    #[cfg(test)]
    pub(crate) fn source_model_mut(&mut self) -> &mut SourceModel {
        &mut self.source_model
    }

    /// Derives the source order key from the retained source model and its correlations.
    pub(crate) fn source_order_key(&self, site: &SourceSite) -> SourceOrderKey {
        canonical_source_order_key(site)
    }

    /// Derives the grammar configuration site from the retained source model and its correlations.
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
/// Grammar-relevant authored structure retained for lowering, diagnostics, and generation.
pub(crate) struct SourceModel {
    /// Grammar-wide configured traits applied before Node-local overrides.
    pub(crate) grammar_trait_defaults: Vec<ConfiguredTraitSelection>,
    /// Node declarations in canonical authored-source order.
    pub(crate) nodes: Vec<Node>,
    /// Unified lexical and fixed-token declarations in stable definition order.
    pub(crate) token_definitions: Vec<TokenDefinition>,
    /// Non-token lexer rules retained for generated recognition behavior.
    pub(crate) lexer_records: Vec<NonTokenLexerRecord>,
    /// Located references to token definitions from grammar positions.
    pub(crate) token_uses: Vec<TokenUse>,
    /// Compiled `lookahead { ... }` declarations in canonical source order.
    pub(crate) lookahead_filters: Vec<LookaheadFilter>,
    /// The compiled `precedence { ... }` declaration: its levels from lowest to highest binding
    /// power, empty without one.
    pub(crate) precedence: Vec<PrecedenceLevel>,
    /// Every `#[parse(prec = NAME)]` override, resolved to a level of `precedence`.
    pub(crate) precedence_overrides: Vec<PrecedenceOverride>,
    /// Authored LR conflict resolutions, compiled to typed source identities and token
    /// definitions.
    pub(crate) lr_conflict_resolutions: Vec<LrConflictResolution>,
    /// Validated `#[restricts(Base)]` declarations in authored alias order.
    pub(crate) restrictions: Vec<SourceRestriction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One validated restricted expression (`docs/declared-precedence.md`, "Restricted
/// expressions"): a grammar nonterminal whose rules are a subset of one base enum Node's
/// variants, never an AST type of its own.
pub(crate) struct SourceRestriction {
    /// The type alias the restricted declaration expanded to, which is how a field that names
    /// the restriction is recognized as parsing its base through it.
    pub(crate) alias: TypeAliasId,
    /// The authored name, which becomes the LR nonterminal's name.
    pub(crate) name: String,
    /// The root base Node, reached through any chain of restrictions: the Node whose type every
    /// rule of this restriction constructs.
    pub(crate) base: NodeId,
    /// The admitted variants of `base`, in the base's own declaration order.
    pub(crate) variants: Vec<ResolvedVariantId>,
    /// Locates the declaration.
    pub(crate) site: SourceSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// What one restricted expression restricts: an enum Node directly, or another restriction.
enum RestrictionBase {
    /// The base is an ordinary enum Node.
    Node(NodeId),
    /// The base is another restricted expression, named by the alias it expanded to.
    Restriction(TypeAliasId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A Node or variant whose lowered rules participate in an authored LR conflict declaration.
pub(crate) enum LrRuleOwner {
    /// One authored Node declaration.
    Type(NodeId),
    /// One authored enum or Pratt variant declaration.
    Variant(ResolvedVariantId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// The explicit action of an authored LR conflict declaration.
pub(crate) enum LrConflictAction {
    /// Keep the shift.
    Shift,
    /// Reduce by the declaration owner's uniquely matching rule.
    Reduce,
    /// Reject the lookahead.
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One validated `#[parse(lr_conflict(...))]` declaration in authored source terms.
pub(crate) struct LrConflictResolution {
    /// The reducing rule's authored owner, and the winner when `action` is `Reduce`.
    pub(crate) owner: LrRuleOwner,
    /// The opposing source identity required in the selected state.
    pub(crate) against: LrRuleOwner,
    /// The required parser action.
    pub(crate) action: LrConflictAction,
    /// Lookahead token kinds named by the declaration.
    pub(crate) lookaheads: Vec<LrLookaheadReference>,
    /// Exact number of conflict cells the declaration must select.
    pub(crate) expected: u32,
    /// Locates the declaration for diagnostics and source ordering.
    pub(crate) site: SourceSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A token kind named as an LR conflict lookahead.
pub(crate) enum LrLookaheadReference {
    /// A fixed or content token definition.
    Token(TokenDefinitionId),
    /// A lookahead filter's merged kind, by index in the source model.
    Merged(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One level of the `precedence { ... }` declaration, bison's `%left`, `%right` or
/// `%nonassoc` line with the binding power that places it on the Pratt scale.
///
/// LR lowering reads this declaration when resolving shift/reduce conflicts.
pub(crate) struct PrecedenceLevel {
    /// The level's associativity.
    pub(crate) associativity: crate::PrecedenceAssociativity,
    /// The level's binding power, strictly above the previous level's.
    pub(crate) binding_power: u8,
    /// Locates the level's declaration.
    pub(crate) site: SourceSite,
    /// The token kinds at this level, in authored order without duplicates.
    pub(crate) kinds: Vec<PrecedenceKindReference>,
    /// Names at this level that are no token: precedence-only names such as bison's
    /// `UMINUS`, each used by at least one `#[parse(prec = NAME)]`.
    pub(crate) names: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One token kind named at a level of the `precedence { ... }` declaration.
///
/// The two spellings a level accepts, in the order the resolver tries them: a token the lexer
/// produces, and the merged kind of a `lookahead { ... }` filter, which only a table-driven
/// parser's token feed produces. gram.y names both in its precedence table, `NOT_LA` beside
/// `BETWEEN` (`gram.y:837`), because to bison the filtered token is a terminal like any other.
pub(crate) enum PrecedenceKindReference {
    /// A token definition: a fixed token by definition name or a content token by type name.
    Token(TokenDefinitionId),
    /// A lookahead filter's merged kind, by its index into [`SourceModel::lookahead_filters`],
    /// which materialization reads to find the merged kind's [`KindId`](crate::KindId).
    Merged(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One `#[parse(prec = NAME)]` attribute, bison's `%prec`: the rules lowered from its owner
/// take the named level's precedence instead of their last declared terminal's.
pub(crate) struct PrecedenceOverride {
    /// The Node or variant that carries the attribute.
    pub(crate) owner: PrecedenceOwner,
    /// The named level, an index into [`SourceModel::precedence`]. It is the same index the
    /// materialized [`GrammarDescription::precedence`](crate::GrammarDescription::precedence)
    /// uses, which is why it carries that vector's id type: materialization maps the two
    /// vectors one to one, in order, keeping precedence-only levels in place.
    pub(crate) level: crate::PrecedenceLevelId,
    /// Locates the authored attribute.
    pub(crate) site: SourceSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// The authored item that carries one `#[parse(prec = NAME)]` attribute.
pub(crate) enum PrecedenceOwner {
    /// A Node; the override covers every rule of the Node, including all its variants.
    Type(NodeId),
    /// A variant of an enum or a `#[pratt]` atom; the override covers the variant's rules.
    Variant(ResolvedVariantId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One compiled lookahead filter: a fixed token that a table-driven parser's token
/// feed reads as the merged kind `name` when the next record is one of `triggers`.
///
/// The merged kind is not a token definition: `#[tok(name)]` stays unresolved, so grammar nodes
/// keep writing the underlying token and only LR lowering substitutes the merged kind.
pub(crate) struct LookaheadFilter {
    /// The authored merged kind name, such as `NOT_LA`.
    pub(crate) name: String,
    /// Locates the merged kind's declaration, which orders the merged kinds.
    pub(crate) site: SourceSite,
    /// The fixed token definition the filter merges.
    pub(crate) token: TokenDefinitionId,
    /// Fixed or single-semantic-kind content token definitions that trigger the merge, in
    /// authored order without duplicates.
    pub(crate) triggers: Vec<TokenDefinitionId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the non token lexer record retained from authored source structure.
pub(crate) struct NonTokenLexerRecord {
    /// Preserves the authored lexical pattern for validation and generation.
    pub(crate) pattern: String,
    /// Preserves the closed matcher executed after the pattern recognizes its starter.
    pub(crate) closed_matcher: Option<ClosedMatcher>,
    /// Preserves authored lexical priority for deterministic matcher ordering.
    pub(crate) priority: Option<u32>,
    /// Supplies the outcome used to correlate authored source with normalized grammar.
    pub(crate) outcome: NonTokenLexerOutcome,
    /// Locates the authored construct represented by this source-model value.
    pub(crate) site: SourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the non token lexer outcome forms retained in the source model.
pub(crate) enum NonTokenLexerOutcome {
    /// Identifies the non emitting authored-source form.
    NonEmitting,
    /// Retains a classified trivia record outside the significant-token stream.
    RetainedTrivia {
        /// Authored grammar-local trivia classification.
        name: String,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Closed source-scanning mechanisms accepted by the central token grammar.
pub(crate) enum ClosedMatcher {
    /// Closes a region with the exact bytes recognized by its opening pattern.
    SameDelimiter {
        /// Regex pattern that recognizes the complete opening delimiter.
        opener: String,
    },
    /// Tracks checked nesting between two fixed delimiters.
    Nested {
        /// Nonempty fixed opening delimiter.
        opener: String,
        /// Nonempty fixed closing delimiter.
        closer: String,
    },
    /// Rejects a pattern match when the immediately following scalar matches an exclusion.
    NextExclusion {
        /// Regex pattern that recognizes the candidate content.
        pattern: String,
        /// Regex pattern that recognizes one excluded following scalar.
        excluded: String,
    },
    /// Recognizes a maximal run with configurable fences and trailing preservation.
    OperatorRun {
        /// Unique ASCII characters admitted to the run.
        characters: String,
        /// Nonempty byte strings that stop a run before their occurrence.
        fences: Vec<String>,
        /// Run characters stripped from the end when no qualifier occurs earlier.
        trailing: String,
        /// Characters whose presence preserves configured trailing characters.
        qualifying: String,
    },
    /// Extends a matched prefix through one exact physical line ending or end of input.
    PhysicalLine {
        /// Regex pattern recognizing the region's nonempty prefix.
        prefix: String,
    },
}

impl ClosedMatcher {
    /// Returns the regular-language starter used for overlap analysis and arbitration planning.
    pub(crate) fn starter_pattern(&self) -> String {
        match self {
            Self::SameDelimiter { opener } => opener.clone(),
            Self::Nested { opener, .. } => regex_syntax::escape(opener),
            Self::NextExclusion { pattern, .. } => pattern.clone(),
            Self::OperatorRun { characters, .. } => {
                let alternatives = characters
                    .chars()
                    .map(|character| regex_syntax::escape(&character.to_string()))
                    .collect::<Vec<_>>()
                    .join("|");
                format!("(?:{alternatives})+")
            }
            Self::PhysicalLine { prefix } => prefix.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the node retained from authored source structure.
pub(crate) struct Node {
    /// Supplies the resolved node used to correlate authored source with normalized grammar.
    pub(crate) resolved_node: NodeId,
    /// Preserves the authored name used by generated bindings and diagnostics.
    pub(crate) name: String,
    /// Supplies the body used to correlate authored source with normalized grammar.
    pub(crate) body: NodeBody,
    /// Supplies the configured traits used to correlate authored source with normalized grammar.
    pub(crate) configured_traits: ConfiguredTraitPlan,
    /// Preserves Pretty configuration after source-level validation.
    pub(crate) pretty: PrettyHints,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the node body forms retained in the source model.
pub(crate) enum NodeBody {
    /// Identifies the struct authored-source form.
    Struct(Vec<GrammarElement>),
    /// Identifies the presence authored-source form.
    Presence(Vec<GrammarElement>),
    /// Identifies the enum authored-source form.
    Enum(Vec<Variant>),
    /// Identifies the pratt authored-source form.
    Pratt(Vec<SourcePrattVariant>),
}

impl Node {
    /// Derives the shape from the retained source model and its correlations.
    pub(crate) const fn shape(&self) -> NodeShape {
        match self.body {
            NodeBody::Struct(_) => NodeShape::Struct,
            NodeBody::Presence(_) => NodeShape::Presence,
            NodeBody::Enum(_) => NodeShape::Enum,
            NodeBody::Pratt(_) => NodeShape::Pratt,
        }
    }

    /// Derives the elements from the retained source model and its correlations.
    pub(crate) fn elements(&self) -> &[GrammarElement] {
        match &self.body {
            NodeBody::Struct(elements) | NodeBody::Presence(elements) => elements,
            NodeBody::Enum(_) | NodeBody::Pratt(_) => &[],
        }
    }

    /// Derives the variants from the retained source model and its correlations.
    pub(crate) fn variants(&self) -> &[Variant] {
        match &self.body {
            NodeBody::Enum(variants) => variants,
            NodeBody::Struct(_) | NodeBody::Presence(_) | NodeBody::Pratt(_) => &[],
        }
    }

    /// Derives the pratt variants from the retained source model and its correlations.
    pub(crate) fn pratt_variants(&self) -> &[SourcePrattVariant] {
        match &self.body {
            NodeBody::Pratt(variants) => variants,
            NodeBody::Struct(_) | NodeBody::Presence(_) | NodeBody::Enum(_) => &[],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the variant retained from authored source structure.
pub(crate) struct Variant {
    /// Supplies the resolved variant used to correlate authored source with normalized grammar.
    pub(crate) resolved_variant: ResolvedVariantId,
    /// Preserves the authored name used by generated bindings and diagnostics.
    pub(crate) name: String,
    /// Supplies the elements used to correlate authored source with normalized grammar.
    pub(crate) elements: Vec<GrammarElement>,
    /// Locates the authored construct represented by this source-model value.
    pub(crate) site: SourceSite,
    /// Preserves Pretty configuration after source-level validation.
    pub(crate) pretty: PrettyHints,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
/// Carries the pretty hints retained from authored source structure.
pub(crate) struct PrettyHints {
    /// Supplies the group used to correlate authored source with normalized grammar.
    pub(crate) group: Option<PrettyGroup>,
    /// Supplies the indent used to correlate authored source with normalized grammar.
    pub(crate) indent: bool,
    /// Supplies the break before used to correlate authored source with normalized grammar.
    pub(crate) break_before: Option<PrettyBreak>,
    /// Supplies the break after used to correlate authored source with normalized grammar.
    pub(crate) break_after: Option<PrettyBreak>,
    /// Supplies the separator used to correlate authored source with normalized grammar.
    pub(crate) separator: Option<PrettySeparatorPosition>,
    /// Supplies the trailing separator used to correlate authored source with normalized grammar.
    pub(crate) trailing_separator: Option<PrettyTrailingSeparator>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Classifies the pretty group forms retained in the source model.
pub(crate) enum PrettyGroup {
    /// Identifies the consistent authored-source form.
    Consistent,
    /// Identifies the inconsistent authored-source form.
    Inconsistent,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Classifies the pretty break forms retained in the source model.
pub(crate) enum PrettyBreak {
    /// Identifies the soft authored-source form.
    Soft,
    /// Identifies the hard authored-source form.
    Hard,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Classifies the pretty separator position forms retained in the source model.
pub(crate) enum PrettySeparatorPosition {
    /// Identifies the leading authored-source form.
    Leading,
    /// Identifies the trailing authored-source form.
    Trailing,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Classifies the pretty trailing separator forms retained in the source model.
pub(crate) enum PrettyTrailingSeparator {
    /// Identifies the always authored-source form.
    Always,
    /// Identifies the never authored-source form.
    Never,
}

impl PrettyHints {
    /// Reports whether the configured-trait selection contains no additions or omissions.
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the source pratt variant retained from authored source structure.
pub(crate) struct SourcePrattVariant {
    /// Declaration metadata shared by all authored Pratt variant roles.
    pub(crate) common: Variant,
    /// Supplies the position used to correlate authored source with normalized grammar.
    pub(crate) position: SourcePrattPosition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the source pratt position forms retained in the source model.
pub(crate) enum SourcePrattPosition {
    /// Identifies the atom authored-source form.
    Atom,
    /// Identifies the prefix authored-source form.
    Prefix {
        #[doc = "Authored right binding power of the prefix operand."]
        bp: u8,
    },
    /// Identifies the infix authored-source form.
    Infix {
        #[doc = "Authored left binding power of the infix operator."]
        lbp: u8,
        #[doc = "Authored right binding power of the infix operand."]
        rbp: u8,
    },
    /// Identifies the postfix authored-source form.
    Postfix {
        #[doc = "Authored left binding power of the postfix operator."]
        bp: u8,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Carries the configured trait plan retained from authored source structure.
pub(crate) struct ConfiguredTraitPlan {
    /// Supplies the effective used to correlate authored source with normalized grammar.
    pub(crate) effective: Vec<ConfiguredTraitSelection>,
    /// Supplies the additions used to correlate authored source with normalized grammar.
    pub(crate) additions: Vec<ConfiguredTraitSelection>,
    /// Supplies the omissions used to correlate authored source with normalized grammar.
    pub(crate) omissions: Vec<ConfiguredTraitOmission>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the configured trait selection retained from authored source structure.
pub(crate) struct ConfiguredTraitSelection {
    /// Supplies the recipe used to correlate authored source with normalized grammar.
    pub(crate) recipe: ConfiguredTraitRecipe,
    /// Supplies the ownership used to correlate authored source with normalized grammar.
    pub(crate) ownership: ConfiguredTraitOwnership,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the configured trait omission retained from authored source structure.
pub(crate) struct ConfiguredTraitOmission {
    /// Supplies the recipe used to correlate authored source with normalized grammar.
    pub(crate) recipe: ConfiguredTraitRecipe,
    /// Supplies the ownership used to correlate authored source with normalized grammar.
    pub(crate) ownership: ConfiguredTraitOwnership,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
/// Classifies the configured trait recipe forms retained in the source model.
pub(crate) enum ConfiguredTraitRecipe {
    /// Identifies the debug authored-source form.
    Debug,
    /// Identifies the clone authored-source form.
    Clone,
    /// Identifies the copy authored-source form.
    Copy,
    /// Identifies the partial eq authored-source form.
    PartialEq,
    /// Identifies the eq authored-source form.
    Eq,
    /// Identifies the partial ord authored-source form.
    PartialOrd,
    /// Identifies the ord authored-source form.
    Ord,
    /// Identifies the hash authored-source form.
    Hash,
    /// Identifies the pretty authored-source form.
    Pretty,
    /// Identifies the visit authored-source form.
    Visit,
    /// Identifies the visit mut authored-source form.
    VisitMut,
    /// Identifies the arbitrary authored-source form.
    Arbitrary,
    /// Identifies the serialize authored-source form.
    Serialize,
    /// Identifies the deserialize authored-source form.
    Deserialize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the configured trait ownership forms retained in the source model.
pub(crate) enum ConfiguredTraitOwnership {
    /// Identifies the grammar default authored-source form.
    GrammarDefault,
    /// Identifies the local addition authored-source form.
    LocalAddition,
    /// Identifies the local omission authored-source form.
    LocalOmission,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the node shape forms retained in the source model.
pub(crate) enum NodeShape {
    /// Identifies the struct authored-source form.
    Struct,
    /// Identifies the enum authored-source form.
    Enum,
    /// Identifies the pratt authored-source form.
    Pratt,
    /// Identifies the presence authored-source form.
    Presence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the grammar element forms retained in the source model.
pub(crate) enum GrammarElement {
    /// Identifies the fixed token authored-source form.
    FixedToken {
        /// Carries the grammar-local identity used by source-model tables.
        id: GrammarElementId,
        /// Supplies the token use used to correlate authored source with normalized grammar.
        token_use: TokenUseId,
        /// Locates the authored construct represented by this source-model value.
        site: SourceSite,
    },
    /// Identifies one erased fixed-token sequence selected atomically when present.
    OptionalFixedTokenGroup {
        /// Carries the grammar-local identity used by source-model tables.
        id: GrammarElementId,
        /// Supplies the source-ordered fixed sequence matched atomically.
        tokens: Vec<AttachedFixedToken>,
        /// Locates the authored optional group.
        site: SourceSite,
    },
    /// Identifies the semantic field authored-source form.
    SemanticField {
        /// Carries the grammar-local identity used by source-model tables.
        id: GrammarElementId,
        /// Supplies the resolved field used to correlate authored source with normalized grammar.
        resolved_field: ResolvedFieldId,
        /// Selects direct semantic parsing or a boolean fixed-token presence projection.
        projection: SemanticFieldProjection,
        /// Preserves the authored name used by generated bindings and diagnostics.
        name: Option<String>,
        /// Supplies the attachment used to correlate authored source with normalized grammar.
        attachment: Option<FixedTokenAttachment>,
        /// Supplies the separator used to correlate authored source with normalized grammar.
        separator: Option<Separator>,
        /// Restricts the Pratt extensions admitted while parsing this field.
        pratt_extensions: Option<PrattExtensionFilter>,
        /// Preserves Pretty configuration after source-level validation.
        pretty: PrettyHints,
        /// Supplies the attachment before used to correlate authored source with normalized grammar.
        attachment_before: usize,
        /// Supplies the attachment after used to correlate authored source with normalized grammar.
        attachment_after: usize,
        /// Locates the authored construct represented by this source-model value.
        site: SourceSite,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries an authored restriction on the Pratt extensions available to one field.
pub(crate) struct PrattExtensionFilter {
    /// Names the Pratt variants excluded from this field and its recursive operands: an atom
    /// or prefix variant as well as an infix or postfix one. The names are resolved once, in
    /// materialization, onto [`crate::ParseSite::pratt_excluded`].
    pub(crate) excluded: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies how one authored semantic field obtains its public value.
pub(crate) enum SemanticFieldProjection {
    /// Parses the field's resolved semantic type directly.
    Direct,
    /// Projects one atomic optional fixed-token sequence to an authored boolean.
    FixedPresence {
        /// Supplies the source-ordered fixed sequence matched atomically.
        tokens: Vec<AttachedFixedToken>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the fixed token attachment retained from authored source structure.
pub(crate) struct FixedTokenAttachment {
    /// Supplies the before used to correlate authored source with normalized grammar.
    pub(crate) before: Vec<AttachedFixedSyntax>,
    /// Supplies the after used to correlate authored source with normalized grammar.
    pub(crate) after: Vec<AttachedFixedSyntax>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies required and erased-optional fixed syntax inside one field attachment.
pub(crate) enum AttachedFixedSyntax {
    /// Selects one required fixed token.
    Required(AttachedFixedToken),
    /// Selects one erased atomic optional fixed-token group.
    Optional {
        /// Supplies the source-ordered fixed sequence.
        tokens: Vec<AttachedFixedToken>,
        /// Locates the authored optional group.
        site: SourceSite,
    },
}

impl AttachedFixedSyntax {
    /// Returns the first fixed token that can bound this attachment entry.
    pub(crate) fn first(&self) -> Option<&AttachedFixedToken> {
        match self {
            Self::Required(token) => Some(token),
            Self::Optional { tokens, .. } => tokens.first(),
        }
    }

    /// Returns the last fixed token that can bound this attachment entry.
    pub(crate) fn last(&self) -> Option<&AttachedFixedToken> {
        match self {
            Self::Required(token) => Some(token),
            Self::Optional { tokens, .. } => tokens.last(),
        }
    }

    /// Returns the authored site for this attachment entry.
    pub(crate) fn site(&self) -> &SourceSite {
        match self {
            Self::Required(token) => &token.site,
            Self::Optional { site, .. } => site,
        }
    }

    /// Visits every fixed token use retained by this attachment entry.
    fn remap_token_uses(&mut self, use_remap: &[TokenUseId]) {
        match self {
            Self::Required(token) => {
                token.token_use = use_remap[token.token_use.index()];
            }
            Self::Optional { tokens, .. } => {
                for token in tokens {
                    token.token_use = use_remap[token.token_use.index()];
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the attached fixed token retained from authored source structure.
pub(crate) struct AttachedFixedToken {
    /// Supplies the token use used to correlate authored source with normalized grammar.
    pub(crate) token_use: TokenUseId,
    /// Locates the authored construct represented by this source-model value.
    pub(crate) site: SourceSite,
}

impl GrammarElement {
    /// Derives the id from the retained source model and its correlations.
    pub(crate) const fn id(&self) -> GrammarElementId {
        match self {
            Self::FixedToken { id, .. }
            | Self::OptionalFixedTokenGroup { id, .. }
            | Self::SemanticField { id, .. } => *id,
        }
    }

    /// Assigns the grammar-local element identity after source-order normalization.
    fn set_id(&mut self, value: GrammarElementId) {
        match self {
            Self::FixedToken { id, .. }
            | Self::OptionalFixedTokenGroup { id, .. }
            | Self::SemanticField { id, .. } => *id = value,
        }
    }

    /// Derives the kind from the retained source model and its correlations.
    pub(crate) const fn kind(&self) -> GrammarElementKind {
        match self {
            Self::FixedToken { .. } | Self::OptionalFixedTokenGroup { .. } => {
                GrammarElementKind::FixedToken
            }
            Self::SemanticField { .. } => GrammarElementKind::SemanticField,
        }
    }

    /// Derives the separator from the retained source model and its correlations.
    pub(crate) fn separator(&self) -> Option<&Separator> {
        match self {
            Self::SemanticField { separator, .. } => separator.as_ref(),
            Self::FixedToken { .. } | Self::OptionalFixedTokenGroup { .. } => None,
        }
    }

    /// Derives the attachment from the retained source model and its correlations.
    pub(crate) fn attachment(&self) -> Option<&FixedTokenAttachment> {
        match self {
            Self::SemanticField { attachment, .. } => attachment.as_ref(),
            Self::FixedToken { .. } | Self::OptionalFixedTokenGroup { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the separator retained from authored source structure.
pub(crate) struct Separator {
    /// Supplies the token use used to correlate authored source with normalized grammar.
    pub(crate) token_use: TokenUseId,
    /// Supplies the declared repetition used to correlate authored source with normalized grammar.
    pub(crate) declared_repetition: Option<DeclaredRepetition>,
    /// Supplies the optional trailing used to correlate authored source with normalized grammar.
    pub(crate) optional_trailing: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the declared repetition forms retained in the source model.
pub(crate) enum DeclaredRepetition {
    /// Identifies the zero or more authored-source form.
    ZeroOrMore,
    /// Identifies the one or more authored-source form.
    OneOrMore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the grammar element kind forms retained in the source model.
pub(crate) enum GrammarElementKind {
    /// Identifies the fixed token authored-source form.
    FixedToken,
    /// Identifies the semantic field authored-source form.
    SemanticField,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the token definition retained from authored source structure.
pub(crate) struct TokenDefinition {
    /// Carries the grammar-local identity used by source-model tables.
    pub(crate) id: TokenDefinitionId,
    /// Preserves the authored name used by generated bindings and diagnostics.
    pub(crate) name: String,
    /// Classifies the source-model value without consulting normalized grammar.
    pub(crate) kind: TokenDefinitionKind,
    /// Supplies the definition sites used to correlate authored source with normalized grammar.
    pub(crate) definition_sites: Vec<SourceSite>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the token definition kind forms retained in the source model.
pub(crate) enum TokenDefinitionKind {
    /// Identifies the fixed authored-source form.
    Fixed(FixedTokenDefinition),
    /// Identifies the content authored-source form.
    Content(ContentTokenDefinition),
    /// Identifies the lexical base authored-source form.
    LexicalBase(LexicalBaseTokenDefinition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the fixed token definition retained from authored source structure.
pub(crate) struct FixedTokenDefinition {
    /// Supplies the spelling used to correlate authored source with normalized grammar.
    pub(crate) spelling: String,
    /// Supplies the authored classifications used to correlate authored source with normalized grammar.
    pub(crate) authored_classifications: Vec<AuthoredFixedClassification>,
    /// Preserves central keyword metadata declarations for order-independent validation.
    keyword_metadata: Vec<AuthoredKeywordMetadata>,
    /// Supplies the pretty spacing used to correlate authored source with normalized grammar.
    pub(crate) pretty_spacing: PrettySpacing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries one authored keyword category and flag assignment.
struct AuthoredKeywordMetadata {
    /// Identifies whether the metadata was attached in a keyword or punctuation section.
    authored_class: AuthoredFixedTokenClass,
    /// Names the keyword category, when the declaration uses the metadata form.
    category: Option<AdmissionReference>,
    /// Names the orthogonal keyword flags in authored order.
    flags: Vec<AdmissionReference>,
    /// Locates the keyword declaration that owns the metadata.
    site: SourceSite,
}

#[derive(Default)]
/// Bundles optional central metadata supplied while defining a fixed token.
struct AuthoredFixedDetails {
    /// Classifies the central section that declared the token.
    authored_class: Option<AuthoredFixedTokenClass>,
    /// Selects the authored Pretty boundary behavior.
    pretty_spacing: PrettySpacing,
    /// Retains keyword category and flag metadata when centrally declared.
    keyword_metadata: Option<AuthoredKeywordMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries one located name used by keyword metadata or an admission expression.
struct AdmissionReference {
    /// Preserves the authored symbolic name.
    name: String,
    /// Locates the authored reference.
    site: SourceSite,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
/// Classifies the pretty spacing forms retained in the source model.
pub(crate) enum PrettySpacing {
    #[default]
    /// Identifies the auto authored-source form.
    Auto,
    /// Identifies the tight authored-source form.
    Tight,
    /// Identifies the before authored-source form.
    Before,
    /// Identifies the after authored-source form.
    After,
    /// Identifies the around authored-source form.
    Around,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the authored fixed token class forms retained in the source model.
pub(crate) enum AuthoredFixedTokenClass {
    /// Identifies the keyword authored-source form.
    Keyword,
    /// Identifies the punctuation authored-source form.
    Punctuation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the authored fixed classification retained from authored source structure.
pub(crate) struct AuthoredFixedClassification {
    /// Supplies the class used to correlate authored source with normalized grammar.
    pub(crate) class: AuthoredFixedTokenClass,
    /// Locates the authored construct represented by this source-model value.
    pub(crate) site: SourceSite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the content token definition retained from authored source structure.
pub(crate) struct ContentTokenDefinition {
    /// Supplies the semantic used to correlate authored source with normalized grammar.
    pub(crate) semantic: ContentTokenId,
    /// Preserves the authored lexical pattern for validation and generation.
    pub(crate) pattern: String,
    /// Preserves the closed matcher used to extend or reject the starter pattern.
    pub(crate) closed_matcher: Option<ClosedMatcher>,
    /// Supplies the authored diagram label, when it differs from the regex fallback used to correlate authored source with normalized grammar.
    pub(crate) diagram_label: Option<String>,
    /// Preserves authored lexical priority for deterministic matcher ordering.
    pub(crate) priority: Option<u32>,
    /// Supplies the admission set used to correlate authored source with normalized grammar.
    pub(crate) admission_set: Vec<TokenDefinitionId>,
    /// Supplies the authored admissions used to correlate authored source with normalized grammar.
    pub(crate) authored_admissions: Vec<AuthoredAdmissions>,
    /// Supplies the text used to correlate authored source with normalized grammar.
    pub(crate) text: ContentTextStorage,
    /// Supplies the configured traits used to correlate authored source with normalized grammar.
    pub(crate) configured_traits: ConfiguredTraitPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the lexical base token definition retained from authored source structure.
pub(crate) struct LexicalBaseTokenDefinition {
    /// Preserves the authored lexical pattern for validation and generation.
    pub(crate) pattern: String,
    /// Preserves the closed matcher used to extend or reject the starter pattern.
    pub(crate) closed_matcher: Option<ClosedMatcher>,
    /// Preserves authored lexical priority for deterministic matcher ordering.
    pub(crate) priority: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the authored admissions retained from authored source structure.
pub(crate) struct AuthoredAdmissions {
    /// Locates the authored construct represented by this source-model value.
    pub(crate) site: SourceSite,
    /// Supplies the definitions used to correlate authored source with normalized grammar.
    pub(crate) definitions: Vec<TokenDefinitionId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the content text storage forms retained in the source model.
pub(crate) enum ContentTextStorage {
    /// Identifies the borrowed or owned matched text authored-source form.
    BorrowedOrOwnedMatchedText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the token use retained from authored source structure.
pub(crate) struct TokenUse {
    /// Carries the grammar-local identity used by source-model tables.
    pub(crate) id: TokenUseId,
    /// Supplies the definition used to correlate authored source with normalized grammar.
    pub(crate) definition: TokenDefinitionId,
    /// Supplies the role used to correlate authored source with normalized grammar.
    pub(crate) role: TokenUseRole,
    /// Locates the authored construct represented by this source-model value.
    pub(crate) site: SourceSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the token use role forms retained in the source model.
pub(crate) enum TokenUseRole {
    /// Identifies the fixed element authored-source form.
    FixedElement,
    /// Identifies the separator authored-source form.
    Separator,
    /// Identifies the content admission authored-source form.
    ContentAdmission,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Carries the source site retained from authored source structure.
pub(crate) struct SourceSite {
    /// Identifies the logical module that owns this source-model value.
    pub(crate) module: ModuleId,
    /// Supplies the logical module used to correlate authored source with normalized grammar.
    pub(crate) logical_module: Vec<String>,
    /// Identifies the authored source file containing this value.
    pub(crate) source: crate::SourceId,
    /// Supplies the path used to correlate authored source with normalized grammar.
    pub(crate) path: PathBuf,
    /// Supplies the range used to correlate authored source with normalized grammar.
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Carries the source order key retained from authored source structure.
pub(crate) struct SourceOrderKey {
    /// Identifies the logical module that owns this source-model value.
    module: usize,
    /// Supplies the start used to correlate authored source with normalized grammar.
    start: u32,
    /// Supplies the end used to correlate authored source with normalized grammar.
    end: u32,
}

impl SourceOrderKey {
    /// Sentinel returned when no authored token attachment exists after an element.
    pub(crate) const LAST: Self = Self {
        module: usize::MAX,
        start: u32::MAX,
        end: u32::MAX,
    };
}

#[derive(Clone, Copy)]
/// Carries the source origin retained from authored source structure.
struct SourceOrigin {
    /// Identifies the logical module that owns this source-model value.
    module: ModuleId,
    /// Identifies the authored source file containing this value.
    source: crate::SourceId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the source model diagnostic kind forms retained in the source model.
pub(crate) enum SourceModelDiagnosticKind {
    /// Identifies the invalid configuration authored-source form.
    InvalidConfiguration,
    /// Identifies the invalid pretty hint authored-source form.
    InvalidPrettyHint,
    /// Identifies the inapplicable pretty hint authored-source form.
    InapplicablePrettyHint,
    /// Identifies the unsafe pretty hint authored-source form.
    UnsafePrettyHint,
    /// Identifies the multiple token sequences authored-source form.
    MultipleTokenSequences,
    /// Identifies the empty token sequence authored-source form.
    EmptyTokenSequence,
    /// Identifies the multiple this anchors authored-source form.
    MultipleThisAnchors,
    /// Identifies the empty this anchor authored-source form.
    EmptyThisAnchor,
    /// Identifies the unresolved fixed token authored-source form.
    UnresolvedFixedToken,
    /// Identifies the unsupported configured trait authored-source form.
    UnsupportedConfiguredTrait,
    /// Identifies the invalid trait delta authored-source form.
    InvalidTraitDelta,
    /// Identifies the conflicting trait delta authored-source form.
    ConflictingTraitDelta,
    /// Identifies the invalid content token authored-source form.
    InvalidContentToken,
    /// Identifies the content display override authored-source form.
    ContentDisplayOverride,
    /// Identifies the invalid separator authored-source form.
    InvalidSeparator,
    /// Identifies the unsupported fixed syntax authored-source form.
    UnsupportedFixedSyntax,
    /// Identifies malformed or conflicting fixed-token presence syntax.
    InvalidFixedTokenPresence,
    /// Identifies the invalid token placement authored-source form.
    InvalidTokenPlacement,
    /// Identifies the invalid pratt token placement authored-source form.
    InvalidPrattTokenPlacement,
    /// Identifies an invalid field-local Pratt extension restriction.
    InvalidPrattExtensionFilter,
    /// Identifies use of the removed `#[greedy(...)]` helper.
    InvalidGreedyAcceptance,
    /// Identifies the configured ordinary derive conflict authored-source form.
    ConfiguredOrdinaryDeriveConflict,
    /// Identifies duplicate membership introduced by an admission union.
    DuplicateAdmissionMembership,
    /// Identifies an unresolved or ambiguous admission-expression name.
    InvalidAdmissionReference,
    /// Identifies a cycle between named admission declarations.
    AdmissionCycle,
    /// Identifies a missing, unknown, or inapplicable keyword category.
    InvalidKeywordCategory,
    /// Identifies incompatible keyword or admission metadata declarations.
    ContradictoryAdmissionMetadata,
    /// Identifies a subtraction whose right-hand side has no current members.
    IneffectiveAdmissionSubtraction,
    /// Identifies an obsolete field-local authored lexer callback.
    ObsoleteFieldLexCallback,
    /// Identifies an obsolete central authored lexer callback.
    ObsoleteCentralLexCallback,
    /// Identifies an obsolete authored post-lex hook.
    ObsoletePostLexHook,
    /// Identifies an obsolete authored parser postcondition.
    ObsoleteParserPostcondition,
    /// A `lookahead { ... }` declaration names an unknown, duplicate or ambiguous token.
    InvalidLookaheadFilter,
    /// A `precedence { ... }` declaration names an unknown token or is declared twice, or a
    /// `#[parse(prec = NAME)]` is malformed, names no declared level, or sits on a Pratt
    /// operator variant whose binding powers already are its precedence.
    InvalidPrecedenceDeclaration,
    /// A `#[parse(lr_conflict(...))]` declaration is malformed or names an unknown source rule
    /// owner or lookahead token.
    InvalidLrConflictResolution,
    /// A `#[restricts(Base)]` declaration is not an enum of bare variant names.
    RestrictedVariantHasFields,
    /// A `#[restricts(Base)]` declaration names a variant its base does not admit.
    RestrictedVariantUnknown,
    /// A `#[restricts(Base)]` declaration names a base that is not an enum Node.
    RestrictedBaseNotEnum,
    /// A `#[restricts(Base)]` declaration names itself, directly or around a cycle.
    RestrictedBaseCycle,
    /// A restricted expression is used where a type is required, or carries configuration only
    /// a type can carry.
    RestrictedExpressionAsType,
}

impl SourceModelDiagnosticKind {
    /// Reports whether this source-model finding prevents a coherent lowered model.
    const fn is_error(self) -> bool {
        !matches!(self, Self::DuplicateAdmissionMembership)
    }
}

impl SourceModelDiagnosticKind {
    /// Builds one diagnostic of this kind at a single authored site.
    fn at(self, site: SourceSite, message: impl Into<String>) -> SourceModelDiagnostic {
        SourceModelDiagnostic {
            kind: self,
            sites: vec![site],
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the source model diagnostic retained from authored source structure.
pub(crate) struct SourceModelDiagnostic {
    /// Classifies the source-model value without consulting normalized grammar.
    pub(crate) kind: SourceModelDiagnosticKind,
    /// Supplies the sites used to correlate authored source with normalized grammar.
    pub(crate) sites: Vec<SourceSite>,
    /// Supplies the message used to correlate authored source with normalized grammar.
    pub(crate) message: String,
}

/// Indexes dense field identities without repeatedly scanning every resolved Node.
fn resolved_field_positions(resolved: &ResolvedSourceGrammar) -> Vec<(usize, usize)> {
    let field_count = resolved.nodes.iter().map(|node| node.fields.len()).sum();
    let mut positions = vec![None; field_count];
    for (node_index, node) in resolved.nodes.iter().enumerate() {
        for (field_index, field) in node.fields.iter().enumerate() {
            let slot = positions
                .get_mut(field.id.index())
                .expect("resolved field identities are dense");
            assert!(
                slot.replace((node_index, field_index)).is_none(),
                "resolved field identities are unique"
            );
        }
    }
    positions
        .into_iter()
        .map(|position| position.expect("resolved field identities are contiguous"))
        .collect()
}

/// Builds the source model from resolved declarations without publishing partial state.
pub(crate) fn lower_source_model(input: SourceModelInput<'_>) -> SourceModelReport {
    let semantic_facts = SemanticFacts::from_resolved(&input.resolved);
    let resolved_field_positions = resolved_field_positions(&input.resolved);
    let mut diagnostics = Vec::new();
    let trait_planner = TraitPlanner::from_grammar(&input.resolved, &mut diagnostics);
    let mut tokens = TokenCatalog::default();
    tokens.collect_fixed_definitions(&input.resolved, input.cfg, &mut diagnostics);
    tokens.compile_named_admissions(
        input.resolved.configuration.syntax.text_equivalence,
        &mut diagnostics,
    );
    let mut builder = SourceModelBuilder {
        resolved: input.resolved,
        cfg: input.cfg,
        semantic_facts,
        trait_planner,
        nodes: Vec::new(),
        tokens,
        pretty_boundary_prover: RefCell::default(),
        pretty_lexical_catalog: None,
        pretty_non_fixed_lexical_catalog: None,
        pretty_trivia_supports_breaks: OnceLock::new(),
        pretty_boundary_analysis: RefCell::default(),
        resolved_field_positions,
        pretty_content_definitions: Vec::new(),
        pending_precedence_overrides: Vec::new(),
        precedence: Vec::new(),
        precedence_overrides: Vec::new(),
        pending_lr_conflict_resolutions: Vec::new(),
        lr_conflict_resolutions: Vec::new(),
        restrictions: Vec::new(),
        diagnostics,
    };
    builder.reject_misplaced_content_declarations();
    builder.lower_central_content_definitions();
    builder.lower_nodes();
    builder.index_pretty_content_definitions();
    builder.validate_pretty_spacing();
    builder.finish()
}

/// Carries the source model builder retained from authored source structure.
struct SourceModelBuilder<'cfg> {
    /// Supplies the resolved used to correlate authored source with normalized grammar.
    resolved: ResolvedSourceGrammar,
    /// Supplies the cfg used to correlate authored source with normalized grammar.
    cfg: &'cfg CargoCfgSnapshot,
    /// Supplies the semantic facts used to correlate authored source with normalized grammar.
    semantic_facts: SemanticFacts,
    /// Supplies the trait planner used to correlate authored source with normalized grammar.
    trait_planner: TraitPlanner,
    /// Supplies the nodes used to correlate authored source with normalized grammar.
    nodes: Vec<Node>,
    /// Supplies the tokens used to correlate authored source with normalized grammar.
    tokens: TokenCatalog,
    /// Reuses compiled lexical languages and boundary-product answers throughout Pretty validation.
    pretty_boundary_prover: RefCell<PrettyBoundaryProver>,
    /// Identifies the immutable source-ordered all-pattern catalog during Pretty validation.
    pretty_lexical_catalog: Option<PrettyPatternCatalogId>,
    /// Identifies non-fixed patterns after fixed/fixed crossings are checked directly.
    pretty_non_fixed_lexical_catalog: Option<PrettyPatternCatalogId>,
    /// Caches the grammar-wide ignored-trivia capability used by every Pretty boundary.
    pretty_trivia_supports_breaks: OnceLock<bool>,
    /// Reuses acyclic Pretty boundary summaries throughout the source graph.
    pretty_boundary_analysis: RefCell<PrettyBoundaryAnalysis>,
    /// Maps dense resolved field identities to their owning Node and field offsets.
    resolved_field_positions: Vec<(usize, usize)>,
    /// Maps content-token semantic identities to their first source-ordered definition.
    pretty_content_definitions: Vec<Option<TokenDefinitionId>>,
    /// Supplies the diagnostics used to correlate authored source with normalized grammar.
    diagnostics: Vec<SourceModelDiagnostic>,
    /// Authored `#[parse(prec = NAME)]` attributes awaiting the compiled declaration.
    pending_precedence_overrides: Vec<PendingPrecedenceOverride>,
    /// The compiled `precedence { ... }` declaration.
    precedence: Vec<PrecedenceLevel>,
    /// Resolved `#[parse(prec = NAME)]` overrides.
    precedence_overrides: Vec<PrecedenceOverride>,
    /// Authored LR conflict declarations awaiting source-path and token resolution.
    pending_lr_conflict_resolutions: Vec<PendingLrConflictResolution>,
    /// Validated LR conflict declarations.
    lr_conflict_resolutions: Vec<LrConflictResolution>,
    /// Validated restricted expressions.
    restrictions: Vec<SourceRestriction>,
}

#[derive(Clone)]
/// One parsed LR conflict declaration awaiting source-path and token resolution.
struct PendingLrConflictResolution {
    /// The declaration's own source owner.
    owner: LrRuleOwner,
    /// Source path fingerprinting the opposing owner.
    against: syn::Path,
    /// The explicit action.
    action: LrConflictAction,
    /// Lookahead names with authored sites.
    lookaheads: Vec<AdmissionReference>,
    /// Exact selected-cell count.
    expected: u32,
    /// Locates the complete attribute.
    site: SourceSite,
}

/// One `#[parse(prec = NAME)]` attribute whose name awaits the compiled declaration.
struct PendingPrecedenceOverride {
    /// The authored item that carries the attribute.
    owner: PrecedenceOwner,
    /// The level name as authored.
    name: String,
    /// Locates the authored attribute.
    site: SourceSite,
}

#[derive(Clone, Copy)]
/// Classifies the token attachment forms retained in the source model.
enum TokenAttachment {
    /// Identifies the struct body authored-source form.
    StructBody,
    /// Identifies the field authored-source form.
    Field,
    /// Identifies the variant authored-source form.
    Variant,
    /// Identifies the pratt variant authored-source form.
    PrattVariant {
        /// Supplies the position used to correlate authored source with normalized grammar.
        position: SourcePrattPosition,
        /// Supplies the second field attachment used to correlate authored source with normalized grammar.
        second_field_attachment: Option<usize>,
    },
}

#[derive(Default)]
/// Carries the lowered fields retained from authored source structure.
struct LoweredFields {
    /// Supplies the elements used to correlate authored source with normalized grammar.
    elements: Vec<GrammarElement>,
    /// Supplies the attachment starts used to correlate authored source with normalized grammar.
    attachment_starts: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Counts required fixed syntax on both sides of one recursive Pratt value.
struct PrettyPrattGroupingBounds {
    /// Number of required fixed tokens before the recursive value.
    before: usize,
    /// Number of required fixed tokens after the recursive value.
    after: usize,
    /// Whether the owner is reached through at least one authored helper Node.
    delegated: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Summarizes empty and single-owner derivations used to prove authored Pratt grouping syntax.
struct PrettyPrattGroupingProof {
    /// Whether this syntax can contribute no rendered or parsed symbols.
    empty: bool,
    /// Distinct authored derivations containing exactly one owning Pratt value, capped at two.
    witnesses: Vec<PrettyPrattGroupingBounds>,
    /// Declarations whose re-entry left this witness list possibly incomplete.
    cyclic: BTreeSet<PrettyPrattGroupingCycle>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Names the declaration whose re-entry left a grouping proof provisional.
///
/// A witness list is exact only once every recorded declaration is discharged.
/// One is discharged when its own frame closes with no witness at all: the cut
/// that produced that emptiness assumed the same emptiness, so the empty set is
/// a fixed point of the declaration's own witness equation, and witnesses are
/// the least fixed point because each stands for a finite derivation.
enum PrettyPrattGroupingCycle {
    /// A helper Node re-entered while its own proof was still being derived.
    Node(usize),
    /// A type alias re-entered while its own proof was still being derived.
    TypeAlias(usize),
}

impl PrettyPrattGroupingProof {
    /// Retains enough witnesses to distinguish a unique path from an ambiguous one.
    fn push_witness(&mut self, witness: PrettyPrattGroupingBounds) {
        if self.witnesses.len() < 2 {
            self.witnesses.push(witness);
        }
    }
}

#[derive(Default)]
/// Run-local memo and cycle state shared by every grouping atom in one Pratt family.
struct PrettyPrattGroupingAnalysis {
    /// Completed helper summaries keyed by owner and helper Node identities.
    memo: BTreeMap<(usize, usize), PrettyPrattGroupingProof>,
    /// Helper Nodes active in the current depth-first proof.
    active_nodes: BTreeSet<usize>,
    /// Type aliases active in the current depth-first proof.
    active_aliases: BTreeSet<usize>,
    /// Exact fixed-only helper lengths, including failed proofs.
    fixed_memo: BTreeMap<usize, Option<usize>>,
    /// Fixed-only helper Nodes active in the current depth-first proof.
    active_fixed_nodes: BTreeSet<usize>,
    /// Type aliases active in the current fixed-only proof.
    active_fixed_aliases: BTreeSet<usize>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Classifies the pretty boundary kind forms retained in the source model.
enum PrettyBoundaryKind {
    /// Identifies the fixed authored-source form.
    Fixed(TokenDefinitionId),
    /// Identifies the dynamic authored-source form.
    Dynamic(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the materialized pretty boundary forms retained in the source model.
pub(crate) enum MaterializedPrettyBoundary {
    /// Identifies the fixed authored-source form.
    Fixed(TokenDefinitionId),
    /// Identifies the regex authored-source form.
    Regex(String),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Carries the pretty boundary retained from authored source structure.
struct PrettyBoundary {
    /// Classifies the source-model value without consulting normalized grammar.
    kind: PrettyBoundaryKind,
    /// Locates the authored construct represented by this source-model value.
    site: SourceSite,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Retains a cached boundary site without binding caller-relative provenance too early.
enum PrettyBoundaryTemplateSite {
    /// Reuses the site supplied by the current Node reference.
    Caller,
    /// Retains an authored site reached inside the cached Node graph.
    Authored(SourceSite),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Carries one cached Pretty boundary with caller-relative source provenance.
struct PrettyBoundaryTemplate {
    /// Classifies the cached boundary language.
    kind: PrettyBoundaryKind,
    /// Selects caller-relative or authored source provenance.
    site: PrettyBoundaryTemplateSite,
}

impl PrettyBoundaryTemplate {
    /// Creates a boundary whose provenance is supplied by the enclosing semantic field.
    fn caller(kind: PrettyBoundaryKind) -> Self {
        Self {
            kind,
            site: PrettyBoundaryTemplateSite::Caller,
        }
    }

    /// Creates a boundary that reached an authored token or semantic field site.
    fn authored(kind: PrettyBoundaryKind, site: SourceSite) -> Self {
        Self {
            kind,
            site: PrettyBoundaryTemplateSite::Authored(site),
        }
    }

    /// Binds caller-relative provenance at one authored semantic field.
    fn bind(&self, caller: &SourceSite) -> Self {
        Self {
            kind: self.kind.clone(),
            site: match &self.site {
                PrettyBoundaryTemplateSite::Caller => {
                    PrettyBoundaryTemplateSite::Authored(caller.clone())
                }
                PrettyBoundaryTemplateSite::Authored(site) => {
                    PrettyBoundaryTemplateSite::Authored(site.clone())
                }
            },
        }
    }

    /// Materializes cached provenance for the current Node reference.
    fn materialize(&self, caller: &SourceSite) -> PrettyBoundary {
        PrettyBoundary {
            kind: self.kind.clone(),
            site: match &self.site {
                PrettyBoundaryTemplateSite::Caller => caller.clone(),
                PrettyBoundaryTemplateSite::Authored(site) => site.clone(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the pretty boundary side forms retained in the source model.
enum PrettyBoundarySide {
    /// Identifies the first authored-source form.
    First,
    /// Identifies the last authored-source form.
    Last,
}

#[derive(Default)]
/// Reuses boundary summaries whose reachable subgraph has no recursive fallback.
struct PrettyBoundaryAnalysis {
    /// Maps Node identity and boundary side to its source-ordered unique endpoints.
    nodes: BTreeMap<(NodeId, PrettyBoundarySide), Vec<PrettyBoundaryTemplate>>,
    /// Maps cycle-independent Node identities to their soft-opportunity result.
    soft_nodes: BTreeMap<NodeId, bool>,
    /// Counts expanded Nodes for deterministic complexity tests.
    #[cfg(test)]
    node_evaluations: usize,
    /// Counts expanded soft-opportunity Nodes for deterministic complexity tests.
    #[cfg(test)]
    soft_node_evaluations: usize,
}

/// One anchored lexical language compiled for repeated Pretty-boundary proofs.
struct PrettyPatternAutomaton {
    /// Complete deterministic automaton for the authored pattern.
    dfa: dense::DFA<Vec<u32>>,
    /// Anchored forward start state, computed once with the automaton.
    start: regex_automata::util::primitives::StateID,
    /// Dense live transition graph, initialized only when a product needs this language.
    reachable: OnceLock<Option<PrettyReachableDfa>>,
}

impl PrettyPatternAutomaton {
    /// Retains one compiled language and lazily prepares its reusable live transition graph.
    fn new(dfa: dense::DFA<Vec<u32>>, start: regex_automata::util::primitives::StateID) -> Self {
        Self {
            dfa,
            start,
            reachable: OnceLock::new(),
        }
    }

    /// Returns the one dense live graph shared by every boundary product using this language.
    fn reachable(&self, counts: &mut PrettyBoundaryProductCounts) -> Option<&PrettyReachableDfa> {
        self.reachable
            .get_or_init(|| PrettyReachableDfa::new(self, counts))
            .as_ref()
    }

    /// Reports the retained heap owned by this compiled language and its lazy dense graph.
    fn retained_bytes(&self) -> usize {
        self.dfa.memory_usage().saturating_add(
            self.reachable
                .get()
                .and_then(Option::as_ref)
                .map_or(0, PrettyReachableDfa::retained_bytes),
        )
    }
}

/// Compiles one complete anchored lexical language or language union within the proof budget.
fn compile_pretty_pattern_automaton<P: AsRef<str>>(
    patterns: &[P],
) -> Option<PrettyPatternAutomaton> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = dense::Builder::new();
    builder.configure(
        dense::DFA::config()
            .match_kind(MatchKind::All)
            .start_kind(StartKind::Anchored)
            .dfa_size_limit(Some(PRETTY_BOUNDARY_DFA_SIZE_BUDGET))
            .determinize_size_limit(Some(PRETTY_BOUNDARY_DFA_SIZE_BUDGET)),
    );
    builder.build_many(patterns).ok().and_then(|dfa| {
        let input = Input::new(b"").anchored(Anchored::Yes);
        let start = dfa.start_state_forward(&input).ok()?;
        Some(PrettyPatternAutomaton::new(dfa, start))
    })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Identifies one immutable source-ordered catalog of Pretty lexical patterns.
pub(crate) struct PrettyPatternCatalogId(usize);

/// Carries one compiled union of an immutable Pretty lexical pattern catalog.
struct PrettyPatternCatalog {
    /// Retains members in source order for exact fallback after union compilation failure.
    pattern_ids: Vec<usize>,
    /// Retains member patterns in source order for lazy child-union compilation.
    patterns: Vec<String>,
    /// Matches the union of every catalog pattern; absent for empty or invalid catalogs.
    automaton: Option<PrettyPatternAutomaton>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Identifies one contiguous source-ordered child of a Pretty lexical catalog.
struct PrettyPatternCatalogNode {
    /// Identifies the owning immutable lexical catalog.
    catalog: PrettyPatternCatalogId,
    /// Supplies the inclusive member offset.
    start: usize,
    /// Supplies the exclusive member offset.
    end: usize,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Counts deterministic Pretty-boundary proof work within one prover instance.
pub(crate) struct PrettyBoundaryProofCounts {
    /// Counts immutable ordered lexical catalogs built for the prover.
    pub(crate) catalogs: usize,
    /// Counts catalog queries, including cache hits.
    pub(crate) catalog_queries: usize,
    /// Counts distinct catalog products evaluated.
    pub(crate) catalog_products: usize,
}

#[derive(Default)]
/// Caches lexical languages and exact three-language boundary products for one analysis run.
pub(crate) struct PrettyBoundaryProver {
    /// Dense identity assigned once to each distinct regex pattern.
    pattern_ids: BTreeMap<String, usize>,
    /// Compiled patterns in dense identity order; `None` preserves fail-closed behavior.
    automata: Vec<Option<PrettyPatternAutomaton>>,
    /// Exact crossing answers keyed by left, right, and candidate lexical language.
    products: BTreeMap<(usize, usize, usize), bool>,
    /// Shared right-side viability products keyed independently of the left language.
    right_products: BTreeMap<(usize, usize), Option<PrettyRightProduct>>,
    /// Dense identities assigned to immutable ordered lexical pattern catalogs.
    catalog_ids: BTreeMap<Vec<usize>, PrettyPatternCatalogId>,
    /// Compiled catalog unions in dense identity order.
    catalogs: Vec<PrettyPatternCatalog>,
    /// Exact crossing answers keyed by left, right, and lexical catalog.
    catalog_products: BTreeMap<(usize, usize, PrettyPatternCatalogId), bool>,
    /// Shared catalog viability products keyed independently of the left language.
    catalog_right_products: BTreeMap<(usize, PrettyPatternCatalogId), Option<PrettyRightProduct>>,
    /// Records catalog-left pairs that have already used one distinct right language.
    catalog_left_queries: BTreeSet<(usize, PrettyPatternCatalogId)>,
    /// Shared accepted-boundary states keyed independently of the right language.
    catalog_left_products: BTreeMap<(usize, PrettyPatternCatalogId), Option<PrettyLeftProduct>>,
    /// Lazily compiled bounded unions for contiguous catalog children.
    catalog_node_automata: BTreeMap<PrettyPatternCatalogNode, Option<PrettyPatternAutomaton>>,
    /// Exact child answers retained independently of root catalog query accounting.
    catalog_node_products: BTreeMap<(usize, usize, PrettyPatternCatalogNode), bool>,
    /// Shared child viability products keyed independently of the left language.
    catalog_node_right_products:
        BTreeMap<(usize, PrettyPatternCatalogNode), Option<PrettyRightProduct>>,
    /// Records child-union left languages that have already seen one distinct right language.
    catalog_node_left_queries: BTreeSet<(usize, PrettyPatternCatalogNode)>,
    /// Shared child-union accepted-boundary states keyed independently of the right language.
    catalog_node_left_products:
        BTreeMap<(usize, PrettyPatternCatalogNode), Option<PrettyLeftProduct>>,
    /// Orders adaptive child nodes by first retention for deterministic constant-time eviction.
    catalog_node_retention_order: VecDeque<PrettyPatternCatalogNode>,
    /// Prevents duplicate entries in the adaptive-child retention order.
    catalog_node_retained_nodes: BTreeSet<PrettyPatternCatalogNode>,
    /// Accounts the retained DFA, reachable-graph, and viability-table heap for adaptive children.
    catalog_node_retained_cache_bytes: usize,
    /// Counts adaptive-child map entries so lightweight answers cannot grow without bound.
    catalog_node_retained_cache_entries: usize,
    /// Overrides the aggregate adaptive-child cache budget in deterministic unit tests.
    #[cfg(test)]
    catalog_node_retained_budget: Option<usize>,
    /// Overrides the adaptive-child entry budget in deterministic unit tests.
    #[cfg(test)]
    catalog_node_retained_entry_budget: Option<usize>,
    /// Counts individual-language right-product construction attempts for invariant tests.
    #[cfg(test)]
    right_product_builds: usize,
    /// Counts catalog right-product construction attempts for invariant tests.
    #[cfg(test)]
    catalog_right_product_builds: usize,
    /// Counts catalog left-product construction attempts for invariant tests.
    #[cfg(test)]
    catalog_left_product_builds: usize,
    /// Counts adaptive-child left-product construction attempts for invariant tests.
    #[cfg(test)]
    catalog_node_left_product_builds: usize,
    /// Counts catalog queries deterministically for focused invariant tests.
    #[cfg(test)]
    catalog_queries: usize,
    /// Counts lazy child-union compilation attempts for invariant tests.
    #[cfg(test)]
    catalog_node_compilations: usize,
    /// Counts inconclusive catalog nodes split into ordered children for invariant tests.
    #[cfg(test)]
    catalog_node_splits: usize,
    /// Counts irreducible individual fallbacks for invariant tests.
    #[cfg(test)]
    catalog_leaf_fallbacks: usize,
}

impl PrettyBoundaryProver {
    /// Reports whether concatenating `left` and `right` can enter `lexical_pattern` across their boundary.
    pub(crate) fn patterns_cross(
        &mut self,
        left: &str,
        right: &str,
        lexical_pattern: &str,
    ) -> bool {
        let left = self.intern_pattern(left);
        let right = self.intern_pattern(right);
        let lexical = self.intern_pattern(lexical_pattern);
        self.pattern_ids_cross(left, right, lexical)
    }

    /// Evaluates one cached exact product after every language has a dense identity.
    fn pattern_ids_cross(&mut self, left: usize, right: usize, lexical: usize) -> bool {
        let key = (left, right, lexical);
        if let Some(result) = self.products.get(&key) {
            return *result;
        }
        let right_key = (right, lexical);
        if !self.right_products.contains_key(&right_key) {
            #[cfg(test)]
            {
                self.right_product_builds += 1;
            }
            let product = match (&self.automata[right], &self.automata[lexical]) {
                (Some(right), Some(lexical)) => build_pretty_right_product(
                    right,
                    lexical,
                    &mut PrettyBoundaryProductCounts::new(),
                ),
                _ => None,
            };
            self.right_products.insert(right_key, product);
        }
        let result = match (
            &self.automata[left],
            &self.automata[lexical],
            self.right_products.get(&right_key).and_then(Option::as_ref),
        ) {
            (Some(left), Some(lexical), Some(product)) => compiled_left_crosses(
                left,
                lexical,
                product,
                &mut PrettyBoundaryProductCounts::new(),
            )
            .unwrap_or(true),
            _ => true,
        };
        self.products.insert(key, result);
        result
    }

    /// Interns one immutable source-ordered lexical pattern catalog.
    pub(crate) fn intern_catalog(&mut self, patterns: &[String]) -> PrettyPatternCatalogId {
        let pattern_ids = patterns
            .iter()
            .map(|pattern| self.intern_pattern(pattern))
            .collect::<Vec<_>>();
        if let Some(id) = self.catalog_ids.get(&pattern_ids) {
            return *id;
        }

        let automaton = compile_pretty_pattern_automaton(patterns);
        let id = PrettyPatternCatalogId(self.catalogs.len());
        self.catalogs.push(PrettyPatternCatalog {
            pattern_ids: pattern_ids.clone(),
            patterns: patterns.to_vec(),
            automaton,
        });
        self.catalog_ids.insert(pattern_ids, id);
        id
    }

    /// Reports whether a boundary can enter any language in an immutable lexical catalog.
    pub(crate) fn patterns_cross_catalog(
        &mut self,
        left: &str,
        right: &str,
        catalog: PrettyPatternCatalogId,
    ) -> bool {
        #[cfg(test)]
        {
            self.catalog_queries += 1;
        }
        let left = self.intern_pattern(left);
        let right = self.intern_pattern(right);
        let key = (left, right, catalog);
        if let Some(result) = self.catalog_products.get(&key) {
            return *result;
        }
        let left_key = (left, catalog);
        let repeated_left = !self.catalog_left_queries.insert(left_key);
        let batched = if let Some(lexical) = &self.catalogs[catalog.0].automaton {
            let right_key = (right, catalog);
            if repeated_left && !self.catalog_right_products.contains_key(&right_key) {
                if !self.catalog_left_products.contains_key(&left_key) {
                    #[cfg(test)]
                    {
                        self.catalog_left_product_builds += 1;
                    }
                    let product = match &self.automata[left] {
                        Some(left) => build_pretty_left_product(
                            left,
                            lexical,
                            &mut PrettyBoundaryProductCounts::new(),
                        ),
                        None => None,
                    };
                    self.catalog_left_products.insert(left_key, product);
                }
                match (
                    &self.automata[right],
                    self.catalog_left_products
                        .get(&left_key)
                        .and_then(Option::as_ref),
                ) {
                    (Some(right), Some(product)) => compiled_right_crosses(
                        right,
                        lexical,
                        product,
                        &mut PrettyBoundaryProductCounts::new(),
                    ),
                    _ => None,
                }
            } else {
                if !self.catalog_right_products.contains_key(&right_key) {
                    #[cfg(test)]
                    {
                        self.catalog_right_product_builds += 1;
                    }
                    let product = match &self.automata[right] {
                        Some(right) => build_pretty_right_product(
                            right,
                            lexical,
                            &mut PrettyBoundaryProductCounts::new(),
                        ),
                        None => None,
                    };
                    self.catalog_right_products.insert(right_key, product);
                }
                match (
                    &self.automata[left],
                    self.catalog_right_products
                        .get(&right_key)
                        .and_then(Option::as_ref),
                ) {
                    (Some(left), Some(product)) => compiled_left_crosses(
                        left,
                        lexical,
                        product,
                        &mut PrettyBoundaryProductCounts::new(),
                    ),
                    _ => None,
                }
            }
        } else {
            None
        };
        let result =
            batched.unwrap_or_else(|| self.catalog_root_fallback_cross(left, right, catalog));
        self.catalog_products.insert(key, result);
        result
    }

    /// Resolves one inconclusive root batch through contiguous source-ordered child unions.
    fn catalog_root_fallback_cross(
        &mut self,
        left: usize,
        right: usize,
        catalog: PrettyPatternCatalogId,
    ) -> bool {
        let len = self.catalogs[catalog.0].pattern_ids.len();
        match len {
            0 => false,
            1 => {
                #[cfg(test)]
                {
                    self.catalog_leaf_fallbacks += 1;
                }
                let lexical = self.catalogs[catalog.0].pattern_ids[0];
                self.pattern_ids_cross(left, right, lexical)
            }
            _ => {
                #[cfg(test)]
                {
                    self.catalog_node_splits += 1;
                }
                let middle = len / 2;
                let left_node = PrettyPatternCatalogNode {
                    catalog,
                    start: 0,
                    end: middle,
                };
                let right_node = PrettyPatternCatalogNode {
                    catalog,
                    start: middle,
                    end: len,
                };
                self.catalog_node_cross(left, right, left_node)
                    || self.catalog_node_cross(left, right, right_node)
            }
        }
    }

    /// Resolves one bounded catalog child exactly, splitting only an inconclusive batch.
    fn catalog_node_cross(
        &mut self,
        left: usize,
        right: usize,
        node: PrettyPatternCatalogNode,
    ) -> bool {
        self.retain_catalog_node(node);
        let key = (left, right, node);
        if let Some(result) = self.catalog_node_products.get(&key) {
            return *result;
        }
        let len = node.end - node.start;
        debug_assert!(len > 0, "catalog children are non-empty");
        let result = if len == 1 {
            #[cfg(test)]
            {
                self.catalog_leaf_fallbacks += 1;
            }
            let lexical = self.catalogs[node.catalog.0].pattern_ids[node.start];
            self.pattern_ids_cross(left, right, lexical)
        } else {
            let batched = self.catalog_node_batch_cross(left, right, node);
            if let Some(result) = batched {
                result
            } else {
                // Do not let one inconclusive node remain retained throughout the recursive
                // descent: each child gets the same aggregate cache budget independently.
                self.trim_catalog_node_cache();
                #[cfg(test)]
                {
                    self.catalog_node_splits += 1;
                }
                let middle = node.start + len / 2;
                let left_node = PrettyPatternCatalogNode {
                    end: middle,
                    ..node
                };
                let right_node = PrettyPatternCatalogNode {
                    start: middle,
                    ..node
                };
                self.catalog_node_cross(left, right, left_node)
                    || self.catalog_node_cross(left, right, right_node)
            }
        };
        self.catalog_node_products.insert(key, result);
        self.catalog_node_retained_cache_entries =
            self.catalog_node_retained_cache_entries.saturating_add(1);
        self.retain_catalog_node(node);
        self.trim_catalog_node_cache();
        result
    }

    /// Attempts one bounded child-union proof, returning `None` when any proof budget declines it.
    fn catalog_node_batch_cross(
        &mut self,
        left: usize,
        right: usize,
        node: PrettyPatternCatalogNode,
    ) -> Option<bool> {
        if !self.catalog_node_automata.contains_key(&node) {
            let patterns = self.catalogs[node.catalog.0].patterns[node.start..node.end].to_vec();
            let automaton = compile_pretty_pattern_automaton(&patterns);
            self.catalog_node_retained_cache_bytes =
                self.catalog_node_retained_cache_bytes.saturating_add(
                    automaton
                        .as_ref()
                        .map_or(0, PrettyPatternAutomaton::retained_bytes),
                );
            self.catalog_node_automata.insert(node, automaton);
            self.catalog_node_retained_cache_entries =
                self.catalog_node_retained_cache_entries.saturating_add(1);
            #[cfg(test)]
            {
                self.catalog_node_compilations += 1;
            }
        }
        let retained_before = self
            .catalog_node_automata
            .get(&node)?
            .as_ref()?
            .retained_bytes();
        let lexical = self.catalog_node_automata.get(&node)?.as_ref()?;
        let right_key = (right, node);
        let left_key = (left, node);
        let repeated_left = !self.catalog_node_left_queries.insert(left_key);
        if !repeated_left {
            self.catalog_node_retained_cache_entries =
                self.catalog_node_retained_cache_entries.saturating_add(1);
        }
        let result = if repeated_left && !self.catalog_node_right_products.contains_key(&right_key)
        {
            if !self.catalog_node_left_products.contains_key(&left_key) {
                #[cfg(test)]
                {
                    self.catalog_node_left_product_builds += 1;
                }
                let product = match &self.automata[left] {
                    Some(left) => build_pretty_left_product(
                        left,
                        lexical,
                        &mut PrettyBoundaryProductCounts::new(),
                    ),
                    None => None,
                };
                self.catalog_node_retained_cache_bytes =
                    self.catalog_node_retained_cache_bytes.saturating_add(
                        product
                            .as_ref()
                            .map_or(0, PrettyLeftProduct::retained_bytes),
                    );
                self.catalog_node_left_products.insert(left_key, product);
                self.catalog_node_retained_cache_entries =
                    self.catalog_node_retained_cache_entries.saturating_add(1);
            }
            match (
                &self.automata[right],
                self.catalog_node_left_products
                    .get(&left_key)
                    .and_then(Option::as_ref),
            ) {
                (Some(right), Some(product)) => compiled_right_crosses(
                    right,
                    lexical,
                    product,
                    &mut PrettyBoundaryProductCounts::new(),
                ),
                _ => None,
            }
        } else {
            if !self.catalog_node_right_products.contains_key(&right_key) {
                #[cfg(test)]
                {
                    self.catalog_right_product_builds += 1;
                }
                let product = match &self.automata[right] {
                    Some(right) => build_pretty_right_product(
                        right,
                        lexical,
                        &mut PrettyBoundaryProductCounts::new(),
                    ),
                    None => None,
                };
                self.catalog_node_retained_cache_bytes =
                    self.catalog_node_retained_cache_bytes.saturating_add(
                        product
                            .as_ref()
                            .map_or(0, PrettyRightProduct::retained_bytes),
                    );
                self.catalog_node_right_products.insert(right_key, product);
                self.catalog_node_retained_cache_entries =
                    self.catalog_node_retained_cache_entries.saturating_add(1);
            }
            match (
                &self.automata[left],
                self.catalog_node_right_products
                    .get(&right_key)
                    .and_then(Option::as_ref),
            ) {
                (Some(left), Some(product)) => compiled_left_crosses(
                    left,
                    lexical,
                    product,
                    &mut PrettyBoundaryProductCounts::new(),
                ),
                _ => None,
            }
        };
        let retained_after = self
            .catalog_node_automata
            .get(&node)?
            .as_ref()?
            .retained_bytes();
        self.catalog_node_retained_cache_bytes = self
            .catalog_node_retained_cache_bytes
            .saturating_add(retained_after.saturating_sub(retained_before));
        result
    }

    /// Enqueues one newly retained adaptive child exactly once for deterministic FIFO eviction.
    fn retain_catalog_node(&mut self, node: PrettyPatternCatalogNode) {
        if self.catalog_node_retained_nodes.insert(node) {
            self.catalog_node_retention_order.push_back(node);
        }
    }

    /// Returns the aggregate heap budget for reusable adaptive catalog children.
    fn catalog_node_retained_budget(&self) -> usize {
        #[cfg(test)]
        if let Some(budget) = self.catalog_node_retained_budget {
            return budget;
        }
        PRETTY_BOUNDARY_CATALOG_NODE_RETAINED_BUDGET
    }

    /// Returns the maximum number of lightweight and heavyweight adaptive-child cache entries.
    fn catalog_node_retained_entry_budget(&self) -> usize {
        #[cfg(test)]
        if let Some(budget) = self.catalog_node_retained_entry_budget {
            return budget;
        }
        PRETTY_BOUNDARY_CATALOG_NODE_RETAINED_ENTRY_BUDGET
    }

    /// Evicts complete oldest-retained child nodes until the adaptive cache fits both budgets.
    fn trim_catalog_node_cache(&mut self) {
        let budget = self.catalog_node_retained_budget();
        let entry_budget = self.catalog_node_retained_entry_budget();
        while self.catalog_node_retained_cache_bytes > budget
            || self.catalog_node_retained_cache_entries > entry_budget
        {
            let Some(node) = self.catalog_node_retention_order.pop_front() else {
                debug_assert_eq!(self.catalog_node_retained_cache_bytes, 0);
                debug_assert_eq!(self.catalog_node_retained_cache_entries, 0);
                break;
            };
            self.catalog_node_retained_nodes.remove(&node);
            if let Some(automaton) = self.catalog_node_automata.remove(&node) {
                self.catalog_node_retained_cache_entries =
                    self.catalog_node_retained_cache_entries.saturating_sub(1);
                if let Some(automaton) = automaton {
                    self.catalog_node_retained_cache_bytes = self
                        .catalog_node_retained_cache_bytes
                        .saturating_sub(automaton.retained_bytes());
                }
            }
            let right_products_before = self.catalog_node_right_products.len();
            self.catalog_node_right_products
                .retain(|(_, candidate), product| {
                    if *candidate != node {
                        return true;
                    }
                    self.catalog_node_retained_cache_bytes =
                        self.catalog_node_retained_cache_bytes.saturating_sub(
                            product
                                .as_ref()
                                .map_or(0, PrettyRightProduct::retained_bytes),
                        );
                    false
                });
            self.catalog_node_retained_cache_entries =
                self.catalog_node_retained_cache_entries.saturating_sub(
                    right_products_before.saturating_sub(self.catalog_node_right_products.len()),
                );
            let left_products_before = self.catalog_node_left_products.len();
            self.catalog_node_left_products
                .retain(|(_, candidate), product| {
                    if *candidate != node {
                        return true;
                    }
                    self.catalog_node_retained_cache_bytes =
                        self.catalog_node_retained_cache_bytes.saturating_sub(
                            product
                                .as_ref()
                                .map_or(0, PrettyLeftProduct::retained_bytes),
                        );
                    false
                });
            self.catalog_node_retained_cache_entries =
                self.catalog_node_retained_cache_entries.saturating_sub(
                    left_products_before.saturating_sub(self.catalog_node_left_products.len()),
                );
            let left_queries_before = self.catalog_node_left_queries.len();
            self.catalog_node_left_queries
                .retain(|(_, candidate)| *candidate != node);
            self.catalog_node_retained_cache_entries =
                self.catalog_node_retained_cache_entries.saturating_sub(
                    left_queries_before.saturating_sub(self.catalog_node_left_queries.len()),
                );
            let products_before = self.catalog_node_products.len();
            self.catalog_node_products
                .retain(|(_, _, candidate), _| *candidate != node);
            self.catalog_node_retained_cache_entries = self
                .catalog_node_retained_cache_entries
                .saturating_sub(products_before.saturating_sub(self.catalog_node_products.len()));
        }
        debug_assert!(self.catalog_node_retained_cache_bytes <= budget);
        debug_assert!(self.catalog_node_retained_cache_entries <= entry_budget);
    }

    /// Reports the accounted aggregate adaptive-child heap retained by the prover.
    #[cfg(test)]
    fn catalog_node_retained_bytes(&self) -> usize {
        self.catalog_node_retained_cache_bytes
    }

    /// Reports the aggregate adaptive-child map entries retained by the prover.
    #[cfg(test)]
    fn catalog_node_retained_entries(&self) -> usize {
        self.catalog_node_retained_cache_entries
    }

    /// Interns and compiles one anchored pattern exactly once for this analysis run.
    fn intern_pattern(&mut self, pattern: &str) -> usize {
        if let Some(id) = self.pattern_ids.get(pattern) {
            return *id;
        }
        let id = self.automata.len();
        let automaton = compile_pretty_pattern_automaton(&[pattern]);
        self.pattern_ids.insert(pattern.to_owned(), id);
        self.automata.push(automaton);
        id
    }

    /// Returns the number of unique compiled or fail-closed patterns for invariant tests.
    #[cfg(test)]
    fn compiled_pattern_count(&self) -> usize {
        self.automata.len()
    }

    /// Returns the number of exact language products evaluated for invariant tests.
    #[cfg(test)]
    fn evaluated_product_count(&self) -> usize {
        self.products.len()
    }

    /// Returns the number of exact catalog language products evaluated for invariant tests.
    #[cfg(test)]
    fn evaluated_catalog_product_count(&self) -> usize {
        self.catalog_products.len()
    }

    /// Returns separately keyed individual and catalog right-product counts for invariant tests.
    #[cfg(test)]
    fn right_product_counts(&self) -> (usize, usize) {
        (self.right_product_builds, self.catalog_right_product_builds)
    }

    /// Returns root and adaptive-child left-product construction attempts for invariant tests.
    #[cfg(test)]
    fn catalog_left_product_counts(&self) -> (usize, usize) {
        (
            self.catalog_left_product_builds,
            self.catalog_node_left_product_builds,
        )
    }

    /// Returns adaptive catalog-tree work counts for invariant tests.
    #[cfg(test)]
    fn catalog_tree_counts(&self) -> (usize, usize, usize) {
        (
            self.catalog_node_compilations,
            self.catalog_node_splits,
            self.catalog_leaf_fallbacks,
        )
    }

    /// Returns deterministic proof-work counts without process-global instrumentation.
    #[cfg(test)]
    pub(crate) fn proof_counts(&self) -> PrettyBoundaryProofCounts {
        PrettyBoundaryProofCounts {
            catalogs: self.catalogs.len(),
            catalog_queries: self.catalog_queries,
            catalog_products: self.catalog_products.len(),
        }
    }
}

/// Reports whether a fixed-token spelling follows Unicode identifier shape.
fn is_word_shaped(spelling: &str) -> bool {
    spelling
        .chars()
        .next()
        .is_some_and(unicode_ident::is_xid_start)
        && spelling.chars().all(unicode_ident::is_xid_continue)
}

/// Canonicalizes a fixed spelling under the grammar's token-kind equivalence policy.
pub(crate) fn canonical_fixed_spelling(
    spelling: &str,
    equivalence: crate::TextEquivalence,
) -> String {
    if is_word_shaped(spelling) && equivalence == crate::TextEquivalence::AsciiInsensitive {
        spelling.to_ascii_uppercase()
    } else {
        spelling.to_owned()
    }
}

/// Reports whether one fixed token can begin in `left` and finish after entering `right`.
fn fixed_spellings_cross(
    left: &str,
    right: &str,
    candidate: &str,
    equivalence: crate::TextEquivalence,
) -> bool {
    if candidate.len() <= left.len() || candidate.len() > left.len().saturating_add(right.len()) {
        return false;
    }
    let left_folds =
        equivalence == crate::TextEquivalence::AsciiInsensitive && is_word_shaped(left);
    let right_folds =
        equivalence == crate::TextEquivalence::AsciiInsensitive && is_word_shaped(right);
    let candidate_folds =
        equivalence == crate::TextEquivalence::AsciiInsensitive && is_word_shaped(candidate);
    left.bytes()
        .map(|byte| (byte, left_folds))
        .chain(right.bytes().map(|byte| (byte, right_folds)))
        .zip(candidate.bytes())
        .all(|((rendered, rendered_folds), candidate)| {
            rendered == candidate
                || ((rendered_folds || candidate_folds)
                    && rendered.is_ascii_alphabetic()
                    && candidate.is_ascii_alphabetic()
                    && rendered.eq_ignore_ascii_case(&candidate))
        })
}

/// Derives the fixed lexical pattern from the retained source model and its correlations.
fn fixed_lexical_pattern(spelling: &str, equivalence: crate::TextEquivalence) -> String {
    if equivalence != crate::TextEquivalence::AsciiInsensitive || !is_word_shaped(spelling) {
        return regex_syntax::escape(spelling);
    }
    spelling
        .chars()
        .map(|character| {
            if character.is_ascii_alphabetic() {
                format!(
                    "[{}{}]",
                    character.to_ascii_lowercase(),
                    character.to_ascii_uppercase()
                )
            } else {
                regex_syntax::escape(&character.to_string())
            }
        })
        .collect()
}

/// Derives the fixed pretty pattern from the retained source model and its correlations.
fn fixed_pretty_pattern(
    fixed: &FixedTokenDefinition,
    equivalence: crate::TextEquivalence,
) -> String {
    fixed_lexical_pattern(&fixed.spelling, equivalence)
}

/// Derives the dfa accepts state from the retained source model and its correlations.
fn dfa_accepts_state(
    dfa: &dense::DFA<Vec<u32>>,
    state: regex_automata::util::primitives::StateID,
) -> bool {
    dfa.is_match_state(dfa.next_eoi_state(state))
}

/// Evaluates one exact crossing product over already compiled anchored languages.
fn compiled_patterns_cross(
    left: &PrettyPatternAutomaton,
    right: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
) -> bool {
    let mut counts = PrettyBoundaryProductCounts::new();
    compiled_patterns_cross_with_counts(left, right, lexical, &mut counts)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Counts the deterministic dense-product work for one compiled boundary proof.
struct PrettyBoundaryProductCounts {
    /// Counts dense reachable graphs initialized while evaluating this product.
    #[cfg(test)]
    reachable_graphs_built: usize,
    /// Counts live right-language states reachable from its anchored start.
    #[cfg(test)]
    right_states: usize,
    /// Counts live lexical-language states reachable from its anchored start.
    #[cfg(test)]
    lexical_states: usize,
    /// Counts right-by-lexical states examined by the shared reverse proof.
    #[cfg(test)]
    right_product_states: usize,
    /// Counts left-by-lexical states reached while looking up boundary viability.
    #[cfg(test)]
    left_product_states: usize,
    /// Counts byte-labelled transitions examined across all dense traversals.
    #[cfg(test)]
    byte_transitions: usize,
    /// Counts accepted left boundary states answered by the shared viability table.
    #[cfg(test)]
    boundary_lookups: usize,
}

impl PrettyBoundaryProductCounts {
    /// Creates empty test instrumentation without affecting production traversal state.
    fn new() -> Self {
        Self::default()
    }

    /// Records one byte-labelled transition examination in test instrumentation.
    fn inspect_byte(&mut self) {
        #[cfg(test)]
        {
            self.byte_transitions += 1;
        }
    }

    /// Records the sizes of the two right-side state spaces in test instrumentation.
    fn record_right_spaces(&mut self, right_states: usize, lexical_states: usize) {
        #[cfg(test)]
        {
            self.right_states = right_states;
            self.lexical_states = lexical_states;
        }
        #[cfg(not(test))]
        let _ = (right_states, lexical_states);
    }

    /// Records one right-by-lexical product state in test instrumentation.
    fn visit_right_product_state(&mut self) {
        #[cfg(test)]
        {
            self.right_product_states += 1;
        }
    }

    /// Records one left-by-lexical product state in test instrumentation.
    fn visit_left_product_state(&mut self) {
        #[cfg(test)]
        {
            self.left_product_states += 1;
        }
    }

    /// Records one accepted left boundary lookup in test instrumentation.
    fn lookup_boundary(&mut self) {
        #[cfg(test)]
        {
            self.boundary_lookups += 1;
        }
    }
}

/// Stores one DFA's dense live transition graph from its anchored start state.
struct PrettyReachableDfa {
    /// Stores one optional live target index for every state and byte.
    transitions: Vec<Vec<Option<usize>>>,
    /// Marks states whose end-of-input transition accepts the language.
    accepts: Vec<bool>,
}

/// Retains right-by-lexical boundary viability independently of any left language.
struct PrettyRightProduct {
    /// Number of lexical states used to encode each product-table row.
    lexical_states: usize,
    /// Marks right-by-lexical states from which a crossing match can finish the right language.
    viable: Vec<bool>,
}

/// Retains lexical states reachable exactly where one left language accepts.
struct PrettyLeftProduct {
    /// Number of lexical states used when the accepted boundaries were built.
    lexical_states: usize,
    /// Unique lexical states active at an accepted left-language boundary.
    boundary_states: Vec<usize>,
}

impl PrettyLeftProduct {
    /// Reports the retained heap owned by one cached accepted-boundary frontier.
    fn retained_bytes(&self) -> usize {
        self.boundary_states
            .capacity()
            .saturating_mul(std::mem::size_of::<usize>())
    }
}

impl PrettyRightProduct {
    /// Reports the retained heap owned by one cached viability table.
    fn retained_bytes(&self) -> usize {
        self.viable
            .capacity()
            .saturating_mul(std::mem::size_of::<bool>())
    }
}

/// Maximum byte-labelled right-by-lexical transitions retained by one boundary proof.
///
/// Each product cell can contribute at most 256 predecessor entries. Bounding that complete
/// transition space before allocating the Cartesian table keeps adversarial but valid regexes
/// from amplifying into an unbounded allocation. Returning no product remains fail-closed: the
/// caller reports that the boundary may cross a lexical language.
const PRETTY_BOUNDARY_PRODUCT_TRANSITION_BUDGET: usize = 8 * 1024 * 1024;

/// Maximum bytes permitted while determinizing or retaining one Pretty proof DFA.
///
/// Catalogs that exceed this bound are divided into contiguous source-ordered children. An
/// individual pattern that exceeds it remains fail-closed through the leaf proof.
const PRETTY_BOUNDARY_DFA_SIZE_BUDGET: usize = 8 * 1024 * 1024;

/// Maximum aggregate heavy heap retained for reusable adaptive catalog children.
///
/// Root catalogs and individual languages are bounded separately by their small, authored
/// cardinalities. Child unions can grow to a full binary catalog tree, so their compiled DFAs,
/// expanded reachable graphs, and right-side viability tables share this deterministic cap.
const PRETTY_BOUNDARY_CATALOG_NODE_RETAINED_BUDGET: usize = 32 * 1024 * 1024;

/// Maximum adaptive-child cache entries retained in addition to the aggregate heavy-heap cap.
const PRETTY_BOUNDARY_CATALOG_NODE_RETAINED_ENTRY_BUDGET: usize = 16 * 1024;

/// Maximum byte-labelled left-by-lexical transitions explored by one boundary query.
///
/// The left traversal is query-specific and therefore remains sparse, but its visited set can
/// otherwise grow to the full Cartesian product. Exceeding this deterministic work bound returns
/// no answer so the caller preserves fail-closed behavior.
const PRETTY_BOUNDARY_LEFT_TRANSITION_BUDGET: usize = 8 * 1024 * 1024;

/// Maximum expanded byte transitions retained for one reusable reachable-language graph.
///
/// Regex automata compress equivalent byte classes, while the boundary product deliberately uses
/// a direct 256-entry row. This cap is checked as each state is discovered so that expanding a
/// valid but unusually large DFA cannot allocate without bound before the Cartesian-product cap.
const PRETTY_BOUNDARY_REACHABLE_TRANSITION_BUDGET: usize = 256 * 1024;

/// Checks one expanded reachable graph's complete row count before retaining another state.
fn pretty_boundary_reachable_transition_len(states: usize) -> Option<usize> {
    let byte_transitions = states.checked_mul(usize::from(u8::MAX) + 1)?;
    (byte_transitions <= PRETTY_BOUNDARY_REACHABLE_TRANSITION_BUDGET).then_some(byte_transitions)
}

/// Checks the complete Cartesian work bound before allocating any product-indexed storage.
fn pretty_boundary_product_len(right_states: usize, lexical_states: usize) -> Option<usize> {
    let product_states = right_states.checked_mul(lexical_states)?;
    let byte_transitions = product_states.checked_mul(usize::from(u8::MAX) + 1)?;
    (byte_transitions <= PRETTY_BOUNDARY_PRODUCT_TRANSITION_BUDGET).then_some(product_states)
}

/// Checks the sparse left traversal's work bound before retaining another product state.
fn pretty_boundary_left_transition_len(states: usize) -> Option<usize> {
    let byte_transitions = states.checked_mul(usize::from(u8::MAX) + 1)?;
    (byte_transitions <= PRETTY_BOUNDARY_LEFT_TRANSITION_BUDGET).then_some(byte_transitions)
}

impl PrettyReachableDfa {
    /// Reports the retained heap owned by one expanded dense transition graph.
    fn retained_bytes(&self) -> usize {
        let rows = self
            .transitions
            .capacity()
            .saturating_mul(std::mem::size_of::<Vec<Option<usize>>>());
        let transitions = self.transitions.iter().fold(0_usize, |bytes, row| {
            bytes.saturating_add(
                row.capacity()
                    .saturating_mul(std::mem::size_of::<Option<usize>>()),
            )
        });
        let accepts = self
            .accepts
            .capacity()
            .saturating_mul(std::mem::size_of::<bool>());
        rows.saturating_add(transitions).saturating_add(accepts)
    }

    /// Builds the complete live subgraph reachable from one anchored DFA state.
    fn new(
        automaton: &PrettyPatternAutomaton,
        counts: &mut PrettyBoundaryProductCounts,
    ) -> Option<Self> {
        let mut states = vec![automaton.start];
        let mut indices = BTreeMap::from([(automaton.start, 0)]);
        let mut transitions = Vec::new();
        let mut accepts = Vec::new();
        let mut state_index = 0;
        while state_index < states.len() {
            pretty_boundary_reachable_transition_len(states.len())?;
            let state = states[state_index];
            accepts.push(dfa_accepts_state(&automaton.dfa, state));
            let mut row = Vec::with_capacity(usize::from(u8::MAX) + 1);
            for byte in u8::MIN..=u8::MAX {
                counts.inspect_byte();
                let next = automaton.dfa.next_state(state, byte);
                if automaton.dfa.is_dead_state(next) || automaton.dfa.is_quit_state(next) {
                    row.push(None);
                    continue;
                }
                let next_index = if let Some(next_index) = indices.get(&next) {
                    *next_index
                } else {
                    pretty_boundary_reachable_transition_len(states.len() + 1)?;
                    let next_index = states.len();
                    states.push(next);
                    indices.insert(next, next_index);
                    next_index
                };
                row.push(Some(next_index));
            }
            transitions.push(row);
            state_index += 1;
        }
        #[cfg(test)]
        {
            counts.reachable_graphs_built += 1;
        }
        Some(Self {
            transitions,
            accepts,
        })
    }
}

/// Chooses one byte for each joint DFA equivalence class.
fn pretty_boundary_joint_byte_representatives(
    first: &PrettyPatternAutomaton,
    second: &PrettyPatternAutomaton,
) -> Vec<u8> {
    let first_classes = first.dfa.byte_classes();
    let second_classes = second.dfa.byte_classes();
    let mut seen = BTreeSet::new();
    let mut representatives = Vec::new();
    for byte in u8::MIN..=u8::MAX {
        if seen.insert((first_classes.get(byte), second_classes.get(byte))) {
            representatives.push(byte);
        }
    }
    representatives
}

/// Builds accepted left-boundary states once for reuse across distinct right languages.
fn build_pretty_left_product(
    left: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
    counts: &mut PrettyBoundaryProductCounts,
) -> Option<PrettyLeftProduct> {
    let joint_bytes = pretty_boundary_joint_byte_representatives(left, lexical);
    let lexical_graph = lexical.reachable(counts)?;
    pretty_boundary_left_transition_len(1)?;
    let lexical_start = 0;
    let mut boundary_states = BTreeSet::new();
    let mut seen = BTreeSet::from([(left.start, lexical_start)]);
    let mut frontier = VecDeque::from([(left.start, lexical_start)]);
    while let Some((left_state, lexical_state)) = frontier.pop_front() {
        counts.visit_left_product_state();
        if dfa_accepts_state(&left.dfa, left_state) {
            counts.lookup_boundary();
            boundary_states.insert(lexical_state);
        }
        for byte in &joint_bytes {
            counts.inspect_byte();
            let left_next = left.dfa.next_state(left_state, *byte);
            let Some(lexical_next) = lexical_graph.transitions[lexical_state][usize::from(*byte)]
            else {
                continue;
            };
            if left.dfa.is_dead_state(left_next) || left.dfa.is_quit_state(left_next) {
                continue;
            }
            let next = (left_next, lexical_next);
            if seen.contains(&next) {
                continue;
            }
            pretty_boundary_left_transition_len(seen.len() + 1)?;
            seen.insert(next);
            frontier.push_back(next);
        }
    }
    Some(PrettyLeftProduct {
        lexical_states: lexical_graph.transitions.len(),
        boundary_states: boundary_states.into_iter().collect(),
    })
}

/// Traverses one right-by-lexical product from cached accepted left-boundary states.
fn compiled_right_crosses(
    right: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
    left_product: &PrettyLeftProduct,
    counts: &mut PrettyBoundaryProductCounts,
) -> Option<bool> {
    let joint_bytes = pretty_boundary_joint_byte_representatives(right, lexical);
    let right_graph = right.reachable(counts)?;
    let lexical_graph = lexical.reachable(counts)?;
    if lexical_graph.transitions.len() != left_product.lexical_states {
        return None;
    }
    pretty_boundary_left_transition_len(left_product.boundary_states.len())?;
    let mut seen = left_product
        .boundary_states
        .iter()
        .map(|state| (0, Some(*state)))
        .collect::<BTreeSet<_>>();
    let mut frontier = seen.iter().copied().collect::<VecDeque<_>>();
    while let Some((right_state, lexical_state)) = frontier.pop_front() {
        counts.visit_right_product_state();
        if lexical_state.is_none() && right_graph.accepts[right_state] {
            return Some(true);
        }
        for byte in &joint_bytes {
            counts.inspect_byte();
            let Some(right_next) = right_graph.transitions[right_state][usize::from(*byte)] else {
                continue;
            };
            let lexical_next = match lexical_state {
                None => None,
                Some(lexical_state) => {
                    let Some(lexical_next) =
                        lexical_graph.transitions[lexical_state][usize::from(*byte)]
                    else {
                        continue;
                    };
                    (!lexical_graph.accepts[lexical_next]).then_some(lexical_next)
                }
            };
            let next = (right_next, lexical_next);
            if seen.contains(&next) {
                continue;
            }
            pretty_boundary_left_transition_len(seen.len() + 1)?;
            seen.insert(next);
            frontier.push_back(next);
        }
    }
    Some(false)
}

/// Builds one reusable right-side viability table independently of the left language.
fn build_pretty_right_product(
    right: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
    counts: &mut PrettyBoundaryProductCounts,
) -> Option<PrettyRightProduct> {
    let joint_bytes = pretty_boundary_joint_byte_representatives(right, lexical);
    let right_bytes = right
        .dfa
        .byte_classes()
        .representatives(..)
        .filter_map(|unit| unit.as_u8())
        .collect::<Vec<_>>();
    let right_graph = right.reachable(counts)?;
    let lexical_graph = lexical.reachable(counts)?;
    let right_len = right_graph.transitions.len();
    let lexical_len = lexical_graph.transitions.len();
    counts.record_right_spaces(right_len, lexical_len);

    let mut right_predecessors = vec![Vec::new(); right_len];
    let mut right_edge_marks = vec![0; right_len];
    for (source, transitions) in right_graph.transitions.iter().enumerate() {
        for byte in &right_bytes {
            let Some(target) = transitions[usize::from(*byte)] else {
                continue;
            };
            let edge_mark = source + 1;
            if right_edge_marks[target] != edge_mark {
                right_edge_marks[target] = edge_mark;
                right_predecessors[target].push(source);
            }
        }
    }
    let mut right_can_accept = right_graph.accepts.clone();
    let mut frontier = right_can_accept
        .iter()
        .enumerate()
        .filter_map(|(state, accepts)| accepts.then_some(state))
        .collect::<VecDeque<_>>();
    while let Some(state) = frontier.pop_front() {
        for predecessor in &right_predecessors[state] {
            if !right_can_accept[*predecessor] {
                right_can_accept[*predecessor] = true;
                frontier.push_back(*predecessor);
            }
        }
    }

    let product_len = pretty_boundary_product_len(right_len, lexical_len)?;
    let product_index = |right_state, lexical_state| right_state * lexical_len + lexical_state;
    let mut product_predecessors = vec![Vec::new(); product_len];
    let mut product_edge_marks = vec![0; product_len];
    let mut viable = vec![false; product_len];
    for right_state in 0..right_len {
        for lexical_state in 0..lexical_len {
            counts.visit_right_product_state();
            let source = product_index(right_state, lexical_state);
            for byte in &joint_bytes {
                counts.inspect_byte();
                let Some(right_next) = right_graph.transitions[right_state][usize::from(*byte)]
                else {
                    continue;
                };
                let Some(lexical_next) =
                    lexical_graph.transitions[lexical_state][usize::from(*byte)]
                else {
                    continue;
                };
                if lexical_graph.accepts[lexical_next] {
                    viable[source] |= right_can_accept[right_next];
                } else {
                    let target = product_index(right_next, lexical_next);
                    let edge_mark = source + 1;
                    if product_edge_marks[target] != edge_mark {
                        product_edge_marks[target] = edge_mark;
                        product_predecessors[target].push(source);
                    }
                }
            }
        }
    }
    let mut frontier = viable
        .iter()
        .enumerate()
        .filter_map(|(state, viable)| viable.then_some(state))
        .collect::<VecDeque<_>>();
    while let Some(state) = frontier.pop_front() {
        for predecessor in &product_predecessors[state] {
            if !viable[*predecessor] {
                viable[*predecessor] = true;
                frontier.push_back(*predecessor);
            }
        }
    }

    Some(PrettyRightProduct {
        lexical_states: lexical_len,
        viable,
    })
}

/// Traverses one left-by-lexical product using a previously built right-side viability table.
fn compiled_left_crosses(
    left: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
    right_product: &PrettyRightProduct,
    counts: &mut PrettyBoundaryProductCounts,
) -> Option<bool> {
    let joint_bytes = pretty_boundary_joint_byte_representatives(left, lexical);
    let lexical_graph = lexical.reachable(counts)?;
    if lexical_graph.transitions.len() != right_product.lexical_states {
        return None;
    }
    pretty_boundary_left_transition_len(1)?;
    let product_index =
        |right_state, lexical_state| right_state * right_product.lexical_states + lexical_state;
    let lexical_start = 0;
    let mut seen = BTreeSet::from([(left.start, lexical_start)]);
    let mut frontier = VecDeque::from([(left.start, lexical_start)]);
    while let Some((left_state, lexical_state)) = frontier.pop_front() {
        counts.visit_left_product_state();
        if dfa_accepts_state(&left.dfa, left_state) {
            counts.lookup_boundary();
            if right_product.viable[product_index(0, lexical_state)] {
                return Some(true);
            }
        }
        for byte in &joint_bytes {
            counts.inspect_byte();
            let left_next = left.dfa.next_state(left_state, *byte);
            let Some(lexical_next) = lexical_graph.transitions[lexical_state][usize::from(*byte)]
            else {
                continue;
            };
            if left.dfa.is_dead_state(left_next) || left.dfa.is_quit_state(left_next) {
                continue;
            }
            let next = (left_next, lexical_next);
            if seen.contains(&next) {
                continue;
            }
            pretty_boundary_left_transition_len(seen.len() + 1)?;
            seen.insert(next);
            frontier.push_back((left_next, lexical_next));
        }
    }
    Some(false)
}

/// Computes exact crossing semantics while retaining deterministic work counts for tests.
fn compiled_patterns_cross_with_counts(
    left: &PrettyPatternAutomaton,
    right: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
    counts: &mut PrettyBoundaryProductCounts,
) -> bool {
    let Some(right_product) = build_pretty_right_product(right, lexical, counts) else {
        return true;
    };
    compiled_left_crosses(left, lexical, &right_product, counts).unwrap_or(true)
}

/// Runs one compiled proof while retaining deterministic test-only work counts.
#[cfg(test)]
fn compiled_patterns_cross_counted(
    left: &PrettyPatternAutomaton,
    right: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
) -> (bool, PrettyBoundaryProductCounts) {
    let mut counts = PrettyBoundaryProductCounts::new();
    let result = compiled_patterns_cross_with_counts(left, right, lexical, &mut counts);
    (result, counts)
}

/// Retains the previous per-boundary-state search as an independent differential-test oracle.
#[cfg(test)]
fn compiled_patterns_cross_old_oracle(
    left: &PrettyPatternAutomaton,
    right: &PrettyPatternAutomaton,
    lexical: &PrettyPatternAutomaton,
) -> bool {
    let right_crosses = |lexical_start| {
        type BoundaryState = (
            regex_automata::util::primitives::StateID,
            Option<regex_automata::util::primitives::StateID>,
            bool,
        );
        let start: BoundaryState = (right.start, Some(lexical_start), false);
        let mut seen = BTreeSet::from([start]);
        let mut frontier = VecDeque::from([start]);
        while let Some((right_state, lexical_state, crossed_match)) = frontier.pop_front() {
            if crossed_match && dfa_accepts_state(&right.dfa, right_state) {
                return true;
            }
            for byte in u8::MIN..=u8::MAX {
                let right_next = right.dfa.next_state(right_state, byte);
                if right.dfa.is_dead_state(right_next) || right.dfa.is_quit_state(right_next) {
                    continue;
                }
                let (lexical_next, crossed_match) = if crossed_match {
                    (None, true)
                } else {
                    let Some(lexical_state) = lexical_state else {
                        continue;
                    };
                    let lexical_next = lexical.dfa.next_state(lexical_state, byte);
                    if lexical.dfa.is_dead_state(lexical_next)
                        || lexical.dfa.is_quit_state(lexical_next)
                    {
                        continue;
                    }
                    let crossed_match = dfa_accepts_state(&lexical.dfa, lexical_next);
                    ((!crossed_match).then_some(lexical_next), crossed_match)
                };
                let next = (right_next, lexical_next, crossed_match);
                if seen.insert(next) {
                    frontier.push_back(next);
                }
            }
        }
        false
    };

    let mut right_results = BTreeMap::new();
    let mut seen = BTreeSet::from([(left.start, lexical.start)]);
    let mut frontier = VecDeque::from([(left.start, lexical.start)]);
    while let Some((left_state, lexical_state)) = frontier.pop_front() {
        if dfa_accepts_state(&left.dfa, left_state)
            && *right_results
                .entry(lexical_state)
                .or_insert_with(|| right_crosses(lexical_state))
        {
            return true;
        }
        for byte in u8::MIN..=u8::MAX {
            let left_next = left.dfa.next_state(left_state, byte);
            let lexical_next = lexical.dfa.next_state(lexical_state, byte);
            if left.dfa.is_dead_state(left_next)
                || left.dfa.is_quit_state(left_next)
                || lexical.dfa.is_dead_state(lexical_next)
                || lexical.dfa.is_quit_state(lexical_next)
                || !seen.insert((left_next, lexical_next))
            {
                continue;
            }
            frontier.push_back((left_next, lexical_next));
        }
    }
    false
}

/// Derives the patterns cross into pattern from the retained source model and its correlations.
fn patterns_cross_into_pattern(left: &str, right: &str, lexical_pattern: &str) -> bool {
    PrettyBoundaryProver::default().patterns_cross(left, right, lexical_pattern)
}

/// Derives the pattern then fixed intersects pattern from the retained source model and its correlations.
fn pattern_then_fixed_intersects_pattern(
    pattern: &str,
    fixed: &str,
    lexical_pattern: &str,
) -> bool {
    patterns_cross_into_pattern(pattern, &regex_syntax::escape(fixed), lexical_pattern)
}

/// Derives the fixed then pattern intersects pattern from the retained source model and its correlations.
fn fixed_then_pattern_intersects_pattern(
    fixed: &str,
    pattern: &str,
    lexical_pattern: &str,
) -> bool {
    patterns_cross_into_pattern(&regex_syntax::escape(fixed), pattern, lexical_pattern)
}

/// Derives the source trivia supports from the retained source model and its correlations.
fn source_trivia_supports(source: &SourceModel, required: char) -> bool {
    source.lexer_records.iter().any(|record| {
        regex_syntax::Parser::new()
            .parse(&record.pattern)
            .is_ok_and(|hir| hir_matches_single_char(&hir, required).1)
    })
}

/// Derives the source trivia supports breaks from the retained source model and its correlations.
fn source_trivia_supports_breaks(source: &SourceModel) -> bool {
    [' ', '\n']
        .into_iter()
        .all(|required| source_trivia_supports(source, required))
}

/// Builds the immutable source-ordered lexical pattern catalog used during materialization.
pub(crate) fn materialized_pretty_lexical_patterns(
    source: &SourceModel,
    equivalence: crate::TextEquivalence,
) -> Vec<String> {
    source
        .token_definitions
        .iter()
        .map(|definition| match &definition.kind {
            TokenDefinitionKind::Fixed(fixed) => {
                fixed_lexical_pattern(&fixed.spelling, equivalence)
            }
            TokenDefinitionKind::Content(content) => content.pattern.clone(),
            TokenDefinitionKind::LexicalBase(base) => base.pattern.clone(),
        })
        .chain(
            source
                .lexer_records
                .iter()
                .map(|record| record.pattern.clone()),
        )
        .collect()
}

/// Derives the materialized pretty boundary changes lexing from the retained source model and its correlations.
pub(crate) fn materialized_pretty_boundary_changes_lexing(
    source: &SourceModel,
    equivalence: crate::TextEquivalence,
    left: &MaterializedPrettyBoundary,
    right: &MaterializedPrettyBoundary,
    lexical_catalog: PrettyPatternCatalogId,
    prover: &mut PrettyBoundaryProver,
) -> bool {
    let fixed = |boundary: &MaterializedPrettyBoundary| match boundary {
        MaterializedPrettyBoundary::Fixed(id) => {
            let TokenDefinitionKind::Fixed(fixed) = &source.token_definitions[id.index()].kind
            else {
                unreachable!("a materialized fixed Pretty boundary names fixed syntax")
            };
            Some(fixed)
        }
        MaterializedPrettyBoundary::Regex(_) => None,
    };
    let left_fixed = fixed(left);
    let right_fixed = fixed(right);
    let left_requests_space = left_fixed.is_some_and(|fixed| {
        matches!(
            fixed.pretty_spacing,
            PrettySpacing::After | PrettySpacing::Around
        )
    });
    let right_requests_space = right_fixed.is_some_and(|fixed| {
        matches!(
            fixed.pretty_spacing,
            PrettySpacing::Before | PrettySpacing::Around
        )
    });
    let explicitly_tight = left_fixed.is_some_and(|fixed| {
        matches!(
            fixed.pretty_spacing,
            PrettySpacing::Tight | PrettySpacing::Before
        )
    }) || right_fixed.is_some_and(|fixed| {
        matches!(
            fixed.pretty_spacing,
            PrettySpacing::Tight | PrettySpacing::After
        )
    });
    let left_auto_punctuation = left_fixed.is_some_and(|fixed| {
        fixed.pretty_spacing == PrettySpacing::Auto
            && !is_word_shaped(&fixed.spelling)
            && !matches!(fixed.spelling.as_str(), "(" | "[" | "{")
    });
    let left_auto_open = left_fixed.is_some_and(|fixed| {
        fixed.pretty_spacing == PrettySpacing::Auto
            && matches!(fixed.spelling.as_str(), "(" | "[" | "{")
    });
    let right_auto_punctuation = right_fixed.is_some_and(|fixed| {
        fixed.pretty_spacing == PrettySpacing::Auto && !is_word_shaped(&fixed.spelling)
    });
    let runtime_auto_tight = left_auto_open || (right_auto_punctuation && !left_auto_punctuation);
    if left_requests_space || right_requests_space || (!explicitly_tight && !runtime_auto_tight) {
        return !source_trivia_supports_breaks(source);
    }

    let boundary_pattern = |boundary: &MaterializedPrettyBoundary| match boundary {
        MaterializedPrettyBoundary::Fixed(_) => fixed_pretty_pattern(
            fixed(boundary).expect("fixed boundary has a definition"),
            equivalence,
        ),
        MaterializedPrettyBoundary::Regex(pattern) => pattern.clone(),
    };
    let left_pattern = boundary_pattern(left);
    let right_pattern = boundary_pattern(right);
    prover.patterns_cross_catalog(&left_pattern, &right_pattern, lexical_catalog)
}

/// Derives the hir matches single char from the retained source model and its correlations.
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

/// Parses and validates the supported Pretty hint vocabulary.
fn parse_pretty_hints(metas: &[Meta]) -> Result<PrettyHints, String> {
    // Parse all Pretty options before validation so duplicate, unknown, and interacting settings share one diagnostic boundary.
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
    /// Appends one token attachment and remembers its boundary in the element sequence.
    fn push_attachment(&mut self, elements: Vec<GrammarElement>) {
        self.attachment_starts.push(self.elements.len());
        self.elements.extend(elements);
    }

    /// Derives the second attachment start from the retained source model and its correlations.
    fn second_attachment_start(&self) -> Option<usize> {
        self.attachment_starts.get(1).copied()
    }

    /// Converts this value into the elements.
    fn into_elements(self) -> Vec<GrammarElement> {
        self.elements
    }
}

impl SourceModelBuilder<'_> {
    /// Indexes the first source-ordered definition for each semantic content token.
    fn index_pretty_content_definitions(&mut self) {
        let mut definitions = vec![None; self.resolved.content_tokens.len()];
        for definition in &self.tokens.definitions {
            let TokenDefinitionKind::Content(content) = &definition.kind else {
                continue;
            };
            let slot = definitions
                .get_mut(content.semantic.index())
                .expect("content token identities are dense");
            if slot.is_none() {
                *slot = Some(definition.id);
            }
        }
        self.pretty_content_definitions = definitions;
    }

    /// Diagnoses spacing hints that conflict with fixed-token lexical boundaries.
    fn validate_pretty_spacing(&mut self) {
        let lexical_patterns = self.pretty_lexical_patterns(true);
        let non_fixed_lexical_patterns = self.pretty_lexical_patterns(false);
        let (lexical_catalog, non_fixed_lexical_catalog) = {
            let mut prover = self.pretty_boundary_prover.borrow_mut();
            (
                prover.intern_catalog(&lexical_patterns),
                prover.intern_catalog(&non_fixed_lexical_patterns),
            )
        };
        self.pretty_lexical_catalog = Some(lexical_catalog);
        self.pretty_non_fixed_lexical_catalog = Some(non_fixed_lexical_catalog);

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
                    let mut grouping_analysis = PrettyPrattGroupingAnalysis::default();
                    let grouping_candidates = variants
                        .iter()
                        .filter_map(|variant| {
                            matches!(variant.position, SourcePrattPosition::Atom)
                                .then(|| {
                                    self.pretty_pratt_grouping_is_delegated(
                                        node.resolved_node,
                                        &variant.common,
                                        &mut grouping_analysis,
                                    )
                                })
                                .flatten()
                        })
                        .collect::<Vec<_>>();
                    let has_grouping = grouping_candidates.iter().any(|delegated| !delegated)
                        || matches!(grouping_candidates.as_slice(), [true]);
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

    /// Derives the report disabled pretty hints from the retained source model and its correlations.
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

    /// Validates Pretty hints against the semantic fields visible in one scope.
    fn validate_pretty_scope(
        &self,
        pretty: &PrettyHints,
        elements: &[GrammarElement],
        scope: &str,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let needs_soft_opportunity = pretty.group.is_some() || pretty.indent;
        self.validate_pretty_scope_facts(
            pretty,
            needs_soft_opportunity
                && self.pretty_scope_has_soft_opportunity(elements, &mut Vec::new()),
            scope,
            diagnostics,
        );
    }

    /// Validates pretty scope facts.
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

    /// Validates pretty multi scope.
    fn validate_pretty_multi_scope<'a>(
        &self,
        pretty: &PrettyHints,
        mut variants: impl Iterator<Item = &'a [GrammarElement]>,
        scope: &str,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let has_soft_opportunity = (pretty.group.is_some() || pretty.indent)
            && variants
                .any(|elements| self.pretty_scope_has_soft_opportunity(elements, &mut Vec::new()));
        self.validate_pretty_scope_facts(pretty, has_soft_opportunity, scope, diagnostics);
    }

    /// Derives the pretty scope has soft opportunity from the retained source model and its correlations.
    fn pretty_scope_has_soft_opportunity(
        &self,
        elements: &[GrammarElement],
        visiting: &mut Vec<NodeId>,
    ) -> bool {
        let mut cycle_dependent = false;
        self.pretty_scope_has_soft_opportunity_inner(elements, visiting, &mut cycle_dependent)
    }

    /// Derives one soft-opportunity result while tracking recursive false fallbacks.
    fn pretty_scope_has_soft_opportunity_inner(
        &self,
        elements: &[GrammarElement],
        visiting: &mut Vec<NodeId>,
        cycle_dependent: &mut bool,
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
                    cycle_dependent,
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

    /// Derives the pretty type has soft opportunity from the retained source model and its correlations.
    fn pretty_type_has_soft_opportunity(
        &self,
        ty: &ResolvedType,
        visiting: &mut Vec<NodeId>,
        cycle_dependent: &mut bool,
    ) -> bool {
        match &ty.kind {
            ResolvedTypeKind::Semantic {
                target, arguments, ..
            } => match target {
                SemanticTarget::Node(node) => {
                    if visiting.contains(node) {
                        *cycle_dependent = true;
                        return false;
                    }
                    let cached = self
                        .pretty_boundary_analysis
                        .borrow()
                        .soft_nodes
                        .get(node)
                        .copied();
                    if let Some(result) = cached {
                        return result;
                    }
                    #[cfg(test)]
                    {
                        let mut analysis = self.pretty_boundary_analysis.borrow_mut();
                        analysis.soft_node_evaluations =
                            analysis.soft_node_evaluations.saturating_add(1);
                    }
                    visiting.push(*node);
                    let mut node_cycle_dependent = false;
                    let result = self
                        .nodes
                        .get(node.index())
                        .filter(|candidate| candidate.resolved_node == *node)
                        .is_some_and(|node| match &node.body {
                            NodeBody::Struct(elements) | NodeBody::Presence(elements) => self
                                .pretty_scope_has_soft_opportunity_inner(
                                    elements,
                                    visiting,
                                    &mut node_cycle_dependent,
                                ),
                            NodeBody::Enum(variants) => variants.iter().any(|variant| {
                                self.pretty_scope_has_soft_opportunity_inner(
                                    &variant.elements,
                                    visiting,
                                    &mut node_cycle_dependent,
                                )
                            }),
                            NodeBody::Pratt(variants) => variants.iter().any(|variant| {
                                self.pretty_scope_has_soft_opportunity_inner(
                                    &variant.common.elements,
                                    visiting,
                                    &mut node_cycle_dependent,
                                )
                            }),
                        });
                    visiting.pop();
                    if result || !node_cycle_dependent {
                        self.pretty_boundary_analysis
                            .borrow_mut()
                            .soft_nodes
                            .insert(*node, result);
                    } else {
                        *cycle_dependent = true;
                    }
                    result
                }
                SemanticTarget::Catalog(CatalogType::Vec | CatalogType::Vec1) => true,
                SemanticTarget::Catalog(CatalogType::Box | CatalogType::Option) => arguments
                    .iter()
                    .find_map(|argument| match argument {
                        ResolvedGenericArgument::Type(inner) => Some(
                            self.pretty_type_has_soft_opportunity(inner, visiting, cycle_dependent),
                        ),
                        ResolvedGenericArgument::Lifetime(_)
                        | ResolvedGenericArgument::Unsupported(_) => None,
                    })
                    .unwrap_or(false),
                SemanticTarget::TypeAlias(alias) => self
                    .resolved
                    .type_aliases
                    .get(alias.index())
                    .filter(|candidate| candidate.id == *alias)
                    .and_then(|alias| alias.target.as_ref())
                    .is_some_and(|target| {
                        self.pretty_type_has_soft_opportunity(target, visiting, cycle_dependent)
                    }),
                SemanticTarget::ContentToken(_) | SemanticTarget::GenericParameter(_) => false,
            },
            ResolvedTypeKind::Group(inner)
            | ResolvedTypeKind::Parenthesized(inner)
            | ResolvedTypeKind::Reference(inner) => {
                self.pretty_type_has_soft_opportunity(inner, visiting, cycle_dependent)
            }
            ResolvedTypeKind::FixedTokenPresence
            | ResolvedTypeKind::Tuple(_)
            | ResolvedTypeKind::Unsupported => false,
        }
    }

    /// Reports whether a valid grouping path exists and whether it delegates through a helper.
    fn pretty_pratt_grouping_is_delegated(
        &self,
        node: NodeId,
        variant: &Variant,
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> Option<bool> {
        let proof = self.pretty_pratt_grouping_elements(node, &variant.elements, analysis);
        let [witness] = proof.witnesses.as_slice() else {
            return None;
        };
        (proof.cyclic.is_empty()
            && if witness.delegated {
                witness.before == 1 && witness.after == 1
            } else {
                witness.before > 0 && witness.after > 0
            })
        .then_some(witness.delegated)
    }

    /// Derives empty and single-owner paths through one authored element sequence.
    fn pretty_pratt_grouping_elements(
        &self,
        owner: NodeId,
        elements: &[GrammarElement],
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> PrettyPrattGroupingProof {
        let empty = elements
            .iter()
            .all(|element| self.pretty_pratt_grouping_element_is_empty(owner, element, analysis));
        let mut proof = PrettyPrattGroupingProof {
            empty,
            ..PrettyPrattGroupingProof::default()
        };
        for (index, element) in elements.iter().enumerate() {
            let GrammarElement::SemanticField {
                resolved_field,
                projection: SemanticFieldProjection::Direct,
                attachment,
                ..
            } = element
            else {
                continue;
            };
            let Some(context_before) =
                self.pretty_pratt_grouping_fixed_context(owner, &elements[..index], analysis)
            else {
                continue;
            };
            let Some(context_after) =
                self.pretty_pratt_grouping_fixed_context(owner, &elements[index + 1..], analysis)
            else {
                continue;
            };
            let Some((attached_before, attached_after)) =
                attachment.as_ref().map_or(Some((0, 0)), |attachment| {
                    Some((
                        Self::required_pretty_attachment_len(&attachment.before)?,
                        Self::required_pretty_attachment_len(&attachment.after)?,
                    ))
                })
            else {
                continue;
            };
            let before = context_before + attached_before;
            let after = context_after + attached_after;
            let nested = self.pretty_pratt_grouping_type(
                owner,
                &self.resolved_field(*resolved_field).ty,
                analysis,
            );
            let context_is_transparent = before == 0 && after == 0;
            let context_is_enclosure = before > 0 && after > 0;
            // A half-open fixed context (required syntax on exactly one side)
            // can never contribute a witness, so an under-enumerated cyclic
            // subtree behind it cannot hide one either; only witness-capable
            // contexts propagate cyclic incompleteness into the proof.
            if context_is_transparent || context_is_enclosure {
                proof.cyclic.extend(nested.cyclic.iter().copied());
            }
            for witness in nested.witnesses {
                let witness_is_transparent = witness.before == 0 && witness.after == 0;
                if context_is_transparent || (context_is_enclosure && witness_is_transparent) {
                    proof.push_witness(PrettyPrattGroupingBounds {
                        before: before + witness.before,
                        after: after + witness.after,
                        delegated: witness.delegated,
                    });
                }
            }
        }
        proof
    }

    /// Counts required fixed syntax around an owner path.
    ///
    /// Optional grammar elements take their empty derivation. A required
    /// semantic sibling is accepted only when it resolves to one exact,
    /// non-empty fixed-only helper expansion.
    fn pretty_pratt_grouping_fixed_context(
        &self,
        owner: NodeId,
        elements: &[GrammarElement],
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> Option<usize> {
        let mut count = 0usize;
        for element in elements {
            match element {
                GrammarElement::FixedToken { .. } => count = count.checked_add(1)?,
                GrammarElement::OptionalFixedTokenGroup { .. } => {}
                GrammarElement::SemanticField { .. }
                    if self.pretty_pratt_grouping_element_is_empty(owner, element, analysis) => {}
                GrammarElement::SemanticField {
                    resolved_field,
                    projection: SemanticFieldProjection::Direct,
                    attachment,
                    ..
                } => {
                    let attachment = attachment.as_ref();
                    let before = attachment.map_or(Some(0), |attachment| {
                        Self::required_pretty_attachment_len(&attachment.before)
                    })?;
                    let after = attachment.map_or(Some(0), |attachment| {
                        Self::required_pretty_attachment_len(&attachment.after)
                    })?;
                    let fixed = self.pretty_pratt_fixed_only_type_len(
                        &self.resolved_field(*resolved_field).ty,
                        analysis,
                    )?;
                    if fixed == 0 {
                        return None;
                    }
                    count = count
                        .checked_add(before)?
                        .checked_add(fixed)?
                        .checked_add(after)?;
                }
                GrammarElement::SemanticField {
                    projection: SemanticFieldProjection::FixedPresence { .. },
                    ..
                } => return None,
            }
        }
        Some(count)
    }

    /// Proves the exact number of required fixed tokens stored by a type.
    fn pretty_pratt_fixed_only_type_len(
        &self,
        ty: &ResolvedType,
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> Option<usize> {
        match &ty.kind {
            ResolvedTypeKind::Semantic {
                target, arguments, ..
            } => match target {
                SemanticTarget::Node(node) => {
                    self.pretty_pratt_fixed_only_node_len(*node, analysis)
                }
                SemanticTarget::TypeAlias(alias) => {
                    if !analysis.active_fixed_aliases.insert(alias.index()) {
                        return None;
                    }
                    let fixed = self
                        .resolved
                        .type_aliases
                        .get(alias.index())
                        .filter(|candidate| candidate.id == *alias)
                        .and_then(|alias| alias.target.as_ref())
                        .and_then(|ty| self.pretty_pratt_fixed_only_type_len(ty, analysis));
                    analysis.active_fixed_aliases.remove(&alias.index());
                    fixed
                }
                SemanticTarget::Catalog(catalog) => {
                    let mut types = arguments.iter().filter_map(|argument| match argument {
                        ResolvedGenericArgument::Type(ty) => Some(ty),
                        ResolvedGenericArgument::Lifetime(_)
                        | ResolvedGenericArgument::Unsupported(_) => None,
                    });
                    let inner = types.next()?;
                    if types.next().is_some() || !matches!(catalog, CatalogType::Box) {
                        return None;
                    }
                    self.pretty_pratt_fixed_only_type_len(inner, analysis)
                }
                SemanticTarget::ContentToken(_) | SemanticTarget::GenericParameter(_) => None,
            },
            ResolvedTypeKind::Group(inner)
            | ResolvedTypeKind::Parenthesized(inner)
            | ResolvedTypeKind::Reference(inner) => {
                self.pretty_pratt_fixed_only_type_len(inner, analysis)
            }
            ResolvedTypeKind::FixedTokenPresence
            | ResolvedTypeKind::Tuple(_)
            | ResolvedTypeKind::Unsupported => None,
        }
    }

    /// Proves one non-recursive helper Node has exactly one fixed-only path.
    fn pretty_pratt_fixed_only_node_len(
        &self,
        node: NodeId,
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> Option<usize> {
        if let Some(fixed) = analysis.fixed_memo.get(&node.index()) {
            return *fixed;
        }
        if !analysis.active_fixed_nodes.insert(node.index()) {
            return None;
        }
        let fixed = self
            .nodes
            .get(node.index())
            .filter(|candidate| candidate.resolved_node == node)
            .and_then(|node| match &node.body {
                NodeBody::Struct(elements) | NodeBody::Presence(elements) => {
                    self.pretty_pratt_fixed_only_elements_len(elements, analysis)
                }
                NodeBody::Enum(variants) if variants.len() == 1 => {
                    self.pretty_pratt_fixed_only_elements_len(&variants[0].elements, analysis)
                }
                NodeBody::Enum(_) | NodeBody::Pratt(_) => None,
            });
        analysis.active_fixed_nodes.remove(&node.index());
        analysis.fixed_memo.insert(node.index(), fixed);
        fixed
    }

    /// Proves an authored helper sequence contains only exact fixed syntax.
    fn pretty_pratt_fixed_only_elements_len(
        &self,
        elements: &[GrammarElement],
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> Option<usize> {
        let mut count = 0usize;
        for element in elements {
            match element {
                GrammarElement::FixedToken { .. } => count = count.checked_add(1)?,
                GrammarElement::SemanticField {
                    resolved_field,
                    projection: SemanticFieldProjection::Direct,
                    attachment,
                    ..
                } => {
                    let attachment = attachment.as_ref();
                    let before = attachment.map_or(Some(0), |attachment| {
                        Self::required_pretty_attachment_len(&attachment.before)
                    })?;
                    let after = attachment.map_or(Some(0), |attachment| {
                        Self::required_pretty_attachment_len(&attachment.after)
                    })?;
                    let nested = self.pretty_pratt_fixed_only_type_len(
                        &self.resolved_field(*resolved_field).ty,
                        analysis,
                    )?;
                    count = count
                        .checked_add(before)?
                        .checked_add(nested)?
                        .checked_add(after)?;
                }
                GrammarElement::OptionalFixedTokenGroup { .. }
                | GrammarElement::SemanticField {
                    projection: SemanticFieldProjection::FixedPresence { .. },
                    ..
                } => return None,
            }
        }
        Some(count)
    }

    /// Reports whether an element can disappear from the selected grouping derivation.
    fn pretty_pratt_grouping_element_is_empty(
        &self,
        owner: NodeId,
        element: &GrammarElement,
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> bool {
        match element {
            GrammarElement::OptionalFixedTokenGroup { .. } => true,
            GrammarElement::SemanticField {
                resolved_field,
                projection: SemanticFieldProjection::Direct,
                ..
            } => {
                let proof = self.pretty_pratt_grouping_type(
                    owner,
                    &self.resolved_field(*resolved_field).ty,
                    analysis,
                );
                let attachment_is_empty = match element {
                    GrammarElement::SemanticField { attachment, .. } => {
                        attachment.as_ref().is_none_or(|attachment| {
                            attachment.before.iter().all(|syntax| {
                                matches!(syntax, AttachedFixedSyntax::Optional { .. })
                            }) && attachment.after.iter().all(|syntax| {
                                matches!(syntax, AttachedFixedSyntax::Optional { .. })
                            })
                        })
                    }
                    GrammarElement::FixedToken { .. }
                    | GrammarElement::OptionalFixedTokenGroup { .. } => false,
                };
                proof.cyclic.is_empty() && proof.empty && attachment_is_empty
            }
            GrammarElement::FixedToken { .. }
            | GrammarElement::SemanticField {
                projection: SemanticFieldProjection::FixedPresence { .. },
                ..
            } => false,
        }
    }

    /// Counts required fixed tokens, rejecting optional syntax inside a grouping boundary.
    fn required_pretty_attachment_len(syntax: &[AttachedFixedSyntax]) -> Option<usize> {
        syntax
            .iter()
            .try_fold(0usize, |count, syntax| match syntax {
                AttachedFixedSyntax::Required(_) => Some(count + 1),
                AttachedFixedSyntax::Optional { .. } => None,
            })
    }

    /// Derives grouping paths through storage wrappers and authored helper Nodes.
    fn pretty_pratt_grouping_type(
        &self,
        owner: NodeId,
        ty: &ResolvedType,
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> PrettyPrattGroupingProof {
        match &ty.kind {
            ResolvedTypeKind::Semantic {
                target, arguments, ..
            } => match target {
                SemanticTarget::Node(candidate) if *candidate == owner => {
                    PrettyPrattGroupingProof {
                        empty: false,
                        witnesses: vec![PrettyPrattGroupingBounds::default()],
                        cyclic: BTreeSet::new(),
                    }
                }
                SemanticTarget::Node(candidate) => {
                    self.pretty_pratt_grouping_node(owner, *candidate, analysis)
                }
                SemanticTarget::Catalog(catalog) => {
                    let mut types = arguments.iter().filter_map(|argument| match argument {
                        ResolvedGenericArgument::Type(ty) => Some(ty),
                        ResolvedGenericArgument::Lifetime(_)
                        | ResolvedGenericArgument::Unsupported(_) => None,
                    });
                    let Some(inner) = types.next() else {
                        return PrettyPrattGroupingProof::default();
                    };
                    if types.next().is_some() {
                        return PrettyPrattGroupingProof::default();
                    }
                    match catalog {
                        CatalogType::Box | CatalogType::Vec1 => {
                            self.pretty_pratt_grouping_type(owner, inner, analysis)
                        }
                        CatalogType::Option | CatalogType::Vec => PrettyPrattGroupingProof {
                            empty: true,
                            witnesses: Vec::new(),
                            cyclic: BTreeSet::new(),
                        },
                    }
                }
                SemanticTarget::TypeAlias(alias) => {
                    if !analysis.active_aliases.insert(alias.index()) {
                        return PrettyPrattGroupingProof {
                            cyclic: BTreeSet::from([PrettyPrattGroupingCycle::TypeAlias(
                                alias.index(),
                            )]),
                            ..PrettyPrattGroupingProof::default()
                        };
                    }
                    let mut proof = self
                        .resolved
                        .type_aliases
                        .get(alias.index())
                        .filter(|candidate| candidate.id == *alias)
                        .and_then(|alias| alias.target.as_ref())
                        .map_or_else(PrettyPrattGroupingProof::default, |ty| {
                            self.pretty_pratt_grouping_type(owner, ty, analysis)
                        });
                    analysis.active_aliases.remove(&alias.index());
                    Self::discharge_witnessless_grouping_cycle(
                        &mut proof,
                        PrettyPrattGroupingCycle::TypeAlias(alias.index()),
                    );
                    proof
                }
                SemanticTarget::ContentToken(_) | SemanticTarget::GenericParameter(_) => {
                    PrettyPrattGroupingProof::default()
                }
            },
            ResolvedTypeKind::Group(inner)
            | ResolvedTypeKind::Parenthesized(inner)
            | ResolvedTypeKind::Reference(inner) => {
                self.pretty_pratt_grouping_type(owner, inner, analysis)
            }
            ResolvedTypeKind::FixedTokenPresence
            | ResolvedTypeKind::Tuple(_)
            | ResolvedTypeKind::Unsupported => PrettyPrattGroupingProof::default(),
        }
    }

    /// Derives grouping paths through one non-owning authored struct or enum helper.
    fn pretty_pratt_grouping_node(
        &self,
        owner: NodeId,
        node: NodeId,
        analysis: &mut PrettyPrattGroupingAnalysis,
    ) -> PrettyPrattGroupingProof {
        let key = (owner.index(), node.index());
        if let Some(proof) = analysis.memo.get(&key) {
            return proof.clone();
        }
        if !analysis.active_nodes.insert(node.index()) {
            return PrettyPrattGroupingProof {
                cyclic: BTreeSet::from([PrettyPrattGroupingCycle::Node(node.index())]),
                ..PrettyPrattGroupingProof::default()
            };
        }
        let mut proof = self
            .nodes
            .get(node.index())
            .filter(|candidate| candidate.resolved_node == node)
            .map_or_else(PrettyPrattGroupingProof::default, |node| match &node.body {
                NodeBody::Struct(elements) | NodeBody::Presence(elements) => {
                    self.pretty_pratt_grouping_elements(owner, elements, analysis)
                }
                NodeBody::Enum(variants) => {
                    let mut proof = PrettyPrattGroupingProof::default();
                    for variant in variants {
                        let variant =
                            self.pretty_pratt_grouping_elements(owner, &variant.elements, analysis);
                        proof.empty |= variant.empty;
                        proof.cyclic.extend(variant.cyclic.iter().copied());
                        for witness in variant.witnesses {
                            proof.push_witness(witness);
                        }
                    }
                    proof
                }
                NodeBody::Pratt(_) => PrettyPrattGroupingProof::default(),
            });
        analysis.active_nodes.remove(&node.index());
        Self::discharge_witnessless_grouping_cycle(
            &mut proof,
            PrettyPrattGroupingCycle::Node(node.index()),
        );
        for witness in &mut proof.witnesses {
            witness.delegated = true;
        }
        if proof.cyclic.is_empty() {
            analysis.memo.insert(key, proof.clone());
        }
        proof
    }

    /// Retires a closed declaration's own cycle mark once it proves witnessless.
    ///
    /// The proof was derived with the declaration's re-entry contributing no
    /// witness. An empty result therefore satisfies the declaration's own
    /// witness equation, and since a witness stands for a finite derivation the
    /// least such solution is the true one: no unrolling of this cycle can add a
    /// witness the proof missed. A declaration that did find a witness keeps its
    /// mark, because unrolling can wrap that witness in the cycle's own fixed
    /// syntax and so supply a second, distinct grouping.
    fn discharge_witnessless_grouping_cycle(
        proof: &mut PrettyPrattGroupingProof,
        cycle: PrettyPrattGroupingCycle,
    ) {
        if proof.witnesses.is_empty() {
            proof.cyclic.remove(&cycle);
        }
    }

    /// Validates pretty element boundaries.
    fn validate_pretty_element_boundaries(
        &self,
        elements: &[GrammarElement],
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        // Validate neighboring semantic elements as a sequence because spacing and breaks are boundary properties.
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
            let needs_internal_break = pretty.group.is_some() || pretty.indent;
            let attachment_has_soft_opportunity = needs_internal_break
                && attachment.as_ref().is_some_and(|attachment| {
                    self.pretty_attachment_has_soft_opportunity(*resolved_field, attachment)
                });
            if let Some(attachment) = attachment {
                self.validate_pretty_attachment_boundaries(
                    *resolved_field,
                    attachment,
                    diagnostics,
                );
            }
            let has_internal_break = needs_internal_break
                && (separator.is_some()
                    || self.field_has_repetition(*resolved_field).unwrap_or(true)
                    || attachment_has_soft_opportunity
                    || self.pretty_scope_has_soft_opportunity(
                        &elements[index..=index],
                        &mut Vec::new(),
                    ));
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
            let tight_before = pretty.break_before.is_some()
                && attachment_start.checked_sub(1).is_some_and(|before| {
                    boundary_is_tight(&elements[before], &elements[attachment_start])
                });
            let tight_after = pretty.break_after.is_some()
                && elements
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
            let break_after = matches!(
                left,
                GrammarElement::SemanticField { pretty, .. }
                    if pretty.break_after.is_some()
            );
            let break_before = matches!(
                right,
                GrammarElement::SemanticField { pretty, .. }
                    if pretty.break_before.is_some()
            );
            if break_after || break_before {
                continue;
            }
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

    /// Derives the pretty attachment has soft opportunity from the retained source model and its correlations.
    fn pretty_attachment_has_soft_opportunity(
        &self,
        field: ResolvedFieldId,
        attachment: &FixedTokenAttachment,
    ) -> bool {
        let fixed = |token: &AttachedFixedToken| PrettyBoundary {
            kind: PrettyBoundaryKind::Fixed(self.tokens.uses[token.token_use.index()].definition),
            site: token.site.clone(),
        };
        let before = attachment
            .before
            .iter()
            .filter_map(AttachedFixedSyntax::first)
            .map(fixed)
            .collect::<Vec<_>>();
        let after = attachment
            .after
            .iter()
            .filter_map(AttachedFixedSyntax::first)
            .map(fixed)
            .collect::<Vec<_>>();
        let site = attachment
            .before
            .first()
            .or_else(|| attachment.after.first())
            .map(|syntax| syntax.site().clone())
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

    /// Validates pretty attachment boundaries.
    fn validate_pretty_attachment_boundaries(
        &self,
        field: ResolvedFieldId,
        attachment: &FixedTokenAttachment,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let fixed = |token: &AttachedFixedToken| PrettyBoundary {
            kind: PrettyBoundaryKind::Fixed(self.tokens.uses[token.token_use.index()].definition),
            site: token.site.clone(),
        };
        let before = attachment
            .before
            .iter()
            .filter_map(AttachedFixedSyntax::first)
            .map(fixed)
            .collect::<Vec<_>>();
        let after = attachment
            .after
            .iter()
            .filter_map(AttachedFixedSyntax::first)
            .map(fixed)
            .collect::<Vec<_>>();
        let site = attachment
            .before
            .first()
            .or_else(|| attachment.after.first())
            .map(|syntax| syntax.site().clone())
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

        for pair in before.windows(2).chain(after.windows(2)) {
            self.validate_pretty_boundary(&pair[0], &pair[1], diagnostics);
        }
        if let Some(left) = before.last() {
            for right in &first {
                self.validate_pretty_boundary(left, right, diagnostics);
            }
        }
        if let Some(right) = after.first() {
            for left in &last {
                self.validate_pretty_boundary(left, right, diagnostics);
            }
        }
    }

    /// Derives the pretty trivia supports breaks from the retained source model and its correlations.
    fn pretty_trivia_supports_breaks(&self) -> bool {
        *self.pretty_trivia_supports_breaks.get_or_init(|| {
            [' ', '\n']
                .into_iter()
                .all(|required| self.pretty_trivia_supports(required))
        })
    }

    /// Derives the pretty trivia supports from the retained source model and its correlations.
    fn pretty_trivia_supports(&self, required: char) -> bool {
        self.tokens.lexer_records.iter().any(|record| {
            regex_syntax::Parser::new()
                .parse(&record.pattern)
                .is_ok_and(|hir| hir_matches_single_char(&hir, required).1)
        })
    }

    /// Derives the field has repetition from the retained source model and its correlations.
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
                        .get(alias.index())
                        .filter(|candidate| candidate.id == *alias)
                        .and_then(|alias| alias.target.as_ref())
                        .and_then(|ty| repeated(builder, ty)),
                    SemanticTarget::Node(_) | SemanticTarget::ContentToken(_) => Some(false),
                    SemanticTarget::GenericParameter(_) => None,
                },
                ResolvedTypeKind::Group(inner)
                | ResolvedTypeKind::Parenthesized(inner)
                | ResolvedTypeKind::Reference(inner) => repeated(builder, inner),
                ResolvedTypeKind::FixedTokenPresence
                | ResolvedTypeKind::Tuple(_)
                | ResolvedTypeKind::Unsupported => Some(false),
            }
        }
        repeated(self, &self.resolved_field(field).ty)
    }

    /// Derives Pretty lexical patterns, optionally retaining fixed-token languages.
    fn pretty_lexical_patterns(&self, include_fixed: bool) -> Vec<String> {
        let equivalence = self.resolved.configuration.syntax.text_equivalence;
        self.tokens
            .definitions
            .iter()
            .filter_map(|definition| match &definition.kind {
                TokenDefinitionKind::Fixed(fixed) => {
                    include_fixed.then(|| fixed_lexical_pattern(&fixed.spelling, equivalence))
                }
                TokenDefinitionKind::Content(content) => Some(content.pattern.clone()),
                TokenDefinitionKind::LexicalBase(base) => Some(base.pattern.clone()),
            })
            .chain(
                self.tokens
                    .lexer_records
                    .iter()
                    .map(|record| record.pattern.clone()),
            )
            .collect()
    }

    /// Reuses compiled languages and product answers for one Pretty-boundary proof.
    fn pretty_patterns_cross(&self, left: &str, right: &str, lexical_pattern: &str) -> bool {
        self.pretty_boundary_prover
            .borrow_mut()
            .patterns_cross(left, right, lexical_pattern)
    }

    /// Reuses one exact product against an immutable Pretty lexical pattern catalog.
    fn pretty_catalog_patterns_cross(
        &self,
        left: &str,
        right: &str,
        catalog: PrettyPatternCatalogId,
    ) -> bool {
        self.pretty_boundary_prover
            .borrow_mut()
            .patterns_cross_catalog(left, right, catalog)
    }

    /// Diagnoses a Pretty boundary that would change token recognition.
    fn validate_pretty_boundary(
        &self,
        left: &PrettyBoundary,
        right: &PrettyBoundary,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        // Check the closed Pretty policy matrix explicitly; implicit fallthrough would accept unsupported formatting states.
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
        let explicitly_tight = matches!(
            left_fixed.map(|fixed| fixed.pretty_spacing),
            Some(PrettySpacing::Tight | PrettySpacing::Before)
        ) || matches!(
            right_fixed.map(|fixed| fixed.pretty_spacing),
            Some(PrettySpacing::Tight | PrettySpacing::After)
        );
        let left_auto_punctuation = left_fixed.is_some_and(|fixed| {
            fixed.pretty_spacing == PrettySpacing::Auto
                && !is_word_shaped(&fixed.spelling)
                && !matches!(fixed.spelling.as_str(), "(" | "[" | "{")
        });
        let left_auto_open = left_fixed.is_some_and(|fixed| {
            fixed.pretty_spacing == PrettySpacing::Auto
                && matches!(fixed.spelling.as_str(), "(" | "[" | "{")
        });
        let right_auto_punctuation = right_fixed.is_some_and(|fixed| {
            fixed.pretty_spacing == PrettySpacing::Auto && !is_word_shaped(&fixed.spelling)
        });
        let runtime_auto_tight =
            left_auto_open || (right_auto_punctuation && !left_auto_punctuation);
        let known = |boundary: &PrettyBoundary| !matches!(&boundary.kind, PrettyBoundaryKind::Dynamic(pattern) if pattern.is_empty());
        if known(left)
            && known(right)
            && (left_requests_space
                || right_requests_space
                || (!explicitly_tight && !runtime_auto_tight))
            && !self.pretty_trivia_supports_breaks()
        {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::UnsafePrettyHint,
                sites: vec![left.site.clone(), right.site.clone()],
                message: "Pretty spacing would insert a width-dependent break without ignored lexer trivia accepting spaces and newlines"
                    .to_owned(),
            });
            return;
        }
        if left_requests_space || right_requests_space {
            return;
        }
        if !explicitly_tight && !runtime_auto_tight {
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
                if pattern.is_empty() {
                    return;
                }
                let right_fixed = right_fixed.expect("fixed boundary");
                let right_pattern = fixed_pretty_pattern(
                    right_fixed,
                    self.resolved.configuration.syntax.text_equivalence,
                );
                if self.pretty_catalog_patterns_cross(
                    pattern,
                    &right_pattern,
                    self.pretty_lexical_catalog
                        .expect("Pretty validation initialized its lexical catalog"),
                ) {
                    diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidConfiguration,
                            sites: vec![left.site.clone(), right.site.clone()],
                            message: "Pretty spacing would create an unproved lexer boundary involving regex-backed content".to_owned(),
                        });
                }
            }
            (PrettyBoundaryKind::Fixed(_), PrettyBoundaryKind::Dynamic(pattern)) => {
                if pattern.is_empty() {
                    return;
                }
                let left_fixed = left_fixed.expect("fixed boundary");
                let left_pattern = fixed_pretty_pattern(
                    left_fixed,
                    self.resolved.configuration.syntax.text_equivalence,
                );
                if self.pretty_catalog_patterns_cross(
                    &left_pattern,
                    pattern,
                    self.pretty_lexical_catalog
                        .expect("Pretty validation initialized its lexical catalog"),
                ) {
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

    /// Validates pretty fixed boundary.
    fn validate_pretty_fixed_boundary(
        &self,
        left_fixed: &FixedTokenDefinition,
        left_site: &SourceSite,
        right_fixed: &FixedTokenDefinition,
        right_site: &SourceSite,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
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

        let equivalence = self.resolved.configuration.syntax.text_equivalence;
        let left_pattern = fixed_pretty_pattern(left_fixed, equivalence);
        let right_pattern = fixed_pretty_pattern(right_fixed, equivalence);
        let longer = self.tokens.definitions.iter().find_map(|definition| {
            let TokenDefinitionKind::Fixed(candidate) = &definition.kind else {
                return None;
            };
            fixed_spellings_cross(
                &left_fixed.spelling,
                &right_fixed.spelling,
                &candidate.spelling,
                equivalence,
            )
            .then_some(candidate)
        });
        if let Some(longer) = longer {
            diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidConfiguration,
                    sites: vec![left_site.clone(), right_site.clone()],
                    message: format!(
                        "Pretty spacing would suppress a lexer-required boundary between `{}` and `{}`; longest match `{}` would change tokenization",
                        left_fixed.spelling, right_fixed.spelling, longer.spelling,
                    ),
                });
            return;
        }

        let lexical_catalog = self
            .pretty_non_fixed_lexical_catalog
            .expect("Pretty validation initialized its non-fixed lexical catalog");
        let pattern = self
            .pretty_catalog_patterns_cross(&left_pattern, &right_pattern, lexical_catalog)
            .then(|| {
                self.pretty_lexical_patterns(false)
                    .into_iter()
                    .find(|pattern| {
                        self.pretty_patterns_cross(&left_pattern, &right_pattern, pattern)
                    })
            })
            .flatten();
        if let Some(pattern) = pattern {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidConfiguration,
                sites: vec![left_site.clone(), right_site.clone()],
                message: format!(
                        "Pretty spacing would suppress a lexer-required boundary between `{}` and `{}`; lexical pattern `{pattern}` would change tokenization",
                    left_fixed.spelling, right_fixed.spelling,
                ),
            });
        }
    }

    /// Derives the fixed definition by id from the retained source model and its correlations.
    fn fixed_definition_by_id(&self, id: TokenDefinitionId) -> &FixedTokenDefinition {
        let TokenDefinitionKind::Fixed(fixed) = &self.tokens.definitions[id.index()].kind else {
            unreachable!("fixed boundary points to fixed syntax")
        };
        fixed
    }

    /// Derives the fixed definition for use from the retained source model and its correlations.
    fn fixed_definition_for_use(&self, id: TokenUseId) -> &FixedTokenDefinition {
        self.fixed_definition_by_id(self.tokens.uses[id.index()].definition)
    }

    /// Derives the pretty boundary emits soft break from the retained source model and its correlations.
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

    /// Derives the pretty boundary spacing from the retained source model and its correlations.
    fn pretty_boundary_spacing(&self, boundary: &PrettyBoundary) -> Option<PrettySpacing> {
        match boundary.kind {
            PrettyBoundaryKind::Fixed(id) => Some(self.fixed_definition_by_id(id).pretty_spacing),
            PrettyBoundaryKind::Dynamic(_) => None,
        }
    }

    /// Derives the pretty boundary forces tight after from the retained source model and its correlations.
    fn pretty_boundary_forces_tight_after(&self, boundary: &PrettyBoundary) -> bool {
        matches!(
            self.pretty_boundary_spacing(boundary),
            Some(PrettySpacing::Tight | PrettySpacing::Before)
        )
    }

    /// Derives the pretty boundary forces tight before from the retained source model and its correlations.
    fn pretty_boundary_forces_tight_before(&self, boundary: &PrettyBoundary) -> bool {
        matches!(
            self.pretty_boundary_spacing(boundary),
            Some(PrettySpacing::Tight | PrettySpacing::After)
        )
    }

    /// Derives the pretty element boundaries from the retained source model and its correlations.
    fn pretty_element_boundaries(
        &self,
        element: &GrammarElement,
        side: PrettyBoundarySide,
        visiting: &mut Vec<NodeId>,
    ) -> Vec<PrettyBoundary> {
        let mut cycle_dependent = false;
        let caller = match element {
            GrammarElement::FixedToken { site, .. }
            | GrammarElement::OptionalFixedTokenGroup { site, .. }
            | GrammarElement::SemanticField { site, .. } => site,
        };
        self.pretty_element_boundary_templates(element, side, visiting, &mut cycle_dependent)
            .iter()
            .map(|boundary| boundary.materialize(caller))
            .collect()
    }

    /// Derives element boundaries while reporting whether recursion supplied a fallback boundary.
    fn pretty_element_boundary_templates(
        &self,
        element: &GrammarElement,
        side: PrettyBoundarySide,
        visiting: &mut Vec<NodeId>,
        cycle_dependent: &mut bool,
    ) -> Vec<PrettyBoundaryTemplate> {
        match element {
            GrammarElement::FixedToken {
                token_use, site, ..
            } => vec![PrettyBoundaryTemplate::authored(
                PrettyBoundaryKind::Fixed(self.tokens.uses[token_use.index()].definition),
                site.clone(),
            )],
            GrammarElement::OptionalFixedTokenGroup { tokens, .. } => {
                let token = match side {
                    PrettyBoundarySide::First => tokens.first(),
                    PrettyBoundarySide::Last => tokens.last(),
                };
                token.map_or_else(Vec::new, |token| {
                    vec![PrettyBoundaryTemplate::authored(
                        PrettyBoundaryKind::Fixed(
                            self.tokens.uses[token.token_use.index()].definition,
                        ),
                        token.site.clone(),
                    )]
                })
            }
            GrammarElement::SemanticField {
                resolved_field,
                projection,
                attachment,
                site,
                ..
            } => {
                let attached = attachment.as_ref().and_then(|attachment| match side {
                    PrettyBoundarySide::First => attachment
                        .before
                        .first()
                        .and_then(AttachedFixedSyntax::first),
                    PrettyBoundarySide::Last => {
                        attachment.after.last().and_then(AttachedFixedSyntax::last)
                    }
                });
                if let Some(attached) = attached {
                    vec![PrettyBoundaryTemplate::authored(
                        PrettyBoundaryKind::Fixed(
                            self.tokens.uses[attached.token_use.index()].definition,
                        ),
                        attached.site.clone(),
                    )]
                } else {
                    match projection {
                        SemanticFieldProjection::Direct => self
                            .pretty_type_boundary_templates(
                                &self.resolved_field(*resolved_field).ty,
                                side,
                                visiting,
                                cycle_dependent,
                            )
                            .iter()
                            .map(|boundary| boundary.bind(site))
                            .collect(),
                        SemanticFieldProjection::FixedPresence { tokens } => {
                            let token = match side {
                                PrettyBoundarySide::First => tokens.first(),
                                PrettyBoundarySide::Last => tokens.last(),
                            };
                            token.map_or_else(Vec::new, |token| {
                                vec![PrettyBoundaryTemplate::authored(
                                    PrettyBoundaryKind::Fixed(
                                        self.tokens.uses[token.token_use.index()].definition,
                                    ),
                                    token.site.clone(),
                                )]
                            })
                        }
                    }
                }
            }
        }
    }

    /// Derives the pretty type boundaries from the retained source model and its correlations.
    fn pretty_type_boundaries(
        &self,
        ty: &ResolvedType,
        side: PrettyBoundarySide,
        site: &SourceSite,
        visiting: &mut Vec<NodeId>,
    ) -> Vec<PrettyBoundary> {
        let mut cycle_dependent = false;
        self.pretty_type_boundary_templates(ty, side, visiting, &mut cycle_dependent)
            .iter()
            .map(|boundary| boundary.materialize(site))
            .collect()
    }

    /// Derives type boundaries while reporting whether recursion supplied a fallback boundary.
    fn pretty_type_boundary_templates(
        &self,
        ty: &ResolvedType,
        side: PrettyBoundarySide,
        visiting: &mut Vec<NodeId>,
        cycle_dependent: &mut bool,
    ) -> Vec<PrettyBoundaryTemplate> {
        match &ty.kind {
            ResolvedTypeKind::Semantic {
                target, arguments, ..
            } => match target {
                SemanticTarget::Node(node) => {
                    self.pretty_node_boundary_templates(*node, side, visiting, cycle_dependent)
                }
                SemanticTarget::ContentToken(content) => {
                    let pattern = self
                        .pretty_content_definitions
                        .get(content.index())
                        .and_then(|definition| *definition)
                        .and_then(|definition| {
                            match &self.tokens.definitions[definition.index()].kind {
                                TokenDefinitionKind::Content(content) => {
                                    Some(content.pattern.clone())
                                }
                                TokenDefinitionKind::Fixed(_)
                                | TokenDefinitionKind::LexicalBase(_) => None,
                            }
                        })
                        .unwrap_or_default();
                    vec![PrettyBoundaryTemplate::caller(PrettyBoundaryKind::Dynamic(
                        pattern,
                    ))]
                }
                SemanticTarget::Catalog(_) => arguments
                    .iter()
                    .find_map(|argument| match argument {
                        ResolvedGenericArgument::Type(inner) => {
                            Some(self.pretty_type_boundary_templates(
                                inner,
                                side,
                                visiting,
                                cycle_dependent,
                            ))
                        }
                        ResolvedGenericArgument::Lifetime(_)
                        | ResolvedGenericArgument::Unsupported(_) => None,
                    })
                    .unwrap_or_default(),
                SemanticTarget::TypeAlias(alias) => self
                    .resolved
                    .type_aliases
                    .get(alias.index())
                    .filter(|candidate| candidate.id == *alias)
                    .and_then(|alias| alias.target.as_ref())
                    .map(|target| {
                        self.pretty_type_boundary_templates(target, side, visiting, cycle_dependent)
                    })
                    .unwrap_or_default(),
                SemanticTarget::GenericParameter(_) => vec![PrettyBoundaryTemplate::caller(
                    PrettyBoundaryKind::Dynamic(String::new()),
                )],
            },
            ResolvedTypeKind::Group(inner)
            | ResolvedTypeKind::Parenthesized(inner)
            | ResolvedTypeKind::Reference(inner) => {
                self.pretty_type_boundary_templates(inner, side, visiting, cycle_dependent)
            }
            ResolvedTypeKind::FixedTokenPresence
            | ResolvedTypeKind::Tuple(_)
            | ResolvedTypeKind::Unsupported => vec![PrettyBoundaryTemplate::caller(
                PrettyBoundaryKind::Dynamic(String::new()),
            )],
        }
    }

    /// Derives the pretty node boundaries from the retained source model and its correlations.
    fn pretty_node_boundary_templates(
        &self,
        node: NodeId,
        side: PrettyBoundarySide,
        visiting: &mut Vec<NodeId>,
        cycle_dependent: &mut bool,
    ) -> Vec<PrettyBoundaryTemplate> {
        if visiting.contains(&node) {
            *cycle_dependent = true;
            return vec![PrettyBoundaryTemplate::caller(PrettyBoundaryKind::Dynamic(
                String::new(),
            ))];
        }
        let key = (node, side);
        if let Some(boundaries) = self.pretty_boundary_analysis.borrow().nodes.get(&key) {
            return boundaries.clone();
        }
        #[cfg(test)]
        {
            let mut analysis = self.pretty_boundary_analysis.borrow_mut();
            analysis.node_evaluations = analysis.node_evaluations.saturating_add(1);
        }
        visiting.push(node);
        let Some(node) = self
            .nodes
            .get(node.index())
            .filter(|candidate| candidate.resolved_node == node)
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
        let mut seen = HashSet::new();
        let mut node_cycle_dependent = false;
        for elements in sequences {
            let element = match side {
                PrettyBoundarySide::First => elements.first(),
                PrettyBoundarySide::Last => elements.last(),
            };
            if let Some(element) = element {
                for boundary in self.pretty_element_boundary_templates(
                    element,
                    side,
                    visiting,
                    &mut node_cycle_dependent,
                ) {
                    if seen.insert(boundary.clone()) {
                        boundaries.push(boundary);
                    }
                }
            }
        }
        visiting.pop();
        *cycle_dependent |= node_cycle_dependent;
        if !node_cycle_dependent {
            self.pretty_boundary_analysis
                .borrow_mut()
                .nodes
                .insert(key, boundaries.clone());
        }
        boundaries
    }

    /// Derives the pretty fixed element from the retained source model and its correlations.
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

    /// Lowers central content definitions.
    fn lower_central_content_definitions(&mut self) {
        // A central `matchers { .. }`/`content { .. }` entry has no enclosing field to anchor a
        // per-declaration origin the way `lower_content_definition` does, so this uses the same
        // root-module origin `TraitPlanner::from_grammar` computes for grammar-wide diagnostics.
        let origin = SourceOrigin {
            module: self
                .resolved
                .tree
                .inclusions
                .first()
                .expect("a resolved grammar has a root module inclusion")
                .id,
            source: self.resolved.configuration.source,
        };
        for definition in std::mem::take(&mut self.tokens.central_content) {
            if let Some(semantic) = definition.semantic {
                // A `#[node(derive(...), omit(...))]` attribute placed before this entry's name
                // (see `CentralTokensSyntax`'s `matchers`/`literals`/`lexer_tokens` parsing)
                // overrides this specific content token's configured traits, reusing `node_plan`
                // exactly like a `#[lex(...)]`-tagged field does in `lower_content_definition`.
                let configured_traits = self.trait_planner.node_plan(
                    &self.resolved,
                    origin,
                    &definition.metas,
                    false,
                    &mut self.diagnostics,
                );
                self.tokens.define_content(
                    semantic,
                    definition.name,
                    definition.pattern,
                    definition.closed_matcher,
                    None,
                    definition.priority,
                    Vec::new(),
                    AuthoredAdmissions {
                        site: definition.site.clone(),
                        definitions: Vec::new(),
                    },
                    definition.site,
                    configured_traits,
                );
            } else {
                self.tokens.define_lexical_base(definition);
            }
        }
    }

    /// Publishes the source model only when all authored-source diagnostics are non-blocking.
    fn finish(mut self) -> SourceModelReport {
        #[cfg(test)]
        let pretty_boundary_node_evaluations =
            self.pretty_boundary_analysis.borrow().node_evaluations;
        #[cfg(test)]
        let pretty_soft_node_evaluations =
            self.pretty_boundary_analysis.borrow().soft_node_evaluations;
        normalize_source_order(&mut self.nodes, &mut self.tokens);
        self.tokens.compile_lookahead_filters(&mut self.diagnostics);
        self.resolve_precedence();
        self.resolve_lr_conflict_resolutions();
        self.resolve_restrictions();
        let source_modeled = self
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.kind.is_error())
            .then(|| SourceModeledGrammar {
                resolved: self.resolved,
                source_model: SourceModel {
                    grammar_trait_defaults: self.trait_planner.grammar_defaults,
                    nodes: self.nodes,
                    token_definitions: self.tokens.definitions,
                    lexer_records: self.tokens.lexer_records,
                    token_uses: self.tokens.uses,
                    lookahead_filters: self.tokens.lookahead_filters,
                    precedence: self.precedence,
                    precedence_overrides: self.precedence_overrides,
                    lr_conflict_resolutions: self.lr_conflict_resolutions,
                    restrictions: self.restrictions,
                },
            });
        SourceModelReport {
            diagnostics: self.diagnostics,
            source_modeled,
            #[cfg(test)]
            pretty_boundary_node_evaluations,
            #[cfg(test)]
            pretty_soft_node_evaluations,
        }
    }

    /// Validates every `#[restricts(Base)]` declaration against its resolved base and records
    /// the ones a grammar can lower (`docs/declared-precedence.md`, "Restricted expressions").
    ///
    /// A restricted expression is a grammar nonterminal, never an AST type: the declaration
    /// expanded to a type alias for its base, so this pass reads the authored body back off the
    /// alias and checks the four things the alias cannot express — that the declaration is an
    /// enum of bare names, that the base is an enum Node, that the base chain is acyclic, and
    /// that every admitted name is a variant the base itself admits — plus that the declaration
    /// carries no configuration only a type can carry.
    ///
    /// The admitted names are checked against the *immediate* base, which for a restriction of
    /// a restriction is that restriction's own admitted set, and are then resolved against the
    /// root base Node, whose type every rule constructs.
    fn resolve_restrictions(&mut self) {
        self.reject_restricted_framing();
        let mut restrictions = Vec::new();
        for index in 0..self.resolved.type_aliases.len() {
            let alias_id = self.resolved.type_aliases[index].id;
            let Some(authored) = self.resolved.type_aliases[index].restriction.clone() else {
                continue;
            };
            let origin = SourceOrigin {
                module: self.resolved.type_aliases[index].module,
                source: self.resolved.type_aliases[index].source,
            };
            let site = self.site(origin, authored.range.clone());
            for (path, range) in &authored.configured {
                self.diagnostics
                    .push(SourceModelDiagnosticKind::RestrictedExpressionAsType.at(
                        self.site(origin, range.clone()),
                        format!(
                            "#[{path}] configures a type, and a restricted expression is a \
                             grammar nonterminal that generates none",
                        ),
                    ));
            }
            if !authored.enumerated {
                self.diagnostics
                    .push(SourceModelDiagnosticKind::RestrictedVariantHasFields.at(
                        site.clone(),
                        "a restricted expression is an enum whose variants are the bare names of \
                         the base variants it admits",
                    ));
                continue;
            }
            let mut malformed = false;
            for variant in &authored.variants {
                if variant.has_fields {
                    malformed = true;
                    self.diagnostics.push(
                        SourceModelDiagnosticKind::RestrictedVariantHasFields.at(
                            self.site(origin, variant.range.clone()),
                            format!(
                                "`{}` names a base variant, so it takes no fields: a restricted \
                                 expression admits the base's own rule unchanged",
                                variant.name,
                            ),
                        ),
                    );
                }
            }
            let Some((base, root)) = self.restriction_base(alias_id, &site, &authored) else {
                continue;
            };
            if malformed {
                continue;
            }
            let admitted = self.admitted_variant_names(base);
            let mut names = Vec::new();
            for variant in &authored.variants {
                if admitted.contains(&variant.name) {
                    names.push(variant.name.clone());
                    continue;
                }
                malformed = true;
                self.diagnostics
                    .push(SourceModelDiagnosticKind::RestrictedVariantUnknown.at(
                        self.site(origin, variant.range.clone()),
                        format!(
                            "`{}` is no variant of `{}`; a restricted expression admits only \
                             variants its base admits",
                            variant.name,
                            self.restriction_base_name(base),
                        ),
                    ));
            }
            if malformed {
                continue;
            }
            let variants = self.root_variant_order(root, &names);
            restrictions.push(SourceRestriction {
                alias: alias_id,
                name: self.resolved.type_aliases[index].ident.clone(),
                base: root,
                variants,
                site,
            });
        }
        self.restrictions = restrictions;
    }

    /// Rejects a `framing` island or payload owner that names a restricted expression: framing
    /// parses each island as a type, and a restriction generates none.
    fn reject_restricted_framing(&mut self) {
        let Some(framing) = self.resolved.configuration.framing.clone() else {
            return;
        };
        let owners = std::iter::once(&framing.island)
            .chain(framing.payload.as_ref().map(|payload| &payload.after));
        let named: Vec<_> = owners
            .filter_map(|ty| match &ty.kind {
                ResolvedTypeKind::Semantic {
                    target: SemanticTarget::TypeAlias(alias),
                    ..
                } if self.resolved.type_aliases[alias.index()]
                    .restriction
                    .is_some() =>
                {
                    Some((
                        ty.range.clone(),
                        self.resolved.type_aliases[alias.index()].ident.clone(),
                    ))
                }
                _ => None,
            })
            .collect();
        for (range, name) in named {
            let origin = SourceOrigin {
                module: self
                    .resolved
                    .tree
                    .inclusions
                    .first()
                    .expect("a resolved grammar has a root module inclusion")
                    .id,
                source: self.resolved.configuration.source,
            };
            let site = self.site(origin, range);
            self.diagnostics
                .push(SourceModelDiagnosticKind::RestrictedExpressionAsType.at(
                    site,
                    format!(
                        "`{name}` is a restricted expression, a grammar nonterminal with no \
                         generated type, and `framing` needs a Node type here",
                    ),
                ));
        }
    }

    /// Resolves one restriction's base: the Node or restriction its alias target names, with a
    /// base that is not an enum Node and a base cycle each reported at its own code.
    fn restriction_base(
        &mut self,
        alias: TypeAliasId,
        site: &SourceSite,
        authored: &crate::declaration::DeclarationRestriction,
    ) -> Option<(RestrictionBase, NodeId)> {
        let origin = SourceOrigin {
            module: self.resolved.type_aliases[alias.index()].module,
            source: self.resolved.type_aliases[alias.index()].source,
        };
        let base_site = self.site(origin, authored.base_range.clone());
        let mut current = alias;
        let mut visited = BTreeSet::from([alias]);
        // The chain is walked all the way to its root Node even once the immediate base is
        // known, because only the root proves the chain terminates and is an enum.
        let mut immediate = None;
        loop {
            let Some(target) = self.resolved.type_aliases[current.index()].target.clone() else {
                self.diagnostics
                    .push(SourceModelDiagnosticKind::RestrictedBaseNotEnum.at(
                        base_site,
                        "#[restricts(Base)] names one enum Node, the expression this Node \
                         restricts",
                    ));
                return None;
            };
            let ResolvedTypeKind::Semantic { target, .. } = &target.kind else {
                self.diagnostics
                    .push(SourceModelDiagnosticKind::RestrictedBaseNotEnum.at(
                        base_site,
                        "#[restricts(Base)] names one enum Node, not a container or an \
                         unresolved type",
                    ));
                return None;
            };
            match *target {
                SemanticTarget::Node(node) => {
                    if !self.node_is_plain_enum(node) {
                        self.diagnostics
                            .push(SourceModelDiagnosticKind::RestrictedBaseNotEnum.at(
                                base_site,
                                format!(
                                    "`{}` is not an enum Node, so it has no variants to admit",
                                    self.resolved.nodes[node.index()].ident,
                                ),
                            ));
                        return None;
                    }
                    return Some((immediate.unwrap_or(RestrictionBase::Node(node)), node));
                }
                SemanticTarget::TypeAlias(next) => {
                    if !visited.insert(next) {
                        self.diagnostics
                            .push(SourceModelDiagnosticKind::RestrictedBaseCycle.at(
                                site.clone(),
                                "a restricted expression cannot restrict itself, directly or \
                                 around a cycle of restrictions",
                            ));
                        return None;
                    }
                    if immediate.is_none()
                        && self.resolved.type_aliases[next.index()]
                            .restriction
                            .is_some()
                    {
                        immediate = Some(RestrictionBase::Restriction(next));
                    }
                    current = next;
                }
                _ => {
                    self.diagnostics
                        .push(SourceModelDiagnosticKind::RestrictedBaseNotEnum.at(
                            base_site,
                            "#[restricts(Base)] names one enum Node, not a token or a generic \
                             parameter",
                        ));
                    return None;
                }
            }
        }
    }

    /// Reports whether one Node is an ordinary enum, the only base a restriction admits.
    fn node_is_plain_enum(&self, node: NodeId) -> bool {
        self.nodes
            .iter()
            .find(|candidate| candidate.resolved_node == node)
            .is_some_and(|candidate| matches!(candidate.body, NodeBody::Enum(_)))
    }

    /// The variant names one restriction base admits: an enum Node's own variants, or the
    /// admitted subset of a base that is itself a restriction.
    fn admitted_variant_names(&self, base: RestrictionBase) -> Vec<String> {
        match base {
            RestrictionBase::Node(node) => self.node_variant_names(node),
            // A base that is itself a restriction admits exactly the names it lists, read from
            // the authored declaration so that the order restrictions are declared in cannot
            // change what a restriction of a restriction admits.
            RestrictionBase::Restriction(alias) => self.resolved.type_aliases[alias.index()]
                .restriction
                .as_ref()
                .map(|restriction| {
                    restriction
                        .variants
                        .iter()
                        .map(|variant| variant.name.clone())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The enum variant names of one Node, in declaration order.
    fn node_variant_names(&self, node: NodeId) -> Vec<String> {
        match &self.resolved.nodes[node.index()].body {
            ResolvedNodeBody::Enum { variants } => variants
                .iter()
                .map(|variant| variant.ident.clone())
                .collect(),
            ResolvedNodeBody::Struct { .. } => Vec::new(),
        }
    }

    /// The display name of one restriction base, for a diagnostic message.
    fn restriction_base_name(&self, base: RestrictionBase) -> String {
        match base {
            RestrictionBase::Node(node) => self.resolved.nodes[node.index()].ident.clone(),
            RestrictionBase::Restriction(alias) => {
                self.resolved.type_aliases[alias.index()].ident.clone()
            }
        }
    }

    /// Orders admitted names by the root base's own declaration order and resolves each to its
    /// variant identity.
    fn root_variant_order(&self, root: NodeId, names: &[String]) -> Vec<ResolvedVariantId> {
        match &self.resolved.nodes[root.index()].body {
            ResolvedNodeBody::Enum { variants } => variants
                .iter()
                .filter(|variant| names.contains(&variant.ident))
                .map(|variant| variant.id)
                .collect(),
            ResolvedNodeBody::Struct { .. } => Vec::new(),
        }
    }

    /// Rejects misplaced content declarations.
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

    /// Lowers resolved Nodes in authored order while accumulating independent diagnostics.
    fn lower_nodes(&mut self) {
        // Allocate Node identities before lowering bodies so recursive references resolve without source-order dependence.
        for node in self.resolved.nodes.clone() {
            let origin = SourceOrigin {
                module: node.module,
                source: node.source,
            };
            let configured = &node.metas;
            reject_obsolete_parser_postconditions(
                &self.resolved,
                origin,
                configured,
                &mut self.diagnostics,
            );
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
                                    reject_obsolete_parser_postconditions(
                                        &self.resolved,
                                        origin,
                                        &variant.metas,
                                        &mut self.diagnostics,
                                    );
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
                                    self.lower_greedy(origin, &variant.metas);
                                    self.lower_precedence_override(
                                        origin,
                                        &variant.metas,
                                        PrecedenceOwner::Variant(variant.id),
                                        !matches!(position, SourcePrattPosition::Atom),
                                    );
                                    self.lower_lr_conflict_resolutions(
                                        origin,
                                        &variant.metas,
                                        LrRuleOwner::Variant(variant.id),
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
                                    reject_obsolete_parser_postconditions(
                                        &self.resolved,
                                        origin,
                                        &variant.metas,
                                        &mut self.diagnostics,
                                    );
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
                                    self.lower_greedy(origin, &variant.metas);
                                    self.lower_precedence_override(
                                        origin,
                                        &variant.metas,
                                        PrecedenceOwner::Variant(variant.id),
                                        false,
                                    );
                                    self.lower_lr_conflict_resolutions(
                                        origin,
                                        &variant.metas,
                                        LrRuleOwner::Variant(variant.id),
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
                true,
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
            self.lower_greedy(origin, configured);
            self.lower_precedence_override(
                origin,
                configured,
                PrecedenceOwner::Type(node.id),
                false,
            );
            self.lower_lr_conflict_resolutions(origin, configured, LrRuleOwner::Type(node.id));
            self.nodes.push(Node {
                resolved_node: node.id,
                name: node.ident,
                body,
                configured_traits,
                pretty,
            });
        }
    }

    /// Rejects the removed `#[greedy(...)]` helper at its authored location.
    ///
    /// The derive macro continues to admit the helper attribute only so this source-aware
    /// diagnostic wins over Rust's unknown-attribute error.
    fn lower_greedy(&mut self, origin: SourceOrigin, metas: &[Meta]) {
        for meta in metas.iter().filter(|meta| meta.path().is_ident("greedy")) {
            let site = self.site(origin, span_range(meta.span()));
            self.diagnostics
                .push(SourceModelDiagnosticKind::InvalidGreedyAcceptance.at(
                    site,
                    "#[greedy(...)] has been removed; make the grammar LALR(1) or resolve its LR conflict explicitly",
                ));
        }
    }

    /// Lowers and validates one authored LR precedence override.
    fn lower_precedence_override(
        &mut self,
        origin: SourceOrigin,
        metas: &[Meta],
        owner: PrecedenceOwner,
        operator: bool,
    ) {
        let override_metas = metas
            .iter()
            .filter(|meta| is_precedence_override_meta(meta))
            .collect::<Vec<_>>();
        let Some(meta) = override_metas.first() else {
            return;
        };
        let site = self.site(origin, span_range(meta.span()));
        if override_metas.len() > 1 {
            self.diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrecedenceDeclaration,
                sites: override_metas
                    .iter()
                    .map(|meta| self.site(origin, span_range(meta.span())))
                    .collect(),
                message: "a Node or variant accepts at most one #[parse(prec = NAME)]".to_owned(),
            });
            return;
        }
        if operator {
            self.diagnostics
                .push(SourceModelDiagnosticKind::InvalidPrecedenceDeclaration.at(
                    site,
                    "#[parse(prec = NAME)] is not accepted on a prefix, infix or postfix Pratt \
                     variant, whose binding powers are its precedence",
                ));
            return;
        }
        let Meta::List(list) = meta else {
            unreachable!("`is_precedence_override_meta` matches list attributes only")
        };
        let parser = |input: ParseStream<'_>| -> syn::Result<syn::Ident> {
            let prec = syn::Ident::parse_any(input)?;
            debug_assert_eq!(prec, "prec");
            input.parse::<Token![=]>()?;
            let name = syn::Ident::parse_any(input)?;
            if !input.is_empty() {
                return Err(input.error("unexpected tokens after the precedence level name"));
            }
            Ok(name)
        };
        match parser.parse2(list.tokens.clone()) {
            Ok(name) => self
                .pending_precedence_overrides
                .push(PendingPrecedenceOverride {
                    owner,
                    name: name.unraw().to_string(),
                    site,
                }),
            Err(error) => {
                self.diagnostics
                    .push(SourceModelDiagnosticKind::InvalidPrecedenceDeclaration.at(
                        self.site(origin, span_range(error.span())),
                        format!(
                            "#[parse(prec = NAME)] names one declared precedence level: {error}"
                        ),
                    ))
            }
        }
    }

    /// Compiles the `precedence { ... }` declaration against the completed, normalized token
    /// catalog and resolves every `#[parse(prec = NAME)]` against it.
    ///
    /// A name resolves as a fixed token by its definition identifier or a content token by its
    /// type name; failing both, a `lookahead { ... }` filter's merged kind by its declared name,
    /// which gram.y's block names too (`NOT_LA` beside `BETWEEN`). A name that is none of the
    /// three is a precedence-only name, bison's `UMINUS`, and must be used by at least one
    /// override so a misspelt token is still an error. A second block, an unused precedence-only
    /// name, and an override naming no level are `RCA3133`.
    ///
    /// Every name a level carries, a merged kind's included, also names that level for
    /// `#[parse(prec = NAME)]`, as gram.y writes `%prec NOT_LA` on the rules that shift the
    /// merged kind (`gram.y:14872`).
    fn resolve_precedence(&mut self) {
        let mut blocks = std::mem::take(&mut self.tokens.pending_precedence).into_iter();
        let overrides = std::mem::take(&mut self.pending_precedence_overrides);
        let Some(block) = blocks.next() else {
            for pending in overrides {
                self.diagnostics
                    .push(SourceModelDiagnosticKind::InvalidPrecedenceDeclaration.at(
                        pending.site,
                        format!(
                            "#[parse(prec = {})] needs a `precedence {{ ... }}` declaration, and \
                             the grammar has none",
                            pending.name
                        ),
                    ));
            }
            return;
        };
        for extra in blocks {
            self.diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrecedenceDeclaration,
                sites: vec![block.site.clone(), extra.site],
                message: "a grammar declares at most one `precedence { ... }` block".to_owned(),
            });
        }
        let mut levels = Vec::new();
        let mut level_of_name = BTreeMap::<String, crate::PrecedenceLevelId>::new();
        for (index, level) in block.levels.into_iter().enumerate() {
            let mut kinds = Vec::new();
            let mut names = Vec::new();
            for reference in level.names {
                let matches = self
                    .tokens
                    .definitions
                    .iter()
                    .filter(|definition| {
                        definition.name == reference.name
                            && !matches!(definition.kind, TokenDefinitionKind::LexicalBase(_))
                    })
                    .map(|definition| PrecedenceKindReference::Token(definition.id))
                    .collect::<Vec<_>>();
                if matches.is_empty() {
                    // No token definition carries the name, so the name is a merged kind if a
                    // `lookahead` filter declares one, and a precedence-only name otherwise. A
                    // merged name can never be a token definition's: the filter is rejected
                    // where it is compiled if it is.
                    match self
                        .tokens
                        .lookahead_filters
                        .iter()
                        .position(|filter| filter.name == reference.name)
                    {
                        Some(filter) => kinds.push(PrecedenceKindReference::Merged(filter)),
                        None => names.push(reference.name.clone()),
                    }
                } else {
                    kinds.extend(matches);
                }
                let level = u32::try_from(index)
                    .expect("a precedence block declares fewer than u32 levels");
                level_of_name.insert(reference.name, crate::PrecedenceLevelId::new(level));
            }
            kinds.dedup();
            levels.push(PrecedenceLevel {
                associativity: level.associativity,
                binding_power: level.binding_power,
                site: level.site,
                kinds,
                names,
            });
        }
        let mut used = BTreeSet::<String>::new();
        for pending in overrides {
            match level_of_name.get(&pending.name) {
                Some(level) => {
                    used.insert(pending.name);
                    self.precedence_overrides.push(PrecedenceOverride {
                        owner: pending.owner,
                        level: *level,
                        site: pending.site,
                    });
                }
                None => self.diagnostics.push(
                    SourceModelDiagnosticKind::InvalidPrecedenceDeclaration.at(
                        pending.site,
                        format!(
                            "#[parse(prec = {})] names no level of the `precedence {{ ... }}` \
                             declaration",
                            pending.name
                        ),
                    ),
                ),
            }
        }
        for level in &levels {
            for name in &level.names {
                if !used.contains(name) {
                    self.diagnostics.push(
                        SourceModelDiagnosticKind::InvalidPrecedenceDeclaration.at(
                            level.site.clone(),
                            format!(
                                "`{name}` in the precedence declaration is not a fixed token, a \
                                 content token type or a lookahead filter's merged kind; a \
                                 precedence-only name must be used by a \
                                 `#[parse(prec = {name})]`"
                            ),
                        ),
                    );
                }
            }
        }
        self.precedence = levels;
    }

    /// Parses every `#[parse(lr_conflict(...))]` attached to a Node or variant.
    ///
    /// The declaration owner is implicit in the attachment site. `against` is a fully qualified
    /// grammar-source path to the opposing Node or variant, `lookahead` is a nonempty exact token
    /// set, and `expect` is a positive exact cell count. Resolution is consumed only by LR
    /// construction.
    fn lower_lr_conflict_resolutions(
        &mut self,
        origin: SourceOrigin,
        metas: &[Meta],
        owner: LrRuleOwner,
    ) {
        for meta in metas.iter().filter(|meta| is_lr_conflict_meta(meta)) {
            let Meta::List(list) = meta else {
                unreachable!("`is_lr_conflict_meta` matches list attributes only")
            };
            let site = self.site(origin, span_range(meta.span()));
            let parser = |input: ParseStream<'_>| -> syn::Result<_> {
                let marker = syn::Ident::parse_any(input)?;
                debug_assert_eq!(marker, "lr_conflict");
                let arguments;
                syn::parenthesized!(arguments in input);
                if !input.is_empty() {
                    return Err(input.error("unexpected tokens after `lr_conflict(...)`"));
                }
                let mut action = None;
                let mut against = None;
                let mut lookaheads = None;
                let mut expected = None;
                while !arguments.is_empty() {
                    let key = syn::Ident::parse_any(&arguments)?;
                    arguments.parse::<Token![=]>()?;
                    match key.to_string().as_str() {
                        "action" if action.is_none() => {
                            let value = syn::Ident::parse_any(&arguments)?;
                            action = Some(match value.to_string().as_str() {
                                "shift" => LrConflictAction::Shift,
                                "reduce" => LrConflictAction::Reduce,
                                "error" => LrConflictAction::Error,
                                _ => {
                                    return Err(syn::Error::new(
                                        value.span(),
                                        "LR conflict action must be `shift`, `reduce`, or `error`",
                                    ));
                                }
                            });
                        }
                        "against" if against.is_none() => against = Some(arguments.parse()?),
                        "lookahead" if lookaheads.is_none() => {
                            let members;
                            syn::braced!(members in arguments);
                            let names = syn::punctuated::Punctuated::<
                                syn::Ident,
                                Token![,],
                            >::parse_terminated(&members)?
                            .into_iter()
                            .collect::<Vec<_>>();
                            if names.is_empty() {
                                return Err(syn::Error::new(
                                    key.span(),
                                    "LR conflict lookahead set must not be empty",
                                ));
                            }
                            let mut seen = BTreeSet::new();
                            for name in &names {
                                if !seen.insert(name.unraw().to_string()) {
                                    return Err(syn::Error::new(
                                        name.span(),
                                        format!(
                                            "LR conflict lookahead `{name}` appears more than once"
                                        ),
                                    ));
                                }
                            }
                            lookaheads = Some(names);
                        }
                        "expect" if expected.is_none() => {
                            let literal: syn::LitInt = arguments.parse()?;
                            let count = literal.base10_parse::<u32>()?;
                            if count == 0 {
                                return Err(syn::Error::new(
                                    literal.span(),
                                    "LR conflict `expect` must be greater than zero",
                                ));
                            }
                            expected = Some(count);
                        }
                        "action" | "against" | "lookahead" | "expect" => {
                            return Err(syn::Error::new(
                                key.span(),
                                format!("LR conflict argument `{key}` is declared more than once"),
                            ));
                        }
                        _ => {
                            return Err(syn::Error::new(
                                key.span(),
                                "expected one of `action`, `against`, `lookahead`, or `expect`",
                            ));
                        }
                    }
                    if arguments.peek(Token![,]) {
                        arguments.parse::<Token![,]>()?;
                    } else if !arguments.is_empty() {
                        return Err(arguments.error("expected `,` between LR conflict arguments"));
                    }
                }
                Ok((
                    action
                        .ok_or_else(|| input.error("LR conflict declaration requires `action`"))?,
                    against
                        .ok_or_else(|| input.error("LR conflict declaration requires `against`"))?,
                    lookaheads.ok_or_else(|| {
                        input.error("LR conflict declaration requires `lookahead`")
                    })?,
                    expected
                        .ok_or_else(|| input.error("LR conflict declaration requires `expect`"))?,
                ))
            };
            match parser.parse2(list.tokens.clone()) {
                Ok((action, against, names, expected)) => {
                    self.pending_lr_conflict_resolutions
                        .push(PendingLrConflictResolution {
                            owner,
                            against,
                            action,
                            lookaheads: names
                                .into_iter()
                                .map(|name| AdmissionReference {
                                    name: name.unraw().to_string(),
                                    site: self.site(origin, span_range(name.span())),
                                })
                                .collect(),
                            expected,
                            site,
                        });
                }
                Err(error) => {
                    self.diagnostics
                        .push(SourceModelDiagnosticKind::InvalidLrConflictResolution.at(
                        self.site(origin, span_range(error.span())),
                        format!(
                            "#[parse(lr_conflict(...))] requires explicit action, opposing source \
                             identity, lookaheads, and expected count: {error}"
                        ),
                    ))
                }
            }
        }
    }

    /// Resolves LR conflict declarations against exact source identities and the completed token
    /// catalog. This rejects stale spelling before materialization; exact conflict-cell matching
    /// remains the LR analysis phase's responsibility.
    fn resolve_lr_conflict_resolutions(&mut self) {
        let pending = std::mem::take(&mut self.pending_lr_conflict_resolutions);
        for declaration in pending {
            let against = match self.resolve_lr_rule_owner(&declaration.against) {
                Ok(owner) => owner,
                Err(message) => {
                    self.diagnostics.push(
                        SourceModelDiagnosticKind::InvalidLrConflictResolution
                            .at(declaration.site, message),
                    );
                    continue;
                }
            };
            let mut lookaheads = Vec::new();
            let mut valid = true;
            for reference in declaration.lookaheads {
                let token_matches = self
                    .tokens
                    .definitions
                    .iter()
                    .filter(|definition| {
                        definition.name == reference.name
                            && !matches!(definition.kind, TokenDefinitionKind::LexicalBase(_))
                    })
                    .map(|definition| definition.id)
                    .collect::<Vec<_>>();
                let resolved = if token_matches.len() == 1 {
                    Some(LrLookaheadReference::Token(token_matches[0]))
                } else if token_matches.is_empty() {
                    self.tokens
                        .lookahead_filters
                        .iter()
                        .position(|filter| filter.name == reference.name)
                        .map(LrLookaheadReference::Merged)
                } else {
                    None
                };
                match resolved {
                    Some(resolved) => lookaheads.push(resolved),
                    None => {
                        valid = false;
                        self.diagnostics.push(
                            SourceModelDiagnosticKind::InvalidLrConflictResolution.at(
                                reference.site,
                                format!(
                                    "LR conflict lookahead `{}` does not name one unique token \
                                     kind or lookahead filter",
                                    reference.name
                                ),
                            ),
                        );
                    }
                }
            }
            if valid {
                self.lr_conflict_resolutions.push(LrConflictResolution {
                    owner: declaration.owner,
                    against,
                    action: declaration.action,
                    lookaheads,
                    expected: declaration.expected,
                    site: declaration.site,
                });
            }
        }
    }

    /// Resolves a root-anchored grammar source path to exactly one authored Node or variant.
    fn resolve_lr_rule_owner(&self, path: &syn::Path) -> Result<LrRuleOwner, String> {
        if path.leading_colon.is_some()
            || path
                .segments
                .iter()
                .any(|segment| !segment.arguments.is_empty())
        {
            return Err(
                "an LR conflict `against` path must be a plain root-anchored grammar path"
                    .to_owned(),
            );
        }
        let mut segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.unraw().to_string())
            .collect::<Vec<_>>();
        if segments.first().is_some_and(|segment| segment == "crate") {
            segments.remove(0);
        }
        let mut matches = Vec::new();
        for node in &self.resolved.nodes {
            if node.key.segments == segments {
                matches.push(LrRuleOwner::Type(node.id));
            }
            if segments.len() == node.key.segments.len() + 1
                && segments.starts_with(&node.key.segments)
            {
                let variant_name = &segments[segments.len() - 1];
                if let ResolvedNodeBody::Enum { variants } = &node.body {
                    matches.extend(
                        variants
                            .iter()
                            .filter(|variant| &variant.ident == variant_name)
                            .map(|variant| LrRuleOwner::Variant(variant.id)),
                    );
                }
            }
        }
        match matches.as_slice() {
            [owner] => Ok(*owner),
            [] => Err(format!(
                "LR conflict `against` path `{}` names no authored Node or variant",
                path_text(path)
            )),
            _ => Err(format!(
                "LR conflict `against` path `{}` is ambiguous",
                path_text(path)
            )),
        }
    }

    /// Converts resolved fields and positional tokens into source-model elements.
    fn lower_fields(&mut self, origin: SourceOrigin, fields: &[ResolvedFieldId]) -> LoweredFields {
        let mut body = LoweredFields::default();
        for field in fields {
            let resolved_field = self.resolved_field(*field).clone();
            let field = &resolved_field.syntax;
            let field_configured = &resolved_field.metas;
            let site = self.site(origin, span_range(field.span()));
            self.lower_greedy(origin, field_configured);
            reject_obsolete_parser_postconditions(
                &self.resolved,
                origin,
                field_configured,
                &mut self.diagnostics,
            );
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
            if matches!(resolved_field.ty.kind, ResolvedTypeKind::FixedTokenPresence) {
                let pretty =
                    self.lower_pretty_hints(origin, field_configured, resolved_field.range.clone());
                if let Some(presence) = PositionalTokenPlanner::lower_presence(
                    &self.resolved,
                    &mut self.tokens,
                    &mut self.diagnostics,
                    origin,
                    field_configured,
                    resolved_field.id,
                    field.ident.as_ref().map(|ident| ident.unraw().to_string()),
                    pretty,
                    site,
                ) {
                    let elements = PositionalTokenPlanner::lower(
                        &self.resolved,
                        &mut self.tokens,
                        &mut self.diagnostics,
                        origin,
                        field_configured,
                        vec![presence],
                        TokenAttachment::Field,
                    );
                    body.push_attachment(elements);
                }
                continue;
            }
            if let Some(meta) = field_configured
                .iter()
                .find(|meta| meta.path().is_ident("presence"))
            {
                self.diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidFixedTokenPresence,
                    sites: vec![self.site(origin, span_range(meta.span()))],
                    message: "#[presence(...)] is supported only on an unqualified bool field"
                        .to_owned(),
                });
            }
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
                projection: SemanticFieldProjection::Direct,
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
                pratt_extensions: lower_pratt_extension_filter(
                    &self.resolved,
                    origin,
                    field_configured,
                    &mut self.diagnostics,
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

    /// Derives the resolved field from the retained source model and its correlations.
    fn resolved_field(&self, id: ResolvedFieldId) -> &ResolvedNodeField {
        let &(node_index, field_index) = self
            .resolved_field_positions
            .get(id.index())
            .expect("resolved Node body owns every field identity");
        let field = &self.resolved.nodes[node_index].fields[field_index];
        debug_assert_eq!(field.id, id);
        field
    }

    /// Resolves inherited and local Pretty configuration for one Node.
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

    /// Derives the site from the retained source model and its correlations.
    fn site(&self, origin: SourceOrigin, range: Range<u32>) -> SourceSite {
        source_site(&self.resolved, origin, range)
    }
}

/// Carries the semantic facts retained from authored source structure.
struct SemanticFacts {
    /// Supplies the content token names used to correlate authored source with normalized grammar.
    content_token_names: BTreeMap<ContentTokenId, String>,
}

impl SemanticFacts {
    /// Indexes semantic token names from the resolved source grammar.
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

    /// Derives the target for field from the retained source model and its correlations.
    fn target_for_field(field: &ResolvedNodeField) -> Option<SemanticTarget> {
        let ResolvedTypeKind::Semantic { target, .. } = field.ty.kind else {
            return None;
        };
        Some(target)
    }

    /// Derives the content token from the retained source model and its correlations.
    fn content_token(&self, field: &ResolvedNodeField) -> Option<(ContentTokenId, String)> {
        let SemanticTarget::ContentToken(token) = Self::target_for_field(field)? else {
            return None;
        };
        Some((token, self.content_token_names.get(&token)?.clone()))
    }

    /// Derives the declared repetition from the retained source model and its correlations.
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
/// Carries the token catalog retained from authored source structure.
struct TokenCatalog {
    /// Supplies the definitions used to correlate authored source with normalized grammar.
    definitions: Vec<TokenDefinition>,
    /// Supplies the fixed by name used to correlate authored source with normalized grammar.
    fixed_by_name: BTreeMap<String, TokenDefinitionId>,
    /// Supplies the content by name used to correlate authored source with normalized grammar.
    content_by_name: BTreeMap<String, Vec<TokenDefinitionId>>,
    /// Supplies the central content used to correlate authored source with normalized grammar.
    central_content: Vec<CentralContentDefinition>,
    /// Supplies the lexer records used to correlate authored source with normalized grammar.
    lexer_records: Vec<NonTokenLexerRecord>,
    /// Supplies the uses used to correlate authored source with normalized grammar.
    uses: Vec<TokenUse>,
    /// Indexes declared keyword categories and their authored sites.
    categories: BTreeMap<String, Vec<SourceSite>>,
    /// Indexes declared keyword flags and their authored sites.
    flags: BTreeMap<String, Vec<SourceSite>>,
    /// Preserves named admission declarations until their dependency graph is compiled.
    pending_admissions: Vec<PendingAdmissionDefinition>,
    /// Indexes canonical compiled admission membership by authored name.
    named_admissions: BTreeMap<String, Vec<TokenDefinitionId>>,
    /// Preserves lookahead filter declarations until every fixed and content token is known.
    pending_lookaheads: Vec<PendingLookaheadDefinition>,
    /// Compiled lookahead filters in authored order.
    lookahead_filters: Vec<LookaheadFilter>,
    /// Preserves every `precedence { ... }` block until the token catalog is complete; only
    /// one is allowed.
    pending_precedence: Vec<PendingPrecedenceDeclaration>,
}

#[derive(Clone)]
/// One `precedence { ... }` block awaiting the completed token catalog.
struct PendingPrecedenceDeclaration {
    /// Locates the block's `precedence` keyword.
    site: SourceSite,
    /// The levels from lowest to highest binding power.
    levels: Vec<PendingPrecedenceLevel>,
}

#[derive(Clone)]
/// One level of a pending `precedence { ... }` block.
struct PendingPrecedenceLevel {
    /// The level's associativity.
    associativity: crate::PrecedenceAssociativity,
    /// The level's binding power.
    binding_power: u8,
    /// Locates the level's associativity keyword.
    site: SourceSite,
    /// The names at this level, in authored order.
    names: Vec<AdmissionReference>,
}

/// Carries the central content definition retained from authored source structure.
struct CentralContentDefinition {
    /// Supplies the semantic used to correlate authored source with normalized grammar.
    semantic: Option<ContentTokenId>,
    /// Preserves the authored name used by generated bindings and diagnostics.
    name: String,
    /// Preserves the authored lexical pattern for validation and generation.
    pattern: String,
    /// Preserves the optional closed matcher declared at the central token seam.
    closed_matcher: Option<ClosedMatcher>,
    /// Preserves explicit arbitration priority on a closed matcher.
    priority: Option<u32>,
    /// Locates the authored construct represented by this source-model value.
    site: SourceSite,
    /// Preserves an optional `#[node(derive(...), omit(...))]` trait delta for this entry.
    metas: Vec<Meta>,
}

#[derive(Clone)]
/// Carries one named admission declaration after source locations are retained.
struct PendingAdmissionDefinition {
    /// Preserves the authored admission name.
    name: String,
    /// Locates the admission declaration.
    site: SourceSite,
    /// Preserves the union-and-subtraction expression for graph evaluation.
    expression: PendingAdmissionExpression,
}

#[derive(Clone)]
/// Carries one lookahead filter declaration after source locations are retained.
struct PendingLookaheadDefinition {
    /// Preserves the authored merged kind name and its declaration site.
    merged: AdmissionReference,
    /// Names the token the filter merges.
    token: AdmissionReference,
    /// Names the triggers in authored order.
    triggers: Vec<AdmissionReference>,
}

#[derive(Clone)]
/// Carries one admission expression as an initial operand followed by ordered operations.
struct PendingAdmissionExpression {
    /// Supplies the initial membership set.
    initial: PendingAdmissionOperand,
    /// Supplies left-associative union and subtraction operations.
    operations: Vec<PendingAdmissionOperation>,
}

#[derive(Clone)]
/// Classifies the operands accepted by admission expressions.
enum PendingAdmissionOperand {
    /// Resolves a keyword, category, flag, named admission, or the `keywords` universe.
    Name(AdmissionReference),
    /// Resolves an explicit set of fixed-token names.
    Set {
        /// Preserves each explicit member and its authored location.
        members: Vec<AdmissionReference>,
        /// Locates the complete set operand.
        site: SourceSite,
    },
}

#[derive(Clone, Copy)]
/// Classifies one left-associative admission expression operation.
enum AdmissionOperationKind {
    /// Adds all right-hand membership.
    Union,
    /// Removes all right-hand membership.
    Subtract,
}

#[derive(Clone)]
/// Carries one located admission operation and its operand.
struct PendingAdmissionOperation {
    /// Selects union or subtraction behavior.
    kind: AdmissionOperationKind,
    /// Supplies the right-hand membership set.
    operand: PendingAdmissionOperand,
    /// Locates the operation for diagnostics.
    site: SourceSite,
}

/// Evaluates the named admission dependency graph without depending on declaration order.
struct AdmissionCompiler<'catalog, 'diagnostics> {
    /// Supplies fixed token definitions that admission membership ultimately names.
    definitions: &'catalog [TokenDefinition],
    /// Resolves explicit fixed-token names.
    fixed_by_name: &'catalog BTreeMap<String, TokenDefinitionId>,
    /// Maps equivalent fixed definitions to the representative used by canonical admission sets.
    canonical_fixed_ids: &'catalog BTreeMap<TokenDefinitionId, TokenDefinitionId>,
    /// Supplies canonical category membership derived from keyword metadata.
    category_members: &'catalog BTreeMap<String, BTreeSet<TokenDefinitionId>>,
    /// Supplies canonical flag membership derived from keyword metadata.
    flag_members: &'catalog BTreeMap<String, BTreeSet<TokenDefinitionId>>,
    /// Supplies unique named admission declarations.
    declarations: Vec<PendingAdmissionDefinition>,
    /// Resolves named admission references independently of source order.
    declaration_by_name: BTreeMap<String, usize>,
    /// Tracks unseen, active, and completed graph nodes.
    states: Vec<AdmissionEvaluationState>,
    /// Retains completed canonical membership or a failed evaluation.
    evaluated: Vec<Option<BTreeSet<TokenDefinitionId>>>,
    /// Tracks the active dependency path for cycle evidence.
    stack: Vec<usize>,
    /// Deduplicates cycles reached through more than one admission root.
    reported_cycles: BTreeSet<Vec<String>>,
    /// Receives structured source-model findings.
    diagnostics: &'diagnostics mut Vec<SourceModelDiagnostic>,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
/// Classifies one admission declaration's graph-evaluation state.
enum AdmissionEvaluationState {
    /// The declaration has not been evaluated.
    #[default]
    Unseen,
    /// The declaration is on the active dependency path.
    Active,
    /// The declaration has completed successfully or failed with a diagnostic.
    Complete,
}

impl<'catalog, 'diagnostics> AdmissionCompiler<'catalog, 'diagnostics> {
    /// Creates an admission compiler and diagnoses duplicate declaration names.
    fn new(
        definitions: &'catalog [TokenDefinition],
        fixed_by_name: &'catalog BTreeMap<String, TokenDefinitionId>,
        canonical_fixed_ids: &'catalog BTreeMap<TokenDefinitionId, TokenDefinitionId>,
        category_members: &'catalog BTreeMap<String, BTreeSet<TokenDefinitionId>>,
        flag_members: &'catalog BTreeMap<String, BTreeSet<TokenDefinitionId>>,
        declarations: Vec<PendingAdmissionDefinition>,
        diagnostics: &'diagnostics mut Vec<SourceModelDiagnostic>,
    ) -> Self {
        let mut declaration_by_name = BTreeMap::<String, usize>::new();
        for (index, declaration) in declarations.iter().enumerate() {
            if let Some(previous) = declaration_by_name.get(&declaration.name).copied() {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::ContradictoryAdmissionMetadata,
                    sites: vec![
                        declarations[previous].site.clone(),
                        declaration.site.clone(),
                    ],
                    message: format!(
                        "admission `{}` has more than one declaration",
                        declaration.name
                    ),
                });
            } else {
                declaration_by_name.insert(declaration.name.clone(), index);
            }
        }
        let count = declarations.len();
        Self {
            definitions,
            fixed_by_name,
            canonical_fixed_ids,
            category_members,
            flag_members,
            declarations,
            declaration_by_name,
            states: vec![AdmissionEvaluationState::Unseen; count],
            evaluated: vec![None; count],
            stack: Vec::new(),
            reported_cycles: BTreeSet::new(),
            diagnostics,
        }
    }

    /// Compiles every uniquely named declaration to sorted token-definition membership.
    fn compile(&mut self) -> BTreeMap<String, Vec<TokenDefinitionId>> {
        let names = self.declaration_by_name.keys().cloned().collect::<Vec<_>>();
        let mut compiled = BTreeMap::new();
        for name in names {
            let index = self.declaration_by_name[&name];
            if let Some(members) = self.evaluate(index) {
                compiled.insert(name, members.into_iter().collect());
            }
        }
        compiled
    }

    /// Evaluates one declaration recursively while rejecting dependency cycles.
    fn evaluate(&mut self, index: usize) -> Option<BTreeSet<TokenDefinitionId>> {
        match self.states[index] {
            AdmissionEvaluationState::Complete => return self.evaluated[index].clone(),
            AdmissionEvaluationState::Active => {
                self.report_cycle(index);
                return None;
            }
            AdmissionEvaluationState::Unseen => {}
        }
        self.states[index] = AdmissionEvaluationState::Active;
        self.stack.push(index);
        let expression = self.declarations[index].expression.clone();
        let mut result = self.resolve_operand(&expression.initial);
        for operation in expression.operations {
            let Some(current) = result.as_mut() else {
                break;
            };
            let Some(operand) = self.resolve_operand(&operation.operand) else {
                result = None;
                break;
            };
            let overlap = current.intersection(&operand).copied().collect::<Vec<_>>();
            match operation.kind {
                AdmissionOperationKind::Union => {
                    if !overlap.is_empty() {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::DuplicateAdmissionMembership,
                            sites: vec![operation.site],
                            message: format!(
                                "admission union repeats membership [{}]",
                                self.member_names(&overlap).join(", ")
                            ),
                        });
                    }
                    current.extend(operand);
                }
                AdmissionOperationKind::Subtract => {
                    if overlap.is_empty() {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::IneffectiveAdmissionSubtraction,
                            sites: vec![operation.site],
                            message: "admission subtraction removes no current member".to_owned(),
                        });
                        result = None;
                        break;
                    }
                    current.retain(|member| !operand.contains(member));
                }
            }
        }
        self.stack.pop();
        self.states[index] = AdmissionEvaluationState::Complete;
        self.evaluated[index] = result.clone();
        result
    }

    /// Resolves one expression operand to fixed-token membership.
    fn resolve_operand(
        &mut self,
        operand: &PendingAdmissionOperand,
    ) -> Option<BTreeSet<TokenDefinitionId>> {
        match operand {
            PendingAdmissionOperand::Name(reference) => self.resolve_name(reference),
            PendingAdmissionOperand::Set { members, site } => {
                let mut resolved = BTreeSet::new();
                let mut valid = true;
                for member in members {
                    let Some(definition) = self.word_fixed(&member.name) else {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::InvalidAdmissionReference,
                            sites: vec![member.site.clone()],
                            message: format!(
                                "admission set member `{}` is not a word-shaped fixed token",
                                member.name
                            ),
                        });
                        valid = false;
                        continue;
                    };
                    if !resolved.insert(definition) {
                        self.diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::DuplicateAdmissionMembership,
                            sites: vec![site.clone(), member.site.clone()],
                            message: format!("admission set repeats membership `{}`", member.name),
                        });
                    }
                }
                valid.then_some(resolved)
            }
        }
    }

    /// Resolves a symbolic operand and rejects unknown or cross-namespace-ambiguous names.
    fn resolve_name(
        &mut self,
        reference: &AdmissionReference,
    ) -> Option<BTreeSet<TokenDefinitionId>> {
        if reference.name == "keywords" {
            return Some(
                self.definitions
                    .iter()
                    .filter(|definition| {
                        matches!(
                            &definition.kind,
                            TokenDefinitionKind::Fixed(fixed) if is_word_shaped(&fixed.spelling)
                        )
                    })
                    .map(|definition| self.canonical_fixed_ids[&definition.id])
                    .collect(),
            );
        }

        let fixed = self.word_fixed(&reference.name);
        let category = self.category_members.get(&reference.name);
        let flag = self.flag_members.get(&reference.name);
        let admission = self.declaration_by_name.get(&reference.name).copied();
        let matches = usize::from(fixed.is_some())
            + usize::from(category.is_some())
            + usize::from(flag.is_some())
            + usize::from(admission.is_some());
        if matches != 1 {
            self.diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidAdmissionReference,
                sites: vec![reference.site.clone()],
                message: if matches == 0 {
                    format!("admission expression name `{}` is unknown", reference.name)
                } else {
                    format!(
                        "admission expression name `{}` is ambiguous across token metadata namespaces",
                        reference.name
                    )
                },
            });
            return None;
        }
        if let Some(fixed) = fixed {
            return Some([fixed].into_iter().collect());
        }
        if let Some(category) = category {
            return Some(category.clone());
        }
        if let Some(flag) = flag {
            return Some(flag.clone());
        }
        self.evaluate(admission.expect("exactly one namespace matched"))
    }

    /// Resolves only mechanically word-shaped fixed tokens.
    fn word_fixed(&self, name: &str) -> Option<TokenDefinitionId> {
        let definition = self.fixed_by_name.get(name).copied()?;
        let TokenDefinitionKind::Fixed(fixed) = &self.definitions[definition.index()].kind else {
            return None;
        };
        is_word_shaped(&fixed.spelling).then(|| self.canonical_fixed_ids[&definition])
    }

    /// Emits one cycle finding with all declarations participating in the active loop.
    fn report_cycle(&mut self, repeated: usize) {
        let start = self
            .stack
            .iter()
            .position(|index| *index == repeated)
            .expect("an active admission is on the evaluation stack");
        let cycle = self.stack[start..].to_vec();
        let mut key = cycle
            .iter()
            .map(|index| self.declarations[*index].name.clone())
            .collect::<Vec<_>>();
        key.sort();
        key.dedup();
        if !self.reported_cycles.insert(key.clone()) {
            return;
        }
        let mut sites = cycle
            .iter()
            .map(|index| self.declarations[*index].site.clone())
            .collect::<Vec<_>>();
        sites.sort_by_key(canonical_source_order_key);
        self.diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::AdmissionCycle,
            sites,
            message: format!("admission declarations form a cycle [{}]", key.join(", ")),
        });
    }

    /// Renders canonical fixed-token names for a membership diagnostic.
    fn member_names(&self, members: &[TokenDefinitionId]) -> Vec<String> {
        let mut names = members
            .iter()
            .map(|member| self.definitions[member.index()].name.clone())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

impl TokenCatalog {
    /// Validates keyword metadata and compiles named admission expressions to canonical sets.
    fn compile_named_admissions(
        &mut self,
        equivalence: crate::TextEquivalence,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        let canonical_fixed_ids = self.canonical_fixed_ids(equivalence);
        let (category_members, flag_members) =
            self.keyword_metadata_sets(&canonical_fixed_ids, diagnostics);
        let pending = std::mem::take(&mut self.pending_admissions);
        let mut compiler = AdmissionCompiler::new(
            &self.definitions,
            &self.fixed_by_name,
            &canonical_fixed_ids,
            &category_members,
            &flag_members,
            pending,
            diagnostics,
        );
        self.named_admissions = compiler.compile();
    }

    /// Validates `lookahead { ... }` declarations against the token definitions.
    ///
    /// Every failure is [`SourceModelDiagnosticKind::InvalidLookaheadFilter`]: a merged name that
    /// is declared twice or names a token definition, a merged token or trigger that is not a
    /// fixed token, a trigger that is neither a fixed token nor one unique content token, a
    /// trigger listed twice, or another filter over the same base token whose trigger set
    /// overlaps. Filters sharing a base token are valid when their trigger sets are disjoint;
    /// then the table-driven feed has one deterministic merged kind for every token pair. A
    /// filter with any failure is dropped.
    fn compile_lookahead_filters(&mut self, diagnostics: &mut Vec<SourceModelDiagnostic>) {
        let pending = std::mem::take(&mut self.pending_lookaheads);
        let mut declared = BTreeMap::<String, SourceSite>::new();
        let mut compiled = Vec::<LookaheadFilter>::new();
        for filter in pending {
            let mut failures = Vec::new();
            let name = &filter.merged.name;
            if let Some(previous) = declared.get(name) {
                failures.push((
                    vec![previous.clone(), filter.merged.site.clone()],
                    format!("lookahead filter `{name}` has more than one declaration"),
                ));
            } else {
                declared.insert(name.clone(), filter.merged.site.clone());
            }
            if self.fixed_by_name.contains_key(name) || self.content_by_name.contains_key(name) {
                failures.push((
                    vec![filter.merged.site.clone()],
                    format!("lookahead filter `{name}` has the name of a token definition"),
                ));
            }
            let token = self.fixed_by_name.get(&filter.token.name).copied();
            if token.is_none() {
                failures.push((
                    vec![filter.token.site.clone()],
                    format!(
                        "lookahead filter `{name}` merges `{}`, which is not a fixed token",
                        filter.token.name
                    ),
                ));
            }
            let mut triggers = Vec::new();
            for trigger in &filter.triggers {
                let content = self.content_by_name.get(&trigger.name);
                let content_id = content.and_then(|definitions| {
                    let identities = definitions
                        .iter()
                        .filter_map(
                            |definition| match &self.definitions[definition.index()].kind {
                                TokenDefinitionKind::Content(content) => Some((
                                    content.semantic,
                                    content.pattern.as_str(),
                                    content.closed_matcher.as_ref(),
                                    content.priority,
                                )),
                                TokenDefinitionKind::Fixed(_)
                                | TokenDefinitionKind::LexicalBase(_) => None,
                            },
                        )
                        .collect::<BTreeSet<_>>();
                    (identities.len() == 1).then(|| definitions[0])
                });
                if content.is_some() && content_id.is_none() {
                    let mut sites = vec![trigger.site.clone()];
                    for definition in content.into_iter().flatten() {
                        sites.extend(
                            self.definitions[definition.index()]
                                .definition_sites
                                .clone(),
                        );
                    }
                    failures.push((
                        sites,
                        format!(
                            "lookahead filter `{name}` names trigger `{}`, whose content-token \
                             definitions resolve to more than one lexical or semantic token kind",
                            trigger.name
                        ),
                    ));
                    continue;
                }
                let id = self
                    .fixed_by_name
                    .get(&trigger.name)
                    .copied()
                    .or(content_id);
                match id {
                    None => failures.push((
                        vec![trigger.site.clone()],
                        format!(
                            "lookahead filter `{name}` names trigger `{}`, which is neither a \
                             fixed token nor one unique content token",
                            trigger.name
                        ),
                    )),
                    Some(id) if triggers.contains(&id) => failures.push((
                        vec![trigger.site.clone()],
                        format!(
                            "lookahead filter `{name}` names trigger `{}` more than once",
                            trigger.name
                        ),
                    )),
                    Some(id) => triggers.push(id),
                }
            }
            if !failures.is_empty() {
                diagnostics.extend(failures.into_iter().map(|(sites, message)| {
                    SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::InvalidLookaheadFilter,
                        sites,
                        message,
                    }
                }));
                continue;
            }
            let token = token.expect("a filter without failures resolved its token");
            let mut overlaps_an_earlier_filter = false;
            for earlier in compiled.iter().filter(|earlier| earlier.token == token) {
                let overlapping = triggers
                    .iter()
                    .filter(|trigger| earlier.triggers.contains(trigger))
                    .map(|trigger| self.definitions[trigger.index()].name.as_str())
                    .collect::<Vec<_>>();
                if !overlapping.is_empty() {
                    overlaps_an_earlier_filter = true;
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::InvalidLookaheadFilter,
                        sites: vec![earlier.site.clone(), filter.merged.site.clone()],
                        message: format!(
                            "lookahead filter `{name}` merges `{}`, which lookahead filter \
                             `{}` already merges before overlapping trigger{} `{}`",
                            filter.token.name,
                            earlier.name,
                            if overlapping.len() == 1 { "" } else { "s" },
                            overlapping.join(", "),
                        ),
                    });
                }
            }
            if overlaps_an_earlier_filter {
                continue;
            }
            compiled.push(LookaheadFilter {
                name: name.clone(),
                site: filter.merged.site,
                token,
                triggers,
            });
        }
        self.lookahead_filters = compiled;
    }

    /// Selects one deterministic source-model definition for each emitted fixed-token kind.
    fn canonical_fixed_ids(
        &self,
        equivalence: crate::TextEquivalence,
    ) -> BTreeMap<TokenDefinitionId, TokenDefinitionId> {
        let mut groups = BTreeMap::<String, Vec<&TokenDefinition>>::new();
        for definition in &self.definitions {
            let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                continue;
            };
            groups
                .entry(canonical_fixed_spelling(&fixed.spelling, equivalence))
                .or_default()
                .push(definition);
        }

        let mut canonical = BTreeMap::new();
        for definitions in groups.values() {
            let representative = definitions
                .iter()
                .min_by_key(|definition| (&definition.name, definition.id))
                .expect("a fixed-spelling group is nonempty")
                .id;
            canonical.extend(
                definitions
                    .iter()
                    .map(|definition| (definition.id, representative)),
            );
        }
        canonical
    }

    /// Validates keyword metadata and materializes its category and flag indexes.
    ///
    /// Validation and materialization remain one phase so an invalid definition never contributes
    /// partial membership to a named admission. The phase first rejects category/flag namespace
    /// collisions and creates empty indexes, then validates and normalizes every metadata-bearing
    /// fixed token before inserting its category and flag membership.
    fn keyword_metadata_sets(
        &self,
        canonical_fixed_ids: &BTreeMap<TokenDefinitionId, TokenDefinitionId>,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> (
        BTreeMap<String, BTreeSet<TokenDefinitionId>>,
        BTreeMap<String, BTreeSet<TokenDefinitionId>>,
    ) {
        for (name, category_sites) in &self.categories {
            if let Some(flag_sites) = self.flags.get(name) {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::ContradictoryAdmissionMetadata,
                    sites: category_sites.iter().chain(flag_sites).cloned().collect(),
                    message: format!(
                        "keyword metadata name `{name}` cannot be both a category and a flag"
                    ),
                });
            }
        }

        let mut category_members = self
            .categories
            .keys()
            .cloned()
            .map(|name| (name, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        let mut flag_members = self
            .flags
            .keys()
            .cloned()
            .map(|name| (name, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();

        let mut canonical_definitions = BTreeMap::<TokenDefinitionId, Vec<&TokenDefinition>>::new();
        for definition in &self.definitions {
            let Some(canonical) = canonical_fixed_ids.get(&definition.id).copied() else {
                continue;
            };
            canonical_definitions
                .entry(canonical)
                .or_default()
                .push(definition);
        }

        for (canonical, definitions) in canonical_definitions {
            let metadata = definitions
                .iter()
                .filter_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Fixed(fixed) => Some(&fixed.keyword_metadata),
                    TokenDefinitionKind::Content(_) | TokenDefinitionKind::LexicalBase(_) => None,
                })
                .flatten()
                .filter(|metadata| metadata.category.is_some() || !metadata.flags.is_empty())
                .collect::<Vec<_>>();
            if metadata.is_empty() {
                continue;
            }
            let mut names = definitions
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>();
            names.sort_unstable();
            names.dedup();
            let display_name = names.join("/");

            let mut normalized = Vec::new();
            let mut valid = true;
            for metadata in &metadata {
                if metadata.authored_class != AuthoredFixedTokenClass::Keyword {
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::InvalidKeywordCategory,
                        sites: vec![metadata.site.clone()],
                        message: format!(
                            "punctuation `{}` cannot carry keyword category or flag metadata",
                            display_name
                        ),
                    });
                    valid = false;
                    continue;
                }
                let Some(category) = &metadata.category else {
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::InvalidKeywordCategory,
                        sites: vec![metadata.site.clone()],
                        message: format!(
                            "keyword `{}` metadata requires exactly one category",
                            display_name
                        ),
                    });
                    valid = false;
                    continue;
                };
                if !self.categories.contains_key(&category.name) {
                    diagnostics.push(SourceModelDiagnostic {
                        kind: SourceModelDiagnosticKind::InvalidKeywordCategory,
                        sites: vec![category.site.clone()],
                        message: format!(
                            "keyword `{}` uses unknown category `{}`",
                            display_name, category.name
                        ),
                    });
                    valid = false;
                }
                let mut flags = Vec::new();
                for flag in &metadata.flags {
                    if !self.flags.contains_key(&flag.name) {
                        diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::ContradictoryAdmissionMetadata,
                            sites: vec![flag.site.clone()],
                            message: format!(
                                "keyword `{}` uses unknown flag `{}`",
                                display_name, flag.name
                            ),
                        });
                        valid = false;
                    }
                    if flags.contains(&flag.name) {
                        diagnostics.push(SourceModelDiagnostic {
                            kind: SourceModelDiagnosticKind::ContradictoryAdmissionMetadata,
                            sites: vec![flag.site.clone()],
                            message: format!(
                                "keyword `{}` repeats flag `{}`",
                                display_name, flag.name
                            ),
                        });
                        valid = false;
                    } else {
                        flags.push(flag.name.clone());
                    }
                }
                flags.sort();
                normalized.push((category.name.clone(), flags));
            }
            normalized.sort();
            normalized.dedup();
            if normalized.len() > 1 {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::ContradictoryAdmissionMetadata,
                    sites: metadata
                        .iter()
                        .map(|metadata| metadata.site.clone())
                        .collect(),
                    message: format!(
                        "keyword `{}` has contradictory category or flag metadata",
                        display_name
                    ),
                });
                valid = false;
            }
            if !valid {
                continue;
            }
            let Some((category, flags)) = normalized.into_iter().next() else {
                continue;
            };
            category_members
                .get_mut(&category)
                .expect("validated category exists")
                .insert(canonical);
            for flag in flags {
                flag_members
                    .get_mut(&flag)
                    .expect("validated flag exists")
                    .insert(canonical);
            }
        }
        (category_members, flag_members)
    }

    /// Collects fixed-token definitions before recording their grammar positions.
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
                        Ok(syntax) => {
                            self.collect_central_definitions(resolved, origin, syntax, diagnostics)
                        }
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

    /// Collects fixed definitions from metas.
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
                    AuthoredFixedDetails::default(),
                );
            }
        }
    }

    /// Collects grammar-level content-token definitions before Node lowering.
    fn collect_central_definitions(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
        syntax: CentralTokensSyntax,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) {
        for post_lex in syntax.obsolete_post_lex {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::ObsoletePostLexHook,
                sites: vec![source_site(resolved, origin, span_range(post_lex.span()))],
                message: "authored `post_lex = path` hooks are unsupported; express tokenization declaratively"
                    .to_owned(),
            });
        }
        for fixed in syntax.fixed {
            let declaration_site = source_site(resolved, origin, span_range(fixed.name.span()));
            let keyword_metadata = AuthoredKeywordMetadata {
                authored_class: fixed.authored_class,
                category: fixed.category.map(|category| AdmissionReference {
                    name: category.unraw().to_string(),
                    site: source_site(resolved, origin, span_range(category.span())),
                }),
                flags: fixed
                    .flags
                    .into_iter()
                    .map(|flag| AdmissionReference {
                        name: flag.unraw().to_string(),
                        site: source_site(resolved, origin, span_range(flag.span())),
                    })
                    .collect(),
                site: declaration_site,
            };
            self.define_fixed(
                resolved,
                origin,
                fixed.name.unraw().to_string(),
                fixed.spelling,
                AuthoredFixedDetails {
                    authored_class: Some(fixed.authored_class),
                    pretty_spacing: fixed.pretty_spacing,
                    keyword_metadata: Some(keyword_metadata),
                },
            );
        }
        for category in syntax.categories {
            self.categories
                .entry(category.unraw().to_string())
                .or_default()
                .push(source_site(resolved, origin, span_range(category.span())));
        }
        for flag in syntax.flags {
            self.flags
                .entry(flag.unraw().to_string())
                .or_default()
                .push(source_site(resolved, origin, span_range(flag.span())));
        }
        self.pending_admissions.extend(
            syntax
                .admissions
                .into_iter()
                .map(|admission| pending_admission(resolved, origin, admission)),
        );
        self.pending_lookaheads
            .extend(syntax.lookaheads.into_iter().map(|lookahead| {
                let reference = |ident: syn::Ident| AdmissionReference {
                    name: ident.unraw().to_string(),
                    site: source_site(resolved, origin, span_range(ident.span())),
                };
                PendingLookaheadDefinition {
                    merged: reference(lookahead.merged),
                    token: reference(lookahead.token),
                    triggers: lookahead.triggers.into_iter().map(reference).collect(),
                }
            }));
        self.pending_precedence
            .extend(syntax.precedence.into_iter().map(|declaration| {
                PendingPrecedenceDeclaration {
                    site: source_site(resolved, origin, span_range(declaration.keyword.span())),
                    levels: declaration
                        .levels
                        .into_iter()
                        .map(|level| PendingPrecedenceLevel {
                            associativity: level.associativity,
                            binding_power: level.binding_power,
                            site: source_site(resolved, origin, span_range(level.span)),
                            names: level
                                .names
                                .into_iter()
                                .map(|ident| AdmissionReference {
                                    name: ident.unraw().to_string(),
                                    site: source_site(resolved, origin, span_range(ident.span())),
                                })
                                .collect(),
                        })
                        .collect(),
                }
            }));
        for content in syntax.content {
            if let Some(callback) = content.obsolete_callback {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::ObsoleteCentralLexCallback,
                    sites: vec![source_site(resolved, origin, span_range(callback.span()))],
                    message: "authored central `with path` lexer callbacks are unsupported; use a closed matcher"
                        .to_owned(),
                });
                continue;
            }
            let name = content.name.unraw().to_string();
            let semantic = resolved
                .content_tokens
                .iter()
                .find_map(|token| (token.ident == name).then_some(token.id));
            self.central_content.push(CentralContentDefinition {
                semantic,
                name,
                pattern: content.pattern.value(),
                closed_matcher: None,
                priority: None,
                site: source_site(resolved, origin, span_range(content.name.span())),
                metas: content.metas,
            });
        }
        for content in syntax.matchers {
            let name = content.name.unraw().to_string();
            let semantic = resolved
                .content_tokens
                .iter()
                .find_map(|token| (token.ident == name).then_some(token.id));
            let pattern = content.matcher.matcher.starter_pattern();
            self.central_content.push(CentralContentDefinition {
                semantic,
                name,
                pattern,
                closed_matcher: Some(content.matcher.matcher),
                priority: content.matcher.priority,
                site: source_site(resolved, origin, span_range(content.name.span())),
                metas: content.metas,
            });
        }
        self.lexer_records
            .extend(
                syntax
                    .non_token_lexer_records
                    .into_iter()
                    .map(|record| NonTokenLexerRecord {
                        pattern: record.pattern.value(),
                        closed_matcher: None,
                        priority: None,
                        outcome: record.outcome,
                        site: source_site(resolved, origin, span_range(record.pattern.span())),
                    }),
            );
        self.lexer_records
            .extend(syntax.non_token_matchers.into_iter().map(|record| {
                let pattern = record.matcher.matcher.starter_pattern();
                NonTokenLexerRecord {
                    pattern,
                    closed_matcher: Some(record.matcher.matcher),
                    priority: record.matcher.priority,
                    outcome: record.outcome,
                    site: source_site(resolved, origin, span_range(record.name.span())),
                }
            }));
    }

    /// Interns a fixed-token definition or diagnoses incompatible duplicate spellings.
    fn define_fixed(
        &mut self,
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
        name: String,
        spelling: LitStr,
        details: AuthoredFixedDetails,
    ) {
        let AuthoredFixedDetails {
            authored_class,
            pretty_spacing,
            keyword_metadata,
        } = details;
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
                if let Some(metadata) = keyword_metadata {
                    existing.keyword_metadata.push(metadata);
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
                keyword_metadata: keyword_metadata.into_iter().collect(),
            }),
            definition_sites: vec![site],
        });
    }

    /// Derives the fixed definition from the retained source model and its correlations.
    fn fixed_definition(&self, name: &str) -> Option<TokenDefinitionId> {
        self.fixed_by_name.get(name).copied()
    }

    /// Derives the record use from the retained source model and its correlations.
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

    /// Derives the fixed element from the retained source model and its correlations.
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
            TokenSequenceEntry::Optional { tokens, span } => {
                let group_site = source_site(resolved, origin, span_range(span));
                let tokens = tokens
                    .into_iter()
                    .filter_map(|token| self.fixed_element(resolved, diagnostics, origin, token))
                    .filter_map(|element| {
                        let GrammarElement::FixedToken {
                            token_use, site, ..
                        } = element
                        else {
                            return None;
                        };
                        Some(AttachedFixedToken { token_use, site })
                    })
                    .collect::<Vec<_>>();
                return (!tokens.is_empty()).then_some(GrammarElement::OptionalFixedTokenGroup {
                    id: GrammarElementId::next(0),
                    tokens,
                    site: group_site,
                });
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

/// Carries the trait planner retained from authored source structure.
struct TraitPlanner {
    /// Supplies the grammar defaults used to correlate authored source with normalized grammar.
    grammar_defaults: Vec<ConfiguredTraitSelection>,
}

impl TraitPlanner {
    /// Resolves grammar-wide configured-trait defaults and diagnoses unsupported selections.
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

    /// Derives the node plan from the retained source model and its correlations.
    fn node_plan(
        &self,
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
        metas: &[Meta],
        declaration_surface: bool,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> ConfiguredTraitPlan {
        // Derive one source-ordered plan per Node while retaining the locations needed by every later diagnostic.
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
            if declaration_surface && !syntax.omissions.is_empty() {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                    sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                    message:
                        "`#[node(omit(...))]` is retired; write `#[omit(...)]` on the declaration"
                            .to_owned(),
                });
            }
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
                    if declaration_surface {
                        Vec::new()
                    } else {
                        syntax.omissions
                    },
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

        let omit_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("omit"))
            .collect::<Vec<_>>();
        if omit_metas.len() > 1 {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                sites: omit_metas
                    .iter()
                    .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                    .collect(),
                message: "a Node accepts at most one standalone `#[omit(...)]` list".to_owned(),
            });
        } else if let Some(meta) = omit_metas.first() {
            let Meta::List(list) = meta else {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                    sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                    message: "`#[omit]` must use parentheses".to_owned(),
                });
                return self.grammar_default_plan();
            };
            match syn::punctuated::Punctuated::<syn::Path, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
            {
                Ok(paths) => {
                    omissions = self
                        .normalize_paths(
                            resolved,
                            origin,
                            paths.into_iter().collect(),
                            ConfiguredTraitOwnership::LocalOmission,
                            diagnostics,
                        )
                        .into_iter()
                        .map(|selection| ConfiguredTraitOmission {
                            recipe: selection.recipe,
                            ownership: selection.ownership,
                        })
                        .collect()
                }
                Err(error) => diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidTraitDelta,
                    sites: vec![source_site(resolved, origin, span_range(error.span()))],
                    message: error.to_string(),
                }),
            }
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

    /// Derives the grammar default plan from the retained source model and its correlations.
    fn grammar_default_plan(&self) -> ConfiguredTraitPlan {
        ConfiguredTraitPlan {
            effective: self.grammar_defaults.clone(),
            additions: Vec::new(),
            omissions: Vec::new(),
        }
    }

    /// Canonicalizes opaque authored paths relative to the configured facade.
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

    /// Validates ordinary derive ownership.
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

/// Carries the separator planner retained from authored source structure.
struct SeparatorPlanner;

impl SeparatorPlanner {
    #[allow(clippy::too_many_arguments)]
    /// Resolves and records the separator attached to one repeated semantic field.
    fn lower(
        resolved: &ResolvedSourceGrammar,
        semantic_facts: &SemanticFacts,
        tokens: &mut TokenCatalog,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
        origin: SourceOrigin,
        metas: &[Meta],
        field: &ResolvedNodeField,
    ) -> Option<Separator> {
        // Lower attributes, fields, and variants in authored order, collecting independent source-model diagnostics in one pass.
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

/// Carries the positional token planner retained from authored source structure.
struct PositionalTokenPlanner;

impl PositionalTokenPlanner {
    /// Lowers one fixed-only presence declaration into its authored boolean field.
    #[allow(clippy::too_many_arguments)]
    fn lower_presence(
        resolved: &ResolvedSourceGrammar,
        tokens: &mut TokenCatalog,
        diagnostics: &mut Vec<SourceModelDiagnostic>,
        origin: SourceOrigin,
        metas: &[Meta],
        resolved_field: ResolvedFieldId,
        name: Option<String>,
        pretty: PrettyHints,
        site: SourceSite,
    ) -> Option<GrammarElement> {
        let presence_metas = metas
            .iter()
            .filter(|meta| meta.path().is_ident("presence"))
            .collect::<Vec<_>>();
        let conflicts = metas
            .iter()
            .filter(|meta| matches!(path_text(meta.path()).as_str(), "lex" | "sep"))
            .collect::<Vec<_>>();
        if presence_metas.len() != 1 || !conflicts.is_empty() {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidFixedTokenPresence,
                sites: presence_metas
                    .iter()
                    .chain(conflicts.iter())
                    .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                    .collect(),
                message:
                    "a bool field accepts exactly one #[presence(...)] and no #[lex] or #[sep]"
                        .to_owned(),
            });
            return None;
        }
        let meta = presence_metas[0];
        let Meta::List(list) = meta else {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidFixedTokenPresence,
                sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                message: "#[presence] must contain one non-empty fixed-token sequence".to_owned(),
            });
            return None;
        };
        let sequence = match TokenSequence::parse.parse2(list.tokens.clone()) {
            Ok(sequence) if !sequence.0.is_empty() => sequence,
            Ok(_) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidFixedTokenPresence,
                    sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                    message: "#[presence] must contain one non-empty fixed-token sequence"
                        .to_owned(),
                });
                return None;
            }
            Err(error) => {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidFixedTokenPresence,
                    sites: vec![source_site(resolved, origin, span_range(error.span()))],
                    message: error.to_string(),
                });
                return None;
            }
        };
        if let Some(span) = sequence.0.iter().find_map(|entry| match entry {
            TokenSequenceEntry::This(span) => Some(*span),
            TokenSequenceEntry::Optional { span, .. } => Some(*span),
            TokenSequenceEntry::Definition(_) | TokenSequenceEntry::Reference(_) => None,
        }) {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidFixedTokenPresence,
                sites: vec![source_site(resolved, origin, span_range(span))],
                message: "#[presence(...)] accepts a flat fixed-token sequence".to_owned(),
            });
            return None;
        }
        let fixed = sequence
            .0
            .into_iter()
            .filter_map(|token| tokens.fixed_element(resolved, diagnostics, origin, token))
            .map(|element| {
                let GrammarElement::FixedToken {
                    token_use, site, ..
                } = element
                else {
                    unreachable!("fixed-element lowering returns fixed syntax")
                };
                AttachedFixedToken { token_use, site }
            })
            .collect::<Vec<_>>();
        (!fixed.is_empty()).then_some(GrammarElement::SemanticField {
            id: GrammarElementId::next(0),
            resolved_field,
            projection: SemanticFieldProjection::FixedPresence { tokens: fixed },
            name,
            attachment: None,
            separator: None,
            pratt_extensions: None,
            pretty,
            attachment_before: 0,
            attachment_after: 0,
            site,
        })
    }

    /// Inserts positional fixed tokens around semantic fields according to authored attributes.
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
                TokenSequenceEntry::Definition(_)
                | TokenSequenceEntry::Reference(_)
                | TokenSequenceEntry::Optional { .. } => None,
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
                                let attached = match element {
                                    GrammarElement::FixedToken {
                                        token_use, site, ..
                                    } => AttachedFixedSyntax::Required(AttachedFixedToken {
                                        token_use,
                                        site,
                                    }),
                                    GrammarElement::OptionalFixedTokenGroup {
                                        tokens,
                                        site,
                                        ..
                                    } => AttachedFixedSyntax::Optional { tokens, site },
                                    GrammarElement::SemanticField { .. } => {
                                        unreachable!("fixed-element lowering returns fixed syntax")
                                    }
                                };
                                if seen_this {
                                    after.push(attached);
                                } else {
                                    before.push(attached);
                                }
                            }
                        }
                    }
                }
                let [field] = body.as_mut_slice() else {
                    unreachable!("a field attachment owns exactly one semantic element")
                };
                let field_attachment = match field {
                    GrammarElement::SemanticField { attachment, .. } => attachment,
                    GrammarElement::FixedToken { .. }
                    | GrammarElement::OptionalFixedTokenGroup { .. } => {
                        unreachable!("a field attachment owns one semantic element")
                    }
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

    /// Derives the place anchorless from the retained source model and its correlations.
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

    /// Diagnoses superseded token attributes instead of silently changing their meaning.
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
    /// Resolves a Pratt variant's role and binding powers from its attributes.
    fn from_attributes(
        resolved: &ResolvedSourceGrammar,
        origin: SourceOrigin,
        metas: &[Meta],
        diagnostics: &mut Vec<SourceModelDiagnostic>,
    ) -> Self {
        let parse_metas = metas
            .iter()
            .filter(|meta| {
                meta.path().is_ident("parse")
                    && !is_precedence_override_meta(meta)
                    && !is_lr_conflict_meta(meta)
            })
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

/// Lowers one field-local Pratt extension restriction.
fn lower_pratt_extension_filter(
    resolved: &ResolvedSourceGrammar,
    origin: SourceOrigin,
    metas: &[Meta],
    diagnostics: &mut Vec<SourceModelDiagnostic>,
) -> Option<PrattExtensionFilter> {
    let restrictions = metas
        .iter()
        .filter(|meta| is_pratt_filter_meta(meta))
        .collect::<Vec<_>>();
    if restrictions.is_empty() {
        return None;
    }
    if restrictions.len() > 1 {
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::InvalidPrattExtensionFilter,
            sites: restrictions
                .iter()
                .map(|meta| source_site(resolved, origin, span_range(meta.span())))
                .collect(),
            message: "a field accepts at most one Pratt extension restriction".to_owned(),
        });
        return None;
    }
    let Meta::List(list) = restrictions[0] else {
        unreachable!("field Pratt restrictions use list-form #[parse(...)] syntax")
    };
    match syn::parse2::<PrattFieldSyntax>(list.tokens.clone()) {
        Ok(syntax) => Some(PrattExtensionFilter {
            excluded: syntax
                .excluded
                .into_iter()
                .map(|ident| ident.unraw().to_string())
                .collect(),
        }),
        Err(error) => {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidPrattExtensionFilter,
                sites: vec![source_site(resolved, origin, span_range(error.span()))],
                message: error.to_string(),
            });
            None
        }
    }
}

/// Diagnoses Pratt-only attributes used outside a Pratt variant.
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

/// Rejects pratt roles off pratt.
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

/// Rejects retired parser predicates at their authored attachment site.
fn reject_obsolete_parser_postconditions(
    resolved: &ResolvedSourceGrammar,
    origin: SourceOrigin,
    metas: &[Meta],
    diagnostics: &mut Vec<SourceModelDiagnostic>,
) {
    for list in metas.iter().filter_map(|meta| {
        let Meta::List(list) = meta else { return None };
        let is_parser_attribute = list.path.is_ident("parse")
            || list
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "parser");
        is_parser_attribute.then_some(list)
    }) {
        let Ok(options) = syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
        else {
            continue;
        };
        let Some(postcondition) = options
            .iter()
            .find(|option| option.path().is_ident("postcondition"))
        else {
            continue;
        };
        diagnostics.push(SourceModelDiagnostic {
            kind: SourceModelDiagnosticKind::ObsoleteParserPostcondition,
            sites: vec![source_site(
                resolved,
                origin,
                span_range(postcondition.span()),
            )],
            message: "authored parser postconditions are unsupported; use a named admission"
                .to_owned(),
        });
    }
}

/// Reports whether an attribute names a prefix, infix, or postfix Pratt role.
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

/// Reports whether an attribute is a `#[parse(prec = NAME)]` precedence override.
fn is_precedence_override_meta(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("parse") {
        return false;
    }
    matches!(
        list.tokens.clone().into_iter().next(),
        Some(proc_macro2::TokenTree::Ident(keyword)) if keyword == "prec"
    )
}

/// Reports whether an attribute is a `#[parse(lr_conflict(...))]` declaration.
fn is_lr_conflict_meta(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("parse") {
        return false;
    }
    matches!(
        list.tokens.clone().into_iter().next(),
        Some(proc_macro2::TokenTree::Ident(keyword)) if keyword == "lr_conflict"
    )
}

/// Reports whether an attribute names a field-local Pratt restriction.
fn is_pratt_filter_meta(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("parse") {
        return false;
    }
    matches!(
        list.tokens.clone().into_iter().next(),
        Some(proc_macro2::TokenTree::Ident(keyword)) if keyword == "pratt"
    )
}

/// Rejects misplaced content declarations.
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

/// Derives the source site from the retained source model and its correlations.
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

/// Renders source order key in canonical order.
fn canonical_source_order_key(site: &SourceSite) -> SourceOrderKey {
    SourceOrderKey {
        module: site.module.index(),
        start: site.range.start,
        end: site.range.end,
    }
}

impl TokenCatalog {
    /// Records a non-token lexer rule for later lexical planning.
    fn define_lexical_base(&mut self, definition: CentralContentDefinition) {
        if let Some(existing) = self.definitions.iter_mut().find(|existing| {
            existing.name == definition.name
                && matches!(
                    &existing.kind,
                    TokenDefinitionKind::LexicalBase(base)
                        if base.pattern == definition.pattern
                            && base.closed_matcher == definition.closed_matcher
                            && base.priority == definition.priority
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
                closed_matcher: definition.closed_matcher,
                priority: definition.priority,
            }),
            definition_sites: vec![definition.site],
        });
    }

    #[allow(clippy::too_many_arguments)]
    /// Converts one authored content-token definition into catalog state.
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
        // Normalize a content-token definition before merging it, so equivalent declarations share one semantic identity.
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
        if let Some(callback) = syntax.obsolete_callback {
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::ObsoleteFieldLexCallback,
                sites: vec![source_site(resolved, origin, span_range(callback.span()))],
                message:
                    "authored `#[lex(..., callback = path)]` is unsupported; use a closed matcher"
                        .to_owned(),
            });
            return;
        }
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

        if syntax.matcher {
            if syntax.diagram.is_some()
                || syntax.priority.is_some()
                || !syntax.admissions.is_empty()
            {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidContentToken,
                    sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                    message: "#[lex(matcher)] takes no field-local matcher options; configure the matcher centrally"
                        .to_owned(),
                });
                return;
            }
            let bound = self
                .content_by_name
                .get(&name)
                .into_iter()
                .flatten()
                .copied()
                .any(|definition| {
                    matches!(
                        &self.definitions[definition.index()].kind,
                        TokenDefinitionKind::Content(content)
                            if content.semantic == semantic && content.closed_matcher.is_some()
                    )
                });
            if !bound {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::InvalidContentToken,
                    sites: vec![source_site(resolved, origin, span_range(meta.span()))],
                    message: format!(
                        "#[lex(matcher)] for `{name}` requires a matching central `matchers` declaration"
                    ),
                });
            }
            return;
        }

        let pattern = syntax
            .pattern
            .as_ref()
            .expect("a non-matcher lex declaration has a pattern");

        let mut authored_definitions = Vec::new();
        for reference in syntax.admissions {
            let site = source_site(resolved, origin, span_range(reference.span()));
            let fixed_name = reference.unraw().to_string();
            let definitions = if let Some(admission) = self.named_admissions.get(&fixed_name) {
                admission.clone()
            } else if let Some(definition) = self.fixed_definition(&fixed_name) {
                vec![definition]
            } else {
                diagnostics.push(SourceModelDiagnostic {
                    kind: SourceModelDiagnosticKind::UnresolvedFixedToken,
                    sites: vec![site],
                    message: format!(
                        "fixed token or named admission `{fixed_name}` has no declaration"
                    ),
                });
                continue;
            };
            for definition in definitions {
                authored_definitions.push(definition);
                self.record_use(definition, TokenUseRole::ContentAdmission, site.clone());
            }
        }

        let definition_site = source_site(resolved, origin, span_range(meta.span()));
        let mut admission_set = authored_definitions.clone();
        admission_set.sort_unstable();
        admission_set.dedup();
        let authored = AuthoredAdmissions {
            site: definition_site.clone(),
            definitions: authored_definitions,
        };
        let diagram_label = syntax.diagram.map(|label| label.value());
        if let Some(conflict) = self.definitions.iter().find(|definition| {
            matches!(
                &definition.kind,
                TokenDefinitionKind::Content(content)
                    if content.semantic == semantic
                        && content.pattern == pattern.value()
                        && content.priority == syntax.priority
                        && content.admission_set == admission_set
                        && content.diagram_label != diagram_label
            )
        }) {
            let mut sites = conflict.definition_sites.clone();
            sites.push(definition_site);
            diagnostics.push(SourceModelDiagnostic {
                kind: SourceModelDiagnosticKind::InvalidContentToken,
                sites,
                message: format!(
                    "conflicting content token diagram labels for `{name}`; use one label at every declaration"
                ),
            });
            return;
        }
        // A content-token-declaring field's own `#[node(...)]` attribute (alongside `#[lex(...)]`
        // on the same field) can add or omit configured traits for this specific token, exactly
        // like `#[node(...)]` already does for a whole Node — reused verbatim via `node_plan`,
        // which only ever scans `metas` for a `#[node(...)]` entry and has no other dependency on
        // what it's attached to.
        let configured_traits = TraitPlanner {
            grammar_defaults: grammar_defaults.to_vec(),
        }
        .node_plan(resolved, origin, metas, false, diagnostics);
        self.define_content(
            semantic,
            name,
            pattern.value(),
            None,
            diagram_label,
            syntax.priority,
            admission_set,
            authored,
            definition_site,
            configured_traits,
        );
    }

    #[allow(clippy::too_many_arguments)]
    /// Interns a content-token definition and all of its authored admissions.
    fn define_content(
        &mut self,
        semantic: ContentTokenId,
        name: String,
        pattern: String,
        closed_matcher: Option<ClosedMatcher>,
        diagram_label: Option<String>,
        priority: Option<u32>,
        admission_set: Vec<TokenDefinitionId>,
        authored: AuthoredAdmissions,
        site: SourceSite,
        configured_traits: ConfiguredTraitPlan,
    ) {
        let compatible = self.content_by_name.get(&name).and_then(|definitions| {
            definitions.iter().copied().find(|id| {
                matches!(
                    &self.definitions[id.index()].kind,
                    TokenDefinitionKind::Content(content)
                    if content.semantic == semantic
                        && content.pattern == pattern
                        && content.closed_matcher == closed_matcher
                        && content.diagram_label == diagram_label
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
                closed_matcher,
                diagram_label,
                priority,
                admission_set,
                authored_admissions: vec![authored],
                text: ContentTextStorage::BorrowedOrOwnedMatchedText,
                configured_traits,
            }),
            definition_sites: vec![site],
        });
    }
}

/// Carries the central tokens syntax retained from authored source structure.
struct CentralTokensSyntax {
    /// Supplies the fixed used to correlate authored source with normalized grammar.
    fixed: Vec<CentralFixedSyntax>,
    /// Supplies the content used to correlate authored source with normalized grammar.
    content: Vec<CentralContentSyntax>,
    /// Supplies named emitting closed matchers.
    matchers: Vec<CentralMatcherSyntax>,
    /// Supplies the non token lexer records used to correlate authored source with normalized grammar.
    non_token_lexer_records: Vec<CentralNonTokenLexerRecordSyntax>,
    /// Supplies named non-token closed matchers and their retention behavior.
    non_token_matchers: Vec<CentralNonTokenMatcherSyntax>,
    /// Retains obsolete post-lex paths only long enough to diagnose their source sites.
    obsolete_post_lex: Vec<syn::Path>,
    /// Supplies declared keyword category names.
    categories: Vec<syn::Ident>,
    /// Supplies declared orthogonal keyword flag names.
    flags: Vec<syn::Ident>,
    /// Supplies named admission expressions.
    admissions: Vec<CentralAdmissionSyntax>,
    /// Supplies `MERGED = TOKEN before { TRIGGER, ... }` lookahead filter declarations.
    lookaheads: Vec<CentralLookaheadSyntax>,
    /// Supplies every `precedence { ... }` block, in authored order.
    precedence: Vec<CentralPrecedenceSyntax>,
}

#[derive(Clone)]
/// Carries one parsed `precedence { ... }` block: bison's precedence table, levels from
/// lowest to highest.
struct CentralPrecedenceSyntax {
    /// Preserves the `precedence` keyword, which locates the block.
    keyword: syn::Ident,
    /// Preserves the levels in authored order.
    levels: Vec<CentralPrecedenceLevelSyntax>,
}

#[derive(Clone)]
/// Carries one parsed level `left(bp = N) { A, B }`, `right(bp = N) { ... }` or
/// `nonassoc(bp = N) { ... }`.
struct CentralPrecedenceLevelSyntax {
    /// The parsed associativity keyword.
    associativity: crate::PrecedenceAssociativity,
    /// The parsed binding power.
    binding_power: u8,
    /// Locates the level, from its associativity keyword.
    span: proc_macro2::Span,
    /// Preserves the authored names in order.
    names: Vec<syn::Ident>,
}

#[derive(Clone)]
/// Carries one parsed `MERGED = TOKEN before { TRIGGER, ... }` lookahead filter declaration.
struct CentralLookaheadSyntax {
    /// Preserves the authored merged kind name.
    merged: syn::Ident,
    /// Preserves the authored name of the token the filter merges.
    token: syn::Ident,
    /// Preserves the authored trigger names in order.
    triggers: Vec<syn::Ident>,
}

/// Carries the central non token lexer record syntax retained from authored source structure.
struct CentralNonTokenLexerRecordSyntax {
    /// Preserves the authored lexical pattern for validation and generation.
    pattern: LitStr,
    /// Supplies the outcome used to correlate authored source with normalized grammar.
    outcome: NonTokenLexerOutcome,
}

/// Carries one named non-token closed matcher before source locations are retained.
struct CentralNonTokenMatcherSyntax {
    /// Preserves the authored name used for diagnostics and optional trivia classification.
    name: syn::Ident,
    /// Preserves the closed matcher and its source literals.
    matcher: ClosedMatcherSyntax,
    /// Selects discard or classified-trivia retention outside significant parsing.
    outcome: NonTokenLexerOutcome,
}

/// Carries the central fixed syntax retained from authored source structure.
struct CentralFixedSyntax {
    /// Preserves the authored name used by generated bindings and diagnostics.
    name: syn::Ident,
    /// Supplies the spelling used to correlate authored source with normalized grammar.
    spelling: LitStr,
    /// Supplies the authored class used to correlate authored source with normalized grammar.
    authored_class: AuthoredFixedTokenClass,
    /// Supplies the pretty spacing used to correlate authored source with normalized grammar.
    pretty_spacing: PrettySpacing,
    /// Names the keyword's single category in the metadata form.
    category: Option<syn::Ident>,
    /// Names the keyword's orthogonal flags in authored order.
    flags: Vec<syn::Ident>,
}

/// Carries the central content syntax retained from authored source structure.
struct CentralContentSyntax {
    /// Preserves the authored name used by generated bindings and diagnostics.
    name: syn::Ident,
    /// Preserves the authored lexical pattern for validation and generation.
    pattern: LitStr,
    /// Retains an obsolete callback path only long enough to diagnose its source site.
    obsolete_callback: Option<syn::Path>,
    /// Preserves an optional `#[node(derive(...), omit(...))]` trait delta for this entry.
    metas: Vec<Meta>,
}

/// Carries one named emitting closed matcher before source locations are retained.
struct CentralMatcherSyntax {
    /// Preserves the nominal generated content-token name.
    name: syn::Ident,
    /// Preserves the closed matcher and its source literals.
    matcher: ClosedMatcherSyntax,
    /// Preserves an optional `#[node(derive(...), omit(...))]` trait delta for this entry.
    metas: Vec<Meta>,
}

/// Parsed closed matcher together with its common arbitration metadata.
struct ClosedMatcherSyntax {
    /// Validated closed matcher configuration.
    matcher: ClosedMatcher,
    /// Explicit priority applied after final extent and fixed-spelling precedence.
    priority: Option<u32>,
}

/// One named argument supplied to a closed matcher constructor.
struct ClosedMatcherArgument {
    /// Identifies the parameter checked for uniqueness and family-specific applicability.
    name: syn::Ident,
    /// Literal or literal-array value.
    value: syn::Expr,
}

impl Parse for ClosedMatcherArgument {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = syn::Ident::parse_any(input)?;
        input.parse::<Token![=]>()?;
        Ok(Self {
            name,
            value: input.parse()?,
        })
    }
}

impl Parse for ClosedMatcherSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let family = syn::Ident::parse_any(input)?;
        let body;
        syn::parenthesized!(body in input);
        let parsed =
            syn::punctuated::Punctuated::<ClosedMatcherArgument, Token![,]>::parse_terminated(
                &body,
            )?;
        let mut arguments = BTreeMap::<String, (Span, syn::Expr)>::new();
        for argument in parsed {
            let name = argument.name.unraw().to_string();
            if arguments
                .insert(name.clone(), (argument.name.span(), argument.value))
                .is_some()
            {
                return Err(syn::Error::new(
                    argument.name.span(),
                    format!("duplicate `{name}` closed-matcher argument"),
                ));
            }
        }
        let priority = take_matcher_u32(&mut arguments, "priority")?;
        let matcher = match family.to_string().as_str() {
            "same_delimiter" => ClosedMatcher::SameDelimiter {
                opener: take_matcher_string(&mut arguments, "opener")?,
            },
            "nested" => {
                let opener = take_matcher_string(&mut arguments, "opener")?;
                let closer = take_matcher_string(&mut arguments, "closer")?;
                if opener.is_empty() || closer.is_empty() || opener == closer {
                    return Err(syn::Error::new(
                        family.span(),
                        "nested matcher delimiters must be nonempty and distinct",
                    ));
                }
                ClosedMatcher::Nested { opener, closer }
            }
            "next_exclusion" => {
                let pattern = take_matcher_string(&mut arguments, "pattern")?;
                let excluded = take_matcher_string(&mut arguments, "excluded")?;
                if !matches_one_complete_scalar(&excluded) {
                    return Err(syn::Error::new(
                        family.span(),
                        "next-exclusion predicate must compile and match at least one complete, nonempty scalar",
                    ));
                }
                ClosedMatcher::NextExclusion { pattern, excluded }
            }
            "operator_run" => {
                let characters = take_matcher_string(&mut arguments, "characters")?;
                let fences = take_matcher_strings(&mut arguments, "fences")?;
                let trailing = take_matcher_string(&mut arguments, "trailing")?;
                let qualifying = take_matcher_string(&mut arguments, "qualifying")?;
                validate_operator_run(&family, &characters, &fences, &trailing, &qualifying)?;
                ClosedMatcher::OperatorRun {
                    characters,
                    fences,
                    trailing,
                    qualifying,
                }
            }
            "physical_line" => ClosedMatcher::PhysicalLine {
                prefix: take_matcher_string(&mut arguments, "prefix")?,
            },
            _ => {
                return Err(syn::Error::new(
                    family.span(),
                    format!("unsupported closed matcher `{family}`"),
                ));
            }
        };
        if let Some((name, (span, _))) = arguments.into_iter().next() {
            return Err(syn::Error::new(
                span,
                format!("unsupported `{name}` closed-matcher argument"),
            ));
        }
        Ok(Self { matcher, priority })
    }
}

/// Proves that a next-exclusion predicate is runtime-compilable and can consume one whole scalar.
fn matches_one_complete_scalar(pattern: &str) -> bool {
    if regex_automata::meta::Regex::new(pattern).is_err() {
        return false;
    }
    let Ok(dfa) = dense::DFA::new(pattern) else {
        return false;
    };
    let input = Input::new(b"").anchored(Anchored::Yes);
    let Ok(start) = dfa.start_state_forward(&input) else {
        return false;
    };
    if dfa_accepts_state(&dfa, start) {
        return false;
    }
    (0..=char::MAX as u32)
        .filter_map(char::from_u32)
        .any(|character| {
            let mut encoded = [0; 4];
            let mut state = start;
            for &byte in character.encode_utf8(&mut encoded).as_bytes() {
                state = dfa.next_state(state, byte);
                if dfa.is_dead_state(state) || dfa.is_quit_state(state) {
                    return false;
                }
            }
            dfa_accepts_state(&dfa, state)
        })
}

/// Removes one required string argument from a closed matcher constructor.
fn take_matcher_string(
    arguments: &mut BTreeMap<String, (Span, syn::Expr)>,
    name: &str,
) -> syn::Result<String> {
    let Some((span, value)) = arguments.remove(name) else {
        return Err(syn::Error::new(
            Span::call_site(),
            format!("closed matcher requires `{name} = \"...\"`"),
        ));
    };
    let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(value),
        ..
    }) = value
    else {
        return Err(syn::Error::new(
            span,
            format!("closed matcher `{name}` must be a string literal"),
        ));
    };
    Ok(value.value())
}

/// Removes one optional unsigned integer argument from a closed matcher constructor.
fn take_matcher_u32(
    arguments: &mut BTreeMap<String, (Span, syn::Expr)>,
    name: &str,
) -> syn::Result<Option<u32>> {
    let Some((span, value)) = arguments.remove(name) else {
        return Ok(None);
    };
    let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Int(value),
        ..
    }) = value
    else {
        return Err(syn::Error::new(
            span,
            format!("closed matcher `{name}` must be an unsigned integer literal"),
        ));
    };
    value.base10_parse().map(Some)
}

/// Removes one required string-array argument from a closed matcher constructor.
fn take_matcher_strings(
    arguments: &mut BTreeMap<String, (Span, syn::Expr)>,
    name: &str,
) -> syn::Result<Vec<String>> {
    let Some((span, value)) = arguments.remove(name) else {
        return Err(syn::Error::new(
            Span::call_site(),
            format!("closed matcher requires `{name} = [\"...\"]`"),
        ));
    };
    let syn::Expr::Array(array) = value else {
        return Err(syn::Error::new(
            span,
            format!("closed matcher `{name}` must be an array of string literals"),
        ));
    };
    array
        .elems
        .into_iter()
        .map(|element| match element {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(value),
                ..
            }) => Ok(value.value()),
            other => Err(syn::Error::new(
                other.span(),
                format!("closed matcher `{name}` accepts only string literals"),
            )),
        })
        .collect()
}

/// Validates the generic language constraints of an operator-run matcher.
fn validate_operator_run(
    family: &syn::Ident,
    characters: &str,
    fences: &[String],
    trailing: &str,
    qualifying: &str,
) -> syn::Result<()> {
    let unique_ascii = |value: &str| {
        value.is_ascii() && value.bytes().collect::<BTreeSet<_>>().len() == value.len()
    };
    if characters.is_empty() || !unique_ascii(characters) {
        return Err(syn::Error::new(
            family.span(),
            "operator-run characters must be a nonempty set of unique ASCII bytes",
        ));
    }
    if !unique_ascii(trailing)
        || !unique_ascii(qualifying)
        || !trailing
            .bytes()
            .all(|byte| characters.as_bytes().contains(&byte))
        || !qualifying
            .bytes()
            .all(|byte| characters.as_bytes().contains(&byte))
    {
        return Err(syn::Error::new(
            family.span(),
            "operator-run trailing and qualifying sets must contain unique admitted ASCII bytes",
        ));
    }
    if fences.iter().any(|fence| {
        fence.is_empty()
            || !fence.is_ascii()
            || !fence
                .bytes()
                .all(|byte| characters.as_bytes().contains(&byte))
    }) {
        return Err(syn::Error::new(
            family.span(),
            "operator-run fences must be nonempty admitted ASCII byte strings",
        ));
    }
    Ok(())
}

#[derive(Clone)]
/// Carries one named central admission declaration before source locations are retained.
struct CentralAdmissionSyntax {
    /// Preserves the authored admission name.
    name: syn::Ident,
    /// Preserves the authored set expression.
    expression: AdmissionExpressionSyntax,
}

#[derive(Clone)]
/// Carries one parsed admission expression.
struct AdmissionExpressionSyntax {
    /// Supplies the initial operand.
    initial: AdmissionOperandSyntax,
    /// Supplies its left-associative operations.
    operations: Vec<AdmissionOperationSyntax>,
}

#[derive(Clone)]
/// Classifies parsed admission operands.
enum AdmissionOperandSyntax {
    /// Names one fixed token, metadata set, named admission, or the keyword universe.
    Name(syn::Ident),
    /// Lists explicit fixed-token names and retains the braces' span.
    Set(Vec<syn::Ident>, Span),
}

#[derive(Clone)]
/// Carries one parsed admission operation.
struct AdmissionOperationSyntax {
    /// Selects union or subtraction behavior.
    kind: AdmissionOperationKind,
    /// Supplies the right-hand operand.
    operand: AdmissionOperandSyntax,
    /// Retains the operator span for diagnostics.
    span: Span,
}

/// Retains source locations while converting one parsed admission declaration.
fn pending_admission(
    resolved: &ResolvedSourceGrammar,
    origin: SourceOrigin,
    syntax: CentralAdmissionSyntax,
) -> PendingAdmissionDefinition {
    let reference = |ident: syn::Ident| AdmissionReference {
        name: ident.unraw().to_string(),
        site: source_site(resolved, origin, span_range(ident.span())),
    };
    let operand = |operand: AdmissionOperandSyntax| match operand {
        AdmissionOperandSyntax::Name(name) => PendingAdmissionOperand::Name(reference(name)),
        AdmissionOperandSyntax::Set(members, span) => PendingAdmissionOperand::Set {
            members: members.into_iter().map(reference).collect(),
            site: source_site(resolved, origin, span_range(span)),
        },
    };
    PendingAdmissionDefinition {
        name: syntax.name.unraw().to_string(),
        site: source_site(resolved, origin, span_range(syntax.name.span())),
        expression: PendingAdmissionExpression {
            initial: operand(syntax.expression.initial),
            operations: syntax
                .expression
                .operations
                .into_iter()
                .map(|operation| PendingAdmissionOperation {
                    kind: operation.kind,
                    operand: operand(operation.operand),
                    site: source_site(resolved, origin, span_range(operation.span)),
                })
                .collect(),
        },
    }
}

impl Parse for CentralTokensSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut fixed = Vec::new();
        let mut content = Vec::new();
        let mut matchers = Vec::new();
        let mut non_token_lexer_records = Vec::new();
        let mut non_token_matchers = Vec::new();
        let mut obsolete_post_lex = Vec::new();
        let mut categories = Vec::new();
        let mut flags = Vec::new();
        let mut admissions = Vec::new();
        let mut lookaheads = Vec::new();
        let mut precedence = Vec::new();
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
                    obsolete_post_lex.push(input.parse()?);
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
                        let (category, keyword_flags) = parse_keyword_metadata(&body)?;
                        let pretty_spacing = parse_fixed_pretty_spacing(&body, &spelling)?;
                        fixed.push(CentralFixedSyntax {
                            name,
                            spelling,
                            authored_class,
                            pretty_spacing,
                            category,
                            flags: keyword_flags,
                        });
                    }
                } else if section == "categories" || section == "flags" {
                    let destination = if section == "categories" {
                        &mut categories
                    } else {
                        &mut flags
                    };
                    destination.extend(
                        syn::punctuated::Punctuated::<syn::Ident, Token![,]>::parse_terminated(
                            &body,
                        )?,
                    );
                } else if section == "admissions" {
                    while !body.is_empty() {
                        let name = syn::Ident::parse_any(&body)?;
                        body.parse::<Token![=]>()?;
                        let expression = body.parse()?;
                        admissions.push(CentralAdmissionSyntax { name, expression });
                        if body.peek(Token![,]) {
                            body.parse::<Token![,]>()?;
                        } else if !body.is_empty() {
                            return Err(body.error("expected `,` after admission declaration"));
                        }
                    }
                } else if section == "precedence" {
                    precedence.push(parse_precedence_block(section, &body)?);
                } else if section == "lookahead" {
                    while !body.is_empty() {
                        let merged = syn::Ident::parse_any(&body)?;
                        body.parse::<Token![=]>()?;
                        let token = syn::Ident::parse_any(&body)?;
                        let before = syn::Ident::parse_any(&body)?;
                        if before != "before" {
                            return Err(syn::Error::new(
                                before.span(),
                                "expected `before` after the merged token name",
                            ));
                        }
                        let set;
                        syn::braced!(set in body);
                        let triggers =
                            syn::punctuated::Punctuated::<syn::Ident, Token![,]>::parse_terminated(
                                &set,
                            )?
                            .into_iter()
                            .collect();
                        lookaheads.push(CentralLookaheadSyntax {
                            merged,
                            token,
                            triggers,
                        });
                        if body.peek(Token![,]) {
                            body.parse::<Token![,]>()?;
                        } else if !body.is_empty() {
                            return Err(
                                body.error("expected `,` after lookahead filter declaration")
                            );
                        }
                    }
                } else if section == "matchers" || section == "ignore" || section == "trivia" {
                    while !body.is_empty() {
                        let attrs = body.call(syn::Attribute::parse_outer)?;
                        let name = syn::Ident::parse_any(&body)?;
                        body.parse::<Token![=>]>()?;
                        if section == "trivia" && body.peek(LitStr) {
                            if let Some(attr) = attrs.first() {
                                return Err(syn::Error::new(
                                    attr.span(),
                                    "`trivia` entries accept no attributes",
                                ));
                            }
                            non_token_lexer_records.push(CentralNonTokenLexerRecordSyntax {
                                pattern: body.parse()?,
                                outcome: NonTokenLexerOutcome::RetainedTrivia {
                                    name: name.unraw().to_string(),
                                },
                            });
                        } else {
                            let matcher = body.parse()?;
                            if section == "matchers" {
                                let metas = attrs
                                    .iter()
                                    .map(|attr| attr.meta.clone())
                                    .collect::<Vec<_>>();
                                matchers.push(CentralMatcherSyntax {
                                    name,
                                    matcher,
                                    metas,
                                });
                            } else {
                                if let Some(attr) = attrs.first() {
                                    return Err(syn::Error::new(
                                        attr.span(),
                                        "`ignore`/`trivia` entries accept no attributes",
                                    ));
                                }
                                let outcome = if section == "ignore" {
                                    NonTokenLexerOutcome::NonEmitting
                                } else {
                                    NonTokenLexerOutcome::RetainedTrivia {
                                        name: name.unraw().to_string(),
                                    }
                                };
                                non_token_matchers.push(CentralNonTokenMatcherSyntax {
                                    name,
                                    matcher,
                                    outcome,
                                });
                            }
                        }
                        if body.peek(Token![,]) {
                            body.parse::<Token![,]>()?;
                        } else if !body.is_empty() {
                            return Err(body.error("expected `,` after closed matcher"));
                        }
                    }
                } else if section == "literals" || section == "lexer_tokens" {
                    while !body.is_empty() {
                        let attrs = body.call(syn::Attribute::parse_outer)?;
                        let name = syn::Ident::parse_any(&body)?;
                        body.parse::<Token![=>]>()?;
                        let pattern = body.parse()?;
                        let obsolete_callback = if body.peek(syn::Ident) {
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
                        let metas = attrs.iter().map(|attr| attr.meta.clone()).collect();
                        content.push(CentralContentSyntax {
                            name,
                            pattern,
                            obsolete_callback,
                            metas,
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
            matchers,
            non_token_lexer_records,
            non_token_matchers,
            obsolete_post_lex,
            categories,
            flags,
            admissions,
            lookaheads,
            precedence,
        })
    }
}

/// Parses the body of one `precedence { ... }` block: levels `left(bp = N) { A, B }`,
/// `right(bp = N) { ... }` or `nonassoc(bp = N) { ... }` from lowest to highest binding power,
/// comma-separated. The shape checks that need no token catalog are made here: the
/// associativity keyword, `bp` as a `u8`, strictly ascending binding powers, at least one name
/// per level, and a name listed at most once in the block.
fn parse_precedence_block(
    keyword: syn::Ident,
    body: ParseStream<'_>,
) -> syn::Result<CentralPrecedenceSyntax> {
    let mut levels: Vec<CentralPrecedenceLevelSyntax> = Vec::new();
    let mut seen = BTreeMap::<String, ()>::new();
    while !body.is_empty() {
        let associativity_ident = syn::Ident::parse_any(body)?;
        let associativity = match associativity_ident.to_string().as_str() {
            "left" => crate::PrecedenceAssociativity::Left,
            "right" => crate::PrecedenceAssociativity::Right,
            "nonassoc" => crate::PrecedenceAssociativity::NonAssociative,
            _ => {
                return Err(syn::Error::new(
                    associativity_ident.span(),
                    "a precedence level must be left, right, or nonassoc",
                ));
            }
        };
        let options;
        syn::parenthesized!(options in body);
        let key = syn::Ident::parse_any(&options)?;
        if key != "bp" {
            return Err(syn::Error::new(
                key.span(),
                "a precedence level takes exactly `bp = N`, its binding power",
            ));
        }
        options.parse::<Token![=]>()?;
        let literal: syn::LitInt = options.parse()?;
        let binding_power = literal.base10_parse::<u8>().map_err(|_| {
            syn::Error::new(
                literal.span(),
                "a precedence level's binding power must be a u8 from 0 to 255",
            )
        })?;
        if !options.is_empty() {
            return Err(options.error("a precedence level takes exactly `bp = N`"));
        }
        if let Some(previous) = levels.last()
            && binding_power <= previous.binding_power
        {
            return Err(syn::Error::new(
                literal.span(),
                format!(
                    "precedence levels are declared from lowest to highest binding power; \
                     bp = {binding_power} does not exceed the previous level's bp = {}",
                    previous.binding_power
                ),
            ));
        }
        let members;
        syn::braced!(members in body);
        let mut names = Vec::new();
        while !members.is_empty() {
            let name = syn::Ident::parse_any(&members)?;
            if seen.insert(name.unraw().to_string(), ()).is_some() {
                return Err(syn::Error::new(
                    name.span(),
                    format!("`{name}` appears more than once in the precedence declaration"),
                ));
            }
            names.push(name);
            if members.peek(Token![,]) {
                members.parse::<Token![,]>()?;
            } else if !members.is_empty() {
                return Err(members.error("expected `,` between precedence level names"));
            }
        }
        if names.is_empty() {
            return Err(syn::Error::new(
                associativity_ident.span(),
                "a precedence level must name at least one token",
            ));
        }
        levels.push(CentralPrecedenceLevelSyntax {
            associativity,
            binding_power,
            span: associativity_ident.span(),
            names,
        });
        if body.peek(Token![,]) {
            body.parse::<Token![,]>()?;
        } else if !body.is_empty() {
            return Err(body.error("expected `,` after a precedence level"));
        }
    }
    Ok(CentralPrecedenceSyntax { keyword, levels })
}

/// Parses the optional `in CATEGORY + flag` keyword metadata tail.
fn parse_keyword_metadata(
    input: ParseStream<'_>,
) -> syn::Result<(Option<syn::Ident>, Vec<syn::Ident>)> {
    let category = if input.peek(Token![in]) {
        input.parse::<Token![in]>()?;
        Some(syn::Ident::parse_any(input)?)
    } else {
        None
    };
    let mut flags = Vec::new();
    while input.peek(Token![+]) {
        input.parse::<Token![+]>()?;
        flags.push(syn::Ident::parse_any(input)?);
    }
    Ok((category, flags))
}

impl Parse for AdmissionExpressionSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let initial = parse_admission_operand(input)?;
        let mut operations = Vec::new();
        while !input.is_empty() && !input.peek(Token![,]) {
            let (kind, span) = if input.peek(Token![+]) {
                let operator: Token![+] = input.parse()?;
                (AdmissionOperationKind::Union, operator.span)
            } else if input.peek(Token![-]) {
                let operator: Token![-] = input.parse()?;
                (AdmissionOperationKind::Subtract, operator.span)
            } else {
                return Err(input.error("admission expressions accept only `+` and `-` operations"));
            };
            operations.push(AdmissionOperationSyntax {
                kind,
                operand: parse_admission_operand(input)?,
                span,
            });
        }
        Ok(Self {
            initial,
            operations,
        })
    }
}

/// Parses one named or explicit-set admission operand.
fn parse_admission_operand(input: ParseStream<'_>) -> syn::Result<AdmissionOperandSyntax> {
    if input.peek(syn::token::Brace) {
        let body;
        let brace = syn::braced!(body in input);
        let members =
            syn::punctuated::Punctuated::<syn::Ident, Token![,]>::parse_terminated(&body)?
                .into_iter()
                .collect();
        Ok(AdmissionOperandSyntax::Set(members, brace.span.join()))
    } else {
        Ok(AdmissionOperandSyntax::Name(syn::Ident::parse_any(input)?))
    }
}

/// Parses fixed pretty spacing.
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

/// Derives the skip central entry tail from the retained source model and its correlations.
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
/// Carries the token sequence retained from authored source structure.
struct TokenSequence(Vec<TokenSequenceEntry>);

#[derive(Clone)]
/// Classifies the token sequence entry forms retained in the source model.
enum TokenSequenceEntry {
    /// Identifies the definition authored-source form.
    Definition(LitStr),
    /// Identifies the reference authored-source form.
    Reference(syn::Ident),
    /// Identifies the this authored-source form.
    This(Span),
    /// Identifies one erased, atomic optional fixed-token group.
    Optional {
        /// Supplies the non-empty unnested fixed-token sequence.
        tokens: Vec<TokenSequenceEntry>,
        /// Locates the authored `optional` entry.
        span: Span,
    },
}

/// Classifies the fixed token syntax forms retained in the source model.
enum FixedTokenSyntax {
    /// Identifies the definition authored-source form.
    Definition(LitStr),
    /// Identifies the reference authored-source form.
    Reference(syn::Ident),
}

/// Carries the pratt syntax retained from authored source structure.
struct PrattSyntax {
    /// Supplies the position used to correlate authored source with normalized grammar.
    position: SourcePrattPosition,
}

/// Carries field-local Pratt extension restriction syntax.
struct PrattFieldSyntax {
    /// Names the variants excluded at this field boundary, of any Pratt position.
    excluded: Vec<syn::Ident>,
}

impl Parse for PrattFieldSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let pratt = syn::Ident::parse_any(input)?;
        if pratt != "pratt" {
            return Err(syn::Error::new(
                pratt.span(),
                "field Pratt restriction must start with pratt",
            ));
        }
        let pratt_content;
        syn::parenthesized!(pratt_content in input);
        let exclude = syn::Ident::parse_any(&pratt_content)?;
        if exclude != "exclude" {
            return Err(syn::Error::new(
                exclude.span(),
                "field Pratt restriction supports only exclude(...)",
            ));
        }
        let excluded_content;
        syn::parenthesized!(excluded_content in pratt_content);
        let excluded = syn::punctuated::Punctuated::<syn::Ident, Token![,]>::parse_terminated(
            &excluded_content,
        )?
        .into_iter()
        .collect::<Vec<_>>();
        if excluded.is_empty() {
            return Err(excluded_content.error("exclude(...) requires at least one Pratt variant"));
        }
        if !pratt_content.is_empty() || !input.is_empty() {
            return Err(input.error("unexpected tokens after field Pratt restriction"));
        }
        let mut names = BTreeSet::new();
        for ident in &excluded {
            if !names.insert(ident.unraw().to_string()) {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("duplicate excluded Pratt variant `{ident}`"),
                ));
            }
        }
        Ok(Self { excluded })
    }
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

/// Carries the separator syntax retained from authored source structure.
struct SeparatorSyntax {
    /// Supplies the token used to correlate authored source with normalized grammar.
    token: FixedTokenSyntax,
    /// Supplies the optional trailing used to correlate authored source with normalized grammar.
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

/// Carries the lex syntax retained from authored source structure.
struct LexSyntax {
    /// Preserves the authored lexical pattern for validation and generation.
    pattern: Option<LitStr>,
    /// Binds the field to a centrally declared closed matcher.
    matcher: bool,
    /// Human-readable content-token label preserved for generated diagrams.
    diagram: Option<LitStr>,
    /// Retains an obsolete callback path only long enough to diagnose its source site.
    obsolete_callback: Option<syn::Path>,
    /// Preserves authored lexical priority for deterministic matcher ordering.
    priority: Option<u32>,
    /// Supplies the admissions used to correlate authored source with normalized grammar.
    admissions: Vec<syn::Ident>,
    /// Supplies the display used to correlate authored source with normalized grammar.
    display: Option<LitStr>,
}

impl Parse for LexSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut pattern = None;
        let mut matcher = false;
        let mut diagram = None;
        let mut obsolete_callback = None;
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
            } else if key == "matcher" {
                if matcher {
                    return Err(syn::Error::new(key.span(), "duplicate lex matcher marker"));
                }
                matcher = true;
            } else if key == "diagram" {
                input.parse::<Token![=]>()?;
                if diagram.replace(input.parse()?).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate diagram label"));
                }
            } else if key == "callback" {
                input.parse::<Token![=]>()?;
                if obsolete_callback.replace(input.parse()?).is_some() {
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
        if pattern.is_some() == matcher {
            return Err(
                input.error("#[lex] requires exactly one of `pattern = r\"...\"` or `matcher`")
            );
        }
        Ok(Self {
            pattern,
            matcher,
            diagram,
            obsolete_callback,
            priority,
            admissions: admissions.unwrap_or_default(),
            display,
        })
    }
}

/// Carries the node trait delta syntax retained from authored source structure.
struct NodeTraitDeltaSyntax {
    /// Supplies the additions used to correlate authored source with normalized grammar.
    additions: Vec<syn::Path>,
    /// Supplies the omissions used to correlate authored source with normalized grammar.
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
                if ident == "optional" && input.peek(syn::token::Paren) {
                    let content;
                    syn::parenthesized!(content in input);
                    let nested = TokenSequence::parse(&content)?;
                    if nested.0.is_empty() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "optional(...) must contain a non-empty fixed-token sequence",
                        ));
                    }
                    if nested.0.iter().any(|entry| {
                        matches!(
                            entry,
                            TokenSequenceEntry::This(_) | TokenSequenceEntry::Optional { .. }
                        )
                    }) {
                        return Err(syn::Error::new(
                            ident.span(),
                            "optional(...) accepts only an unnested fixed-token sequence",
                        ));
                    }
                    entries.push(TokenSequenceEntry::Optional {
                        tokens: nested.0,
                        span: ident.span(),
                    });
                } else if ident == "this" {
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

/// Derives the configured trait recipe from the retained source model and its correlations.
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

/// Reassigns element identities after sorting Nodes and token uses by authored location.
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
    for definition in tokens.fixed_by_name.values_mut() {
        *definition = definition_remap[definition.index()];
    }
    for definitions in tokens.content_by_name.values_mut() {
        for definition in definitions.iter_mut() {
            *definition = definition_remap[definition.index()];
        }
    }
    for definitions in tokens.named_admissions.values_mut() {
        for definition in definitions.iter_mut() {
            *definition = definition_remap[definition.index()];
        }
        definitions.sort_unstable();
        definitions.dedup();
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
    for filter in &mut tokens.lookahead_filters {
        filter.token = definition_remap[filter.token.index()];
        for trigger in &mut filter.triggers {
            *trigger = definition_remap[trigger.index()];
        }
    }
    tokens
        .lookahead_filters
        .sort_by_key(|filter| canonical_source_order_key(&filter.site));

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

/// Derives the reassign element ids from the retained source model and its correlations.
fn reassign_element_ids(mut elements: Vec<GrammarElement>) -> Vec<GrammarElement> {
    for (index, element) in elements.iter_mut().enumerate() {
        element.set_id(GrammarElementId::next(index));
    }
    elements
}

/// Derives the assign ordered elements from the retained source model and its correlations.
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
            GrammarElement::OptionalFixedTokenGroup { tokens, .. } => {
                for fixed in tokens {
                    fixed.token_use = use_remap[fixed.token_use.index()];
                }
            }
            GrammarElement::SemanticField {
                projection,
                attachment,
                separator,
                ..
            } => {
                if let SemanticFieldProjection::FixedPresence { tokens } = projection {
                    for fixed in tokens {
                        fixed.token_use = use_remap[fixed.token_use.index()];
                    }
                }
                if let Some(attachment) = attachment {
                    for fixed in attachment.before.iter_mut().chain(&mut attachment.after) {
                        fixed.remap_token_uses(use_remap);
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
    use std::fmt::Write as _;
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        CentralTokensSyntax, ClosedMatcherSyntax, ConfiguredTraitOwnership, ConfiguredTraitRecipe,
        ContentTextStorage, DeclaredRepetition, GrammarElement, GrammarElementKind, LexSyntax,
        NodeBody, NodeShape, PRETTY_BOUNDARY_LEFT_TRANSITION_BUDGET,
        PRETTY_BOUNDARY_PRODUCT_TRANSITION_BUDGET, PRETTY_BOUNDARY_REACHABLE_TRANSITION_BUDGET,
        PrettyBoundaryProductCounts, PrettyBoundaryProver, PrettyPatternAutomaton, PrettySpacing,
        SourceModelDiagnosticKind, SourceModelInput, SourcePrattPosition, TokenDefinitionKind,
        TokenUseRole, build_pretty_left_product, compiled_patterns_cross,
        compiled_patterns_cross_counted, compiled_patterns_cross_old_oracle,
        compiled_right_crosses, fixed_lexical_pattern, fixed_spellings_cross,
        fixed_then_pattern_intersects_pattern, lower_source_model,
        pattern_then_fixed_intersects_pattern, pretty_boundary_joint_byte_representatives,
        pretty_boundary_left_transition_len, pretty_boundary_product_len,
        pretty_boundary_reachable_transition_len,
    };
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::{ElementId, RustEdition};
    use crate::resolution::{
        ResolutionInput, ResolvedFieldStyle, ResolvedNodeBody, resolve_local_semantics,
    };
    use crate::source_syntax::CargoCfgSnapshot;

    #[test]
    fn fixed_spelling_crossing_is_exact_and_honors_ascii_word_equivalence() {
        assert!(fixed_spellings_cross(
            "<",
            "=tail",
            "<=",
            crate::TextEquivalence::Sensitive,
        ));
        assert!(!fixed_spellings_cross(
            "<",
            "=tail",
            "<",
            crate::TextEquivalence::Sensitive,
        ));
        assert!(fixed_spellings_cross(
            "word",
            ":tail",
            "WoRd:",
            crate::TextEquivalence::AsciiInsensitive,
        ));
        assert!(!fixed_spellings_cross(
            "word",
            ":tail",
            "other:",
            crate::TextEquivalence::AsciiInsensitive,
        ));
    }

    #[test]
    fn fixed_spelling_crossing_matches_the_dfa_oracle_matrix() {
        let compile = |pattern: &str| {
            let dfa = regex_automata::dfa::dense::DFA::new(pattern).expect("a valid fixed pattern");
            let input = regex_automata::Input::new(b"").anchored(regex_automata::Anchored::Yes);
            let start = regex_automata::dfa::Automaton::start_state_forward(&dfa, &input)
                .expect("an anchored start state");
            PrettyPatternAutomaton::new(dfa, start)
        };
        let spellings = ["a", "A", "ab", ":", "a:", "<="];
        for equivalence in [
            crate::TextEquivalence::Sensitive,
            crate::TextEquivalence::AsciiInsensitive,
        ] {
            for left in spellings {
                for right in spellings {
                    for candidate in spellings {
                        let left_pattern = compile(&fixed_lexical_pattern(left, equivalence));
                        let right_pattern = compile(&fixed_lexical_pattern(right, equivalence));
                        let candidate_pattern =
                            compile(&fixed_lexical_pattern(candidate, equivalence));
                        assert_eq!(
                            fixed_spellings_cross(left, right, candidate, equivalence),
                            compiled_patterns_cross(
                                &left_pattern,
                                &right_pattern,
                                &candidate_pattern,
                            ),
                            "left={left:?}, right={right:?}, candidate={candidate:?}, equivalence={equivalence:?}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn next_exclusion_requires_a_runtime_compilable_nonempty_whole_scalar_predicate() {
        for valid in [
            r#"next_exclusion(pattern = r"[0-9]+", excluded = r"[A-Z]")"#,
            r#"next_exclusion(pattern = r"[0-9]+", excluded = r"\p{Greek}")"#,
        ] {
            syn::parse_str::<ClosedMatcherSyntax>(valid)
                .unwrap_or_else(|error| panic!("valid predicate was rejected: {valid}: {error}"));
        }
        for invalid in [
            r#"next_exclusion(pattern = r"[0-9]+", excluded = r"")"#,
            r#"next_exclusion(pattern = r"[0-9]+", excluded = r"a*")"#,
            r#"next_exclusion(pattern = r"[0-9]+", excluded = r"ab")"#,
            r#"next_exclusion(pattern = r"[0-9]+", excluded = r"(")"#,
        ] {
            let error = syn::parse_str::<ClosedMatcherSyntax>(invalid)
                .err()
                .expect("invalid next-exclusion predicate must be rejected");
            assert!(
                error.to_string().contains("complete, nonempty scalar"),
                "{invalid}: {error}"
            );
        }
    }

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

    fn assert_code(report: &super::SourceModelReport, kind: SourceModelDiagnosticKind, code: &str) {
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == kind)
            .unwrap_or_else(|| panic!("missing {kind:?} in {:?}", report.diagnostics));
        assert_eq!(
            crate::source_analysis::source_model_code(diagnostic.kind),
            code
        );
    }

    #[test]
    fn lowers_positional_tokens_and_presence_nodes_in_source_order() {
        let report = model(
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[tok("(", this, ")")]
    struct Root {
        value: Present,
    }
}

recursa::ast_node! {

    #[tok("PRESENT")]
    struct Present;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    struct Root {
        #[tok("?", this, "!")]
        value: Option<Present>,
    }
}

recursa::ast_node! {

    #[tok("PRESENT")]
    struct Present;
}
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
            model.token_definitions[model.token_uses[attachment.before[0]
                .first()
                .expect("required prefix token")
                .token_use
                .index()]
            .definition
            .index()]
            .name,
            "?",
        );
        assert_eq!(
            model.token_definitions[model.token_uses[attachment.after[0]
                .last()
                .expect("required suffix token")
                .token_use
                .index()]
            .definition
            .index()]
            .name,
            "!",
        );
    }

    #[test]
    fn source_modeled_grammar_retains_visibility_and_field_styles_for_emission() {
        let report = model(
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    pub struct Named {
        value: Leaf,
    }
}

recursa::ast_node! {

    pub(crate) struct Tuple(Leaf);
}

recursa::ast_node! {

    struct Unit;
}

recursa::ast_node! {

    pub enum Choice {
        Named { value: Leaf },
        Tuple(Leaf),
        Unit,
    }
}

recursa::ast_node! {

    struct Leaf;
}
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
        assert_eq!(unit.visibility, syn::parse_quote!(pub));
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
            r#"recursa::grammar! {
    module = crate,
    derives(Debug, Clone, Visit),
}

recursa::ast_node! {

    #[node(derive(Pretty))]
    #[omit(Clone)]
    struct Root {
        child: Leaf,
    }
}

recursa::ast_node! {

    struct Leaf;
}
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
            r#"recursa::grammar! { module = crate, derives(Clone) }

recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
        word: Word,
    }
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
        assert_eq!(content.diagram_label, None);
        assert_eq!(content.text, ContentTextStorage::BorrowedOrOwnedMatchedText);
        assert_eq!(content.configured_traits.effective.len(), 1);
        assert_eq!(
            content.configured_traits.effective[0].recipe,
            ConfiguredTraitRecipe::Clone,
        );
    }

    #[test]
    fn content_diagram_labels_are_authored_once_and_conflicts_are_rejected() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"[0-9]+", diagram = "integer")]
        value: Number,
    }
}
"#,
        );
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let modeled = completed(report);
        let content = modeled
            .token_definitions
            .iter()
            .find_map(|definition| match &definition.kind {
                TokenDefinitionKind::Content(content) => Some(content),
                _ => None,
            })
            .expect("content token definition");
        assert_eq!(content.diagram_label.as_deref(), Some("integer"));

        let duplicate = match syn::parse_str::<LexSyntax>(
            r#"pattern = r"[0-9]+", diagram = "integer", diagram = "number""#,
        ) {
            Ok(_) => panic!("a declaration has one deterministic label"),
            Err(error) => error,
        };
        assert_eq!(duplicate.to_string(), "duplicate diagram label");

        let conflict = model(
            r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    struct First {
        #[lex(pattern = r"[0-9]+", diagram = "integer")]
        value: Number,
    }
}
recursa::ast_node! {

    struct Second {
        #[lex(pattern = r"[0-9]+", diagram = "number")]
        value: Number,
    }
}
"#,
        );
        assert!(conflict.source_modeled.is_none());
        assert!(
            conflict
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("conflicting content token")),
            "{:?}",
            conflict.diagnostics
        );
    }

    #[test]
    fn separators_are_repetition_metadata_and_model_optional_trailing_use() {
        let report = model(
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    struct Root {
        #[sep(",")]
        zero_or_more: zero_or_many!(Leaf),
        #[sep(",", trailing)]
        one_or_more: one_or_many!(Leaf),
    }
}

recursa::ast_node! {

    struct Leaf;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    enum Choice {
        #[tok("(", this, ")")]
        Wrapped(Leaf),
        #[tok("NULL")]
        Null,
    }
}

recursa::ast_node! {

    struct Leaf;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[pratt]
    enum Expr {
        #[parse(prefix, bp = 10)]
        #[tok("!", this)]
        Not(boxed!(Expr)),

        #[tok("(", this, this, ")")]
        Group(boxed!(Expr)),
    }
}

recursa::ast_node! {

    #[kwd("OLD")]
    struct Legacy;
}
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
            r#"recursa::grammar! { module = crate, derives(Debug, Default) }

recursa::ast_node! {
    #[derive(Debug)]
    #[node(derive(Clone))]
    #[omit(Clone)]
    struct Root;
}

recursa::ast_node! {

    #[node(derive(Clone))]
    #[node(derive(Eq))]
    struct InvalidDelta;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

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
}

recursa::ast_node! {

    #[lex(pattern = "misplaced")]
    struct Misplaced;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[tok(IF, this)]
    struct Root {
        value: First,
    }
}

recursa::ast_node! {

    #[tok("IF")]
    struct First;
}

recursa::ast_node! {

    #[tok("IF")]
    struct Second;
}
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
            r#"recursa::grammar! { module = crate, derives(Debug, Visit) }

recursa::ast_node! {

    #[cfg_attr(unix, node(derive(Clone)), omit(Debug))]
    struct Root;
}
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
            r#"my_recursa::grammar! {
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

my_recursa::ast_node! {

    struct Root;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    struct Root {
        #[lex(pattern = "word")]
        first: Word,
        later: Later,
    }
}

recursa::ast_node! {

    #[tok("LATE")]
    struct Later;
}
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
        let elements = model
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
            .collect::<Vec<_>>();
        assert!(
            elements
                .iter()
                .enumerate()
                .all(|(index, element)| element.id() == ElementId::next(index))
        );
    }

    #[test]
    fn content_admissions_resolve_through_the_fixed_token_namespace() {
        let report = model(
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    struct Root {
        #[lex(pattern = "word", admits(IF))]
        word: Word,
    }
}

recursa::ast_node! {

    #[tok("IF")]
    struct IfPresence;
}
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
    fn named_admissions_compile_transitive_unions_and_subtractions_to_canonical_membership() {
        let report = model(
            r#"recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}
recursa::tokens! {
    categories { UNRESERVED, RESERVED }
    flags { bare_label }
    keywords {
        WINDOW => "window" in UNRESERVED + bare_label,
        OTHER => "other" in UNRESERVED,
        ROWS => "rows" in RESERVED,
        RANGE => "range" in RESERVED,
        GROUPS => "groups" in RESERVED,
        NULL => "null" in RESERVED,
        TRUE => "true" in RESERVED,
        FALSE => "false" in RESERVED,
    }
    admissions {
        UnreservedNames = UNRESERVED,
        ColId = UnreservedNames + RESERVED,
        WindowRefName = ColId - { ROWS, RANGE, GROUPS },
        AllWordKinds = keywords,
        PsqlVariableName = AllWordKinds - { NULL, TRUE, FALSE },
        BareLabels = bare_label,
    }
}

recursa::ast_node! {

    struct Root {
        #[lex(pattern = "word", admits(WindowRefName))]
        window: WindowName,
        #[lex(pattern = "word", admits(PsqlVariableName))]
        variable: VariableName,
        #[lex(pattern = "word", admits(BareLabels))]
        bare: BareName,
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let admissions = model
            .token_definitions
            .iter()
            .filter_map(|definition| match &definition.kind {
                TokenDefinitionKind::Content(content) => Some((
                    definition.name.as_str(),
                    content
                        .admission_set
                        .iter()
                        .map(|admission| model.token_definitions[admission.index()].name.as_str())
                        .collect::<Vec<_>>(),
                )),
                TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            admissions["WindowName"],
            ["WINDOW", "OTHER", "NULL", "TRUE", "FALSE"]
        );
        assert_eq!(
            admissions["VariableName"],
            ["WINDOW", "OTHER", "ROWS", "RANGE", "GROUPS"]
        );
        assert_eq!(admissions["BareName"], ["WINDOW"]);
    }

    #[test]
    fn duplicate_admission_union_membership_warns_without_discarding_the_model() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! {
    categories { UNRESERVED }
    keywords { WINDOW => "window" in UNRESERVED }
    admissions { Names = UNRESERVED + { WINDOW } }
}
recursa::ast_node! {

    struct Root {
        #[lex(pattern = "word", admits(Names))]
        name: Name,
    }
}
"#,
        );

        assert!(report.source_modeled.is_some());
        assert_code(
            &report,
            SourceModelDiagnosticKind::DuplicateAdmissionMembership,
            "RCA3119",
        );
        assert_eq!(
            crate::build_diagnostic::diagnostic_code_spec(crate::RecursaDiagnosticCode::new(
                "RCA3119"
            ))
            .severity,
            crate::Severity::Warning,
        );
    }

    #[test]
    fn ascii_equivalent_admission_membership_uses_canonical_token_kinds() {
        let report = model(
            r#"recursa::grammar! { module = crate, keyword_matching = ascii_insensitive }
recursa::tokens! {
    categories { NAME }
    keywords {
        LOWER => "word" in NAME,
        UPPER => "WORD" in NAME,
    }
    admissions {
        Union = LOWER + UPPER,
        Difference = LOWER - UPPER,
    }
}
recursa::ast_node! {

    struct Root {
        #[lex(pattern = "word", admits(Union))]
        union: UnionName,
        #[lex(pattern = "word", admits(Difference))]
        difference: DifferenceName,
    }
}
"#,
        );

        assert!(report.source_modeled.is_some(), "{:?}", report.diagnostics);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.kind == SourceModelDiagnosticKind::DuplicateAdmissionMembership
                })
                .count(),
            1,
        );
        assert!(!report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::IneffectiveAdmissionSubtraction
        }));
        let model = completed(report);
        let membership = model
            .token_definitions
            .iter()
            .filter_map(|definition| match &definition.kind {
                TokenDefinitionKind::Content(content) => {
                    Some((definition.name.as_str(), content.admission_set.len()))
                }
                TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(membership["UnionName"], 1);
        assert_eq!(membership["DifferenceName"], 0);
    }

    #[test]
    fn ascii_equivalent_keywords_reject_contradictory_categories() {
        let report = model(
            r#"recursa::grammar! { module = crate, keyword_matching = ascii_insensitive }
recursa::tokens! {
    categories { FIRST, SECOND }
    keywords {
        LOWER => "word" in FIRST,
        UPPER => "WORD" in SECOND,
    }
}
recursa::ast_node! {
    struct Root;
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert_code(
            &report,
            SourceModelDiagnosticKind::ContradictoryAdmissionMetadata,
            "RCA3123",
        );
    }

    #[test]
    fn exact_admission_membership_keeps_ascii_case_distinct() {
        let report = model(
            r#"recursa::grammar! { module = crate, keyword_matching = exact }
recursa::tokens! {
    categories { NAME }
    keywords {
        LOWER => "word" in NAME,
        UPPER => "WORD" in NAME,
    }
    admissions { Both = LOWER + UPPER }
}
recursa::ast_node! {

    struct Root {
        #[lex(pattern = "word", admits(Both))]
        name: Name,
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let model = completed(report);
        let membership = model
            .token_definitions
            .iter()
            .find_map(|definition| match &definition.kind {
                TokenDefinitionKind::Content(content) => Some(content.admission_set.len()),
                TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => None,
            })
            .unwrap();
        assert_eq!(membership, 2);
    }

    #[test]
    fn invalid_admission_graphs_and_keyword_metadata_have_structured_diagnostics() {
        let cases = [
            (
                r#"recursa::grammar! { module = crate }
recursa::tokens! { admissions { Names = Missing } }
recursa::ast_node! {
    struct Root;
}
"#,
                SourceModelDiagnosticKind::InvalidAdmissionReference,
                "RCA3120",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::tokens! { admissions { First = Second, Second = First } }
recursa::ast_node! {
    struct Root;
}
"#,
                SourceModelDiagnosticKind::AdmissionCycle,
                "RCA3121",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::tokens! {
    categories { KNOWN }
    keywords { WINDOW => "window" in MISSING }
}
recursa::ast_node! {
    struct Root;
}
"#,
                SourceModelDiagnosticKind::InvalidKeywordCategory,
                "RCA3122",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::tokens! {
    categories { FIRST, SECOND }
    keywords {
        WINDOW => "window" in FIRST,
        WINDOW => "window" in SECOND,
    }
}
recursa::ast_node! {
    struct Root;
}
"#,
                SourceModelDiagnosticKind::ContradictoryAdmissionMetadata,
                "RCA3123",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::tokens! {
    categories { FIRST, SECOND }
    keywords { WINDOW => "window" in FIRST, OTHER => "other" in SECOND }
    admissions { Names = FIRST - SECOND }
}
recursa::ast_node! {
    struct Root;
}
"#,
                SourceModelDiagnosticKind::IneffectiveAdmissionSubtraction,
                "RCA3124",
            ),
        ];
        assert_eq!(
            cases.map(|(_, _, code)| code),
            ["RCA3120", "RCA3121", "RCA3122", "RCA3123", "RCA3124"],
        );

        for (source, kind, code) in cases {
            let report = model(source);
            assert!(report.source_modeled.is_none(), "{kind:?} was non-blocking");
            assert_code(&report, kind, code);
        }
    }

    #[test]
    fn lookahead_filters_compile_to_fixed_definitions_in_authored_order() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! {
    keywords {
        NOT => "not",
        BETWEEN => "between",
        IN => "in",
        LIKE => "like",
        NULLS => "nulls",
        FIRST => "first",
    }
    lookahead {
        NOT_LA = NOT before { BETWEEN, IN, LIKE },
        NULLS_LA = NULLS before { FIRST },
    }
}
recursa::ast_node! {
    struct Root;
}
"#,
        );
        assert!(report.source_modeled.is_some(), "{:?}", report.diagnostics);
        let model = completed(report);
        let by_name = |name: &str| {
            model
                .token_definitions
                .iter()
                .find(|definition| definition.name == name)
                .unwrap_or_else(|| panic!("no token definition named {name}"))
                .id
        };
        assert_eq!(model.lookahead_filters.len(), 2);
        assert_eq!(model.lookahead_filters[0].name, "NOT_LA");
        assert_eq!(model.lookahead_filters[0].token, by_name("NOT"));
        assert_eq!(
            model.lookahead_filters[0].triggers,
            [by_name("BETWEEN"), by_name("IN"), by_name("LIKE")]
        );
        assert_eq!(model.lookahead_filters[1].name, "NULLS_LA");
        assert_eq!(model.lookahead_filters[1].token, by_name("NULLS"));
        assert_eq!(model.lookahead_filters[1].triggers, [by_name("FIRST")]);
        // A merged kind is not a token definition: `#[tok(NOT_LA)]` stays unresolved.
        assert!(
            model
                .token_definitions
                .iter()
                .all(|definition| definition.name != "NOT_LA")
        );
    }

    #[test]
    fn invalid_lookahead_filters_have_structured_diagnostics() {
        let prelude = r#"
recursa::grammar! { module = crate }
recursa::tokens! {
    keywords { NOT => "not", BETWEEN => "between", IN => "in" }
    punctuation { PLUS => "+" }
"#;
        let cases = [
            (
                "lookahead { NOT_LA = MISSING before { BETWEEN } }",
                "lookahead filter `NOT_LA` merges `MISSING`, which is not a fixed token",
            ),
            (
                "lookahead { NOT_LA = NOT before { BETWEEN, MISSING } }",
                "lookahead filter `NOT_LA` names trigger `MISSING`, which is neither a fixed token nor one unique content token",
            ),
            (
                "lookahead { NOT_LA = NOT before { BETWEEN }, NOT_LA = NOT before { IN } }",
                "lookahead filter `NOT_LA` has more than one declaration",
            ),
            (
                "lookahead { BETWEEN = NOT before { IN } }",
                "lookahead filter `BETWEEN` has the name of a token definition",
            ),
            (
                "lookahead { NOT_LA = NOT before { BETWEEN }, NOT_LB = NOT before { BETWEEN, IN } }",
                "lookahead filter `NOT_LB` merges `NOT`, which lookahead filter `NOT_LA` already merges before overlapping trigger `BETWEEN`",
            ),
            (
                "lookahead { NOT_LA = NOT before { BETWEEN }, NOT_LB = NOT_LA before { IN } }",
                "lookahead filter `NOT_LB` merges `NOT_LA`, which is not a fixed token",
            ),
            (
                "lookahead { NOT_LA = NOT before { BETWEEN, BETWEEN } }",
                "lookahead filter `NOT_LA` names trigger `BETWEEN` more than once",
            ),
        ];
        for (block, message) in cases {
            let source = format!(
                r#"{prelude}
    {block}
}}
recursa::ast_node! {{
    struct Root;
}}
"#
            );
            let report = model(&source);
            assert!(report.source_modeled.is_none(), "{block} was non-blocking");
            assert_code(
                &report,
                SourceModelDiagnosticKind::InvalidLookaheadFilter,
                "RCA3132",
            );
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message == message),
                "{block}: expected {message:?} in {:?}",
                report.diagnostics
            );
        }
    }

    #[test]
    fn removed_greedy_helper_has_a_targeted_migration_diagnostic() {
        let report = model(
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[greedy(all)]
    struct Root;
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert_code(
            &report,
            SourceModelDiagnosticKind::InvalidGreedyAcceptance,
            "RCA3131",
        );
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.message
                == "#[greedy(...)] has been removed; make the grammar LALR(1) or resolve its LR conflict explicitly"
        }));
    }

    #[test]
    fn lookahead_filters_accept_disjoint_triggers_for_one_base_token() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! {
    keywords { NOT => "not", BETWEEN => "between", IN => "in" }
    lookahead {
        NOT_BEFORE_BETWEEN = NOT before { BETWEEN },
        NOT_BEFORE_IN = NOT before { IN },
    }
}
recursa::ast_node! {

    #[tok(NOT, BETWEEN, IN)]
    struct Root;
}
"#,
        );
        assert!(report.source_modeled.is_some(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.lookahead_filters.len(), 2);
        let first = &model.lookahead_filters[0];
        let second = &model.lookahead_filters[1];
        assert_eq!(first.token, second.token, "the filters share `NOT`");
        assert!(
            first
                .triggers
                .iter()
                .all(|trigger| !second.triggers.contains(trigger)),
            "the accepted filters have disjoint trigger sets"
        );
    }

    #[test]
    fn lookahead_filters_accept_one_unique_content_token_as_a_trigger() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! {
    keywords { UESCAPE => "uescape" }
    punctuation { RPAREN => ")" }
    lookahead {
        UESCAPE_LA = UESCAPE before { StringLit },
        RPAREN_LA = RPAREN before { StringLit },
    }
}
recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"'[^']*'")]
        value: StringLit,
    }
}
"#,
        );
        assert!(report.source_modeled.is_some(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.lookahead_filters.len(), 2);
        let filter = &model.lookahead_filters[0];
        assert_eq!(filter.name, "UESCAPE_LA");
        assert_eq!(
            model.token_definitions[filter.triggers[0].index()].name,
            "StringLit"
        );
        let punctuation = &model.lookahead_filters[1];
        assert_eq!(punctuation.name, "RPAREN_LA");
        assert_eq!(
            model.token_definitions[punctuation.triggers[0].index()].name,
            "StringLit"
        );
    }

    #[test]
    fn lookahead_filters_reject_a_content_name_with_distinct_lexical_identities() {
        let report = model(
            r##"recursa::grammar! { module = crate }
recursa::tokens! {
    keywords { UESCAPE => "uescape" }
    lookahead { UESCAPE_LA = UESCAPE before { StringLit } }
}
recursa::ast_node! {

    struct SingleQuoted {
        #[lex(pattern = r"'[^']*'")]
        value: StringLit,
    }
}
recursa::ast_node! {

    struct DoubleQuoted {
        #[lex(pattern = r#""[^"]*""#)]
        value: StringLit,
    }
}
"##,
        );
        assert!(report.source_modeled.is_none(), "{:?}", report.diagnostics);
        assert_code(
            &report,
            SourceModelDiagnosticKind::InvalidLookaheadFilter,
            "RCA3132",
        );
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == SourceModelDiagnosticKind::InvalidLookaheadFilter)
            .expect("the ambiguous content trigger is diagnosed");
        assert!(
            diagnostic
                .message
                .contains("more than one lexical or semantic token kind"),
            "{diagnostic:?}",
        );
        assert_eq!(
            diagnostic.sites.len(),
            3,
            "the trigger reference and both competing definitions are located",
        );
    }

    #[test]
    fn a_precedence_declaration_compiles_its_levels_and_overrides() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! {
    keywords { LIKE => "like", ESCAPE => "escape", SELECT => "select", LOCAL => "local", NOT => "not" }
    punctuation { LPAREN => "(", RPAREN => ")", DOT => ".", PLUS => "+" }
    lookahead { NOT_LA = NOT before { LIKE } }
    precedence {
        nonassoc(bp = 5) { LIKE, NOT_LA, LOCAL },
        nonassoc(bp = 6) { ESCAPE },
        nonassoc(bp = 7) { NameText, ESCAPE_ONLY },
        left(bp = 10) { PLUS },
        right(bp = 18) { UMINUS },
        left(bp = 20) { DOT },
    }
}

recursa::ast_node! {

    struct Name {
        #[lex(pattern = r"[a-z]+")]
        text: NameText,
    }
}
recursa::ast_node! {

    #[parse(prec = ESCAPE_ONLY)]
    struct Scope {
        #[tok(LOCAL)]
        name: Name,
    }
}
recursa::ast_node! {

    #[pratt]
    enum Expr {
        Name(Name),
        #[parse(prec = UMINUS)]
        Sub(#[tok(LPAREN, SELECT, this, RPAREN)] Name),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Expr), boxed!(Expr)),
    }
}
recursa::ast_node! {

    enum Stmt {
        #[parse(prec = LIKE)]
        Scoped(Scope),
        Bare(Expr),
    }
}
"#,
        );
        assert!(report.source_modeled.is_some(), "{:?}", report.diagnostics);
        let model = completed(report);
        let by_name = |name: &str| {
            model
                .token_definitions
                .iter()
                .find(|definition| definition.name == name)
                .unwrap_or_else(|| panic!("no token definition named {name}"))
                .id
        };
        let levels: Vec<(crate::PrecedenceAssociativity, u8, Vec<String>, Vec<String>)> = model
            .precedence
            .iter()
            .map(|level| {
                (
                    level.associativity,
                    level.binding_power,
                    level
                        .kinds
                        .iter()
                        .map(|reference| match reference {
                            super::PrecedenceKindReference::Token(id) => {
                                model.token_definitions[id.index()].name.clone()
                            }
                            super::PrecedenceKindReference::Merged(filter) => {
                                model.lookahead_filters[*filter].name.clone()
                            }
                        })
                        .collect(),
                    level.names.clone(),
                )
            })
            .collect();
        use crate::PrecedenceAssociativity::{Left, NonAssociative, Right};
        assert_eq!(
            levels,
            [
                // `NOT_LA` is no token definition: it resolves as the merged kind of the
                // `lookahead` filter, in its authored place between `LIKE` and `LOCAL`.
                (
                    NonAssociative,
                    5,
                    vec!["LIKE".to_owned(), "NOT_LA".to_owned(), "LOCAL".to_owned()],
                    vec![],
                ),
                (NonAssociative, 6, vec!["ESCAPE".to_owned()], vec![]),
                (
                    NonAssociative,
                    7,
                    vec!["NameText".to_owned()],
                    vec!["ESCAPE_ONLY".to_owned()]
                ),
                (Left, 10, vec!["PLUS".to_owned()], vec![]),
                (Right, 18, vec![], vec!["UMINUS".to_owned()]),
                (Left, 20, vec!["DOT".to_owned()], vec![]),
            ]
        );
        assert_eq!(
            model.precedence[0].kinds[0],
            super::PrecedenceKindReference::Token(by_name("LIKE"))
        );
        assert_eq!(
            model.precedence[0].kinds[1],
            super::PrecedenceKindReference::Merged(0)
        );
        let mut overrides: Vec<(usize, bool)> = model
            .precedence_overrides
            .iter()
            .map(|override_| {
                (
                    override_.level.index(),
                    matches!(override_.owner, super::PrecedenceOwner::Type(_)),
                )
            })
            .collect();
        overrides.sort_unstable();
        // `Scope` names level 2 (`ESCAPE_ONLY`), `Expr::Sub` level 4 (`UMINUS`) and
        // `Stmt::Scoped` level 0 (`LIKE`, a token, which an override may name too).
        assert_eq!(overrides, [(0, false), (2, true), (4, false)]);
    }

    #[test]
    fn lr_conflict_declarations_compile_source_owners_actions_and_lookaheads() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! { keywords { ORDER => "order" } punctuation { RPAREN => ")" } }
recursa::ast_node! {

    #[parse(lr_conflict(action = reduce, against = SelectClause::Simple, lookahead = { RPAREN }, expect = 2))]
    struct DirectSubquery;
}
recursa::ast_node! {

    enum SelectClause {
        #[parse(lr_conflict(action = shift, against = DirectSubquery, lookahead = { ORDER }, expect = 2))]
        Simple,
    }
}
"#,
        );
        assert!(report.source_modeled.is_some(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.lr_conflict_resolutions.len(), 2);
        assert_eq!(
            model.lr_conflict_resolutions[0].action,
            super::LrConflictAction::Reduce
        );
        assert!(matches!(
            model.lr_conflict_resolutions[0].against,
            super::LrRuleOwner::Variant(_)
        ));
        assert_eq!(
            model.lr_conflict_resolutions[1].action,
            super::LrConflictAction::Shift
        );
        assert!(matches!(
            model.lr_conflict_resolutions[1].against,
            super::LrRuleOwner::Type(_)
        ));
    }

    #[test]
    fn restricted_expressions_lower_to_their_admitted_base_variants() {
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! { punctuation { PLUS => "+", LPAREN => "(", RPAREN => ")" } }
recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[a-z]+")]
        pub text: NameText,
    }
}
recursa::ast_node! {

    pub enum Expr {
        Name(Name),
        Parenthesized(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
recursa::ast_node! {

    #[restricts(Expr)]
    pub enum BExpr {
        Name,
        Parenthesized,
    }
}
recursa::ast_node! {

    #[restricts(BExpr)]
    pub enum CExpr {
        Name,
    }
}
"#,
        );
        assert!(report.source_modeled.is_some(), "{:?}", report.diagnostics);
        let model = completed(report);
        assert_eq!(model.restrictions.len(), 2);
        assert_eq!(model.restrictions[0].name, "BExpr");
        assert_eq!(model.restrictions[0].variants.len(), 2);
        assert_eq!(model.restrictions[1].name, "CExpr");
        // A restriction of a restriction resolves to the root base Node and admits one of its
        // variants, so its rules still build the type the author declared.
        assert_eq!(model.restrictions[1].base, model.restrictions[0].base);
        assert_eq!(model.restrictions[1].variants.len(), 1);
    }

    #[test]
    fn invalid_restricted_expressions_have_structured_diagnostics() {
        const PRELUDE: &str = r#"recursa::grammar! { module = crate }
recursa::tokens! { punctuation { PLUS => "+" } }
recursa::ast_node! {

    pub struct Name {
        #[lex(pattern = r"[a-z]+")]
        pub text: NameText,
    }
}
recursa::ast_node! {

    pub enum Expr {
        Name(Name),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
"#;
        let with = |declaration: &str| model(&format!("{PRELUDE}{declaration}"));

        let fields = with(
            r#"recursa::ast_node! {

    #[restricts(Expr)]
    pub enum BExpr {
        Name(Name),
    }
}
"#,
        );
        assert!(fields.source_modeled.is_none());
        assert_code(
            &fields,
            SourceModelDiagnosticKind::RestrictedVariantHasFields,
            "RCA3135",
        );

        let unknown = with(
            r#"recursa::ast_node! {

    #[restricts(Expr)]
    pub enum BExpr {
        Missing,
    }
}
"#,
        );
        assert!(unknown.source_modeled.is_none());
        assert_code(
            &unknown,
            SourceModelDiagnosticKind::RestrictedVariantUnknown,
            "RCA3136",
        );

        let not_enum = with(
            r#"recursa::ast_node! {

    #[restricts(Name)]
    pub enum BExpr {
        Name,
    }
}
"#,
        );
        assert!(not_enum.source_modeled.is_none());
        assert_code(
            &not_enum,
            SourceModelDiagnosticKind::RestrictedBaseNotEnum,
            "RCA3137",
        );

        let cycle = with(
            r#"recursa::ast_node! {

    #[restricts(CExpr)]
    pub enum BExpr {
        Name,
    }
}
recursa::ast_node! {

    #[restricts(BExpr)]
    pub enum CExpr {
        Name,
    }
}
"#,
        );
        assert!(cycle.source_modeled.is_none());
        assert_code(
            &cycle,
            SourceModelDiagnosticKind::RestrictedBaseCycle,
            "RCA3138",
        );

        let configured = with(
            r#"recursa::ast_node! {

    #[node(derive(Pretty))]
    #[restricts(Expr)]
    pub enum BExpr {
        Name,
    }
}
"#,
        );
        assert!(configured.source_modeled.is_none());
        assert_code(
            &configured,
            SourceModelDiagnosticKind::RestrictedExpressionAsType,
            "RCA3139",
        );
    }

    #[test]
    fn malformed_lr_conflict_declarations_are_structured_errors() {
        let source = r#"recursa::grammar! { module = crate }
recursa::tokens! { keywords { ORDER => "order" } }
recursa::ast_node! {
    struct Other;
}
recursa::ast_node! {

    #[parse(lr_conflict(action = shift, against = Missing, lookahead = { ORDER }, expect = 1))]
    struct Root;
}
"#;
        let report = model(source);
        assert!(report.source_modeled.is_none());
        assert_code(
            &report,
            SourceModelDiagnosticKind::InvalidLrConflictResolution,
            "RCA3134",
        );
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("names no authored Node or variant")
        }));
    }

    #[test]
    fn invalid_precedence_declarations_have_structured_diagnostics() {
        let prelude = r#"
recursa::grammar! { module = crate }
recursa::tokens! {
    keywords { LIKE => "like", ESCAPE => "escape" }
    punctuation { PLUS => "+" }
"#;
        let cases = [
            (
                "precedence { nonassoc(bp = 5) { LIKE, MISSING } }",
                r#"recursa::ast_node! {
    struct Root;
}
"#,
                "`MISSING` in the precedence declaration is not a fixed token, a content token \
                 type or a lookahead filter's merged kind; a precedence-only name must be used \
                 by a `#[parse(prec = MISSING)]`",
            ),
            // A merged kind resolves only where a `lookahead` filter declares it: the name is
            // otherwise as unknown as any other, which keeps a misspelt twin an error.
            (
                "precedence { nonassoc(bp = 5) { LIKE, LIKE_LA } }",
                r#"recursa::ast_node! {
    struct Root;
}
"#,
                "`LIKE_LA` in the precedence declaration is not a fixed token, a content token \
                 type or a lookahead filter's merged kind; a precedence-only name must be used \
                 by a `#[parse(prec = LIKE_LA)]`",
            ),
            (
                "precedence { nonassoc(bp = 5) { LIKE } } precedence { left(bp = 6) { PLUS } }",
                r#"recursa::ast_node! {
    struct Root;
}
"#,
                "a grammar declares at most one `precedence { ... }` block",
            ),
            (
                "precedence { nonassoc(bp = 5) { LIKE } }",
                r#"recursa::ast_node! {
    #[parse(prec = ESCAPE)]
    struct Root;
}
"#,
                "#[parse(prec = ESCAPE)] names no level of the `precedence { ... }` declaration",
            ),
            (
                "",
                r#"recursa::ast_node! {
    #[parse(prec = LIKE)]
    struct Root;
}
"#,
                "#[parse(prec = LIKE)] needs a `precedence { ... }` declaration, and the grammar \
                 has none",
            ),
            (
                "precedence { nonassoc(bp = 5) { LIKE } }",
                r#"recursa::ast_node! {
    #[parse(prec = LIKE)]
    #[parse(prec = LIKE)]
    struct Root;
}
"#,
                "a Node or variant accepts at most one #[parse(prec = NAME)]",
            ),
            (
                "precedence { nonassoc(bp = 5) { LIKE } }",
                r#"recursa::ast_node! {
    #[parse(prec = LIKE, other)]
    struct Root;
}
"#,
                "#[parse(prec = NAME)] names one declared precedence level: unexpected tokens \
                 after the precedence level name",
            ),
            (
                "precedence { left(bp = 10) { PLUS } }",
                r#"recursa::ast_node! {
    #[pratt]
    enum Root {
        Atom(#[tok(LIKE)] ()),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[parse(prec = PLUS)]
        #[tok(PLUS)]
        Add(boxed!(Root), boxed!(Root)),
    }
}
"#,
                "#[parse(prec = NAME)] is not accepted on a prefix, infix or postfix Pratt \
                 variant, whose binding powers are its precedence",
            ),
        ];
        for (block, node, message) in cases {
            let source = format!("{prelude}{block}\n}}\n{node}\n");
            let report = model(&source);
            assert!(
                report.source_modeled.is_none(),
                "{block} / {node} was non-blocking"
            );
            assert_code(
                &report,
                SourceModelDiagnosticKind::InvalidPrecedenceDeclaration,
                "RCA3133",
            );
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message == message),
                "{block} / {node}: expected {message:?} in {:?}",
                report.diagnostics
            );
        }
    }

    #[test]
    fn a_malformed_precedence_block_is_rejected_where_the_token_grammar_is_parsed() {
        let cases = [
            (
                "precedence { left(bp = 1) { A }, sideways(bp = 2) { B } }",
                "a precedence level must be left, right, or nonassoc",
            ),
            ("precedence { left { A } }", "expected parentheses"),
            (
                "precedence { left(lbp = 1) { A } }",
                "a precedence level takes exactly `bp = N`, its binding power",
            ),
            (
                "precedence { left(bp = 300) { A } }",
                "a precedence level's binding power must be a u8 from 0 to 255",
            ),
            (
                "precedence { left(bp = 2) { A }, left(bp = 2) { B } }",
                "precedence levels are declared from lowest to highest binding power; bp = 2 \
                 does not exceed the previous level's bp = 2",
            ),
            (
                "precedence { left(bp = 1) { A, B }, right(bp = 2) { A } }",
                "`A` appears more than once in the precedence declaration",
            ),
            (
                "precedence { left(bp = 1) { } }",
                "a precedence level must name at least one token",
            ),
            (
                "precedence { left(bp = 1) { A } right(bp = 2) { B } }",
                "expected `,` after a precedence level",
            ),
        ];
        for (block, message) in cases {
            let error = syn::parse_str::<CentralTokensSyntax>(block)
                .err()
                .unwrap_or_else(|| panic!("{block} was accepted"));
            assert_eq!(error.to_string(), message, "{block}");
        }
        let report = model(
            r#"recursa::grammar! { module = crate }
recursa::tokens! {
    keywords { A => "a" }
    precedence { left(bp = 1) { A, A } }
}
recursa::ast_node! {
    struct Root;
}
"#,
        );
        assert!(report.source_modeled.is_none());
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                    && diagnostic.message
                        == "`A` appears more than once in the precedence declaration"
            }),
            "{:?}",
            report.diagnostics
        );
    }

    #[test]
    fn a_token_sequence_without_this_precedes_the_semantic_body() {
        let report = model(
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[tok("BEGIN")]
    struct Root {
        value: Leaf,
    }
}

recursa::ast_node! {

    struct Leaf;
}
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
    fn rejects_every_invalid_positional_attachment_before_semantic_analysis() {
        let cases = [
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[tok("A")]
    #[tok("B")]
    struct Root {
        value: Leaf,
    }
}
recursa::ast_node! {
    struct Leaf;
}
"#,
                SourceModelDiagnosticKind::MultipleTokenSequences,
                "RCA3102",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[tok()]
    struct Root {
        value: Leaf,
    }
}
recursa::ast_node! {
    struct Leaf;
}
"#,
                SourceModelDiagnosticKind::EmptyTokenSequence,
                "RCA3103",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[tok(this)]
    struct Root;
}
"#,
                SourceModelDiagnosticKind::EmptyThisAnchor,
                "RCA3105",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[tok("BAD")]
    enum Root {
        Variant,
    }
}
"#,
                SourceModelDiagnosticKind::InvalidTokenPlacement,
                "RCA3114",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[tok(MISSING)]
    struct Root;
}
"#,
                SourceModelDiagnosticKind::UnresolvedFixedToken,
                "RCA3106",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[pratt]
    enum Root {
        #[tok(this, this)]
        Group(boxed!(Self)),
    }
}
"#,
                SourceModelDiagnosticKind::MultipleThisAnchors,
                "RCA3104",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[kwd("OLD")]
    struct Root;
}
"#,
                SourceModelDiagnosticKind::UnsupportedFixedSyntax,
                "RCA3113",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[pratt]
    enum Root {
        #[parse(prefix)]
        Prefix(boxed!(Self)),
    }
}
"#,
                SourceModelDiagnosticKind::InvalidPrattTokenPlacement,
                "RCA3115",
            ),
            (
                r#"recursa::grammar! { module = crate }
recursa::ast_node! {
    struct Leaf;
}
recursa::ast_node! {

    struct Root {
        #[sep]
        values: zero_or_many!(Leaf),
    }
}
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
            emitted_codes.push(crate::source_analysis::source_model_code(expected));
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
            r#"recursa::grammar! { module = crate }
#[cfg_attr(feature = "strict", cfg(unknown_source_cfg))]
recursa::tokens! { punctuation { SEMI => ";" } }
recursa::ast_node! {
    struct Root;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[pratt]
    enum Expr {
        #[parse(prefix, bp = 10)]
        #[tok("!")]
        Not(#[tok("(", this, ")")] boxed!(Expr)),
    }
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    struct Vec<T> {
        value: T,
    }
}

recursa::ast_node! {

    struct Root {
        #[sep(",")]
        values: zero_or_many!(Leaf),
    }
}

recursa::ast_node! {

    struct Leaf;
}
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    struct Root<Word> {
        #[lex(pattern = "word")]
        word: Word,
    }
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[pratt]
    enum Expr {
        #[parse(prefix, bp = 30)]
        #[tok("!")]
        Prefix(boxed!(Expr)),

        #[parse(infix, lbp = 20, rbp = 21)]
        #[tok("+")]
        Infix(boxed!(Expr), boxed!(Expr)),

        #[parse(postfix, bp = 40)]
        #[tok("?")]
        Postfix(boxed!(Expr)),
    }
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    struct Root {
        #[lex(pattern = "word", admits(IF, ELSE))]
        first: Word,
        #[lex(pattern = "word", admits(ELSE, IF))]
        second: Word,
    }
}

recursa::ast_node! {

    #[tok("IF")]
    struct IfToken;
}

recursa::ast_node! {

    #[tok("ELSE")]
    struct ElseToken;
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[pratt]
    enum Expr {
        #[tok("ATOM")]
        Atom,
    }
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[pratt]
    enum Expr {
        #[tok("ATOM")]
        Atom,
        #[parse(prefix, bp = 0)]
        Prefix(boxed!(Self)),
        #[parse(infix, lbp = 254, rbp = 255)]
        Infix(boxed!(Self), boxed!(Self)),
        #[parse(postfix, bp = 37)]
        Postfix(boxed!(Self)),
    }
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
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[pratt]
    enum Expr {
        #[parse(infix, lbp = 20, rbp = 21)]
        #[tok("+")]
        Infix(boxed!(Expr), #[tok("AND")] boxed!(Expr)),
    }
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
                super::GrammarElement::OptionalFixedTokenGroup { .. } => {
                    unreachable!("fixture contains only required syntax")
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered, ["field", "+", "AND", "field"]);
    }

    #[test]
    fn multiple_or_malformed_pratt_roles_remain_diagnostics() {
        let report = model(
            r#"recursa::grammar! { module = crate }

recursa::ast_node! {

    #[pratt]
    enum Expr {
        #[parse(prefix)]
        #[parse(postfix)]
        Multiple(boxed!(Expr)),

        #[parse(sideways)]
        Malformed(boxed!(Expr)),
    }
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
                r#"recursa::grammar! {{ module = crate }}
recursa::ast_node! {{

    #[pratt]
    enum Expr {{
        #[parse({role})]
        Invalid(boxed!(Self)),
    }}
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
            r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[pratt]
    struct NotAnEnum;
}
"#,
            r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    enum NotPratt {
        #[parse(prefix, bp = 1)]
        Variant,
    }
}
"#,
            r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[pratt(unexpected)]
    enum InvalidMarker {
        Atom,
    }
}
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
            r#"recursa::grammar! { module = crate }
recursa::ast_node! {

    #[parse(skip)]
    struct FutureNodeParseForm {
        #[parse(prefix, bp = 1)]
        misplaced_role: Leaf,
        #[pratt]
        misplaced_marker: Leaf,
    }
}
recursa::ast_node! {

    enum Choice {
        #[pratt]
        MisplacedMarker,
    }
}
recursa::ast_node! {

    struct Leaf;
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation {
        COLON => ":", spacing = tight,
        DOUBLE_COLON => "::",
    }
}

recursa::ast_node! {

    #[tok(COLON, COLON)]
    struct Root;
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("longest match `::`")
        }));
    }

    #[test]
    fn pretty_spacing_rejects_runtime_tight_auto_boundaries() {
        for source in [
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { LPAREN => "(", RPAREN => ")", WHOLE => "()" } }
recursa::ast_node! {
    #[tok(LPAREN, RPAREN)]
    struct Root;
}
"#,
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":" } }
recursa::ast_node! {
    #[tok(COLON)]
    struct Tail;
}
recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"[A-Za-z:]+")]
        left: RootText,
        right: Tail,
    }
}
"#,
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":" } }
recursa::ast_node! {

    struct Root {
        #[tok(this, COLON)]
        #[lex(pattern = r"[A-Za-z:]+")]
        value: RootText,
    }
}
"#,
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { LPAREN => "(", RPAREN => ")", WHOLE => "()" } }
recursa::ast_node! {
    struct Child;
}
recursa::ast_node! {
    #[tok(RPAREN)]
    struct Tail;
}
recursa::ast_node! {
    struct Root(#[tok(this, LPAREN)] Child, Tail);
}
"#,
        ] {
            let report = model(source);
            assert!(report.source_modeled.is_none(), "{source}");
            assert!(
                report.diagnostics.iter().any(|diagnostic| {
                    diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                        && (diagnostic.message.contains("lexer")
                            || diagnostic.message.contains("regex-backed"))
                }),
                "{source}: {:?}",
                report.diagnostics
            );
        }
    }

    #[test]
    fn pretty_boundary_language_checks_only_crossing_lexemes() {
        let word = r"[A-Za-z_][A-Za-z0-9_]*";
        assert!(!pattern_then_fixed_intersects_pattern(word, "]", word));
        assert!(!fixed_then_pattern_intersects_pattern("[", word, word));
        assert!(pattern_then_fixed_intersects_pattern(
            r"[A-Za-z:]+",
            ":",
            r"[A-Za-z:]+"
        ));
        assert!(pattern_then_fixed_intersects_pattern("a", "bc", "ab"));
        assert!(fixed_then_pattern_intersects_pattern("a", "bc", "ab"));
    }

    #[test]
    fn pretty_boundary_prover_compiles_each_pattern_and_product_once() {
        let mut prover = PrettyBoundaryProver::default();
        for _ in 0..64 {
            assert!(prover.patterns_cross("a", "bc", "ab"));
            assert!(prover.patterns_cross("a", "bc", "abc"));
        }

        assert_eq!(prover.compiled_pattern_count(), 4);
        assert_eq!(prover.evaluated_product_count(), 2);
    }

    #[test]
    fn pretty_boundary_prover_reuses_right_products_across_distinct_left_languages() {
        let mut prover = PrettyBoundaryProver::default();
        let catalog = prover.intern_catalog(&["a+b+".to_owned(), "unrelated".to_owned()]);

        let _ = prover.patterns_cross("a", "b+", "a+b+");
        let _ = prover.patterns_cross("a+", "b+", "a+b+");
        let _ = prover.patterns_cross_catalog("a", "b+", catalog);
        let _ = prover.patterns_cross_catalog("a+", "b+", catalog);

        assert_eq!(prover.right_product_counts(), (1, 1));
        assert_eq!(prover.evaluated_product_count(), 2);
        assert_eq!(prover.evaluated_catalog_product_count(), 2);
    }

    #[test]
    fn pretty_boundary_catalog_reuses_one_left_language_across_distinct_rights() {
        let mut patterns = (0..128)
            .map(|index| format!("unrelated{index:03}"))
            .collect::<Vec<_>>();
        patterns.push(r"[a-z]+suffix017".to_owned());
        let left = r"[a-z]+";
        let rights = (0..128)
            .map(|index| format!("suffix{index:03}"))
            .collect::<Vec<_>>();
        let mut prover = PrettyBoundaryProver::default();
        let catalog = prover.intern_catalog(&patterns);

        let actual = rights
            .iter()
            .map(|right| prover.patterns_cross_catalog(left, right, catalog))
            .collect::<Vec<_>>();
        let expected = (0..rights.len())
            .map(|index| index == 17)
            .collect::<Vec<_>>();

        assert_eq!(actual, expected, "only the worked crossing may match");
        assert_eq!(actual.iter().filter(|crosses| **crosses).count(), 1);
        assert_eq!(
            prover.right_product_counts().1,
            1,
            "one repeated left language must not build one catalog product per right language",
        );
        assert_eq!(prover.catalog_left_product_counts(), (1, 0));
    }

    #[test]
    fn pretty_boundary_prover_batches_an_ordered_catalog_exactly() {
        let mut prover = PrettyBoundaryProver::default();
        let catalog =
            prover.intern_catalog(&["unrelated".to_owned(), "ab".to_owned(), "abc".to_owned()]);

        for _ in 0..64 {
            assert!(prover.patterns_cross_catalog("a", "bc", catalog));
        }
        assert_eq!(prover.evaluated_catalog_product_count(), 1);
        assert_eq!(prover.evaluated_product_count(), 0);

        let empty = prover.intern_catalog(&[]);
        assert!(!prover.patterns_cross_catalog("a", "bc", empty));
        let invalid = prover.intern_catalog(&["(".to_owned()]);
        assert!(prover.patterns_cross_catalog("a", "bc", invalid));

        let fallback = prover.intern_catalog(&["unrelated".to_owned(), "ab".to_owned()]);
        prover.catalogs[fallback.0].automaton = None;
        assert!(prover.patterns_cross_catalog("a", "bc", fallback));
        assert!(!prover.patterns_cross_catalog("x", "y", fallback));
    }

    #[test]
    fn pretty_boundary_catalog_positive_requires_one_crossing_member() {
        let left = r"[A-Za-z_][A-Za-z0-9_]*";
        let right = r"\)";
        let patterns = [left.to_owned(), right.to_owned()];
        let mut prover = PrettyBoundaryProver::default();
        let catalog = prover.intern_catalog(&patterns);

        let individual = patterns
            .iter()
            .any(|pattern| prover.patterns_cross(left, right, pattern));
        assert!(!individual, "neither member spans the authored boundary");
        assert_eq!(
            prover.patterns_cross_catalog(left, right, catalog),
            individual,
            "a catalog match cannot combine distinct member identities across the boundary",
        );
    }

    #[test]
    fn pretty_boundary_catalog_falls_back_when_batch_proof_is_inconclusive() {
        let patterns = [
            "ab".to_owned(),
            "abc".to_owned(),
            "xy".to_owned(),
            "unrelated".to_owned(),
        ];
        let mut prover = PrettyBoundaryProver::default();
        let catalog = prover.intern_catalog(&patterns);
        assert!(
            prover.catalogs[catalog.0]
                .automaton
                .as_ref()
                .expect("the valid catalog compiles")
                .reachable
                .set(None)
                .is_ok(),
            "the batch reachable graph is initially uncomputed",
        );

        let cases = [("a", "b"), ("x", "y"), ("q", "r")];
        let mut oracle = PrettyBoundaryProver::default();
        let expected = cases.map(|(left, right)| {
            patterns
                .iter()
                .any(|pattern| oracle.patterns_cross(left, right, pattern))
        });
        let actual = cases.map(|(left, right)| prover.patterns_cross_catalog(left, right, catalog));
        assert_eq!(
            actual, expected,
            "the ordered tree matches its member oracle"
        );

        let counts = prover.catalog_tree_counts();
        for (left, right) in cases {
            let _ = prover.patterns_cross_catalog(left, right, catalog);
        }
        assert_eq!(
            prover.catalog_tree_counts(),
            counts,
            "root and child answers are cached after one exact query",
        );
    }

    #[test]
    fn pretty_boundary_catalog_tree_prunes_a_large_false_catalog_before_leaves() {
        let patterns = (0..128)
            .map(|index| format!("token{index:03}"))
            .collect::<Vec<_>>();
        let mut prover = PrettyBoundaryProver::default();
        let catalog = prover.intern_catalog(&patterns);
        assert!(
            prover.catalogs[catalog.0]
                .automaton
                .as_ref()
                .expect("the valid catalog compiles")
                .reachable
                .set(None)
                .is_ok(),
            "force only the root batch to be inconclusive",
        );

        assert!(!prover.patterns_cross_catalog("x+", "y+", catalog));
        let (compilations, splits, leaf_fallbacks) = prover.catalog_tree_counts();
        assert_eq!(compilations, 2, "only the two half-catalogs are compiled");
        assert_eq!(splits, 1, "only the inconclusive root is divided");
        assert_eq!(
            leaf_fallbacks, 0,
            "conclusive child unions avoid all 128 individual products",
        );
    }

    #[test]
    fn pretty_boundary_catalog_tree_reuses_one_left_language_across_distinct_rights() {
        let mut patterns = (0..128)
            .map(|index| format!("unrelated{index:03}"))
            .collect::<Vec<_>>();
        patterns.push(r"[a-z]+suffix017".to_owned());
        let left = r"[a-z]+";
        let rights = (0..32)
            .map(|index| format!("suffix{index:03}"))
            .collect::<Vec<_>>();
        let mut prover = PrettyBoundaryProver::default();
        let catalog = prover.intern_catalog(&patterns);
        assert!(
            prover.catalogs[catalog.0]
                .automaton
                .as_ref()
                .expect("the valid root catalog compiles")
                .reachable
                .set(None)
                .is_ok(),
            "force every query onto adaptive child unions",
        );

        let actual = rights
            .iter()
            .map(|right| prover.patterns_cross_catalog(left, right, catalog))
            .collect::<Vec<_>>();
        let expected = (0..rights.len())
            .map(|index| index == 17)
            .collect::<Vec<_>>();

        assert_eq!(actual, expected, "only the worked crossing may match");
        assert!(
            prover.right_product_counts().1 <= 3,
            "root fallback must reuse accepted left frontiers in both half catalogs: {:?}",
            prover.right_product_counts(),
        );
        assert_eq!(prover.catalog_left_product_counts(), (1, 2));
    }

    #[test]
    fn pretty_boundary_catalog_tree_caps_aggregate_retained_cache_memory() {
        let patterns = (0..64)
            .map(|index| format!("token{index:03}"))
            .collect::<Vec<_>>();
        let mut prover = PrettyBoundaryProver {
            catalog_node_retained_budget: Some(0),
            catalog_node_retained_entry_budget: Some(0),
            ..PrettyBoundaryProver::default()
        };
        let catalog = prover.intern_catalog(&patterns);
        assert!(
            prover.catalogs[catalog.0]
                .automaton
                .as_ref()
                .expect("the valid catalog compiles")
                .reachable
                .set(None)
                .is_ok(),
            "force the root batch onto adaptive child unions",
        );

        assert!(!prover.patterns_cross_catalog("x+", "y+", catalog));
        assert_eq!(prover.catalog_node_retained_bytes(), 0);
        assert_eq!(prover.catalog_node_retained_entries(), 0);
        assert!(prover.catalog_node_automata.is_empty());
        assert!(prover.catalog_node_right_products.is_empty());
        assert!(prover.catalog_node_left_products.is_empty());
        assert!(prover.catalog_node_left_queries.is_empty());
        assert!(prover.catalog_node_products.is_empty());

        assert!(
            !prover.patterns_cross_catalog("q+", "r+", catalog),
            "a distinct boundary remains exact after every child cache entry was evicted",
        );
        assert_eq!(prover.catalog_node_retained_bytes(), 0);
        assert_eq!(prover.catalog_node_retained_entries(), 0);

        let counts = prover.catalog_tree_counts();
        assert!(
            !prover.patterns_cross_catalog("x+", "y+", catalog),
            "eviction cannot weaken the exact cached root answer",
        );
        assert_eq!(prover.catalog_tree_counts(), counts);
    }

    #[test]
    fn pretty_boundary_individual_language_preserves_longer_alternation() {
        let mut prover = PrettyBoundaryProver::default();

        assert!(
            prover.patterns_cross("a", "b", "a|ab"),
            "the lexical language contains `ab` even when its shorter alternative is listed first",
        );
    }

    #[test]
    fn pretty_boundary_product_scans_each_joint_byte_class_once() {
        let compile = |pattern| {
            let dfa = regex_automata::dfa::dense::DFA::new(pattern).expect("a valid test pattern");
            let input = regex_automata::Input::new(b"").anchored(regex_automata::Anchored::Yes);
            let start = regex_automata::dfa::Automaton::start_state_forward(&dfa, &input)
                .expect("an anchored start state");
            PrettyPatternAutomaton::new(dfa, start)
        };
        let compile_catalog = |patterns: &[&str]| {
            let mut builder = regex_automata::dfa::dense::Builder::new();
            builder.configure(
                regex_automata::dfa::dense::DFA::config()
                    .match_kind(regex_automata::MatchKind::All)
                    .start_kind(regex_automata::dfa::StartKind::Anchored),
            );
            let dfa = builder.build_many(patterns).expect("valid test patterns");
            let input = regex_automata::Input::new(b"").anchored(regex_automata::Anchored::Yes);
            let start = regex_automata::dfa::Automaton::start_state_forward(&dfa, &input)
                .expect("an anchored start state");
            PrettyPatternAutomaton::new(dfa, start)
        };
        let left = compile(r"a{1,8}");
        let right = compile(r"b+");
        let lexical = compile_catalog(&[r"unrelated", r"a{1,8}c", r"a{1,8}d"]);

        let (_, counts) = compiled_patterns_cross_counted(&left, &right, &lexical);
        let right_classes = pretty_boundary_joint_byte_representatives(&right, &lexical).len();
        let left_classes = pretty_boundary_joint_byte_representatives(&left, &lexical).len();
        let dense_byte_scan = (usize::from(u8::MAX) + 1)
            * (counts.right_states
                + counts.lexical_states
                + counts.right_product_states
                + counts.left_product_states);

        assert_eq!(
            counts.byte_transitions,
            (usize::from(u8::MAX) + 1) * (counts.right_states + counts.lexical_states)
                + right_classes * counts.right_product_states
                + left_classes * counts.left_product_states
        );
        assert!(right_classes < usize::from(u8::MAX) + 1);
        assert!(left_classes < usize::from(u8::MAX) + 1);
        assert!(
            counts.byte_transitions * 2 < dense_byte_scan,
            "joint byte classes must materially reduce this representative product",
        );
        assert_eq!(
            counts.right_product_states,
            counts.right_states * counts.lexical_states
        );
        assert!(
            counts.boundary_lookups > 1,
            "exercise repeated boundary states"
        );
    }

    #[test]
    fn pretty_boundary_products_reuse_reachable_language_graphs() {
        let compile = |pattern| {
            let dfa = regex_automata::dfa::dense::DFA::new(pattern).expect("a valid test pattern");
            let input = regex_automata::Input::new(b"").anchored(regex_automata::Anchored::Yes);
            let start = regex_automata::dfa::Automaton::start_state_forward(&dfa, &input)
                .expect("an anchored start state");
            PrettyPatternAutomaton::new(dfa, start)
        };
        let left = compile("a+");
        let right = compile("b+");
        let lexical = compile("a+b+");

        let (_, first) = compiled_patterns_cross_counted(&left, &right, &lexical);
        let (_, second) = compiled_patterns_cross_counted(&left, &right, &lexical);

        assert_eq!(first.reachable_graphs_built, 2);
        assert_eq!(second.reachable_graphs_built, 0);
    }

    #[test]
    fn pretty_boundary_product_budget_precedes_cartesian_allocation() {
        let byte_width = usize::from(u8::MAX) + 1;
        let maximum_product = PRETTY_BOUNDARY_PRODUCT_TRANSITION_BUDGET / byte_width;

        assert_eq!(
            pretty_boundary_product_len(1, maximum_product),
            Some(maximum_product)
        );
        assert_eq!(pretty_boundary_product_len(1, maximum_product + 1), None);
        assert_eq!(pretty_boundary_product_len(usize::MAX, 2), None);
    }

    #[test]
    fn pretty_boundary_left_budget_precedes_sparse_product_growth() {
        let byte_width = usize::from(u8::MAX) + 1;
        let maximum_states = PRETTY_BOUNDARY_LEFT_TRANSITION_BUDGET / byte_width;

        assert_eq!(
            pretty_boundary_left_transition_len(maximum_states),
            Some(PRETTY_BOUNDARY_LEFT_TRANSITION_BUDGET)
        );
        assert_eq!(
            pretty_boundary_left_transition_len(maximum_states + 1),
            None
        );
        assert_eq!(pretty_boundary_left_transition_len(usize::MAX), None);
    }

    #[test]
    fn pretty_boundary_reachable_graph_budget_precedes_row_allocation() {
        let byte_width = usize::from(u8::MAX) + 1;
        let maximum_states = PRETTY_BOUNDARY_REACHABLE_TRANSITION_BUDGET / byte_width;

        assert_eq!(
            pretty_boundary_reachable_transition_len(maximum_states),
            Some(PRETTY_BOUNDARY_REACHABLE_TRANSITION_BUDGET)
        );
        assert_eq!(
            pretty_boundary_reachable_transition_len(maximum_states + 1),
            None
        );
        assert_eq!(pretty_boundary_reachable_transition_len(usize::MAX), None);
    }

    #[test]
    fn pretty_boundary_dense_product_matches_the_old_oracle_matrix() {
        let compile = |pattern| {
            let dfa = regex_automata::dfa::dense::DFA::new(pattern).expect("a valid test pattern");
            let input = regex_automata::Input::new(b"").anchored(regex_automata::Anchored::Yes);
            let start = regex_automata::dfa::Automaton::start_state_forward(&dfa, &input)
                .expect("an anchored start state");
            PrettyPatternAutomaton::new(dfa, start)
        };
        let patterns = ["", "a", "ab", r"a+", r"(?:ab|ac)", r"[a-c]{0,3}", "☃+"];
        let lexical_catalogs: &[&[&str]] = &[
            &[""],
            &["a"],
            &["ab"],
            &[r"a+"],
            &[r"(?:ab|ac)"],
            &[r"[a-c]{0,3}"],
            &["☃+", r"[\x00-\xFF]+"],
            &["unrelated", "ab", "abc"],
        ];

        for left_pattern in patterns {
            for right_pattern in patterns {
                for lexical_catalog in lexical_catalogs {
                    let left = compile(left_pattern);
                    let right = compile(right_pattern);
                    let mut builder = regex_automata::dfa::dense::Builder::new();
                    builder.configure(
                        regex_automata::dfa::dense::DFA::config()
                            .match_kind(regex_automata::MatchKind::All)
                            .start_kind(regex_automata::dfa::StartKind::Anchored),
                    );
                    let dfa = builder
                        .build_many(lexical_catalog)
                        .expect("valid catalog test patterns");
                    let input =
                        regex_automata::Input::new(b"").anchored(regex_automata::Anchored::Yes);
                    let start = regex_automata::dfa::Automaton::start_state_forward(&dfa, &input)
                        .expect("an anchored catalog start state");
                    let lexical = PrettyPatternAutomaton::new(dfa, start);
                    let expected = compiled_patterns_cross_old_oracle(&left, &right, &lexical);
                    assert_eq!(
                        compiled_patterns_cross(&left, &right, &lexical),
                        expected,
                        "left={left_pattern:?}, right={right_pattern:?}, lexical={lexical_catalog:?}"
                    );
                    let mut counts = PrettyBoundaryProductCounts::new();
                    let adaptive = build_pretty_left_product(&left, &lexical, &mut counts)
                        .and_then(|product| {
                            compiled_right_crosses(&right, &lexical, &product, &mut counts)
                        })
                        .unwrap_or(true);
                    assert_eq!(
                        adaptive, expected,
                        "adaptive left product: left={left_pattern:?}, right={right_pattern:?}, lexical={lexical_catalog:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn pretty_boundary_catalog_preserves_first_source_order_diagnostic() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { PLUS => "+", spacing = tight, MINUS => "-" }
    lexer_tokens { FIRST => r"\+-", SECOND => r"\+(?:-)" }
}
recursa::ast_node! {
    #[tok(PLUS, MINUS)]
    struct Root;
}
"#,
        );

        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                    && diagnostic.message.contains("lexical pattern")
            })
            .expect("a lexical Pretty boundary diagnostic");
        assert!(diagnostic.message.contains(r"lexical pattern `\+-`"));
    }

    #[test]
    fn pretty_spacing_checks_explicit_tight_fixed_edges_against_regex_rules() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { PLUS => "+", spacing = tight, MINUS => "-" } }
recursa::ast_node! {

    struct Lexeme {
        #[lex(pattern = r"\+-")]
        value: LexemeText,
    }
}
recursa::ast_node! {
    #[tok(PLUS, MINUS)]
    struct Root;
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("lexical pattern")
        }));
    }

    #[test]
    fn pretty_spacing_checks_crossing_prefixes_from_central_lexical_rules() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { SUFFIX => "+c" }
    lexer_tokens { MERGED => r"a\+" }
}
recursa::ast_node! {

    struct Prefix {
        #[lex(pattern = "a")]
        value: PrefixText,
    }
}
recursa::ast_node! {
    #[tok(SUFFIX)]
    struct Suffix;
}
recursa::ast_node! {
    struct Root {
        left: Prefix,
        right: Suffix,
    }
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("lexer boundary")
        }));
    }

    #[test]
    fn pretty_dynamic_boundary_retains_fixed_languages_in_the_full_catalog() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", MERGED => "a:" }
}
recursa::ast_node! {

    struct Prefix {
        #[lex(pattern = "a")]
        value: PrefixText,
    }
}
recursa::ast_node! {
    #[tok(COLON)]
    struct Suffix;
}
recursa::ast_node! {
    struct Root {
        left: Prefix,
        right: Suffix,
    }
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("regex-backed content")
        }));
    }

    #[test]
    fn ascii_insensitive_pretty_boundaries_check_canonical_fixed_spelling() {
        for source in [
            r#"recursa::grammar! {
    module = crate,
    derives(Pretty),
    keyword_matching = ascii_insensitive,
}
recursa::tokens! {
    keywords { WORD => "word" }
    punctuation { COLON => ":" }
    lexer_tokens { UPPER_MERGED => r"WORD:" }
}
recursa::ast_node! {
    #[tok(WORD, COLON)]
    struct Root;
}
"#,
            r#"recursa::grammar! {
    module = crate,
    derives(Pretty),
    keyword_matching = ascii_insensitive,
}
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { WORD => "word" }
    punctuation { COLON => ":" }
    lexer_tokens { MIXED_MERGED => r"WoRd:" }
}
recursa::ast_node! {
    struct Empty;
}
recursa::ast_node! {

    struct Root(#[tok(WORD, COLON, this)] Empty);
}
"#,
        ] {
            let report = model(source);

            assert!(report.source_modeled.is_none(), "{source}");
            assert!(
                report.diagnostics.iter().any(|diagnostic| {
                    diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                        && diagnostic.message.contains("lexical pattern")
                }),
                "{source}: {:?}",
                report.diagnostics
            );
        }
    }

    #[test]
    fn pretty_spacing_requires_ignored_trivia_for_automatic_spaces() {
        let unsafe_report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"[0-9]+")]
        left: LeftText,
        #[lex(pattern = r"[A-Za-z]+")]
        right: RightText,
    }
}
"#,
        );
        assert!(unsafe_report.source_modeled.is_none());
        assert!(unsafe_report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::UnsafePrettyHint
                && diagnostic.sites.len() == 2
                && diagnostic
                    .message
                    .contains("ignored lexer trivia accepting spaces")
        }));

        let narrow_report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"[ ]+" }
recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"[0-9]+")]
        left: LeftText,
        #[lex(pattern = r"[A-Za-z]+")]
        right: RightText,
    }
}
"#,
        );
        assert!(narrow_report.source_modeled.is_none());
        assert!(narrow_report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::UnsafePrettyHint
                && diagnostic.message.contains("spaces and newlines")
        }));

        let safe_report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"[ \t\r\n]+" }
recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"[0-9]+")]
        left: LeftText,
        #[lex(pattern = r"[A-Za-z]+")]
        right: RightText,
    }
}
"#,
        );
        assert!(
            safe_report.diagnostics.is_empty(),
            "{:?}",
            safe_report.diagnostics
        );
        assert!(safe_report.source_modeled.is_some());
    }

    #[test]
    fn pretty_identifier_boundaries_check_only_the_relevant_regex_edge() {
        for source in [
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { SUFFIX => "x-" } }
recursa::ast_node! {
    #[tok(SUFFIX)]
    struct Suffix;
}
recursa::ast_node! {

    struct Root {
        #[lex(pattern = r"[A-Za-z]+-")]
        left: RootText,
        right: Suffix,
    }
}
"#,
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { PREFIX => "-x", spacing = tight } }
recursa::ast_node! {
    #[tok(PREFIX)]
    struct Prefix;
}
recursa::ast_node! {

    struct Root {
        left: Prefix,
        #[lex(pattern = r"-[A-Za-z]+")]
        right: RootText,
    }
}
"#,
        ] {
            let report = model(source);
            assert!(
                report.diagnostics.is_empty(),
                "edge-safe regex was rejected: {source}: {:?}",
                report.diagnostics
            );
            assert!(report.source_modeled.is_some());
        }
    }

    #[test]
    fn pretty_spacing_rejects_a_tight_unicode_identifier_boundary() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation {
        BASE => "a",
        COMBINING_ACUTE => "\u{301}", spacing = after,
    }
}

recursa::ast_node! {

    #[tok(BASE, COMBINING_ACUTE)]
    struct Root;
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
    fn pretty_spacing_accepts_safe_requested_boundaries() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    punctuation {
        COLON => ":", spacing = after,
        DOUBLE_COLON => "::",
    }
}

recursa::ast_node! {

    #[tok(COLON, COLON)]
    struct Root;
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_spacing_rejects_a_longest_match_across_a_semantic_child_boundary() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation {
        COLON => ":", spacing = tight,
        DOUBLE_COLON => "::",
    }
}

recursa::ast_node! {

    #[tok(COLON)]
    struct Child;
}

recursa::ast_node! {

    #[tok(this, COLON)]
    struct Root(Child);
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("longest match `::`")
        }));
    }

    #[test]
    fn pretty_boundary_walk_reuses_shared_acyclic_dag_nodes() {
        const DEPTH: usize = 14;

        let mut source = String::from(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", RPAREN => ")" }
    ignore = r"\s+"
}
recursa::ast_node! {
    #[tok(COLON)]
    struct Leaf;
}
recursa::ast_node! {
    #[tok(RPAREN)]
    struct Tail;
}
"#,
        );
        for depth in 0..DEPTH {
            let child = if depth == 0 {
                "Leaf".to_owned()
            } else {
                format!("Layer{}", depth - 1)
            };
            writeln!(
                source,
                r#"recursa::ast_node! {{
    enum Layer{depth} {{
        Left({child}),
        Right({child}),
    }}
}}
"#
            )
            .unwrap();
        }
        writeln!(
            source,
            r#"recursa::ast_node! {{
    struct Root(Layer{}, Tail);
}}
"#,
            DEPTH - 1
        )
        .unwrap();

        let report = model(&source);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert_eq!(
            report.pretty_boundary_node_evaluations, 16,
            "each of the fifteen shared left-boundary Nodes and the Tail must expand once",
        );
        assert_eq!(
            report.pretty_soft_node_evaluations, 0,
            "hint-free scopes must not traverse the shared soft-opportunity DAG",
        );
    }

    #[test]
    fn pretty_soft_opportunity_walk_reuses_shared_acyclic_dag_nodes() {
        const DEPTH: usize = 14;

        let mut source = String::from(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { LPAREN => "(", RPAREN => ")" }
    ignore = r"\s+"
}
recursa::ast_node! {
    struct Leaf;
}
recursa::ast_node! {
    #[tok(LPAREN, RPAREN)]
    struct Breakable;
}
"#,
        );
        for depth in 0..DEPTH {
            let child = if depth == 0 {
                "Leaf".to_owned()
            } else {
                format!("Layer{}", depth - 1)
            };
            writeln!(
                source,
                r#"recursa::ast_node! {{
    enum Layer{depth} {{
        Left({child}),
        Right({child}),
    }}
}}
"#
            )
            .unwrap();
        }
        writeln!(
            source,
            r#"recursa::ast_node! {{
    #[pretty(group = consistent)]
    struct Root(Layer{}, Breakable);
}}
"#,
            DEPTH - 1
        )
        .unwrap();

        let report = model(&source);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert_eq!(
            report.pretty_soft_node_evaluations, 16,
            "each of the fourteen shared DAG Nodes, Leaf, and Breakable must expand once",
        );
    }

    #[test]
    fn pretty_soft_opportunity_cycle_fallback_is_not_cached_as_false() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { LPAREN => "(", RPAREN => ")" }
    ignore = r"\s+"
}
recursa::ast_node! {
    struct Cycle {
        next: Option<boxed!(Cycle)>,
    }
}
recursa::ast_node! {
    #[tok(LPAREN, RPAREN)]
    struct Breakable;
}
recursa::ast_node! {
    #[pretty(group = consistent)]
    struct First(Cycle, Breakable);
}
recursa::ast_node! {
    #[pretty(group = consistent)]
    struct Second(Cycle, Breakable);
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert_eq!(
            report.pretty_soft_node_evaluations, 3,
            "the recursive false fallback must be recomputed while the definitive true result is reused",
        );
    }

    #[test]
    fn pretty_spacing_validates_internal_and_external_attachment_boundaries() {
        for source in [
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = tight, DOUBLE_COLON => "::" }
}
recursa::ast_node! {
    #[tok(COLON)]
    struct Child;
}
recursa::ast_node! {
    struct Root {
        #[tok(COLON, this)]
        child: Child,
    }
}
"#,
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = tight, DOUBLE_COLON => "::" }
}
recursa::ast_node! {
    struct Child;
}
recursa::ast_node! {
    #[tok(COLON)]
    struct Tail;
}
recursa::ast_node! {
    struct Root(#[tok(this, COLON)] Child, Tail);
}
"#,
        ] {
            let report = model(source);
            assert!(report.source_modeled.is_none());
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                    && diagnostic.message.contains("longest match `::`")
            }));
        }
    }

    #[test]
    fn pretty_spacing_rejects_explicit_tight_inside_an_attachment() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight } }
recursa::ast_node! {

    struct Root {
        #[tok(this, COLON)]
        #[lex(pattern = r"[A-Za-z:]+")]
        text: RootText,
    }
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
    fn pretty_spacing_rejects_an_unproved_regex_boundary() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = tight }
}

recursa::ast_node! {

    #[tok(this, COLON)]
    struct Root {
        #[lex(pattern = r"[A-Za-z:]+")]
        text: RootText,
    }
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
    fn pretty_boundary_cache_preserves_each_dynamic_use_site() {
        let source = r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight } }

recursa::ast_node! {

    struct Dynamic {
        #[lex(pattern = r"[A-Za-z:]+")]
        value: DynamicText,
    }
}

recursa::ast_node! {

    struct Root {
        #[tok(this, COLON)]
        first: Dynamic,
        #[tok(this, COLON)]
        second: Dynamic,
    }
}
"#;
        let report = model(source);
        let dynamic_site = source.find("value: DynamicText").unwrap() as u32;
        let second_site = source.rfind("COLON").unwrap() as u32;

        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                    && diagnostic.message.contains("regex-backed")
                    && diagnostic.sites.len() == 2
                    && diagnostic.sites[0].range.contains(&dynamic_site)
                    && diagnostic.sites[1].range.start == second_site
            }),
            "{:?}",
            report.diagnostics
        );
    }

    #[test]
    fn pretty_boundary_dedup_preserves_distinct_sites_in_source_order() {
        let source = r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = tight, DOUBLE_COLON => "::" }
}
recursa::ast_node! {

    #[tok(COLON)]
    struct Left;
}
recursa::ast_node! {

    #[tok(COLON)]
    struct Right;
}
recursa::ast_node! {

    enum Choice {
        First(Left),
        Second(Right),
    }
}
recursa::ast_node! {

    struct Root(#[tok(this, COLON)] Choice);
}
"#;
        let report = model(source);
        let definition_sites = source
            .match_indices("#[tok(COLON)]")
            .map(|(offset, _)| offset as u32 + 6)
            .collect::<Vec<_>>();
        assert_eq!(definition_sites.len(), 2);
        let primary_sites = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("longest match `::`"))
            .map(|diagnostic| diagnostic.sites[0].range.start)
            .collect::<Vec<_>>();

        assert_eq!(primary_sites, definition_sites);
    }

    #[test]
    fn pretty_boundary_cycles_keep_the_unknown_fallback_uncached() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = tight, DOUBLE_COLON => "::" }
}
recursa::ast_node! {
    #[tok(COLON)]
    struct Leaf;
}
recursa::ast_node! {

    enum Recursive {
        Recur(boxed!(Recursive)),
        End(Leaf),
    }
}
recursa::ast_node! {

    struct Root(#[tok(this, COLON)] Recursive);
}
"#,
        );
        let longest_match = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("longest match `::`"))
            .count();

        assert_eq!(longest_match, 1);
        assert!(report.diagnostics.iter().all(|diagnostic| {
            !diagnostic
                .message
                .contains("unproved lexer boundary involving regex-backed content")
        }));
        assert!(
            report.pretty_boundary_node_evaluations > 2,
            "the cyclic Recursive boundary must not enter the acyclic memo"
        );
    }

    #[test]
    fn pretty_breaks_prove_neighboring_regex_boundaries_are_separated() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { MINUS => "-" }
    lexer_tokens { MERGED => r"(?:a-|-a)" }
    ignore = r"\s+"
}

recursa::ast_node! {

    struct Word {
        #[lex(pattern = "a")]
        text: WordText,
    }
}
recursa::ast_node! {

    #[tok(MINUS)]
    struct Minus;
}
recursa::ast_node! {

    struct BreakBefore {
        left: Word,
        #[pretty(break_before = soft)]
        right: Minus,
    }
}
recursa::ast_node! {

    struct BreakAfter {
        #[pretty(break_after = soft)]
        left: Word,
        right: Minus,
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_spacing_rejects_a_unicode_boundary_from_dynamic_content() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COMBINING_ACUTE => "\u{301}", spacing = after }
}

recursa::ast_node! {

    #[tok(this, COMBINING_ACUTE)]
    struct Root {
        #[lex(pattern = r"\p{XID_Start}\p{XID_Continue}*")]
        text: RootText,
    }
}
"#,
        );

        assert!(report.source_modeled.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                && diagnostic.message.contains("lexer boundary")
        }));
    }

    #[test]
    fn invalid_pretty_spacing_is_reported_during_source_analysis() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { COLON => ":", spacing = sideways }
}

recursa::ast_node! {

    #[tok(COLON)]
    struct Root;
}
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { PLUS => "+" } }

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { BEGIN => "begin", END => "end" }
    punctuation { PLUS => "+" }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Group(#[tok(BEGIN, this, END)] boxed!(Self)),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_pratt_accepts_exact_fixed_only_grouping_markers() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    punctuation { OPEN => "(", CLOSE => ")", PLUS => "+" }
}

recursa::ast_node! {

    enum Open {
        #[tok(OPEN)]
        Value,
    }
}
recursa::ast_node! {

    enum Close {
        #[tok(CLOSE)]
        Value,
    }
}
recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}
recursa::ast_node! {

    struct Group {
        open: Open,
        value: boxed!(Expr),
        close: Close,
    }
}
recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Group(Group),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_pratt_rejects_inexact_semantic_grouping_markers() {
        for source in [
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { OPEN => "(", CLOSE => ")", PLUS => "+" } }
recursa::ast_node! {

    struct Open {
        #[lex(pattern = r"[a-z]+\[")]
        text: OpenText,
    }
}
recursa::ast_node! {
    enum Close {
        #[tok(CLOSE)]
        Value,
    }
}
recursa::ast_node! {
    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}
recursa::ast_node! {
    struct Group {
        open: Open,
        value: boxed!(Expr),
        close: Close,
    }
}
recursa::ast_node! {
    #[pratt]
    enum Expr {
        Integer(Integer),
        Group(Group),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { OPEN => "(", ALT => "[", CLOSE => ")", PLUS => "+" } }
recursa::ast_node! {
    enum Open {
        #[tok(OPEN)]
        Round,
        #[tok(ALT)]
        Square,
    }
}
recursa::ast_node! {
    enum Close {
        #[tok(CLOSE)]
        Value,
    }
}
recursa::ast_node! {
    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}
recursa::ast_node! {
    struct Group {
        open: Open,
        value: boxed!(Expr),
        close: Close,
    }
}
recursa::ast_node! {
    #[pratt]
    enum Expr {
        Integer(Integer),
        Group(Group),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { OPEN => "(", CLOSE => ")", PLUS => "+" } }
recursa::ast_node! {
    enum Open {
        #[tok(OPEN)]
        Value,
    }
}
recursa::ast_node! {
    enum Close {
        #[tok(CLOSE)]
        Value,
    }
}
recursa::ast_node! {
    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}
recursa::ast_node! {
    struct Group {
        open: one_or_many!(Open),
        value: boxed!(Expr),
        close: Close,
    }
}
recursa::ast_node! {
    #[pratt]
    enum Expr {
        Integer(Integer),
        Group(Group),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
        ] {
            let report = model(source);
            assert!(report.source_modeled.is_none());
            assert!(
                report.diagnostics.iter().any(|diagnostic| {
                    diagnostic.kind == SourceModelDiagnosticKind::InvalidConfiguration
                        && diagnostic.message.contains("cannot repair precedence")
                }),
                "{:?}",
                report.diagnostics
            );
        }
    }

    #[test]
    fn pretty_pratt_accepts_unique_grouping_through_authored_helpers() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { ANY => "any", QUERY => "query" }
    punctuation { OPEN => "(", CLOSE => ")", COMMA => ",", DOT => ".", STAR => "*", PLUS => "+" }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    enum ParenthesizedSuffix {
        #[tok(DOT, STAR)]
        Star,
    }
}

recursa::ast_node! {

    #[tok(QUERY)]
    struct Query {
        value: boxed!(Expr),
    }
}

recursa::ast_node! {

    enum ParenthesizedContent {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Query(Query),
    }
}

recursa::ast_node! {

    struct ParenthesizedExpr {
        #[tok(OPEN, this, CLOSE)]
        content: ParenthesizedContent,
        suffix: Option<ParenthesizedSuffix>,
    }
}

recursa::ast_node! {

    struct QuantifiedExpr {
        #[tok(ANY, OPEN, this, CLOSE)]
        content: ParenthesizedContent,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Quantified(QuantifiedExpr),
        Parenthesized(ParenthesizedExpr),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_pratt_grouping_ignores_cycles_in_witness_dead_branches() {
        // `Sub`'s `items` sits after required fixed syntax with nothing
        // following (before=1, after=0): that context can never contribute a
        // grouping witness, so the self-recursive `R` it reaches must not
        // poison the proof carried by the sibling `Exprs` witness.
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { KW => "kw" }
    punctuation {
        OPEN => "(", CLOSE => ")", COMMA => ",",
        LBRACKET => "[", RBRACKET => "]", PLUS => "+"
    }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    enum R {
        Nested(#[tok(LBRACKET, this, RBRACKET)] boxed!(R)),
        Leaf(Integer),
    }
}

recursa::ast_node! {

    #[tok(KW, this)]
    struct S {
        #[sep(COMMA)]
        items: one_or_many!(R),
    }
}

recursa::ast_node! {

    enum C {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Sub(boxed!(S)),
    }
}

recursa::ast_node! {

    struct P {
        #[tok(OPEN, this, CLOSE)]
        content: C,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Parenthesized(P),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_pratt_grouping_survives_a_witnessless_cyclic_sibling_variant() {
        // Mirrors gram.y's `select_with_parens: '(' select_no_parens ')' |
        // '(' select_with_parens ')'`: `C::Query` re-enters `Q` through a
        // plain enclosure, so the cycle carries no owning `Expr` of its own.
        // The sibling `C::Exprs` still supplies the one grouping witness, and
        // the cycle cannot hide a second one, so RCA3101 must stay silent.
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { SELECT => "select" }
    punctuation {
        OPEN => "(", CLOSE => ")", LBRACKET => "[", RBRACKET => "]",
        COMMA => ",", PLUS => "+"
    }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    #[tok(SELECT)]
    struct Simple {
        #[sep(COMMA)]
        targets: one_or_many!(Expr),
    }
}

recursa::ast_node! {

    struct Paren {
        #[tok(LBRACKET, this, RBRACKET)]
        query: boxed!(Q),
    }
}

recursa::ast_node! {

    enum Q {
        Simple(Simple),
        Paren(Paren),
    }
}

recursa::ast_node! {

    enum C {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Query(boxed!(Q)),
    }
}

recursa::ast_node! {

    struct P {
        #[tok(OPEN, this, CLOSE)]
        content: C,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Parenthesized(P),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_pratt_rejects_an_atom_whose_every_variant_is_cyclic() {
        // `C` reaches no owning `Expr` at all: both of its variants lead into
        // the `Q`/`Paren` cycle. Discharging a witnessless cycle must not
        // invent a grouping path, so RCA3101 still fires.
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { SELECT => "select", STAR => "star" }
    punctuation { OPEN => "(", CLOSE => ")", PLUS => "+" }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    #[tok(SELECT, STAR)]
    struct Simple;
}

recursa::ast_node! {

    struct Paren {
        #[tok(OPEN, this, CLOSE)]
        query: boxed!(Q),
    }
}

recursa::ast_node! {

    enum Q {
        Simple(Simple),
        Paren(Paren),
    }
}

recursa::ast_node! {

    enum C {
        Query(boxed!(Q)),
        Nested(boxed!(Paren)),
    }
}

recursa::ast_node! {

    struct P {
        #[tok(OPEN, this, CLOSE)]
        content: C,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Parenthesized(P),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
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
    fn pretty_pratt_rejects_a_cycle_that_unrolls_a_second_grouping_witness() {
        // `Q` owns a transparent witness through `Q::Direct`, so unrolling the
        // `Q -> Paren -> Q` enclosure cycle yields a second, wider grouping
        // (`[ ( expr ) ]` beside `[ expr ]`). The cycle is therefore not
        // witnessless and must keep the proof inconclusive.
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    punctuation {
        OPEN => "(", CLOSE => ")", LBRACKET => "[", RBRACKET => "]",
        COMMA => ",", PLUS => "+"
    }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    struct Paren {
        #[tok(OPEN, this, CLOSE)]
        query: boxed!(Q),
    }
}

recursa::ast_node! {

    enum Q {
        Direct(#[sep(COMMA)] one_or_many!(Expr)),
        Paren(Paren),
    }
}

recursa::ast_node! {

    struct P {
        #[tok(LBRACKET, this, RBRACKET)]
        content: Q,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Parenthesized(P),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
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
    fn pretty_pratt_rejects_ambiguous_grouping_through_authored_helpers() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    punctuation { OPEN => "(", CLOSE => ")", COMMA => ",", PLUS => "+" }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    enum ParenthesizedContent {
        Exprs(#[sep(COMMA)] one_or_many!(Expr)),
        Single(boxed!(Expr)),
    }
}

recursa::ast_node! {

    struct ParenthesizedExpr {
        #[tok(OPEN, this, CLOSE)]
        content: ParenthesizedContent,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        Parenthesized(ParenthesizedExpr),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
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
    fn pretty_pratt_rejects_optional_grouping_like_atoms() {
        let report = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    punctuation { OPEN => "[", CLOSE => "]", PLUS => "+" }
}

recursa::ast_node! {

    struct Integer {
        #[lex(pattern = r"[0-9]+")]
        value: IntegerText,
    }
}

recursa::ast_node! {

    #[pratt]
    enum Expr {
        Integer(Integer),
        NotAGroup(#[tok(OPEN, this, CLOSE)] Option<boxed!(Self)>),
        #[parse(infix, lbp = 10, rbp = 11)]
        #[tok(PLUS)]
        Add(boxed!(Self), boxed!(Self)),
    }
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"\s+" }
recursa::tokens! { punctuation { COMMA => ",", TAIL => "." } }
recursa::ast_node! {

    struct Item {
        #[lex(pattern = r"[a-z]+")]
        text: ItemText,
    }
}
recursa::ast_node! {
    #[tok(TAIL)]
    struct Tail;
}
recursa::ast_node! {

    #[pretty(group = inconsistent, indent, break_before = soft, break_after = hard)]
    struct Root {
        head: Tail,
        #[sep(COMMA, trailing)]
        #[pretty(group = consistent, indent, break_before = soft, break_after = hard, separator = leading, trailing_separator = always)]
        values: zero_or_many!(Item),
        tail: Tail,
    }
}
"#,
        );
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(report.source_modeled.is_some());
    }

    #[test]
    fn pretty_hints_reject_unknown_duplicate_and_inapplicable_settings() {
        let unknown = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::ast_node! {

    #[pretty(text = normalized)]
    struct Root;
}
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::ast_node! {

    #[other::pretty(group = consistent)]
    struct Root {
        value: Leaf,
    }
}
recursa::ast_node! {
    struct Leaf;
}
"#,
        );
        assert!(qualified.source_modeled.is_none());
        assert!(qualified.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidPrettyHint
                && diagnostic.message.contains("exact unqualified")
        }));

        let inapplicable = model(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::ast_node! {

    struct Item;
}
recursa::ast_node! {

    struct Root {
        #[pretty(separator = leading, trailing_separator = never)]
        value: Item,
    }
}
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::ast_node! {

    #[pretty(group = consistent)]
    #[pretty(group = inconsistent)]
    struct Root {
        first: Leaf,
        second: Leaf,
    }
}
recursa::ast_node! {
    struct Leaf;
}
"#,
        );
        assert!(duplicate.source_modeled.is_none());
        assert!(duplicate.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SourceModelDiagnosticKind::InvalidPrettyHint
                && diagnostic.message.contains("duplicate")
        }));

        let cfg_selected = model_with_cfg(
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"[ \t\r\n]+", keywords { A => "a", B => "b" } }
recursa::ast_node! {

    #[cfg_attr(feature = "pretty-layout", pretty(group = inconsistent))]
    #[tok(A, B)]
    struct Root;
}
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { keywords { ONLY => "only" } }
recursa::ast_node! {

    #[pretty(group = consistent)]
    enum Single {
        #[tok(ONLY)]
        Only,
    }
}
recursa::ast_node! {

    #[pratt]
    #[pretty(indent)]
    enum Atom {
        Value(#[lex(pattern = r"[a-z]+")] Word),
    }
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { COLON => ":", spacing = tight } }
recursa::ast_node! {

    struct Empty;
}
recursa::ast_node! {

    #[tok(COLON, this)]
    struct Root {
        #[pretty(break_before = hard)]
        value: Empty,
    }
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
            r#"recursa::grammar! { module = crate }
recursa::ast_node! {
    struct Leaf;
}
recursa::ast_node! {

    #[pretty(group = consistent)]
    enum Empty {
        #[pretty(group = inconsistent)]
        Value(#[pretty(group = consistent)] Leaf),
    }
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { ignore = r"\s+" }
recursa::ast_node! {
    struct Leaf;
}
recursa::ast_node! {

    #[pretty(break_before = soft)]
    struct Root {
        #[pretty(break_before = soft, break_after = hard)]
        value: Leaf,
    }
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! { punctuation { DOT => "." } }
recursa::ast_node! {
    #[tok(DOT)]
    struct Marker;
}
recursa::ast_node! {
    struct Leaf;
}
recursa::ast_node! {

    #[pretty(break_before = soft)]
    struct Root {
        before: Marker,
        #[pretty(break_before = soft, break_after = hard)]
        value: Leaf,
        after: Marker,
    }
}
recursa::ast_node! {

    enum Choice {
        #[pretty(break_after = hard)]
        Value(Leaf),
    }
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"\s+"
    punctuation { LEFT => "<", spacing = tight, RIGHT => ">", spacing = tight }
}
recursa::ast_node! {

    #[pretty(group = consistent)]
    #[tok(LEFT, RIGHT)]
    struct AllTight;
}
recursa::ast_node! {

    #[pretty(group = inconsistent, break_before = hard)]
    struct HardOnly;
}
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
            r#"recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \n]+"
    punctuation {
        BEFORE => "@", spacing = before,
        AFTER => "%", spacing = after,
    }
}
recursa::ast_node! {
    #[tok(BEFORE)]
    struct EndsBefore;
}
recursa::ast_node! {
    #[tok(AFTER)]
    struct StartsAfter;
}
recursa::ast_node! {
    struct Empty;
}
recursa::ast_node! {

    struct BreakBefore {
        left: EndsBefore,
        #[pretty(break_before = hard)]
        right: Empty,
    }
}
recursa::ast_node! {

    struct BreakAfter {
        #[pretty(break_after = hard)]
        left: Empty,
        right: StartsAfter,
    }
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
