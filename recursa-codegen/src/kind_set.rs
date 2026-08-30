//! Width-checked symbolic sets of grammar-local token kinds.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

use std::fmt;

use crate::KindId;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Symbolic token-kind set carrying the grammar width that defines its domain.
pub struct KindSet {
    /// Number of token kinds in the owning grammar.
    kind_count: usize,
    /// Packed membership words with unused tail bits cleared.
    words: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Width or membership violation while combining token-kind sets.
pub enum KindSetError {
    /// A kind is outside the owning grammar's domain.
    KindOutOfRange {
        /// Rejected kind identity.
        kind: KindId,
        /// Owning grammar width.
        kind_count: usize,
    },
    /// An operation combined sets belonging to different grammar widths.
    WidthMismatch {
        /// Left-hand grammar width.
        left: usize,
        /// Right-hand grammar width.
        right: usize,
    },
}

impl fmt::Display for KindSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindOutOfRange { kind, kind_count } => {
                write!(
                    formatter,
                    "kind {} is outside grammar width {kind_count}",
                    kind.get()
                )
            }
            Self::WidthMismatch { left, right } => {
                write!(formatter, "kind-set widths differ: {left} and {right}")
            }
        }
    }
}

impl std::error::Error for KindSetError {}

impl KindSet {
    /// Creates an empty set for a grammar with `kind_count` token kinds.
    pub fn empty(kind_count: usize) -> Self {
        Self {
            kind_count,
            words: vec![0; kind_count.div_ceil(64)],
        }
    }

    /// Creates a set containing one validated token kind.
    pub fn singleton(kind_count: usize, kind: KindId) -> Result<Self, KindSetError> {
        let mut set = Self::empty(kind_count);
        set.insert(kind)?;
        Ok(set)
    }

    /// Creates a set from validated token kinds in any input order.
    pub fn from_kinds(
        kind_count: usize,
        kinds: impl IntoIterator<Item = KindId>,
    ) -> Result<Self, KindSetError> {
        let mut set = Self::empty(kind_count);
        for kind in kinds {
            set.insert(kind)?;
        }
        Ok(set)
    }

    /// Returns the owning grammar's token-kind width.
    pub const fn kind_count(&self) -> usize {
        self.kind_count
    }

    /// Returns the canonical packed membership representation.
    pub fn words(&self) -> &[u64] {
        &self.words
    }

    /// Reports whether `kind` belongs to this set and grammar width.
    pub fn contains(&self, kind: KindId) -> bool {
        let index = kind.index();
        index < self.kind_count && self.words[index / 64] & (1 << (index % 64)) != 0
    }

    /// Reports whether the set contains no token kinds.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    /// Returns the union of two sets with the same grammar width.
    pub fn union(&self, other: &Self) -> Result<Self, KindSetError> {
        self.combine(other, |left, right| left | right)
    }

    /// Returns the intersection of two sets with the same grammar width.
    pub fn intersection(&self, other: &Self) -> Result<Self, KindSetError> {
        self.combine(other, |left, right| left & right)
    }

    /// Reports whether two equally wide sets share at least one token kind.
    pub fn intersects(&self, other: &Self) -> Result<bool, KindSetError> {
        if self.kind_count != other.kind_count {
            return Err(KindSetError::WidthMismatch {
                left: self.kind_count,
                right: other.kind_count,
            });
        }
        Ok(self
            .words
            .iter()
            .zip(&other.words)
            .any(|(left, right)| left & right != 0))
    }

    /// Returns kinds present here but absent from a set with the same width.
    pub fn difference(&self, other: &Self) -> Result<Self, KindSetError> {
        self.combine(other, |left, right| left & !right)
    }

    /// Iterates member kinds in ascending numeric order.
    pub fn iter(&self) -> KindSetIter<'_> {
        KindSetIter {
            set: self,
            word: 0,
            remaining: self.words.first().copied().unwrap_or_default(),
        }
    }

    /// Inserts a kind after validating it against the grammar width.
    pub(crate) fn insert(&mut self, kind: KindId) -> Result<(), KindSetError> {
        let index = kind.index();
        if index >= self.kind_count {
            return Err(KindSetError::KindOutOfRange {
                kind,
                kind_count: self.kind_count,
            });
        }
        self.words[index / 64] |= 1 << (index % 64);
        Ok(())
    }

    /// Adds every kind in an equally wide set.
    pub(crate) fn union_assign(&mut self, other: &Self) {
        debug_assert_eq!(self.kind_count, other.kind_count);
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            *left |= right;
        }
        self.clear_tail();
    }

    /// Applies a packed binary set operation after validating equal widths.
    fn combine(
        &self,
        other: &Self,
        operation: impl Fn(u64, u64) -> u64,
    ) -> Result<Self, KindSetError> {
        if self.kind_count != other.kind_count {
            return Err(KindSetError::WidthMismatch {
                left: self.kind_count,
                right: other.kind_count,
            });
        }
        let mut combined = Self {
            kind_count: self.kind_count,
            words: self
                .words
                .iter()
                .zip(&other.words)
                .map(|(left, right)| operation(*left, *right))
                .collect(),
        };
        combined.clear_tail();
        Ok(combined)
    }

    /// Clears unused bits in the final packed word to preserve canonical equality.
    fn clear_tail(&mut self) {
        let used = self.kind_count % 64;
        if used == 0 {
            return;
        }
        if let Some(last) = self.words.last_mut() {
            *last &= (1_u64 << used) - 1;
        }
    }
}

/// Ascending iterator over the members of a [`KindSet`].
pub struct KindSetIter<'a> {
    /// Set whose membership is being traversed.
    set: &'a KindSet,
    /// Packed word currently being traversed.
    word: usize,
    /// Unvisited members of the current packed word.
    remaining: u64,
}

impl Iterator for KindSetIter<'_> {
    type Item = KindId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.remaining != 0 {
                let bit = self.remaining.trailing_zeros() as usize;
                self.remaining &= self.remaining - 1;
                let index = self.word * u64::BITS as usize + bit;
                debug_assert!(index < self.set.kind_count);
                return Some(KindId::new(index as u16));
            }
            self.word += 1;
            self.remaining = *self.set.words.get(self.word)?;
        }
    }
}
