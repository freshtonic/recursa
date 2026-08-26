#![allow(
    dead_code,
    reason = "private M3 discovery seam is consumed by later lowering and generation tickets"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use proc_macro2::Span;
use syn::ext::IdentExt as _;
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::{Attribute, Expr, Item, ItemMod, Lit, Meta, Token};

use crate::SourceId;
use crate::lowering::{BuildInputs, DiagnosticStage, ModuleId, RustEdition};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CargoCfgSnapshot {
    flags: BTreeSet<String>,
    values: BTreeMap<String, BTreeSet<String>>,
}

impl CargoCfgSnapshot {
    pub(crate) fn with_flag(mut self, name: impl Into<String>) -> Self {
        self.flags.insert(name.into());
        self
    }

    pub(crate) fn with_value(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.values
            .entry(name.into())
            .or_default()
            .insert(value.into());
        self
    }

    pub(crate) fn from_cargo_environment(
        variables: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let mut snapshot = Self::default();
        for (name, value) in variables {
            if let Some(cfg_name) = name.strip_prefix("CARGO_CFG_") {
                let cfg_name = cfg_name.to_ascii_lowercase();
                if value.is_empty() && !empty_value_cfg(&cfg_name) {
                    snapshot.flags.insert(cfg_name);
                } else {
                    for value in value
                        .split(',')
                        .filter(|value| !value.is_empty() || empty_value_cfg(&cfg_name))
                    {
                        snapshot
                            .values
                            .entry(cfg_name.clone())
                            .or_default()
                            .insert(value.to_owned());
                    }
                }
            }
        }
        snapshot
    }

    fn canonical_facts(&self) -> Vec<String> {
        let mut facts = self.flags.iter().cloned().collect::<Vec<_>>();
        for (name, values) in &self.values {
            facts.extend(values.iter().map(|value| format!("{name}=\"{value}\"")));
        }
        facts.sort();
        facts
    }

    fn flag(&self, name: &str) -> Result<bool, CfgFailureKind> {
        if invocation_only_cfg(name) {
            return Err(CfgFailureKind::InvocationOnly);
        }
        if self.flags.contains(name) {
            Ok(true)
        } else if self.values.contains_key(name) || compiler_owned_cfg(name) {
            Ok(false)
        } else {
            Err(CfgFailureKind::Unknown)
        }
    }

    fn value(&self, name: &str, value: &str) -> Result<bool, CfgFailureKind> {
        if invocation_only_cfg(name) {
            return Err(CfgFailureKind::InvocationOnly);
        }
        if let Some(values) = self.values.get(name) {
            Ok(values.contains(value))
        } else if self.flags.contains(name) || compiler_owned_cfg(name) {
            Ok(false)
        } else {
            Err(CfgFailureKind::Unknown)
        }
    }
}

fn invocation_only_cfg(name: &str) -> bool {
    matches!(name, "test" | "doc" | "doctest")
}

fn empty_value_cfg(name: &str) -> bool {
    matches!(name, "target_abi" | "target_env")
}

fn compiler_owned_cfg(name: &str) -> bool {
    matches!(
        name,
        "contract_checks"
            | "debug_assertions"
            | "feature"
            | "miri"
            | "overflow_checks"
            | "panic"
            | "proc_macro"
            | "relocation_model"
            | "sanitize"
            | "target_abi"
            | "target_arch"
            | "target_endian"
            | "target_env"
            | "target_family"
            | "target_feature"
            | "target_has_atomic"
            | "target_has_atomic_equal_alignment"
            | "target_has_atomic_load_store"
            | "target_os"
            | "target_pointer_width"
            | "target_thread_local"
            | "target_vendor"
            | "ub_checks"
            | "unix"
            | "windows"
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiscoveryInput<'input> {
    pub(crate) manifest_dir: &'input Path,
    pub(crate) selected_root: &'input Path,
    pub(crate) edition: RustEdition,
    pub(crate) cfg: &'input CargoCfgSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryReport {
    pub(crate) build_inputs: BuildInputs,
    pub(crate) diagnostics: Vec<DiscoveryDiagnostic>,
    pub(crate) tree: Option<DiscoveredModuleTree>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DiscoveryDiagnosticKind {
    InvalidManifest,
    AbsolutePath,
    PackageEscape,
    SymlinkPath,
    MissingRoot,
    UnreadableSource,
    RustSyntax,
    MissingModule,
    AmbiguousModule,
    InvalidModulePath,
    MultipleModulePaths,
    AncestorCycle,
    UnknownCfg,
    InvocationOnlyCfg,
    MalformedCfg,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryDiagnostic {
    pub(crate) kind: DiscoveryDiagnosticKind,
    pub(crate) stage: DiagnosticStage,
    pub(crate) path: PathBuf,
    pub(crate) range: Option<Range<u32>>,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveredModuleTree {
    pub(crate) files: Vec<DiscoveredSourceFile>,
    pub(crate) inclusions: Vec<ModuleInclusion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveredSourceFile {
    pub(crate) id: SourceId,
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModuleSourceKind {
    Root,
    Inline,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModuleInclusion {
    pub(crate) id: ModuleId,
    pub(crate) parent: Option<ModuleId>,
    pub(crate) logical_path: Vec<String>,
    pub(crate) source: SourceId,
    pub(crate) source_kind: ModuleSourceKind,
    pub(crate) declaration_range: Range<u32>,
    pub(crate) ownership: DirectoryOwnership,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectoryOwnership {
    pub(crate) directory: PathBuf,
    pub(crate) relative_stem: Option<String>,
}

impl DirectoryOwnership {
    fn default_base(&self) -> PathBuf {
        self.relative_stem.as_ref().map_or_else(
            || self.directory.clone(),
            |relative| self.directory.join(relative),
        )
    }
}

pub(crate) fn discover_module_tree(input: DiscoveryInput<'_>) -> DiscoveryReport {
    let selected_root = input.selected_root.to_owned();
    let mut state = DiscoveryState {
        manifest_dir: PathBuf::new(),
        cfg: input.cfg,
        build_inputs: BuildInputs {
            selected_root,
            source_files: Vec::new(),
            edition: input.edition,
            watched_module_candidates: Vec::new(),
            cfg_snapshot: input.cfg.canonical_facts(),
            environment_dependencies: Vec::new(),
        },
        diagnostics: Vec::new(),
        files: Vec::new(),
        file_by_path: BTreeMap::new(),
        inclusions: Vec::new(),
        candidates: BTreeSet::new(),
    };

    let manifest_dir = match fs::canonicalize(input.manifest_dir) {
        Ok(path) if path.is_dir() => path,
        Ok(_) => {
            state.push_diagnostic(DiscoveryDiagnostic {
                kind: DiscoveryDiagnosticKind::InvalidManifest,
                stage: DiagnosticStage::Discovery,
                path: input.manifest_dir.to_owned(),
                range: None,
                message: "Cargo manifest directory is not a directory".to_owned(),
            });
            return state.finish();
        }
        Err(error) => {
            state.push_diagnostic(DiscoveryDiagnostic {
                kind: DiscoveryDiagnosticKind::InvalidManifest,
                stage: DiagnosticStage::Discovery,
                path: input.manifest_dir.to_owned(),
                range: None,
                message: format!("cannot resolve Cargo manifest directory: {error}"),
            });
            return state.finish();
        }
    };
    state.manifest_dir = manifest_dir;

    let Some(root_path) = state.resolve_authored_path(
        Path::new(""),
        input.selected_root,
        input.selected_root,
        None,
        true,
    ) else {
        return state.finish();
    };
    state.build_inputs.selected_root = root_path.relative.clone();
    if !root_path.absolute.exists() {
        state.push_diagnostic(DiscoveryDiagnostic {
            kind: DiscoveryDiagnosticKind::MissingRoot,
            stage: DiagnosticStage::Discovery,
            path: root_path.relative,
            range: None,
            message: "selected grammar root does not exist".to_owned(),
        });
        return state.finish();
    }
    let Some((root_source, root_items, root_length)) =
        state.load_source(&root_path, DiscoveryDiagnosticKind::MissingRoot, None)
    else {
        return state.finish();
    };
    let root_ownership = DirectoryOwnership {
        directory: root_path
            .relative
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_owned(),
        relative_stem: None,
    };
    let root_module = state.push_inclusion(ModuleInclusion {
        id: ModuleId::next(state.inclusions.len()),
        parent: None,
        logical_path: Vec::new(),
        source: root_source,
        source_kind: ModuleSourceKind::Root,
        declaration_range: 0..root_length.try_into().unwrap_or(u32::MAX),
        ownership: root_ownership.clone(),
    });
    let mut ancestors = vec![root_path.relative];
    state.visit_items(
        &root_items,
        root_source,
        root_module,
        &[],
        &root_ownership,
        &mut ancestors,
    );
    state.finish()
}

struct DiscoveryState<'cfg> {
    manifest_dir: PathBuf,
    cfg: &'cfg CargoCfgSnapshot,
    build_inputs: BuildInputs,
    diagnostics: Vec<DiscoveryDiagnostic>,
    files: Vec<DiscoveredSourceFile>,
    file_by_path: BTreeMap<PathBuf, SourceId>,
    inclusions: Vec<ModuleInclusion>,
    candidates: BTreeSet<PathBuf>,
}

impl DiscoveryState<'_> {
    fn finish(mut self) -> DiscoveryReport {
        self.build_inputs.source_files = self.files.iter().map(|file| file.path.clone()).collect();
        self.build_inputs.watched_module_candidates = self.candidates.into_iter().collect();
        let tree = self.diagnostics.is_empty().then_some(DiscoveredModuleTree {
            files: self.files,
            inclusions: self.inclusions,
        });
        DiscoveryReport {
            build_inputs: self.build_inputs,
            diagnostics: self.diagnostics,
            tree,
        }
    }

    fn push_diagnostic(&mut self, diagnostic: DiscoveryDiagnostic) {
        if !self.diagnostics.contains(&diagnostic) {
            self.diagnostics.push(diagnostic);
        }
    }

    fn push_inclusion(&mut self, inclusion: ModuleInclusion) -> ModuleId {
        let id = inclusion.id;
        self.inclusions.push(inclusion);
        id
    }

    fn visit_items(
        &mut self,
        items: &[Item],
        source: SourceId,
        parent: ModuleId,
        logical_parent: &[String],
        ownership: &DirectoryOwnership,
        ancestors: &mut Vec<PathBuf>,
    ) {
        for item in items {
            let Item::Mod(module) = item else {
                continue;
            };
            let source_path = self.source_path(source).to_owned();
            let Some(configured) = self.configure_module(module, source) else {
                continue;
            };
            if !configured.active {
                continue;
            }
            let module_name = module.ident.unraw().to_string();
            let mut logical_path = logical_parent.to_vec();
            logical_path.push(module_name.clone());

            if configured.paths.len() > 1 {
                if module.content.is_none() {
                    for path in &configured.paths {
                        if let Some(candidate) = self.resolve_authored_path(
                            &ownership.directory,
                            Path::new(&path.value),
                            &source_path,
                            Some(path.range.clone()),
                            false,
                        ) {
                            self.candidates.insert(candidate.relative);
                        }
                    }
                }
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: DiscoveryDiagnosticKind::MultipleModulePaths,
                    stage: DiagnosticStage::Discovery,
                    path: source_path.clone(),
                    range: Some(span_range(module.ident.span())),
                    message: "module has multiple active path attributes".to_owned(),
                });
                continue;
            }

            if let Some((_, inline_items)) = &module.content {
                let child_ownership = if let Some(path) = configured.paths.first() {
                    let Some(directory) = self.resolve_authored_path(
                        &ownership.directory,
                        Path::new(&path.value),
                        &source_path,
                        Some(path.range.clone()),
                        false,
                    ) else {
                        continue;
                    };
                    DirectoryOwnership {
                        directory: directory.relative,
                        relative_stem: None,
                    }
                } else {
                    DirectoryOwnership {
                        directory: ownership.default_base().join(module_name),
                        relative_stem: None,
                    }
                };
                let module_id = self.push_inclusion(ModuleInclusion {
                    id: ModuleId::next(self.inclusions.len()),
                    parent: Some(parent),
                    logical_path: logical_path.clone(),
                    source,
                    source_kind: ModuleSourceKind::Inline,
                    declaration_range: span_range(module.ident.span()),
                    ownership: child_ownership.clone(),
                });
                self.visit_items(
                    inline_items,
                    source,
                    module_id,
                    &logical_path,
                    &child_ownership,
                    ancestors,
                );
                continue;
            }

            let selected = if let Some(path) = configured.paths.first() {
                let Some(candidate) = self.resolve_authored_path(
                    &ownership.directory,
                    Path::new(&path.value),
                    &source_path,
                    Some(path.range.clone()),
                    false,
                ) else {
                    continue;
                };
                self.candidates.insert(candidate.relative.clone());
                if self.reject_symlink_candidate(
                    &candidate,
                    &source_path,
                    span_range(module.ident.span()),
                ) {
                    continue;
                }
                if candidate.absolute.exists() {
                    Some((candidate, None))
                } else {
                    self.push_diagnostic(DiscoveryDiagnostic {
                        kind: DiscoveryDiagnosticKind::MissingModule,
                        stage: DiagnosticStage::Discovery,
                        path: source_path.clone(),
                        range: Some(span_range(module.ident.span())),
                        message: "explicit module path does not exist".to_owned(),
                    });
                    None
                }
            } else {
                self.select_default_candidate(module, source, ownership)
            };
            let Some((selected, default_stem)) = selected else {
                continue;
            };
            if ancestors.contains(&selected.relative) {
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: DiscoveryDiagnosticKind::AncestorCycle,
                    stage: DiagnosticStage::Discovery,
                    path: source_path.clone(),
                    range: Some(span_range(module.ident.span())),
                    message: format!(
                        "module inclusion forms an ancestor cycle through {}",
                        selected.relative.display(),
                    ),
                });
                continue;
            }
            let Some((child_source, child_items, _)) = self.load_source(
                &selected,
                DiscoveryDiagnosticKind::MissingModule,
                Some((source_path, span_range(module.ident.span()))),
            ) else {
                continue;
            };
            let child_ownership = DirectoryOwnership {
                directory: selected
                    .relative
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_owned(),
                relative_stem: default_stem,
            };
            let module_id = self.push_inclusion(ModuleInclusion {
                id: ModuleId::next(self.inclusions.len()),
                parent: Some(parent),
                logical_path: logical_path.clone(),
                source: child_source,
                source_kind: ModuleSourceKind::External,
                declaration_range: span_range(module.ident.span()),
                ownership: child_ownership.clone(),
            });
            ancestors.push(selected.relative);
            self.visit_items(
                &child_items,
                child_source,
                module_id,
                &logical_path,
                &child_ownership,
                ancestors,
            );
            ancestors.pop();
        }
    }

    fn configure_module(&mut self, module: &ItemMod, source: SourceId) -> Option<ConfiguredModule> {
        let mut configured = ConfiguredModule {
            active: true,
            paths: Vec::new(),
        };
        let mut valid = true;
        for attribute in &module.attrs {
            if !matches_attribute(attribute, &["cfg", "cfg_attr", "path"]) {
                continue;
            }
            if let Err(failure) = apply_attribute(&attribute.meta, self.cfg, &mut configured) {
                valid = false;
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: failure.kind,
                    stage: DiagnosticStage::Discovery,
                    path: self.source_path(source).to_owned(),
                    range: Some(failure.range),
                    message: failure.message,
                });
            }
        }
        valid.then_some(configured)
    }

    fn select_default_candidate(
        &mut self,
        module: &ItemMod,
        source: SourceId,
        ownership: &DirectoryOwnership,
    ) -> Option<(ResolvedPath, Option<String>)> {
        let base = ownership.default_base();
        let name = module.ident.unraw().to_string();
        let flat_relative = base.join(format!("{name}.rs"));
        let nested_relative = base.join(&name).join("mod.rs");
        let flat = self.resolve_generated_path(&flat_relative)?;
        let nested = self.resolve_generated_path(&nested_relative)?;
        self.candidates.insert(flat.relative.clone());
        self.candidates.insert(nested.relative.clone());
        let declaration_range = span_range(module.ident.span());
        let diagnostic_path = self.source_path(source).to_owned();
        if self.reject_symlink_candidate(&flat, &diagnostic_path, declaration_range.clone())
            | self.reject_symlink_candidate(&nested, &diagnostic_path, declaration_range)
        {
            return None;
        }
        match (flat.absolute.exists(), nested.absolute.exists()) {
            (true, false) => Some((flat, Some(name))),
            (false, true) => Some((nested, None)),
            (false, false) => {
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: DiscoveryDiagnosticKind::MissingModule,
                    stage: DiagnosticStage::Discovery,
                    path: self.source_path(source).to_owned(),
                    range: Some(span_range(module.ident.span())),
                    message: "neither default module candidate exists".to_owned(),
                });
                None
            }
            (true, true) => {
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: DiscoveryDiagnosticKind::AmbiguousModule,
                    stage: DiagnosticStage::Discovery,
                    path: self.source_path(source).to_owned(),
                    range: Some(span_range(module.ident.span())),
                    message: "both default module candidates exist".to_owned(),
                });
                None
            }
        }
    }

    fn load_source(
        &mut self,
        path: &ResolvedPath,
        missing_kind: DiscoveryDiagnosticKind,
        declaration: Option<(PathBuf, Range<u32>)>,
    ) -> Option<(SourceId, Vec<Item>, usize)> {
        if let Some(component) = first_symlink_component(&self.manifest_dir, &path.relative) {
            let (diagnostic_path, range) = declaration.clone().map_or_else(
                || (path.relative.clone(), None),
                |(path, range)| (path, Some(range)),
            );
            self.push_diagnostic(DiscoveryDiagnostic {
                kind: DiscoveryDiagnosticKind::SymlinkPath,
                stage: DiagnosticStage::Discovery,
                path: diagnostic_path,
                range,
                message: format!(
                    "source path traverses unsupported symlink {}",
                    component.display(),
                ),
            });
            return None;
        }
        let text = match fs::read_to_string(&path.absolute) {
            Ok(text) => text,
            Err(error) => {
                let (diagnostic_path, range) = declaration.map_or_else(
                    || (path.relative.clone(), None),
                    |(path, range)| (path, Some(range)),
                );
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: if error.kind() == std::io::ErrorKind::NotFound {
                        missing_kind
                    } else {
                        DiscoveryDiagnosticKind::UnreadableSource
                    },
                    stage: DiagnosticStage::Discovery,
                    path: diagnostic_path,
                    range,
                    message: format!("cannot read module source: {error}"),
                });
                return None;
            }
        };
        let syntax = match syn::parse_file(&text) {
            Ok(syntax) => syntax,
            Err(error) => {
                let id = self.intern_file(&path.relative, text.clone());
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: DiscoveryDiagnosticKind::RustSyntax,
                    stage: DiagnosticStage::Syntax,
                    path: path.relative.clone(),
                    range: Some(span_range(error.span())),
                    message: error.to_string(),
                });
                let _ = id;
                return None;
            }
        };
        let length = text.len();
        let id = self.intern_file(&path.relative, text);
        Some((id, syntax.items, length))
    }

    fn reject_symlink_candidate(
        &mut self,
        candidate: &ResolvedPath,
        diagnostic_path: &Path,
        range: Range<u32>,
    ) -> bool {
        let Some(component) = first_symlink_component(&self.manifest_dir, &candidate.relative)
        else {
            return false;
        };
        self.push_diagnostic(DiscoveryDiagnostic {
            kind: DiscoveryDiagnosticKind::SymlinkPath,
            stage: DiagnosticStage::Discovery,
            path: diagnostic_path.to_owned(),
            range: Some(range),
            message: format!(
                "module candidate traverses unsupported symlink {}",
                component.display(),
            ),
        });
        true
    }

    fn intern_file(&mut self, relative: &Path, text: String) -> SourceId {
        if let Some(id) = self.file_by_path.get(relative) {
            return *id;
        }
        let id = SourceId::new(
            self.files
                .len()
                .try_into()
                .expect("discovered source ID overflow"),
        );
        self.file_by_path.insert(relative.to_owned(), id);
        self.files.push(DiscoveredSourceFile {
            id,
            path: relative.to_owned(),
            text,
        });
        id
    }

    fn source_path(&self, source: SourceId) -> &Path {
        &self.files[source.index()].path
    }

    fn resolve_generated_path(&mut self, relative: &Path) -> Option<ResolvedPath> {
        let relative = match normalize_relative(relative) {
            Ok(path) => path,
            Err(()) => {
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: DiscoveryDiagnosticKind::PackageEscape,
                    stage: DiagnosticStage::Discovery,
                    path: relative.to_owned(),
                    range: None,
                    message: "generated module candidate escapes the package".to_owned(),
                });
                return None;
            }
        };
        Some(ResolvedPath {
            absolute: self.manifest_dir.join(&relative),
            relative,
        })
    }

    fn resolve_authored_path(
        &mut self,
        base: &Path,
        authored: &Path,
        diagnostic_path: &Path,
        range: Option<Range<u32>>,
        root: bool,
    ) -> Option<ResolvedPath> {
        if authored.is_absolute() {
            self.push_diagnostic(DiscoveryDiagnostic {
                kind: DiscoveryDiagnosticKind::AbsolutePath,
                stage: DiagnosticStage::Discovery,
                path: diagnostic_path.to_owned(),
                range,
                message: if root {
                    "selected grammar root must be manifest-relative".to_owned()
                } else {
                    "module path must be package-relative".to_owned()
                },
            });
            return None;
        }
        let joined = base.join(authored);
        let relative = match normalize_relative(&joined) {
            Ok(path) => path,
            Err(()) => {
                self.push_diagnostic(DiscoveryDiagnostic {
                    kind: DiscoveryDiagnosticKind::PackageEscape,
                    stage: DiagnosticStage::Discovery,
                    path: diagnostic_path.to_owned(),
                    range,
                    message: "source path escapes the Cargo package".to_owned(),
                });
                return None;
            }
        };
        Some(ResolvedPath {
            absolute: self.manifest_dir.join(&relative),
            relative,
        })
    }
}

#[derive(Clone, Debug)]
struct ResolvedPath {
    absolute: PathBuf,
    relative: PathBuf,
}

#[derive(Clone, Debug)]
struct ConfiguredModule {
    active: bool,
    paths: Vec<ActivePath>,
}

#[derive(Clone, Debug)]
struct ActivePath {
    value: String,
    range: Range<u32>,
}

#[derive(Clone, Debug)]
struct CfgFailure {
    kind: DiscoveryDiagnosticKind,
    range: Range<u32>,
    message: String,
}

#[derive(Clone, Copy, Debug)]
enum CfgFailureKind {
    Unknown,
    InvocationOnly,
}

fn matches_attribute(attribute: &Attribute, names: &[&str]) -> bool {
    names.iter().any(|name| attribute.path().is_ident(name))
}

fn apply_attribute(
    meta: &Meta,
    snapshot: &CargoCfgSnapshot,
    configured: &mut ConfiguredModule,
) -> Result<(), CfgFailure> {
    if meta.path().is_ident("cfg") {
        let Meta::List(list) = meta else {
            return Err(malformed_cfg(meta.span(), "cfg must use parentheses"));
        };
        let predicate = syn::parse2::<Meta>(list.tokens.clone())
            .map_err(|error| malformed_cfg(error.span(), error.to_string()))?;
        configured.active &= evaluate_cfg(&predicate, snapshot)?;
        Ok(())
    } else if meta.path().is_ident("cfg_attr") {
        let Meta::List(list) = meta else {
            return Err(malformed_cfg(meta.span(), "cfg_attr must use parentheses"));
        };
        let nested = Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|error| malformed_cfg(error.span(), error.to_string()))?;
        let Some(predicate) = nested.first() else {
            return Err(malformed_cfg(meta.span(), "cfg_attr requires a predicate"));
        };
        if evaluate_cfg(predicate, snapshot)? {
            for attribute in nested.iter().skip(1) {
                apply_attribute(attribute, snapshot, configured)?;
            }
        }
        Ok(())
    } else if meta.path().is_ident("path") {
        let Meta::NameValue(name_value) = meta else {
            return Err(CfgFailure {
                kind: DiscoveryDiagnosticKind::InvalidModulePath,
                range: span_range(meta.span()),
                message: "path attribute must be path = \"...\"".to_owned(),
            });
        };
        let Expr::Lit(expression) = &name_value.value else {
            return Err(CfgFailure {
                kind: DiscoveryDiagnosticKind::InvalidModulePath,
                range: span_range(name_value.value.span()),
                message: "module path must be a string literal".to_owned(),
            });
        };
        let Lit::Str(path) = &expression.lit else {
            return Err(CfgFailure {
                kind: DiscoveryDiagnosticKind::InvalidModulePath,
                range: span_range(expression.lit.span()),
                message: "module path must be a string literal".to_owned(),
            });
        };
        configured.paths.push(ActivePath {
            value: path.value(),
            range: span_range(path.span()),
        });
        Ok(())
    } else {
        Ok(())
    }
}

fn evaluate_cfg(meta: &Meta, snapshot: &CargoCfgSnapshot) -> Result<bool, CfgFailure> {
    let name = meta
        .path()
        .get_ident()
        .ok_or_else(|| malformed_cfg(meta.span(), "cfg predicate must be an identifier"))?
        .to_string();
    match meta {
        Meta::Path(_) => snapshot
            .flag(&name)
            .map_err(|failure| cfg_lookup_failure(failure, meta.span(), &name)),
        Meta::NameValue(name_value) => {
            let Expr::Lit(expression) = &name_value.value else {
                return Err(malformed_cfg(
                    name_value.value.span(),
                    "cfg value must be a string literal",
                ));
            };
            let Lit::Str(value) = &expression.lit else {
                return Err(malformed_cfg(
                    expression.lit.span(),
                    "cfg value must be a string literal",
                ));
            };
            snapshot
                .value(&name, &value.value())
                .map_err(|failure| cfg_lookup_failure(failure, meta.span(), &name))
        }
        Meta::List(list) => {
            let nested = Punctuated::<Meta, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .map_err(|error| malformed_cfg(error.span(), error.to_string()))?;
            let values = nested
                .iter()
                .map(|predicate| evaluate_cfg(predicate, snapshot))
                .collect::<Result<Vec<_>, _>>()?;
            match name.as_str() {
                "all" => Ok(values.into_iter().all(|value| value)),
                "any" => Ok(values.into_iter().any(|value| value)),
                "not" if values.len() == 1 => Ok(!values[0]),
                "not" => Err(malformed_cfg(meta.span(), "not requires one predicate")),
                _ => Err(malformed_cfg(
                    meta.span(),
                    "cfg supports only all, any, and not predicate functions",
                )),
            }
        }
    }
}

fn cfg_lookup_failure(failure: CfgFailureKind, span: Span, name: &str) -> CfgFailure {
    match failure {
        CfgFailureKind::Unknown => CfgFailure {
            kind: DiscoveryDiagnosticKind::UnknownCfg,
            range: span_range(span),
            message: format!("cfg `{name}` is absent from Cargo's target snapshot"),
        },
        CfgFailureKind::InvocationOnly => CfgFailure {
            kind: DiscoveryDiagnosticKind::InvocationOnlyCfg,
            range: span_range(span),
            message: format!("cfg `{name}` is unavailable to build-script discovery"),
        },
    }
}

fn malformed_cfg(span: Span, message: impl Into<String>) -> CfgFailure {
    CfgFailure {
        kind: DiscoveryDiagnosticKind::MalformedCfg,
        range: span_range(span),
        message: message.into(),
    }
}

fn normalize_relative(path: &Path) -> Result<PathBuf, ()> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => normalized.push(component),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(());
                }
            }
            Component::RootDir | Component::Prefix(_) => return Err(()),
        }
    }
    Ok(normalized)
}

fn first_symlink_component(manifest_dir: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = manifest_dir.to_owned();
    let mut relative_current = PathBuf::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        current.push(component);
        relative_current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Some(relative_current),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => return None,
        }
    }
    None
}

fn span_range(span: Span) -> Range<u32> {
    let range = span.byte_range();
    range.start.try_into().unwrap_or(u32::MAX)..range.end.try_into().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::fs::OpenOptions;
    use std::io::Read as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::time::{Duration, SystemTime};

    use tempfile::TempDir;

    use super::{
        CargoCfgSnapshot, DiscoveryDiagnosticKind, DiscoveryInput, ModuleSourceKind,
        discover_module_tree,
    };
    use crate::lowering::RustEdition;

    fn write(package: &Path, relative: &str, source: &str) {
        let path = package.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file parent")).unwrap();
        fs::write(path, source).unwrap();
    }

    fn discover<'a>(
        package: &'a TempDir,
        root: &'a str,
        cfg: &'a CargoCfgSnapshot,
    ) -> super::DiscoveryReport {
        discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new(root),
            edition: RustEdition::Rust2024,
            cfg,
        })
    }

    fn scaffold_cargo_package(package: &Path, features: bool) {
        write(
            package,
            "Cargo.toml",
            if features {
                "[package]\nname = \"discovery-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[features]\nalternate = []\n\n[workspace]\n"
            } else {
                "[package]\nname = \"discovery-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[workspace]\n"
            },
        );
        write(package, "src/lib.rs", "pub fn fixture() {}\n");
        write(
            package,
            "Cargo.lock",
            "# This file is automatically @generated by Cargo.\nversion = 4\n\n[[package]]\nname = \"discovery-fixture\"\nversion = \"0.0.0\"\n",
        );
    }

    fn write_tracking_build_script(
        package: &Path,
        watched: &[PathBuf],
        required: &[PathBuf],
        candidate_pairs: &[(PathBuf, PathBuf)],
    ) {
        let watched = watched
            .iter()
            .map(|path| format!("{:?}", path.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(", ");
        let required = required
            .iter()
            .map(|path| format!("{:?}", path.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(", ");
        let candidate_pairs = candidate_pairs
            .iter()
            .map(|(left, right)| {
                format!(
                    "({:?}, {:?})",
                    left.to_string_lossy(),
                    right.to_string_lossy(),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        write(
            package,
            "build.rs",
            &format!(
                r#"
use std::fs::OpenOptions;
use std::io::Write as _;

fn main() {{
    let watched: &[&str] = &[{watched}];
    for path in watched {{
        println!("cargo::rerun-if-changed={{path}}");
    }}
    let log = std::env::var_os("REC_TEST_LOG").expect("test log");
    let required: &[&str] = &[{required}];
    let observed = required
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect::<Vec<_>>();
    writeln!(
        OpenOptions::new().create(true).append(true).open(log).unwrap(),
        "run {{observed:?}}",
    )
    .unwrap();
    for path in required {{
        assert!(std::path::Path::new(path).is_file(), "required input {{path}} is missing");
    }}
    let candidate_pairs: &[(&str, &str)] = &[{candidate_pairs}];
    for (left, right) in candidate_pairs {{
        assert!(
            !(std::path::Path::new(left).exists() && std::path::Path::new(right).exists()),
            "ambiguous candidates {{left}} and {{right}}",
        );
    }}
}}
"#,
            ),
        );
    }

    fn run_cargo(package: &Path, arguments: &[&str]) -> Output {
        let mut command = Command::new("cargo");
        command
            .arg("+1.88.0")
            .arg("check")
            .arg("--offline")
            .arg("--locked")
            .arg("-vv")
            .args(arguments)
            .current_dir(package)
            .env("CARGO_TARGET_DIR", package.join("target"))
            .env("CARGO_HOME", package.join("cargo-home"))
            .env("REC_TEST_LOG", package.join("runs.log"));
        command.output().expect("run Cargo 1.88 fixture")
    }

    fn run_count(package: &Path) -> usize {
        let mut contents = String::new();
        OpenOptions::new()
            .read(true)
            .open(package.join("runs.log"))
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        contents.lines().count()
    }

    fn last_run(package: &Path) -> String {
        fs::read_to_string(package.join("runs.log"))
            .unwrap()
            .lines()
            .last()
            .unwrap()
            .to_owned()
    }

    fn make_newer(path: &Path, offset_seconds: u64) {
        let file = OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(offset_seconds))
            .unwrap();
    }

    #[test]
    fn discovers_rust_ownership_and_distinct_logical_inclusions_from_the_manifest() {
        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/root.rs",
            r#"
mod ordinary;
mod nested_form;
mod inline { mod nested; }
#[path = "owned"] mod remapped { mod child; }
#[path = "shared.rs"] mod left;
#[path = "shared.rs"] mod right;
#[path = "explicit.rs"] mod explicit;
"#,
        );
        write(
            package.path(),
            "grammar/ordinary.rs",
            r#"
mod nested;
mod nested_inline { mod leaf; }
#[path = "explicit_nested.rs"] mod explicit_nested;
#[path = "inline_owned"] mod inline_remap { mod leaf; }
"#,
        );
        write(package.path(), "grammar/ordinary/nested.rs", "");
        write(package.path(), "grammar/ordinary/nested_inline/leaf.rs", "");
        write(package.path(), "grammar/explicit_nested.rs", "");
        write(package.path(), "grammar/inline_owned/leaf.rs", "");
        write(package.path(), "grammar/nested_form/mod.rs", "mod leaf;");
        write(package.path(), "grammar/nested_form/leaf.rs", "");
        write(package.path(), "grammar/inline/nested.rs", "");
        write(package.path(), "grammar/owned/child.rs", "");
        write(
            package.path(),
            "grammar/shared.rs",
            "#[path = \"shared_leaf.rs\"] mod leaf;",
        );
        write(package.path(), "grammar/shared_leaf.rs", "");
        write(package.path(), "grammar/explicit.rs", "mod leaf;");
        write(package.path(), "grammar/leaf.rs", "");

        let report = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());

        assert!(report.diagnostics.is_empty());
        assert_eq!(
            report,
            discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default()),
        );
        let tree = report.tree.expect("valid discovered tree");
        assert_eq!(
            tree.files
                .iter()
                .map(|file| file.path.as_path())
                .collect::<Vec<_>>(),
            [
                Path::new("grammar/root.rs"),
                Path::new("grammar/ordinary.rs"),
                Path::new("grammar/ordinary/nested.rs"),
                Path::new("grammar/ordinary/nested_inline/leaf.rs"),
                Path::new("grammar/explicit_nested.rs"),
                Path::new("grammar/inline_owned/leaf.rs"),
                Path::new("grammar/nested_form/mod.rs"),
                Path::new("grammar/nested_form/leaf.rs"),
                Path::new("grammar/inline/nested.rs"),
                Path::new("grammar/owned/child.rs"),
                Path::new("grammar/shared.rs"),
                Path::new("grammar/shared_leaf.rs"),
                Path::new("grammar/explicit.rs"),
                Path::new("grammar/leaf.rs"),
            ],
        );
        assert_eq!(
            tree.inclusions
                .iter()
                .map(|module| module.logical_path.join("::"))
                .collect::<Vec<_>>(),
            [
                "".to_owned(),
                "ordinary".to_owned(),
                "ordinary::nested".to_owned(),
                "ordinary::nested_inline".to_owned(),
                "ordinary::nested_inline::leaf".to_owned(),
                "ordinary::explicit_nested".to_owned(),
                "ordinary::inline_remap".to_owned(),
                "ordinary::inline_remap::leaf".to_owned(),
                "nested_form".to_owned(),
                "nested_form::leaf".to_owned(),
                "inline".to_owned(),
                "inline::nested".to_owned(),
                "remapped".to_owned(),
                "remapped::child".to_owned(),
                "left".to_owned(),
                "left::leaf".to_owned(),
                "right".to_owned(),
                "right::leaf".to_owned(),
                "explicit".to_owned(),
                "explicit::leaf".to_owned(),
            ],
        );
        assert_eq!(
            tree.inclusions
                .iter()
                .filter(|module| tree.files[module.source.index()].path
                    == Path::new("grammar/shared.rs"))
                .count(),
            2,
        );
        let inline = tree
            .inclusions
            .iter()
            .find(|module| module.logical_path == ["inline"])
            .unwrap();
        assert_eq!(inline.source_kind, ModuleSourceKind::Inline);
        assert_eq!(inline.ownership.directory, Path::new("grammar/inline"));
        let ordinary = tree
            .inclusions
            .iter()
            .find(|module| module.logical_path == ["ordinary"])
            .unwrap();
        assert_eq!(ordinary.ownership.directory, Path::new("grammar"));
        assert_eq!(
            ordinary.ownership.relative_stem.as_deref(),
            Some("ordinary")
        );
        let explicit = tree
            .inclusions
            .iter()
            .find(|module| module.logical_path == ["explicit"])
            .unwrap();
        assert_eq!(explicit.ownership.directory, Path::new("grammar"));
        assert_eq!(explicit.ownership.relative_stem, None);

        assert_eq!(
            report.build_inputs.selected_root,
            Path::new("grammar/root.rs")
        );
        assert!(
            report
                .build_inputs
                .watched_module_candidates
                .contains(&PathBuf::from("grammar/ordinary/mod.rs")),
        );
    }

    #[test]
    fn collects_missing_and_ambiguous_candidates_and_keeps_known_inputs() {
        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/root.rs",
            concat!(
                "mod ambiguous; mod missing; ",
                "#[path = \"explicit_missing.rs\"] mod explicit_missing;",
            ),
        );
        write(package.path(), "grammar/ambiguous.rs", "");
        write(package.path(), "grammar/ambiguous/mod.rs", "");

        let report = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());

        assert!(report.tree.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                DiscoveryDiagnosticKind::AmbiguousModule,
                DiscoveryDiagnosticKind::MissingModule,
                DiscoveryDiagnosticKind::MissingModule,
            ],
        );
        assert_eq!(
            report.build_inputs.watched_module_candidates,
            [
                PathBuf::from("grammar/ambiguous/mod.rs"),
                PathBuf::from("grammar/ambiguous.rs"),
                PathBuf::from("grammar/explicit_missing.rs"),
                PathBuf::from("grammar/missing/mod.rs"),
                PathBuf::from("grammar/missing.rs"),
            ],
        );
        assert_eq!(
            report.build_inputs.source_files,
            [PathBuf::from("grammar/root.rs")],
        );
        let root = fs::read_to_string(package.path().join("grammar/root.rs")).unwrap();
        for (diagnostic, expected) in
            report
                .diagnostics
                .iter()
                .zip(["ambiguous", "missing", "explicit_missing"])
        {
            let range = diagnostic.range.clone().expect("module identifier range");
            assert_eq!(&root[range.start as usize..range.end as usize], expected,);
        }
    }

    #[test]
    fn configures_modules_and_recursive_cfg_attr_paths_before_selection() {
        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/root.rs",
            r#"
#[cfg(unix)]
#[cfg_attr(unix, cfg_attr(feature = "alt", path = "alt.rs"))]
#[cfg_attr(not(feature = "alt"), path = "base.rs")]
mod selected;
#[cfg(windows)] mod inactive_missing;
"#,
        );
        write(package.path(), "grammar/base.rs", "");
        write(package.path(), "grammar/alt.rs", "");

        let base_cfg = CargoCfgSnapshot::default().with_flag("unix");
        let base = discover(&package, "grammar/root.rs", &base_cfg);
        assert!(base.diagnostics.is_empty());
        assert_eq!(
            base.build_inputs.watched_module_candidates,
            [PathBuf::from("grammar/base.rs")],
        );

        let alt_cfg = CargoCfgSnapshot::default()
            .with_flag("unix")
            .with_value("feature", "alt");
        let alt = discover(&package, "grammar/root.rs", &alt_cfg);
        assert!(alt.diagnostics.is_empty());
        assert_eq!(
            alt.build_inputs.watched_module_candidates,
            [PathBuf::from("grammar/alt.rs")],
        );
    }

    #[test]
    fn rejects_unknown_invocation_cfg_and_multiple_active_paths_without_guessing() {
        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/root.rs",
            r#"
#[cfg(custom_build_cfg)] mod unknown;
#[cfg(test)] mod invocation_only;
#[path = "a.rs"] #[cfg_attr(unix, path = "b.rs")] mod multiple;
"#,
        );
        write(package.path(), "grammar/a.rs", "");
        write(package.path(), "grammar/b.rs", "");

        let cfg = CargoCfgSnapshot::default().with_flag("unix");
        let report = discover(&package, "grammar/root.rs", &cfg);

        assert!(report.tree.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                DiscoveryDiagnosticKind::UnknownCfg,
                DiscoveryDiagnosticKind::InvocationOnlyCfg,
                DiscoveryDiagnosticKind::MultipleModulePaths,
            ],
        );
        assert_eq!(
            report.build_inputs.watched_module_candidates,
            [PathBuf::from("grammar/a.rs"), PathBuf::from("grammar/b.rs"),],
        );
    }

    #[test]
    fn distinguishes_sibling_reuse_from_an_ancestor_cycle() {
        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/root.rs",
            "#[path = \"a.rs\"] mod a;",
        );
        write(
            package.path(),
            "grammar/a.rs",
            "#[path = \"root.rs\"] mod back_to_root;",
        );

        let report = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());

        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].kind,
            DiscoveryDiagnosticKind::AncestorCycle,
        );
        assert_eq!(
            report.build_inputs.watched_module_candidates,
            [
                PathBuf::from("grammar/a.rs"),
                PathBuf::from("grammar/root.rs"),
            ],
        );
    }

    #[test]
    fn rejects_absolute_and_package_escaping_paths_without_watching_them() {
        let package = TempDir::new().unwrap();
        let absolute = discover(
            &package,
            package.path().join("root.rs").to_str().unwrap(),
            &CargoCfgSnapshot::default(),
        );
        assert_eq!(
            absolute.diagnostics[0].kind,
            DiscoveryDiagnosticKind::AbsolutePath,
        );
        assert!(absolute.build_inputs.source_files.is_empty());
        assert!(absolute.build_inputs.watched_module_candidates.is_empty());

        write(
            package.path(),
            "grammar/root.rs",
            "#[path = \"../../outside.rs\"] mod outside;",
        );
        let escaping = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());
        assert_eq!(
            escaping.diagnostics[0].kind,
            DiscoveryDiagnosticKind::PackageEscape,
        );
        assert_eq!(
            escaping.build_inputs.source_files,
            [PathBuf::from("grammar/root.rs")],
        );
        assert!(escaping.build_inputs.watched_module_candidates.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_root_file_and_intermediate_directory_symlinks_below_the_trust_anchor() {
        use std::os::unix::fs::symlink;

        let package = TempDir::new().unwrap();
        write(package.path(), "real/root.rs", "");
        fs::create_dir_all(package.path().join("grammar")).unwrap();
        symlink("../real/root.rs", package.path().join("grammar/root.rs")).unwrap();
        let root_link = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());
        assert_eq!(
            root_link.diagnostics[0].kind,
            DiscoveryDiagnosticKind::SymlinkPath,
        );

        let package = TempDir::new().unwrap();
        write(package.path(), "grammar/root.rs", "mod child;");
        write(package.path(), "real/child.rs", "");
        symlink("../real/child.rs", package.path().join("grammar/child.rs")).unwrap();
        let file_link = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());
        assert_eq!(
            file_link.diagnostics[0].kind,
            DiscoveryDiagnosticKind::SymlinkPath,
        );

        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/root.rs",
            "#[path = \"linked\"] mod inline { mod child; }",
        );
        write(package.path(), "real/child.rs", "");
        symlink("../real", package.path().join("grammar/linked")).unwrap();
        let directory_link = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());
        assert_eq!(
            directory_link.diagnostics[0].kind,
            DiscoveryDiagnosticKind::SymlinkPath,
        );

        let package = TempDir::new().unwrap();
        write(package.path(), "real-package/grammar/root.rs", "");
        symlink(
            package.path().join("real-package"),
            package.path().join("package-link"),
        )
        .unwrap();
        let linked_manifest = discover_module_tree(DiscoveryInput {
            manifest_dir: &package.path().join("package-link"),
            selected_root: Path::new("grammar/root.rs"),
            edition: RustEdition::Rust2024,
            cfg: &CargoCfgSnapshot::default(),
        });
        assert!(linked_manifest.diagnostics.is_empty());
    }

    #[test]
    fn evaluates_all_cfg_forms_and_builds_a_canonical_cargo_snapshot() {
        let snapshot = CargoCfgSnapshot::from_cargo_environment([
            ("CARGO_CFG_UNIX".to_owned(), String::new()),
            ("CARGO_CFG_TARGET_ABI".to_owned(), String::new()),
            ("CARGO_CFG_TARGET_ENV".to_owned(), String::new()),
            ("CARGO_CFG_TARGET_OS".to_owned(), "linux".to_owned()),
            ("CARGO_CFG_TARGET_FEATURE".to_owned(), "sse2,avx".to_owned()),
            ("CARGO_CFG_FEATURE".to_owned(), "alt,foo-bar".to_owned()),
            ("CARGO_FEATURE_FOO_BAR".to_owned(), "1".to_owned()),
        ]);
        assert_eq!(
            snapshot.canonical_facts(),
            [
                "feature=\"alt\"".to_owned(),
                "feature=\"foo-bar\"".to_owned(),
                "target_abi=\"\"".to_owned(),
                "target_env=\"\"".to_owned(),
                "target_feature=\"avx\"".to_owned(),
                "target_feature=\"sse2\"".to_owned(),
                "target_os=\"linux\"".to_owned(),
                "unix".to_owned(),
            ],
        );

        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/root.rs",
            r#"
#[cfg(all(unix, any(target_os = "linux", target_os = "macos"), not(windows)))]
#[path = "selected.rs"]
mod selected;
#[cfg(any(windows, target_os = "macos"))]
mod inactive;
#[cfg(all(target_abi = "", target_env = ""))]
#[path = "empty_target_values.rs"]
mod empty_target_values;
#[cfg(contract_checks)]
mod inactive_contract_checks;
"#,
        );
        write(package.path(), "grammar/selected.rs", "");
        write(package.path(), "grammar/empty_target_values.rs", "");
        let report = discover(&package, "grammar/root.rs", &snapshot);
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.build_inputs.cfg_snapshot, snapshot.canonical_facts(),);
        assert_eq!(
            report.build_inputs.watched_module_candidates,
            [
                PathBuf::from("grammar/empty_target_values.rs"),
                PathBuf::from("grammar/selected.rs"),
            ],
        );
    }

    #[test]
    fn raw_identifiers_use_unraw_module_names() {
        let package = TempDir::new().unwrap();
        write(package.path(), "grammar/root.rs", "mod r#type;");
        write(package.path(), "grammar/type.rs", "");

        let report = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());

        assert!(report.diagnostics.is_empty());
        assert_eq!(
            report.build_inputs.watched_module_candidates,
            [
                PathBuf::from("grammar/type/mod.rs"),
                PathBuf::from("grammar/type.rs"),
            ],
        );
        assert_eq!(report.tree.unwrap().inclusions[1].logical_path, ["type"],);
    }

    #[test]
    fn selected_mod_rs_root_owns_its_directory_without_a_relative_stem() {
        let package = TempDir::new().unwrap();
        write(package.path(), "src/grammar/mod.rs", "mod child;");
        write(package.path(), "src/grammar/child.rs", "");

        let report = discover(&package, "src/grammar/mod.rs", &CargoCfgSnapshot::default());

        assert!(report.diagnostics.is_empty());
        assert_eq!(
            report.build_inputs.watched_module_candidates,
            [
                PathBuf::from("src/grammar/child/mod.rs"),
                PathBuf::from("src/grammar/child.rs"),
            ],
        );
        let tree = report.tree.unwrap();
        assert_eq!(
            tree.inclusions[0].ownership.directory,
            Path::new("src/grammar")
        );
        assert_eq!(tree.inclusions[0].ownership.relative_stem, None);
        assert_eq!(tree.inclusions[1].logical_path, ["child"]);
    }

    #[test]
    fn syntax_failure_retains_the_discovered_file_and_exact_location() {
        let package = TempDir::new().unwrap();
        write(package.path(), "grammar/root.rs", "mod broken;");
        write(package.path(), "grammar/broken.rs", "struct Broken {");

        let report = discover(&package, "grammar/root.rs", &CargoCfgSnapshot::default());

        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].kind,
            DiscoveryDiagnosticKind::RustSyntax,
        );
        assert_eq!(report.diagnostics[0].path, Path::new("grammar/broken.rs"));
        assert!(report.diagnostics[0].range.is_some());
        assert_eq!(
            report.build_inputs.source_files,
            [
                PathBuf::from("grammar/root.rs"),
                PathBuf::from("grammar/broken.rs"),
            ],
        );
    }

    #[test]
    fn cargo_1_88_invalidation_covers_edits_failures_candidates_and_cfg_switches() {
        let explicit = TempDir::new().unwrap();
        scaffold_cargo_package(explicit.path(), false);
        write(
            explicit.path(),
            "grammar/root.rs",
            "#[path = \"child.rs\"] mod child;",
        );
        write(
            explicit.path(),
            "grammar/child.rs",
            "pub const VALUE: u8 = 1;",
        );
        let discovery = discover(&explicit, "grammar/root.rs", &CargoCfgSnapshot::default());
        assert!(discovery.diagnostics.is_empty());
        write_tracking_build_script(
            explicit.path(),
            &discovery.build_inputs.watched_paths(),
            &[
                PathBuf::from("grammar/root.rs"),
                PathBuf::from("grammar/child.rs"),
            ],
            &[],
        );

        let initial = run_cargo(explicit.path(), &[]);
        assert!(
            initial.status.success(),
            "{}{}",
            String::from_utf8_lossy(&initial.stdout),
            String::from_utf8_lossy(&initial.stderr),
        );
        assert_eq!(run_count(explicit.path()), 1);
        assert!(run_cargo(explicit.path(), &[]).status.success());
        assert_eq!(
            run_count(explicit.path()),
            1,
            "explicit warm build is fresh"
        );

        write(
            explicit.path(),
            "grammar/child.rs",
            "pub const VALUE: u8 = 2;",
        );
        make_newer(&explicit.path().join("grammar/child.rs"), 2);
        assert!(run_cargo(explicit.path(), &[]).status.success());
        assert_eq!(run_count(explicit.path()), 2);
        assert!(last_run(explicit.path()).contains("VALUE: u8 = 2"));

        fs::remove_file(explicit.path().join("grammar/child.rs")).unwrap();
        let deleted = run_cargo(explicit.path(), &[]);
        assert!(!deleted.status.success());
        assert_eq!(run_count(explicit.path()), 3);
        let deleted_output = format!(
            "{}{}",
            String::from_utf8_lossy(&deleted.stdout),
            String::from_utf8_lossy(&deleted.stderr),
        );
        assert!(deleted_output.contains("cargo::rerun-if-changed=grammar/child.rs"));

        write(
            explicit.path(),
            "grammar/child.rs",
            "pub const VALUE: u8 = 3;",
        );
        make_newer(&explicit.path().join("grammar/child.rs"), 4);
        assert!(run_cargo(explicit.path(), &[]).status.success());
        assert_eq!(run_count(explicit.path()), 4);

        let default = TempDir::new().unwrap();
        scaffold_cargo_package(default.path(), false);
        write(default.path(), "grammar/root.rs", "mod child;");
        write(default.path(), "grammar/child.rs", "");
        let discovery = discover(&default, "grammar/root.rs", &CargoCfgSnapshot::default());
        assert!(discovery.diagnostics.is_empty());
        write_tracking_build_script(
            default.path(),
            &discovery.build_inputs.watched_paths(),
            &[PathBuf::from("grammar/root.rs")],
            &[(
                PathBuf::from("grammar/child.rs"),
                PathBuf::from("grammar/child/mod.rs"),
            )],
        );
        assert!(run_cargo(default.path(), &[]).status.success());
        assert_eq!(run_count(default.path()), 1);
        assert!(run_cargo(default.path(), &[]).status.success());
        assert_eq!(
            run_count(default.path()),
            2,
            "Cargo 1.88 reruns while a watched alternate is absent",
        );
        write(default.path(), "grammar/child/mod.rs", "");
        make_newer(&default.path().join("grammar/child/mod.rs"), 2);
        assert!(!run_cargo(default.path(), &[]).status.success());
        assert_eq!(run_count(default.path()), 3);
        fs::remove_file(default.path().join("grammar/child/mod.rs")).unwrap();
        assert!(run_cargo(default.path(), &[]).status.success());
        assert_eq!(run_count(default.path()), 4);

        let conditional = TempDir::new().unwrap();
        scaffold_cargo_package(conditional.path(), true);
        write(conditional.path(), "grammar/base.rs", "");
        write(conditional.path(), "grammar/alternate.rs", "");
        write(
            conditional.path(),
            "build.rs",
            r#"
use std::fs::OpenOptions;
use std::io::Write as _;

fn main() {
    let selected = if std::env::var_os("CARGO_FEATURE_ALTERNATE").is_some() {
        "grammar/alternate.rs"
    } else {
        "grammar/base.rs"
    };
    println!("cargo::rerun-if-changed={selected}");
    let log = std::env::var_os("REC_TEST_LOG").unwrap();
    writeln!(OpenOptions::new().create(true).append(true).open(log).unwrap(), "{selected}").unwrap();
}
"#,
        );
        assert!(run_cargo(conditional.path(), &[]).status.success());
        assert!(
            run_cargo(conditional.path(), &["--features", "alternate"])
                .status
                .success()
        );
        let selections = fs::read_to_string(conditional.path().join("runs.log")).unwrap();
        assert_eq!(
            selections.lines().collect::<Vec<_>>(),
            ["grammar/base.rs", "grammar/alternate.rs"],
        );
    }
}
