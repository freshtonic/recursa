//! Canonical proof chains explaining derived semantic facts.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

use std::collections::BTreeMap;

use crate::{
    AtomId, DeclId, KindId, KindSet, PrattPosition, ProofId, SiteId, TextPredicateId, TypeId,
};

#[cfg(test)]
std::thread_local! {
    /// Recursive proof comparisons performed on the current test thread.
    static PROOF_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
/// Resets the recursive proof-comparison counter for the current test thread.
fn reset_proof_comparisons_for_test() {
    PROOF_COMPARISONS.set(0);
}

#[cfg(test)]
/// Returns recursive proof comparisons performed on the current test thread.
fn proof_comparisons_for_test() -> usize {
    PROOF_COMPARISONS.get()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One typed derivation step in a canonical semantic proof chain.
pub enum ProvenanceStep {
    /// A token kind follows directly from an authored atom declaration.
    DirectDeclaration {
        /// Atom whose derived membership is explained.
        atom: AtomId,
        /// Token kind established by the declaration.
        kind: KindId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// An atom contributes through category membership.
    CategoryMembership {
        /// Category through which membership propagates.
        category: AtomId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// An atom contributes through token-class membership.
    ClassMembership {
        /// Token class through which membership propagates.
        class: AtomId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// A parsing target admits a reclaimable token kind.
    TargetAdmission {
        /// Target receiving or supplying the propagated fact.
        target: AtomId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// A grammar-semantic reference propagates facts from its target.
    Reference {
        /// Grammar type whose fact is being derived.
        owner: TypeId,
        /// Target receiving or supplying the propagated fact.
        target: TypeId,
        /// Grammar position at which propagation occurs.
        site: SiteId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// FIRST propagates through a referenced grammar type.
    FirstPropagation {
        /// Grammar type receiving the derived fact.
        type_id: TypeId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// A nullable prefix exposes the following grammar site.
    NullablePrefix {
        /// Grammar type whose fact is being derived.
        owner: TypeId,
        /// Grammar position at which propagation occurs.
        site: SiteId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// A Pratt led variant contributes an active extender.
    PrattExtender {
        /// Pratt grammar type owning the variant.
        pratt: TypeId,
        /// Authored Pratt declaration.
        pratt_declaration: DeclId,
        /// Authored led-variant declaration.
        variant: DeclId,
        /// Pratt role and binding-power position.
        position: PrattPosition,
    },
    /// An exact-text predicate refines a token-kind fact.
    TextPredicate {
        /// Exact-text predicate refining the token kind.
        predicate: TextPredicateId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// A parser entry contributes end-of-input to FOLLOW.
    EntryEof {
        /// Grammar type receiving the derived fact.
        type_id: TypeId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// A grammar suffix contributes its FIRST set to FOLLOW.
    FollowSuffix {
        /// Grammar type whose fact is being derived.
        owner: TypeId,
        /// Grammar position at which propagation occurs.
        site: SiteId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// FOLLOW propagates from an owner into a nullable target site.
    FollowPropagation {
        /// Grammar type whose fact is being derived.
        owner: TypeId,
        /// Target receiving or supplying the propagated fact.
        target: TypeId,
        /// Grammar position at which propagation occurs.
        site: Option<SiteId>,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
    /// A repetition contributes its continuation back to the repeated item.
    RepeatContinuation {
        /// Grammar type whose fact is being derived.
        owner: TypeId,
        /// Grammar position at which propagation occurs.
        site: SiteId,
        /// Authored declaration supporting this derivation.
        declaration: DeclId,
    },
}

impl ProvenanceStep {
    /// Returns the authored declaration that determines source-order ranking.
    pub const fn declaration(&self) -> DeclId {
        match self {
            Self::DirectDeclaration { declaration, .. }
            | Self::CategoryMembership { declaration, .. }
            | Self::ClassMembership { declaration, .. }
            | Self::TargetAdmission { declaration, .. }
            | Self::Reference { declaration, .. }
            | Self::FirstPropagation { declaration, .. }
            | Self::NullablePrefix { declaration, .. }
            | Self::PrattExtender {
                variant: declaration,
                ..
            }
            | Self::TextPredicate { declaration, .. }
            | Self::EntryEof { declaration, .. }
            | Self::FollowSuffix { declaration, .. }
            | Self::FollowPropagation { declaration, .. }
            | Self::RepeatContinuation { declaration, .. } => *declaration,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Canonical preference rank used when multiple proofs establish one fact.
pub struct ProofRank {
    /// Number of derivation steps from a direct fact.
    pub hops: u32,
    /// Source order of the determining declaration.
    pub source_order: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Interned node in a semantic derivation chain.
pub struct ProofNode {
    /// Earlier derivation step, absent for a direct fact.
    pub parent: Option<ProofId>,
    /// Typed derivation represented by this node.
    pub step: ProvenanceStep,
    /// Canonical preference rank for witness selection.
    pub rank: ProofRank,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Interned arena of canonical semantic proof chains.
pub struct ProofArena {
    /// Proof nodes indexed by [`ProofId`].
    nodes: Vec<ProofNode>,
    /// Existing identities keyed by parent and typed derivation step.
    interned: BTreeMap<(Option<ProofId>, ProvenanceStep), ProofId>,
    /// Stable source order indexed by authored declaration identity.
    source_order_by_declaration: Vec<u32>,
}

/// Old-to-new identities produced while compacting reachable proofs.
pub(crate) struct ProofRemap {
    /// Optional compacted identity indexed by the old proof identity.
    by_old_id: Vec<Option<ProofId>>,
}

impl ProofRemap {
    /// Returns the compacted identity of a proof known to be reachable.
    pub(crate) fn get(&self, old: ProofId) -> ProofId {
        self.by_old_id
            .get(old.index())
            .copied()
            .flatten()
            .expect("returned proof is reachable and remapped")
    }
}

impl ProofArena {
    /// Creates an empty arena using the supplied declaration source order.
    pub(crate) fn with_source_order(source_order_by_declaration: Vec<u32>) -> Self {
        Self {
            nodes: Vec::new(),
            interned: BTreeMap::new(),
            source_order_by_declaration,
        }
    }

    /// Returns interned nodes in identity order.
    pub fn nodes(&self) -> &[ProofNode] {
        &self.nodes
    }

    /// Resolves a proof identity when it belongs to this arena.
    pub fn get(&self, proof: ProofId) -> Option<&ProofNode> {
        self.nodes.get(proof.index())
    }

    /// Iterates a proof from its selected witness back to its direct origin.
    pub fn chain(&self, proof: ProofId) -> ProofChain<'_> {
        ProofChain {
            arena: self,
            next: Some(proof),
        }
    }

    /// Interns one derivation step and assigns its canonical witness rank.
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

    /// Compares proofs by rank, derivation step, then parent chain.
    pub(crate) fn compare(&self, left: ProofId, right: ProofId) -> std::cmp::Ordering {
        #[cfg(test)]
        PROOF_COMPARISONS.set(PROOF_COMPARISONS.get() + 1);
        let left_node = self.get(left).expect("valid proof ID");
        let right_node = self.get(right).expect("valid proof ID");
        left_node
            .rank
            .cmp(&right_node.rank)
            .then_with(|| left_node.step.cmp(&right_node.step))
            .then_with(|| self.compare_parents(left_node.parent, right_node.parent))
    }

    /// Compacts the arena to proofs reachable from `roots`, preserving canonical rank.
    pub(crate) fn retain_reachable(
        &mut self,
        roots: impl IntoIterator<Item = ProofId>,
    ) -> ProofRemap {
        let mut reachable = vec![false; self.nodes.len()];
        let mut pending = roots.into_iter().collect::<Vec<_>>();
        while let Some(proof) = pending.pop() {
            let node = self.get(proof).expect("returned proof ID is valid");
            if std::mem::replace(&mut reachable[proof.index()], true) {
                continue;
            }
            if let Some(parent) = node.parent {
                pending.push(parent);
            }
        }

        let max_hops = self
            .nodes
            .iter()
            .zip(&reachable)
            .filter_map(|(node, reachable)| reachable.then_some(node.rank.hops))
            .max()
            .unwrap_or(0);
        let mut retained_by_hops = vec![Vec::new(); max_hops as usize + 1];
        for (index, retained) in reachable.iter().copied().enumerate() {
            if retained {
                let proof = ProofId::new(index.try_into().expect("proof ID overflow"));
                let hops = self
                    .get(proof)
                    .expect("reachable proof ID is valid")
                    .rank
                    .hops;
                retained_by_hops[hops as usize].push(proof);
            }
        }

        let mut remap = ProofRemap {
            by_old_id: vec![None; self.nodes.len()],
        };
        let mut compact = Self::with_source_order(self.source_order_by_declaration.clone());
        for mut retained in retained_by_hops {
            retained.sort_unstable_by(|left, right| {
                let left_node = self.get(*left).expect("reachable proof ID is valid");
                let right_node = self.get(*right).expect("reachable proof ID is valid");
                left_node
                    .rank
                    .source_order
                    .cmp(&right_node.rank.source_order)
                    .then_with(|| left_node.step.cmp(&right_node.step))
                    .then_with(|| {
                        left_node
                            .parent
                            .map(|parent| remap.get(parent))
                            .cmp(&right_node.parent.map(|parent| remap.get(parent)))
                    })
            });
            for old in retained {
                let old_node = self.get(old).expect("reachable proof ID is valid");
                let parent = old_node.parent.map(|parent| remap.get(parent));
                let new = compact.intern(parent, old_node.step.clone());
                assert_eq!(
                    compact.get(new).expect("compacted proof exists").rank,
                    old_node.rank,
                    "proof compaction preserves canonical rank",
                );
                remap.by_old_id[old.index()] = Some(new);
            }
        }
        *self = compact;
        remap
    }

    /// Orders optional parent chains, placing a direct origin first.
    fn compare_parents(&self, left: Option<ProofId>, right: Option<ProofId>) -> std::cmp::Ordering {
        match (left, right) {
            (Some(left), Some(right)) => self.compare(left, right),
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates one declaration proof step with compact, distinct identities.
    fn direct_step(identity: u32) -> ProvenanceStep {
        ProvenanceStep::DirectDeclaration {
            atom: AtomId::new(identity),
            kind: KindId::new(identity.try_into().expect("fixture kind ID fits")),
            declaration: DeclId::new(identity),
        }
    }

    #[test]
    fn reachable_compaction_preserves_recursive_canonical_order_without_recursive_comparisons() {
        let mut arena = ProofArena::with_source_order(vec![3, 2, 1, 0]);
        let roots = (0..4)
            .rev()
            .map(|identity| {
                let direct = arena.intern(None, direct_step(identity));
                let middle = arena.intern(
                    Some(direct),
                    ProvenanceStep::FirstPropagation {
                        type_id: TypeId::new(identity),
                        declaration: DeclId::new(identity),
                    },
                );
                arena.intern(
                    Some(middle),
                    ProvenanceStep::TargetAdmission {
                        target: AtomId::new(identity),
                        declaration: DeclId::new(identity),
                    },
                )
            })
            .collect::<Vec<_>>();

        let mut expected = (0..arena.nodes().len())
            .map(|index| ProofId::new(index.try_into().expect("fixture proof ID fits")))
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| arena.compare(*left, *right));
        let expected_steps = expected
            .into_iter()
            .map(|proof| arena.get(proof).expect("fixture proof exists").step.clone())
            .collect::<Vec<_>>();

        reset_proof_comparisons_for_test();
        arena.retain_reachable(roots);

        assert_eq!(
            arena
                .nodes()
                .iter()
                .map(|node| node.step.clone())
                .collect::<Vec<_>>(),
            expected_steps,
            "iterative compaction must preserve the recursive canonical order",
        );
        assert_eq!(
            proof_comparisons_for_test(),
            0,
            "compaction must not recursively compare already-ranked parent chains",
        );
    }
}

/// Iterator from a selected proof node toward its direct origin.
pub struct ProofChain<'a> {
    /// Arena that owns every yielded node.
    arena: &'a ProofArena,
    /// Next proof identity toward the origin.
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
/// Symbolic token-kind set paired with one canonical proof per member.
pub struct ProvenancedKindSet {
    /// Proven token-kind membership.
    kinds: KindSet,
    /// Canonical proof indexed by grammar-local token kind.
    proof_by_kind: Vec<Option<ProofId>>,
}

impl ProvenancedKindSet {
    /// Creates an empty provenanced set for one grammar width.
    pub(crate) fn empty(kind_count: usize) -> Self {
        Self {
            kinds: KindSet::empty(kind_count),
            proof_by_kind: vec![None; kind_count],
        }
    }

    /// Returns the proven symbolic membership.
    pub fn kinds(&self) -> &KindSet {
        &self.kinds
    }

    /// Returns the selected canonical proof for `kind`.
    pub fn proof(&self, kind: KindId) -> Option<ProofId> {
        self.proof_by_kind.get(kind.index()).copied().flatten()
    }

    /// Iterates the proof roots needed to retain this set's evidence.
    pub(crate) fn proof_roots(&self) -> impl Iterator<Item = ProofId> + '_ {
        self.proof_by_kind.iter().filter_map(|proof| *proof)
    }

    /// Applies an arena-compaction identity remap to every proof.
    pub(crate) fn remap_proofs(&mut self, remap: &ProofRemap) {
        for proof in self.proof_by_kind.iter_mut().flatten() {
            *proof = remap.get(*proof);
        }
    }

    /// Inserts membership and retains the canonically preferred proof.
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

    /// Merges another equally wide set using canonical proof preference.
    pub(crate) fn merge(&mut self, other: &Self, arena: &ProofArena) {
        debug_assert_eq!(self.kinds.kind_count(), other.kinds.kind_count());
        for kind in other.kinds.iter() {
            self.insert(kind, other.proof(kind).expect("bit has proof"), arena);
        }
    }
}
