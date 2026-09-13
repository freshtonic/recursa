//! Self-exec worker mode for isolated Arbitrary sampling.
//!
//! Generating an Arbitrary sample recurses through generated `arbitrary()` bodies; the
//! depth-guard in `recursa-core` bounds that recursion, but running the whole
//! sample-then-materialize-then-render pipeline in a disposable child process is a second,
//! independent safety net. If a sample ever does overflow the stack, the OS kills only that
//! child — the Explorer server keeps serving every other request.

use recursa::PrettyConfig;
use recursa::explorer::Adapter;

/// Argv flag this process was re-invoked with to act as a one-shot sample worker.
const WORKER_FLAG: &str = "--recursa-explorer-sample-worker";

/// Stack size given to the thread that actually samples and renders, instead of relying on this
/// process's default main-thread stack. Isolating sampling in its own subprocess (see the module
/// doc comment) only bounds the blast radius of a stack overflow — it does not by itself raise
/// how deep sampling can safely recurse. A generously large stack does: it is what lets
/// `recursa_core::arbitrary_depth::MAX_DEPTH` be tuned higher than a default OS stack (typically
/// ~8 MiB) would allow before a genuine overflow. 64 MiB was empirically crash-free up to
/// `MAX_DEPTH = 128` against pg-sql's real grammar (see the tuning notes on `MAX_DEPTH` itself).
const SAMPLE_WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Returns the hex-decoded entropy this process should sample from, when invoked as a worker.
pub(crate) fn requested_sample_bytes() -> Option<Vec<u8>> {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == WORKER_FLAG {
            return args.next().and_then(|hex| decode_hex(&hex));
        }
    }
    None
}

/// Samples, materializes, and Pretty-renders one document from `bytes`, then exits the process.
///
/// Exits `0` with the rendered source on stdout on success. Exits `1` with a diagnostic on
/// stderr for every failure this code detects itself (an unsupported grammar, or a sampling
/// error such as the depth-guard's `NotEnoughData` backstop); an undetected failure such as a
/// genuine stack overflow instead terminates this process by signal. The parent treats every
/// non-zero or signal-terminated exit identically — see [`spawn_and_sample`].
pub(crate) fn run_sample_worker(adapter: Adapter, bytes: &[u8]) -> ! {
    // Run on a spawned thread with an explicit large stack rather than this process's default
    // main-thread stack, so a deep sample has real headroom before it genuinely overflows.
    let bytes = bytes.to_vec();
    let result = std::thread::Builder::new()
        .stack_size(SAMPLE_WORKER_STACK_SIZE)
        .spawn(move || sample_and_render(adapter, &bytes))
        .expect("spawning the sample worker's large-stack thread")
        .join();
    match result {
        Ok(Ok(rendered)) => {
            print!("{rendered}");
            std::process::exit(0);
        }
        Ok(Err(message)) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
        Err(_) => {
            // The spawned thread itself panicked (distinct from a stack overflow, which kills
            // this whole process rather than unwinding): still just a detected failure.
            eprintln!("sample worker thread panicked");
            std::process::exit(1);
        }
    }
}

/// The worker's actual sample-then-render logic, factored out so its ordinary error paths
/// (as opposed to a genuine crash) are unit-testable in-process.
fn sample_and_render(adapter: Adapter, bytes: &[u8]) -> Result<String, String> {
    let document = adapter
        .sample(bytes)
        .ok_or("this grammar does not configure Arbitrary sampling")?
        .map_err(|error| error.message().to_owned())?;
    document
        .pretty(&PrettyConfig::default())
        .map(|output| output.rendered)
        .ok_or_else(|| "this grammar does not configure Pretty".to_owned())
}

/// Spawns one subprocess worker for `bytes` and returns its rendered source, or why it failed.
///
/// Every non-success exit — including one caused by the OS terminating the child for a stack
/// overflow — is reported the same way; the caller does not need to distinguish a graceful
/// sampling error from a crash, and the frontend retries either kind with fresh entropy.
pub(crate) fn spawn_and_sample(bytes: &[u8]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = std::process::Command::new(exe)
        .arg(WORKER_FLAG)
        .arg(encode_hex(bytes))
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "sample generation process {}",
            describe_exit(&output.status)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

/// Describes a non-zero exit, calling out the common stack-overflow signature on Unix.
fn describe_exit(status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("was terminated by signal {signal} (likely a stack overflow)");
        }
    }
    format!("exited with {status}")
}

/// Encodes bytes as lowercase hex for a single argv-safe argument.
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Decodes lowercase or uppercase hex, rejecting an odd-length or non-hex string.
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_arbitrary_byte_sequences() {
        for bytes in [
            Vec::new(),
            vec![0],
            vec![255, 0, 16, 1],
            (0..=255).collect::<Vec<u8>>(),
        ] {
            assert_eq!(decode_hex(&encode_hex(&bytes)), Some(bytes));
        }
    }

    #[test]
    fn decode_hex_rejects_malformed_input() {
        assert_eq!(decode_hex("1"), None, "odd length");
        assert_eq!(decode_hex("zz"), None, "non-hex digits");
    }

    #[test]
    fn sample_and_render_reports_a_grammar_without_arbitrary() {
        let adapter = recursa_example_json::grammar::Value::explorer();
        let error = sample_and_render(adapter, &[1, 2, 3]).unwrap_err();
        assert_eq!(error, "this grammar does not configure Arbitrary sampling");
    }

    #[test]
    fn sample_and_render_is_deterministic_for_a_grammar_with_arbitrary_and_pretty() {
        let adapter = recursa_explorer_capabilities_fixture::grammar::Root::explorer();
        let first = sample_and_render(adapter, &[7, 11, 13]).unwrap();
        let repeated = sample_and_render(adapter, &[7, 11, 13]).unwrap();
        assert_eq!(first, repeated);
    }
}
