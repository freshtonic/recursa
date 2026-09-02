//! Atomic generation publication and selection.
//!
//! This module owns staging, atomic rename, selection invalidation, and failure
//! checkpoints so rendering code cannot accidentally authorize partial output.

use super::*;

/// Generation-relative directory holding the encoded diagram plan and doc-time SVG files.
const DIAGRAM_DIRECTORY: &str = "diagrams";
/// File name of the encoded diagram plan inside [`DIAGRAM_DIRECTORY`].
const PLAN_FILE_NAME: &str = "diagrams.plan";

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
    focused_case!(same_identity_cannot_reuse_a_different_generated_payload);
    focused_case!(every_failed_publication_checkpoint_invalidates_stale_selection_and_recovers);
    focused_case!(filesystem_invalidation_failure_poisons_stale_selection_and_recovers);
    focused_case!(filesystem_rename_failures_leave_no_loadable_or_staged_generation);
}

/// Loader file that atomically selects one complete generated payload.
pub(super) const SELECTED_GENERATION: &str = "recursa-selected.rs";

/// Publishes one complete rendered generation and atomically selects it for loaders.
pub(super) fn publish(
    out_dir: &Path,
    identity: &str,
    rendered: &RenderedGeneration,
) -> Result<(), String> {
    publish_with_checkpoint(out_dir, identity, rendered, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Observable checkpoints used to verify rollback at each publication barrier.
pub(super) enum PublicationCheckpoint {
    /// Fails before any candidate output is staged.
    BeforeStaging,
    /// Fails while writing the isolated candidate directory.
    DuringStaging,
    /// Fails after staging but before the candidate becomes selectable.
    BeforeSelection,
    /// Fails while atomically selecting the staged candidate.
    DuringSelection,
}

/// Stages and publishes a generation while exposing deterministic failure checkpoints.
///
/// Existing complete generations are immutable and reusable. New payloads remain under a
/// private staging name until every root, Node, and loader-map file exists; any failure removes
/// staging and leaves no selector that can load partial output.
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
        validate_reusable_generation(&published, rendered)?;
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

/// Writes every logical file into staging and crosses the publication barrier with one rename.
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
    let loader_map = render_loader_map(rendered);
    fs::write(staging.join("loader-map"), loader_map)
        .map_err(|error| format!("cannot stage the Node loader map: {error}"))?;
    if let Some(diagram_plan) = &rendered.diagram_plan {
        let directory = staging.join(DIAGRAM_DIRECTORY);
        fs::create_dir(&directory)
            .map_err(|error| format!("cannot stage the diagram directory: {error}"))?;
        fs::write(directory.join(PLAN_FILE_NAME), diagram_plan)
            .map_err(|error| format!("cannot stage the diagram plan: {error}"))?;
    }
    fs::rename(staging, published)
        .map_err(|error| format!("cannot cross the Generation publication barrier: {error}"))?;
    Ok(())
}

/// Confirms that an immutable payload with the same identity has exactly the rendered content.
fn validate_reusable_generation(
    published: &Path,
    rendered: &RenderedGeneration,
) -> Result<(), String> {
    let loader_map = render_loader_map(rendered);
    let diagram_plan = format!("{DIAGRAM_DIRECTORY}/{PLAN_FILE_NAME}");
    let expected_files = std::iter::once(("root.rs", rendered.root.as_str()))
        .chain(std::iter::once(("loader-map", loader_map.as_str())))
        .chain(
            rendered
                .nodes
                .iter()
                .map(|(name, source)| (name.as_str(), source.as_str())),
        )
        .chain(
            rendered
                .diagram_plan
                .iter()
                .map(|plan| (diagram_plan.as_str(), plan.as_str())),
        );
    for (name, expected) in expected_files {
        let path = published.join(name);
        let actual = fs::read_to_string(&path)
            .map_err(|_| "an existing published Generation is incomplete".to_owned())?;
        if actual != expected {
            return Err(format!(
                "Generation identity collision: generated payload `{name}` differs from the existing immutable payload"
            ));
        }
    }
    Ok(())
}

/// Renders the loader map shared by staging and immutable-payload validation.
fn render_loader_map(rendered: &RenderedGeneration) -> String {
    rendered
        .node_anchors
        .iter()
        .zip(&rendered.nodes)
        .map(|(anchor, (name, _))| format!("{anchor}\t{name}\n"))
        .collect()
}

/// Atomically replaces the structural-loader selector after its target is fully published.
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

/// Removes the previous selector, poisoning it on removal failure to prevent stale loading.
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
/// Reads the identity recorded by the atomic selector, if a complete Generation is selected.
pub(super) fn selected_generation(out_dir: &Path) -> Option<String> {
    let selector = fs::read_to_string(out_dir.join(SELECTED_GENERATION)).ok()?;
    selector
        .lines()
        .next()?
        .strip_prefix("// recursa-generation:")
        .map(str::to_owned)
}

/// Returns a process-local staging suffix derived from the current system-clock instant.
pub(super) fn staging_nonce() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock cannot name staging output: {error}"))
        .map(|duration| duration.as_nanos())
}
