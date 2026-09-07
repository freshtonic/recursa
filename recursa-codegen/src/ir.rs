#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Normalized, source-independent grammar representation.
//!
//! Every table uses dense typed IDs, making invalid cross-table references
//! detectable before semantic analysis. Source locations are retained only as
//! diagnostic provenance; authoring syntax and module structure do not affect
//! grammar meaning at this boundary.

use std::ops::Range;

use crate::{
    AtomId, DeclId, KindId, LocationId, PrecedenceLevelId, SiteId, SourceId, TextPredicateId,
    TypeId,
};

#[derive(Clone, Debug, Eq, PartialEq)]
/// A complete normalized grammar ready for source-independent analysis.
///
/// Vectors are dense ID tables: an ID's numeric index selects the matching
/// entry. Construct values through the `push_*` methods to preserve that
/// invariant.
pub struct GrammarDescription {
    /// Human-readable grammar name used in diagnostics and projections.
    pub name: String,
    /// Number of grammar-local token kinds representable by each [`KindSet`](crate::KindSet).
    pub kind_count: usize,
    /// Grammar-wide equivalence relation for exact-text predicates.
    pub text_equivalence: TextEquivalence,
    /// Number of source identities accepted by [`Location`] values.
    pub source_count: u32,
    /// Root types whose FOLLOW set is seeded with end-of-input.
    pub entry_types: Vec<TypeId>,
    /// Dense token-kind descriptions indexed by [`KindId`].
    pub kinds: Vec<KindDescription>,
    /// Dense lexical atom expressions indexed by [`AtomId`].
    pub atoms: Vec<AtomDescription>,
    /// Dense declaration metadata indexed by [`DeclId`].
    pub declarations: Vec<Declaration>,
    /// Dense source locations indexed by [`LocationId`].
    pub locations: Vec<Location>,
    /// Dense text refinements indexed by [`TextPredicateId`].
    pub text_predicates: Vec<TextPredicate>,
    /// Dense parse sites indexed by [`SiteId`].
    pub sites: Vec<ParseSite>,
    /// Dense grammar types indexed by [`TypeId`].
    pub types: Vec<TypeDescription>,
    /// Lookahead filters in declaration order; a merged kind is a kind no lexer produces.
    pub lookahead_filters: Vec<LookaheadFilter>,
    /// The `precedence { ... }` declaration: levels from lowest to highest binding power,
    /// indexed by [`PrecedenceLevelId`]; empty without one.
    pub precedence: Vec<PrecedenceLevel>,
    /// Authored resolutions for specific unresolved LR conflict cells, validated after precedence
    /// resolution.
    pub lr_conflict_resolutions: Vec<LrConflictResolution>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The parser action an authored LR conflict declaration requires.
pub enum LrConflictAction {
    /// Keep the terminal shift.
    Shift,
    /// Reduce by the uniquely selected rule lowered from the declaration's owner.
    Reduce,
    /// Reject the lookahead as a syntax error.
    Error,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// A stable authored source identity used to select lowered LR rules.
pub enum LrRuleOwner {
    /// Every construction rule lowered from one concrete Node application.
    Type(TypeId),
    /// Every rule lowered from one enum or Pratt variant declaration.
    Variant(DeclId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One fail-closed declaration resolving an exact family of otherwise unresolved LR cells.
///
/// `owner` selects the reducing rule (and the winning rule for a reduce action), while `against`
/// fingerprints the opposing authored source identity. Every named lookahead must match and the
/// total number of cells must equal `expected`; otherwise table-driven generation fails.
pub struct LrConflictResolution {
    /// The authored Node or variant whose lowered rule reduces in every selected cell.
    pub owners: Vec<LrRuleOwner>,
    /// The opposing authored Node or variant required in each selected state.
    pub against: Vec<LrRuleOwner>,
    /// The explicit parser action.
    pub action: LrConflictAction,
    /// Exact lookahead kinds this declaration claims, in authored order without duplicates.
    pub lookaheads: Vec<KindId>,
    /// Exact number of conflict cells the declaration must match.
    pub expected: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The associativity of one level of the `precedence { ... }` declaration, bison's `%left`,
/// `%right` and `%nonassoc`.
pub enum PrecedenceAssociativity {
    /// `left`: on a tie the reduce wins.
    Left,
    /// `right`: on a tie the shift wins.
    Right,
    /// `nonassoc`: a tie is a syntax error.
    NonAssociative,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One level of the `precedence { ... }` declaration over token kinds.
///
/// Only the table-driven lowering reads it: every kind at the level, as a lookahead, has the
/// level's binding power and associativity, and a rule outside the Pratt hierarchy takes the
/// level of its last such terminal, or of the level its declaration's
/// [`precedence`](Declaration::precedence) override names.
pub struct PrecedenceLevel {
    /// The level's associativity.
    pub associativity: PrecedenceAssociativity,
    /// The level's binding power on the Pratt scale, strictly above the previous level's.
    pub binding_power: u8,
    /// The token kinds at this level, in authored order without duplicates; empty for a level
    /// of precedence-only names, which only an override can name.
    pub kinds: Vec<KindId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One `lookahead { MERGED = TOKEN before { TRIGGER, ... } }` declaration over token kinds.
///
/// The parser's token feed produces `merged` when a `token` record is followed by a `triggers`
/// record; LR lowering substitutes `merged` for `token` at every rule position a trigger can
/// follow.
pub struct LookaheadFilter {
    /// The merged kind, which exists in the kind table but has no lexical rule.
    pub merged: KindId,
    /// The lexed kind the filter merges.
    pub token: KindId,
    /// The lexed kinds that trigger the merge, in authored order without duplicates.
    pub triggers: Vec<KindId>,
}

impl GrammarDescription {
    /// Assembles one normalized grammar and checks collection widths while indexing it.
    pub fn new(
        name: impl Into<String>,
        kind_count: usize,
        text_equivalence: TextEquivalence,
    ) -> Self {
        Self {
            name: name.into(),
            kind_count,
            text_equivalence,
            source_count: 1,
            entry_types: Vec::new(),
            kinds: Vec::new(),
            atoms: Vec::new(),
            declarations: Vec::new(),
            locations: Vec::new(),
            text_predicates: Vec::new(),
            sites: Vec::new(),
            types: Vec::new(),
            lookahead_filters: Vec::new(),
            precedence: Vec::new(),
            lr_conflict_resolutions: Vec::new(),
        }
    }

    /// Returns the ID that the next pushed type will receive.
    pub fn next_type_id(&self) -> TypeId {
        TypeId::new(self.types.len().try_into().expect("type ID overflow"))
    }

    /// Appends a location and returns its dense ID.
    pub fn push_location(&mut self, location: Location) -> LocationId {
        let id = LocationId::new(
            self.locations
                .len()
                .try_into()
                .expect("location ID overflow"),
        );
        self.locations.push(location);
        id
    }

    /// Appends a declaration and returns its dense ID.
    pub fn push_declaration(&mut self, declaration: Declaration) -> DeclId {
        let id = DeclId::new(
            self.declarations
                .len()
                .try_into()
                .expect("declaration ID overflow"),
        );
        self.declarations.push(declaration);
        id
    }

    /// Appends a token kind and returns its dense ID.
    pub fn push_kind(&mut self, kind: KindDescription) -> KindId {
        let id = KindId::new(self.kinds.len().try_into().expect("kind ID overflow"));
        self.kinds.push(kind);
        id
    }

    /// Appends an atom and returns its dense ID.
    pub fn push_atom(&mut self, atom: AtomDescription) -> AtomId {
        let id = AtomId::new(self.atoms.len().try_into().expect("atom ID overflow"));
        self.atoms.push(atom);
        id
    }

    /// Appends a text predicate and returns its dense ID.
    pub fn push_text_predicate(&mut self, predicate: TextPredicate) -> TextPredicateId {
        let id = TextPredicateId::new(
            self.text_predicates
                .len()
                .try_into()
                .expect("text-predicate ID overflow"),
        );
        self.text_predicates.push(predicate);
        id
    }

    /// Appends a parse site and returns its dense ID.
    pub fn push_site(&mut self, site: ParseSite) -> SiteId {
        let id = SiteId::new(self.sites.len().try_into().expect("site ID overflow"));
        self.sites.push(site);
        id
    }

    /// Appends a grammar type and returns its dense ID.
    pub fn push_type(&mut self, description: TypeDescription) -> TypeId {
        let id = self.next_type_id();
        self.types.push(description);
        id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A byte range in one normalized source identity.
pub struct Location {
    /// Source containing the range.
    pub source: SourceId,
    /// Half-open UTF-8 byte range within the source.
    pub range: Range<u32>,
}

impl Location {
    /// Associates a byte range with its entry in the grammar source table.
    pub fn new(source: SourceId, range: Range<u32>) -> Self {
        Self { source, range }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Human-facing metadata shared by a normalized declaration.
pub struct Declaration {
    /// Stable name used in facts and diagnostics.
    pub name: String,
    /// Authored location that introduced the declaration.
    pub location: LocationId,
    /// The `precedence` level a `#[parse(prec = NAME)]` at this declaration names, bison's
    /// `%prec`: the rules lowered from the declaration take that level's precedence.
    pub precedence: Option<PrecedenceLevelId>,
    /// The authored item this declaration was materialized from, for diagnostics.
    pub authored: Option<AuthoredSite>,
}

impl Declaration {
    /// Records a named declaration at its authored source location.
    pub fn new(name: impl Into<String>, location: LocationId) -> Self {
        Self {
            name: name.into(),
            location,
            precedence: None,
            authored: None,
        }
    }

    /// Records the `precedence` level a `#[parse(prec = NAME)]` at this declaration names.
    #[must_use]
    pub fn with_precedence(mut self, level: PrecedenceLevelId) -> Self {
        self.precedence = Some(level);
        self
    }

    /// Records the authored item that diagnostics should point at for this declaration.
    #[must_use]
    pub fn with_authored(mut self, authored: AuthoredSite) -> Self {
        self.authored = Some(authored);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// The authored item a materialized parse site came from, as diagnostics name it.
pub struct AuthoredSite {
    /// Authored Node identifier.
    pub node: String,
    /// Authored variant identifier when the site belongs to an enum or Pratt variant.
    pub variant: Option<String>,
    /// Authored field identifier when the field is named.
    pub field: Option<String>,
    /// Position of the field among its siblings, for unnamed fields.
    pub index: usize,
    /// What the site represents relative to the authored field.
    pub role: AuthoredRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// What a materialized site represents relative to its authored field.
pub enum AuthoredRole {
    /// The field's own semantic use site.
    Field,
    /// The optional fixed-token sequence of a `bool` presence field.
    Presence,
    /// A fixed-token group attached by `#[tok(...)]`, with or without a field.
    Attachment,
}

impl AuthoredSite {
    /// Names the authored field itself, without its owner.
    fn field_name(&self) -> String {
        match (&self.field, &self.variant) {
            (Some(field), Some(variant)) => format!("`{variant}::{field}`"),
            (Some(field), None) => format!("`{field}`"),
            (None, Some(variant)) => format!("field {} of `{variant}`", self.index),
            (None, None) => format!("field {}", self.index),
        }
    }

    /// Describes the site in prose, for example "`options` in `DropDatabase`".
    pub fn phrase(&self) -> String {
        let node = &self.node;
        match self.role {
            AuthoredRole::Field => format!("{} in `{node}`", self.field_name()),
            AuthoredRole::Presence => format!("the presence of {} in `{node}`", self.field_name()),
            AuthoredRole::Attachment => match (&self.field, &self.variant) {
                (Some(_), _) => {
                    format!(
                        "the optional token group attached to {} in `{node}`",
                        self.field_name()
                    )
                }
                (None, Some(variant)) => {
                    format!("the optional token group of variant `{variant}` in `{node}`")
                }
                (None, None) => format!("the optional token group of `{node}`"),
            },
        }
    }

    /// Names the item an attribute for this site goes on, for example "`options`".
    pub fn attribute_target(&self) -> String {
        match (&self.field, &self.variant) {
            (Some(_), _) => self.field_name(),
            (None, Some(variant)) if self.role == AuthoredRole::Attachment => {
                format!("variant `{variant}`")
            }
            (None, Some(variant)) => format!("field {} of `{variant}`", self.index),
            (None, None) if self.role == AuthoredRole::Attachment => format!("`{}`", self.node),
            (None, None) => format!("field {} of `{}`", self.index, self.node),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A grammar-local token kind and its declaring source identity.
pub struct KindDescription {
    /// Supplies the name to normalized grammar consumers.
    pub name: String,
    /// Supplies the declaration to normalized grammar consumers.
    pub declaration: DeclId,
    /// Authored token identifier, as `#[tok(...)]` and `#[greedy(...)]` spell this kind.
    pub authored: Option<String>,
}

impl KindDescription {
    /// Associates a token-kind name with the declaration that introduced it.
    pub fn new(name: impl Into<String>, declaration: DeclId) -> Self {
        Self {
            name: name.into(),
            declaration,
            authored: None,
        }
    }

    /// Records the authored token identifier that names this kind in attributes.
    #[must_use]
    pub fn with_authored(mut self, authored: impl Into<String>) -> Self {
        self.authored = Some(authored.into());
        self
    }

    /// Returns the spelling that `#[greedy(...)]` uses for this kind.
    pub fn attribute_spelling(&self) -> &str {
        self.authored.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A named lexical atom expression and its declaring source identity.
pub struct AtomDescription {
    /// Supplies the name to normalized grammar consumers.
    pub name: String,
    /// Supplies the declaration to normalized grammar consumers.
    pub declaration: DeclId,
    /// Supplies the expression to normalized grammar consumers.
    pub expression: AtomExpression,
}

impl AtomDescription {
    /// Defines a named atom from its declaration and normalized expression.
    pub fn new(name: impl Into<String>, declaration: DeclId, expression: AtomExpression) -> Self {
        Self {
            name: name.into(),
            declaration,
            expression,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Supplies the closed ways a lexical atom contributes token kinds to normalized grammar consumers.
pub enum AtomExpression {
    /// One concrete token kind.
    Kind(KindId),
    /// Union of other atom expressions used as a grammar category.
    Category(Vec<AtomId>),
    /// Union of other atom expressions used as a lexical class.
    Class(Vec<AtomId>),
    /// A base atom extended with contextually admitted atoms.
    Target {
        #[doc = "Base atom whose recognized kind is retained."]
        base: AtomId,
        #[doc = "Contextual atoms admitted by the parser target."]
        admits: Vec<AtomId>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Supplies the grammar-wide comparison policy for exact token text to normalized grammar consumers.
pub enum TextEquivalence {
    /// Unicode scalar strings must match exactly.
    Sensitive,
    /// ASCII letters compare case-insensitively; other scalars remain exact.
    AsciiInsensitive,
}

impl TextEquivalence {
    /// Compares two spellings under this policy.
    pub fn equivalent(self, left: &str, right: &str) -> bool {
        match self {
            Self::Sensitive => left == right,
            Self::AsciiInsensitive => left.eq_ignore_ascii_case(right),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A text refinement applied after a token-kind match.
pub enum TextPredicate {
    /// Any text accepted by the token kind.
    Any,
    /// Exactly one spelling under [`TextEquivalence`].
    Exact(String),
    /// Any spelling from a finite admitted set.
    OneOf(Vec<String>),
    /// Any spelling except members of a finite excluded set.
    NotIn(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One typed reference from an owning grammar type to a target type.
pub struct ParseSite {
    /// Type containing the reference.
    pub owner: TypeId,
    /// Referenced grammar type.
    pub target: TypeId,
    /// Source declaration responsible for the reference.
    pub declaration: DeclId,
    /// Cardinality and boxing around the target.
    pub container: ContainerShape,
    /// Minimum Pratt binding power required at this site.
    pub min_bp: u8,
    /// The variants of the target Pratt type this site excludes
    /// (`#[parse(pratt(exclude(...)))]`), in ascending order without duplicates: an atom or
    /// prefix variant leaves the operand's nud selection and its FIRST set, an infix or
    /// postfix one its extenders. Empty at every other site, including one that targets the
    /// attachment unit around such an operand only through the unit's own semantic site.
    ///
    /// The generated loop never selects an excluded variant at this site or in the
    /// recursive operands it parses, so analysis leaves an excluded extender's starters out
    /// of the site's active extenders instead of reporting them as a caller-FOLLOW overlap.
    pub pratt_excluded: Vec<DeclId>,
}

impl ParseSite {
    /// Records one typed parse edge and the grammar position that owns it.
    pub fn new(
        owner: TypeId,
        target: TypeId,
        declaration: DeclId,
        container: ContainerShape,
    ) -> Self {
        Self {
            owner,
            target,
            declaration,
            container,
            min_bp: 0,
            pratt_excluded: Vec::new(),
        }
    }

    /// Sets the minimum Pratt binding power for this site.
    pub const fn with_min_bp(mut self, min_bp: u8) -> Self {
        self.min_bp = min_bp;
        self
    }

    /// Records the target's Pratt led variants that this site excludes.
    pub fn with_pratt_excluded(mut self, excluded: Vec<DeclId>) -> Self {
        self.pratt_excluded = excluded;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Semantic cardinality of a repeated parse site.
pub enum RepetitionCardinality {
    /// Identifies the zero or more normalized grammar form.
    ZeroOrMore,
    /// Identifies the one or more normalized grammar form.
    OneOrMore,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Separator type and trailing-separator policy for a repetition.
pub struct RepeatSeparator {
    /// Grammar type parsed between repeated elements.
    pub type_id: TypeId,
    /// Whether one final separator may appear after the last element.
    pub optional_trailing: bool,
}

impl RepeatSeparator {
    /// Creates a separator policy.
    pub const fn new(type_id: TypeId, optional_trailing: bool) -> Self {
        Self {
            type_id,
            optional_trailing,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Recursive supported-container shape surrounding a parse target.
pub enum ContainerShape {
    /// Exactly one unboxed target.
    Direct,
    /// A storage-only `Box` layer that does not change cardinality.
    Boxed(Box<ContainerShape>),
    /// Zero or one occurrence of the nested shape.
    Optional(Box<ContainerShape>),
    /// A repeated nested shape with optional separator syntax.
    Repeat {
        /// Supplies the element to normalized grammar consumers.
        element: Box<ContainerShape>,
        /// Supplies the cardinality to normalized grammar consumers.
        cardinality: RepetitionCardinality,
        /// Supplies the separator to normalized grammar consumers.
        separator: Option<RepeatSeparator>,
    },
}

impl ContainerShape {
    /// Wraps `element` in a storage-only `Box` layer.
    pub fn boxed(element: Self) -> Self {
        Self::Boxed(Box::new(element))
    }

    /// Wraps `element` in an optional layer.
    pub fn optional(element: Self) -> Self {
        Self::Optional(Box::new(element))
    }

    /// Creates a zero-or-more repetition with an optional separator type.
    pub fn zero_or_more(element: Self, separator: Option<TypeId>) -> Self {
        Self::Repeat {
            element: Box::new(element),
            cardinality: RepetitionCardinality::ZeroOrMore,
            separator: separator.map(|type_id| RepeatSeparator::new(type_id, false)),
        }
    }

    /// Creates a one-or-more repetition with an optional separator type.
    pub fn one_or_more(element: Self, separator: Option<TypeId>) -> Self {
        Self::Repeat {
            element: Box::new(element),
            cardinality: RepetitionCardinality::OneOrMore,
            separator: separator.map(|type_id| RepeatSeparator::new(type_id, false)),
        }
    }

    /// Creates a repetition with its complete separator policy.
    pub fn repeated_with_separator(
        element: Self,
        cardinality: RepetitionCardinality,
        separator: Option<RepeatSeparator>,
    ) -> Self {
        Self::Repeat {
            element: Box::new(element),
            cardinality,
            separator,
        }
    }

    /// Derives the semantics from canonical normalized grammar structure.
    pub(crate) fn semantics(&self) -> Option<ContainerSemantics> {
        let mut layers = Vec::new();
        self.collect_cardinality_layers(&mut layers);
        match layers.as_slice() {
            [] => Some(ContainerSemantics {
                cardinality: ContainerCardinality::One,
                separator: None,
            }),
            [CardinalityLayer::Optional] => Some(ContainerSemantics {
                cardinality: ContainerCardinality::Optional,
                separator: None,
            }),
            [
                CardinalityLayer::Repeat {
                    cardinality: RepetitionCardinality::ZeroOrMore,
                    separator,
                },
            ] => Some(ContainerSemantics {
                cardinality: ContainerCardinality::ZeroOrMore,
                separator: *separator,
            }),
            [
                CardinalityLayer::Repeat {
                    cardinality: RepetitionCardinality::OneOrMore,
                    separator,
                },
            ] => Some(ContainerSemantics {
                cardinality: ContainerCardinality::OneOrMore,
                separator: *separator,
            }),
            [
                CardinalityLayer::Optional,
                CardinalityLayer::Repeat {
                    cardinality: RepetitionCardinality::OneOrMore,
                    separator,
                },
            ] => Some(ContainerSemantics {
                cardinality: ContainerCardinality::OptionalOneOrMore,
                separator: *separator,
            }),
            _ => None,
        }
    }

    /// Visits repetition separators from the outermost container layer inward.
    #[cfg(test)]
    pub(crate) fn visit_separators(&self, visit: &mut impl FnMut(TypeId)) {
        match self {
            Self::Direct => {}
            Self::Boxed(element) | Self::Optional(element) => element.visit_separators(visit),
            Self::Repeat {
                element, separator, ..
            } => {
                if let Some(separator) = separator {
                    visit(separator.type_id);
                }
                element.visit_separators(visit);
            }
        }
    }

    /// Flattens nested containers into their semantic cardinality sequence.
    fn collect_cardinality_layers(&self, layers: &mut Vec<CardinalityLayer>) {
        match self {
            Self::Direct => {}
            Self::Boxed(element) => element.collect_cardinality_layers(layers),
            Self::Optional(element) => {
                layers.push(CardinalityLayer::Optional);
                element.collect_cardinality_layers(layers);
            }
            Self::Repeat {
                element,
                cardinality,
                separator,
            } => {
                layers.push(CardinalityLayer::Repeat {
                    cardinality: *cardinality,
                    separator: *separator,
                });
                element.collect_cardinality_layers(layers);
            }
        }
    }
}

impl std::fmt::Display for ContainerShape {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => formatter.write_str("T"),
            Self::Boxed(element) => write!(formatter, "Box<{element}>"),
            Self::Optional(element) => write!(formatter, "Option<{element}>"),
            Self::Repeat {
                element,
                cardinality: RepetitionCardinality::ZeroOrMore,
                ..
            } => write!(formatter, "Vec<{element}>"),
            Self::Repeat {
                element,
                cardinality: RepetitionCardinality::OneOrMore,
                ..
            } => write!(formatter, "Vec1<{element}>"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the container cardinality forms preserved in normalized grammar.
pub(crate) enum ContainerCardinality {
    /// Identifies the one normalized grammar form.
    One,
    /// Identifies the optional normalized grammar form.
    Optional,
    /// Identifies the zero or more normalized grammar form.
    ZeroOrMore,
    /// Identifies the one or more normalized grammar form.
    OneOrMore,
    /// Identifies the optional one or more normalized grammar form.
    OptionalOneOrMore,
}

impl ContainerCardinality {
    /// Reports whether this value is repeated.
    pub(crate) const fn is_repeated(self) -> bool {
        matches!(
            self,
            Self::ZeroOrMore | Self::OneOrMore | Self::OptionalOneOrMore
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the container semantics within the normalized grammar IR.
pub(crate) struct ContainerSemantics {
    /// Supplies the cardinality to normalized grammar consumers.
    pub cardinality: ContainerCardinality,
    /// Supplies the separator to normalized grammar consumers.
    pub separator: Option<RepeatSeparator>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the cardinality layer forms preserved in normalized grammar.
enum CardinalityLayer {
    /// Identifies the optional normalized grammar form.
    Optional,
    /// Identifies the repeat normalized grammar form.
    Repeat {
        /// Supplies the cardinality to normalized grammar consumers.
        cardinality: RepetitionCardinality,
        /// Supplies the separator to normalized grammar consumers.
        separator: Option<RepeatSeparator>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A named normalized grammar type and its semantic shape.
pub struct TypeDescription {
    /// Supplies the name to normalized grammar consumers.
    pub name: String,
    /// Supplies the declaration to normalized grammar consumers.
    pub declaration: DeclId,
    /// Supplies the shape to normalized grammar consumers.
    pub shape: TypeShape,
}

impl TypeDescription {
    /// Defines a named grammar type from its declaration and normalized shape.
    pub fn new(name: impl Into<String>, declaration: DeclId, shape: TypeShape) -> Self {
        Self {
            name: name.into(),
            declaration,
            shape,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Supplies the closed semantic forms of normalized grammar types to normalized grammar consumers.
pub enum TypeShape {
    /// Identifies the token normalized grammar form.
    Token {
        /// Supplies the atom to normalized grammar consumers.
        atom: AtomId,
        /// Supplies the text predicate to normalized grammar consumers.
        text_predicate: Option<TextPredicateId>,
    },
    /// Identifies the alias normalized grammar form.
    Alias {
        /// Supplies the site to normalized grammar consumers.
        site: SiteId,
    },
    /// Identifies the sequence normalized grammar form.
    Sequence {
        /// Supplies the sites to normalized grammar consumers.
        sites: Vec<SiteId>,
    },
    /// Identifies the enum normalized grammar form.
    Enum {
        /// Supplies the alternatives to normalized grammar consumers.
        alternatives: Vec<Alternative>,
    },
    /// Identifies the pratt normalized grammar form.
    Pratt {
        /// Supplies the variants to normalized grammar consumers.
        variants: Vec<PrattVariant>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One enum alternative and its ordered parse sites.
pub struct Alternative {
    /// Supplies the declaration to normalized grammar consumers.
    pub declaration: DeclId,
    /// Supplies the sites to normalized grammar consumers.
    pub sites: Vec<SiteId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One Pratt variant, role, binding power, and ordered parse sites.
pub struct PrattVariant {
    /// Supplies the declaration to normalized grammar consumers.
    pub declaration: DeclId,
    /// Supplies the position to normalized grammar consumers.
    pub position: PrattPosition,
    /// Supplies the sites to normalized grammar consumers.
    pub sites: Vec<SiteId>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Structural role and binding powers of a Pratt variant.
pub enum PrattPosition {
    /// Identifies the atom normalized grammar form.
    Atom,
    /// Identifies the prefix normalized grammar form.
    Prefix {
        #[doc = "Right binding power of the prefix operand."]
        bp: u8,
    },
    /// Identifies the infix normalized grammar form.
    Infix {
        #[doc = "Left binding power required to select the operator."]
        lbp: u8,
        #[doc = "Right binding power used to parse the operand."]
        rbp: u8,
    },
    /// Identifies the postfix normalized grammar form.
    Postfix {
        #[doc = "Left binding power required to select the operator."]
        bp: u8,
    },
}
