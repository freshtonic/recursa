use std::path::Path;
use std::process::Command;

/// Uses Cargo's package graph to clear one downstream package from a reusable target.
///
/// Cargo and rustdoc may make package artifacts read-only. A later scenario must
/// therefore remove the fixture package's own artifacts before rebuilding, but
/// retain compiled dependencies and similarly named packages. Delegating ownership
/// to `cargo clean -p` also covers compiler-specific incremental and build-output
/// naming without scanning or following paths ourselves.
pub(crate) fn clean_downstream_package(package: &Path, package_name: &str, target: &Path) {
    let manifest = package.join("Cargo.toml");
    if !manifest.is_file() || !target.exists() {
        return;
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .arg("clean")
        .arg("--offline")
        .arg("-p")
        .arg(package_name)
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(target)
        .current_dir(package)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run Cargo package cleanup for stable fixture");
    assert!(
        output.status.success(),
        "Cargo cleanup failed for {package_name}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Uses Cargo to clear rustdoc output owned by one dedicated fixture target.
///
/// Cargo does not permit combining `clean -p` with `clean --doc`. Stable
/// downstream fixture targets are not shared with another package root, so the
/// complete doc tree belongs to that fixture and can be reset authoritatively
/// immediately before its rustdoc scenario.
#[allow(dead_code)]
pub(crate) fn clean_downstream_docs(package: &Path, target: &Path) {
    let manifest = package.join("Cargo.toml");
    if !manifest.is_file() || !target.join("doc").exists() {
        return;
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .arg("clean")
        .arg("--offline")
        .arg("--doc")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(target)
        .current_dir(package)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run Cargo rustdoc cleanup for stable fixture");
    assert!(
        output.status.success(),
        "Cargo rustdoc cleanup failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
