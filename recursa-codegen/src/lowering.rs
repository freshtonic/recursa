#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Shared identities and reproducible inputs for source lowering.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::TextEquivalence;
use crate::source_syntax::{CaptureMode, PrettyDefaults};

/// Defines a dense source-model identity with checked allocation and indexing.
macro_rules! source_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(u32);

        impl $name {
            pub(crate) fn next(length: usize) -> Self {
                Self(length.try_into().expect("source model ID overflow"))
            }
        }
    };
    ($name:ident, indexed) => {
        source_id!($name);

        impl $name {
            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

source_id!(ModuleId, indexed);
source_id!(NodeId, indexed);
source_id!(ElementId);
source_id!(TokenDefinitionId, indexed);
source_id!(TokenUseId, indexed);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Rust syntax edition accepted by source discovery.
pub(crate) enum RustEdition {
    /// Parses authored source using Rust 2024 syntax rules.
    Rust2024,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source pipeline phase that owns a discovery diagnostic.
pub(crate) enum DiagnosticStage {
    /// The selected grammar source or module tree could not be discovered.
    Discovery,
    /// An authored Rust source file could not be parsed.
    Syntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Filesystem and Cargo facts that make source discovery reproducible.
pub(crate) struct DiscoveryInputs {
    /// Grammar source file selected by the build script.
    pub(crate) selected_root: PathBuf,
    /// Active source files traversed in canonical path order.
    pub(crate) source_files: Vec<PathBuf>,
    /// Rust syntax edition used for every selected source file.
    pub(crate) edition: RustEdition,
    /// Existing and missing module paths that must invalidate the build.
    pub(crate) watched_module_candidates: Vec<PathBuf>,
    /// Canonical active Cargo configuration supplied to source filtering.
    pub(crate) cfg_snapshot: Vec<String>,
    /// Environment keys whose values can alter discovery or lowering.
    pub(crate) environment_dependencies: Vec<String>,
}

impl DiscoveryInputs {
    /// Returns authored paths that can safely participate in Cargo invalidation.
    pub(crate) fn watched_paths(&self) -> Vec<PathBuf> {
        let mut paths = self
            .source_files
            .iter()
            .chain(&self.watched_module_candidates)
            .cloned()
            .collect::<BTreeSet<_>>();
        if safe_manifest_relative_path(&self.selected_root) {
            paths.insert(self.selected_root.clone());
        }
        paths.into_iter().collect()
    }

    /// Returns existing paths to emit as `cargo::rerun-if-changed`.
    ///
    /// Cargo treats a missing watched path as always dirty. Missing module candidates are
    /// therefore represented by their nearest existing ancestor below the package root.
    pub(crate) fn invalidation_paths(&self, manifest_dir: &Path) -> Vec<PathBuf> {
        let mut paths = BTreeSet::new();
        for path in self.watched_paths() {
            if manifest_dir.join(&path).exists() {
                paths.insert(path);
            } else if let Some(ancestor) = nearest_existing_ancestor(manifest_dir, &path) {
                paths.insert(ancestor);
            }
        }
        paths.into_iter().collect()
    }
}

/// Returns the closest existing ancestor directory, excluding the package root.
fn nearest_existing_ancestor(manifest_dir: &Path, path: &Path) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(directory) = current {
        if directory.as_os_str().is_empty() {
            return None;
        }
        if manifest_dir.join(directory).is_dir() {
            return Some(directory.to_path_buf());
        }
        current = directory.parent();
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Reproducible configuration carried from discovery into source modeling.
pub(crate) struct BuildInputs {
    /// Filesystem and Cargo discovery inputs.
    pub(crate) discovery: DiscoveryInputs,
    /// Selected module anchor.
    pub(crate) module_anchor: SelectedRootAnchor,
    /// Path used to reach the Recursa facade.
    pub(crate) facade: FacadeMapping,
    /// Grammar-wide text comparison policy.
    pub(crate) text_equivalence: TextEquivalence,
    /// Grammar-wide source capture policy.
    pub(crate) spans: CaptureMode,
    /// Whether grammar diagrams are generated.
    pub(crate) diagrams: bool,
    /// Whether semantic containers allocate in a parse-owned bump arena.
    pub(crate) arena_ast: bool,
    /// Grammar-wide Pretty defaults.
    pub(crate) pretty: PrettyDefaults,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Logical module path containing the selected grammar root.
pub(crate) struct SelectedRootAnchor {
    /// Path segments relative to the crate root.
    pub(crate) segments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Authored crate identifier used to reach Recursa APIs.
pub(crate) struct FacadeMapping {
    /// Facade crate identifier.
    pub(crate) ident: String,
}

/// Returns whether a path remains relative to its package after normalization.
fn safe_manifest_relative_path(path: &Path) -> bool {
    let mut depth = 0_usize;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth += 1,
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}
