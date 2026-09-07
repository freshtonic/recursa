#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Opt-in, low-overhead stage timing for generation and LR analysis.
//!
//! Setting `RECURSA_PROFILE` to any value other than the empty string or `0` writes one line per
//! completed span to standard error. Nesting is represented by two-space indentation, so the
//! output remains useful when Recursa runs inside a Cargo build script without mixing profiler
//! records with Cargo instructions on standard output.

use std::cell::Cell;
use std::env;
use std::ffi::OsStr;
use std::io::Write as _;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Environment variable that enables stage timing.
pub(crate) const ENVIRONMENT: &str = "RECURSA_PROFILE";

/// Process-wide environment decision; build scripts do not change profiler configuration after
/// generation starts.
static ENABLED: OnceLock<bool> = OnceLock::new();

thread_local! {
    /// Nesting depth of enabled spans on this thread.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// One timed stage whose completion is reported when it leaves scope.
pub(crate) struct Span {
    /// Stable, dot-separated stage name.
    name: &'static str,
    /// Start time, absent when profiling is disabled.
    started: Option<Instant>,
    /// Nesting depth captured at entry.
    depth: usize,
}

impl Span {
    /// Starts a stage if `RECURSA_PROFILE` enables profiling.
    pub(crate) fn enter(name: &'static str) -> Self {
        Self::enter_if(name, enabled())
    }

    /// Starts a stage when `enabled`, keeping environment inspection out of deterministic tests.
    fn enter_if(name: &'static str, enabled: bool) -> Self {
        if !enabled {
            return Self {
                name,
                started: None,
                depth: 0,
            };
        }
        let depth = DEPTH.with(|depth| {
            let current = depth.get();
            depth.set(current + 1);
            current
        });
        Self {
            name,
            started: Some(Instant::now()),
            depth,
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some(started) = self.started else {
            return;
        };
        DEPTH.with(|depth| {
            debug_assert_eq!(depth.get(), self.depth + 1);
            depth.set(self.depth);
        });
        let _ = writeln!(
            std::io::stderr().lock(),
            "{}",
            record(self, started.elapsed())
        );
    }
}

/// Whether the process environment requests profiling.
fn enabled() -> bool {
    *ENABLED.get_or_init(|| requested(env::var_os(ENVIRONMENT).as_deref()))
}

/// Interprets the environment value without reading process-global state.
fn requested(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// Formats one stable profiler record around its measured duration.
fn record(span: &Span, elapsed: Duration) -> String {
    format!(
        "recursa-profile {}{}: {:.3} ms",
        "  ".repeat(span.depth),
        span.name,
        elapsed.as_secs_f64() * 1_000.0
    )
}

#[cfg(test)]
mod tests {
    //! Deterministic profiler configuration, hierarchy, and rendering checks.

    use super::*;

    #[test]
    fn only_nonempty_nonzero_values_request_profiling() {
        assert!(!requested(None));
        assert!(!requested(Some(OsStr::new(""))));
        assert!(!requested(Some(OsStr::new("0"))));
        assert!(requested(Some(OsStr::new("1"))));
        assert!(requested(Some(OsStr::new("yes"))));
    }

    #[test]
    fn spans_track_lifo_hierarchy_and_render_stable_indentation() {
        DEPTH.set(0);
        let root = Span::enter_if("generation", true);
        assert_eq!(root.depth, 0);
        assert_eq!(DEPTH.get(), 1);
        {
            let child = Span::enter_if("render", true);
            assert_eq!(child.depth, 1);
            assert_eq!(DEPTH.get(), 2);
            assert_eq!(
                record(&child, Duration::from_micros(1_234)),
                "recursa-profile   render: 1.234 ms"
            );
        }
        assert_eq!(DEPTH.get(), 1);
        drop(root);
        assert_eq!(DEPTH.get(), 0);

        let disabled = Span::enter_if("disabled", false);
        assert!(disabled.started.is_none());
        assert_eq!(DEPTH.get(), 0);
    }
}
