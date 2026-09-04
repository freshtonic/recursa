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
    /// Unions two sets into one interned `'static` set.
    ///
    /// Composed sets are interned by content in a thread-local table: the first
    /// composition that produces a given content allocates once and leaks it,
    /// every later composition with the same content returns the same reference
    /// without allocating. Parsers compose FOLLOW sets at every use site, so the
    /// steady state of a parse allocates no FOLLOW storage at all.
    pub fn union(&self, other: &Self) -> Result<&'static Self, FollowSetError> {
        self.compose(other, BoundaryMode::Union)
    }

    /// Unions with the caller FOLLOW while keeping only this set as the Pratt boundary.
    pub fn with_caller(&self, caller: &Self) -> Result<&'static Self, FollowSetError> {
        self.compose(caller, BoundaryMode::KeepSelf)
    }

    /// Composes two sets, memoized first by operand identity, then by content.
    ///
    /// The fast path hashes only the two operand addresses and the mode. Every
    /// operand a parser composes is either a generated static or a set this
    /// table returned earlier, so addresses are stable and the pair lookup
    /// needs no word-level work. A pair miss builds the content in reusable
    /// scratch buffers and canonicalizes it through the content table, so the
    /// number of leaked sets is bounded by the number of distinct contents.
    fn compose(&self, other: &Self, mode: BoundaryMode) -> Result<&'static Self, FollowSetError> {
        if self.kind_count != other.kind_count {
            return Err(FollowSetError::WidthMismatch {
                left: self.kind_count,
                right: other.kind_count,
            });
        }
        let pair = (
            self as *const Self as usize,
            other as *const Self as usize,
            mode as u8,
        );
        INTERNER.with(|interner| {
            let mut interner = interner.borrow_mut();
            if let Some(found) = interner.pairs.get(&pair) {
                return Ok(*found);
            }
            let Interner {
                scratch,
                contents,
                pairs,
            } = &mut *interner;
            scratch.kinds.clear();
            scratch.kinds.extend(
                self.kinds
                    .iter()
                    .zip(other.kinds.iter())
                    .map(|(l, r)| l | r),
            );
            scratch.boundary.clear();
            match mode {
                BoundaryMode::Union => scratch.boundary.extend(
                    self.pratt_boundary_kinds
                        .iter()
                        .zip(other.pratt_boundary_kinds.iter())
                        .map(|(l, r)| l | r),
                ),
                BoundaryMode::KeepSelf => scratch.boundary.extend_from_slice(&self.kinds),
            }
            scratch.expected.clear();
            merge_sorted(&self.expected, &other.expected, &mut scratch.expected);
            let hash = content_hash(
                self.kind_count,
                &scratch.kinds,
                &scratch.boundary,
                &scratch.expected,
            );
            let bucket = contents.entry(hash).or_default();
            let canonical = match bucket.iter().copied().find(|set| {
                set.kind_count == self.kind_count
                    && *set.kinds == *scratch.kinds
                    && *set.pratt_boundary_kinds == *scratch.boundary
                    && *set.expected == *scratch.expected
            }) {
                Some(found) => found,
                None => {
                    let leaked: &'static Self = Box::leak(Box::new(Self {
                        kind_count: self.kind_count,
                        kinds: Cow::Owned(scratch.kinds.clone()),
                        expected: Cow::Owned(scratch.expected.clone()),
                        pratt_boundary_kinds: Cow::Owned(scratch.boundary.clone()),
                    }));
                    bucket.push(leaked);
                    leaked
                }
            };
            pairs.insert(pair, canonical);
            Ok(canonical)
        })
    }

    /// Returns how many distinct composed sets this thread has interned.
    #[doc(hidden)]
    pub fn interned_composition_count() -> usize {
        INTERNER.with(|interner| interner.borrow().contents.values().map(Vec::len).sum())
    }

    /// Returns how many operand pairs this thread has memoized.
    #[doc(hidden)]
    pub fn memoized_pair_count() -> usize {
        INTERNER.with(|interner| interner.borrow().pairs.len())
    }

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

/// Which Pratt boundary a composition keeps.
#[derive(Clone, Copy)]
enum BoundaryMode {
    /// The union of both boundaries (`union`).
    Union,
    /// Only the nearer set's kinds (`with_caller`).
    KeepSelf,
}

/// Reusable composition buffers and the two memo tables of leaked sets.
#[derive(Default)]
struct Interner {
    /// Buffers the composition is built in before the content lookup.
    scratch: Scratch,
    /// Canonical interned sets grouped by content hash.
    contents: std::collections::HashMap<u64, Vec<&'static FollowSet>, IdentityHashBuilder>,
    /// Operand-identity memo: (left address, right address, mode) to the canonical set.
    pairs: std::collections::HashMap<(usize, usize, u8), &'static FollowSet, PairHashBuilder>,
}

/// Composition buffers reused across calls so a table hit allocates nothing.
#[derive(Default)]
struct Scratch {
    /// Unioned kind words.
    kinds: Vec<u64>,
    /// Pratt boundary words for the chosen mode.
    boundary: Vec<u64>,
    /// Merged sorted atom refinements.
    expected: Vec<ExpectedAtomId>,
}

thread_local! {
    static INTERNER: std::cell::RefCell<Interner> = std::cell::RefCell::new(Interner::default());
}

/// Merges two sorted, deduplicated atom lists into `out`.
fn merge_sorted(left: &[ExpectedAtomId], right: &[ExpectedAtomId], out: &mut Vec<ExpectedAtomId>) {
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        match left[i].cmp(&right[j]) {
            std::cmp::Ordering::Less => {
                out.push(left[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(right[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(left[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
}

/// Hashes composed content with a multiply-rotate mix (no per-call allocation).
fn content_hash(
    kind_count: u32,
    kinds: &[u64],
    boundary: &[u64],
    expected: &[ExpectedAtomId],
) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut hash = u64::from(kind_count).wrapping_mul(K);
    for word in kinds.iter().chain(boundary.iter()) {
        hash = (hash.rotate_left(5) ^ word).wrapping_mul(K);
    }
    for atom in expected {
        hash = (hash.rotate_left(5) ^ u64::from(atom.0)).wrapping_mul(K);
    }
    hash
}

/// Mixes an operand-pair key with one multiply per word.
#[derive(Clone, Copy, Default)]
struct PairHashBuilder;

impl std::hash::BuildHasher for PairHashBuilder {
    type Hasher = PairHasher;
    fn build_hasher(&self) -> PairHasher {
        PairHasher(0)
    }
}

/// Hasher for `(usize, usize, u8)` keys: rotate, xor, multiply per written word.
#[derive(Default)]
struct PairHasher(u64);

impl std::hash::Hasher for PairHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }
    fn write_u8(&mut self, value: u8) {
        self.write_u64(u64::from(value));
    }
    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

/// Passes an already-mixed `u64` key through unchanged.
#[derive(Clone, Copy, Default)]
struct IdentityHashBuilder;

impl std::hash::BuildHasher for IdentityHashBuilder {
    type Hasher = IdentityHasher;
    fn build_hasher(&self) -> IdentityHasher {
        IdentityHasher(0)
    }
}

/// Hasher that returns the single `u64` it was fed.
#[derive(Default)]
struct IdentityHasher(u64);

impl std::hash::Hasher for IdentityHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0.rotate_left(8)) ^ u64::from(*byte);
        }
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

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
