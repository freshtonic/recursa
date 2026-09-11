#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Local loopback launcher for the grammar-linked Recursa Explorer.

mod assets;
mod browser;
mod protocol;
mod server;
mod worker;

use recursa::explorer::Adapter;
use std::fmt;

/// Runs a local Explorer until its browser requests orderly shutdown.
///
/// The launcher binds an operating-system-assigned loopback port before opening
/// the system browser, then blocks while it serves the bundled application and
/// grammar-specific protocol.
///
/// When re-invoked as a sample worker (the mechanism the running server itself uses to
/// isolate Arbitrary sampling in a disposable child process — see [`worker`]), this instead
/// samples, renders, and exits without ever binding a listener.
///
/// # Errors
///
/// Returns [`RunError`] when the listener cannot be bound, the browser cannot be
/// launched, or the server cannot receive or answer a request.
pub fn run(adapter: Adapter) -> Result<(), RunError> {
    if let Some(bytes) = worker::requested_sample_bytes() {
        worker::run_sample_worker(adapter, &bytes);
    }
    server::run(adapter)
}

/// Failure to bind, launch, or serve the local Explorer.
#[derive(Debug)]
pub struct RunError {
    /// Stable operation that failed.
    operation: &'static str,
    /// Underlying implementation error retained for diagnostics.
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl RunError {
    /// Wraps a private launcher failure without exposing transport dependencies.
    fn new(
        operation: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            operation,
            source: Box::new(source),
        }
    }

    /// Retains an already type-erased bind failure.
    fn boxed(operation: &'static str, source: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self { operation, source }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Explorer {} failed: {}",
            self.operation, self.source
        )
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
