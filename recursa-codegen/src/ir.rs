use std::ops::Range;

use crate::{AtomId, DeclId, KindId, LocationId, SiteId, SourceId, TextPredicateId, TypeId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrammarDescription {
    pub name: String,
    pub kind_count: usize,
    pub max_lookahead: usize,
    pub text_equivalence: TextEquivalence,
    pub source_count: u32,
    pub entry_types: Vec<TypeId>,
    pub kinds: Vec<KindDescription>,
    pub atoms: Vec<AtomDescription>,
    pub declarations: Vec<Declaration>,
    pub locations: Vec<Location>,
    pub text_predicates: Vec<TextPredicate>,
    pub sites: Vec<ParseSite>,
    pub types: Vec<TypeDescription>,
}

impl GrammarDescription {
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

    pub fn next_type_id(&self) -> TypeId {
        TypeId::new(self.types.len().try_into().expect("type ID overflow"))
    }

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

    pub fn push_kind(&mut self, kind: KindDescription) -> KindId {
        let id = KindId::new(self.kinds.len().try_into().expect("kind ID overflow"));
        self.kinds.push(kind);
        id
    }

    pub fn push_atom(&mut self, atom: AtomDescription) -> AtomId {
        let id = AtomId::new(self.atoms.len().try_into().expect("atom ID overflow"));
        self.atoms.push(atom);
        id
    }

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

    pub fn push_site(&mut self, site: ParseSite) -> SiteId {
        let id = SiteId::new(self.sites.len().try_into().expect("site ID overflow"));
        self.sites.push(site);
        id
    }

    pub fn push_type(&mut self, description: TypeDescription) -> TypeId {
        let id = self.next_type_id();
        self.types.push(description);
        id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    pub source: SourceId,
    pub range: Range<u32>,
}

impl Location {
    pub fn new(source: SourceId, range: Range<u32>) -> Self {
        Self { source, range }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub location: LocationId,
}

impl Declaration {
    pub fn new(name: impl Into<String>, location: LocationId) -> Self {
        Self {
            name: name.into(),
            location,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KindDescription {
    pub name: String,
    pub declaration: DeclId,
}

impl KindDescription {
    pub fn new(name: impl Into<String>, declaration: DeclId) -> Self {
        Self {
            name: name.into(),
            declaration,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomDescription {
    pub name: String,
    pub declaration: DeclId,
    pub expression: AtomExpression,
}

impl AtomDescription {
    pub fn new(name: impl Into<String>, declaration: DeclId, expression: AtomExpression) -> Self {
        Self {
            name: name.into(),
            declaration,
            expression,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AtomExpression {
    Kind(KindId),
    Category(Vec<AtomId>),
    Class(Vec<AtomId>),
    Target { base: AtomId, admits: Vec<AtomId> },
}

impl AtomExpression {
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
pub enum TextEquivalence {
    Sensitive,
    AsciiInsensitive,
}

impl TextEquivalence {
    pub fn equivalent(self, left: &str, right: &str) -> bool {
        match self {
            Self::Sensitive => left == right,
            Self::AsciiInsensitive => left.eq_ignore_ascii_case(right),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextPredicate {
    Any,
    Exact(String),
    OneOf(Vec<String>),
    NotIn(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseSite {
    pub owner: TypeId,
    pub target: TypeId,
    pub declaration: DeclId,
    pub container: ContainerShape,
    pub min_bp: u8,
}

impl ParseSite {
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

    pub const fn with_min_bp(mut self, min_bp: u8) -> Self {
        self.min_bp = min_bp;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RepetitionCardinality {
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RepeatSeparator {
    pub type_id: TypeId,
    pub optional_trailing: bool,
}

impl RepeatSeparator {
    pub const fn new(type_id: TypeId, optional_trailing: bool) -> Self {
        Self {
            type_id,
            optional_trailing,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerShape {
    Direct,
    Boxed(Box<ContainerShape>),
    Optional(Box<ContainerShape>),
    Repeat {
        element: Box<ContainerShape>,
        cardinality: RepetitionCardinality,
        separator: Option<RepeatSeparator>,
    },
}

impl ContainerShape {
    pub fn boxed(element: Self) -> Self {
        Self::Boxed(Box::new(element))
    }

    pub fn optional(element: Self) -> Self {
        Self::Optional(Box::new(element))
    }

    pub fn zero_or_more(element: Self, separator: Option<TypeId>) -> Self {
        Self::Repeat {
            element: Box::new(element),
            cardinality: RepetitionCardinality::ZeroOrMore,
            separator: separator.map(|type_id| RepeatSeparator::new(type_id, false)),
        }
    }

    pub fn one_or_more(element: Self, separator: Option<TypeId>) -> Self {
        Self::Repeat {
            element: Box::new(element),
            cardinality: RepetitionCardinality::OneOrMore,
            separator: separator.map(|type_id| RepeatSeparator::new(type_id, false)),
        }
    }

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
pub(crate) enum ContainerCardinality {
    One,
    Optional,
    ZeroOrMore,
    OneOrMore,
    OptionalOneOrMore,
}

impl ContainerCardinality {
    pub(crate) const fn is_repeated(self) -> bool {
        matches!(
            self,
            Self::ZeroOrMore | Self::OneOrMore | Self::OptionalOneOrMore
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ContainerSemantics {
    pub cardinality: ContainerCardinality,
    pub separator: Option<RepeatSeparator>,
}

impl ContainerSemantics {
    pub(crate) const fn separator_type(self) -> Option<TypeId> {
        match self.separator {
            Some(separator) => Some(separator.type_id),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CardinalityLayer {
    Optional,
    Repeat {
        cardinality: RepetitionCardinality,
        separator: Option<RepeatSeparator>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeDescription {
    pub name: String,
    pub declaration: DeclId,
    pub shape: TypeShape,
}

impl TypeDescription {
    pub fn new(name: impl Into<String>, declaration: DeclId, shape: TypeShape) -> Self {
        Self {
            name: name.into(),
            declaration,
            shape,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeShape {
    Token {
        atom: AtomId,
        text_predicate: Option<TextPredicateId>,
    },
    Alias {
        site: SiteId,
    },
    Sequence {
        sites: Vec<SiteId>,
    },
    Enum {
        alternatives: Vec<Alternative>,
    },
    Pratt {
        variants: Vec<PrattVariant>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Alternative {
    pub declaration: DeclId,
    pub sites: Vec<SiteId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrattVariant {
    pub declaration: DeclId,
    pub position: PrattPosition,
    pub sites: Vec<SiteId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PrattPosition {
    Atom,
    Prefix { bp: u8 },
    Infix { lbp: u8, rbp: u8 },
    Postfix { bp: u8 },
}
