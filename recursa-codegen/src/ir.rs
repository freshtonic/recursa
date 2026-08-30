#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Normalized, source-independent grammar representation.
//!
//! Every table uses dense typed IDs, making invalid cross-table references
//! detectable before semantic analysis. Source locations are retained only as
//! diagnostic provenance; authoring syntax and module structure do not affect
//! grammar meaning at this boundary.

use std::ops::Range;

use crate::{AtomId, DeclId, KindId, LocationId, SiteId, SourceId, TextPredicateId, TypeId};

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
    /// Maximum token prefix semantic analysis may retain for dispatch.
    pub max_lookahead: usize,
    /// Grammar-wide equivalence relation for exact-text predicates.
    pub text_equivalence: TextEquivalence,
    /// Fixed token-kind pairs whose nesting may be scanned structurally during dispatch.
    pub balanced_delimiters: Vec<BalancedDelimiter>,
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
}

impl GrammarDescription {
    /// Assembles one normalized grammar and checks collection widths while indexing it.
    pub fn new(
        name: impl Into<String>,
        kind_count: usize,
        max_lookahead: usize,
        text_equivalence: TextEquivalence,
    ) -> Self {
        Self {
            name: name.into(),
            kind_count,
            max_lookahead,
            text_equivalence,
            balanced_delimiters: Vec::new(),
            source_count: 1,
            entry_types: Vec::new(),
            kinds: Vec::new(),
            atoms: Vec::new(),
            declarations: Vec::new(),
            locations: Vec::new(),
            text_predicates: Vec::new(),
            sites: Vec::new(),
            types: Vec::new(),
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

    /// Registers one fixed opening and closing token-kind pair for structural dispatch.
    pub fn push_balanced_delimiter(&mut self, open: KindId, close: KindId) {
        let delimiter = BalancedDelimiter { open, close };
        if !self.balanced_delimiters.contains(&delimiter) {
            self.balanced_delimiters.push(delimiter);
            self.balanced_delimiters.sort_unstable();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One fixed delimiter pair whose nested occurrences have balanced structural meaning.
pub struct BalancedDelimiter {
    /// Opening token kind.
    pub open: KindId,
    /// Closing token kind.
    pub close: KindId,
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
}

impl Declaration {
    /// Records a named declaration at its authored source location.
    pub fn new(name: impl Into<String>, location: LocationId) -> Self {
        Self {
            name: name.into(),
            location,
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
}

impl KindDescription {
    /// Associates a token-kind name with the declaration that introduced it.
    pub fn new(name: impl Into<String>, declaration: DeclId) -> Self {
        Self {
            name: name.into(),
            declaration,
        }
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

impl AtomExpression {
    /// Derives the references from canonical normalized grammar structure.
    pub(crate) fn references(&self) -> Vec<AtomId> {
        match self {
            Self::Kind(_) => Vec::new(),
            Self::Category(members) | Self::Class(members) => members.clone(),
            Self::Target { base, admits } => {
                let mut references = Vec::with_capacity(1 + admits.len());
                references.push(*base);
                references.extend(admits);
                references
            }
        }
    }
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
        }
    }

    /// Sets the minimum Pratt binding power for this site.
    pub const fn with_min_bp(mut self, min_bp: u8) -> Self {
        self.min_bp = min_bp;
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

impl ContainerSemantics {
    /// Derives the separator type from canonical normalized grammar structure.
    pub(crate) const fn separator_type(self) -> Option<TypeId> {
        match self.separator {
            Some(separator) => Some(separator.type_id),
            None => None,
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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
