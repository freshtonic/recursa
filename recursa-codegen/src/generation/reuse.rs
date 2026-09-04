//! Reuse of a published Generation when the discovered grammar source is unchanged.
//!
//! Every build-script run must run discovery, because Cargo invalidation directives come from
//! it. Everything after discovery is a deterministic function of the discovered module tree,
//! the active configuration, and the compiled generator. This module keys that function's
//! input and remembers which Generation it produced, so an unchanged grammar re-selects its
//! published Generation without resolution, analysis, rendering, or publication.

use super::*;

/// Output-directory-relative directory holding one reuse entry per discovered-source key.
const REUSE_DIRECTORY: &str = "recursa-reuse";
/// Entry file naming the published Generation identity.
const IDENTITY_FILE: &str = "identity";
/// Entry file holding the exact loader selector published with the Generation.
const SELECTOR_FILE: &str = "selector.rs";
/// Entry file holding the Cargo warning directives emitted when the Generation was produced.
const WARNINGS_FILE: &str = "warnings";

/// A published Generation re-selected from the reuse index.
pub(super) struct ReusedGeneration {
    /// Cargo warning directives recorded when the Generation was produced.
    pub(super) warnings: Vec<String>,
}

/// Keys everything after discovery that can change generated output.
///
/// The key covers the compiled generator and protocol, the codegen capabilities, the edition
/// and Cargo configuration, the values of every environment variable discovery depends on, the
/// discovered file set, and the active items of every logical module as token text. Token text
/// carries no comments and no spans, so a comment-only edit keeps the key; a doc comment is an
/// attribute and changes it.
pub(super) fn reuse_key(discovered: &DiscoveredSourceTree, cfg: &CargoCfgSnapshot) -> String {
    let inputs = &discovered.discovery_inputs;
    let mut canonical = format!(
        "{};diagrams={};explorer={};edition={:?};cfg={cfg:?};cfg_snapshot={:?};root={}",
        generation_protocol_identity(),
        cfg!(feature = "diagrams"),
        cfg!(feature = "explorer"),
        inputs.edition,
        inputs.cfg_snapshot,
        inputs.selected_root.display(),
    );
    let mut environment = inputs.environment_dependencies.clone();
    environment.sort();
    environment.dedup();
    for variable in environment {
        canonical.push_str(&format!(";env={variable}={:?}", env::var_os(&variable)));
    }
    for file in &inputs.source_files {
        canonical.push_str(&format!(";file={}", file.display()));
    }
    for inclusion in &discovered.tree.inclusions {
        let items = &inclusion.items;
        canonical.push_str(&format!(
            ";module={:?}:{:?}:{:?}:{:?}:{:?}:{}:{}",
            inclusion.id,
            inclusion.parent,
            inclusion.logical_path,
            inclusion.source,
            inclusion.source_kind,
            inclusion.visibility.to_token_stream(),
            quote!(#(#items)*),
        ));
    }
    format!("r_{:016x}", stable_hash(canonical.as_bytes()))
}

/// Re-selects the Generation recorded for `key` when it is still completely published.
///
/// Returns `None` when there is no entry, when the entry is unreadable, or when the recorded
/// Generation directory is missing its root or loader map. A `None` never prevents a fresh
/// publication from overwriting the entry.
pub(super) fn reuse_published(
    out_dir: &Path,
    key: &str,
) -> Result<Option<ReusedGeneration>, String> {
    let entry = out_dir.join(REUSE_DIRECTORY).join(key);
    let Ok(identity) = fs::read_to_string(entry.join(IDENTITY_FILE)) else {
        return Ok(None);
    };
    let identity = identity.trim().to_owned();
    if identity.is_empty() || !identity.starts_with("g_") {
        return Ok(None);
    }
    let published = out_dir.join("recursa-generations").join(&identity);
    if !published.join("root.rs").is_file() || !published.join("loader-map").is_file() {
        return Ok(None);
    }
    let Ok(selector) = fs::read_to_string(entry.join(SELECTOR_FILE)) else {
        return Ok(None);
    };
    if !selector
        .lines()
        .next()
        .is_some_and(|line| line == format!("// recursa-generation:{identity}"))
    {
        return Ok(None);
    }
    let warnings = fs::read_to_string(entry.join(WARNINGS_FILE))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    select_generation(out_dir, &selector, || Ok(()))?;
    Ok(Some(ReusedGeneration { warnings }))
}

/// Records which Generation the discovered source produced, after that Generation is selected.
///
/// The entry is staged privately and moved into place with one rename, so a reader never
/// observes a partial entry. Recording happens after publication; a failure here leaves a
/// complete, selected Generation behind and only costs the next build its reuse.
pub(super) fn record_reuse(
    out_dir: &Path,
    key: &str,
    identity: &str,
    selector: &str,
    warnings: &[String],
) -> Result<(), String> {
    let reuse = out_dir.join(REUSE_DIRECTORY);
    fs::create_dir_all(&reuse)
        .map_err(|error| format!("cannot create the reuse index: {error}"))?;
    let nonce = staging_nonce()?;
    let staging = reuse.join(format!(".staging-{}-{nonce}-{key}", std::process::id()));
    let staged = (|| -> Result<(), String> {
        fs::create_dir(&staging)
            .map_err(|error| format!("cannot stage the reuse entry: {error}"))?;
        fs::write(staging.join(IDENTITY_FILE), format!("{identity}\n"))
            .map_err(|error| format!("cannot stage the reuse identity: {error}"))?;
        fs::write(staging.join(SELECTOR_FILE), selector)
            .map_err(|error| format!("cannot stage the reuse selector: {error}"))?;
        let mut recorded = warnings.join("\n");
        if !recorded.is_empty() {
            recorded.push('\n');
        }
        fs::write(staging.join(WARNINGS_FILE), recorded)
            .map_err(|error| format!("cannot stage the reuse warnings: {error}"))?;
        let entry = reuse.join(key);
        if entry.exists() {
            fs::remove_dir_all(&entry)
                .map_err(|error| format!("cannot replace the previous reuse entry: {error}"))?;
        }
        fs::rename(&staging, &entry)
            .map_err(|error| format!("cannot publish the reuse entry: {error}"))
    })();
    if staged.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    staged
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    macro_rules! focused_case {
        ($name:ident) => {
            #[test]
            fn $name() {
                test_support::$name();
            }
        };
    }
    focused_case!(unchanged_sources_reuse_the_published_generation_without_analysis);
    focused_case!(comment_only_edits_reuse_the_published_generation);
    focused_case!(semantic_edits_do_not_reuse_the_published_generation);
    focused_case!(missing_or_incomplete_generations_are_not_reused);
    focused_case!(debug_directory_requests_bypass_reuse);
}
