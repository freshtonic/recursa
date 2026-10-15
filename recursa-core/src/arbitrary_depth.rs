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
/// A real generated Node's own `arbitrary()` body is not one thin stack frame — it can pass
/// through several generated helpers per level (`arbitrary()`, `__recursa_arbitrary_with()`,
/// per-field construction closures), and a large real grammar's Node types are far larger than a
/// minimal reproduction's. 64 was empirically still enough to overflow an 8 MiB (default) main
/// thread's stack against a ~1,400-node-type real-world grammar.
///
/// `recursa-explorer`'s sample worker now runs sampling on a dedicated 64 MiB thread stack (see
/// `SAMPLE_WORKER_STACK_SIZE` in its `worker.rs`) instead of a process's default stack, which is
/// what makes 64 safe now: re-verified empirically against pg-sql's real ~1,400-node-type grammar
/// with well over 1,000 live `/api/sample` requests spread across `MAX_DEPTH` values from 12 up
/// to 128 (including a stress pass with a much larger entropy buffer, to make deep recursion more
/// likely rather than less) — zero crashes at any tested depth, so 64 lands with real headroom
/// below the highest depth actually exercised crash-free. This value must stay well below
/// whatever stack a caller actually runs sampling on; raising it again needs the same empirical
/// re-check, not a guess.
pub const MAX_DEPTH: u32 = 64;

/// How many times a sampled struct with declared text rules samples again after a rule refuses
/// it, before the sampler answers that the input bytes describe no value
/// (`docs/declared-rejections.md`, "Sampling").
pub const REJECTION_ATTEMPTS: u32 = 8;

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
