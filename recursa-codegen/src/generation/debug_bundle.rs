//! Optional generated-code debug bundle staging and rendering.
//!
//! Debug bundles are non-authoritative: this boundary stages complete logical files,
//! indexes them deterministically, and commits only after generation publication succeeds.

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
    focused_case!(debug_directory_environment_is_always_tracked_by_cargo);
    focused_case!(debug_bundle_is_deterministic_formatted_complete_and_identity_neutral);
    #[cfg(all(feature = "diagrams", feature = "explorer"))]
    focused_case!(debug_bundle_includes_every_enabled_diagram_and_explorer_capability_body);
    focused_case!(failed_debug_commit_is_cleaned_and_makes_the_generation_unselectable);
    focused_case!(debug_destination_and_bundle_entries_reject_symlinks_without_following_them);
}

/// Opt-in destination for a non-authoritative generated-code debug bundle.
pub(super) const DEBUG_DIRECTORY_ENVIRONMENT: &str = "RECURSA_DEBUG_DIR";
/// Stable index header and private directory name used by debug bundles.
pub(super) const DEBUG_BUNDLE_FORMAT: &str = "recursa-generated-code-debug-v1";
/// Canonical DEBUG BUNDLE OWNERSHIP MARKER value for this boundary.
const DEBUG_BUNDLE_OWNERSHIP_MARKER: &str = ".recursa-debug-bundle";

#[derive(Clone, Debug, Eq, PartialEq)]
/// One logical debug artifact and its deterministic contents.
pub(super) struct DebugLogicalFile {
    /// Logical artifact role written into the bundle index.
    role: &'static str,
    /// Generation-relative filename shown to maintainers.
    logical_name: String,
    /// Collision-free filename used inside the debug bundle directory.
    private_name: String,
    /// Formatted, validated Rust or metadata bytes represented as text.
    contents: String,
}

/// Isolated candidate bundle awaiting atomic publication.
pub(super) struct StagedDebugBundle {
    /// Unpublished directory removed on drop unless commit succeeds.
    pub(super) staging: Option<PathBuf>,
    /// Final opt-in debug destination.
    pub(super) published: PathBuf,
    /// Previous complete bundle temporarily retained for rollback.
    pub(super) replacement: Option<PathBuf>,
}

impl StagedDebugBundle {
    /// Publishes the staged bundle through the supplied atomic writer.
    pub(super) fn commit(mut self) -> Result<(), String> {
        let Some(staging) = self.staging.as_ref() else {
            return Ok(());
        };
        if let Some(replacement) = &self.replacement {
            fs::rename(&self.published, replacement).map_err(|error| {
                format!("cannot stage replacement of the previous debug bundle: {error}")
            })?;
        }
        match fs::rename(staging, &self.published) {
            Ok(()) => {
                self.staging = None;
                if let Some(replacement) = self.replacement.take() {
                    let _ = remove_debug_staging(&replacement);
                }
                Ok(())
            }
            Err(error) => {
                let detail =
                    format!("cannot atomically publish the generated-code debug bundle: {error}");
                if let Some(replacement) = self.replacement.take()
                    && let Err(rollback) = fs::rename(&replacement, &self.published)
                {
                    return Err(format!(
                        "{detail}; cannot restore the previous complete debug bundle: {rollback}"
                    ));
                }
                Err(detail)
            }
        }
    }
}

impl Drop for StagedDebugBundle {
    fn drop(&mut self) {
        if let Some(staging) = self.staging.take() {
            let _ = remove_debug_staging(&staging);
        }
    }
}

/// Publishes authoritative output before committing its optional non-authoritative debug bundle.
pub(super) fn publish_generation_and_debug(
    out_dir: &Path,
    identity: &str,
    rendered: &RenderedGeneration,
    debug_bundle: Option<StagedDebugBundle>,
) -> Result<(), String> {
    publish(out_dir, identity, rendered)?;
    if let Err(detail) = debug_bundle.map(StagedDebugBundle::commit).transpose() {
        return Err(match invalidate_selection(out_dir) {
            Ok(()) => detail,
            Err(invalidation) => format!(
                "{detail}; the failed debug emission also could not invalidate the Generation selector: {invalidation}"
            ),
        });
    }
    Ok(())
}

/// Materializes a complete debug bundle under a private staging directory.
pub(super) fn stage_debug_bundle(
    directory: &Path,
    plan: &EmissionPlan,
    rendered: &RenderedGeneration,
) -> Result<StagedDebugBundle, String> {
    if directory.as_os_str().is_empty() {
        return Err(format!(
            "{DEBUG_DIRECTORY_ENVIRONMENT} names an empty debug destination"
        ));
    }
    ensure_plain_directory(directory, "generated-code debug destination")?;
    let bundles = directory.join(format!(".{DEBUG_BUNDLE_FORMAT}"));
    ensure_plain_directory(&bundles, "generated-code debug bundle namespace")?;

    let files = render_debug_logical_files(plan, rendered)?;
    let index = render_debug_index(&plan.identity, &files);
    let published = bundles.join(&plan.identity);
    let replacing = published.try_exists().map_err(|error| {
        format!("cannot inspect the generated-code debug bundle destination: {error}")
    })?;
    if replacing {
        ensure_owned_debug_bundle(&published)?;
    }

    let nonce = staging_nonce()?;
    let staging = bundles.join(format!(
        ".staging-{}-{nonce}-{}",
        std::process::id(),
        plan.identity
    ));
    fs::create_dir(&staging)
        .map_err(|error| format!("cannot create debug bundle staging directory: {error}"))?;
    let result = (|| {
        write_debug_file(
            &staging.join(DEBUG_BUNDLE_OWNERSHIP_MARKER),
            DEBUG_BUNDLE_FORMAT.as_bytes(),
        )?;
        write_debug_file(&staging.join("index"), index.as_bytes())?;
        for file in &files {
            write_debug_file(&staging.join(&file.private_name), file.contents.as_bytes())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = remove_debug_staging(&staging);
        return Err(error);
    }
    Ok(StagedDebugBundle {
        staging: Some(staging),
        published,
        replacement: replacing.then(|| {
            bundles.join(format!(
                ".replaced-{}-{nonce}-{}",
                std::process::id(),
                plan.identity
            ))
        }),
    })
}

/// Verifies that an existing destination is a Recursa-owned plain directory.
pub(super) fn ensure_owned_debug_bundle(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect the existing debug bundle: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("the existing debug bundle is not a plain directory".to_owned());
    }
    let marker =
        fs::symlink_metadata(path.join(DEBUG_BUNDLE_OWNERSHIP_MARKER)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "the debug bundle destination collides with a directory not owned by Recursa"
                    .to_owned()
            } else {
                format!("cannot inspect debug bundle ownership metadata: {error}")
            }
        })?;
    if marker.file_type().is_symlink() || !marker.is_file() {
        return Err("the debug bundle ownership marker is not a plain file".to_owned());
    }
    Ok(())
}

/// Creates or validates a non-symlink directory used by debug publication.
pub(super) fn ensure_plain_directory(path: &Path, role: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("the {role} may not be a symbolic link"))
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!("the {role} is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)
                .map_err(|error| format!("cannot create the {role}: {error}"))?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|error| format!("cannot inspect the created {role}: {error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!("the created {role} is not a plain directory"));
            }
            Ok(())
        }
        Err(error) => Err(format!("cannot inspect the {role}: {error}")),
    }
}

/// Removes a staging entry without following a symbolic link.
pub(super) fn remove_debug_staging(staging: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(staging) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(staging),
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(staging),
        Ok(_) => fs::remove_file(staging),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Creates one new debug file and synchronizes its contents before publication.
pub(super) fn write_debug_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create debug bundle file: {error}"))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write debug bundle file: {error}"))
}

/// Collects root, Node, selector, and capability views into deterministic debug-file order.
pub(super) fn render_debug_logical_files(
    plan: &EmissionPlan,
    rendered: &RenderedGeneration,
) -> Result<Vec<DebugLogicalFile>, String> {
    let mut files = Vec::new();
    files.push(DebugLogicalFile {
        role: "root",
        logical_name: plan.grammar_name.clone(),
        private_name: "root.rs".to_owned(),
        contents: format_debug_rust("root", &rendered.root)?,
    });
    for (index, ((_, contents), node)) in rendered.nodes.iter().zip(&plan.nodes).enumerate() {
        files.push(DebugLogicalFile {
            role: "node",
            logical_name: node.semantic_key.clone(),
            private_name: format!("node-{index:04}.rs"),
            contents: format_debug_rust("Node", contents)?,
        });
    }

    let mut capabilities = render_debug_capability_fragments(plan)?;
    for (index, (logical_name, contents)) in capabilities.drain(..).enumerate() {
        files.push(DebugLogicalFile {
            role: "capability",
            logical_name,
            private_name: format!("capability-{index:04}.rs"),
            contents: format_debug_rust("capability", &contents)?,
        });
    }
    Ok(files)
}

/// Renders enabled capability fragments solely for maintainer inspection.
///
/// # Long-function contract
///
/// Rationale: assembles the optional debug view without making it authoritative generation output.
/// Invariants: entries are deterministic, complete for enabled capabilities, and identity-neutral.
/// Phases: collects capability fragments, formats Rust, then assigns stable logical names.
pub(super) fn render_debug_capability_fragments(
    plan: &EmissionPlan,
) -> Result<Vec<(String, String)>, String> {
    let mut fragments = Vec::new();
    for capability in &plan.capabilities {
        fragments.push((
            format!(
                "facade:core,diagrams={},explorer={},arbitrary={},serde={}",
                capability.diagrams, capability.explorer, capability.arbitrary, capability.serde
            ),
            render_facade_capability_fragment(plan, *capability).to_string(),
        ));
    }
    for (index, node) in plan.nodes.iter().enumerate() {
        let mut emitted_serde = false;
        for recipe in &node.configured_traits {
            let (name, body) = match recipe {
                ConfiguredTraitRecipe::Debug
                | ConfiguredTraitRecipe::Clone
                | ConfiguredTraitRecipe::Copy
                | ConfiguredTraitRecipe::PartialEq
                | ConfiguredTraitRecipe::Eq
                | ConfiguredTraitRecipe::PartialOrd
                | ConfiguredTraitRecipe::Ord
                | ConfiguredTraitRecipe::Hash => (
                    format!("configured:{}:{recipe:?}", node.semantic_key),
                    render_node_standard_trait(plan, node, *recipe),
                ),
                ConfiguredTraitRecipe::Serialize | ConfiguredTraitRecipe::Deserialize => {
                    if emitted_serde {
                        continue;
                    }
                    emitted_serde = true;
                    (
                        format!("configured:{}:Serde", node.semantic_key),
                        render_node_serde(plan, node),
                    )
                }
                ConfiguredTraitRecipe::Arbitrary => (
                    format!("configured:{}:Arbitrary", node.semantic_key),
                    render_node_arbitrary(plan, node),
                ),
                ConfiguredTraitRecipe::Pretty => (
                    format!("configured:{}:Pretty", node.semantic_key),
                    render_node_pretty(plan, node, index),
                ),
                ConfiguredTraitRecipe::Visit => (
                    format!("configured:{}:Visit", node.semantic_key),
                    render_node_visit(plan, node, false),
                ),
                ConfiguredTraitRecipe::VisitMut => (
                    format!("configured:{}:VisitMut", node.semantic_key),
                    render_node_visit(plan, node, true),
                ),
            };
            fragments.push((name, body.to_string()));
        }
    }
    let intrinsic_token_traits = INTRINSIC_CONTENT_TOKEN_VALUE_TRAITS
        .iter()
        .map(|recipe| format!("{recipe:?}"))
        .collect::<Vec<_>>()
        .join("+");
    for (index, token) in plan.tokens.iter().enumerate() {
        let rendered = render_token(plan, token, index).to_string();
        fragments.push((
            format!("intrinsic-token:{}:{intrinsic_token_traits}", token.ident),
            rendered.clone(),
        ));
        for recipe in &token.configured_traits {
            fragments.push((
                format!("configured-token:{}:{recipe:?}", token.ident),
                rendered.clone(),
            ));
        }
        for recipe in PRIVATE_CONTENT_TOKEN_SUPPORT_TRAITS {
            if content_token_support_required(plan, token.semantic, recipe) {
                fragments.push((
                    format!("support-token:{}:{recipe:?}", token.ident),
                    rendered.clone(),
                ));
            }
        }
    }
    #[cfg(feature = "diagrams")]
    if let Some(diagrams) = &plan.diagrams {
        let documentation = syn::LitStr::new(
            &render_aggregate_diagram_document(diagrams),
            proc_macro2::Span::call_site(),
        );
        fragments.push((
            "diagrams:aggregate".to_owned(),
            quote!(#[doc = #documentation] pub mod grammar_docs {}).to_string(),
        ));
        for node in &diagrams.nodes {
            let documentation = syn::LitStr::new(
                &render_node_diagram_document(node, diagrams.anchor_depth),
                proc_macro2::Span::call_site(),
            );
            fragments.push((
                format!("diagrams:{}", node.semantic_key),
                quote!(#[doc = #documentation] mod node_diagram {}).to_string(),
            ));
        }
        for token in &diagrams.tokens {
            let documentation = syn::LitStr::new(
                &render_token_diagram_document(token, diagrams.anchor_depth),
                proc_macro2::Span::call_site(),
            );
            fragments.push((
                format!("diagrams:token:{}", token.semantic_key),
                quote!(#[doc = #documentation] mod token_diagram {}).to_string(),
            ));
        }
    }
    #[cfg(feature = "explorer")]
    if let Some(explorer) = &plan.explorer {
        fragments.push((
            "explorer:static-schema".to_owned(),
            render_explorer_static_schema(plan, explorer).to_string(),
        ));
        fragments.push((
            "explorer:lexer".to_owned(),
            render_explorer_lex(plan).to_string(),
        ));
        for token in &plan.tokens {
            fragments.push((
                format!("explorer:token:{}", token.kind_variant),
                render_token_explorer(plan, token).to_string(),
            ));
        }
        for node in &plan.nodes {
            for application in &node.parse_applications {
                fragments.push((
                    format!(
                        "explorer:node:{}:application-{}",
                        node.semantic_key,
                        application.application.index()
                    ),
                    render_node_application_explorer(plan, explorer, node, application).to_string(),
                ));
            }
        }
    }
    Ok(fragments)
}

/// Renders the facade handshake arm for one accepted capability combination.
pub(super) fn render_facade_capability_fragment(
    plan: &EmissionPlan,
    capability: FacadeCapability,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let release_text = &plan.private_abi.release;
    let release = format!("release_{release_text}").replace(['.', '-'], "_");
    let release = format_ident!("{release}");
    let protocol_number = plan.private_abi.protocol;
    let protocol = format_ident!("protocol_{protocol_number}");
    let diagrams = u8::from(capability.diagrams);
    let explorer = u8::from(capability.explorer);
    let arbitrary = u8::from(capability.arbitrary);
    let serde = u8::from(capability.serde);
    let capability = format_ident!("cap_core_d{diagrams}_e{explorer}_a{arbitrary}_s{serde}");
    quote! {
        macro_rules! __recursa_debug_capability {
            (#release, #protocol, #capability) => {
                const _: #facade::__private::FacadeCompatibility<
                    { #facade::__private::release_stamp(#release_text) },
                    #protocol_number,
                > = #facade::__private::FACADE_COMPATIBILITY;
            };
        }
    }
}

/// Parses and pretty-prints one logical Rust artifact for the debug bundle.
pub(super) fn format_debug_rust(role: &str, source: &str) -> Result<String, String> {
    let syntax = syn::parse_file(source)
        .map_err(|error| format!("the logical generated {role} is invalid Rust: {error}"))?;
    let formatted = prettyplease::unparse(&syntax);
    syn::parse_file(&formatted)
        .map_err(|error| format!("the formatted generated {role} is invalid Rust: {error}"))?;
    Ok(formatted)
}

/// Serializes the bundle format, Generation identity, and logical-to-private filename map.
pub(super) fn render_debug_index(identity: &str, files: &[DebugLogicalFile]) -> String {
    let mut index = format!("{DEBUG_BUNDLE_FORMAT}\ngeneration\t{identity}\n");
    for file in files {
        index.push_str(file.role);
        index.push('\t');
        index.push_str(&file.logical_name);
        index.push('\t');
        index.push_str(&file.private_name);
        index.push('\n');
    }
    index
}
