use std::borrow::Cow;

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

/// Immutable token-kind and symbolic-expectation set used by generated LR entry points.
///
/// Generated roots use kind membership to delimit prefix acceptance. Lookahead correction
/// constructs owned sets on the error path so [`crate::ParseError`] can expose stable symbolic
/// expectations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FollowSet {
    /// Exact token-kind width used to validate generated bitsets.
    kind_count: u32,
    /// Dense bitset of accepted grammar-local token kinds.
    kinds: Cow<'static, [u64]>,
    /// Sorted symbolic expectations used in diagnostics.
    expected: Cow<'static, [ExpectedAtomId]>,
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
        }
    }

    /// Builds the owned expected set computed by LR lookahead correction.
    pub(crate) fn owned(kind_count: u32, kinds: Vec<u64>, expected: Vec<ExpectedAtomId>) -> Self {
        debug_assert_eq!(kinds.len(), (kind_count as usize).div_ceil(64));
        debug_assert!(expected.windows(2).all(|pair| pair[0] < pair[1]));
        Self {
            kind_count,
            kinds: Cow::Owned(kinds),
            expected: Cow::Owned(expected),
        }
    }

    /// Reports whether this set contains `kind`.
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

    /// Returns the symbolic expectations carried for diagnostics.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_sets_reject_invalid_width_tails_and_atoms() {
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
}
