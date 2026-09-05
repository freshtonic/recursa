use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{LazyLock, Mutex};

/// Proves at compile time that interned sets may be shared between threads.
const fn assert_shared_across_threads<T: Send + Sync>() {}
const _: () = assert_shared_across_threads::<FollowSet>();

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
    /// Boundary kinds a `#[greedy(...)]` acceptance lets a Pratt loop extend on.
    ///
    /// Empty for every set that no acceptance produced, including every generated
    /// static, so the slice may be shorter than the boundary it refines.
    pub(crate) greedy_kinds: Cow<'static, [u64]>,
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
            greedy_kinds: Cow::Borrowed(&[]),
        }
    }

    /// Builds an owned set of accepted kinds with no symbolic expectations, the crude expected
    /// set the table-driven loop reports until lookahead correction computes a symbolic one.
    pub(crate) fn owned(kind_count: u32, kinds: Vec<u64>) -> Self {
        debug_assert_eq!(kinds.len(), (kind_count as usize).div_ceil(64));
        Self {
            kind_count,
            kinds: Cow::Owned(kinds.clone()),
            expected: Cow::Borrowed(&[]),
            pratt_boundary_kinds: Cow::Owned(kinds),
            greedy_kinds: Cow::Borrowed(&[]),
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

    /// Exact token-kind width this set was cut to.
    pub(crate) fn kind_count(&self) -> u32 {
        self.kind_count
    }

    /// Raw bitset words of the accepted kinds, least significant kind first.
    pub(crate) fn kind_words(&self) -> &[u64] {
        &self.kinds
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
        self.compose(other, None, BoundaryMode::Union)
    }

    /// Unions with the caller FOLLOW while keeping only this set as the Pratt boundary.
    pub fn with_caller(&self, caller: &Self) -> Result<&'static Self, FollowSetError> {
        self.compose(caller, None, BoundaryMode::KeepSelf)
    }

    /// Unions two sets and marks the `#[greedy(...)]`-accepted kinds of the result.
    ///
    /// # Errors
    ///
    /// Returns [`FollowSetError::WidthMismatch`] when the operands describe
    /// different grammar widths.
    pub fn union_accepting(
        &self,
        other: &Self,
        accepted: &Self,
    ) -> Result<&'static Self, FollowSetError> {
        self.compose(other, Some(accepted), BoundaryMode::Union)
    }

    /// Composes with the caller and marks the `#[greedy(...)]`-accepted kinds of the result.
    ///
    /// FOLLOW membership, expectations, and the Pratt boundary are what the unmarked
    /// composition would produce: the accepted kinds really can follow the site and really
    /// are a boundary. What the mark changes is what a Pratt loop does when it reaches one
    /// of them with a viable extension — it extends instead of reporting `RCA4102`. An
    /// extension that bounded lookahead already rules out still ends the loop.
    ///
    /// # Errors
    ///
    /// Returns [`FollowSetError::WidthMismatch`] when the operands describe
    /// different grammar widths.
    pub fn with_caller_accepting(
        &self,
        caller: &Self,
        accepted: &Self,
    ) -> Result<&'static Self, FollowSetError> {
        self.compose(caller, Some(accepted), BoundaryMode::KeepSelf)
    }

    /// Composes two sets and returns only the interned result.
    fn compose(
        &self,
        other: &Self,
        accepted: Option<&Self>,
        mode: BoundaryMode,
    ) -> Result<&'static Self, FollowSetError> {
        Ok(self.compose_entry(other, accepted, mode)?.composed)
    }

    /// Composes two sets, memoized first by operand identity, then by content.
    ///
    /// The fast path hashes only the two operand addresses and the mode. Every
    /// operand a parser composes is either a generated static or a set this
    /// table returned earlier, so addresses are stable and the pair lookup
    /// needs no word-level work. A pair miss builds the content in reusable
    /// scratch buffers and canonicalizes it through the content table, so the
    /// number of leaked sets is bounded by the number of distinct contents.
    ///
    /// The returned record is leaked once per distinct `(left, right, acceptance, mode)`
    /// key, so a per-site inline cache can publish one pointer to it and later
    /// recognize the same caller by address without touching this table again.
    /// Callers must keep every operand alive for the whole process; generated
    /// parsers only ever compose generated statics and earlier interned results.
    fn compose_entry(
        &self,
        other: &Self,
        accepted: Option<&Self>,
        mode: BoundaryMode,
    ) -> Result<&'static ComposedFollow, FollowSetError> {
        if self.kind_count != other.kind_count {
            return Err(FollowSetError::WidthMismatch {
                left: self.kind_count,
                right: other.kind_count,
            });
        }
        if let Some(accepted) = accepted
            && self.kind_count != accepted.kind_count
        {
            return Err(FollowSetError::WidthMismatch {
                left: self.kind_count,
                right: accepted.kind_count,
            });
        }
        let pair = (
            std::ptr::from_ref(self) as usize,
            std::ptr::from_ref(other) as usize,
            accepted.map_or(0, |accepted| std::ptr::from_ref(accepted) as usize),
            mode as u8,
        );
        let mut interner = INTERNER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(found) = interner.pairs.get(&pair) {
            return Ok(*found);
        }
        let Interner {
            scratch,
            contents,
            pairs,
        } = &mut *interner;
        scratch.kinds.clear();
        scratch.boundary.clear();
        scratch.expected.clear();
        scratch.greedy.clear();
        scratch.kinds.extend(
            self.kinds
                .iter()
                .zip(other.kinds.iter())
                .map(|(l, r)| l | r),
        );
        match mode {
            BoundaryMode::Union => scratch.boundary.extend(
                self.pratt_boundary_kinds
                    .iter()
                    .zip(other.pratt_boundary_kinds.iter())
                    .map(|(l, r)| l | r),
            ),
            BoundaryMode::KeepSelf => scratch.boundary.extend_from_slice(&self.kinds),
        }
        merge_sorted(&self.expected, &other.expected, &mut scratch.expected);
        // An acceptance covers exactly the site that declared it, so a composition without
        // one leaves the mark empty instead of inheriting an operand's.
        if let Some(accepted) = accepted {
            scratch.greedy.extend_from_slice(&accepted.kinds);
        }
        let hash = content_hash(
            self.kind_count,
            &scratch.kinds,
            &scratch.boundary,
            &scratch.greedy,
            &scratch.expected,
        );
        let bucket = contents.entry(hash).or_default();
        let canonical = match bucket.iter().copied().find(|set| {
            set.kind_count == self.kind_count
                && *set.kinds == *scratch.kinds
                && *set.pratt_boundary_kinds == *scratch.boundary
                && *set.greedy_kinds == *scratch.greedy
                && *set.expected == *scratch.expected
        }) {
            Some(found) => found,
            None => {
                let leaked: &'static Self = Box::leak(Box::new(Self {
                    kind_count: self.kind_count,
                    kinds: Cow::Owned(scratch.kinds.clone()),
                    expected: Cow::Owned(scratch.expected.clone()),
                    pratt_boundary_kinds: Cow::Owned(scratch.boundary.clone()),
                    greedy_kinds: Cow::Owned(scratch.greedy.clone()),
                }));
                bucket.push(leaked);
                leaked
            }
        };
        let entry: &'static ComposedFollow = Box::leak(Box::new(ComposedFollow {
            nearer: std::ptr::from_ref(self),
            caller: std::ptr::from_ref(other),
            composed: canonical,
        }));
        pairs.insert(pair, entry);
        Ok(entry)
    }

    /// Returns how many distinct composed sets this process has interned.
    #[doc(hidden)]
    pub fn interned_composition_count() -> usize {
        let interner = INTERNER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        interner.contents.values().map(Vec::len).sum()
    }

    /// Returns how many operand pairs this process has memoized.
    #[doc(hidden)]
    pub fn memoized_pair_count() -> usize {
        let interner = INTERNER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        interner.pairs.len()
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

    /// Whether a `#[greedy(...)]` acceptance lets a Pratt loop extend past `kind` here.
    pub(crate) fn greedy_contains(&self, kind: TokenKind) -> bool {
        if u32::from(kind) >= self.kind_count {
            return false;
        }
        let word = usize::from(kind) / 64;
        let bit = usize::from(kind) % 64;
        self.greedy_kinds
            .get(word)
            .is_some_and(|value| value & (1_u64 << bit) != 0)
    }
}

/// One interned composition together with the operand identities that produced it.
///
/// The record is allocated once per distinct `(nearer, caller, mode)` triple and never
/// freed, so an inline cache can publish a single pointer to it and a concurrent reader
/// can never observe a half-written operand/result pair. The operand fields are compared
/// by address and are never dereferenced.
struct ComposedFollow {
    /// Address of the nearer operand, used only to check an inline-cache entry.
    nearer: *const FollowSet,
    /// Address of the caller operand, used only to check an inline-cache entry.
    caller: *const FollowSet,
    /// Canonical interned result of the composition.
    composed: &'static FollowSet,
}

// SAFETY: the two pointer fields are opaque identities that are compared but never
// dereferenced, and the composed set is `Send + Sync`. Records are leaked, so no
// thread can observe a dangling record.
unsafe impl Send for ComposedFollow {}
// SAFETY: see the `Send` justification; the record is immutable once published.
unsafe impl Sync for ComposedFollow {}

/// One-entry inline cache for a single generated FOLLOW composition site.
///
/// A generated parser composes the same static nearer FOLLOW with whatever caller
/// FOLLOW its context carries. At one site the caller is usually the same reference
/// as on the previous visit, so a single acquire load and one pointer comparison
/// answer the composition without reaching the process-wide memo. A miss composes
/// through that memo and republishes the record; because records are interned per
/// operand pair, repeated misses reuse records instead of leaking new ones.
///
/// Generated code declares one cache per site as a `static`, so entries live for the
/// whole process. Both operands must likewise outlive the process: generated parsers
/// only pass grammar statics and sets the memo returned earlier.
#[doc(hidden)]
pub struct FollowSiteCache {
    /// Last record published at this site, or null before the first composition.
    entry: AtomicPtr<ComposedFollow>,
}

impl FollowSiteCache {
    /// Constructs an empty site cache usable in a `static` item.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entry: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    /// Caches [`FollowSet::with_caller`] for one generated descent site.
    ///
    /// # Errors
    ///
    /// Returns [`FollowSetError::WidthMismatch`] when the operands describe
    /// different grammar widths.
    #[inline]
    pub fn with_caller(
        &self,
        nearer: &'static FollowSet,
        caller: &FollowSet,
    ) -> Result<&'static FollowSet, FollowSetError> {
        self.composed(nearer, caller, None, BoundaryMode::KeepSelf)
    }

    /// Caches [`FollowSet::union`] for one generated descent site.
    ///
    /// # Errors
    ///
    /// Returns [`FollowSetError::WidthMismatch`] when the operands describe
    /// different grammar widths.
    #[inline]
    pub fn union(
        &self,
        nearer: &'static FollowSet,
        caller: &FollowSet,
    ) -> Result<&'static FollowSet, FollowSetError> {
        self.composed(nearer, caller, None, BoundaryMode::Union)
    }

    /// Caches [`FollowSet::with_caller_accepting`] for one generated `#[greedy(...)]` site.
    ///
    /// # Errors
    ///
    /// Returns [`FollowSetError::WidthMismatch`] when the operands describe
    /// different grammar widths.
    #[inline]
    pub fn with_caller_accepting(
        &self,
        nearer: &'static FollowSet,
        caller: &FollowSet,
        accepted: &'static FollowSet,
    ) -> Result<&'static FollowSet, FollowSetError> {
        self.composed(nearer, caller, Some(accepted), BoundaryMode::KeepSelf)
    }

    /// Caches [`FollowSet::union_accepting`] for one generated `#[greedy(...)]` site.
    ///
    /// # Errors
    ///
    /// Returns [`FollowSetError::WidthMismatch`] when the operands describe
    /// different grammar widths.
    #[inline]
    pub fn union_accepting(
        &self,
        nearer: &'static FollowSet,
        caller: &FollowSet,
        accepted: &'static FollowSet,
    ) -> Result<&'static FollowSet, FollowSetError> {
        self.composed(nearer, caller, Some(accepted), BoundaryMode::Union)
    }

    /// Answers one composition from the published record, or refills the site.
    #[inline]
    fn composed(
        &self,
        nearer: &'static FollowSet,
        caller: &FollowSet,
        accepted: Option<&'static FollowSet>,
        mode: BoundaryMode,
    ) -> Result<&'static FollowSet, FollowSetError> {
        let published = self.entry.load(Ordering::Acquire);
        if !published.is_null() {
            // SAFETY: a non-null entry was published by `refill` from a leaked record,
            // and records are never freed or mutated after publication.
            let published: &'static ComposedFollow = unsafe { &*published };
            if std::ptr::eq(published.caller, caller) {
                debug_assert!(
                    std::ptr::eq(published.nearer, nearer),
                    "one generated site composes one fixed nearer FOLLOW"
                );
                return Ok(published.composed);
            }
        }
        self.refill(nearer, caller, accepted, mode)
    }

    /// Composes through the process-wide memo and republishes this site's record.
    #[cold]
    fn refill(
        &self,
        nearer: &'static FollowSet,
        caller: &FollowSet,
        accepted: Option<&'static FollowSet>,
        mode: BoundaryMode,
    ) -> Result<&'static FollowSet, FollowSetError> {
        let entry = nearer.compose_entry(caller, accepted, mode)?;
        self.entry
            .store(std::ptr::from_ref(entry).cast_mut(), Ordering::Release);
        Ok(entry.composed)
    }
}

impl Default for FollowSiteCache {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FollowSiteCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FollowSiteCache")
            .finish_non_exhaustive()
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
    /// Operand-identity memo: (left, right, acceptance, mode) to the shared record.
    ///
    /// A composition with no acceptance stores `0` in the acceptance slot, which no
    /// operand address can collide with.
    pairs: std::collections::HashMap<
        (usize, usize, usize, u8),
        &'static ComposedFollow,
        PairHashBuilder,
    >,
}

/// Composition buffers reused across calls so a table hit allocates nothing.
#[derive(Default)]
struct Scratch {
    /// Unioned kind words.
    kinds: Vec<u64>,
    /// Pratt boundary words for the chosen mode.
    boundary: Vec<u64>,
    /// Accepted boundary words contributed by a `#[greedy(...)]` composition.
    greedy: Vec<u64>,
    /// Merged sorted atom refinements.
    expected: Vec<ExpectedAtomId>,
}

/// Process-wide composition memo shared by every parsing thread.
///
/// Interned sets are leaked and immutable, and [`FollowSet`] is `Send + Sync`, so
/// one table can serve every thread. After the per-site inline caches in
/// [`FollowSiteCache`] absorb the repeated compositions, this lock is only reached
/// the first time a site meets a given caller.
static INTERNER: LazyLock<Mutex<Interner>> = LazyLock::new(|| Mutex::new(Interner::default()));

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
    greedy: &[u64],
    expected: &[ExpectedAtomId],
) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut hash = u64::from(kind_count).wrapping_mul(K);
    for word in kinds.iter().chain(boundary.iter()).chain(greedy.iter()) {
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

/// Hasher for `(usize, usize, usize, u8)` keys: rotate, xor, multiply per written word.
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
    fn a_greedy_acceptance_marks_kinds_without_changing_what_the_composition_means() {
        let nearer = FollowSet::from_static(3, &[0b001], &[ExpectedAtomId(0)]);
        let caller = FollowSet::from_static(3, &[0b110], &[ExpectedAtomId(2)]);
        let composed = nearer.union(&caller).unwrap();
        assert!(composed.pratt_boundary_contains(1));
        assert!(!composed.greedy_contains(1));

        let accepted = FollowSet::from_static(3, &[0b010], &[]);
        let greedy = nearer.union_accepting(&caller, &accepted).unwrap();

        assert_eq!(
            &*greedy.kinds, &*composed.kinds,
            "FOLLOW membership is what the unmarked union produces"
        );
        assert_eq!(
            &*greedy.expected, &*composed.expected,
            "the mask contributes no expectation"
        );
        assert_eq!(
            &*greedy.pratt_boundary_kinds, &*composed.pratt_boundary_kinds,
            "the accepted kind is still a boundary"
        );
        assert!(greedy.greedy_contains(1), "the accepted kind may extend");
        assert!(
            !greedy.greedy_contains(0) && !greedy.greedy_contains(2),
            "no other kind is accepted"
        );
        assert!(
            std::ptr::eq(greedy, nearer.union_accepting(&caller, &accepted).unwrap()),
            "the acceptance interns by content like every other composition"
        );
        assert!(
            !std::ptr::eq(greedy, composed),
            "the marked and unmarked compositions stay separate sets"
        );
    }

    #[test]
    fn a_descent_composition_drops_the_acceptance_of_its_operands() {
        let base = FollowSet::from_static(3, &[0b011], &[]);
        let accepted = FollowSet::from_static(3, &[0b001], &[]);
        let empty = FollowSet::from_static(3, &[0], &[]);
        let greedy = empty.union_accepting(&base, &accepted).unwrap();
        assert!(greedy.greedy_contains(0));

        let nearer = FollowSet::from_static(3, &[0b100], &[]);
        assert!(!nearer.union(greedy).unwrap().greedy_contains(0));
        assert!(!nearer.with_caller(greedy).unwrap().greedy_contains(0));
    }

    #[test]
    fn a_greedy_acceptance_keeps_the_boundary_its_composition_would_have() {
        let nearer = FollowSet::from_static(3, &[0b001], &[]);
        let caller = FollowSet::from_static(3, &[0b110], &[]);
        let accepted = FollowSet::from_static(3, &[0b010], &[]);

        let kept = nearer.with_caller_accepting(&caller, &accepted).unwrap();

        assert!(kept.contains(1) && kept.contains(2));
        assert!(kept.pratt_boundary_contains(0));
        assert!(
            !kept.pratt_boundary_contains(1) && !kept.pratt_boundary_contains(2),
            "with_caller still keeps only the nearer boundary"
        );
        assert!(kept.greedy_contains(1));
    }

    #[test]
    fn a_greedy_acceptance_site_cache_answers_a_repeated_caller_by_address() {
        static NEARER: FollowSet = FollowSet::from_static(3, &[0b011], &[]);
        static FIRST: FollowSet = FollowSet::from_static(3, &[0b100], &[]);
        static SECOND: FollowSet = FollowSet::from_static(3, &[0b010], &[]);
        static ACCEPTED: FollowSet = FollowSet::from_static(3, &[0b001], &[]);
        static SITE: FollowSiteCache = FollowSiteCache::new();

        let first = SITE
            .with_caller_accepting(&NEARER, &FIRST, &ACCEPTED)
            .unwrap();
        assert!(std::ptr::eq(
            first,
            SITE.with_caller_accepting(&NEARER, &FIRST, &ACCEPTED)
                .unwrap()
        ));
        assert!(first.greedy_contains(0));
        assert!(!first.greedy_contains(1));

        let switched = SITE
            .with_caller_accepting(&NEARER, &SECOND, &ACCEPTED)
            .unwrap();
        assert!(!std::ptr::eq(switched, first));
        assert_eq!(
            switched,
            NEARER.with_caller_accepting(&SECOND, &ACCEPTED).unwrap()
        );
        assert!(std::ptr::eq(
            first,
            SITE.with_caller_accepting(&NEARER, &FIRST, &ACCEPTED)
                .unwrap()
        ));
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
    fn site_caches_answer_repeat_callers_without_reaching_the_memo() {
        static NEARER: FollowSet = FollowSet::from_static(3, &[0b001], &[ExpectedAtomId(0)]);
        static FIRST_CALLER: FollowSet = FollowSet::from_static(3, &[0b010], &[ExpectedAtomId(1)]);
        static SECOND_CALLER: FollowSet = FollowSet::from_static(3, &[0b100], &[ExpectedAtomId(2)]);
        static SITE: FollowSiteCache = FollowSiteCache::new();

        let direct = NEARER.with_caller(&FIRST_CALLER).unwrap();
        let cached = SITE.with_caller(&NEARER, &FIRST_CALLER).unwrap();
        assert!(std::ptr::eq(direct, cached));

        // The memo is process-global and other tests compose concurrently, so
        // prove the hit through the site's own published record instead of a
        // global pair count: a hit returns without republishing the entry.
        let entry_before = SITE.entry.load(std::sync::atomic::Ordering::Acquire);
        let repeat = SITE.with_caller(&NEARER, &FIRST_CALLER).unwrap();
        assert!(std::ptr::eq(direct, repeat));
        assert!(std::ptr::eq(
            SITE.entry.load(std::sync::atomic::Ordering::Acquire),
            entry_before
        ));

        let switched = SITE.with_caller(&NEARER, &SECOND_CALLER).unwrap();
        assert!(std::ptr::eq(
            switched,
            NEARER.with_caller(&SECOND_CALLER).unwrap()
        ));
        assert_eq!(&*switched.kinds, &[0b101]);
        assert!(switched.pratt_boundary_contains(0));
        assert!(!switched.pratt_boundary_contains(2));

        let back = SITE.with_caller(&NEARER, &FIRST_CALLER).unwrap();
        assert!(std::ptr::eq(direct, back));
    }

    #[test]
    fn site_caches_union_matches_direct_composition_and_reports_width_mismatch() {
        static NEARER: FollowSet = FollowSet::from_static(3, &[0b001], &[]);
        static CALLER: FollowSet = FollowSet::from_static(3, &[0b100], &[]);
        static WIDE: FollowSet = FollowSet::from_static(127, &[0, 0], &[]);
        static SITE: FollowSiteCache = FollowSiteCache::new();
        static MISMATCHED: FollowSiteCache = FollowSiteCache::new();

        let composed = SITE.union(&NEARER, &CALLER).unwrap();
        assert!(std::ptr::eq(composed, NEARER.union(&CALLER).unwrap()));
        assert!(composed.pratt_boundary_contains(2));

        assert_eq!(
            MISMATCHED.with_caller(&NEARER, &WIDE),
            Err(FollowSetError::WidthMismatch {
                left: 3,
                right: 127,
            })
        );
    }

    #[test]
    fn interned_sets_cross_thread_boundaries() {
        static NEARER: FollowSet = FollowSet::from_static(3, &[0b001], &[]);
        static CALLER: FollowSet = FollowSet::from_static(3, &[0b010], &[]);

        let composed = NEARER.with_caller(&CALLER).unwrap();
        let observed = std::thread::spawn(|| NEARER.with_caller(&CALLER).unwrap())
            .join()
            .expect("the composing thread completes");
        assert!(std::ptr::eq(composed, observed));
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
