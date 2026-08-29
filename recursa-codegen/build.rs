//! Computes the generator implementation fingerprint used by immutable Generation identities.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// FNV offset used by the generator's existing stable identity hash.
const STABLE_HASH_OFFSET: u64 = 0x0cbf_29ce_4842_2325;
/// FNV prime used by the generator's existing stable identity hash.
const STABLE_HASH_PRIME: u64 = 0x0100_0000_01b3;
/// Test-only source files excluded from the published `recursa-codegen` package.
const EXCLUDED_SOURCES: &[&str] = &[
    "src/semantic_projection_tests.rs",
    "src/source_analysis_contract_tests.rs",
    "src/source_analysis_tests.rs",
];

/// Hashes every shipped generator source in stable relative-path order.
fn main() {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo provides CARGO_MANIFEST_DIR to build.rs"),
    );
    let mut sources = vec![manifest_dir.join("build.rs")];
    collect_rust_sources(&manifest_dir.join("src"), &mut sources);
    sources.sort_by(|left, right| {
        relative_source_name(&manifest_dir, left).cmp(&relative_source_name(&manifest_dir, right))
    });

    let mut hash = STABLE_HASH_OFFSET;
    for source in sources {
        let relative = relative_source_name(&manifest_dir, &source);
        if EXCLUDED_SOURCES.contains(&relative.as_str()) {
            continue;
        }
        let contents = fs::read(&source)
            .unwrap_or_else(|error| panic!("cannot fingerprint {}: {error}", source.display()));
        hash_bytes(&mut hash, &(relative.len() as u64).to_le_bytes());
        hash_bytes(&mut hash, relative.as_bytes());
        hash_bytes(&mut hash, &(contents.len() as u64).to_le_bytes());
        hash_bytes(&mut hash, &contents);
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src");
    println!("cargo::rustc-env=RECURSA_CODEGEN_SOURCE_FINGERPRINT={hash:016x}");
}

/// Collects Rust sources recursively without depending on host directory iteration order.
fn collect_rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!("cannot inspect {} entry: {error}", directory.display())
        });
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", path.display()));
        if file_type.is_dir() {
            collect_rust_sources(&path, sources);
        } else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs")
        {
            sources.push(path);
        }
    }
}

/// Returns one slash-separated source name independent of the checkout's absolute path.
fn relative_source_name(manifest_dir: &Path, source: &Path) -> String {
    source
        .strip_prefix(manifest_dir)
        .expect("fingerprinted source belongs to recursa-codegen")
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Extends one deterministic source fingerprint with a framed byte sequence.
fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(STABLE_HASH_PRIME);
    }
}
