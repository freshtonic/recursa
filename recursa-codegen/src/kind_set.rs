use std::fmt;

use crate::KindId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KindSet {
    kind_count: usize,
    words: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KindSetError {
    KindOutOfRange { kind: KindId, kind_count: usize },
    WidthMismatch { left: usize, right: usize },
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
    pub fn empty(kind_count: usize) -> Self {
        Self {
            kind_count,
            words: vec![0; kind_count.div_ceil(64)],
        }
    }

    pub fn singleton(kind_count: usize, kind: KindId) -> Result<Self, KindSetError> {
        let mut set = Self::empty(kind_count);
        set.insert(kind)?;
        Ok(set)
    }

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

    pub const fn kind_count(&self) -> usize {
        self.kind_count
    }

    pub fn words(&self) -> &[u64] {
        &self.words
    }

    pub fn contains(&self, kind: KindId) -> bool {
        let index = kind.index();
        index < self.kind_count && self.words[index / 64] & (1 << (index % 64)) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    pub fn union(&self, other: &Self) -> Result<Self, KindSetError> {
        self.combine(other, |left, right| left | right)
    }

    pub fn intersection(&self, other: &Self) -> Result<Self, KindSetError> {
        self.combine(other, |left, right| left & right)
    }

    pub fn difference(&self, other: &Self) -> Result<Self, KindSetError> {
        self.combine(other, |left, right| left & !right)
    }

    pub fn iter(&self) -> KindSetIter<'_> {
        KindSetIter { set: self, next: 0 }
    }

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

    pub(crate) fn union_assign(&mut self, other: &Self) {
        debug_assert_eq!(self.kind_count, other.kind_count);
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            *left |= right;
        }
        self.clear_tail();
    }

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

pub struct KindSetIter<'a> {
    set: &'a KindSet,
    next: usize,
}

impl Iterator for KindSetIter<'_> {
    type Item = KindId;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next < self.set.kind_count {
            let index = self.next;
            self.next += 1;
            let kind = KindId::new(index as u16);
            if self.set.contains(kind) {
                return Some(kind);
            }
        }
        None
    }
}
