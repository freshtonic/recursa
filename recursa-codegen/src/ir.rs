use std::ops::Range;

use crate::{AtomId, DeclId, KindId, LocationId, SiteId, SourceId, TextPredicateId, TypeId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrammarDescription {
    pub name: String,
    pub kind_count: usize,
    pub max_lookahead: usize,
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
    pub fn new(name: impl Into<String>, kind_count: usize, max_lookahead: usize) -> Self {
        Self {
            name: name.into(),
            kind_count,
            max_lookahead,
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
    pub container: SiteContainer,
}

impl ParseSite {
    pub fn new(
        owner: TypeId,
        target: TypeId,
        declaration: DeclId,
        container: SiteContainer,
    ) -> Self {
        Self {
            owner,
            target,
            declaration,
            container,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SiteContainer {
    Direct,
    Boxed,
    Optional,
    Repeat {
        at_least_one: bool,
        separator: Option<TypeId>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrattPosition {
    Atom,
    Prefix { bp: u8 },
    Infix { lbp: u8, rbp: u8 },
    Postfix { bp: u8 },
}
