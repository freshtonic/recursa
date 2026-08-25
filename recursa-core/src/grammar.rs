use std::borrow::Cow;

/// Erased identity of a token kind in one grammar.
pub type TokenKind = u16;

/// Erased identity of a symbolic expectation in one grammar.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExpectedAtomId(pub u32);

/// Static names and distinguished kinds for one generated grammar.
#[derive(Debug, Eq, PartialEq)]
pub struct GrammarInfo {
    pub name: &'static str,
    pub kind_names: &'static [&'static str],
    pub atom_names: &'static [&'static str],
    pub lexical_error_kind: TokenKind,
}

/// Dynamic caller FOLLOW supplied to every generated parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FollowSet {
    pub(crate) kind_count: u32,
    pub(crate) kinds: Cow<'static, [u64]>,
    pub(crate) expected: Cow<'static, [ExpectedAtomId]>,
}

impl FollowSet {
    pub(crate) const fn from_static(
        kind_count: u32,
        kinds: &'static [u64],
        expected: &'static [ExpectedAtomId],
    ) -> Self {
        Self {
            kind_count,
            kinds: Cow::Borrowed(kinds),
            expected: Cow::Borrowed(expected),
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
}
