#![allow(
    dead_code,
    reason = "private source analysis resolution seam is consumed by later lowering tickets"
)]
#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Anchored resolution of grammar-semantic source references.
//!
//! Resolution models the supported Rust module, import, re-export, alias, and
//! visibility rules from the selected-root anchor. It traces only references
//! that change grammar meaning; derive paths remain opaque Rust fragments for
//! faithful generation.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;

use syn::ext::IdentExt as _;
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::{
    Field, GenericArgument, Item, Meta, Path, PathArguments, Token, Type, UseTree, Visibility,
};

use crate::SourceId;
use crate::discovery::{DiscoveredModuleTree, ModuleInclusion};
use crate::lowering::{
    BuildInputs, DiscoveryInputs, FacadeMapping, ModuleId, NodeId, SelectedRootAnchor,
};
use crate::source_syntax::{
    CaptureMode, CargoCfgSnapshot, GrammarConfigSyntax, capture_override,
    configured_attribute_metas as configured_metas, item_attributes as item_attrs, path_text,
    span_range,
};

/// Derives the resolution id while resolving typed grammar-semantic references.
macro_rules! resolution_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(u32);

        impl $name {
            fn next(length: usize) -> Self {
                Self(length.try_into().expect("resolution identity overflow"))
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

resolution_id!(SemanticUseId);
resolution_id!(ResolutionTraceId);
resolution_id!(BindingId);
resolution_id!(GenericParameterId);
resolution_id!(ImportId);
resolution_id!(ContentTokenId);
resolution_id!(OpaqueTargetId);
resolution_id!(TypeAliasId);
resolution_id!(ResolvedFieldId);
resolution_id!(ResolvedVariantId);

#[derive(Clone, Debug)]
/// Carries the resolution input through grammar-semantic name resolution.
pub(crate) struct ResolutionInput<'cfg> {
    /// Supplies the discovery inputs needed either to resolve the reference or diagnose its failure.
    pub(crate) discovery_inputs: DiscoveryInputs,
    /// Supplies the tree needed either to resolve the reference or diagnose its failure.
    pub(crate) tree: DiscoveredModuleTree,
    /// Supplies the cfg needed either to resolve the reference or diagnose its failure.
    pub(crate) cfg: &'cfg CargoCfgSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolution report through grammar-semantic name resolution.
pub(crate) struct ResolutionReport {
    /// Supplies the discovery inputs needed either to resolve the reference or diagnose its failure.
    pub(crate) discovery_inputs: DiscoveryInputs,
    /// Supplies the build inputs needed either to resolve the reference or diagnose its failure.
    pub(crate) build_inputs: Option<BuildInputs>,
    /// Supplies the diagnostics needed either to resolve the reference or diagnose its failure.
    pub(crate) diagnostics: Vec<ResolutionDiagnostic>,
    /// Supplies the resolved needed either to resolve the reference or diagnose its failure.
    pub(crate) resolved: Option<ResolvedSourceGrammar>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved source grammar through grammar-semantic name resolution.
pub(crate) struct ResolvedSourceGrammar {
    /// Supplies the build inputs needed either to resolve the reference or diagnose its failure.
    pub(crate) build_inputs: BuildInputs,
    /// Supplies the configuration needed either to resolve the reference or diagnose its failure.
    pub(crate) configuration: SelectedGrammarConfiguration,
    /// Supplies the tree needed either to resolve the reference or diagnose its failure.
    pub(crate) tree: DiscoveredModuleTree,
    /// Supplies the nodes needed either to resolve the reference or diagnose its failure.
    pub(crate) nodes: Vec<ResolvedNode>,
    /// Supplies the type aliases needed either to resolve the reference or diagnose its failure.
    pub(crate) type_aliases: Vec<ResolvedTypeAlias>,
    /// Records the generic parameters visible while resolving this declaration.
    pub(crate) generic_parameters: Vec<ResolvedGenericParameter>,
    /// Supplies the content tokens needed either to resolve the reference or diagnose its failure.
    pub(crate) content_tokens: Vec<ResolvedContentToken>,
    /// Supplies the token bindings needed either to resolve the reference or diagnose its failure.
    pub(crate) token_bindings: Vec<TokenTypeBinding>,
    /// Supplies the imports needed either to resolve the reference or diagnose its failure.
    pub(crate) imports: Vec<ResolvedImport>,
    /// Supplies the uses needed either to resolve the reference or diagnose its failure.
    pub(crate) uses: Vec<SemanticUse>,
    /// Supplies the resolutions needed either to resolve the reference or diagnose its failure.
    pub(crate) resolutions: Vec<SemanticResolution>,
    /// Supplies the traces needed either to resolve the reference or diagnose its failure.
    pub(crate) traces: Vec<ResolutionTrace>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the selected grammar configuration through grammar-semantic name resolution.
pub(crate) struct SelectedGrammarConfiguration {
    /// Supplies the syntax needed either to resolve the reference or diagnose its failure.
    pub(crate) syntax: GrammarConfigSyntax,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the node key through grammar-semantic name resolution.
pub(crate) struct NodeKey {
    /// Supplies the segments needed either to resolve the reference or diagnose its failure.
    pub(crate) segments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved node through grammar-semantic name resolution.
pub(crate) struct ResolvedNode {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: NodeId,
    /// Identifies the logical module in which resolution occurs.
    pub(crate) module: ModuleId,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Preserves the authored identifier before terminal resolution.
    pub(crate) ident: String,
    /// Preserves authored visibility for accessibility checks.
    pub(crate) visibility: Visibility,
    /// Supplies the key needed either to resolve the reference or diagnose its failure.
    pub(crate) key: NodeKey,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Supplies the generics needed either to resolve the reference or diagnose its failure.
    pub(crate) generics: syn::Generics,
    /// Preserves parsed attributes for source-model construction.
    pub(crate) metas: Vec<Meta>,
    /// Preserves configured span-capture behavior across resolution.
    pub(crate) capture: Option<CaptureMode>,
    /// Supplies the body needed either to resolve the reference or diagnose its failure.
    pub(crate) body: ResolvedNodeBody,
    /// Supplies the fields needed either to resolve the reference or diagnose its failure.
    pub(crate) fields: Vec<ResolvedNodeField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the resolved node body alternatives handled by semantic resolution.
pub(crate) enum ResolvedNodeBody {
    /// Identifies the struct resolution alternative.
    Struct {
        #[doc = "Authored field style retained for the resolved struct body."]
        style: ResolvedFieldStyle,
    },
    /// Identifies the enum resolution alternative.
    Enum {
        #[doc = "Resolved variants in authored declaration order."]
        variants: Vec<ResolvedNodeVariant>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the resolved field style alternatives handled by semantic resolution.
pub(crate) enum ResolvedFieldStyle {
    /// Identifies the named resolution alternative.
    Named,
    /// Identifies the unnamed resolution alternative.
    Unnamed,
    /// Identifies the unit resolution alternative.
    Unit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved node variant through grammar-semantic name resolution.
pub(crate) struct ResolvedNodeVariant {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: ResolvedVariantId,
    /// Preserves the authored identifier before terminal resolution.
    pub(crate) ident: String,
    /// Supplies the style needed either to resolve the reference or diagnose its failure.
    pub(crate) style: ResolvedFieldStyle,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Preserves parsed attributes for source-model construction.
    pub(crate) metas: Vec<Meta>,
    /// Preserves configured span-capture behavior across resolution.
    pub(crate) capture: Option<CaptureMode>,
    /// Supplies the fields needed either to resolve the reference or diagnose its failure.
    pub(crate) fields: Vec<ResolvedFieldId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved node field through grammar-semantic name resolution.
pub(crate) struct ResolvedNodeField {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: ResolvedFieldId,
    /// Supplies the syntax needed either to resolve the reference or diagnose its failure.
    pub(crate) syntax: Field,
    /// Preserves parsed attributes for source-model construction.
    pub(crate) metas: Vec<Meta>,
    /// Preserves configured span-capture behavior across resolution.
    pub(crate) capture: Option<CaptureMode>,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Supplies the ty needed either to resolve the reference or diagnose its failure.
    pub(crate) ty: ResolvedType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved type through grammar-semantic name resolution.
pub(crate) struct ResolvedType {
    /// Supplies the syntax needed either to resolve the reference or diagnose its failure.
    pub(crate) syntax: Type,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Supplies the kind needed either to resolve the reference or diagnose its failure.
    pub(crate) kind: ResolvedTypeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the resolved type kind alternatives handled by semantic resolution.
pub(crate) enum ResolvedTypeKind {
    /// Identifies the semantic resolution alternative.
    Semantic {
        /// Supplies the use site needed either to resolve the reference or diagnose its failure.
        use_site: SemanticUseId,
        /// Supplies the target needed either to resolve the reference or diagnose its failure.
        target: SemanticTarget,
        /// Supplies the trace needed either to resolve the reference or diagnose its failure.
        trace: ResolutionTraceId,
        /// Supplies the arguments needed either to resolve the reference or diagnose its failure.
        arguments: Vec<ResolvedGenericArgument>,
    },
    /// Identifies the group resolution alternative.
    Group(Box<ResolvedType>),
    /// Identifies the parenthesized resolution alternative.
    Parenthesized(Box<ResolvedType>),
    /// Identifies the reference resolution alternative.
    Reference(Box<ResolvedType>),
    /// Identifies the tuple resolution alternative.
    Tuple(Vec<ResolvedType>),
    /// Identifies the unsupported resolution alternative.
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the resolved generic argument alternatives handled by semantic resolution.
pub(crate) enum ResolvedGenericArgument {
    /// Identifies the lifetime resolution alternative.
    Lifetime(syn::Lifetime),
    /// Identifies the type resolution alternative.
    Type(ResolvedType),
    /// Identifies the unsupported resolution alternative.
    Unsupported(GenericArgument),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved generic parameter through grammar-semantic name resolution.
pub(crate) struct ResolvedGenericParameter {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: GenericParameterId,
    /// Supplies the owner needed either to resolve the reference or diagnose its failure.
    pub(crate) owner: SemanticOwner,
    /// Preserves the authored identifier before terminal resolution.
    pub(crate) ident: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the semantic owner alternatives handled by semantic resolution.
pub(crate) enum SemanticOwner {
    /// Identifies the node resolution alternative.
    Node(NodeId),
    /// Identifies the type alias resolution alternative.
    TypeAlias(TypeAliasId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved type alias through grammar-semantic name resolution.
pub(crate) struct ResolvedTypeAlias {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: TypeAliasId,
    /// Identifies the logical module in which resolution occurs.
    pub(crate) module: ModuleId,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Preserves the authored identifier before terminal resolution.
    pub(crate) ident: String,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Supplies the generics needed either to resolve the reference or diagnose its failure.
    pub(crate) generics: syn::Generics,
    /// Supplies the ty needed either to resolve the reference or diagnose its failure.
    pub(crate) ty: Type,
    /// Supplies the target needed either to resolve the reference or diagnose its failure.
    pub(crate) target: Option<ResolvedType>,
    /// Supplies the defaults needed either to resolve the reference or diagnose its failure.
    pub(crate) defaults: Vec<ResolvedAliasDefault>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved alias default through grammar-semantic name resolution.
pub(crate) struct ResolvedAliasDefault {
    /// Supplies the parameter needed either to resolve the reference or diagnose its failure.
    pub(crate) parameter: GenericParameterId,
    /// Supplies the ty needed either to resolve the reference or diagnose its failure.
    pub(crate) ty: ResolvedType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved content token through grammar-semantic name resolution.
pub(crate) struct ResolvedContentToken {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: ContentTokenId,
    /// Preserves the authored identifier before terminal resolution.
    pub(crate) ident: String,
    /// Supplies the sites needed either to resolve the reference or diagnose its failure.
    pub(crate) sites: Vec<ContentTokenSite>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the content token site through grammar-semantic name resolution.
pub(crate) struct ContentTokenSite {
    /// Identifies the logical module in which resolution occurs.
    pub(crate) module: ModuleId,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Preserves authored visibility for accessibility checks.
    pub(crate) visibility: Visibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the token type binding through grammar-semantic name resolution.
pub(crate) struct TokenTypeBinding {
    /// Identifies the logical module in which resolution occurs.
    pub(crate) module: ModuleId,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Preserves the authored identifier before terminal resolution.
    pub(crate) ident: String,
    /// Supplies the target needed either to resolve the reference or diagnose its failure.
    pub(crate) target: ContentTokenId,
    /// Preserves authored visibility for accessibility checks.
    pub(crate) visibility: Visibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolved import through grammar-semantic name resolution.
pub(crate) struct ResolvedImport {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: ImportId,
    /// Identifies the logical module in which resolution occurs.
    pub(crate) module: ModuleId,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Supplies the authored needed either to resolve the reference or diagnose its failure.
    pub(crate) authored: String,
    /// Supplies the local name needed either to resolve the reference or diagnose its failure.
    pub(crate) local_name: Option<String>,
    /// Supplies the glob needed either to resolve the reference or diagnose its failure.
    pub(crate) glob: bool,
    /// Supplies the target needed either to resolve the reference or diagnose its failure.
    pub(crate) target: Option<ImportTarget>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the import target alternatives handled by semantic resolution.
pub(crate) enum ImportTarget {
    /// Identifies the module resolution alternative.
    Module(ModuleId),
    /// Identifies the node resolution alternative.
    Node(NodeId),
    /// Identifies the type alias resolution alternative.
    TypeAlias(TypeAliasId),
    /// Identifies the content token resolution alternative.
    ContentToken(ContentTokenId),
    /// Identifies the catalog resolution alternative.
    Catalog(CatalogType),
    /// Identifies the unsupported resolution alternative.
    Unsupported,
    /// Identifies the external resolution alternative.
    External,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the semantic use through grammar-semantic name resolution.
pub(crate) struct SemanticUse {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: SemanticUseId,
    /// Identifies the logical module in which resolution occurs.
    pub(crate) module: ModuleId,
    /// Supplies the owner needed either to resolve the reference or diagnose its failure.
    pub(crate) owner: SemanticOwner,
    /// Identifies the authored source file used for resolution diagnostics.
    pub(crate) source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Range<u32>,
    /// Supplies the authored needed either to resolve the reference or diagnose its failure.
    pub(crate) authored: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the semantic resolution through grammar-semantic name resolution.
pub(crate) struct SemanticResolution {
    /// Supplies the use site needed either to resolve the reference or diagnose its failure.
    pub(crate) use_site: SemanticUseId,
    /// Supplies the authored needed either to resolve the reference or diagnose its failure.
    pub(crate) authored: String,
    /// Supplies the target needed either to resolve the reference or diagnose its failure.
    pub(crate) target: SemanticTarget,
    /// Supplies the trace needed either to resolve the reference or diagnose its failure.
    pub(crate) trace: ResolutionTraceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the semantic target alternatives handled by semantic resolution.
pub(crate) enum SemanticTarget {
    /// Identifies the node resolution alternative.
    Node(NodeId),
    /// Identifies the type alias resolution alternative.
    TypeAlias(TypeAliasId),
    /// Identifies the generic parameter resolution alternative.
    GenericParameter(GenericParameterId),
    /// Identifies the content token resolution alternative.
    ContentToken(ContentTokenId),
    /// Identifies the catalog resolution alternative.
    Catalog(CatalogType),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the catalog type alternatives handled by semantic resolution.
pub(crate) enum CatalogType {
    /// Identifies the option resolution alternative.
    Option,
    /// Identifies the box resolution alternative.
    Box,
    /// Identifies the vec resolution alternative.
    Vec,
    /// Identifies the vec1 resolution alternative.
    Vec1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolution trace through grammar-semantic name resolution.
pub(crate) struct ResolutionTrace {
    /// Carries the grammar-local identity used by resolution tables.
    pub(crate) id: ResolutionTraceId,
    /// Records the canonical binding steps that form the resolution trace.
    pub(crate) steps: Vec<ResolutionTraceStep>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the resolution trace step alternatives handled by semantic resolution.
pub(crate) enum ResolutionTraceStep {
    /// Identifies the start resolution alternative.
    Start(ModuleId),
    /// Identifies the containing node resolution alternative.
    ContainingNode(NodeId),
    /// Identifies the current module resolution alternative.
    CurrentModule(ModuleId),
    /// Identifies the parent module resolution alternative.
    ParentModule(ModuleId),
    /// Identifies the anchor resolution alternative.
    Anchor(ModuleId),
    /// Identifies the module resolution alternative.
    Module(ModuleId),
    /// Identifies the import resolution alternative.
    Import(ImportId),
    /// Identifies the declaration resolution alternative.
    Declaration(NodeId),
    /// Identifies the type alias resolution alternative.
    TypeAlias(TypeAliasId),
    /// Identifies the token binding resolution alternative.
    TokenBinding(ContentTokenId),
    /// Identifies the generic parameter resolution alternative.
    GenericParameter(GenericParameterId),
    /// Identifies the prelude resolution alternative.
    Prelude(CatalogType),
    /// Identifies the catalog resolution alternative.
    Catalog(CatalogType),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the resolution diagnostic kind alternatives handled by semantic resolution.
pub(crate) enum ResolutionDiagnosticKind {
    /// Identifies the missing grammar config resolution alternative.
    MissingGrammarConfig,
    /// Identifies the multiple grammar configs resolution alternative.
    MultipleGrammarConfigs,
    /// Identifies the invalid grammar config resolution alternative.
    InvalidGrammarConfig,
    /// Identifies the invalid configuration resolution alternative.
    InvalidConfiguration,
    /// Identifies the duplicate declaration resolution alternative.
    DuplicateDeclaration,
    /// Identifies the duplicate explicit import resolution alternative.
    DuplicateExplicitImport,
    /// Identifies the import collision resolution alternative.
    ImportCollision,
    /// Identifies the ambiguous import resolution alternative.
    AmbiguousImport,
    /// Identifies the inaccessible reexport resolution alternative.
    InaccessibleReexport,
    /// Identifies the token binding collision resolution alternative.
    TokenBindingCollision,
    /// Identifies the inaccessible token binding resolution alternative.
    InaccessibleTokenBinding,
    /// Identifies the invalid visibility resolution alternative.
    InvalidVisibility,
    /// Identifies the unsupported declaration resolution alternative.
    UnsupportedDeclaration,
    /// Identifies the unresolved resolution alternative.
    Unresolved,
    /// Identifies the inaccessible resolution alternative.
    Inaccessible,
    /// Identifies the above anchor resolution alternative.
    AboveAnchor,
    /// Identifies the outside anchor resolution alternative.
    OutsideAnchor,
    /// Identifies the external path resolution alternative.
    ExternalPath,
    /// Identifies the leading external path resolution alternative.
    LeadingExternalPath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the resolution diagnostic through grammar-semantic name resolution.
pub(crate) struct ResolutionDiagnostic {
    /// Supplies the kind needed either to resolve the reference or diagnose its failure.
    pub(crate) kind: ResolutionDiagnosticKind,
    /// Identifies the logical module in which resolution occurs.
    pub(crate) module: Option<ModuleId>,
    /// Names the logical module used in deterministic diagnostics.
    pub(crate) logical_module: Option<Vec<String>>,
    /// Preserves the authored path whose terminal target is resolved.
    pub(crate) path: PathBuf,
    /// Locates the authored syntax used for resolution diagnostics.
    pub(crate) range: Option<Range<u32>>,
    /// Supplies the message needed either to resolve the reference or diagnose its failure.
    pub(crate) message: String,
}

#[derive(Clone, Debug)]
/// Carries the binding through grammar-semantic name resolution.
struct Binding {
    /// Carries the grammar-local identity used by resolution tables.
    id: BindingId,
    /// Identifies the logical module in which resolution occurs.
    module: ModuleId,
    /// Supplies the name needed either to resolve the reference or diagnose its failure.
    name: String,
    /// Supplies the kind needed either to resolve the reference or diagnose its failure.
    kind: BindingKind,
    /// Preserves authored visibility for accessibility checks.
    visibility: Visibility,
    /// Identifies the authored source file used for resolution diagnostics.
    source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    range: Range<u32>,
    /// Supplies the origin needed either to resolve the reference or diagnose its failure.
    origin: BindingOrigin,
    /// Supplies the import chain needed either to resolve the reference or diagnose its failure.
    import_chain: Vec<ImportId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the binding kind alternatives handled by semantic resolution.
enum BindingKind {
    /// Identifies the module resolution alternative.
    Module(ModuleId),
    /// Identifies the node resolution alternative.
    Node(NodeId),
    /// Identifies the type alias resolution alternative.
    TypeAlias(TypeAliasId),
    /// Identifies the content token resolution alternative.
    ContentToken(ContentTokenId),
    /// Identifies the catalog resolution alternative.
    Catalog(CatalogType),
    /// Identifies the unsupported resolution alternative.
    Unsupported(OpaqueTargetId),
    /// Identifies the external resolution alternative.
    External(OpaqueTargetId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the binding origin alternatives handled by semantic resolution.
enum BindingOrigin {
    /// Identifies the declaration resolution alternative.
    Declaration,
    /// Identifies the explicit import resolution alternative.
    ExplicitImport,
    /// Identifies the glob import resolution alternative.
    GlobImport,
    /// Identifies the content token resolution alternative.
    ContentToken,
}

#[derive(Clone, Debug)]
/// Carries the import directive through grammar-semantic name resolution.
struct ImportDirective {
    /// Carries the grammar-local identity used by resolution tables.
    id: ImportId,
    /// Identifies the logical module in which resolution occurs.
    module: ModuleId,
    /// Identifies the authored source file used for resolution diagnostics.
    source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    range: Range<u32>,
    /// Preserves authored visibility for accessibility checks.
    visibility: Visibility,
    /// Supplies the names needed either to resolve the reference or diagnose its failure.
    names: Vec<String>,
    /// Supplies the local name needed either to resolve the reference or diagnose its failure.
    local_name: Option<String>,
    /// Supplies the glob needed either to resolve the reference or diagnose its failure.
    glob: bool,
    /// Supplies the leading colon needed either to resolve the reference or diagnose its failure.
    leading_colon: bool,
}

/// Carries the import resolution through grammar-semantic name resolution.
struct ImportResolution {
    /// Supplies the kind needed either to resolve the reference or diagnose its failure.
    kind: BindingKind,
    /// Supplies the import chain needed either to resolve the reference or diagnose its failure.
    import_chain: Vec<ImportId>,
    /// Supplies the visibility scope needed either to resolve the reference or diagnose its failure.
    visibility_scope: ModuleId,
    /// Supplies the publicly exportable needed either to resolve the reference or diagnose its failure.
    publicly_exportable: bool,
}

/// Carries the selected path start through grammar-semantic name resolution.
struct SelectedPathStart {
    /// Identifies the logical module in which resolution occurs.
    module: ModuleId,
    /// Supplies the consumed needed either to resolve the reference or diagnose its failure.
    consumed: usize,
    /// Supplies the explicit local needed either to resolve the reference or diagnose its failure.
    explicit_local: bool,
    /// Records the canonical binding steps that form the resolution trace.
    steps: Vec<ResolutionTraceStep>,
}

#[derive(Clone, Copy)]
/// Classifies the binding walk step alternatives handled by semantic resolution.
enum BindingWalkStep {
    /// Identifies the import resolution alternative.
    Import(ImportId),
    /// Identifies the module resolution alternative.
    Module(ModuleId),
}

/// Carries the binding walk through grammar-semantic name resolution.
struct BindingWalk<'resolver> {
    /// Supplies the terminal needed either to resolve the reference or diagnose its failure.
    terminal: &'resolver Binding,
    /// Records the canonical binding steps that form the resolution trace.
    steps: Vec<BindingWalkStep>,
}

/// Classifies the binding walk error alternatives handled by semantic resolution.
enum BindingWalkError<'resolver> {
    /// Identifies the diagnostic resolution alternative.
    Diagnostic(ResolutionDiagnosticKind, String),
    /// Identifies the inaccessible resolution alternative.
    Inaccessible(&'resolver Binding),
    /// Identifies the non module resolution alternative.
    NonModule(&'resolver Binding),
    /// Identifies the missing child resolution alternative.
    MissingChild {
        /// Identifies the logical module in which resolution occurs.
        module: &'resolver Binding,
        /// Supplies the name needed either to resolve the reference or diagnose its failure.
        name: String,
    },
}

/// Carries the flattened use through grammar-semantic name resolution.
struct FlattenedUse {
    /// Supplies the names needed either to resolve the reference or diagnose its failure.
    names: Vec<String>,
    /// Supplies the local name needed either to resolve the reference or diagnose its failure.
    local_name: Option<String>,
    /// Supplies the glob needed either to resolve the reference or diagnose its failure.
    glob: bool,
    /// Locates the authored syntax used for resolution diagnostics.
    range: Range<u32>,
}

#[derive(Clone, Debug)]
/// Carries the pending content token site through grammar-semantic name resolution.
struct PendingContentTokenSite {
    /// Identifies the logical module in which resolution occurs.
    module: ModuleId,
    /// Identifies the authored source file used for resolution diagnostics.
    source: SourceId,
    /// Locates the authored syntax used for resolution diagnostics.
    range: Range<u32>,
    /// Preserves the authored identifier before terminal resolution.
    ident: String,
    /// Preserves authored visibility for accessibility checks.
    visibility: Visibility,
}

/// Carries the configured node field through grammar-semantic name resolution.
struct ConfiguredNodeField {
    /// Supplies the field needed either to resolve the reference or diagnose its failure.
    field: Field,
    /// Preserves parsed attributes for source-model construction.
    metas: Vec<Meta>,
    /// Preserves configured span-capture behavior across resolution.
    capture: Option<CaptureMode>,
}

/// Carries the configured node variant through grammar-semantic name resolution.
struct ConfiguredNodeVariant {
    /// Preserves the authored identifier before terminal resolution.
    ident: String,
    /// Supplies the style needed either to resolve the reference or diagnose its failure.
    style: ResolvedFieldStyle,
    /// Locates the authored syntax used for resolution diagnostics.
    range: Range<u32>,
    /// Preserves parsed attributes for source-model construction.
    metas: Vec<Meta>,
    /// Preserves configured span-capture behavior across resolution.
    capture: Option<CaptureMode>,
    /// Supplies the fields needed either to resolve the reference or diagnose its failure.
    fields: Vec<ConfiguredNodeField>,
}

/// Classifies the configured node body alternatives handled by semantic resolution.
enum ConfiguredNodeBody {
    /// Identifies the struct resolution alternative.
    Struct {
        /// Supplies the style needed either to resolve the reference or diagnose its failure.
        style: ResolvedFieldStyle,
        /// Supplies the fields needed either to resolve the reference or diagnose its failure.
        fields: Vec<ConfiguredNodeField>,
    },
    /// Identifies the enum resolution alternative.
    Enum {
        /// Supplies the variants needed either to resolve the reference or diagnose its failure.
        variants: Vec<ConfiguredNodeVariant>,
    },
}

#[derive(Clone, Debug)]
/// Carries the pending node through grammar-semantic name resolution.
struct PendingNode {
    /// Supplies the resolved needed either to resolve the reference or diagnose its failure.
    resolved: ResolvedNode,
    /// Records the generic parameters visible while resolving this declaration.
    generic_parameters: BTreeMap<String, GenericParameterId>,
    /// Supplies the fields needed either to resolve the reference or diagnose its failure.
    fields: Vec<PendingNodeField>,
}

#[derive(Clone, Debug)]
/// Carries the pending node field through grammar-semantic name resolution.
struct PendingNodeField {
    /// Carries the grammar-local identity used by resolution tables.
    id: ResolvedFieldId,
    /// Supplies the syntax needed either to resolve the reference or diagnose its failure.
    syntax: Field,
    /// Preserves parsed attributes for source-model construction.
    metas: Vec<Meta>,
    /// Preserves configured span-capture behavior across resolution.
    capture: Option<CaptureMode>,
    /// Locates the authored syntax used for resolution diagnostics.
    range: Range<u32>,
}

#[derive(Clone, Debug)]
/// Carries the pending type alias through grammar-semantic name resolution.
struct PendingTypeAlias {
    /// Supplies the resolved needed either to resolve the reference or diagnose its failure.
    resolved: ResolvedTypeAlias,
    /// Records the generic parameters visible while resolving this declaration.
    generic_parameters: BTreeMap<String, GenericParameterId>,
}

/// Carries the resolver through grammar-semantic name resolution.
struct Resolver<'cfg> {
    /// Supplies the cfg needed either to resolve the reference or diagnose its failure.
    cfg: &'cfg CargoCfgSnapshot,
    /// Supplies the build inputs needed either to resolve the reference or diagnose its failure.
    build_inputs: BuildInputs,
    /// Supplies the configuration needed either to resolve the reference or diagnose its failure.
    configuration: SelectedGrammarConfiguration,
    /// Supplies the tree needed either to resolve the reference or diagnose its failure.
    tree: DiscoveredModuleTree,
    /// Supplies the items needed either to resolve the reference or diagnose its failure.
    items: BTreeMap<ModuleId, Vec<Item>>,
    /// Supplies the bindings needed either to resolve the reference or diagnose its failure.
    bindings: Vec<Binding>,
    /// Supplies the binding by name needed either to resolve the reference or diagnose its failure.
    binding_by_name: BTreeMap<(ModuleId, String), BindingId>,
    /// Supplies the glob candidates needed either to resolve the reference or diagnose its failure.
    glob_candidates: BTreeMap<(ModuleId, String), Vec<Binding>>,
    /// Supplies the import directives needed either to resolve the reference or diagnose its failure.
    import_directives: Vec<ImportDirective>,
    /// Supplies the resolved imports needed either to resolve the reference or diagnose its failure.
    resolved_imports: Vec<ResolvedImport>,
    /// Supplies the content tokens needed either to resolve the reference or diagnose its failure.
    content_tokens: Vec<ResolvedContentToken>,
    /// Supplies the token bindings needed either to resolve the reference or diagnose its failure.
    token_bindings: Vec<TokenTypeBinding>,
    /// Supplies the pending content tokens needed either to resolve the reference or diagnose its failure.
    pending_content_tokens: Vec<PendingContentTokenSite>,
    /// Supplies the next opaque target needed either to resolve the reference or diagnose its failure.
    next_opaque_target: usize,
    /// Supplies the next resolved field needed either to resolve the reference or diagnose its failure.
    next_resolved_field: usize,
    /// Supplies the next resolved variant needed either to resolve the reference or diagnose its failure.
    next_resolved_variant: usize,
    /// Supplies the nodes needed either to resolve the reference or diagnose its failure.
    nodes: Vec<PendingNode>,
    /// Supplies the type aliases needed either to resolve the reference or diagnose its failure.
    type_aliases: Vec<PendingTypeAlias>,
    /// Records the generic parameters visible while resolving this declaration.
    generic_parameters: Vec<ResolvedGenericParameter>,
    /// Supplies the uses needed either to resolve the reference or diagnose its failure.
    uses: Vec<SemanticUse>,
    /// Supplies the resolutions needed either to resolve the reference or diagnose its failure.
    resolutions: Vec<SemanticResolution>,
    /// Supplies the traces needed either to resolve the reference or diagnose its failure.
    traces: Vec<ResolutionTrace>,
    /// Supplies the diagnostics needed either to resolve the reference or diagnose its failure.
    diagnostics: Vec<ResolutionDiagnostic>,
}

/// Resolves local semantics.
pub(crate) fn resolve_local_semantics(input: ResolutionInput<'_>) -> ResolutionReport {
    let discovery_inputs = input.discovery_inputs.clone();
    let items = input
        .tree
        .inclusions
        .iter()
        .map(|module| (module.id, module.items.clone()))
        .collect();
    let (config, mut diagnostics) = find_grammar_config(&input.tree, &items, input.cfg);
    let Some((anchor, facade, configuration)) = config else {
        return ResolutionReport {
            discovery_inputs,
            build_inputs: None,
            diagnostics,
            resolved: None,
        };
    };
    let build_inputs = BuildInputs {
        discovery: input.discovery_inputs,
        module_anchor: anchor.clone(),
        facade: facade.clone(),
        text_equivalence: configuration.syntax.text_equivalence,
        max_lookahead: configuration.syntax.max_lookahead,
        spans: configuration.syntax.spans,
        diagrams: configuration.syntax.diagrams,
        pretty: configuration.syntax.pretty,
    };
    let mut resolver = Resolver {
        cfg: input.cfg,
        build_inputs,
        configuration,
        tree: input.tree,
        items,
        bindings: Vec::new(),
        binding_by_name: BTreeMap::new(),
        glob_candidates: BTreeMap::new(),
        import_directives: Vec::new(),
        resolved_imports: Vec::new(),
        content_tokens: Vec::new(),
        token_bindings: Vec::new(),
        pending_content_tokens: Vec::new(),
        next_opaque_target: 0,
        next_resolved_field: 0,
        next_resolved_variant: 0,
        nodes: Vec::new(),
        type_aliases: Vec::new(),
        generic_parameters: Vec::new(),
        uses: Vec::new(),
        resolutions: Vec::new(),
        traces: Vec::new(),
        diagnostics: Vec::new(),
    };
    resolver.diagnostics.append(&mut diagnostics);
    resolver.build_symbols();
    resolver.build_content_token_bindings();
    resolver.resolve_imports();
    resolver.validate_visibility_scopes();
    resolver.resolve_fields();
    resolver.finish()
}

impl Resolver<'_> {
    /// Publishes a resolved grammar only when no blocking resolution diagnostics remain.
    fn finish(self) -> ResolutionReport {
        let resolved = self.diagnostics.is_empty().then(|| ResolvedSourceGrammar {
            build_inputs: self.build_inputs.clone(),
            configuration: self.configuration,
            tree: self.tree,
            nodes: self.nodes.into_iter().map(|node| node.resolved).collect(),
            type_aliases: self
                .type_aliases
                .into_iter()
                .map(|alias| alias.resolved)
                .collect(),
            generic_parameters: self.generic_parameters,
            content_tokens: self.content_tokens,
            token_bindings: self.token_bindings,
            imports: self.resolved_imports,
            uses: self.uses,
            resolutions: self.resolutions,
            traces: self.traces,
        });
        ResolutionReport {
            discovery_inputs: self.build_inputs.discovery.clone(),
            build_inputs: Some(self.build_inputs),
            diagnostics: self.diagnostics,
            resolved,
        }
    }

    /// Builds symbols.
    fn build_symbols(&mut self) {
        // Index declarations before resolving uses so module traversal order cannot change name lookup results.
        let inclusions = self.tree.inclusions.clone();
        for inclusion in inclusions.iter().filter(|module| module.parent.is_some()) {
            let parent = inclusion.parent.expect("filtered module has parent");
            let name = inclusion
                .logical_path
                .last()
                .expect("non-root module has a name")
                .clone();
            self.insert_binding(Binding {
                id: BindingId::next(self.bindings.len()),
                module: parent,
                name,
                kind: BindingKind::Module(inclusion.id),
                visibility: inclusion.visibility.clone(),
                source: inclusion.declaration_source,
                range: inclusion.declaration_range.clone(),
                origin: BindingOrigin::Declaration,
                import_chain: Vec::new(),
            });
        }

        for inclusion in inclusions {
            let Some(items) = self.items.get(&inclusion.id).cloned() else {
                continue;
            };
            for item in items {
                let configured = match configured_metas(item_attrs(&item), self.cfg) {
                    Ok(Some(metas)) => metas,
                    Ok(None) => continue,
                    Err(error) => {
                        self.push_site_diagnostic(
                            ResolutionDiagnosticKind::InvalidConfiguration,
                            inclusion.id,
                            inclusion.source,
                            Some(error.range),
                            error.message,
                        );
                        continue;
                    }
                };
                if let Item::Use(item_use) = &item {
                    let mut flattened = Vec::new();
                    flatten_use_tree(&item_use.tree, &mut Vec::new(), &mut flattened);
                    for flattened in flattened {
                        self.import_directives.push(ImportDirective {
                            id: ImportId::next(0),
                            module: inclusion.id,
                            source: inclusion.source,
                            range: flattened.range,
                            visibility: item_use.vis.clone(),
                            names: flattened.names,
                            local_name: flattened.local_name,
                            glob: flattened.glob,
                            leading_colon: item_use.leading_colon.is_some(),
                        });
                    }
                    continue;
                }
                let Some((ident, visibility, generics, may_be_node)) = item_declaration(&item)
                else {
                    if let Item::ExternCrate(item) = item {
                        let name = item
                            .rename
                            .as_ref()
                            .map_or_else(|| item.ident.unraw(), |(_, ident)| ident.unraw())
                            .to_string();
                        let binding_id = BindingId::next(self.bindings.len());
                        let target = self.allocate_opaque_target();
                        self.insert_binding(Binding {
                            id: binding_id,
                            module: inclusion.id,
                            name,
                            kind: BindingKind::External(target),
                            visibility: Visibility::Inherited,
                            source: inclusion.source,
                            range: span_range(item.ident.span()),
                            origin: BindingOrigin::Declaration,
                            import_chain: Vec::new(),
                        });
                    }
                    continue;
                };
                let name = ident.unraw().to_string();
                let is_node =
                    may_be_node && has_node_derive(&configured, &self.build_inputs.facade.ident);
                let is_type_alias = matches!(item, Item::Type(_));
                let binding_id = BindingId::next(self.bindings.len());
                let opaque_target =
                    (!is_node && !is_type_alias).then(|| self.allocate_opaque_target());
                let kind = if is_node {
                    let id = NodeId::next(self.nodes.len());
                    let capture = match capture_override(&configured) {
                        Ok(capture) => capture,
                        Err(error) => {
                            self.push_site_diagnostic(
                                ResolutionDiagnosticKind::InvalidConfiguration,
                                inclusion.id,
                                inclusion.source,
                                Some(span_range(error.span())),
                                error.to_string(),
                            );
                            continue;
                        }
                    };
                    let configured_body = configured_node_body(
                        &item,
                        self.cfg,
                        &mut self.diagnostics,
                        &self.tree,
                        inclusion.id,
                        inclusion.source,
                    );
                    let mut fields = Vec::new();
                    let body = match configured_body {
                        ConfiguredNodeBody::Struct {
                            style,
                            fields: configured_fields,
                        } => {
                            fields.extend(
                                configured_fields
                                    .into_iter()
                                    .map(|field| self.register_node_field(field)),
                            );
                            ResolvedNodeBody::Struct { style }
                        }
                        ConfiguredNodeBody::Enum { variants } => ResolvedNodeBody::Enum {
                            variants: variants
                                .into_iter()
                                .map(|variant| {
                                    let variant_id =
                                        ResolvedVariantId::next(self.next_resolved_variant);
                                    self.next_resolved_variant += 1;
                                    let field_ids = variant
                                        .fields
                                        .into_iter()
                                        .map(|field| {
                                            let field = self.register_node_field(field);
                                            let id = field.id;
                                            fields.push(field);
                                            id
                                        })
                                        .collect();
                                    ResolvedNodeVariant {
                                        id: variant_id,
                                        ident: variant.ident,
                                        style: variant.style,
                                        range: variant.range,
                                        metas: variant.metas,
                                        capture: variant.capture,
                                        fields: field_ids,
                                    }
                                })
                                .collect(),
                        },
                    };
                    let content_tokens = fields
                        .iter()
                        .filter_map(pending_content_token_site)
                        .map(|(ident, visibility, range)| PendingContentTokenSite {
                            module: inclusion.id,
                            source: inclusion.source,
                            range,
                            ident,
                            visibility,
                        })
                        .collect::<Vec<_>>();
                    let generic_parameters =
                        self.register_generic_parameters(&generics, SemanticOwner::Node(id));
                    let mut key = self.build_inputs.module_anchor.segments.clone();
                    key.extend(inclusion.logical_path.iter().cloned());
                    key.push(name.clone());
                    self.nodes.push(PendingNode {
                        resolved: ResolvedNode {
                            id,
                            module: inclusion.id,
                            source: inclusion.source,
                            ident: name.clone(),
                            visibility: visibility.clone(),
                            key: NodeKey { segments: key },
                            range: span_range(ident.span()),
                            generics,
                            metas: configured.clone(),
                            capture,
                            body,
                            fields: Vec::new(),
                        },
                        generic_parameters,
                        fields,
                    });
                    self.pending_content_tokens.extend(content_tokens);
                    BindingKind::Node(id)
                } else if let Item::Type(item_type) = &item {
                    let id = TypeAliasId::next(self.type_aliases.len());
                    let generic_parameters =
                        self.register_generic_parameters(&generics, SemanticOwner::TypeAlias(id));
                    self.type_aliases.push(PendingTypeAlias {
                        resolved: ResolvedTypeAlias {
                            id,
                            module: inclusion.id,
                            source: inclusion.source,
                            ident: name.clone(),
                            range: span_range(ident.span()),
                            generics,
                            ty: (*item_type.ty).clone(),
                            target: None,
                            defaults: Vec::new(),
                        },
                        generic_parameters,
                    });
                    BindingKind::TypeAlias(id)
                } else {
                    BindingKind::Unsupported(
                        opaque_target.expect("non-Node declarations have opaque identities"),
                    )
                };
                self.insert_binding(Binding {
                    id: binding_id,
                    module: inclusion.id,
                    name,
                    kind,
                    visibility,
                    source: inclusion.source,
                    range: span_range(ident.span()),
                    origin: BindingOrigin::Declaration,
                    import_chain: Vec::new(),
                });
            }
        }
    }

    /// Registers node field.
    fn register_node_field(&mut self, configured: ConfiguredNodeField) -> PendingNodeField {
        let id = ResolvedFieldId::next(self.next_resolved_field);
        self.next_resolved_field += 1;
        PendingNodeField {
            id,
            range: span_range(configured.field.ty.span()),
            syntax: configured.field,
            metas: configured.metas,
            capture: configured.capture,
        }
    }

    /// Registers generic parameters.
    fn register_generic_parameters(
        &mut self,
        generics: &syn::Generics,
        owner: SemanticOwner,
    ) -> BTreeMap<String, GenericParameterId> {
        generics
            .params
            .iter()
            .filter_map(|parameter| {
                let syn::GenericParam::Type(parameter) = parameter else {
                    return None;
                };
                let id = GenericParameterId::next(self.generic_parameters.len());
                let ident = parameter.ident.unraw().to_string();
                self.generic_parameters.push(ResolvedGenericParameter {
                    id,
                    owner,
                    ident: ident.clone(),
                });
                Some((ident, id))
            })
            .collect()
    }

    /// Inserts one type-namespace binding or diagnoses a duplicate declaration.
    fn insert_binding(&mut self, binding: Binding) {
        let key = (binding.module, binding.name.clone());
        if self.binding_by_name.contains_key(&key) {
            self.push_site_diagnostic(
                ResolutionDiagnosticKind::DuplicateDeclaration,
                binding.module,
                binding.source,
                Some(binding.range),
                format!(
                    "duplicate active type-namespace declaration `{}`",
                    binding.name
                ),
            );
            return;
        }
        self.binding_by_name.insert(key, binding.id);
        self.bindings.push(binding);
    }

    /// Allocates opaque target.
    fn allocate_opaque_target(&mut self) -> OpaqueTargetId {
        let id = OpaqueTargetId::next(self.next_opaque_target);
        self.next_opaque_target += 1;
        id
    }

    /// Builds content token bindings.
    fn build_content_token_bindings(&mut self) {
        // Synthesize type-namespace bindings only after duplicate token definitions have a single semantic identity.
        let logical_paths = self
            .tree
            .inclusions
            .iter()
            .map(|module| (module.id, module.logical_path.clone()))
            .collect::<BTreeMap<_, _>>();
        self.pending_content_tokens.sort_by(|left, right| {
            (
                left.ident.as_str(),
                logical_paths[&left.module].as_slice(),
                left.source,
                left.range.start,
            )
                .cmp(&(
                    right.ident.as_str(),
                    logical_paths[&right.module].as_slice(),
                    right.source,
                    right.range.start,
                ))
        });
        let mut token_by_name = BTreeMap::new();
        let mut binding_sites =
            BTreeMap::<(ModuleId, String), (ContentTokenId, PendingContentTokenSite)>::new();
        for site in self.pending_content_tokens.clone() {
            let token = *token_by_name.entry(site.ident.clone()).or_insert_with(|| {
                let id = ContentTokenId::next(self.content_tokens.len());
                self.content_tokens.push(ResolvedContentToken {
                    id,
                    ident: site.ident.clone(),
                    sites: Vec::new(),
                });
                id
            });
            self.content_tokens[token.index()]
                .sites
                .push(ContentTokenSite {
                    module: site.module,
                    source: site.source,
                    range: site.range.clone(),
                    visibility: site.visibility.clone(),
                });
            let key = (site.module, site.ident.clone());
            match binding_sites.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((token, site));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if self.visibility_is_broader(
                        site.module,
                        &site.visibility,
                        &entry.get().1.visibility,
                    ) {
                        entry.insert((token, site));
                    }
                }
            }
        }

        let mut binding_sites = binding_sites.into_values().collect::<Vec<_>>();
        binding_sites.sort_by(|(_, left), (_, right)| {
            (
                left.ident.as_str(),
                logical_paths[&left.module].as_slice(),
                left.source,
                left.range.start,
            )
                .cmp(&(
                    right.ident.as_str(),
                    logical_paths[&right.module].as_slice(),
                    right.source,
                    right.range.start,
                ))
        });
        for (token, site) in binding_sites {
            let binding = Binding {
                id: BindingId::next(self.bindings.len()),
                module: site.module,
                name: site.ident.clone(),
                kind: BindingKind::ContentToken(token),
                visibility: site.visibility.clone(),
                source: site.source,
                range: site.range.clone(),
                origin: BindingOrigin::ContentToken,
                import_chain: Vec::new(),
            };
            let key = (binding.module, binding.name.clone());
            if self.binding_by_name.contains_key(&key) {
                self.push_site_diagnostic(
                    ResolutionDiagnosticKind::TokenBindingCollision,
                    binding.module,
                    binding.source,
                    Some(binding.range),
                    format!(
                        "generated content-token binding `{}` collides with a source declaration",
                        binding.name
                    ),
                );
                continue;
            }
            self.token_bindings.push(TokenTypeBinding {
                module: site.module,
                source: site.source,
                range: site.range,
                ident: site.ident,
                target: token,
                visibility: site.visibility,
            });
            self.binding_by_name.insert(key, binding.id);
            self.bindings.push(binding);
        }
    }

    /// Derives the visibility is broader while resolving typed grammar-semantic references.
    fn visibility_is_broader(
        &self,
        module: ModuleId,
        candidate: &Visibility,
        current: &Visibility,
    ) -> bool {
        if matches!(candidate, Visibility::Public(_)) {
            return !matches!(current, Visibility::Public(_));
        }
        if matches!(current, Visibility::Public(_)) {
            return false;
        }
        match (
            self.declared_visibility_scope(module, candidate),
            self.declared_visibility_scope(module, current),
        ) {
            (Some(candidate), Some(current)) => {
                candidate != current && self.is_descendant(current, candidate)
            }
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Resolves imports.
    fn resolve_imports(&mut self) {
        self.prepare_import_directives();
        let diagnosed_duplicate_ids = self.diagnose_duplicate_explicit_imports();
        let complete = self.resolve_import_fixed_point(&diagnosed_duplicate_ids);
        self.revalidate_explicit_imports();
        self.diagnose_unresolved_imports(&complete);
    }

    /// Prepares import directives.
    fn prepare_import_directives(&mut self) {
        let logical_paths = self
            .tree
            .inclusions
            .iter()
            .map(|module| (module.id, module.logical_path.clone()))
            .collect::<BTreeMap<_, _>>();
        self.import_directives.sort_by(|left, right| {
            (
                &logical_paths[&left.module],
                left.local_name.as_deref(),
                left.glob,
                &left.names,
                left.source,
                left.range.start,
            )
                .cmp(&(
                    &logical_paths[&right.module],
                    right.local_name.as_deref(),
                    right.glob,
                    &right.names,
                    right.source,
                    right.range.start,
                ))
        });
        for (index, directive) in self.import_directives.iter_mut().enumerate() {
            directive.id = ImportId::next(index);
        }
        self.resolved_imports = self
            .import_directives
            .iter()
            .map(|directive| ResolvedImport {
                id: directive.id,
                module: directive.module,
                source: directive.source,
                range: directive.range.clone(),
                authored: import_text(directive),
                local_name: directive.local_name.clone(),
                glob: directive.glob,
                target: None,
            })
            .collect();
    }

    /// Diagnoses duplicate explicit imports.
    fn diagnose_duplicate_explicit_imports(&mut self) -> BTreeSet<ImportId> {
        let mut duplicate_imports = BTreeMap::<(ModuleId, String), Vec<ImportId>>::new();
        for directive in self
            .import_directives
            .iter()
            .filter(|directive| !directive.glob)
        {
            duplicate_imports
                .entry((
                    directive.module,
                    directive
                        .local_name
                        .clone()
                        .expect("named import has a local name"),
                ))
                .or_default()
                .push(directive.id);
        }
        let mut diagnosed_duplicate_ids = BTreeSet::new();
        for ((_, name), imports) in duplicate_imports {
            for import in imports.into_iter().skip(1) {
                diagnosed_duplicate_ids.insert(import);
                let directive = self.import_directives[import.index()].clone();
                self.push_site_diagnostic(
                    ResolutionDiagnosticKind::DuplicateExplicitImport,
                    directive.module,
                    directive.source,
                    Some(directive.range),
                    format!("repeated explicit import of semantic name `{name}`"),
                );
            }
        }

        diagnosed_duplicate_ids
    }

    /// Resolves import fixed point.
    fn resolve_import_fixed_point(
        &mut self,
        diagnosed_duplicate_ids: &BTreeSet<ImportId>,
    ) -> Vec<bool> {
        let mut complete = vec![false; self.import_directives.len()];
        let mut seeded_external_imports = false;
        loop {
            let mut progress = false;
            let directives = self.import_directives.clone();
            for directive in directives.iter().filter(|directive| !directive.glob) {
                progress |=
                    self.resolve_explicit_import(directive, &mut complete, diagnosed_duplicate_ids);
            }
            for directive in directives.iter().filter(|directive| directive.glob) {
                progress |= self.propagate_glob_import(directive, &mut complete);
            }
            if !progress && !seeded_external_imports {
                seeded_external_imports = true;
                progress |=
                    self.seed_external_imports(&directives, &mut complete, diagnosed_duplicate_ids);
            }
            if !progress {
                break;
            }
        }

        complete
    }

    /// Resolves explicit import.
    fn resolve_explicit_import(
        &mut self,
        directive: &ImportDirective,
        complete: &mut [bool],
        diagnosed_duplicate_ids: &BTreeSet<ImportId>,
    ) -> bool {
        if complete[directive.id.index()] {
            return false;
        }
        let Ok(import_resolution) = self.resolve_import_target(directive) else {
            return false;
        };
        let kind = import_resolution.kind;
        if matches!(kind, BindingKind::External(_))
            && !matches!(directive.visibility, Visibility::Inherited)
        {
            self.push_site_diagnostic(
                ResolutionDiagnosticKind::ExternalPath,
                directive.module,
                directive.source,
                Some(directive.range.clone()),
                "external re-export graphs are outside the closed semantic catalog".to_owned(),
            );
            self.resolved_imports[directive.id.index()].target = Some(ImportTarget::External);
            complete[directive.id.index()] = true;
            return true;
        }
        if self.import_visibility_exceeds_target(directive, &import_resolution) {
            self.push_site_diagnostic(
                ResolutionDiagnosticKind::InaccessibleReexport,
                directive.module,
                directive.source,
                Some(directive.range.clone()),
                "import visibility exceeds the terminal binding's visibility".to_owned(),
            );
            self.resolved_imports[directive.id.index()].target = Some(import_target(kind));
            complete[directive.id.index()] = true;
            return true;
        }

        let name = directive
            .local_name
            .clone()
            .expect("named import has a local name");
        let key = (directive.module, name.clone());
        if let Some(existing) = self.lookup_direct(directive.module, &name) {
            if !(existing.origin == BindingOrigin::ExplicitImport
                && diagnosed_duplicate_ids.contains(&directive.id))
            {
                self.push_import_collision(directive, existing.origin, &name);
            }
        } else {
            let mut import_chain = vec![directive.id];
            import_chain.extend(import_resolution.import_chain);
            let binding = Binding {
                id: BindingId::next(self.bindings.len()),
                module: directive.module,
                name,
                kind,
                visibility: directive.visibility.clone(),
                source: directive.source,
                range: directive.range.clone(),
                origin: BindingOrigin::ExplicitImport,
                import_chain,
            };
            self.binding_by_name.insert(key, binding.id);
            self.bindings.push(binding);
        }
        self.resolved_imports[directive.id.index()].target = Some(import_target(kind));
        complete[directive.id.index()] = true;
        true
    }

    /// Derives the import visibility exceeds target while resolving typed grammar-semantic references.
    fn import_visibility_exceeds_target(
        &self,
        directive: &ImportDirective,
        resolution: &ImportResolution,
    ) -> bool {
        self.declared_visibility_scope(directive.module, &directive.visibility)
            .is_some_and(|scope| !self.is_descendant(scope, resolution.visibility_scope))
            || matches!(directive.visibility, Visibility::Public(_))
                && !resolution.publicly_exportable
    }

    /// Propagates glob import.
    fn propagate_glob_import(
        &mut self,
        directive: &ImportDirective,
        complete: &mut [bool],
    ) -> bool {
        let Ok(import_resolution) = self.resolve_import_target(directive) else {
            return false;
        };
        let BindingKind::Module(target_module) = import_resolution.kind else {
            return false;
        };
        self.resolved_imports[directive.id.index()].target =
            Some(ImportTarget::Module(target_module));
        complete[directive.id.index()] = true;
        let candidates = self.module_binding_candidates(target_module);
        let glob_scope = self.declared_visibility_scope(directive.module, &directive.visibility);
        let mut progress = false;
        for candidate in candidates {
            if !self.glob_candidate_visible(directive, &candidate, glob_scope) {
                continue;
            }
            let mut import_chain = vec![directive.id];
            import_chain.extend(import_resolution.import_chain.iter().copied());
            import_chain.extend(candidate.import_chain.iter().copied());
            progress |= self.insert_glob_candidate(Binding {
                id: BindingId::next(0),
                module: directive.module,
                name: candidate.name,
                kind: candidate.kind,
                visibility: directive.visibility.clone(),
                source: directive.source,
                range: directive.range.clone(),
                origin: BindingOrigin::GlobImport,
                import_chain,
            });
        }
        progress
    }

    /// Derives the glob candidate visible while resolving typed grammar-semantic references.
    fn glob_candidate_visible(
        &self,
        directive: &ImportDirective,
        candidate: &Binding,
        glob_scope: Option<ModuleId>,
    ) -> bool {
        if !self.binding_visible(candidate, directive.module) {
            return false;
        }
        let Some(candidate_scope) = self.visibility_scope(candidate) else {
            return false;
        };
        glob_scope.is_some_and(|scope| self.is_descendant(scope, candidate_scope))
            && (!matches!(directive.visibility, Visibility::Public(_))
                || matches!(candidate.visibility, Visibility::Public(_)))
    }

    /// Seeds external imports.
    fn seed_external_imports(
        &mut self,
        directives: &[ImportDirective],
        complete: &mut [bool],
        diagnosed_duplicate_ids: &BTreeSet<ImportId>,
    ) -> bool {
        let unresolved = directives.iter().filter(|directive| {
            !directive.glob
                && !complete[directive.id.index()]
                && matches!(directive.visibility, Visibility::Inherited)
        });
        let mut progress = false;
        for directive in unresolved.cloned().collect::<Vec<_>>() {
            let error = self.resolve_import_target(&directive).err();
            if import_error_must_be_diagnosed(error.as_ref()) {
                continue;
            }
            let name = directive
                .local_name
                .clone()
                .expect("named import has a local name");
            if let Some(existing) = self.lookup_direct(directive.module, &name) {
                if !(existing.origin == BindingOrigin::ExplicitImport
                    && diagnosed_duplicate_ids.contains(&directive.id))
                {
                    self.push_import_collision(&directive, existing.origin, &name);
                }
            } else {
                let binding_id = BindingId::next(self.bindings.len());
                let target = self.allocate_opaque_target();
                let binding = Binding {
                    id: binding_id,
                    module: directive.module,
                    name: name.clone(),
                    kind: BindingKind::External(target),
                    visibility: directive.visibility.clone(),
                    source: directive.source,
                    range: directive.range.clone(),
                    origin: BindingOrigin::ExplicitImport,
                    import_chain: vec![directive.id],
                };
                self.binding_by_name
                    .insert((directive.module, name), binding.id);
                self.bindings.push(binding);
            }
            self.resolved_imports[directive.id.index()].target = Some(ImportTarget::External);
            complete[directive.id.index()] = true;
            progress = true;
        }
        progress
    }

    /// Classifies and records a collision with an existing imported binding.
    fn push_import_collision(
        &mut self,
        directive: &ImportDirective,
        existing_origin: BindingOrigin,
        name: &str,
    ) {
        let kind = match existing_origin {
            BindingOrigin::ContentToken => ResolutionDiagnosticKind::TokenBindingCollision,
            BindingOrigin::ExplicitImport => ResolutionDiagnosticKind::DuplicateExplicitImport,
            BindingOrigin::Declaration | BindingOrigin::GlobImport => {
                ResolutionDiagnosticKind::ImportCollision
            }
        };
        self.push_site_diagnostic(
            kind,
            directive.module,
            directive.source,
            Some(directive.range.clone()),
            format!("explicit import `{name}` collides with an existing binding"),
        );
    }

    /// Derives the revalidate explicit imports while resolving typed grammar-semantic references.
    fn revalidate_explicit_imports(&mut self) {
        let directives = self
            .import_directives
            .iter()
            .filter(|directive| !directive.glob)
            .cloned()
            .collect::<Vec<_>>();
        for directive in directives {
            let name = directive
                .local_name
                .as_deref()
                .expect("named import has a local name");
            let Some(binding) = self.lookup_direct(directive.module, name) else {
                continue;
            };
            if binding.origin != BindingOrigin::ExplicitImport
                || binding.import_chain.first() != Some(&directive.id)
            {
                continue;
            }
            if let Err((ResolutionDiagnosticKind::AmbiguousImport, message)) =
                self.resolve_import_target(&directive)
            {
                self.push_site_diagnostic(
                    ResolutionDiagnosticKind::AmbiguousImport,
                    directive.module,
                    directive.source,
                    Some(directive.range),
                    message,
                );
            }
        }
    }

    /// Diagnoses unresolved imports.
    fn diagnose_unresolved_imports(&mut self, complete: &[bool]) {
        let directives = self.import_directives.clone();
        for directive in directives {
            if complete[directive.id.index()] {
                continue;
            }
            if directive.glob {
                let (mut kind, mut message) = match self.resolve_import_target(&directive) {
                    Ok(ImportResolution {
                        kind: BindingKind::External(_),
                        ..
                    }) => (
                        ResolutionDiagnosticKind::ExternalPath,
                        "external glob imports are outside the closed semantic catalog".to_owned(),
                    ),
                    Ok(_) => (
                        ResolutionDiagnosticKind::UnsupportedDeclaration,
                        "glob import target is not a selected-tree module".to_owned(),
                    ),
                    Err(error) => error,
                };
                if kind == ResolutionDiagnosticKind::Unresolved {
                    kind = ResolutionDiagnosticKind::ExternalPath;
                    message = format!(
                        "external glob import `{}` is outside the closed semantic catalog",
                        directive.names.join("::")
                    );
                }
                self.push_site_diagnostic(
                    kind,
                    directive.module,
                    directive.source,
                    Some(directive.range),
                    message,
                );
                continue;
            }
            match self.resolve_import_target(&directive) {
                Err((ResolutionDiagnosticKind::Inaccessible, message)) => {
                    self.push_site_diagnostic(
                        ResolutionDiagnosticKind::InaccessibleReexport,
                        directive.module,
                        directive.source,
                        Some(directive.range),
                        message,
                    );
                }
                Err((ResolutionDiagnosticKind::InaccessibleTokenBinding, message)) => {
                    self.push_site_diagnostic(
                        ResolutionDiagnosticKind::InaccessibleTokenBinding,
                        directive.module,
                        directive.source,
                        Some(directive.range),
                        message,
                    );
                }
                Err((ResolutionDiagnosticKind::AmbiguousImport, message)) => {
                    self.push_site_diagnostic(
                        ResolutionDiagnosticKind::AmbiguousImport,
                        directive.module,
                        directive.source,
                        Some(directive.range),
                        message,
                    );
                }
                Err((kind, message)) if !matches!(directive.visibility, Visibility::Inherited) => {
                    self.push_site_diagnostic(
                        kind,
                        directive.module,
                        directive.source,
                        Some(directive.range),
                        message,
                    );
                }
                Err((kind, message)) => self.push_site_diagnostic(
                    kind,
                    directive.module,
                    directive.source,
                    Some(directive.range),
                    message,
                ),
                Ok(import_resolution) => self.push_site_diagnostic(
                    if matches!(import_resolution.kind, BindingKind::External(_)) {
                        ResolutionDiagnosticKind::ExternalPath
                    } else {
                        ResolutionDiagnosticKind::UnsupportedDeclaration
                    },
                    directive.module,
                    directive.source,
                    Some(directive.range),
                    "import did not produce a usable semantic binding".to_owned(),
                ),
            }
        }
    }

    /// Resolves import target.
    fn resolve_import_target(
        &self,
        directive: &ImportDirective,
    ) -> Result<ImportResolution, (ResolutionDiagnosticKind, String)> {
        let names = &directive.names;
        if names.is_empty() {
            return Err((
                ResolutionDiagnosticKind::Unresolved,
                "empty import path".to_owned(),
            ));
        }
        let start = self.selected_path_start(directive.module, names, directive.leading_colon)?;
        let module = start.module;
        let consumed = start.consumed;
        if consumed == names.len() {
            return Ok(ImportResolution {
                kind: BindingKind::Module(module),
                import_chain: Vec::new(),
                visibility_scope: self.module_visibility_scope(module),
                publicly_exportable: module == self.root_module()
                    || self.module_publicly_exportable(module),
            });
        }
        let Some(binding) =
            self.lookup_available_ignoring(module, &names[consumed], Some(directive.id))?
        else {
            if consumed == 0
                && let Some(catalog) = catalog_path(names, &self.build_inputs.facade.ident)
            {
                return Ok(ImportResolution {
                    kind: BindingKind::Catalog(catalog),
                    import_chain: Vec::new(),
                    visibility_scope: self.root_module(),
                    publicly_exportable: true,
                });
            }
            return Err(if consumed == 0 && names.len() > 1 {
                (
                    ResolutionDiagnosticKind::ExternalPath,
                    format!(
                        "external import `{}` is outside the closed semantic catalog",
                        names.join("::")
                    ),
                )
            } else {
                (
                    ResolutionDiagnosticKind::Unresolved,
                    format!("unresolved selected-tree import `{}`", names.join("::")),
                )
            });
        };
        self.resolve_import_from_binding(
            binding,
            names,
            consumed + 1,
            directive.module,
            directive.id,
        )
    }

    /// Derives the selected path start while resolving typed grammar-semantic references.
    fn selected_path_start(
        &self,
        use_module: ModuleId,
        names: &[String],
        leading_colon: bool,
    ) -> Result<SelectedPathStart, (ResolutionDiagnosticKind, String)> {
        if leading_colon {
            return Err((
                ResolutionDiagnosticKind::LeadingExternalPath,
                "leading `::` paths use the unsupported extern prelude".to_owned(),
            ));
        }
        let mut steps = vec![ResolutionTraceStep::Start(use_module)];
        match names[0].as_str() {
            "self" => {
                steps.push(ResolutionTraceStep::CurrentModule(use_module));
                Ok(SelectedPathStart {
                    module: use_module,
                    consumed: 1,
                    explicit_local: true,
                    steps,
                })
            }
            "super" => {
                let mut module = use_module;
                let mut consumed = 0;
                while names.get(consumed).is_some_and(|name| name == "super") {
                    let Some(parent) = self.module(module).parent else {
                        return Err((
                            ResolutionDiagnosticKind::AboveAnchor,
                            "`super` traverses above the selected root".to_owned(),
                        ));
                    };
                    module = parent;
                    steps.push(ResolutionTraceStep::ParentModule(module));
                    consumed += 1;
                }
                Ok(SelectedPathStart {
                    module,
                    consumed,
                    explicit_local: true,
                    steps,
                })
            }
            "crate" => {
                let anchor_end = 1 + self.build_inputs.module_anchor.segments.len();
                if names.len() < anchor_end
                    || names[1..anchor_end] != self.build_inputs.module_anchor.segments
                {
                    Err((
                        ResolutionDiagnosticKind::OutsideAnchor,
                        "crate-rooted path does not enter the selected anchor".to_owned(),
                    ))
                } else {
                    let module = self.root_module();
                    steps.push(ResolutionTraceStep::Anchor(module));
                    Ok(SelectedPathStart {
                        module,
                        consumed: anchor_end,
                        explicit_local: true,
                        steps,
                    })
                }
            }
            _ => Ok(SelectedPathStart {
                module: use_module,
                consumed: 0,
                explicit_local: false,
                steps,
            }),
        }
    }

    /// Resolves import from binding.
    fn resolve_import_from_binding<'resolver>(
        &'resolver self,
        binding: &'resolver Binding,
        names: &[String],
        next: usize,
        use_module: ModuleId,
        import: ImportId,
    ) -> Result<ImportResolution, (ResolutionDiagnosticKind, String)> {
        let walk = self
            .walk_binding_path(binding, names, next, use_module, Some(import))
            .map_err(|error| import_binding_walk_error(error, names))?;
        Ok(ImportResolution {
            kind: walk.terminal.kind,
            import_chain: walk
                .steps
                .into_iter()
                .filter_map(|step| match step {
                    BindingWalkStep::Import(import) => Some(import),
                    BindingWalkStep::Module(_) => None,
                })
                .collect(),
            visibility_scope: self
                .visibility_scope(walk.terminal)
                .expect("a visible binding has a valid visibility scope"),
            publicly_exportable: matches!(walk.terminal.visibility, Visibility::Public(_)),
        })
    }

    /// Walks binding path.
    fn walk_binding_path<'resolver>(
        &'resolver self,
        mut binding: &'resolver Binding,
        names: &[String],
        mut next: usize,
        use_module: ModuleId,
        ignored_import: Option<ImportId>,
    ) -> Result<BindingWalk<'resolver>, BindingWalkError<'resolver>> {
        let mut steps = Vec::new();
        loop {
            if !self.binding_visible(binding, use_module) {
                return Err(BindingWalkError::Inaccessible(binding));
            }
            steps.extend(
                binding
                    .import_chain
                    .iter()
                    .copied()
                    .map(BindingWalkStep::Import),
            );
            if next == names.len() {
                return Ok(BindingWalk {
                    terminal: binding,
                    steps,
                });
            }
            let BindingKind::Module(child) = binding.kind else {
                return Err(BindingWalkError::NonModule(binding));
            };
            steps.push(BindingWalkStep::Module(child));
            let next_name = &names[next];
            let next_binding = self
                .lookup_available_ignoring(child, next_name, ignored_import)
                .map_err(|(kind, message)| BindingWalkError::Diagnostic(kind, message))?;
            let Some(next_binding) = next_binding else {
                return Err(BindingWalkError::MissingChild {
                    module: binding,
                    name: next_name.clone(),
                });
            };
            binding = next_binding;
            next += 1;
        }
    }

    /// Looks up direct.
    fn lookup_direct(&self, module: ModuleId, name: &str) -> Option<&Binding> {
        self.binding_by_name
            .get(&(module, name.to_owned()))
            .map(|id| &self.bindings[id.index()])
    }

    /// Looks up available.
    fn lookup_available(
        &self,
        module: ModuleId,
        name: &str,
    ) -> Result<Option<&Binding>, (ResolutionDiagnosticKind, String)> {
        self.lookup_available_ignoring(module, name, None)
    }

    /// Looks up available ignoring.
    fn lookup_available_ignoring(
        &self,
        module: ModuleId,
        name: &str,
        ignored_import: Option<ImportId>,
    ) -> Result<Option<&Binding>, (ResolutionDiagnosticKind, String)> {
        if let Some(binding) = self.lookup_direct(module, name)
            && !ignored_import.is_some_and(|import| {
                binding.origin == BindingOrigin::ExplicitImport
                    && binding.import_chain.first() == Some(&import)
            })
        {
            return Ok(Some(binding));
        }
        let candidates = self
            .glob_candidates
            .get(&(module, name.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let distinct_targets = candidates
            .iter()
            .map(|candidate| candidate.kind)
            .collect::<BTreeSet<_>>();
        if distinct_targets.len() > 1 {
            return Err((
                ResolutionDiagnosticKind::AmbiguousImport,
                format!("semantic name `{name}` is ambiguous between glob imports"),
            ));
        }
        Ok(candidates.first())
    }

    /// Derives the module binding candidates while resolving typed grammar-semantic references.
    fn module_binding_candidates(&self, module: ModuleId) -> Vec<Binding> {
        let mut candidates = self
            .bindings
            .iter()
            .filter(|binding| binding.module == module)
            .cloned()
            .collect::<Vec<_>>();
        for ((candidate_module, _), bindings) in &self.glob_candidates {
            if *candidate_module == module {
                candidates.extend(bindings.iter().cloned());
            }
        }
        candidates.sort_by(|left, right| {
            (left.name.as_str(), left.kind, &left.import_chain).cmp(&(
                right.name.as_str(),
                right.kind,
                &right.import_chain,
            ))
        });
        candidates
    }

    /// Merges one glob-import candidate while retaining the broadest visibility path.
    fn insert_glob_candidate(&mut self, binding: Binding) -> bool {
        if self.lookup_direct(binding.module, &binding.name).is_some() {
            return false;
        }
        let key = (binding.module, binding.name.clone());
        let existing_index = self.glob_candidates.get(&key).and_then(|candidates| {
            candidates
                .iter()
                .position(|candidate| candidate.kind == binding.kind)
        });
        if let Some(existing_index) = existing_index {
            let existing = &self.glob_candidates[&key][existing_index];
            let new_visibility_is_broader = self.visibility_is_broader(
                binding.module,
                &binding.visibility,
                &existing.visibility,
            );
            let existing_visibility_is_broader = self.visibility_is_broader(
                binding.module,
                &existing.visibility,
                &binding.visibility,
            );
            let existing_rank = (existing.import_chain.len(), &existing.import_chain);
            let new_rank = (binding.import_chain.len(), &binding.import_chain);
            if new_visibility_is_broader
                || (!existing_visibility_is_broader && new_rank < existing_rank)
            {
                self.glob_candidates
                    .get_mut(&key)
                    .expect("the existing glob candidate key is present")[existing_index] = binding;
                return true;
            }
            return false;
        }
        let candidates = self.glob_candidates.entry(key).or_default();
        candidates.push(binding);
        candidates.sort_by_key(|candidate| candidate.kind);
        true
    }

    /// Validates visibility scopes.
    fn validate_visibility_scopes(&mut self) {
        let bindings = self.bindings.clone();
        for binding in bindings {
            if self.visibility_scope(&binding).is_none() {
                self.push_site_diagnostic(
                    ResolutionDiagnosticKind::InvalidVisibility,
                    binding.module,
                    binding.source,
                    Some(binding.range),
                    format!(
                        "visibility for `{}` is not an enclosing selected module",
                        binding.name
                    ),
                );
            }
        }
        let glob_imports = self
            .import_directives
            .iter()
            .filter(|directive| directive.glob)
            .cloned()
            .collect::<Vec<_>>();
        for directive in glob_imports {
            if self
                .declared_visibility_scope(directive.module, &directive.visibility)
                .is_none()
            {
                self.push_site_diagnostic(
                    ResolutionDiagnosticKind::InvalidVisibility,
                    directive.module,
                    directive.source,
                    Some(directive.range),
                    "glob import visibility is not an enclosing selected module".to_owned(),
                );
            }
        }
    }

    /// Resolves fields.
    fn resolve_fields(&mut self) {
        for node_index in 0..self.nodes.len() {
            let node = self.nodes[node_index].clone();
            let mut resolved_fields = Vec::new();
            for field in &node.fields {
                if let Some(ty) = self.resolve_type(
                    &field.syntax.ty,
                    node.resolved.module,
                    SemanticOwner::Node(node.resolved.id),
                    node.resolved.source,
                ) {
                    resolved_fields.push(ResolvedNodeField {
                        id: field.id,
                        syntax: field.syntax.clone(),
                        metas: field.metas.clone(),
                        capture: field.capture,
                        range: field.range.clone(),
                        ty,
                    });
                }
            }
            self.nodes[node_index].resolved.fields = resolved_fields;
        }
        let mut resolved_aliases = BTreeSet::new();
        let mut resolution_index = 0;
        while resolution_index < self.resolutions.len() {
            let target = self.resolutions[resolution_index].target;
            resolution_index += 1;
            let SemanticTarget::TypeAlias(alias_id) = target else {
                continue;
            };
            if !resolved_aliases.insert(alias_id) {
                continue;
            }
            let alias = self.type_aliases[alias_id.index()].clone();
            let target = self.resolve_type(
                &alias.resolved.ty,
                alias.resolved.module,
                SemanticOwner::TypeAlias(alias.resolved.id),
                alias.resolved.source,
            );
            self.type_aliases[alias_id.index()].resolved.target = target;
            let parameter_ids = alias.generic_parameters;
            let mut defaults = Vec::new();
            for parameter in &alias.resolved.generics.params {
                if let syn::GenericParam::Type(parameter) = parameter
                    && let Some(default) = &parameter.default
                    && let Some(ty) = self.resolve_type(
                        default,
                        alias.resolved.module,
                        SemanticOwner::TypeAlias(alias.resolved.id),
                        alias.resolved.source,
                    )
                {
                    defaults.push(ResolvedAliasDefault {
                        parameter: parameter_ids[&parameter.ident.unraw().to_string()],
                        ty,
                    });
                }
            }
            self.type_aliases[alias_id.index()].resolved.defaults = defaults;
        }
    }

    /// Resolves type.
    fn resolve_type(
        &mut self,
        ty: &Type,
        module: ModuleId,
        owner: SemanticOwner,
        source: SourceId,
    ) -> Option<ResolvedType> {
        // Resolve container layers and terminal grammar targets separately to retain an exact trace for diagnostics.
        let range = span_range(ty.span());
        match ty {
            Type::Path(type_path) => {
                let path = &type_path.path;
                let authored = path_text(path);
                let use_id = SemanticUseId::next(self.uses.len());
                let path_range = span_range(path.span());
                self.uses.push(SemanticUse {
                    id: use_id,
                    module,
                    owner,
                    source,
                    range: path_range.clone(),
                    authored: authored.clone(),
                });
                let outcome = if type_path.qself.is_some() {
                    Err((
                        ResolutionDiagnosticKind::ExternalPath,
                        "qualified semantic paths are outside the local resolver".to_owned(),
                    ))
                } else {
                    self.resolve_path(path, module, owner)
                };
                let resolved_target = match outcome {
                    Ok((target, steps)) => {
                        let trace = ResolutionTraceId::next(self.traces.len());
                        self.traces.push(ResolutionTrace { id: trace, steps });
                        self.resolutions.push(SemanticResolution {
                            use_site: use_id,
                            authored,
                            target,
                            trace,
                        });
                        Some((target, trace))
                    }
                    Err((kind, message)) => {
                        self.push_site_diagnostic(kind, module, source, Some(path_range), message);
                        None
                    }
                };
                let mut arguments = Vec::new();
                if let Some(segment) = path.segments.last()
                    && let PathArguments::AngleBracketed(authored_arguments) = &segment.arguments
                {
                    for argument in &authored_arguments.args {
                        arguments.push(match argument {
                            GenericArgument::Lifetime(lifetime) => {
                                ResolvedGenericArgument::Lifetime(lifetime.clone())
                            }
                            GenericArgument::Type(argument) => ResolvedGenericArgument::Type(
                                self.resolve_type(argument, module, owner, source)?,
                            ),
                            other => ResolvedGenericArgument::Unsupported(other.clone()),
                        });
                    }
                }
                let (target, trace) = resolved_target?;
                Some(ResolvedType {
                    syntax: ty.clone(),
                    source,
                    range,
                    kind: ResolvedTypeKind::Semantic {
                        use_site: use_id,
                        target,
                        trace,
                        arguments,
                    },
                })
            }
            Type::Group(group) => Some(ResolvedType {
                syntax: ty.clone(),
                source,
                range,
                kind: ResolvedTypeKind::Group(Box::new(self.resolve_type(
                    &group.elem,
                    module,
                    owner,
                    source,
                )?)),
            }),
            Type::Paren(paren) => Some(ResolvedType {
                syntax: ty.clone(),
                source,
                range,
                kind: ResolvedTypeKind::Parenthesized(Box::new(self.resolve_type(
                    &paren.elem,
                    module,
                    owner,
                    source,
                )?)),
            }),
            Type::Reference(reference) => Some(ResolvedType {
                syntax: ty.clone(),
                source,
                range,
                kind: ResolvedTypeKind::Reference(Box::new(self.resolve_type(
                    &reference.elem,
                    module,
                    owner,
                    source,
                )?)),
            }),
            Type::Tuple(tuple) => {
                let mut elements = Vec::new();
                for element in &tuple.elems {
                    elements.push(self.resolve_type(element, module, owner, source)?);
                }
                Some(ResolvedType {
                    syntax: ty.clone(),
                    source,
                    range,
                    kind: ResolvedTypeKind::Tuple(elements),
                })
            }
            _ => Some(ResolvedType {
                syntax: ty.clone(),
                source,
                range,
                kind: ResolvedTypeKind::Unsupported,
            }),
        }
    }

    /// Resolves path.
    fn resolve_path(
        &self,
        path: &Path,
        use_module: ModuleId,
        owner: SemanticOwner,
    ) -> Result<(SemanticTarget, Vec<ResolutionTraceStep>), (ResolutionDiagnosticKind, String)>
    {
        // Apply anchored Rust path rules segment by segment, recording the chosen import or re-export at each hop.
        let names = path
            .segments
            .iter()
            .map(|segment| segment.ident.unraw().to_string())
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Err((
                ResolutionDiagnosticKind::Unresolved,
                "empty semantic path".to_owned(),
            ));
        }
        let start = self.selected_path_start(use_module, &names, path.leading_colon.is_some())?;
        if names[0] == "Self" {
            if names.len() != 1 {
                return Err((
                    ResolutionDiagnosticKind::UnsupportedDeclaration,
                    "associated paths through `Self` are unsupported".to_owned(),
                ));
            }
            let SemanticOwner::Node(containing_node) = owner else {
                return Err((
                    ResolutionDiagnosticKind::UnsupportedDeclaration,
                    "`Self` is unsupported in a selected-tree type alias".to_owned(),
                ));
            };
            return Ok((
                SemanticTarget::Node(containing_node),
                vec![
                    ResolutionTraceStep::Start(use_module),
                    ResolutionTraceStep::ContainingNode(containing_node),
                ],
            ));
        }
        if !matches!(names[0].as_str(), "self" | "super" | "crate")
            && let Some(parameter) = self.generic_parameter(owner, &names[0])
        {
            if names.len() != 1 {
                return Err((
                    ResolutionDiagnosticKind::UnsupportedDeclaration,
                    "associated paths through a generic parameter are unsupported".to_owned(),
                ));
            }
            return Ok((
                SemanticTarget::GenericParameter(parameter),
                vec![
                    ResolutionTraceStep::Start(use_module),
                    ResolutionTraceStep::GenericParameter(parameter),
                ],
            ));
        }

        let mut steps = start.steps;
        let module = start.module;
        let consumed = start.consumed;
        let explicit_local = start.explicit_local;
        if consumed == names.len() {
            return Err((
                ResolutionDiagnosticKind::UnsupportedDeclaration,
                "a module cannot be used as a semantic type".to_owned(),
            ));
        }

        if !explicit_local {
            if let Some(binding) = self.lookup_available(module, &names[0])? {
                return self.resolve_from_binding(binding, &names, 1, use_module, steps);
            }
            if let Some(catalog) = catalog_path(&names, &self.build_inputs.facade.ident) {
                let step = if names.len() == 1 {
                    ResolutionTraceStep::Prelude(catalog)
                } else {
                    ResolutionTraceStep::Catalog(catalog)
                };
                steps.push(step);
                return Ok((SemanticTarget::Catalog(catalog), steps));
            }
            return Err(if names.len() > 1 {
                (
                    ResolutionDiagnosticKind::ExternalPath,
                    format!(
                        "external semantic path `{}` is outside the closed catalog",
                        path_text(path)
                    ),
                )
            } else {
                (
                    ResolutionDiagnosticKind::Unresolved,
                    format!("unresolved semantic name `{}`", names[0]),
                )
            });
        }

        let Some(binding) = self.lookup_available(module, &names[consumed])? else {
            return Err((
                ResolutionDiagnosticKind::Unresolved,
                format!("unresolved selected-tree path `{}`", path_text(path)),
            ));
        };
        self.resolve_from_binding(binding, &names, consumed + 1, use_module, steps)
    }

    /// Resolves from binding.
    fn resolve_from_binding<'resolver>(
        &'resolver self,
        binding: &'resolver Binding,
        names: &[String],
        next: usize,
        use_module: ModuleId,
        mut steps: Vec<ResolutionTraceStep>,
    ) -> Result<(SemanticTarget, Vec<ResolutionTraceStep>), (ResolutionDiagnosticKind, String)>
    {
        let walk = self
            .walk_binding_path(binding, names, next, use_module, None)
            .map_err(semantic_binding_walk_error)?;
        steps.extend(walk.steps.into_iter().map(|step| match step {
            BindingWalkStep::Import(import) => ResolutionTraceStep::Import(import),
            BindingWalkStep::Module(module) => ResolutionTraceStep::Module(module),
        }));
        match walk.terminal.kind {
            BindingKind::Node(node) => {
                steps.push(ResolutionTraceStep::Declaration(node));
                Ok((SemanticTarget::Node(node), steps))
            }
            BindingKind::TypeAlias(alias) => {
                steps.push(ResolutionTraceStep::TypeAlias(alias));
                Ok((SemanticTarget::TypeAlias(alias), steps))
            }
            BindingKind::ContentToken(token) => {
                steps.push(ResolutionTraceStep::TokenBinding(token));
                Ok((SemanticTarget::ContentToken(token), steps))
            }
            BindingKind::Catalog(catalog) => {
                steps.push(ResolutionTraceStep::Catalog(catalog));
                Ok((SemanticTarget::Catalog(catalog), steps))
            }
            BindingKind::External(_) => Err((
                ResolutionDiagnosticKind::ExternalPath,
                "extern-crate-dependent semantic lookup is unsupported".to_owned(),
            )),
            BindingKind::Module(_) | BindingKind::Unsupported(_) => Err((
                ResolutionDiagnosticKind::UnsupportedDeclaration,
                format!(
                    "`{}` is not a supported grammar-semantic declaration",
                    walk.terminal.name
                ),
            )),
        }
    }

    /// Derives the generic parameter while resolving typed grammar-semantic references.
    fn generic_parameter(&self, owner: SemanticOwner, name: &str) -> Option<GenericParameterId> {
        match owner {
            SemanticOwner::Node(node) => self.nodes[node.index()]
                .generic_parameters
                .get(name)
                .copied(),
            SemanticOwner::TypeAlias(alias) => self.type_aliases[alias.index()]
                .generic_parameters
                .get(name)
                .copied(),
        }
    }

    /// Derives the binding visible while resolving typed grammar-semantic references.
    fn binding_visible(&self, binding: &Binding, use_module: ModuleId) -> bool {
        self.visibility_scope(binding)
            .is_some_and(|scope| self.is_descendant(use_module, scope))
    }

    /// Derives the visibility scope while resolving typed grammar-semantic references.
    fn visibility_scope(&self, binding: &Binding) -> Option<ModuleId> {
        self.declared_visibility_scope(binding.module, &binding.visibility)
    }

    /// Derives the declared visibility scope while resolving typed grammar-semantic references.
    fn declared_visibility_scope(
        &self,
        declaring_module: ModuleId,
        visibility: &Visibility,
    ) -> Option<ModuleId> {
        match visibility {
            Visibility::Public(_) => Some(self.root_module()),
            Visibility::Inherited => Some(declaring_module),
            Visibility::Restricted(restricted) => {
                let names = restricted
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.unraw().to_string())
                    .collect::<Vec<_>>();
                if names == ["crate"] {
                    return Some(self.root_module());
                }
                if names == ["self"] {
                    return Some(declaring_module);
                }
                if names.iter().all(|name| name == "super") {
                    let logical = &self.module(declaring_module).logical_path;
                    let full_depth = self.build_inputs.module_anchor.segments.len() + logical.len();
                    if names.len() > full_depth {
                        return None;
                    }
                    let target_depth = full_depth - names.len();
                    if target_depth <= self.build_inputs.module_anchor.segments.len() {
                        return Some(self.root_module());
                    }
                    let logical_depth =
                        target_depth - self.build_inputs.module_anchor.segments.len();
                    return self.module_by_logical_path(&logical[..logical_depth]);
                }
                let scope = if names.first().is_some_and(|name| name == "crate") {
                    let restriction = &names[1..];
                    let mut declaring = self.build_inputs.module_anchor.segments.clone();
                    declaring.extend(self.module(declaring_module).logical_path.iter().cloned());
                    if !declaring.starts_with(restriction) {
                        return None;
                    }
                    if restriction.len() <= self.build_inputs.module_anchor.segments.len() {
                        self.root_module()
                    } else {
                        self.module_by_logical_path(
                            &restriction[self.build_inputs.module_anchor.segments.len()..],
                        )?
                    }
                } else {
                    return None;
                };
                self.is_descendant(declaring_module, scope).then_some(scope)
            }
        }
    }

    /// Derives the module visibility scope while resolving typed grammar-semantic references.
    fn module_visibility_scope(&self, module: ModuleId) -> ModuleId {
        if module == self.root_module() {
            return module;
        }
        self.bindings
            .iter()
            .find(|binding| binding.kind == BindingKind::Module(module))
            .and_then(|binding| self.visibility_scope(binding))
            .unwrap_or(module)
    }

    /// Derives the module publicly exportable while resolving typed grammar-semantic references.
    fn module_publicly_exportable(&self, module: ModuleId) -> bool {
        self.bindings.iter().any(|binding| {
            binding.kind == BindingKind::Module(module)
                && matches!(binding.visibility, Visibility::Public(_))
        })
    }

    /// Derives the root module while resolving typed grammar-semantic references.
    fn root_module(&self) -> ModuleId {
        self.tree
            .inclusions
            .iter()
            .find(|module| module.parent.is_none())
            .expect("discovered tree has a root")
            .id
    }

    /// Looks up a module inclusion by its traversal-assigned identity.
    fn module(&self, id: ModuleId) -> &ModuleInclusion {
        &self.tree.inclusions[id.index()]
    }

    /// Derives the module by logical path while resolving typed grammar-semantic references.
    fn module_by_logical_path(&self, path: &[String]) -> Option<ModuleId> {
        self.tree
            .inclusions
            .iter()
            .find(|module| module.logical_path == path)
            .map(|module| module.id)
    }

    /// Reports whether a logical module is nested within the proposed ancestor.
    fn is_descendant(&self, mut module: ModuleId, ancestor: ModuleId) -> bool {
        loop {
            if module == ancestor {
                return true;
            }
            let Some(parent) = self.module(module).parent else {
                return false;
            };
            module = parent;
        }
    }

    /// Adds a resolution diagnostic with its logical-module and source provenance.
    fn push_site_diagnostic(
        &mut self,
        kind: ResolutionDiagnosticKind,
        module: ModuleId,
        source: SourceId,
        range: Option<Range<u32>>,
        message: String,
    ) {
        self.diagnostics.push(ResolutionDiagnostic {
            kind,
            module: Some(module),
            logical_module: Some(self.tree.inclusions[module.index()].logical_path.clone()),
            path: self.tree.files[source.index()].path.clone(),
            range,
            message,
        });
    }
}

/// Finds grammar config.
fn find_grammar_config(
    tree: &DiscoveredModuleTree,
    items: &BTreeMap<ModuleId, Vec<Item>>,
    cfg: &CargoCfgSnapshot,
) -> (
    Option<(
        SelectedRootAnchor,
        FacadeMapping,
        SelectedGrammarConfiguration,
    )>,
    Vec<ResolutionDiagnostic>,
) {
    // Collect every candidate before selecting one so duplicate or malformed configuration is diagnosed deterministically.
    let mut diagnostics = Vec::new();
    let mut configurations = Vec::new();
    for inclusion in tree
        .inclusions
        .iter()
        .filter(|module| module.parent.is_none())
    {
        for item in &items[&inclusion.id] {
            let Item::Macro(item) = item else { continue };
            if item.mac.path.leading_colon.is_some()
                || item.mac.path.segments.len() != 2
                || item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_none_or(|segment| segment.ident != "grammar")
            {
                continue;
            }
            let configured = match configured_metas(&item.attrs, cfg) {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(error) => {
                    diagnostics.push(ResolutionDiagnostic {
                        kind: ResolutionDiagnosticKind::InvalidConfiguration,
                        module: Some(inclusion.id),
                        logical_module: Some(inclusion.logical_path.clone()),
                        path: tree.files[inclusion.source.index()].path.clone(),
                        range: Some(error.range),
                        message: error.message,
                    });
                    false
                }
            };
            if !configured {
                continue;
            }
            let macro_root = item.mac.path.segments[0].ident.unraw().to_string();
            match syn::parse2::<GrammarConfigSyntax>(item.mac.tokens.clone()) {
                Ok(syntax) if syntax.facade.unraw() == macro_root => {
                    configurations.push((inclusion, item, syntax));
                }
                Ok(syntax) if syntax.facade_explicit || macro_root == "recursa" => {
                    diagnostics.push(ResolutionDiagnostic {
                        kind: ResolutionDiagnosticKind::InvalidGrammarConfig,
                        module: Some(inclusion.id),
                        logical_module: Some(inclusion.logical_path.clone()),
                        path: tree.files[inclusion.source.index()].path.clone(),
                        range: Some(span_range(item.mac.path.span())),
                        message: "grammar invocation must use the configured facade mapping"
                            .to_owned(),
                    });
                }
                Ok(_) => {}
                Err(_) if macro_root != "recursa" => {}
                Err(error) => diagnostics.push(ResolutionDiagnostic {
                    kind: ResolutionDiagnosticKind::InvalidGrammarConfig,
                    module: Some(inclusion.id),
                    logical_module: Some(inclusion.logical_path.clone()),
                    path: tree.files[inclusion.source.index()].path.clone(),
                    range: Some(span_range(error.span())),
                    message: error.to_string(),
                }),
            }
        }
    }
    if configurations.is_empty() {
        if !diagnostics.is_empty() {
            return (None, diagnostics);
        }
        diagnostics.push(ResolutionDiagnostic {
            kind: ResolutionDiagnosticKind::MissingGrammarConfig,
            module: Some(tree.inclusions[0].id),
            logical_module: Some(tree.inclusions[0].logical_path.clone()),
            path: tree.files[tree.inclusions[0].source.index()].path.clone(),
            range: None,
            message: "expected one active grammar configuration".to_owned(),
        });
        return (None, diagnostics);
    }
    if configurations.len() > 1 {
        for (inclusion, item, _) in configurations.iter().skip(1) {
            diagnostics.push(ResolutionDiagnostic {
                kind: ResolutionDiagnosticKind::MultipleGrammarConfigs,
                module: Some(inclusion.id),
                logical_module: Some(inclusion.logical_path.clone()),
                path: tree.files[inclusion.source.index()].path.clone(),
                range: Some(span_range(item.mac.path.span())),
                message: "multiple active grammar configurations".to_owned(),
            });
        }
        return (None, diagnostics);
    }
    let (inclusion, item, syntax) = configurations
        .pop()
        .expect("exactly one grammar configuration remains");
    let anchor = match selected_anchor(&syntax.module) {
        Some(anchor) => anchor,
        None => {
            diagnostics.push(ResolutionDiagnostic {
                kind: ResolutionDiagnosticKind::InvalidGrammarConfig,
                module: Some(inclusion.id),
                logical_module: Some(inclusion.logical_path.clone()),
                path: tree.files[inclusion.source.index()].path.clone(),
                range: Some(span_range(syntax.module.span())),
                message: "module anchor must be `crate` or a path below `crate`".to_owned(),
            });
            return (None, diagnostics);
        }
    };
    let facade = syntax.facade.unraw().to_string();
    let configuration = SelectedGrammarConfiguration {
        syntax,
        source: inclusion.source,
        range: span_range(item.span()),
    };
    (
        Some((anchor, FacadeMapping { ident: facade }, configuration)),
        diagnostics,
    )
}

/// Derives the selected anchor while resolving typed grammar-semantic references.
fn selected_anchor(path: &Path) -> Option<SelectedRootAnchor> {
    if path.leading_colon.is_some()
        || path.segments.first()?.ident != "crate"
        || path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return None;
    }
    Some(SelectedRootAnchor {
        segments: path
            .segments
            .iter()
            .skip(1)
            .map(|segment| segment.ident.unraw().to_string())
            .collect(),
    })
}

/// Derives the item declaration while resolving typed grammar-semantic references.
fn item_declaration(item: &Item) -> Option<(syn::Ident, Visibility, syn::Generics, bool)> {
    match item {
        Item::Struct(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            true,
        )),
        Item::Enum(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            true,
        )),
        Item::Union(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        Item::Trait(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        Item::TraitAlias(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        Item::Type(item) => Some((
            item.ident.clone(),
            item.vis.clone(),
            item.generics.clone(),
            false,
        )),
        _ => None,
    }
}

/// Derives the configured node body while resolving typed grammar-semantic references.
fn configured_node_body(
    item: &Item,
    cfg: &CargoCfgSnapshot,
    diagnostics: &mut Vec<ResolutionDiagnostic>,
    tree: &DiscoveredModuleTree,
    module: ModuleId,
    source: SourceId,
) -> ConfiguredNodeBody {
    // Apply configuration after structural resolution; configuration may refine supported meaning but never invent a target.
    fn configured_fields(
        fields: impl IntoIterator<Item = Field>,
        cfg: &CargoCfgSnapshot,
        diagnostics: &mut Vec<ResolutionDiagnostic>,
        tree: &DiscoveredModuleTree,
        module: ModuleId,
        source: SourceId,
    ) -> Vec<ConfiguredNodeField> {
        fields
            .into_iter()
            .filter_map(|field| match configured_metas(&field.attrs, cfg) {
                Ok(Some(metas)) => match capture_override(&metas) {
                    Ok(capture) => Some(ConfiguredNodeField {
                        field,
                        metas,
                        capture,
                    }),
                    Err(error) => {
                        diagnostics.push(ResolutionDiagnostic {
                            kind: ResolutionDiagnosticKind::InvalidConfiguration,
                            module: Some(module),
                            logical_module: Some(
                                tree.inclusions[module.index()].logical_path.clone(),
                            ),
                            path: tree.files[source.index()].path.clone(),
                            range: Some(span_range(error.span())),
                            message: error.to_string(),
                        });
                        None
                    }
                },
                Ok(None) => None,
                Err(error) => {
                    diagnostics.push(ResolutionDiagnostic {
                        kind: ResolutionDiagnosticKind::InvalidConfiguration,
                        module: Some(module),
                        logical_module: Some(tree.inclusions[module.index()].logical_path.clone()),
                        path: tree.files[source.index()].path.clone(),
                        range: Some(error.range),
                        message: error.message,
                    });
                    None
                }
            })
            .collect()
    }

    match item {
        Item::Struct(item) => ConfiguredNodeBody::Struct {
            style: resolved_field_style(&item.fields),
            fields: configured_fields(
                item.fields.iter().cloned(),
                cfg,
                diagnostics,
                tree,
                module,
                source,
            ),
        },
        Item::Enum(item) => ConfiguredNodeBody::Enum {
            variants: item
                .variants
                .iter()
                .filter_map(|variant| match configured_metas(&variant.attrs, cfg) {
                    Ok(Some(metas)) => match capture_override(&metas) {
                        Ok(capture) => Some(ConfiguredNodeVariant {
                            ident: variant.ident.unraw().to_string(),
                            style: resolved_field_style(&variant.fields),
                            range: span_range(variant.ident.span()),
                            metas,
                            capture,
                            fields: configured_fields(
                                variant.fields.iter().cloned(),
                                cfg,
                                diagnostics,
                                tree,
                                module,
                                source,
                            ),
                        }),
                        Err(error) => {
                            diagnostics.push(ResolutionDiagnostic {
                                kind: ResolutionDiagnosticKind::InvalidConfiguration,
                                module: Some(module),
                                logical_module: Some(
                                    tree.inclusions[module.index()].logical_path.clone(),
                                ),
                                path: tree.files[source.index()].path.clone(),
                                range: Some(span_range(error.span())),
                                message: error.to_string(),
                            });
                            None
                        }
                    },
                    Ok(None) => None,
                    Err(error) => {
                        diagnostics.push(ResolutionDiagnostic {
                            kind: ResolutionDiagnosticKind::InvalidConfiguration,
                            module: Some(module),
                            logical_module: Some(
                                tree.inclusions[module.index()].logical_path.clone(),
                            ),
                            path: tree.files[source.index()].path.clone(),
                            range: Some(error.range),
                            message: error.message,
                        });
                        None
                    }
                })
                .collect(),
        },
        _ => unreachable!("only struct and enum Nodes reach configured_node_body"),
    }
}

/// Derives the resolved field style while resolving typed grammar-semantic references.
fn resolved_field_style(fields: &syn::Fields) -> ResolvedFieldStyle {
    match fields {
        syn::Fields::Named(_) => ResolvedFieldStyle::Named,
        syn::Fields::Unnamed(_) => ResolvedFieldStyle::Unnamed,
        syn::Fields::Unit => ResolvedFieldStyle::Unit,
    }
}

/// Derives the pending content token site while resolving typed grammar-semantic references.
fn pending_content_token_site(
    configured: &PendingNodeField,
) -> Option<(String, Visibility, Range<u32>)> {
    if !configured
        .metas
        .iter()
        .any(|meta| meta.path().is_ident("lex"))
    {
        return None;
    }
    let field = &configured.syntax;
    let Type::Path(path) = &field.ty else {
        return None;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    let arguments_are_content_lifetime = match &path.path.segments[0].arguments {
        PathArguments::None => true,
        PathArguments::AngleBracketed(arguments) => {
            arguments.args.len() == 1 && matches!(arguments.args[0], GenericArgument::Lifetime(_))
        }
        PathArguments::Parenthesized(_) => false,
    };
    if !arguments_are_content_lifetime {
        return None;
    }
    Some((
        path.path.segments[0].ident.unraw().to_string(),
        field.vis.clone(),
        span_range(field.ty.span()),
    ))
}

/// Flattens use tree.
fn flatten_use_tree(tree: &UseTree, prefix: &mut Vec<String>, output: &mut Vec<FlattenedUse>) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.unraw().to_string());
            flatten_use_tree(&path.tree, prefix, output);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let ident = name.ident.unraw().to_string();
            if ident == "self" {
                if let Some(local_name) = prefix.last().cloned() {
                    output.push(FlattenedUse {
                        names: prefix.clone(),
                        local_name: Some(local_name),
                        glob: false,
                        range: span_range(name.ident.span()),
                    });
                }
            } else {
                let mut names = prefix.clone();
                names.push(ident.clone());
                output.push(FlattenedUse {
                    names,
                    local_name: Some(ident),
                    glob: false,
                    range: span_range(name.ident.span()),
                });
            }
        }
        UseTree::Rename(rename) => {
            if rename.rename == "_" {
                return;
            }
            let ident = rename.ident.unraw().to_string();
            let mut names = prefix.clone();
            if ident != "self" {
                names.push(ident);
            }
            output.push(FlattenedUse {
                names,
                local_name: Some(rename.rename.unraw().to_string()),
                glob: false,
                range: span_range(rename.rename.span()),
            });
        }
        UseTree::Glob(glob) => output.push(FlattenedUse {
            names: prefix.clone(),
            local_name: None,
            glob: true,
            range: span_range(glob.star_token.span),
        }),
        UseTree::Group(group) => {
            for tree in &group.items {
                flatten_use_tree(tree, prefix, output);
            }
        }
    }
}

/// Derives the import target while resolving typed grammar-semantic references.
fn import_target(kind: BindingKind) -> ImportTarget {
    match kind {
        BindingKind::Module(module) => ImportTarget::Module(module),
        BindingKind::Node(node) => ImportTarget::Node(node),
        BindingKind::TypeAlias(alias) => ImportTarget::TypeAlias(alias),
        BindingKind::ContentToken(token) => ImportTarget::ContentToken(token),
        BindingKind::Catalog(catalog) => ImportTarget::Catalog(catalog),
        BindingKind::Unsupported(_) => ImportTarget::Unsupported,
        BindingKind::External(_) => ImportTarget::External,
    }
}

/// Derives the import error must be diagnosed while resolving typed grammar-semantic references.
fn import_error_must_be_diagnosed(error: Option<&(ResolutionDiagnosticKind, String)>) -> bool {
    error.is_some_and(|(kind, _)| {
        matches!(
            kind,
            ResolutionDiagnosticKind::Inaccessible
                | ResolutionDiagnosticKind::InaccessibleTokenBinding
                | ResolutionDiagnosticKind::AmbiguousImport
                | ResolutionDiagnosticKind::AboveAnchor
                | ResolutionDiagnosticKind::OutsideAnchor
        )
    })
}

/// Derives the inaccessible binding kind while resolving typed grammar-semantic references.
fn inaccessible_binding_kind(binding: &Binding) -> ResolutionDiagnosticKind {
    if binding.origin == BindingOrigin::ContentToken {
        ResolutionDiagnosticKind::InaccessibleTokenBinding
    } else {
        ResolutionDiagnosticKind::Inaccessible
    }
}

/// Derives the import binding walk error while resolving typed grammar-semantic references.
fn import_binding_walk_error(
    error: BindingWalkError<'_>,
    names: &[String],
) -> (ResolutionDiagnosticKind, String) {
    match error {
        BindingWalkError::Diagnostic(kind, message) => (kind, message),
        BindingWalkError::Inaccessible(binding) => (
            inaccessible_binding_kind(binding),
            format!("`{}` cannot be imported from this module", binding.name),
        ),
        BindingWalkError::NonModule(binding) => (
            ResolutionDiagnosticKind::UnsupportedDeclaration,
            format!("`{}` is not a selected-tree module", binding.name),
        ),
        BindingWalkError::MissingChild { .. } => (
            ResolutionDiagnosticKind::Unresolved,
            format!("unresolved selected-tree import `{}`", names.join("::")),
        ),
    }
}

/// Derives the semantic binding walk error while resolving typed grammar-semantic references.
fn semantic_binding_walk_error(error: BindingWalkError<'_>) -> (ResolutionDiagnosticKind, String) {
    match error {
        BindingWalkError::Diagnostic(kind, message) => (kind, message),
        BindingWalkError::Inaccessible(binding) => (
            inaccessible_binding_kind(binding),
            format!("`{}` is not visible from this logical module", binding.name),
        ),
        BindingWalkError::NonModule(binding) => (
            match binding.kind {
                BindingKind::External(_) => ResolutionDiagnosticKind::ExternalPath,
                _ => ResolutionDiagnosticKind::UnsupportedDeclaration,
            },
            format!("`{}` is not a selected-tree module", binding.name),
        ),
        BindingWalkError::MissingChild { module, name } => (
            ResolutionDiagnosticKind::Unresolved,
            format!("module `{}` has no semantic name `{name}`", module.name),
        ),
    }
}

/// Derives the import text while resolving typed grammar-semantic references.
fn import_text(directive: &ImportDirective) -> String {
    let mut authored = directive.names.join("::");
    if directive.leading_colon {
        authored.insert_str(0, "::");
    }
    if directive.glob {
        if !authored.is_empty() {
            authored.push_str("::");
        }
        authored.push('*');
    }
    authored
}

/// Reports whether an authored derive list delegates Node behavior to Recursa.
fn has_node_derive(metas: &[Meta], facade: &str) -> bool {
    metas
        .iter()
        .filter_map(|meta| {
            let Meta::List(list) = meta else { return None };
            list.path.is_ident("derive").then_some(list)
        })
        .any(|list| {
            Punctuated::<Path, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .is_ok_and(|paths| {
                    paths.iter().any(|path| {
                        path.leading_colon.is_none()
                            && path.segments.len() == 2
                            && path.segments[0].ident.unraw() == facade
                            && path.segments[1].ident == "Node"
                    })
                })
        })
}

/// Derives the catalog path while resolving typed grammar-semantic references.
fn catalog_path(names: &[String], facade: &str) -> Option<CatalogType> {
    match names {
        [name] if name == "Option" => Some(CatalogType::Option),
        [name] if name == "Box" => Some(CatalogType::Box),
        [name] if name == "Vec" => Some(CatalogType::Vec),
        [root, module, name]
            if (root == "core" || root == "std") && module == "option" && name == "Option" =>
        {
            Some(CatalogType::Option)
        }
        [root, module, name]
            if (root == "alloc" || root == "std") && module == "boxed" && name == "Box" =>
        {
            Some(CatalogType::Box)
        }
        [root, module, name]
            if (root == "alloc" || root == "std") && module == "vec" && name == "Vec" =>
        {
            Some(CatalogType::Vec)
        }
        [root, name] if root == facade && name == "Vec1" => Some(CatalogType::Vec1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::{
        CatalogType, ResolutionDiagnosticKind, ResolutionInput, ResolvedFieldStyle,
        ResolvedNodeBody, SemanticOwner, SemanticTarget, resolve_local_semantics,
    };
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::RustEdition;
    use crate::source_analysis::{SourceAnalysisInput, analyze_source_tree};
    use crate::source_syntax::CargoCfgSnapshot;

    fn write(package: &Path, relative: &str, source: &str) {
        let path = package.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file parent")).unwrap();
        fs::write(path, source).unwrap();
    }

    fn resolve_with_cfg(
        package: &TempDir,
        source: &str,
        cfg: &CargoCfgSnapshot,
    ) -> super::ResolutionReport {
        write(package.path(), "grammar/root.rs", source);
        let discovery = discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new("grammar/root.rs"),
            edition: RustEdition::Rust2024,
            cfg,
        });
        assert!(discovery.diagnostics.is_empty());
        resolve_local_semantics(ResolutionInput {
            discovery_inputs: discovery.discovery_inputs,
            tree: discovery.tree.unwrap(),
            cfg,
        })
    }

    fn resolve(package: &TempDir, source: &str) -> super::ResolutionReport {
        resolve_with_cfg(package, source, &CargoCfgSnapshot::default())
    }

    fn assert_source_analysis_codes(package: &TempDir, source: &str, expected: &[&str]) {
        write(package.path(), "grammar/root.rs", source);
        let mut cargo = Vec::new();
        let error = analyze_source_tree(
            SourceAnalysisInput::new(
                package.path(),
                Path::new("grammar/root.rs"),
                RustEdition::Rust2024,
                CargoCfgSnapshot::default(),
            ),
            &mut cargo,
        )
        .expect_err("fixture must fail before generation");
        assert_eq!(
            error
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code().as_str())
                .collect::<Vec<_>>(),
            expected,
        );
    }

    #[test]
    fn rca1015_reports_a_missing_grammar_configuration() {
        assert_source_analysis_codes(&TempDir::new().unwrap(), "struct Root;", &["RCA1015"]);
    }

    #[test]
    fn rca1016_reports_multiple_active_grammar_configurations() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            concat!(
                "recursa::grammar! { module = crate }\n",
                "recursa::grammar! { module = crate }\n",
            ),
            &["RCA1016"],
        );
    }

    #[test]
    fn rca1017_reports_an_invalid_grammar_configuration() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            "recursa::grammar! { module = self::sql }",
            &["RCA1017"],
        );
    }

    #[test]
    fn rca2002_reports_invalid_item_configuration() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            concat!(
                "recursa::grammar! { module = crate }\n",
                "#[cfg()] struct Root;\n",
            ),
            &["RCA2002"],
        );
    }

    #[test]
    fn rca2003_reports_duplicate_type_namespace_declarations() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            concat!(
                "recursa::grammar! { module = crate }\n",
                "struct Duplicate;\n",
                "struct Duplicate;\n",
            ),
            &["RCA2003"],
        );
    }

    #[test]
    fn rca2004_reports_a_duplicate_explicit_import() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] pub struct Leaf { this: Self } }
mod consumer {
    use super::source::Leaf;
    use super::source::Leaf;
    #[derive(recursa::Node)] struct Root { value: Leaf }
}
"#,
            &["RCA2004"],
        );
    }

    #[test]
    fn rca2005_reports_an_import_collision() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] pub struct Leaf { this: Self } }
mod consumer {
    use super::source::Leaf as Root;
    #[derive(recursa::Node)] struct Root { this: Self }
}
"#,
            &["RCA2005"],
        );
    }

    #[test]
    fn rca2006_reports_an_ambiguous_import_used_by_a_node() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod one { #[derive(recursa::Node)] pub struct Shared { this: Self } }
mod two { #[derive(recursa::Node)] pub struct Shared { this: Self } }
mod consumer {
    use super::one::*;
    use super::two::*;
    #[derive(recursa::Node)] struct Root { value: Shared }
}
"#,
            &["RCA2006"],
        );
    }

    #[test]
    fn rca2007_reports_an_inaccessible_reexport() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod parent {
    #[derive(recursa::Node)] struct Private { this: Self }
    pub mod exports { pub use super::Private; }
}
"#,
            &["RCA2007"],
        );
    }

    #[test]
    fn rca2008_reports_a_content_token_binding_collision() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct Ident { this: Self }
#[derive(recursa::Node)] struct Definition {
    #[lex(pattern = r".+")] value: Ident,
}
"#,
            &["RCA2008"],
        );
    }

    #[test]
    fn rca2009_reports_an_inaccessible_content_token_binding() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod tokens {
    #[derive(recursa::Node)]
    struct Definition { #[lex(pattern = r".+")] value: Ident }
}
mod consumer {
    #[derive(recursa::Node)] struct Root { value: super::tokens::Ident }
}
"#,
            &["RCA2009"],
        );
    }

    #[test]
    fn rca2010_reports_a_visibility_outside_the_selected_tree() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)]
pub(in crate::outside) struct Root { this: Self }
"#,
            &["RCA2010"],
        );
    }

    #[test]
    fn rca2011_reports_an_unsupported_local_declaration() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
struct Vec;
#[derive(recursa::Node)] struct Root { value: Vec }
"#,
            &["RCA2011"],
        );
    }

    #[test]
    fn rca2012_reports_an_unresolved_grammar_semantic_reference() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct Root { value: Missing }
"#,
            &["RCA2012"],
        );
    }

    #[test]
    fn rca2013_reports_an_inaccessible_local_declaration() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod left { #[derive(recursa::Node)] struct Private { this: Self } }
mod right {
    #[derive(recursa::Node)] struct Root { value: super::left::Private }
}
"#,
            &["RCA2013"],
        );
    }

    #[test]
    fn rca2014_reports_a_reference_above_the_selected_root_anchor() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct Root { value: super::Root }
"#,
            &["RCA2014"],
        );
    }

    #[test]
    fn rca2015_reports_a_crate_path_outside_the_selected_root_anchor() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct Root { value: crate::outside::Thing }
"#,
            &["RCA2015"],
        );
    }

    #[test]
    fn rca2016_reports_an_external_path() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct Root { value: dependency::Thing }
"#,
            &["RCA2016"],
        );
    }

    #[test]
    fn rca2017_reports_a_leading_external_path() {
        assert_source_analysis_codes(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct Root { value: ::std::vec::Vec }
"#,
            &["RCA2017"],
        );
    }

    #[test]
    fn resolves_anchored_local_paths_self_and_the_closed_catalog() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
my_recursa::grammar! {
    module = crate::sql,
    facade = my_recursa,
}

#[derive(my_recursa::Node)]
pub(super) struct Root {
    local: Root,
    relative: branch::Branch,
    explicit_self: self::branch::Branch,
    anchored: crate::sql::branch::Branch,
    optional: Option<branch::Branch>,
    boxed: std::boxed::Box<branch::Branch>,
    repeated: alloc::vec::Vec<branch::Branch>,
    nonempty: my_recursa::Vec1<branch::Branch>,
}

mod branch {
    #[derive(my_recursa::Node)]
    pub struct Branch {
        this: Self,
        root: super::Root,
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty());
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.build_inputs.module_anchor.segments, ["sql"]);
        assert_eq!(resolved.build_inputs.facade.ident, "my_recursa");
        assert_eq!(
            resolved
                .nodes
                .iter()
                .map(|node| node.key.segments.join("::"))
                .collect::<Vec<_>>(),
            ["sql::Root", "sql::branch::Branch"],
        );
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "Option"
                && resolution.target == SemanticTarget::Catalog(CatalogType::Option)
        }));
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "my_recursa::Vec1"
                && resolution.target == SemanticTarget::Catalog(CatalogType::Vec1)
        }));
        assert!(
            resolved
                .resolutions
                .iter()
                .all(|resolution| { !resolved.traces[resolution.trace.index()].steps.is_empty() })
        );
    }

    #[test]
    fn preserves_node_visibility_and_every_authored_field_style() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::grammar }

#[derive(recursa::Node)]
pub struct Named { value: Leaf }

#[derive(recursa::Node)]
pub(crate) struct Tuple(Leaf);

#[derive(recursa::Node)]
struct Unit;

#[derive(recursa::Node)]
pub enum Choice {
    Named { value: Leaf },
    Tuple(Leaf),
    EmptyNamed {},
    EmptyTuple(),
    Unit,
}

#[derive(recursa::Node)]
struct Leaf;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        let named = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Named")
            .unwrap();
        let tuple = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Tuple")
            .unwrap();
        let unit = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Unit")
            .unwrap();
        let choice = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "Choice")
            .unwrap();

        assert_eq!(named.visibility, syn::parse_quote!(pub));
        assert_eq!(tuple.visibility, syn::parse_quote!(pub(crate)));
        assert_eq!(unit.visibility, syn::Visibility::Inherited);
        assert_eq!(choice.visibility, syn::parse_quote!(pub));
        assert!(matches!(
            named.body,
            ResolvedNodeBody::Struct {
                style: ResolvedFieldStyle::Named
            }
        ));
        assert!(matches!(
            tuple.body,
            ResolvedNodeBody::Struct {
                style: ResolvedFieldStyle::Unnamed
            }
        ));
        assert!(matches!(
            unit.body,
            ResolvedNodeBody::Struct {
                style: ResolvedFieldStyle::Unit
            }
        ));

        let ResolvedNodeBody::Enum { variants } = &choice.body else {
            panic!("Choice remains an enum")
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| (variant.ident.as_str(), variant.style))
                .collect::<Vec<_>>(),
            [
                ("Named", ResolvedFieldStyle::Named),
                ("Tuple", ResolvedFieldStyle::Unnamed),
                ("EmptyNamed", ResolvedFieldStyle::Named),
                ("EmptyTuple", ResolvedFieldStyle::Unnamed),
                ("Unit", ResolvedFieldStyle::Unit),
            ]
        );
    }

    #[test]
    fn repeated_physical_sources_have_distinct_logical_node_keys() {
        let package = TempDir::new().unwrap();
        write(
            package.path(),
            "grammar/shared.rs",
            r#"
#[derive(recursa::Node)]
pub struct Shared { helper: Helper }
#[derive(recursa::Node)]
struct Helper { again: Self }
"#,
        );
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
#[path = "shared.rs"] mod left;
#[path = "shared.rs"] mod right;
#[derive(recursa::Node)]
struct Root { left: left::Shared, right: right::Shared }
"#,
        );

        assert!(report.diagnostics.is_empty());
        let resolved = report.resolved.unwrap();
        let shared = resolved
            .nodes
            .iter()
            .filter(|node| node.ident == "Shared")
            .collect::<Vec<_>>();
        assert_eq!(shared.len(), 2);
        assert_ne!(shared[0].id, shared[1].id);
        assert_ne!(shared[0].module, shared[1].module);
        assert_ne!(shared[0].key, shared[1].key);
        assert_eq!(
            resolved.tree.files[shared[0].source.index()].path,
            resolved.tree.files[shared[1].source.index()].path,
        );
    }

    #[test]
    fn resolves_restricted_visibility_raw_names_and_multiple_parent_steps() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }

#[derive(recursa::Node)]
struct Root { keyword: r#type::r#match }

mod r#type {
    #[derive(recursa::Node)]
    pub struct r#match { root: super::Root }
}

mod left {
    #[derive(recursa::Node)]
    pub(super) struct Shared { root: super::Root }

    pub mod nested {
        #[derive(recursa::Node)]
        pub(in crate::sql::left) struct Restricted {
            root: super::super::Root,
        }
    }

    #[derive(recursa::Node)]
    struct Left { restricted: self::nested::Restricted }
}

mod right {
    #[derive(recursa::Node)]
    struct Right { shared: super::left::Shared }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        assert!(resolved.nodes.iter().any(|node| {
            node.ident == "match" && node.key.segments.join("::") == "sql::type::match"
        }));
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "super::super::Root"
                && matches!(resolution.target, SemanticTarget::Node(_))
        }));
    }

    #[test]
    fn applies_configuration_to_nodes_variants_and_fields_before_resolution() {
        let package = TempDir::new().unwrap();
        let cfg = CargoCfgSnapshot::default().with_flag("unix");
        let report = resolve_with_cfg(
            &package,
            r#"
recursa::grammar! { module = crate }
#[cfg(windows)]
struct Option;

#[cfg_attr(unix, derive(recursa::Node))]
enum Root {
    Present(Option<Leaf>),
    #[cfg(windows)]
    Inactive(dep::Missing),
}

#[derive(recursa::Node)]
struct Leaf {
    #[cfg(unix)]
    this: Self,
    #[cfg(windows)]
    missing: dep::Missing,
}
"#,
            &cfg,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.nodes.len(), 2);
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "Option"
                && resolution.target == SemanticTarget::Catalog(CatalogType::Option)
        }));
        assert!(
            !resolved
                .uses
                .iter()
                .any(|use_site| use_site.authored.contains("dep"))
        );
    }

    #[test]
    fn enforces_visibility_from_sibling_logical_modules() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod left {
    #[derive(recursa::Node)]
    struct Private { this: Self }
    pub mod nested {
        #[derive(recursa::Node)]
        pub(in crate::sql::left) struct Restricted { this: Self }
    }
}
mod right {
    #[derive(recursa::Node)]
    struct Right {
        private: super::left::Private,
        restricted: super::left::nested::Restricted,
    }
}
"#,
        );

        assert!(report.resolved.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                ResolutionDiagnosticKind::Inaccessible,
                ResolutionDiagnosticKind::Inaccessible,
            ],
        );
    }

    #[test]
    fn rejects_shadowed_catalog_visibility_and_external_paths_before_lowering() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
struct Vec;
mod left {
    #[derive(recursa::Node)]
    struct Private { this: Self }
}
#[derive(recursa::Node)]
struct Root {
    shadowed: Vec,
    inaccessible: left::Private,
    above: super::Root,
    outside: crate::outside::Thing,
    external: dep::Thing,
    absolute: ::std::vec::Vec,
}
"#,
        );

        assert!(report.resolved.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                ResolutionDiagnosticKind::UnsupportedDeclaration,
                ResolutionDiagnosticKind::Inaccessible,
                ResolutionDiagnosticKind::AboveAnchor,
                ResolutionDiagnosticKind::OutsideAnchor,
                ResolutionDiagnosticKind::ExternalPath,
                ResolutionDiagnosticKind::LeadingExternalPath,
            ],
        );
        let build_inputs = report
            .build_inputs
            .expect("valid anchor facts survive semantic failure");
        assert_eq!(build_inputs.module_anchor.segments, ["sql"]);
        assert_eq!(build_inputs.facade.ident, "recursa");
        assert_eq!(
            report.discovery_inputs.selected_root,
            PathBuf::from("grammar/root.rs")
        );
    }

    #[test]
    fn generic_parameters_shadow_catalog_names_and_keep_typed_traces() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] pub struct Leaf { this: Self } }
use self::source::Leaf as Vec;
type Identity<T> = T;
#[derive(recursa::Node)]
struct Root<Vec> { value: Vec }
"#,
        );

        assert!(report.diagnostics.is_empty());
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.generic_parameters.len(), 2);
        let node_parameter = resolved
            .generic_parameters
            .iter()
            .find(|parameter| matches!(parameter.owner, SemanticOwner::Node(_)))
            .unwrap();
        let alias_parameter = resolved
            .generic_parameters
            .iter()
            .find(|parameter| matches!(parameter.owner, SemanticOwner::TypeAlias(_)))
            .unwrap();
        assert!(matches!(
            node_parameter.owner,
            SemanticOwner::Node(node) if resolved.nodes[node.index()].ident == "Root"
        ));
        assert!(matches!(
            alias_parameter.owner,
            SemanticOwner::TypeAlias(alias)
                if resolved.type_aliases[alias.index()].ident == "Identity"
        ));
        assert_eq!(
            resolved.resolutions[0].target,
            SemanticTarget::GenericParameter(node_parameter.id),
        );
    }

    #[test]
    fn resolves_grouped_aliased_and_self_leaf_imports() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }

mod model {
    #[derive(recursa::Node)]
    pub struct Leaf { this: Self }
    #[derive(recursa::Node)]
    pub struct Branch { leaf: Leaf }
}

mod consumer {
    use super::model::{self as types, Leaf as ImportedLeaf};

    #[derive(recursa::Node)]
    struct Root {
        direct: ImportedLeaf,
        qualified: types::Branch,
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        let target_names = resolved
            .resolutions
            .iter()
            .filter(|resolution| {
                matches!(
                    resolution.authored.as_str(),
                    "ImportedLeaf" | "types::Branch"
                )
            })
            .map(|resolution| match resolution.target {
                SemanticTarget::Node(node) => resolved.nodes[node.index()].ident.as_str(),
                _ => panic!("import must resolve to a Node"),
            })
            .collect::<Vec<_>>();
        assert_eq!(target_names, ["Leaf", "Branch"]);
    }

    #[test]
    fn configures_use_items_before_building_the_import_graph() {
        let package = TempDir::new().unwrap();
        let cfg = CargoCfgSnapshot::default().with_flag("unix");
        let report = resolve_with_cfg(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
#[cfg(unix)] use std::option::Option as Wrapper;
#[cfg(windows)] use std::vec::Vec as Wrapper;
#[derive(recursa::Node)] struct Leaf { this: Self }
#[derive(recursa::Node)] struct Root { value: Wrapper<Leaf> }
"#,
            &cfg,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.imports.len(), 1);
        assert_eq!(resolved.imports[0].authored, "std::option::Option");
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "Wrapper"
                && resolution.target == SemanticTarget::Catalog(CatalogType::Option)
        }));
    }

    #[test]
    fn resolves_selected_tree_reexports_and_globs_to_a_fixed_point() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }

mod consumer {
    use super::second::Leaf;
    #[derive(recursa::Node)]
    struct Root { value: Leaf }
}

mod second {
    pub use super::first::*;
}

mod first {
    pub use super::origin::Leaf;
}

mod origin {
    #[derive(recursa::Node)]
    pub struct Leaf { this: Self }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        let resolution = resolved
            .resolutions
            .iter()
            .find(|resolution| resolution.authored == "Leaf")
            .unwrap();
        assert!(matches!(resolution.target, SemanticTarget::Node(_)));
        assert_eq!(
            resolved.traces[resolution.trace.index()]
                .steps
                .iter()
                .filter(|step| matches!(step, super::ResolutionTraceStep::Import(_)))
                .count(),
            3,
        );
    }

    #[test]
    fn rechecks_explicit_imports_after_transitive_glob_closure() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod a { #[derive(recursa::Node)] pub struct X { this: Self } }
mod b { #[derive(recursa::Node)] pub struct X { this: Self } }
mod consumer {
    use super::exports::X;
    #[derive(recursa::Node)] struct Root { value: X }
}
mod exports {
    pub use super::a::*;
    pub use super::late::*;
}
mod late { pub use super::b::*; }
"#,
        );

        assert!(report.resolved.is_none());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == ResolutionDiagnosticKind::AmbiguousImport
                && diagnostic.range.is_some()
                && diagnostic.message.contains("glob imports")
        }));
    }

    #[test]
    fn filters_globs_applies_precedence_and_reports_only_used_ambiguity() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod one {
    #[derive(recursa::Node)] pub struct Shared { this: Self }
    #[derive(recursa::Node)] struct Private { this: Self }
}
mod two {
    #[derive(recursa::Node)] pub struct Shared { this: Self }
}
mod unused_ambiguity {
    use super::one::*;
    use super::two::*;
    use super::one::Shared;
    #[derive(recursa::Node)] struct Root { chosen: Shared }
}
"#,
        );
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);

        let ambiguous = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod one { #[derive(recursa::Node)] pub struct Shared { this: Self } }
mod two { #[derive(recursa::Node)] pub struct Shared { this: Self } }
mod consumer {
    use super::one::*;
    use super::two::*;
    #[derive(recursa::Node)] struct Root { value: Shared }
}
"#,
        );
        assert_eq!(
            ambiguous
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [ResolutionDiagnosticKind::AmbiguousImport],
        );

        let filtered = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] struct Private { this: Self } }
mod consumer {
    use super::source::*;
    #[derive(recursa::Node)] struct Root { value: Private }
}
"#,
        );
        assert_eq!(
            filtered.diagnostics[0].kind,
            ResolutionDiagnosticKind::Unresolved,
        );
    }

    #[test]
    fn reexports_cannot_widen_private_bindings_and_public_globs_filter_them() {
        let explicit = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod parent {
    #[derive(recursa::Node)] struct Private { this: Self }
    pub mod exports { pub use super::Private; }
}
"#,
        );
        assert_eq!(
            explicit.diagnostics[0].kind,
            ResolutionDiagnosticKind::InaccessibleReexport,
        );

        let glob = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod parent {
    #[derive(recursa::Node)] struct Private { this: Self }
    #[derive(recursa::Node)] pub struct Public { this: Self }
    pub mod exports { pub use super::*; }
}
#[derive(recursa::Node)]
struct Root {
    visible: parent::exports::Public,
    filtered: parent::exports::Private,
}
"#,
        );
        assert_eq!(
            glob.diagnostics[0].kind,
            ResolutionDiagnosticKind::Unresolved,
        );
        assert!(glob.diagnostics[0].message.contains("Private"));
    }

    #[test]
    fn same_target_globs_prefer_the_publicly_reexportable_route() {
        let report = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod origin { #[derive(recursa::Node)] pub struct X { this: Self } }
mod public_route { pub use super::origin::*; }
mod exports {
    use super::origin::*;
    pub use super::public_route::*;
}
mod consumer {
    use super::exports::X;
    #[derive(recursa::Node)] struct Root { value: X }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        let resolution = resolved
            .resolutions
            .iter()
            .find(|resolution| resolution.authored == "X")
            .unwrap();
        assert!(matches!(resolution.target, SemanticTarget::Node(_)));
    }

    #[test]
    fn diagnoses_each_repeated_explicit_import_once_at_the_import_site() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] pub struct Leaf { this: Self } }
mod consumer {
    use super::source::Leaf;
    use super::source::Leaf;
    #[derive(recursa::Node)] struct Root { value: Leaf }
}
"#,
        );

        assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.kind,
            ResolutionDiagnosticKind::DuplicateExplicitImport,
        );
        assert_eq!(diagnostic.path, PathBuf::from("grammar/root.rs"));
        assert!(diagnostic.range.is_some());
    }

    #[test]
    fn locates_import_collisions_inaccessible_reexports_and_external_globs() {
        let collision = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] pub struct Leaf { this: Self } }
mod consumer {
    use super::source::Leaf as Root;
    #[derive(recursa::Node)] struct Root { this: Self }
}
"#,
        );
        assert_eq!(
            collision.diagnostics[0].kind,
            ResolutionDiagnosticKind::ImportCollision,
        );

        let inaccessible = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] struct Hidden { this: Self } }
mod exports { pub use super::source::Hidden; }
"#,
        );
        assert_eq!(
            inaccessible.diagnostics[0].kind,
            ResolutionDiagnosticKind::InaccessibleReexport,
        );

        let external_glob = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
use dependency::*;
#[derive(recursa::Node)] struct Root { this: Self }
"#,
        );
        assert_eq!(
            external_glob.diagnostics[0].kind,
            ResolutionDiagnosticKind::ExternalPath,
        );

        let external_reexport = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod exports {
    use dependency::Thing;
    pub use self::Thing as PublicThing;
}
#[derive(recursa::Node)] struct Root { this: Self }
"#,
        );
        assert!(external_reexport.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == ResolutionDiagnosticKind::ExternalPath
                && diagnostic.message.contains("re-export")
        }));

        for report in [collision, inaccessible, external_glob, external_reexport] {
            assert_eq!(report.diagnostics[0].path, PathBuf::from("grammar/root.rs"));
            assert!(report.diagnostics[0].range.is_some());
        }
    }

    #[test]
    fn synthesizes_module_local_content_token_bindings_with_one_semantic_identity() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod first {
    #[derive(recursa::Node)]
    pub struct Definition {
        #[lex(pattern = r"[A-Za-z_]+", admits(KEYWORD))]
        pub value: Ident,
    }
    #[derive(recursa::Node)]
    struct AlsoDefinition {
        #[lex(pattern = r"[A-Za-z_]+", admits(KEYWORD))]
        value: Ident,
    }
}
mod second {
    #[derive(recursa::Node)]
    pub struct Definition {
        #[lex(pattern = r"[A-Za-z_]+", admits(KEYWORD))]
        pub value: Ident,
    }
}
mod consumer {
    use super::first::Ident as Name;
    #[derive(recursa::Node)]
    struct Root {
        #[tok(Ident)]
        value: Name,
    }
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.content_tokens.len(), 1);
        assert_eq!(resolved.content_tokens[0].ident, "Ident");
        assert_eq!(resolved.content_tokens[0].sites.len(), 3);
        assert_eq!(resolved.token_bindings.len(), 2);
        assert!(
            resolved
                .token_bindings
                .iter()
                .all(|binding| binding.target == resolved.content_tokens[0].id)
        );
        let imported = resolved
            .resolutions
            .iter()
            .find(|resolution| resolution.authored == "Name")
            .unwrap();
        assert_eq!(
            imported.target,
            SemanticTarget::ContentToken(resolved.content_tokens[0].id),
        );
        assert!(
            resolved.traces[imported.trace.index()]
                .steps
                .iter()
                .any(|step| matches!(step, super::ResolutionTraceStep::Import(_)))
        );
        assert!(
            resolved.traces[imported.trace.index()]
                .steps
                .iter()
                .any(|step| matches!(step, super::ResolutionTraceStep::TokenBinding(_)))
        );
    }

    #[test]
    fn fixed_token_names_do_not_collide_with_rust_type_bindings() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct SELECT { this: Self }
#[derive(recursa::Node)] struct Root {
    #[tok(SELECT)]
    value: SELECT,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let resolved = report.resolved.unwrap();
        let select = resolved
            .nodes
            .iter()
            .find(|node| node.ident == "SELECT")
            .unwrap();
        assert!(resolved.resolutions.iter().any(|resolution| {
            resolution.authored == "SELECT" && resolution.target == SemanticTarget::Node(select.id)
        }));
        assert!(resolved.content_tokens.is_empty());
    }

    #[test]
    fn locates_inaccessible_and_colliding_content_token_bindings() {
        let inaccessible = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod tokens {
    #[derive(recursa::Node)]
    struct Definition { #[lex(pattern = r".+")] value: Ident }
}
mod consumer {
    #[derive(recursa::Node)] struct Root { value: super::tokens::Ident }
}
"#,
        );
        assert_eq!(
            inaccessible.diagnostics[0].kind,
            ResolutionDiagnosticKind::InaccessibleTokenBinding,
        );

        let collision = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
#[derive(recursa::Node)] struct Ident { this: Self }
#[derive(recursa::Node)] struct Definition {
    #[lex(pattern = r".+")] value: Ident,
}
"#,
        );
        assert!(collision.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == ResolutionDiagnosticKind::TokenBindingCollision
                && diagnostic.range.is_some()
        }));

        let import_collision = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod source { #[derive(recursa::Node)] pub struct Other { this: Self } }
mod tokens {
    use super::source::Other as Ident;
    #[derive(recursa::Node)] struct Definition {
        #[lex(pattern = r".+")] value: Ident,
    }
}
"#,
        );
        assert!(import_collision.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == ResolutionDiagnosticKind::TokenBindingCollision
                && diagnostic.range.is_some()
        }));
    }

    #[test]
    fn import_resolution_and_traces_ignore_harmless_import_order_permutations() {
        fn source(imports: &str) -> String {
            format!(
                r#"
recursa::grammar! {{ module = crate::sql }}
mod origin {{ #[derive(recursa::Node)] pub struct Leaf {{ this: Self }} }}
mod middle {{ pub use super::origin::Leaf; }}
mod exports {{ pub use super::middle::*; }}
mod consumer {{
{imports}
    #[derive(recursa::Node)] struct Root {{ value: Heap<Leaf> }}
}}
"#,
            )
        }
        fn projection(
            resolved: &super::ResolvedSourceGrammar,
        ) -> Vec<(String, SemanticTarget, Vec<String>)> {
            resolved
                .resolutions
                .iter()
                .map(|resolution| {
                    let steps = resolved.traces[resolution.trace.index()]
                        .steps
                        .iter()
                        .map(|step| match step {
                            super::ResolutionTraceStep::Import(import) => {
                                format!("import:{}", resolved.imports[import.index()].authored)
                            }
                            other => format!("{other:?}"),
                        })
                        .collect();
                    (resolution.authored.clone(), resolution.target, steps)
                })
                .collect()
        }

        let first = resolve(
            &TempDir::new().unwrap(),
            &source("    use std::boxed::Box as Heap;\n    use super::exports::Leaf;"),
        );
        let second = resolve(
            &TempDir::new().unwrap(),
            &source("    use super::exports::Leaf;\n    use std::boxed::Box as Heap;"),
        );
        assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
        assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
        let first = first.resolved.unwrap();
        let second = second.resolved.unwrap();
        assert_eq!(projection(&first), projection(&second));

        let leaf = first
            .resolutions
            .iter()
            .find(|resolution| resolution.authored == "Leaf")
            .unwrap();
        assert_eq!(
            first.traces[leaf.trace.index()]
                .steps
                .iter()
                .filter_map(|step| match step {
                    super::ResolutionTraceStep::Import(import) => {
                        Some(first.imports[import.index()].authored.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [
                "super::exports::Leaf",
                "super::middle::*",
                "super::origin::Leaf",
            ],
        );
    }

    #[test]
    fn normalized_resolution_ignores_module_declaration_order() {
        fn module_path(
            resolved: &super::ResolvedSourceGrammar,
            module: crate::lowering::ModuleId,
        ) -> String {
            let path = &resolved.tree.inclusions[module.index()].logical_path;
            if path.is_empty() {
                "<root>".to_owned()
            } else {
                path.join("::")
            }
        }
        fn node_key(
            resolved: &super::ResolvedSourceGrammar,
            node: crate::lowering::NodeId,
        ) -> String {
            resolved.nodes[node.index()].key.segments.join("::")
        }
        fn projection(
            resolved: &super::ResolvedSourceGrammar,
        ) -> Vec<(String, String, String, Vec<String>)> {
            let mut projection = resolved
                .resolutions
                .iter()
                .map(|resolution| {
                    let use_site = &resolved.uses[resolution.use_site.index()];
                    let target = match resolution.target {
                        SemanticTarget::Node(node) => format!("node:{}", node_key(resolved, node)),
                        SemanticTarget::TypeAlias(alias) => {
                            format!("alias:{}", resolved.type_aliases[alias.index()].ident)
                        }
                        SemanticTarget::GenericParameter(parameter) => format!(
                            "generic:{}",
                            resolved.generic_parameters[parameter.index()].ident
                        ),
                        SemanticTarget::ContentToken(token) => {
                            format!("token:{}", resolved.content_tokens[token.index()].ident)
                        }
                        SemanticTarget::Catalog(catalog) => format!("catalog:{catalog:?}"),
                    };
                    let steps = resolved.traces[resolution.trace.index()]
                        .steps
                        .iter()
                        .map(|step| match step {
                            super::ResolutionTraceStep::Start(module) => {
                                format!("start:{}", module_path(resolved, *module))
                            }
                            super::ResolutionTraceStep::ContainingNode(node) => {
                                format!("self:{}", node_key(resolved, *node))
                            }
                            super::ResolutionTraceStep::CurrentModule(module) => {
                                format!("current:{}", module_path(resolved, *module))
                            }
                            super::ResolutionTraceStep::ParentModule(module) => {
                                format!("parent:{}", module_path(resolved, *module))
                            }
                            super::ResolutionTraceStep::Anchor(module) => {
                                format!("anchor:{}", module_path(resolved, *module))
                            }
                            super::ResolutionTraceStep::Module(module) => {
                                format!("module:{}", module_path(resolved, *module))
                            }
                            super::ResolutionTraceStep::Import(import) => {
                                let import = &resolved.imports[import.index()];
                                format!(
                                    "import:{}:{}",
                                    module_path(resolved, import.module),
                                    import.authored
                                )
                            }
                            super::ResolutionTraceStep::Declaration(node) => {
                                format!("declaration:{}", node_key(resolved, *node))
                            }
                            super::ResolutionTraceStep::TypeAlias(alias) => {
                                format!("alias:{}", resolved.type_aliases[alias.index()].ident)
                            }
                            super::ResolutionTraceStep::TokenBinding(token) => format!(
                                "token-binding:{}",
                                resolved.content_tokens[token.index()].ident
                            ),
                            super::ResolutionTraceStep::GenericParameter(parameter) => format!(
                                "generic:{}",
                                resolved.generic_parameters[parameter.index()].ident
                            ),
                            super::ResolutionTraceStep::Prelude(catalog) => {
                                format!("prelude:{catalog:?}")
                            }
                            super::ResolutionTraceStep::Catalog(catalog) => {
                                format!("catalog:{catalog:?}")
                            }
                        })
                        .collect();
                    (
                        node_key(
                            resolved,
                            match use_site.owner {
                                super::SemanticOwner::Node(node) => node,
                                super::SemanticOwner::TypeAlias(_) => {
                                    panic!("fixture projection contains only Node fields")
                                }
                            },
                        ),
                        resolution.authored.clone(),
                        target,
                        steps,
                    )
                })
                .collect::<Vec<_>>();
            projection.sort();
            projection
        }

        let first = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod a { #[derive(recursa::Node)] pub struct X { this: Self } }
mod b { #[derive(recursa::Node)] pub struct Y { this: Self } }
mod exports { pub use super::a::X; pub use super::b::Y; }
mod consumer {
    use super::exports::{X, Y};
    #[derive(recursa::Node)] struct Root { x: X, y: Y }
}
"#,
        );
        let second = resolve(
            &TempDir::new().unwrap(),
            r#"
recursa::grammar! { module = crate::sql }
mod consumer {
    use super::exports::{X, Y};
    #[derive(recursa::Node)] struct Root { x: X, y: Y }
}
mod exports { pub use super::b::Y; pub use super::a::X; }
mod b { #[derive(recursa::Node)] pub struct Y { this: Self } }
mod a { #[derive(recursa::Node)] pub struct X { this: Self } }
"#,
        );
        assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
        assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
        assert_eq!(
            projection(&first.resolved.unwrap()),
            projection(&second.resolved.unwrap()),
        );
    }

    #[test]
    fn collects_nested_failures_and_ignores_descendant_grammar_named_macros() {
        let package = TempDir::new().unwrap();
        let report = resolve(
            &package,
            r#"
recursa::grammar! { module = crate::sql }
mod child {
    recursa::grammar! { module = crate::not_the_root }
    foo::grammar! { arbitrary tokens }
}
#[derive(recursa::Node)]
struct Root { nested: Unknown<AlsoMissing> }
"#,
        );

        assert!(report.resolved.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [
                ResolutionDiagnosticKind::Unresolved,
                ResolutionDiagnosticKind::Unresolved,
            ],
        );
        assert_eq!(report.build_inputs.unwrap().module_anchor.segments, ["sql"],);
    }
}
