use std::borrow::Cow;
use std::fmt;

/// Erased identity of a token kind in one grammar.
pub type TokenKind = u16;

/// Erased identity of a symbolic expectation in one grammar.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExpectedAtomId(pub u32);

/// Static names and distinguished kinds for one generated grammar.
#[derive(Debug, Eq, PartialEq)]
pub struct GrammarInfo {
    /// User-configured grammar name.
    pub(crate) name: &'static str,
    /// Display name indexed by each grammar-local token kind.
    pub(crate) kind_names: &'static [&'static str],
    /// Display name indexed by each symbolic expectation.
    pub(crate) atom_names: &'static [&'static str],
    /// Token kind reserved for lexer failures.
    pub(crate) lexical_error_kind: TokenKind,
}

impl GrammarInfo {
    /// The configured grammar name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the exact grammar-local token-kind cardinality.
    pub(crate) fn kind_count(&self) -> usize {
        self.kind_names.len()
    }

    /// Resolves a grammar-local token kind to its diagnostic name.
    pub(crate) fn kind_name(&self, kind: TokenKind) -> Option<&'static str> {
        self.kind_names.get(usize::from(kind)).copied()
    }

    /// Resolves a symbolic expectation to its diagnostic name.
    pub(crate) fn atom_name(&self, atom: ExpectedAtomId) -> Option<&'static str> {
        usize::try_from(atom.0)
            .ok()
            .and_then(|index| self.atom_names.get(index))
            .copied()
    }
}

/// Recursive FOLLOW state transported only by matching generated parsers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FollowSet {
    /// Exact token-kind width used to validate compatible unions.
    pub(crate) kind_count: u32,
    /// Dense bitset of accepted grammar-local token kinds.
    pub(crate) kinds: Cow<'static, [u64]>,
    /// Sorted symbolic expectations used in diagnostics.
    pub(crate) expected: Cow<'static, [ExpectedAtomId]>,
    /// Nearest static suffix boundary used by Pratt ambiguity checks.
    pub(crate) pratt_boundary_kinds: Cow<'static, [u64]>,
}

impl FollowSet {
    /// Constructs a validated borrowed FOLLOW constant emitted by codegen.
    pub(crate) const fn from_static(
        kind_count: u32,
        kinds: &'static [u64],
        expected: &'static [ExpectedAtomId],
    ) -> Self {
        assert!(kind_count <= u16::MAX as u32 + 1);
        let word_count = (kind_count as usize).div_ceil(64);
        assert!(kinds.len() == word_count);

        let tail_bits = kind_count % 64;
        if tail_bits != 0 {
            let tail_mask = u64::MAX >> (64 - tail_bits);
            assert!(kinds[word_count - 1] & !tail_mask == 0);
        }

        let mut atom_index = 1;
        while atom_index < expected.len() {
            assert!(expected[atom_index - 1].0 < expected[atom_index].0);
            atom_index += 1;
        }

        Self {
            kind_count,
            kinds: Cow::Borrowed(kinds),
            expected: Cow::Borrowed(expected),
            pratt_boundary_kinds: Cow::Borrowed(kinds),
        }
    }

    /// Whether this set contains `kind`.
    pub fn contains(&self, kind: TokenKind) -> bool {
        if u32::from(kind) >= self.kind_count {
            return false;
        }
        let word = usize::from(kind) / 64;
        let bit = usize::from(kind) % 64;
        self.kinds
            .get(word)
            .is_some_and(|value| value & (1_u64 << bit) != 0)
    }

    /// Symbolic expectations carried for diagnostics.
    pub fn expected(&self) -> &[ExpectedAtomId] {
        &self.expected
    }

    /// Reports whether this set is exactly one kind at the supplied width.
    pub(crate) fn is_exact_kind(&self, kind_count: usize, kind: TokenKind) -> bool {
        self.kind_count as usize == kind_count
            && usize::from(kind) < kind_count
            && self.expected.is_empty()
            && self.kinds.iter().map(|word| word.count_ones()).sum::<u32>() == 1
            && self.contains(kind)
    }

    /// Combines two sets of the same grammar width into owned storage.
    pub fn union(&self, other: &Self) -> Result<Self, FollowSetError> {
        if self.kind_count != other.kind_count {
            return Err(FollowSetError::WidthMismatch {
                left: self.kind_count,
                right: other.kind_count,
            });
        }

        let kinds = self
            .kinds
            .iter()
            .zip(other.kinds.iter())
            .map(|(left, right)| left | right)
            .collect();
        let pratt_boundary_kinds = self
            .pratt_boundary_kinds
            .iter()
            .zip(other.pratt_boundary_kinds.iter())
            .map(|(left, right)| left | right)
            .collect();

        let mut expected = Vec::with_capacity(self.expected.len() + other.expected.len());
        let mut left = self.expected.iter().copied().peekable();
        let mut right = other.expected.iter().copied().peekable();
        while let (Some(left_atom), Some(right_atom)) = (left.peek(), right.peek()) {
            match left_atom.cmp(right_atom) {
                std::cmp::Ordering::Less => expected.push(left.next().unwrap()),
                std::cmp::Ordering::Greater => expected.push(right.next().unwrap()),
                std::cmp::Ordering::Equal => {
                    expected.push(left.next().unwrap());
                    right.next();
                }
            }
        }
        expected.extend(left);
        expected.extend(right);

        Ok(Self {
            kind_count: self.kind_count,
            kinds: Cow::Owned(kinds),
            expected: Cow::Owned(expected),
            pratt_boundary_kinds: Cow::Owned(pratt_boundary_kinds),
        })
    }

    /// Composes a nearer static suffix with an inherited caller FOLLOW.
    ///
    /// Ordinary parsing observes the complete union. Pratt ambiguity checks
    /// retain the nearer static suffix so a farther nullable suffix cannot
    /// fabricate an immediate extender collision.
    pub fn with_caller(&self, caller: &Self) -> Result<Self, FollowSetError> {
        let mut composed = self.union(caller)?;
        composed.pratt_boundary_kinds = self.kinds.clone();
        Ok(composed)
    }

    /// Reports whether `kind` belongs to the nearest static Pratt boundary.
    pub(crate) fn pratt_boundary_contains(&self, kind: TokenKind) -> bool {
        if u32::from(kind) >= self.kind_count {
            return false;
        }
        let word = usize::from(kind) / 64;
        let bit = usize::from(kind) % 64;
        self.pratt_boundary_kinds
            .get(word)
            .is_some_and(|value| value & (1_u64 << bit) != 0)
    }
}

/// A runtime FOLLOW operation combined sets from different grammar widths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FollowSetError {
    /// The sets describe different grammar-local token-kind widths.
    WidthMismatch {
        /// Width of the left operand.
        left: u32,
        /// Width of the right operand.
        right: u32,
    },
}

impl fmt::Display for FollowSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WidthMismatch { left, right } => {
                write!(formatter, "FOLLOW widths differ: {left} and {right}")
            }
        }
    }
}

impl std::error::Error for FollowSetError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_follow_sets_reject_invalid_width_tails_and_atoms() {
        assert!(std::panic::catch_unwind(|| FollowSet::from_static(65, &[0], &[])).is_err());
        assert!(std::panic::catch_unwind(|| FollowSet::from_static(65, &[0, 0b10], &[])).is_err());
        assert!(
            std::panic::catch_unwind(|| FollowSet::from_static(
                1,
                &[0],
                &[ExpectedAtomId(2), ExpectedAtomId(2)],
            ))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| FollowSet::from_static(
                1,
                &[0],
                &[ExpectedAtomId(2), ExpectedAtomId(1)],
            ))
            .is_err()
        );
    }

    #[test]
    fn borrowed_constants_union_into_owned_deterministic_storage() {
        let left = FollowSet::from_static(3, &[0b001], &[ExpectedAtomId(0), ExpectedAtomId(2)]);
        let right = FollowSet::from_static(3, &[0b100], &[ExpectedAtomId(1), ExpectedAtomId(2)]);

        assert!(matches!(left.kinds, Cow::Borrowed(_)));
        assert!(matches!(left.expected, Cow::Borrowed(_)));
        assert!(matches!(left.pratt_boundary_kinds, Cow::Borrowed(_)));

        let union = left.union(&right).unwrap();

        assert!(matches!(union.kinds, Cow::Owned(_)));
        assert!(matches!(union.expected, Cow::Owned(_)));
        assert!(matches!(union.pratt_boundary_kinds, Cow::Owned(_)));
        assert_eq!(&*union.kinds, &[0b101]);
        assert_eq!(
            &*union.expected,
            &[ExpectedAtomId(0), ExpectedAtomId(1), ExpectedAtomId(2)]
        );
        assert_eq!(right.union(&left).unwrap(), union);
    }

    #[test]
    fn caller_composition_retains_the_nearest_static_pratt_boundary() {
        let nearer = FollowSet::from_static(3, &[0b001], &[ExpectedAtomId(0)]);
        let farther = FollowSet::from_static(3, &[0b100], &[ExpectedAtomId(2)]);

        let composed = nearer.with_caller(&farther).unwrap();

        assert_eq!(&*composed.kinds, &[0b101]);
        assert!(composed.pratt_boundary_contains(0));
        assert!(!composed.pratt_boundary_contains(2));
    }

    #[test]
    fn union_rejects_different_exact_widths() {
        let left = FollowSet::from_static(65, &[0, 0], &[]);
        let right = FollowSet::from_static(127, &[0, 0], &[]);

        assert_eq!(
            left.union(&right),
            Err(FollowSetError::WidthMismatch {
                left: 65,
                right: 127,
            })
        );
    }
}
