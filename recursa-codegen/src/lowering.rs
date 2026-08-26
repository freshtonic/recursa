#![allow(
    dead_code,
    reason = "private M3 seam is consumed by later source-discovery and generation tickets"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use proc_macro2::Span;
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::{Attribute, Fields, Item, LitStr, Path as SynPath, Token, Type};

use crate::source_syntax::{CaptureMode, GrammarConfigSyntax, PrettyDefaults};
use crate::{
    AnalysisReport, AtomDescription, AtomExpression, AtomId, ContainerShape, DeclId, Declaration,
    Diagnostic, GrammarDescription, KindDescription, KindId, Location, LocationId, ParseSite,
    SiteId, SourceId, TextEquivalence, TypeDescription, TypeId, TypeShape, analyze,
};

macro_rules! source_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(u32);

        impl $name {
            pub(crate) fn next(length: usize) -> Self {
                Self(length.try_into().expect("source model ID overflow"))
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

source_id!(ModuleId);
source_id!(NodeId);
source_id!(VariantId);
source_id!(ElementId);
source_id!(TokenDefinitionId);
source_id!(TokenUseId);
source_id!(SourceDeclarationId);
source_id!(SourceLocationId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RustEdition {
    Rust2024,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SingleFileInput<'source> {
    pub(crate) manifest_relative_path: &'source Path,
    pub(crate) source: &'source str,
    pub(crate) edition: RustEdition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticStage {
    Discovery,
    Syntax,
    Lowering,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceDiagnostic {
    pub(crate) stage: DiagnosticStage,
    pub(crate) path: PathBuf,
    pub(crate) range: Option<Range<u32>>,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SingleFileAnalysisReport {
    pub(crate) diagnostics: Vec<SourceDiagnostic>,
    pub(crate) semantic_diagnostics: Vec<Diagnostic>,
    pub(crate) analyzed: Option<AnalyzedGrammar>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AnalyzedGrammar {
    pub(crate) lowered: SingleFileLoweredGrammar,
    pub(crate) analysis: AnalysisReport,
}

impl AnalyzedGrammar {
    pub(crate) fn canonical_semantic_projection(&self) -> Option<String> {
        let grammar = &self.lowered.grammar;
        let source = &self.lowered.source_model;
        let output = self.analysis.output.as_ref()?;
        let mut rendered = String::new();
        let equivalence = match grammar.text_equivalence {
            TextEquivalence::Sensitive => "sensitive",
            TextEquivalence::AsciiInsensitive => "ascii_insensitive",
        };
        writeln!(
            rendered,
            "grammar crate equivalence={equivalence} lookahead={}",
            grammar.max_lookahead,
        )
        .expect("writing to String cannot fail");
        for token in &source.token_definitions {
            writeln!(
                rendered,
                "token {} pattern={}",
                token.ident,
                token.pattern.value(),
            )
            .expect("writing to String cannot fail");
        }
        for node in &source.nodes {
            let targets = source
                .elements
                .iter()
                .filter(|element| element.node == node.id)
                .map(|element| {
                    let token_use = &source.token_uses[element.token_use.index()];
                    source.token_definitions[token_use.definition.index()]
                        .ident
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(rendered, "node {} sequence=({targets})", node.ident)
                .expect("writing to String cannot fail");
        }
        for (index, ty) in grammar.types.iter().enumerate() {
            let first = output
                .first(TypeId::new(index.try_into().expect("type index overflow")))
                .expect("M2 returns FIRST facts for every type");
            let kinds = first
                .kinds()
                .iter()
                .map(|kind| grammar.kinds[kind.index()].name.as_str())
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                rendered,
                "analysis {} FIRST={{{kinds}}} NULLABLE={}",
                ty.name, first.nullable,
            )
            .expect("writing to String cannot fail");
        }
        Some(rendered)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SingleFileLoweredGrammar {
    pub(crate) build_inputs: BuildInputs,
    pub(crate) grammar: GrammarDescription,
    pub(crate) source_model: SourceModel,
    pub(crate) lowering_map: LoweringMap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryInputs {
    pub(crate) selected_root: PathBuf,
    pub(crate) source_files: Vec<PathBuf>,
    pub(crate) edition: RustEdition,
    pub(crate) watched_module_candidates: Vec<PathBuf>,
    pub(crate) cfg_snapshot: Vec<String>,
    pub(crate) environment_dependencies: Vec<String>,
}

impl DiscoveryInputs {
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BuildInputs {
    pub(crate) discovery: DiscoveryInputs,
    pub(crate) module_anchor: SelectedRootAnchor,
    pub(crate) facade: FacadeMapping,
    pub(crate) text_equivalence: TextEquivalence,
    pub(crate) max_lookahead: usize,
    pub(crate) spans: CaptureMode,
    pub(crate) diagrams: bool,
    pub(crate) pretty: PrettyDefaults,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectedRootAnchor {
    pub(crate) segments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FacadeMapping {
    pub(crate) ident: String,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceModel {
    pub(crate) files: Vec<SourceFile>,
    pub(crate) modules: Vec<SourceModule>,
    pub(crate) grammar: SourceGrammarConfig,
    pub(crate) nodes: Vec<SourceNode>,
    pub(crate) variants: Vec<SourceVariant>,
    pub(crate) elements: Vec<SourceElement>,
    pub(crate) token_definitions: Vec<SourceTokenDefinition>,
    pub(crate) token_uses: Vec<SourceTokenUse>,
    pub(crate) declarations: Vec<SourceDeclaration>,
    pub(crate) locations: Vec<SourceLocation>,
}

impl SourceModel {
    pub(crate) fn location(&self, id: SourceLocationId) -> &SourceLocation {
        &self.locations[id.index()]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceFile {
    pub(crate) id: SourceId,
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceModule {
    pub(crate) id: ModuleId,
    pub(crate) file: SourceId,
    pub(crate) path: SynPath,
    pub(crate) location: SourceLocationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceGrammarConfig {
    pub(crate) module: ModuleId,
    pub(crate) text_equivalence: TextEquivalence,
    pub(crate) max_lookahead: usize,
    pub(crate) location: SourceLocationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceNode {
    pub(crate) id: NodeId,
    pub(crate) module: ModuleId,
    pub(crate) declaration: SourceDeclarationId,
    pub(crate) ident: syn::Ident,
    pub(crate) generics: syn::Generics,
    pub(crate) visibility: syn::Visibility,
    pub(crate) location: SourceLocationId,
    pub(crate) elements: Vec<ElementId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceVariant {
    pub(crate) id: VariantId,
    pub(crate) node: NodeId,
    pub(crate) declaration: SourceDeclarationId,
    pub(crate) ident: syn::Ident,
    pub(crate) location: SourceLocationId,
    pub(crate) elements: Vec<ElementId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceElement {
    pub(crate) id: ElementId,
    pub(crate) node: NodeId,
    pub(crate) variant: Option<VariantId>,
    pub(crate) declaration: SourceDeclarationId,
    pub(crate) ident: Option<syn::Ident>,
    pub(crate) visibility: syn::Visibility,
    pub(crate) ty: Type,
    pub(crate) token_use: TokenUseId,
    pub(crate) location: SourceLocationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceTokenDefinition {
    pub(crate) id: TokenDefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) declaration: SourceDeclarationId,
    pub(crate) ident: syn::Ident,
    pub(crate) pattern: LitStr,
    pub(crate) location: SourceLocationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceTokenUse {
    pub(crate) id: TokenUseId,
    pub(crate) definition: TokenDefinitionId,
    pub(crate) element: ElementId,
    pub(crate) location: SourceLocationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceDeclaration {
    pub(crate) id: SourceDeclarationId,
    pub(crate) location: SourceLocationId,
    pub(crate) kind: SourceDeclarationKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceDeclarationKind {
    Node(NodeId),
    Element(ElementId),
    Token(TokenDefinitionId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceLocation {
    pub(crate) id: SourceLocationId,
    pub(crate) source: SourceId,
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoweringMap {
    pub(crate) source_files: Vec<SourceFileCorrelation>,
    pub(crate) nodes: Vec<NodeTypeCorrelation>,
    pub(crate) elements: Vec<ElementSiteCorrelation>,
    pub(crate) token_definitions: Vec<TokenDefinitionCorrelation>,
    pub(crate) token_uses: Vec<TokenUseSiteCorrelation>,
    pub(crate) declarations: Vec<DeclarationCorrelation>,
    pub(crate) locations: Vec<LocationCorrelation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceFileCorrelation {
    pub(crate) source: SourceId,
    pub(crate) m2_source: SourceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NodeTypeCorrelation {
    pub(crate) source: NodeId,
    pub(crate) lowered: TypeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ElementSiteCorrelation {
    pub(crate) source: ElementId,
    pub(crate) lowered: SiteId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TokenDefinitionCorrelation {
    pub(crate) source: TokenDefinitionId,
    pub(crate) kind: KindId,
    pub(crate) atom: AtomId,
    pub(crate) ty: TypeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TokenUseSiteCorrelation {
    pub(crate) source: TokenUseId,
    pub(crate) lowered: SiteId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DeclarationCorrelation {
    pub(crate) source: SourceDeclarationId,
    pub(crate) lowered: DeclId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocationCorrelation {
    pub(crate) source: SourceLocationId,
    pub(crate) lowered: LocationId,
}

impl LoweringMap {
    pub(crate) fn validate(
        &self,
        source: &SourceModel,
        grammar: &GrammarDescription,
    ) -> Result<(), String> {
        validate_targets(
            "source file identity",
            source.files.len(),
            source.files.iter().map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "module identity",
            source.modules.len(),
            source.modules.iter().map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "Node identity",
            source.nodes.len(),
            source.nodes.iter().map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "variant identity",
            source.variants.len(),
            source.variants.iter().map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "element identity",
            source.elements.len(),
            source.elements.iter().map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "token-definition identity",
            source.token_definitions.len(),
            source
                .token_definitions
                .iter()
                .map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "token-use identity",
            source.token_uses.len(),
            source.token_uses.iter().map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "source-declaration identity",
            source.declarations.len(),
            source.declarations.iter().map(|entry| entry.id.index()),
        )?;
        validate_targets(
            "source-location identity",
            source.locations.len(),
            source.locations.iter().map(|entry| entry.id.index()),
        )?;
        validate_total(
            "source file",
            source.files.len(),
            &self.source_files,
            |entry| entry.source.index(),
        )?;
        validate_total("Node", source.nodes.len(), &self.nodes, |entry| {
            entry.source.index()
        })?;
        validate_total("element", source.elements.len(), &self.elements, |entry| {
            entry.source.index()
        })?;
        validate_total(
            "token definition",
            source.token_definitions.len(),
            &self.token_definitions,
            |entry| entry.source.index(),
        )?;
        validate_total(
            "token use",
            source.token_uses.len(),
            &self.token_uses,
            |entry| entry.source.index(),
        )?;
        validate_total(
            "source declaration",
            source.declarations.len(),
            &self.declarations,
            |entry| entry.source.index(),
        )?;
        validate_total(
            "M2 location",
            grammar.locations.len(),
            &self.locations,
            |entry| entry.lowered.index(),
        )?;

        validate_targets(
            "M2 source",
            grammar.source_count as usize,
            self.source_files
                .iter()
                .map(|entry| entry.m2_source.index()),
        )?;
        validate_unique_targets(
            "Node type",
            grammar.types.len(),
            self.nodes.iter().map(|entry| entry.lowered.index()),
        )?;
        validate_unique_targets(
            "token type",
            grammar.types.len(),
            self.token_definitions.iter().map(|entry| entry.ty.index()),
        )?;
        let all_type_origins = self
            .nodes
            .iter()
            .map(|entry| entry.lowered.index())
            .chain(self.token_definitions.iter().map(|entry| entry.ty.index()))
            .collect::<BTreeSet<_>>();
        if all_type_origins != (0..grammar.types.len()).collect() {
            return Err("M2 type origins are not total and one-to-one".to_owned());
        }
        validate_targets(
            "element site",
            grammar.sites.len(),
            self.elements.iter().map(|entry| entry.lowered.index()),
        )?;
        validate_targets(
            "token kind",
            grammar.kinds.len(),
            self.token_definitions
                .iter()
                .map(|entry| entry.kind.index()),
        )?;
        validate_targets(
            "token atom",
            grammar.atoms.len(),
            self.token_definitions
                .iter()
                .map(|entry| entry.atom.index()),
        )?;
        validate_targets(
            "M2 declaration",
            grammar.declarations.len(),
            self.declarations.iter().map(|entry| entry.lowered.index()),
        )?;

        for correlation in &self.source_files {
            if correlation.source != correlation.m2_source {
                return Err("M2 SourceId does not correspond to its source file".to_owned());
            }
        }
        if source.grammar.module.index() >= source.modules.len()
            || source.grammar.location.index() >= source.locations.len()
        {
            return Err("grammar configuration has an invalid source origin".to_owned());
        }
        if grammar.text_equivalence != source.grammar.text_equivalence
            || grammar.max_lookahead != source.grammar.max_lookahead
        {
            return Err("M2 grammar configuration differs from its source origin".to_owned());
        }
        if grammar.name != "crate"
            || !path_is(
                &source.modules[source.grammar.module.index()].path,
                &["crate"],
            )
        {
            return Err("M2 grammar anchor differs from its source origin".to_owned());
        }
        for module in &source.modules {
            if module.file.index() >= source.files.len()
                || module.location.index() >= source.locations.len()
            {
                return Err("module inclusion has an invalid source origin".to_owned());
            }
        }
        if !source.variants.is_empty() {
            return Err("enum variants are outside ticket #53's single-struct seam".to_owned());
        }
        validate_targets(
            "Node element membership",
            source.elements.len(),
            source
                .nodes
                .iter()
                .flat_map(|node| node.elements.iter().map(|element| element.index())),
        )?;
        for node in &source.nodes {
            if node.elements.iter().any(|element| {
                source
                    .elements
                    .get(element.index())
                    .is_none_or(|element| element.node != node.id)
            }) {
                return Err("Node element plan differs from element ownership".to_owned());
            }
        }
        for node in &source.nodes {
            if node.module.index() >= source.modules.len()
                || node.declaration.index() >= source.declarations.len()
                || node.location.index() >= source.locations.len()
            {
                return Err("Node contains an invalid source identity".to_owned());
            }
            let correlation = self
                .nodes
                .iter()
                .find(|entry| entry.source == node.id)
                .ok_or_else(|| "Node has no M2 type origin".to_owned())?;
            let description = &grammar.types[correlation.lowered.index()];
            let declaration = lowered_declaration(self, node.declaration)?;
            let TypeShape::Sequence { sites } = &description.shape else {
                return Err("source Node does not map to an M2 sequence type".to_owned());
            };
            let expected_sites = node
                .elements
                .iter()
                .map(|element| {
                    self.elements
                        .iter()
                        .find(|entry| entry.source == *element)
                        .map(|entry| entry.lowered)
                        .ok_or_else(|| "Node element has no M2 site origin".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            if node.ident != description.name
                || description.declaration != declaration
                || *sites != expected_sites
            {
                return Err("Node-to-type correlation does not preserve Node semantics".to_owned());
            }
        }
        for token in &source.token_definitions {
            if token.module.index() >= source.modules.len()
                || token.declaration.index() >= source.declarations.len()
                || token.location.index() >= source.locations.len()
            {
                return Err("token definition contains an invalid source identity".to_owned());
            }
            let correlation = self
                .token_definitions
                .iter()
                .find(|entry| entry.source == token.id)
                .ok_or_else(|| "token definition has no M2 origin".to_owned())?;
            let declaration = lowered_declaration(self, token.declaration)?;
            let kind = &grammar.kinds[correlation.kind.index()];
            let atom = &grammar.atoms[correlation.atom.index()];
            let ty = &grammar.types[correlation.ty.index()];
            let token_name = token.ident.to_string();
            let atom_matches = matches!(
                atom.expression,
                AtomExpression::Kind(kind) if kind == correlation.kind
            );
            let type_matches = matches!(
                ty.shape,
                TypeShape::Token { atom, text_predicate: None } if atom == correlation.atom
            );
            if kind.name != token_name
                || kind.declaration != declaration
                || atom.name != token_name
                || atom.declaration != declaration
                || !atom_matches
                || ty.name != token_name
                || ty.declaration != declaration
                || !type_matches
            {
                return Err(
                    "token-definition correlation does not preserve token semantics".to_owned(),
                );
            }
        }
        for element in &source.elements {
            if element.node.index() >= source.nodes.len()
                || element.declaration.index() >= source.declarations.len()
                || element.token_use.index() >= source.token_uses.len()
                || element.location.index() >= source.locations.len()
                || element
                    .variant
                    .is_some_and(|variant| variant.index() >= source.variants.len())
            {
                return Err("grammar element contains an invalid source identity".to_owned());
            }
            let correlation = self
                .elements
                .iter()
                .find(|entry| entry.source == element.id)
                .ok_or_else(|| "grammar element has no M2 site origin".to_owned())?;
            let site = &grammar.sites[correlation.lowered.index()];
            let owner = self
                .nodes
                .iter()
                .find(|entry| entry.source == element.node)
                .ok_or_else(|| "element owner has no M2 type origin".to_owned())?
                .lowered;
            let token_use = &source.token_uses[element.token_use.index()];
            if token_use.definition.index() >= source.token_definitions.len() {
                return Err("element token use has an invalid definition".to_owned());
            }
            let target_correlation = self
                .token_definitions
                .iter()
                .find(|entry| entry.source == token_use.definition)
                .ok_or_else(|| "element token has no M2 type origin".to_owned())?;
            let target_definition = &source.token_definitions[token_use.definition.index()];
            if token_type_ident(&element.ty).map_err(|message| {
                format!("stored grammar-element type is not a valid token reference: {message}")
            })? != target_definition.ident
            {
                return Err(
                    "grammar-element type differs from its token-definition origin".to_owned(),
                );
            }
            let declaration = lowered_declaration(self, element.declaration)?;
            if site.owner != owner
                || site.target != target_correlation.ty
                || site.declaration != declaration
                || site.container != ContainerShape::Direct
            {
                return Err(
                    "element-to-site correlation does not preserve element semantics".to_owned(),
                );
            }
        }
        for declaration in &source.declarations {
            if declaration.location.index() >= source.locations.len() {
                return Err("source declaration has an invalid location".to_owned());
            }
            let source_record_matches = match declaration.kind {
                SourceDeclarationKind::Node(id) => {
                    source.nodes.get(id.index()).is_some_and(|node| {
                        node.declaration == declaration.id && node.location == declaration.location
                    })
                }
                SourceDeclarationKind::Element(id) => {
                    source.elements.get(id.index()).is_some_and(|element| {
                        element.declaration == declaration.id
                            && element.location == declaration.location
                    })
                }
                SourceDeclarationKind::Token(id) => source
                    .token_definitions
                    .get(id.index())
                    .is_some_and(|token| {
                        token.declaration == declaration.id
                            && token.location == declaration.location
                    }),
            };
            if !source_record_matches {
                return Err("source declaration differs from its typed origin".to_owned());
            }
            let lowered = lowered_declaration(self, declaration.id)?;
            let location = self
                .locations
                .iter()
                .find(|entry| entry.source == declaration.location)
                .ok_or_else(|| "source declaration location has no M2 origin".to_owned())?
                .lowered;
            if grammar.declarations[lowered.index()].location != location {
                return Err("M2 declaration differs from its source origin".to_owned());
            }
        }

        for token_use in &source.token_uses {
            if token_use.definition.index() >= source.token_definitions.len()
                || token_use.element.index() >= source.elements.len()
                || token_use.location.index() >= source.locations.len()
            {
                return Err("token use contains an invalid source identity".to_owned());
            }
            if source.elements[token_use.element.index()].token_use != token_use.id {
                return Err("token use differs from its grammar-element origin".to_owned());
            }
            let use_site = self
                .token_uses
                .iter()
                .find(|entry| entry.source == token_use.id)
                .ok_or_else(|| "token use has no M2 site origin".to_owned())?
                .lowered;
            let element_site = self
                .elements
                .iter()
                .find(|entry| entry.source == token_use.element)
                .ok_or_else(|| "token-use element has no M2 site origin".to_owned())?
                .lowered;
            if use_site != element_site {
                return Err("token use and containing element map to different sites".to_owned());
            }
        }
        for location in &source.locations {
            let file = source
                .files
                .get(location.source.index())
                .ok_or_else(|| "source location refers to an unknown file".to_owned())?;
            let range = location.range.start as usize..location.range.end as usize;
            if range.start >= range.end
                || range.end > file.text.len()
                || !file.text.is_char_boundary(range.start)
                || !file.text.is_char_boundary(range.end)
            {
                return Err("source location is not a valid non-empty UTF-8 range".to_owned());
            }
        }
        for correlation in &self.locations {
            let source_location = source
                .locations
                .get(correlation.source.index())
                .ok_or_else(|| "M2 location has no source origin".to_owned())?;
            let lowered = &grammar.locations[correlation.lowered.index()];
            if lowered.source != source_location.source || lowered.range != source_location.range {
                return Err("M2 location differs from its source origin".to_owned());
            }
        }
        Ok(())
    }
}

fn lowered_declaration(
    lowering_map: &LoweringMap,
    source: SourceDeclarationId,
) -> Result<DeclId, String> {
    lowering_map
        .declarations
        .iter()
        .find(|entry| entry.source == source)
        .map(|entry| entry.lowered)
        .ok_or_else(|| "source declaration has no M2 origin".to_owned())
}

fn validate_total<T>(
    name: &str,
    expected: usize,
    entries: &[T],
    index: impl Fn(&T) -> usize,
) -> Result<(), String> {
    if entries.len() != expected {
        return Err(format!("{name} correlation is not total"));
    }
    let indices = entries.iter().map(index).collect::<BTreeSet<_>>();
    if indices != (0..expected).collect() {
        return Err(format!("{name} correlation is not one-to-one"));
    }
    Ok(())
}

fn validate_targets(
    name: &str,
    upper_bound: usize,
    indices: impl IntoIterator<Item = usize>,
) -> Result<(), String> {
    let seen = validate_unique_targets(name, upper_bound, indices)?;
    if seen != (0..upper_bound).collect() {
        return Err(format!("{name} correlation is not total"));
    }
    Ok(())
}

fn validate_unique_targets(
    name: &str,
    upper_bound: usize,
    indices: impl IntoIterator<Item = usize>,
) -> Result<BTreeSet<usize>, String> {
    let mut seen = BTreeSet::new();
    for index in indices {
        if index >= upper_bound || !seen.insert(index) {
            return Err(format!("{name} correlation is invalid or duplicated"));
        }
    }
    Ok(seen)
}

pub(crate) fn analyze_single_file(input: SingleFileInput<'_>) -> SingleFileAnalysisReport {
    let lowering = lower_single_file(input);
    if !lowering.diagnostics.is_empty() {
        return SingleFileAnalysisReport {
            diagnostics: lowering.diagnostics,
            semantic_diagnostics: Vec::new(),
            analyzed: None,
        };
    }
    let lowered = lowering
        .lowered
        .expect("successful lowering has an aggregate");
    let analysis = analyze(&lowered.grammar);
    if analysis.output.is_none() {
        return SingleFileAnalysisReport {
            diagnostics: Vec::new(),
            semantic_diagnostics: analysis.diagnostics,
            analyzed: None,
        };
    }
    SingleFileAnalysisReport {
        diagnostics: Vec::new(),
        semantic_diagnostics: Vec::new(),
        analyzed: Some(AnalyzedGrammar { lowered, analysis }),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoweringReport {
    pub(crate) diagnostics: Vec<SourceDiagnostic>,
    pub(crate) lowered: Option<SingleFileLoweredGrammar>,
}

pub(crate) fn lower_single_file(input: SingleFileInput<'_>) -> LoweringReport {
    let syntax = match syn::parse_file(input.source) {
        Ok(syntax) => syntax,
        Err(error) => {
            return failure(
                input.manifest_relative_path,
                DiagnosticStage::Syntax,
                Some(span_range(error.span())),
                error.to_string(),
            );
        }
    };

    let grammar_macros = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Macro(item) if path_is(&item.mac.path, &["recursa", "grammar"]) => Some(item),
            _ => None,
        })
        .collect::<Vec<_>>();
    if grammar_macros.len() != 1 {
        let range = grammar_macros
            .get(1)
            .or_else(|| grammar_macros.first())
            .map(|item| span_range(item.span()));
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            range,
            format!(
                "expected exactly one active recursa::grammar! configuration, found {}",
                grammar_macros.len(),
            ),
        );
    }
    let grammar_item = grammar_macros[0];
    let config = match syn::parse2::<GrammarConfigSyntax>(grammar_item.mac.tokens.clone()) {
        Ok(config) => config,
        Err(error) => {
            return failure(
                input.manifest_relative_path,
                DiagnosticStage::Lowering,
                Some(span_range(error.span())),
                error.to_string(),
            );
        }
    };
    if config.facade != "recursa" {
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            Some(span_range(config.facade.span())),
            "ticket #53's single-file seam supports only the default `recursa` facade; configured facade resolution is handled by ticket #58"
                .to_owned(),
        );
    }
    if !path_is(&config.module, &["crate"]) {
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            Some(span_range(config.module.span())),
            "ticket #53 supports only the direct `module = crate` anchor; anchored path resolution is handled by ticket #58"
                .to_owned(),
        );
    }

    let nodes = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(item) if has_node_derive(&item.attrs) => Some(item),
            _ => None,
        })
        .collect::<Vec<_>>();
    if nodes.len() != 1 {
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            nodes.first().map(|item| span_range(item.ident.span())),
            format!(
                "ticket #53 single-file lowering requires exactly one Node, found {}",
                nodes.len(),
            ),
        );
    }
    let node_item = nodes[0];
    let Fields::Named(fields) = &node_item.fields else {
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            Some(span_range(node_item.fields.span())),
            "ticket #53 single-file lowering requires a Node with named fields".to_owned(),
        );
    };
    if fields.named.len() != 1 {
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            Some(span_range(fields.span())),
            format!(
                "ticket #53 single-file lowering requires exactly one semantic field, found {}",
                fields.named.len(),
            ),
        );
    }
    let field = &fields.named[0];
    let lex_attributes = field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("lex"))
        .collect::<Vec<_>>();
    if lex_attributes.len() != 1 {
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            Some(span_range(field.span())),
            format!(
                "ticket #53 single-file lowering requires exactly one #[lex(...)] content-token declaration, found {}",
                lex_attributes.len(),
            ),
        );
    }
    let pattern = match parse_lex(lex_attributes[0]) {
        Ok(pattern) => pattern,
        Err(error) => {
            return failure(
                input.manifest_relative_path,
                DiagnosticStage::Lowering,
                Some(span_range(error.span())),
                error.to_string(),
            );
        }
    };
    let token_ident = match token_type_ident(&field.ty) {
        Ok(ident) => ident,
        Err(message) => {
            return failure(
                input.manifest_relative_path,
                DiagnosticStage::Lowering,
                Some(span_range(field.ty.span())),
                message,
            );
        }
    };

    let mut locations = Vec::new();
    let file_id = SourceId::new(0);
    let config_location = push_source_location(&mut locations, file_id, grammar_item.span());
    let module_location = push_source_location(&mut locations, file_id, config.module.span());
    let node_location = push_source_location(&mut locations, file_id, node_item.ident.span());
    let element_location = push_source_location(
        &mut locations,
        file_id,
        field
            .ident
            .as_ref()
            .map_or_else(|| field.span(), syn::Ident::span),
    );
    let token_location = push_source_location(&mut locations, file_id, token_ident.span());
    let token_use_location = push_source_location(&mut locations, file_id, field.ty.span());

    let module_id = ModuleId::next(0);
    let node_id = NodeId::next(0);
    let element_id = ElementId::next(0);
    let token_definition_id = TokenDefinitionId::next(0);
    let token_use_id = TokenUseId::next(0);
    let token_declaration_id = SourceDeclarationId::next(0);
    let node_declaration_id = SourceDeclarationId::next(1);
    let element_declaration_id = SourceDeclarationId::next(2);

    let source_model = SourceModel {
        files: vec![SourceFile {
            id: file_id,
            path: input.manifest_relative_path.to_owned(),
            text: input.source.to_owned(),
        }],
        modules: vec![SourceModule {
            id: module_id,
            file: file_id,
            path: config.module.clone(),
            location: module_location,
        }],
        grammar: SourceGrammarConfig {
            module: module_id,
            text_equivalence: config.text_equivalence,
            max_lookahead: config.max_lookahead,
            location: config_location,
        },
        nodes: vec![SourceNode {
            id: node_id,
            module: module_id,
            declaration: node_declaration_id,
            ident: node_item.ident.clone(),
            generics: node_item.generics.clone(),
            visibility: node_item.vis.clone(),
            location: node_location,
            elements: vec![element_id],
        }],
        variants: Vec::new(),
        elements: vec![SourceElement {
            id: element_id,
            node: node_id,
            variant: None,
            declaration: element_declaration_id,
            ident: field.ident.clone(),
            visibility: field.vis.clone(),
            ty: field.ty.clone(),
            token_use: token_use_id,
            location: element_location,
        }],
        token_definitions: vec![SourceTokenDefinition {
            id: token_definition_id,
            module: module_id,
            declaration: token_declaration_id,
            ident: token_ident.clone(),
            pattern,
            location: token_location,
        }],
        token_uses: vec![SourceTokenUse {
            id: token_use_id,
            definition: token_definition_id,
            element: element_id,
            location: token_use_location,
        }],
        declarations: vec![
            SourceDeclaration {
                id: token_declaration_id,
                location: token_location,
                kind: SourceDeclarationKind::Token(token_definition_id),
            },
            SourceDeclaration {
                id: node_declaration_id,
                location: node_location,
                kind: SourceDeclarationKind::Node(node_id),
            },
            SourceDeclaration {
                id: element_declaration_id,
                location: element_location,
                kind: SourceDeclarationKind::Element(element_id),
            },
        ],
        locations,
    };

    let (grammar, lowering_map) = lower_semantics(&source_model, &config);
    if let Err(message) = lowering_map.validate(&source_model, &grammar) {
        return failure(
            input.manifest_relative_path,
            DiagnosticStage::Lowering,
            Some(source_model.location(config_location).range.clone()),
            message,
        );
    }
    LoweringReport {
        diagnostics: Vec::new(),
        lowered: Some(SingleFileLoweredGrammar {
            build_inputs: BuildInputs {
                discovery: DiscoveryInputs {
                    selected_root: input.manifest_relative_path.to_owned(),
                    source_files: vec![input.manifest_relative_path.to_owned()],
                    edition: input.edition,
                    watched_module_candidates: Vec::new(),
                    cfg_snapshot: Vec::new(),
                    environment_dependencies: Vec::new(),
                },
                module_anchor: SelectedRootAnchor {
                    segments: Vec::new(),
                },
                facade: FacadeMapping {
                    ident: config.facade.to_string(),
                },
                text_equivalence: config.text_equivalence,
                max_lookahead: config.max_lookahead,
                spans: config.spans,
                diagrams: config.diagrams,
                pretty: config.pretty,
            },
            grammar,
            source_model,
            lowering_map,
        }),
    }
}

fn lower_semantics(
    source: &SourceModel,
    config: &GrammarConfigSyntax,
) -> (GrammarDescription, LoweringMap) {
    let token = &source.token_definitions[0];
    let node = &source.nodes[0];
    let element = &source.elements[0];
    let mut grammar =
        GrammarDescription::new("crate", 1, config.max_lookahead, config.text_equivalence);
    grammar.source_count = source
        .files
        .len()
        .try_into()
        .expect("source count overflow");

    let token_m2_location = grammar.push_location(source_m2_location(source, token.location));
    let node_m2_location = grammar.push_location(source_m2_location(source, node.location));
    let element_m2_location = grammar.push_location(source_m2_location(source, element.location));
    let token_declaration =
        grammar.push_declaration(Declaration::new(token.ident.to_string(), token_m2_location));
    let node_declaration =
        grammar.push_declaration(Declaration::new(node.ident.to_string(), node_m2_location));
    let element_name = element
        .ident
        .as_ref()
        .map_or_else(|| "<unnamed>".to_owned(), ToString::to_string);
    let element_declaration = grammar.push_declaration(Declaration::new(
        format!("{}::{element_name}", node.ident),
        element_m2_location,
    ));
    let kind = grammar.push_kind(KindDescription::new(
        token.ident.to_string(),
        token_declaration,
    ));
    let atom = grammar.push_atom(AtomDescription::new(
        token.ident.to_string(),
        token_declaration,
        AtomExpression::Kind(kind),
    ));
    let token_type = grammar.push_type(TypeDescription::new(
        token.ident.to_string(),
        token_declaration,
        TypeShape::Token {
            atom,
            text_predicate: None,
        },
    ));
    let node_type = grammar.next_type_id();
    let site = grammar.push_site(ParseSite::new(
        node_type,
        token_type,
        element_declaration,
        ContainerShape::Direct,
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            node.ident.to_string(),
            node_declaration,
            TypeShape::Sequence { sites: vec![site] },
        )),
        node_type,
    );
    grammar.entry_types.push(node_type);

    let map = LoweringMap {
        source_files: vec![SourceFileCorrelation {
            source: SourceId::new(0),
            m2_source: SourceId::new(0),
        }],
        nodes: vec![NodeTypeCorrelation {
            source: node.id,
            lowered: node_type,
        }],
        elements: vec![ElementSiteCorrelation {
            source: element.id,
            lowered: site,
        }],
        token_definitions: vec![TokenDefinitionCorrelation {
            source: token.id,
            kind,
            atom,
            ty: token_type,
        }],
        token_uses: vec![TokenUseSiteCorrelation {
            source: source.token_uses[0].id,
            lowered: site,
        }],
        declarations: vec![
            DeclarationCorrelation {
                source: token.declaration,
                lowered: token_declaration,
            },
            DeclarationCorrelation {
                source: node.declaration,
                lowered: node_declaration,
            },
            DeclarationCorrelation {
                source: element.declaration,
                lowered: element_declaration,
            },
        ],
        locations: vec![
            LocationCorrelation {
                source: token.location,
                lowered: token_m2_location,
            },
            LocationCorrelation {
                source: node.location,
                lowered: node_m2_location,
            },
            LocationCorrelation {
                source: element.location,
                lowered: element_m2_location,
            },
        ],
    };
    (grammar, map)
}

fn source_m2_location(source: &SourceModel, id: SourceLocationId) -> Location {
    let location = source.location(id);
    Location::new(location.source, location.range.clone())
}

fn failure(
    path: &Path,
    stage: DiagnosticStage,
    range: Option<Range<u32>>,
    message: String,
) -> LoweringReport {
    LoweringReport {
        diagnostics: vec![SourceDiagnostic {
            stage,
            path: path.to_owned(),
            range,
            message,
        }],
        lowered: None,
    }
}

fn push_source_location(
    locations: &mut Vec<SourceLocation>,
    source: SourceId,
    span: Span,
) -> SourceLocationId {
    let id = SourceLocationId::next(locations.len());
    locations.push(SourceLocation {
        id,
        source,
        range: span_range(span),
    });
    id
}

fn span_range(span: Span) -> Range<u32> {
    let range = span.byte_range();
    range.start.try_into().expect("source offset overflow")
        ..range.end.try_into().expect("source offset overflow")
}

fn path_is(path: &SynPath, expected: &[&str]) -> bool {
    path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected)
}

fn has_node_derive(attributes: &[Attribute]) -> bool {
    attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("derive"))
        .any(|attribute| {
            Punctuated::<SynPath, Token![,]>::parse_terminated
                .parse2(attribute.meta.require_list().map_or_else(
                    |_| proc_macro2::TokenStream::new(),
                    |list| list.tokens.clone(),
                ))
                .is_ok_and(|paths| paths.iter().any(|path| path_is(path, &["recursa", "Node"])))
        })
}

fn parse_lex(attribute: &Attribute) -> syn::Result<LitStr> {
    let mut pattern = None;
    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("pattern") {
            if pattern.is_some() {
                return Err(meta.error("duplicate lex pattern"));
            }
            pattern = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error("unsupported #[lex] option in ticket #53 lowering"))
        }
    })?;
    pattern.ok_or_else(|| syn::Error::new(attribute.span(), "#[lex] requires pattern = r\"...\""))
}

fn token_type_ident(ty: &Type) -> Result<syn::Ident, String> {
    let Type::Path(path) = ty else {
        return Err(
            "ticket #53 requires a direct token type with one lifetime argument".to_owned(),
        );
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return Err(
            "ticket #53 requires an unqualified direct token type; path resolution is handled by ticket #52"
                .to_owned(),
        );
    }
    let segment = &path.path.segments[0];
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(
            "ticket #53 requires a direct token type with one lifetime argument".to_owned(),
        );
    };
    if arguments.args.len() != 1 || !matches!(arguments.args[0], syn::GenericArgument::Lifetime(_))
    {
        return Err(
            "ticket #53 requires a direct token type with one lifetime argument; containers are handled by ticket #51"
                .to_owned(),
        );
    }
    Ok(segment.ident.clone())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        DiagnosticStage, RustEdition, SingleFileInput, analyze_single_file, lower_single_file,
    };
    use crate::{ContainerShape, TextEquivalence, TypeShape};

    const VALID_SOURCE: &str = r#"// λ keeps byte offsets honest.
recursa::grammar! {
    module = crate,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

#[derive(recursa::Node)]
struct Root<'input> {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub word: Word<'input>,
}
"#;

    fn valid_input() -> SingleFileInput<'static> {
        SingleFileInput {
            manifest_relative_path: Path::new("src/grammar.rs"),
            source: VALID_SOURCE,
            edition: RustEdition::Rust2024,
        }
    }

    #[test]
    fn lowers_one_source_into_validated_source_and_semantic_models() {
        let report = analyze_single_file(valid_input());

        assert!(report.diagnostics.is_empty());
        let analyzed = report.analyzed.expect("valid grammar is analyzed");
        let lowered = &analyzed.lowered;

        assert_eq!(lowered.build_inputs.discovery.source_files.len(), 1);
        assert_eq!(
            lowered.build_inputs.discovery.selected_root,
            Path::new("src/grammar.rs"),
        );
        assert_eq!(
            lowered.build_inputs.discovery.source_files,
            [Path::new("src/grammar.rs")],
        );
        assert_eq!(lowered.source_model.files.len(), 1);
        assert_eq!(lowered.source_model.modules.len(), 1);
        assert_eq!(lowered.source_model.nodes.len(), 1);
        assert_eq!(lowered.source_model.elements.len(), 1);
        assert_eq!(lowered.source_model.token_definitions.len(), 1);
        assert_eq!(lowered.source_model.token_uses.len(), 1);

        let file = &lowered.source_model.files[0];
        assert_eq!(file.path, Path::new("src/grammar.rs"));
        assert_eq!(file.text, VALID_SOURCE);
        let node = &lowered.source_model.nodes[0];
        assert_eq!(node.ident, "Root");
        assert_eq!(node.generics.lifetimes().count(), 1);
        let element = &lowered.source_model.elements[0];
        assert_eq!(element.ident.as_ref().expect("named field"), "word");
        assert!(matches!(element.visibility, syn::Visibility::Public(_)));
        let token = &lowered.source_model.token_definitions[0];
        assert_eq!(token.ident, "Word");
        assert_eq!(token.pattern.value(), "[A-Za-z_][A-Za-z0-9_]*");

        for location in &lowered.source_model.locations {
            let range = location.range.start as usize..location.range.end as usize;
            assert!(file.text.is_char_boundary(range.start));
            assert!(file.text.is_char_boundary(range.end));
            assert!(!file.text[range].is_empty());
        }
        let node_range = lowered.source_model.location(node.location).range.clone();
        assert_eq!(
            &file.text[node_range.start as usize..node_range.end as usize],
            "Root",
        );

        lowered
            .lowering_map
            .validate(&lowered.source_model, &lowered.grammar)
            .expect("all source/M2 correlations are total and bidirectional");
        assert_eq!(lowered.grammar.text_equivalence, TextEquivalence::Sensitive);
        assert_eq!(lowered.grammar.max_lookahead, 1);
        assert_eq!(lowered.grammar.kinds.len(), 1);
        assert_eq!(lowered.grammar.kinds[0].name, "Word");
        assert_eq!(lowered.grammar.types.len(), 2);
        assert!(matches!(
            lowered.grammar.types[0].shape,
            TypeShape::Token { .. }
        ));
        let TypeShape::Sequence { sites } = &lowered.grammar.types[1].shape else {
            panic!("Root is a sequence");
        };
        assert_eq!(sites.len(), 1);
        assert_eq!(
            lowered.grammar.sites[sites[0].index()].container,
            ContainerShape::Direct,
        );
        assert!(analyzed.analysis.diagnostics.is_empty());
        let output = analyzed.analysis.output.as_ref().expect("M2 facts");
        assert!(
            output
                .follow(crate::TypeId::new(1))
                .expect("Root facts")
                .eof()
        );
        assert!(output.follow_site(sites[0]).expect("field facts").eof());
    }

    #[test]
    fn canonical_projection_is_symbolic_and_byte_deterministic() {
        let first = analyze_single_file(valid_input());
        let repeated = analyze_single_file(valid_input());

        assert_eq!(first, repeated);
        let projection = first
            .analyzed
            .expect("valid grammar")
            .canonical_semantic_projection()
            .expect("analyzed grammar has M2 output");
        assert_eq!(
            projection,
            concat!(
                "grammar crate equivalence=sensitive lookahead=1\n",
                "token Word pattern=[A-Za-z_][A-Za-z0-9_]*\n",
                "node Root sequence=(Word)\n",
                "analysis Word FIRST={Word} NULLABLE=false\n",
                "analysis Root FIRST={Word} NULLABLE=false\n",
            ),
        );
        assert!(!projection.contains("type["));
        assert!(!projection.contains("src/grammar.rs"));
    }

    #[test]
    fn rust_syntax_failure_has_a_structured_diagnostic_and_no_aggregate() {
        let report = analyze_single_file(SingleFileInput {
            manifest_relative_path: Path::new("src/broken.rs"),
            source: "struct Root {",
            edition: RustEdition::Rust2024,
        });

        assert!(report.analyzed.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].stage, DiagnosticStage::Syntax);
        assert_eq!(report.diagnostics[0].path, Path::new("src/broken.rs"));
        assert!(report.diagnostics[0].range.is_some());
    }

    #[test]
    fn lowering_failure_has_a_structured_diagnostic_and_no_aggregate() {
        let duplicate_config = format!(
            "{}\nrecursa::grammar! {{ module = crate, keyword_matching = sensitive, max_lookahead = 1 }}",
            VALID_SOURCE,
        );
        let report = analyze_single_file(SingleFileInput {
            manifest_relative_path: Path::new("src/duplicate.rs"),
            source: &duplicate_config,
            edition: RustEdition::Rust2024,
        });

        assert!(report.analyzed.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].stage, DiagnosticStage::Lowering);
        assert!(report.diagnostics[0].message.contains("exactly one"));
        assert!(report.diagnostics[0].range.is_some());
    }

    #[test]
    fn unsupported_or_unresolved_field_types_fail_before_lowering() {
        for unsupported in ["Option<Word<'input>>", "crate::Word<'input>"] {
            let source = VALID_SOURCE.replace("Word<'input>", unsupported);
            let report = analyze_single_file(SingleFileInput {
                manifest_relative_path: Path::new("src/unsupported.rs"),
                source: &source,
                edition: RustEdition::Rust2024,
            });

            assert!(report.analyzed.is_none(), "accepted {unsupported}");
            assert_eq!(report.diagnostics.len(), 1);
            assert_eq!(report.diagnostics[0].stage, DiagnosticStage::Lowering);
            assert!(report.diagnostics[0].message.contains("direct token type"));
        }
    }

    #[test]
    fn validation_rejects_semantically_swapped_type_and_declaration_origins() {
        let mut lowered = lower_single_file(valid_input())
            .lowered
            .expect("valid lowering");
        let node_type = lowered.lowering_map.nodes[0].lowered;
        let token_type = lowered.lowering_map.token_definitions[0].ty;
        lowered.lowering_map.nodes[0].lowered = token_type;
        lowered.lowering_map.token_definitions[0].ty = node_type;
        assert!(
            lowered
                .lowering_map
                .validate(&lowered.source_model, &lowered.grammar)
                .is_err(),
        );

        let mut lowered = lower_single_file(valid_input())
            .lowered
            .expect("valid lowering");
        let token_declaration = lowered.lowering_map.declarations[0].lowered;
        lowered.lowering_map.declarations[0].lowered = lowered.lowering_map.declarations[1].lowered;
        lowered.lowering_map.declarations[1].lowered = token_declaration;
        assert!(
            lowered
                .lowering_map
                .validate(&lowered.source_model, &lowered.grammar)
                .is_err(),
        );
    }

    #[test]
    fn validation_rejects_corrupted_source_side_origins() {
        let mut lowered = lower_single_file(valid_input())
            .lowered
            .expect("valid lowering");
        lowered.source_model.declarations[0].kind =
            super::SourceDeclarationKind::Node(lowered.source_model.nodes[0].id);
        assert!(
            lowered
                .lowering_map
                .validate(&lowered.source_model, &lowered.grammar)
                .is_err(),
        );

        let mut lowered = lower_single_file(valid_input())
            .lowered
            .expect("valid lowering");
        lowered.source_model.nodes[0].location = lowered.source_model.token_definitions[0].location;
        assert!(
            lowered
                .lowering_map
                .validate(&lowered.source_model, &lowered.grammar)
                .is_err(),
        );

        let mut lowered = lower_single_file(valid_input())
            .lowered
            .expect("valid lowering");
        lowered.source_model.token_uses[0].location =
            super::SourceLocationId::next(lowered.source_model.locations.len());
        assert!(
            lowered
                .lowering_map
                .validate(&lowered.source_model, &lowered.grammar)
                .is_err(),
        );

        let mut lowered = lower_single_file(valid_input())
            .lowered
            .expect("valid lowering");
        lowered.source_model.nodes[0].elements.clear();
        assert!(
            lowered
                .lowering_map
                .validate(&lowered.source_model, &lowered.grammar)
                .is_err(),
        );
    }

    #[test]
    fn m2_failure_does_not_construct_an_analyzed_grammar() {
        let invalid_analysis = VALID_SOURCE.replace("max_lookahead = 1", "max_lookahead = 0");
        let report = analyze_single_file(SingleFileInput {
            manifest_relative_path: Path::new("src/invalid-analysis.rs"),
            source: &invalid_analysis,
            edition: RustEdition::Rust2024,
        });

        assert!(report.diagnostics.is_empty());
        assert_eq!(report.semantic_diagnostics.len(), 1);
        assert_eq!(
            report.semantic_diagnostics[0].code,
            crate::DiagnosticCode::InvalidGrammar,
        );
        assert!(report.analyzed.is_none());
    }
}
