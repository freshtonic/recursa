//! Bounds how deeply `Arbitrary` sampling can nest Node construction.
//!
//! `arbitrary::Unstructured::int_in_range` deterministically returns the range's lower bound once
//! its byte buffer is exhausted — confirmed empirically, not merely deep or unlikely. For a
//! mutually-recursive grammar (any real one), the generated enum-variant selection
//! (`input.int_in_range(0..=last)?`) can then deterministically keep re-selecting a recursive
//! variant forever, causing a genuine stack overflow rather than a large-but-finite tree. A
//! thread-local depth counter is the standard, reliable mitigation: it terminates regardless of
//! how the input bytes are distributed, unlike anything that depends on entropy remaining.

use std::cell::Cell;

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Maximum nested Node-construction depth before generated code is asked to prefer a
/// non-recursive ("leaf") variant instead of choosing uniformly at random.
///
/// Deliberately conservative: a real generated Node's own `arbitrary()` body is not one thin
/// stack frame — it can pass through several generated helpers per level (`arbitrary()`,
/// `__recursa_arbitrary_with()`, per-field construction closures), and a large real grammar's
/// Node types are far larger than a minimal reproduction's. 64 was empirically still enough to
/// overflow the main thread's stack against a ~1,400-node-type real-world grammar; this value
/// needs to stay well below the actual stack limit, not just below the *number* of frames a
/// trivial recursive type would use.
pub const MAX_DEPTH: u32 = 12;

/// RAII guard incrementing the shared sampling-depth counter for its lifetime; decrements on
/// drop, including on an early return, so a stray non-zero counter cannot outlive one `arbitrary`
/// call tree even if a variant's construction returns an error partway through.
#[doc(hidden)]
pub struct DepthGuard(());

impl DepthGuard {
    /// Enters one more level of Node construction, returning the guard and the resulting depth.
    pub fn enter() -> (Self, u32) {
        let depth = DEPTH.with(|cell| {
            let depth = cell.get() + 1;
            cell.set(depth);
            depth
        });
        (Self(()), depth)
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DEPTH.with(|cell| cell.set(cell.get() - 1));
    }
}
