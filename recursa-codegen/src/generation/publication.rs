//! Atomic generation publication and selection.
//!
//! This module owns staging, atomic rename, selection invalidation, and failure
//! checkpoints so rendering code cannot accidentally authorize partial output.

use super::*;

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
    focused_case!(incomplete_staging_is_unselectable);
    focused_case!(every_failed_publication_checkpoint_invalidates_stale_selection_and_recovers);
    focused_case!(filesystem_invalidation_failure_poisons_stale_selection_and_recovers);
    focused_case!(filesystem_rename_failures_leave_no_loadable_or_staged_generation);
}

/// Loader file that atomically selects one complete generated payload.
pub(super) const SELECTED_GENERATION: &str = "recursa-selected.rs";

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn publish(
    out_dir: &Path,
    identity: &str,
    rendered: &RenderedGeneration,
) -> Result<(), String> {
    publish_with_checkpoint(out_dir, identity, rendered, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) enum PublicationCheckpoint {
    BeforeStaging,
    DuringStaging,
    BeforeSelection,
    DuringSelection,
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn publish_with_checkpoint(
    out_dir: &Path,
    identity: &str,
    rendered: &RenderedGeneration,
    mut checkpoint: impl FnMut(PublicationCheckpoint) -> Result<(), String>,
) -> Result<(), String> {
    let generations = out_dir.join("recursa-generations");
    fs::create_dir_all(&generations)
        .map_err(|error| format!("cannot create Generation directory: {error}"))?;
    let published = generations.join(identity);
    if published.exists() {
        if !published.join("root.rs").is_file()
            || !published.join("loader-map").is_file()
            || rendered
                .nodes
                .iter()
                .any(|(name, _)| !published.join(name).is_file())
        {
            return Err("an existing published Generation is incomplete".to_owned());
        }
    } else {
        checkpoint(PublicationCheckpoint::BeforeStaging)?;
        let nonce = staging_nonce()?;
        let staging = generations.join(format!(
            ".staging-{}-{nonce}-{identity}",
            std::process::id()
        ));
        fs::create_dir(&staging)
            .map_err(|error| format!("cannot create Generation staging directory: {error}"))?;
        let staged = stage_and_publish(&staging, &published, rendered, || {
            checkpoint(PublicationCheckpoint::DuringStaging)
        });
        if staged.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        staged?;
    }
    checkpoint(PublicationCheckpoint::BeforeSelection)?;
    select_generation(out_dir, rendered, || {
        checkpoint(PublicationCheckpoint::DuringSelection)
    })
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn stage_and_publish(
    staging: &Path,
    published: &Path,
    rendered: &RenderedGeneration,
    after_root: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    fs::write(staging.join("root.rs"), &rendered.root)
        .map_err(|error| format!("cannot stage generated root: {error}"))?;
    after_root()?;
    for (name, node) in &rendered.nodes {
        fs::write(staging.join(name), node)
            .map_err(|error| format!("cannot stage generated Node `{name}`: {error}"))?;
    }
    let loader_map = rendered
        .node_anchors
        .iter()
        .zip(&rendered.nodes)
        .map(|(anchor, (name, _))| format!("{anchor}\t{name}\n"))
        .collect::<String>();
    fs::write(staging.join("loader-map"), loader_map)
        .map_err(|error| format!("cannot stage the Node loader map: {error}"))?;
    fs::rename(staging, published)
        .map_err(|error| format!("cannot cross the Generation publication barrier: {error}"))?;
    Ok(())
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn select_generation(
    out_dir: &Path,
    rendered: &RenderedGeneration,
    during_selection: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let nonce = staging_nonce()?;
    let staging = out_dir.join(format!(
        ".recursa-selector-{}-{nonce}.rs",
        std::process::id()
    ));
    if let Err(error) = fs::write(&staging, &rendered.selector) {
        let _ = fs::remove_file(&staging);
        return Err(format!("cannot stage the Generation selector: {error}"));
    }
    if let Err(error) = during_selection() {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    let selected = out_dir.join(SELECTED_GENERATION);
    match fs::rename(&staging, selected) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&staging);
            Err(format!(
                "cannot atomically select the complete Generation: {error}"
            ))
        }
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn invalidate_selection(out_dir: &Path) -> Result<(), String> {
    let selected = out_dir.join(SELECTED_GENERATION);
    match fs::remove_file(&selected) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            let detail = format!("cannot invalidate the previously selected Generation: {error}");
            let poison = "// recursa-generation-invalid\ncompile_error!(\"previous Recursa Generation selection could not be invalidated\");\n";
            let poisoned = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&selected)
                .and_then(|mut selector| {
                    selector.write_all(poison.as_bytes())?;
                    selector.sync_all()
                });
            match poisoned {
                Ok(()) => Err(detail),
                Err(poison_error) => Err(format!(
                    "{detail}; cannot poison the stale Generation selector: {poison_error}"
                )),
            }
        }
    }
}

#[cfg(test)]
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn selected_generation(out_dir: &Path) -> Option<String> {
    let selector = fs::read_to_string(out_dir.join(SELECTED_GENERATION)).ok()?;
    selector
        .lines()
        .next()?
        .strip_prefix("// recursa-generation:")
        .map(str::to_owned)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn staging_nonce() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock cannot name staging output: {error}"))
        .map(|duration| duration.as_nanos())
}
