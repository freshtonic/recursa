use std::collections::BTreeMap;

use crate::{AtomId, DeclId, KindId, KindSet, ProofId, SiteId, TextPredicateId, TypeId};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProvenanceStep {
    DirectDeclaration {
        atom: AtomId,
        kind: KindId,
        declaration: DeclId,
    },
    CategoryMembership {
        category: AtomId,
        declaration: DeclId,
    },
    ClassMembership {
        class: AtomId,
        declaration: DeclId,
    },
    TargetAdmission {
        target: AtomId,
        declaration: DeclId,
    },
    Reference {
        owner: TypeId,
        target: TypeId,
        site: SiteId,
        declaration: DeclId,
    },
    FirstPropagation {
        type_id: TypeId,
        declaration: DeclId,
    },
    NullablePrefix {
        owner: TypeId,
        site: SiteId,
        declaration: DeclId,
    },
    TextPredicate {
        predicate: TextPredicateId,
        declaration: DeclId,
    },
    EntryEof {
        type_id: TypeId,
        declaration: DeclId,
    },
    FollowSuffix {
        owner: TypeId,
        site: SiteId,
        declaration: DeclId,
    },
    FollowPropagation {
        owner: TypeId,
        target: TypeId,
        site: Option<SiteId>,
        declaration: DeclId,
    },
    RepeatContinuation {
        owner: TypeId,
        site: SiteId,
        declaration: DeclId,
    },
}

impl ProvenanceStep {
    pub const fn declaration(&self) -> DeclId {
        match self {
            Self::DirectDeclaration { declaration, .. }
            | Self::CategoryMembership { declaration, .. }
            | Self::ClassMembership { declaration, .. }
            | Self::TargetAdmission { declaration, .. }
            | Self::Reference { declaration, .. }
            | Self::FirstPropagation { declaration, .. }
            | Self::NullablePrefix { declaration, .. }
            | Self::TextPredicate { declaration, .. }
            | Self::EntryEof { declaration, .. }
            | Self::FollowSuffix { declaration, .. }
            | Self::FollowPropagation { declaration, .. }
            | Self::RepeatContinuation { declaration, .. } => *declaration,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProofRank {
    pub hops: u32,
    pub source_order: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofNode {
    pub parent: Option<ProofId>,
    pub step: ProvenanceStep,
    pub rank: ProofRank,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProofArena {
    nodes: Vec<ProofNode>,
    interned: BTreeMap<(Option<ProofId>, ProvenanceStep), ProofId>,
    source_order_by_declaration: Vec<u32>,
}

impl ProofArena {
    pub(crate) fn with_source_order(source_order_by_declaration: Vec<u32>) -> Self {
        Self {
            nodes: Vec::new(),
            interned: BTreeMap::new(),
            source_order_by_declaration,
        }
    }

    pub fn nodes(&self) -> &[ProofNode] {
        &self.nodes
    }

    pub fn get(&self, proof: ProofId) -> Option<&ProofNode> {
        self.nodes.get(proof.index())
    }

    pub fn chain(&self, proof: ProofId) -> ProofChain<'_> {
        ProofChain {
            arena: self,
            next: Some(proof),
        }
    }

    pub(crate) fn intern(&mut self, parent: Option<ProofId>, step: ProvenanceStep) -> ProofId {
        let key = (parent, step.clone());
        if let Some(id) = self.interned.get(&key) {
            return *id;
        }
        let hops = parent
            .and_then(|id| self.get(id))
            .map_or(1, |node| node.rank.hops + 1);
        let rank = ProofRank {
            hops,
            source_order: self
                .source_order_by_declaration
                .get(step.declaration().index())
                .copied()
                .unwrap_or_else(|| step.declaration().get()),
        };
        let id = ProofId::new(self.nodes.len().try_into().expect("proof ID overflow"));
        self.nodes.push(ProofNode {
            parent,
            step: step.clone(),
            rank,
        });
        self.interned.insert(key, id);
        id
    }

    pub(crate) fn compare(&self, left: ProofId, right: ProofId) -> std::cmp::Ordering {
        let left_node = self.get(left).expect("valid proof ID");
        let right_node = self.get(right).expect("valid proof ID");
        left_node
            .rank
            .cmp(&right_node.rank)
            .then_with(|| left_node.step.cmp(&right_node.step))
            .then_with(|| self.compare_parents(left_node.parent, right_node.parent))
    }

    fn compare_parents(&self, left: Option<ProofId>, right: Option<ProofId>) -> std::cmp::Ordering {
        match (left, right) {
            (Some(left), Some(right)) => self.compare(left, right),
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
        }
    }
}

pub struct ProofChain<'a> {
    arena: &'a ProofArena,
    next: Option<ProofId>,
}

impl<'a> Iterator for ProofChain<'a> {
    type Item = &'a ProofNode;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next?;
        let node = self.arena.get(id)?;
        self.next = node.parent;
        Some(node)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenancedKindSet {
    kinds: KindSet,
    proof_by_kind: Vec<Option<ProofId>>,
}

impl ProvenancedKindSet {
    pub(crate) fn empty(kind_count: usize) -> Self {
        Self {
            kinds: KindSet::empty(kind_count),
            proof_by_kind: vec![None; kind_count],
        }
    }

    pub fn kinds(&self) -> &KindSet {
        &self.kinds
    }

    pub fn proof(&self, kind: KindId) -> Option<ProofId> {
        self.proof_by_kind.get(kind.index()).copied().flatten()
    }

    pub(crate) fn insert(&mut self, kind: KindId, proof: ProofId, arena: &ProofArena) {
        self.kinds
            .insert(kind)
            .expect("kind is within grammar width");
        let slot = &mut self.proof_by_kind[kind.index()];
        if slot.is_none_or(|current| arena.compare(proof, current).is_lt()) {
            *slot = Some(proof);
        }
        debug_assert_eq!(self.kinds.contains(kind), slot.is_some());
    }

    pub(crate) fn merge(&mut self, other: &Self, arena: &ProofArena) {
        debug_assert_eq!(self.kinds.kind_count(), other.kinds.kind_count());
        for kind in other.kinds.iter() {
            self.insert(kind, other.proof(kind).expect("bit has proof"), arena);
        }
    }
}
