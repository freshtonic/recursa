#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Fast deterministic hashing for analysis-internal interning tables.
//!
//! Every map in this crate keyed through [`FxHasher`](crate::fast_hash::FxHasher) is a pure lookup
//! structure: entries are only inserted and queried, never iterated in an
//! order that reaches published output. The hasher is the Fx polynomial used
//! by rustc: seedless, deterministic across runs, and far cheaper than the
//! standard SipHash for the short structural keys interned during grammar
//! analysis. Determinism of generated output therefore never depends on
//! these hash values, while run-to-run reproducibility is preserved because
//! the function has no random state.

use std::hash::Hasher;

/// Multiplicative constant from the Firefox/rustc Fx hash function.
const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
/// Bit rotation applied between folded words.
const ROTATE: u32 = 5;

#[derive(Clone, Copy, Debug, Default)]
/// Seedless Fx hasher producing deterministic 64-bit hashes.
pub(crate) struct FxHasher {
    /// Current folded hash state.
    hash: u64,
}

impl FxHasher {
    /// Folds one word into the hash state.
    #[inline]
    fn add_to_hash(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(ROTATE) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let (word, rest) = bytes.split_at(8);
            self.add_to_hash(u64::from_le_bytes(word.try_into().expect("eight bytes")));
            bytes = rest;
        }
        if bytes.len() >= 4 {
            let (word, rest) = bytes.split_at(4);
            self.add_to_hash(u64::from(u32::from_le_bytes(
                word.try_into().expect("four bytes"),
            )));
            bytes = rest;
        }
        for byte in bytes {
            self.add_to_hash(u64::from(*byte));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add_to_hash(u64::from(value));
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.add_to_hash(u64::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add_to_hash(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.add_to_hash(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add_to_hash(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}
